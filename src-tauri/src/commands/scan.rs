use super::{background_task, database_task, now_seconds, AppState, DatabaseActivity};
use crate::{
    cleanup::{self, CleanupSummary},
    database,
    duplicates::{self, DuplicateGroup},
    error::AppError,
    models::{
        FileEntry, InsightSummary, ScanComparison, ScanFinished, ScanStatus, ScanSummary,
        SearchRequest, SearchResponse, StartScanRequest,
    },
    projects::{self, ProjectCandidate},
    scanner, search, storage,
};
use std::path::PathBuf;
use std::sync::{atomic::AtomicBool, atomic::Ordering, Arc};
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_opener::OpenerExt;
use uuid::Uuid;

#[tauri::command]
pub async fn start_scan(
    app: AppHandle,
    state: State<'_, AppState>,
    request: StartScanRequest,
) -> Result<String, AppError> {
    let activity = DatabaseActivity::acquire()?;
    let scan_id = Uuid::new_v4().to_string();
    let cancel = Arc::new(AtomicBool::new(false));
    let database_path = state.database_path.clone();
    let setup_database_path = database_path.clone();
    let setup_scan_id = scan_id.clone();
    let root_path = request.root_path.clone();
    background_task(move || {
        scanner::validate_root(&root_path)?;
        database::create_scan_run(
            &setup_database_path,
            &setup_scan_id,
            &root_path,
            now_seconds(),
        )
    })
    .await?;

    state
        .cancellations
        .lock()
        .map_err(|_| AppError::new("SCAN_FAILED", "扫描任务状态不可用。"))?
        .insert(scan_id.clone(), cancel.clone());

    let task_scan_id = scan_id.clone();
    tauri::async_runtime::spawn(async move {
        let _activity = activity;
        let progress_app = app.clone();
        let worker_database_path = database_path.clone();
        let worker_scan_id = task_scan_id.clone();
        let result = tauri::async_runtime::spawn_blocking(move || {
            let mut connection = database::open(&worker_database_path)?;
            let used_bytes_limit =
                storage::used_bytes(&connection)?.saturating_add(storage::SCAN_BUDGET_BYTES);
            let outcome = scanner::scan_directory(
                &request,
                &worker_scan_id,
                cancel,
                |files| {
                    database::insert_file_batch_with_limit(
                        &mut connection,
                        &worker_scan_id,
                        &files,
                        used_bytes_limit,
                    )
                },
                |progress| {
                    let _ = progress_app.emit("scan-progress", progress);
                },
            )?;

            database::finish_scan(
                &connection,
                &worker_scan_id,
                outcome.status,
                &outcome.stats,
                now_seconds(),
            )?;

            let summary = if outcome.status == ScanStatus::Completed {
                database::scan_summary(&worker_database_path, &worker_scan_id)?
            } else {
                None
            };

            Ok::<_, AppError>(ScanFinished {
                scan_id: worker_scan_id,
                status: outcome.status,
                summary,
                error: None,
            })
        })
        .await;

        let finished = match result {
            Ok(Ok(finished)) => finished,
            Ok(Err(error)) => {
                let _ = database::mark_scan_failed(&database_path, &task_scan_id, now_seconds());
                ScanFinished {
                    scan_id: task_scan_id.clone(),
                    status: ScanStatus::Failed,
                    summary: None,
                    error: Some(error),
                }
            }
            Err(error) => {
                let app_error = AppError::new("SCAN_FAILED", error.to_string());
                let _ = database::mark_scan_failed(&database_path, &task_scan_id, now_seconds());
                ScanFinished {
                    scan_id: task_scan_id.clone(),
                    status: ScanStatus::Failed,
                    summary: None,
                    error: Some(app_error),
                }
            }
        };

        // Purge partial failed/cancelled listings too; preserve the previous
        // successful index and independently persisted undo records.
        let maintenance_path = database_path.clone();
        if let Err(error) = background_task(move || storage::maintain(&maintenance_path)).await {
            eprintln!("Luma index retention: {error}");
        }
        if let Some(state) = app.try_state::<AppState>() {
            if let Ok(mut cancellations) = state.cancellations.lock() {
                cancellations.remove(&task_scan_id);
            }
        }
        drop(_activity);
        let _ = app.emit("scan-finished", finished);
    });

    Ok(scan_id)
}

#[tauri::command]
pub fn cancel_scan(state: State<'_, AppState>, scan_id: String) -> Result<(), AppError> {
    let cancellations = state
        .cancellations
        .lock()
        .map_err(|_| AppError::new("SCAN_FAILED", "扫描任务状态不可用。"))?;
    let flag = cancellations
        .get(&scan_id)
        .ok_or_else(|| AppError::new("SCAN_NOT_FOUND", "扫描任务已结束或不存在。"))?;
    flag.store(true, Ordering::Relaxed);
    Ok(())
}

#[tauri::command]
pub async fn get_latest_scan(state: State<'_, AppState>) -> Result<Option<ScanSummary>, AppError> {
    database_task(state, move |database_path| {
        if let Ok(_activity) = DatabaseActivity::acquire() {
            storage::maintain(database_path)?;
        }
        database::latest_scan(database_path)
    })
    .await
}

#[tauri::command]
pub async fn get_scan_summary(
    state: State<'_, AppState>,
    scan_id: String,
) -> Result<ScanSummary, AppError> {
    let still_active = state
        .cancellations
        .lock()
        .map_err(|_| AppError::new("OP_BUSY", "扫描状态不可用。"))?
        .contains_key(&scan_id);
    database_task(state, move |database_path| {
        let mut summary = database::scan_summary(database_path, &scan_id)?
            .ok_or_else(|| AppError::new("SCAN_NOT_FOUND", "找不到扫描记录。"))?;
        // Do not unlock the UI while retention/compaction is still running.
        if still_active {
            summary.status = ScanStatus::Running;
        }
        Ok(summary)
    })
    .await
}

