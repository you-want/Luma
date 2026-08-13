use crate::{
    cleanup::{self, CleanupSummary},
    database,
    duplicates::{self, DuplicateGroup},
    error::AppError,
    file_manager::{self, DirNode, DirectoryListing},
    file_ops::{self, OpResult, UndoRecord},
    organizer,
    models::{
        FileEntry, InsightSummary, ScanComparison, ScanFinished, ScanStatus, ScanSummary,
        SearchRequest, SearchResponse, StartScanRequest,
    },
    projects::{self, ProjectCandidate},
    scanner,
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

#[tauri::command]
pub async fn start_scan(
    app: AppHandle,
    state: State<'_, AppState>,
    request: StartScanRequest,
) -> Result<String, AppError> {
    scanner::validate_root(&request.root_path)?;
    let scan_id = Uuid::new_v4().to_string();
    let cancel = Arc::new(AtomicBool::new(false));
    let database_path = state.database_path.clone();
    database::create_scan_run(&database_path, &scan_id, &request.root_path, now_seconds())?;

    state
        .cancellations
        .lock()
        .map_err(|_| AppError::new("SCAN_FAILED", "扫描任务状态不可用。"))?
        .insert(scan_id.clone(), cancel.clone());

    let task_scan_id = scan_id.clone();
    tauri::async_runtime::spawn(async move {
        let progress_app = app.clone();
        let worker_database_path = database_path.clone();
        let worker_scan_id = task_scan_id.clone();
        let result = tauri::async_runtime::spawn_blocking(move || {
            let mut connection = database::open(&worker_database_path)?;
            let outcome = scanner::scan_directory(
                &request,
                &worker_scan_id,
                cancel,
                |files| database::insert_file_batch(&mut connection, &worker_scan_id, &files),
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
            database::prune_old_scans(&connection)?;

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

        let _ = app.emit("scan-finished", finished);
        if let Some(state) = app.try_state::<AppState>() {
            if let Ok(mut cancellations) = state.cancellations.lock() {
                cancellations.remove(&task_scan_id);
            }
        }
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
pub fn get_latest_scan(state: State<'_, AppState>) -> Result<Option<ScanSummary>, AppError> {
    database::latest_scan(&state.database_path)
}

#[tauri::command]
pub fn get_scan_summary(
    state: State<'_, AppState>,
    scan_id: String,
) -> Result<ScanSummary, AppError> {
    database::scan_summary(&state.database_path, &scan_id)?
        .ok_or_else(|| AppError::new("SCAN_NOT_FOUND", "找不到对应的扫描记录。"))
}

#[tauri::command]
pub fn list_large_files(
    state: State<'_, AppState>,
    scan_id: String,
    limit: Option<u32>,
    offset: Option<u32>,
) -> Result<Vec<FileEntry>, AppError> {
    database::list_large_files(
        &state.database_path,
        &scan_id,
        limit.unwrap_or(20).clamp(1, 100),
        offset.unwrap_or(0),
    )
}

#[tauri::command]
pub fn list_insights(
    state: State<'_, AppState>,
    scan_id: String,
    large_file_threshold: Option<u64>,
    stale_days: Option<u32>,
) -> Result<Vec<InsightSummary>, AppError> {
    let threshold = large_file_threshold.unwrap_or(1024_u64.pow(3));
    let days = stale_days.unwrap_or(180).clamp(1, 3650);
    let stale_before = now_seconds().saturating_sub(i64::from(days) * 24 * 60 * 60);
    database::list_insights(&state.database_path, &scan_id, threshold, stale_before)
}

#[tauri::command]
pub fn list_projects(
    state: State<'_, AppState>,
    scan_id: String,
) -> Result<Vec<ProjectCandidate>, AppError> {
    projects::identify_projects(&state.database_path, &scan_id)
}

#[tauri::command]
pub fn list_insight_files(
    state: State<'_, AppState>,
    scan_id: String,
    kind: String,
    large_file_threshold: Option<u64>,
    stale_days: Option<u32>,
    limit: Option<u32>,
) -> Result<Vec<FileEntry>, AppError> {
    let threshold = large_file_threshold.unwrap_or(1024_u64.pow(3));
    let days = stale_days.unwrap_or(180).clamp(1, 3650);
    let stale_before = now_seconds().saturating_sub(i64::from(days) * 24 * 60 * 60);
    database::list_insight_files(
        &state.database_path,
        &scan_id,
        &kind,
        threshold,
        stale_before,
        limit.unwrap_or(10).clamp(1, 100),
    )
}

#[tauri::command]
pub fn find_duplicates(
    state: State<'_, AppState>,
    scan_id: String,
    min_size: Option<u64>,
) -> Result<Vec<DuplicateGroup>, AppError> {
    let size_threshold = min_size.unwrap_or(1024 * 1024); // 默认 1MB
    duplicates::find_duplicate_candidates(&state.database_path, &scan_id, size_threshold)
}

#[tauri::command]
pub fn list_scan_history(
    state: State<'_, AppState>,
    scan_id: String,
) -> Result<Vec<ScanSummary>, AppError> {
    database::list_scan_history(&state.database_path, &scan_id)
}

#[tauri::command]
pub fn compare_scans(
    state: State<'_, AppState>,
    base_scan_id: String,
    target_scan_id: String,
) -> Result<ScanComparison, AppError> {
    database::compare_scans(&state.database_path, &base_scan_id, &target_scan_id)
}

/// Search one scan snapshot's indexed rows. Pagination and total count are
/// computed by SQLite so the frontend never loads the full file table.
#[tauri::command]
pub fn search_files(
    state: State<'_, AppState>,
    request: SearchRequest,
) -> Result<SearchResponse, AppError> {
    database::search_files(&state.database_path, &request)
}

/// Reveal a file in the OS file manager (Finder / Explorer). Paths are stored
/// in canonical `/`-separated form (see `scanner::normalize_stored_path`); a
/// `PathBuf` treats `/` and the native separator equivalently, so converting
/// back to a native path here lets Explorer's "select item" work on Windows.
#[tauri::command]
pub fn reveal_path(app: AppHandle, path: String) -> Result<(), AppError> {
    // Rewrite the canonical `/` back to the platform separator: no-op on Unix
    // (MAIN_SEPARATOR is `/`), `\` on Windows, which Explorer's select needs.
    let native = PathBuf::from(path.replace('/', std::path::MAIN_SEPARATOR_STR));
    app.opener()
        .reveal_item_in_dir(&native)
        .map_err(|error| AppError::new("REVEAL_FAILED", error.to_string()))
}

#[tauri::command]
pub fn get_cleanup_summary(
    state: State<'_, AppState>,
    scan_id: String,
    old_downloads_days: Option<u32>,
) -> Result<CleanupSummary, AppError> {
    cleanup::build_cleanup_summary(
        &state.database_path,
        &scan_id,
        old_downloads_days.unwrap_or(180),
    )
}

#[tauri::command]
pub fn list_cleanup_files(
    state: State<'_, AppState>,
    scan_id: String,
    kind: String,
    limit: Option<u32>,
    old_downloads_days: Option<u32>,
) -> Result<Vec<FileEntry>, AppError> {
    cleanup::list_cleanup_files(
        &state.database_path,
        &scan_id,
        &kind,
        limit.unwrap_or(20).clamp(1, 100),
        old_downloads_days.unwrap_or(180),
    )
}

/// Read the first 64 KiB of a text file and return it as a UTF-8 string.
/// Used by the frontend preview panel to show file contents without loading
/// the entire file into memory.
#[tauri::command]
pub fn read_text_preview(path: String) -> Result<String, AppError> {
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
}

/// Returns the direct subdirectories of `parent_path` for the given scan,
/// derived from the stored path index — no filesystem access needed.
#[tauri::command]
pub fn get_directory_nodes(
    state: State<'_, AppState>,
    scan_id: String,
    parent_path: String,
) -> Result<Vec<DirNode>, AppError> {
    file_manager::get_directory_nodes(&state.database_path, &scan_id, &parent_path)
}

/// Returns files sitting directly inside `dir_path` (non-recursive) with
/// pagination. Sort is a closed enum transmitted as a camelCase string.
#[tauri::command]
pub fn list_directory_files(
    state: State<'_, AppState>,
    scan_id: String,
    dir_path: String,
    include_hidden: Option<bool>,
    sort: Option<String>,
    limit: Option<u32>,
    offset: Option<u32>,
) -> Result<DirectoryListing, AppError> {
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
        &state.database_path,
        &scan_id,
        &dir_path,
        hidden,
        order,
        lim,
        off,
    )?;

    let dirs =
        file_manager::get_directory_nodes(&state.database_path, &scan_id, &dir_path)?;

    Ok(DirectoryListing { dirs, files, total_files })
}

/// Open a file or directory with the default OS application.
/// Uses the opener plugin (already available for `reveal_path`).
#[tauri::command]
pub fn open_path(app: AppHandle, path: String) -> Result<(), AppError> {
    let native = PathBuf::from(path.replace('/', std::path::MAIN_SEPARATOR_STR));
    app.opener()
        .open_path(native.to_string_lossy().as_ref(), None::<&str>)
        .map_err(|e| AppError::new("OPEN_FAILED", e.to_string()))
}

// ── File operations ────────────────────────────────────────────────────────────

/// Move files to the OS trash. Never permanently deletes.
#[tauri::command]
pub fn trash_files(
    state: State<'_, AppState>,
    scan_id: String,
    paths: Vec<String>,
) -> Result<OpResult, AppError> {
    file_ops::trash_files(&state.database_path, &scan_id, &paths)
}

/// Rename a file or directory in-place (same parent directory).
/// Returns `{ newPath, undoRecord }` on success.
#[tauri::command]
pub fn rename_file(
    state: State<'_, AppState>,
    scan_id: String,
    old_path: String,
    new_name: String,
) -> Result<RenameResult, AppError> {
    let new_path = file_ops::rename_file(&state.database_path, &scan_id, &old_path, &new_name)?;
    Ok(RenameResult {
        new_path: new_path.clone(),
        undo: UndoRecord {
            kind: file_ops::UndoKind::Rename,
            from: vec![old_path],
            to: vec![new_path],
        },
    })
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RenameResult {
    pub new_path: String,
    pub undo: UndoRecord,
}

/// Move files into a destination directory.
#[tauri::command]
pub fn move_files(
    state: State<'_, AppState>,
    scan_id: String,
    paths: Vec<String>,
    dest_dir: String,
) -> Result<OpResult, AppError> {
    file_ops::move_files(&state.database_path, &scan_id, &paths, &dest_dir)
}

/// Copy files into a destination directory.
#[tauri::command]
pub fn copy_files(
    state: State<'_, AppState>,
    scan_id: String,
    paths: Vec<String>,
    dest_dir: String,
) -> Result<OpResult, AppError> {
    file_ops::copy_files(&state.database_path, &scan_id, &paths, &dest_dir)
}

// ── Smart organizer ────────────────────────────────────────────────────────────

/// Dry-run: compute the full move plan without touching the filesystem.
#[tauri::command]
pub fn plan_organize(
    state: State<'_, AppState>,
    scan_id: String,
    source_dir: String,
    dest_dir: String,
    rule: organizer::OrganizeRule,
) -> Result<organizer::OrganizePlan, AppError> {
    organizer::plan_organize(&state.database_path, &scan_id, &source_dir, &dest_dir, &rule)
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
        organizer::execute_organize_plan(
            &database_path,
            &task_scan_id,
            &task_moves,
            |progress| {
                let _ = app.emit("organize-progress", &progress);
            },
        )
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
