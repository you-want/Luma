use crate::{
    cleanup::{self, CleanupSummary},
    database,
    duplicates::{self, DuplicateGroup},
    error::AppError,
    file_manager::{self, DirNode, DirectoryListing},
    file_ops::{self, OpResult, OperationRecord},
    models::{
        FileEntry, InsightSummary, ScanComparison, ScanFinished, ScanStatus, ScanSummary,
        SearchRequest, SearchResponse, StartScanRequest,
    },
    organizer,
    projects::{self, ProjectCandidate},
    scanner, search, storage,
};
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::{SystemTime, UNIX_EPOCH},
};
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_opener::OpenerExt;
use uuid::Uuid;

pub struct AppState {
    pub database_path: PathBuf,
    pub cancellations: Mutex<HashMap<String, Arc<AtomicBool>>>,
}

// Synchronous SQLite, filesystem and OS calls must never run on the WebView's
// event thread. Keep the command wrappers asynchronous and use the blocking pool.
async fn background_task<T: Send + 'static>(
    task: impl FnOnce() -> Result<T, AppError> + Send + 'static,
) -> Result<T, AppError> {
    tauri::async_runtime::spawn_blocking(task)
        .await
        .map_err(|error| AppError::new("OP_FAILED", error.to_string()))?
}

async fn database_task<T: Send + 'static>(
    state: State<'_, AppState>,
    task: impl FnOnce(&std::path::Path) -> Result<T, AppError> + Send + 'static,
) -> Result<T, AppError> {
    let database_path = state.database_path.clone();
    background_task(move || task(&database_path)).await
}

static DATABASE_BUSY: AtomicBool = AtomicBool::new(false);

struct DatabaseActivity;
impl DatabaseActivity {
    fn acquire() -> Result<Self, AppError> {
        DATABASE_BUSY
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| AppError::new("OP_BUSY", "扫描、文件操作或索引维护仍在进行。"))?;
        Ok(Self)
    }
}
impl Drop for DatabaseActivity {
    fn drop(&mut self) {
        DATABASE_BUSY.store(false, Ordering::Release);
    }
}

fn file_mutation<T>(task: impl FnOnce() -> Result<T, AppError>) -> Result<T, AppError> {
    // Async commands can overlap across tabs or windows. Reject another write
    // instead of queueing a duplicate operation against a stale file snapshot.
    let _guard = DatabaseActivity::acquire()?;
    task()
}

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

/// Read the first 64 KiB of a text file and return it as a UTF-8 string.
/// Used by the frontend preview panel to show file contents without loading
/// the entire file into memory.
#[tauri::command]
pub async fn read_text_preview(path: String) -> Result<String, AppError> {
    background_task(move || {
        use std::io::Read;
        let native = PathBuf::from(path.replace('/', std::path::MAIN_SEPARATOR_STR));
        let file = std::fs::File::open(&native)
            .map_err(|e| AppError::new("FILE_READ_ERROR", e.to_string()))?;
        const LIMIT: usize = 64 * 1024;
        let mut buf = Vec::with_capacity(LIMIT);
        file.take(LIMIT as u64)
            .read_to_end(&mut buf)
            .map_err(|e| AppError::new("FILE_READ_ERROR", e.to_string()))?;
        // Replace invalid UTF-8 sequences with the replacement character so the
        // frontend always receives a valid string, even for mixed-encoding files.
        Ok(String::from_utf8_lossy(&buf).into_owned())
    })
    .await
}

#[tauri::command]
pub async fn read_binary_preview(
    state: State<'_, AppState>,
    scan_id: String,
    path: String,
) -> Result<file_manager::BinaryPreview, AppError> {
    let database_path = state.database_path.clone();
    tauri::async_runtime::spawn_blocking(move || {
        file_manager::read_binary_preview(&database_path, &scan_id, &path)
    })
    .await
    .map_err(|e| AppError::new("FILE_READ_ERROR", e.to_string()))?
}

/// Returns the direct subdirectories of `parent_path` for the given scan,
/// derived from the stored path index — no filesystem access needed.
#[tauri::command]
pub async fn get_directory_nodes(
    state: State<'_, AppState>,
    scan_id: String,
    parent_path: String,
) -> Result<Vec<DirNode>, AppError> {
    database_task(state, move |database_path| {
        file_manager::get_directory_nodes(database_path, &scan_id, &parent_path)
    })
    .await
}

