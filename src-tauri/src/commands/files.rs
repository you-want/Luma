use super::{database_task, file_mutation, AppState};
use crate::{
    error::AppError,
    file_ops::{self, OpResult, OperationLogEntry, OperationRecord},
};
use tauri::State;

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

/// Return the unified operation log (all scans, including trash/copy and the
/// cleaner/sessions family) for the 操作记录 view. Read-only.
#[tauri::command]
pub async fn list_recent_operations(
    state: State<'_, AppState>,
    limit: Option<u32>,
) -> Result<Vec<OperationLogEntry>, AppError> {
    database_task(state, move |database_path| {
        file_ops::list_recent_operations(database_path, limit.unwrap_or(100))
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