#[tauri::command]
pub async fn list_large_files(
    state: State<'_, AppState>,
    scan_id: String,
    limit: Option<u32>,
    offset: Option<u32>,
) -> Result<Vec<FileEntry>, AppError> {
    database_task(state, move |database_path| {
        database::list_large_files(
            database_path,
            &scan_id,
            limit.unwrap_or(20).clamp(1, 100),
            offset.unwrap_or(0),
        )
    })
    .await
}

#[tauri::command]
pub async fn list_insights(
    state: State<'_, AppState>,
    scan_id: String,
    large_file_threshold: Option<u64>,
    stale_days: Option<u32>,
) -> Result<Vec<InsightSummary>, AppError> {
    database_task(state, move |database_path| {
        let threshold = large_file_threshold.unwrap_or(1024_u64.pow(3));
        let days = stale_days.unwrap_or(180).clamp(1, 3650);
        let stale_before = now_seconds().saturating_sub(i64::from(days) * 24 * 60 * 60);
        database::list_insights(database_path, &scan_id, threshold, stale_before)
    })
    .await
}

#[tauri::command]
pub async fn list_projects(
    state: State<'_, AppState>,
    scan_id: String,
) -> Result<Vec<ProjectCandidate>, AppError> {
    database_task(state, move |database_path| {
        projects::identify_projects(database_path, &scan_id)
    })
    .await
}

#[tauri::command]
pub async fn list_insight_files(
    state: State<'_, AppState>,
    scan_id: String,
    kind: String,
    large_file_threshold: Option<u64>,
    stale_days: Option<u32>,
    limit: Option<u32>,
) -> Result<Vec<FileEntry>, AppError> {
    database_task(state, move |database_path| {
        let threshold = large_file_threshold.unwrap_or(1024_u64.pow(3));
        let days = stale_days.unwrap_or(180).clamp(1, 3650);
        let stale_before = now_seconds().saturating_sub(i64::from(days) * 24 * 60 * 60);
        database::list_insight_files(
            database_path,
            &scan_id,
            &kind,
            threshold,
            stale_before,
            limit.unwrap_or(10).clamp(1, 100),
        )
    })
    .await
}

#[tauri::command]
pub async fn find_duplicates(
    state: State<'_, AppState>,
    scan_id: String,
    min_size: Option<u64>,
) -> Result<Vec<DuplicateGroup>, AppError> {
    database_task(state, move |database_path| {
        let size_threshold = min_size.unwrap_or(1024 * 1024); // 默认 1MB
        duplicates::find_duplicate_candidates(database_path, &scan_id, size_threshold)
    })
    .await
}

#[tauri::command]
pub async fn list_scan_history(
    state: State<'_, AppState>,
    scan_id: String,
) -> Result<Vec<ScanSummary>, AppError> {
    database_task(state, move |database_path| {
        database::list_scan_history(database_path, &scan_id)
    })
    .await
}

#[tauri::command]
pub async fn compare_scans(
    state: State<'_, AppState>,
    base_scan_id: String,
    target_scan_id: String,
) -> Result<ScanComparison, AppError> {
    database_task(state, move |database_path| {
        database::compare_scans(database_path, &base_scan_id, &target_scan_id)
    })
    .await
}

/// Search one scan snapshot's indexed rows. Pagination and total count are
/// computed by SQLite so the frontend never loads the full file table.
#[tauri::command]
pub async fn search_files(
    state: State<'_, AppState>,
    request: SearchRequest,
) -> Result<SearchResponse, AppError> {
    database_task(state, move |database_path| {
        search::search_files(database_path, &request)
    })
    .await
}

/// Reveal a file in the OS file manager (Finder / Explorer). Paths are stored
/// in canonical `/`-separated form (see `scanner::normalize_stored_path`); a
/// `PathBuf` treats `/` and the native separator equivalently, so converting
/// back to a native path here lets Explorer's "select item" work on Windows.
#[tauri::command]
pub async fn reveal_path(app: AppHandle, path: String) -> Result<(), AppError> {
    background_task(move || {
        // Rewrite the canonical `/` back to the platform separator: no-op on Unix
        // (MAIN_SEPARATOR is `/`), `\` on Windows, which Explorer's select needs.
        let native = PathBuf::from(path.replace('/', std::path::MAIN_SEPARATOR_STR));
        app.opener()
            .reveal_item_in_dir(&native)
            .map_err(|error| AppError::new("REVEAL_FAILED", error.to_string()))
    })
    .await
}

#[tauri::command]
pub async fn get_cleanup_summary(
    state: State<'_, AppState>,
    scan_id: String,
    old_downloads_days: Option<u32>,
) -> Result<CleanupSummary, AppError> {
    database_task(state, move |database_path| {
        cleanup::build_cleanup_summary(database_path, &scan_id, old_downloads_days.unwrap_or(180))
    })
    .await
}

#[tauri::command]
pub async fn list_cleanup_files(
    state: State<'_, AppState>,
    scan_id: String,
    kind: String,
    limit: Option<u32>,
    old_downloads_days: Option<u32>,
) -> Result<Vec<FileEntry>, AppError> {
    database_task(state, move |database_path| {
        cleanup::list_cleanup_files(
            database_path,
            &scan_id,
            &kind,
            limit.unwrap_or(20).clamp(1, 100),
            old_downloads_days.unwrap_or(180),
        )
    })
    .await
}