/// Returns files sitting directly inside `dir_path` (non-recursive) with
/// pagination. Sort is a closed enum transmitted as a camelCase string.
#[tauri::command]
pub async fn list_directory_files(
    state: State<'_, AppState>,
    scan_id: String,
    dir_path: String,
    include_hidden: Option<bool>,
    sort: Option<String>,
    limit: Option<u32>,
    offset: Option<u32>,
) -> Result<DirectoryListing, AppError> {
    database_task(state, move |database_path| {
        // Map the frontend sort token to a safe ORDER BY fragment. Only these
        // known strings are ever interpolated — never raw client input.
        let order = match sort.as_deref().unwrap_or("nameAsc") {
            "sizeDesc" => "size_bytes DESC, id ASC",
            "sizeAsc" => "size_bytes ASC, id ASC",
            "nameDesc" => "name COLLATE NOCASE DESC, id ASC",
            "modifiedDesc" => "modified_at DESC, id ASC",
            "modifiedAsc" => "modified_at ASC, id ASC",
            _ => "name COLLATE NOCASE ASC, id ASC",
        };
        let lim = limit.unwrap_or(100).clamp(1, 500);
        let off = offset.unwrap_or(0);
        let hidden = include_hidden.unwrap_or(false);

        let (files, total_files) = file_manager::list_directory_files(
            database_path,
            &scan_id,
            &dir_path,
            hidden,
            order,
            lim,
            off,
        )?;

        let dirs = file_manager::get_directory_nodes(database_path, &scan_id, &dir_path)?;

        Ok(DirectoryListing {
            dirs,
            files,
            total_files,
        })
    })
    .await
}

/// Open a file or directory with the default OS application.
/// Uses the opener plugin (already available for `reveal_path`).
#[tauri::command]
pub async fn open_path(app: AppHandle, path: String) -> Result<(), AppError> {
    background_task(move || {
        let native = PathBuf::from(path.replace('/', std::path::MAIN_SEPARATOR_STR));
        app.opener()
            .open_path(native.to_string_lossy().as_ref(), None::<&str>)
            .map_err(|e| AppError::new("OPEN_FAILED", e.to_string()))
    })
    .await
}

// ── File operations ────────────────────────────────────────────────────────────

/// Move files to the OS trash. Never permanently deletes.
#[tauri::command]
pub async fn trash_files(
    state: State<'_, AppState>,
    scan_id: String,
    paths: Vec<String>,
) -> Result<OpResult, AppError> {
    database_task(state, move |database_path| {
        file_mutation(|| file_ops::trash_files(database_path, &scan_id, &paths))
    })
    .await
}

