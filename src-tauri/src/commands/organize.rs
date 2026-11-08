use super::{file_mutation, AppState};
use crate::{error::AppError, organizer};
use tauri::{AppHandle, Emitter, State};

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
