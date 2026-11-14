use super::{background_task, database_task, file_mutation, AppState};
use crate::cleaner::{CleanOutcome, CleanRequestItem};
use crate::{
    error::AppError,
    sessions::{self, AgentSessionsReport},
};
use tauri::State;

// ── Agent sessions (Agent 会话) ────────────────────────────────────────────────

/// Discover local AI-agent CLI sessions (Claude Code / Codex / Gemini CLI).
/// Read-only: walks each agent's transcript directory with head/tail windowed
/// reads, cached by file mtime + size between calls.
#[tauri::command]
pub async fn discover_agent_sessions() -> Result<AgentSessionsReport, AppError> {
    background_task(sessions::discover_sessions).await
}

/// Trash confirmed sessions (transcript + per-agent sidecar data). Re-checks
/// every request against a fresh discovery and never touches unknown paths.
#[tauri::command]
pub async fn trash_agent_sessions(
    state: State<'_, AppState>,
    items: Vec<CleanRequestItem>,
) -> Result<CleanOutcome, AppError> {
    database_task(state, move |database_path| {
        file_mutation(|| sessions::trash_sessions(database_path, &items))
    })
    .await
}