/// Rename a file or directory in-place (same parent directory).
/// Returns the new path and persistent operation id on success.
#[tauri::command]
pub async fn rename_file(
    state: State<'_, AppState>,
    scan_id: String,
    old_path: String,
    new_name: String,
) -> Result<RenameResult, AppError> {
    database_task(state, move |database_path| {
        file_mutation(|| {
            let outcome = file_ops::rename_file(database_path, &scan_id, &old_path, &new_name)?;
            Ok(RenameResult {
                new_path: outcome.new_path,
                operation_id: outcome.operation_id,
            })
        })
    })
    .await
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RenameResult {
    pub new_path: String,
    pub operation_id: String,
}

/// Move files into a destination directory.
#[tauri::command]
pub async fn move_files(
    state: State<'_, AppState>,
    scan_id: String,
    paths: Vec<String>,
    dest_dir: String,
) -> Result<OpResult, AppError> {
    database_task(state, move |database_path| {
        file_mutation(|| file_ops::move_files(database_path, &scan_id, &paths, &dest_dir))
    })
    .await
}

/// Copy files into a destination directory.
#[tauri::command]
pub async fn copy_files(
    state: State<'_, AppState>,
    scan_id: String,
    paths: Vec<String>,
    dest_dir: String,
) -> Result<OpResult, AppError> {
    database_task(state, move |database_path| {
        file_mutation(|| file_ops::copy_files(database_path, &scan_id, &paths, &dest_dir))
    })
    .await
}

/// Return recent write operations that still have a persisted undo plan.
#[tauri::command]
pub async fn list_undoable_operations(
    state: State<'_, AppState>,
    scan_id: String,
    limit: Option<u32>,
) -> Result<Vec<OperationRecord>, AppError> {
    database_task(state, move |database_path| {
        file_ops::list_undoable_operations(database_path, &scan_id, limit.unwrap_or(20))
    })
    .await
}

/// Undo one persisted rename, move, or organize operation.
#[tauri::command]
pub async fn undo_file_operation(
    state: State<'_, AppState>,
    operation_id: String,
    scan_id: String,
) -> Result<OpResult, AppError> {
    database_task(state, move |database_path| {
        file_mutation(|| file_ops::undo_operation(database_path, &operation_id, &scan_id))
    })
    .await
}

// ── Smart organizer ────────────────────────────────────────────────────────────

/// Dry-run: compute the full move plan without touching the filesystem.
#[tauri::command]
pub async fn plan_organize(
    state: State<'_, AppState>,
    scan_id: String,
    source_dir: String,
    dest_dir: String,
    rule: organizer::OrganizeRule,
    options: Option<organizer::OrganizeOptions>,
) -> Result<organizer::OrganizePlan, AppError> {
    let database_path = state.database_path.clone();
    tauri::async_runtime::spawn_blocking(move || {
        organizer::plan_organize(
            &database_path,
            &scan_id,
            &source_dir,
            &dest_dir,
            &rule,
            &options.unwrap_or_default(),
        )
    })
    .await
    .map_err(|e| AppError::new("OP_FAILED", e.to_string()))?
}

/// Execute an approved organize plan, emitting `organize-progress` events
/// so the frontend can show a live progress bar.
#[tauri::command]
pub async fn execute_organize_plan(
    app: AppHandle,
    state: State<'_, AppState>,
    scan_id: String,
    moves: Vec<organizer::OrganizeMoveInput>,
) -> Result<organizer::OrganizeResult, AppError> {
    let database_path = state.database_path.clone();
    let task_scan_id = scan_id.clone();
    let task_moves = moves.clone();

    tauri::async_runtime::spawn_blocking(move || {
        file_mutation(|| {
            organizer::execute_organize_plan(
                &database_path,
                &task_scan_id,
                &task_moves,
                |progress| {
                    let _ = app.emit("organize-progress", &progress);
                },
            )
        })
    })
    .await
    .map_err(|e| AppError::new("OP_FAILED", e.to_string()))?
}

fn now_seconds() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|duration| i64::try_from(duration.as_secs()).ok())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocking_io_runs_on_a_worker_and_preserves_results_and_errors() {
        let caller = std::thread::current().id();
        let worker =
            tauri::async_runtime::block_on(background_task(|| Ok(std::thread::current().id())))
                .unwrap();
        assert_ne!(caller, worker);

        let error = tauri::async_runtime::block_on(background_task(|| {
            Err::<(), _>(AppError::new("TEST_IO", "original error"))
        }))
        .unwrap_err();
        assert_eq!(error.code, "TEST_IO");
        assert_eq!(error.message, "original error");
    }

    #[test]
    fn overlapping_file_writes_are_rejected_without_executing_and_unlock_after_failure() {
        let guard = DatabaseActivity::acquire().unwrap();
        let mut executed = false;
        let result = file_mutation(|| {
            executed = true;
            Ok(())
        });
        assert_eq!(result.unwrap_err().code, "OP_BUSY");
        assert!(!executed);
        drop(guard);
        assert!(file_mutation(|| Err::<(), _>(AppError::new("TEST_IO", "failure"))).is_err());
        assert_eq!(file_mutation(|| Ok(42)).unwrap(), 42);
    }
}

#[tauri::command]
pub async fn get_index_storage(
    state: State<'_, AppState>,
) -> Result<storage::StorageUsage, AppError> {
    database_task(state, storage::usage).await
}

#[tauri::command]
pub async fn compact_index_storage(
    state: State<'_, AppState>,
) -> Result<storage::StorageUsage, AppError> {
    database_task(state, |path| {
        file_mutation(|| {
            let mut connection = database::open(path)?;
            storage::retain(&mut connection)?;
            storage::compact(&connection, true)?;
            drop(connection);
            storage::usage(path)
        })
    })
    .await
}

#[tauri::command]
pub async fn clear_scan_index(
    state: State<'_, AppState>,
) -> Result<storage::StorageUsage, AppError> {
    database_task(state, |path| file_mutation(|| storage::clear(path))).await
}
