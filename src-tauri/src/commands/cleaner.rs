use super::{background_task, database_task, file_mutation, AppState};
use crate::{
    cleaner::{
        self, CleanOutcome, CleanRequestItem, CleanerFile, CleanerOperation, CleanerOverview,
        CleanerWhitelistEntry, DEFAULT_OLD_DOWNLOADS_DAYS,
    },
    error::AppError,
    maintenance::{self, MaintenanceOutcome, MaintenanceReport},
    orphans::{self, AppDataReport},
    purge::{self, PurgeReport},
    settings,
    slim::{self, SlimOutcome, SlimReport, SlimRequestItem},
    system_info::{self, SystemStatus},
    tray_monitor,
};
use tauri::{AppHandle, State};

// ── Disk cleaner (磁盘管家) ────────────────────────────────────────────────────

/// Overview of reclaimable space at registered fixed locations. Independent
/// of any scan; fails closed when the whitelist cannot be read.
#[tauri::command]
pub async fn get_cleaner_overview(state: State<'_, AppState>) -> Result<CleanerOverview, AppError> {
    database_task(state, |database_path| {
        cleaner::build_cleaner_overview(database_path, DEFAULT_OLD_DOWNLOADS_DAYS)
    })
    .await
}

/// List the deletion units of one cleaner kind, largest first.
#[tauri::command]
pub async fn list_cleaner_files(
    state: State<'_, AppState>,
    kind: String,
    limit: Option<u32>,
) -> Result<Vec<CleanerFile>, AppError> {
    database_task(state, move |database_path| {
        cleaner::list_cleaner_files(database_path, &kind, limit.unwrap_or(20).clamp(1, 100))
    })
    .await
}

/// Move user-confirmed units of one kind to the OS trash. The backend
/// re-verifies every path (scope, size, whitelist) at the deletion boundary.
#[tauri::command]
pub async fn execute_cleaner(
    state: State<'_, AppState>,
    kind: String,
    items: Vec<CleanRequestItem>,
) -> Result<CleanOutcome, AppError> {
    database_task(state, move |database_path| {
        file_mutation(|| cleaner::execute_clean(database_path, &kind, &items))
    })
    .await
}

#[tauri::command]
pub async fn get_cleaner_whitelist(
    state: State<'_, AppState>,
) -> Result<Vec<CleanerWhitelistEntry>, AppError> {
    database_task(state, |database_path| {
        cleaner::list_whitelist_entries(database_path)
    })
    .await
}

#[tauri::command]
pub async fn add_cleaner_whitelist(
    state: State<'_, AppState>,
    path: String,
) -> Result<Vec<CleanerWhitelistEntry>, AppError> {
    database_task(state, move |database_path| {
        file_mutation(|| cleaner::add_whitelist_entry(database_path, &path))
    })
    .await
}

#[tauri::command]
pub async fn remove_cleaner_whitelist(
    state: State<'_, AppState>,
    path: String,
) -> Result<Vec<CleanerWhitelistEntry>, AppError> {
    database_task(state, move |database_path| {
        file_mutation(|| cleaner::remove_whitelist_entry(database_path, &path))
    })
    .await
}

/// Recent cleaner-family runs, newest first (read-only history view).
#[tauri::command]
pub async fn list_cleaner_operations(
    state: State<'_, AppState>,
    limit: Option<u32>,
) -> Result<Vec<CleanerOperation>, AppError> {
    database_task(state, move |database_path| {
        cleaner::list_operations(database_path, limit.unwrap_or(20))
    })
    .await
}

// ── App slimming (应用瘦身) ────────────────────────────────────────────────────

/// Universal apps that could be thinned on this Apple Silicon Mac.
#[tauri::command]
pub async fn discover_slim_candidates(state: State<'_, AppState>) -> Result<SlimReport, AppError> {
    database_task(state, |database_path| {
        slim::discover_slim_candidates(database_path)
    })
    .await
}

/// Thin confirmed apps. Everything happens on a copy; the original only
/// moves to the trash after the slimmed copy is verified and swapped in.
#[tauri::command]
pub async fn execute_slim(
    state: State<'_, AppState>,
    items: Vec<SlimRequestItem>,
) -> Result<SlimOutcome, AppError> {
    database_task(state, move |database_path| {
        file_mutation(|| slim::execute_slim(database_path, &items))
    })
    .await
}

// ── System maintenance & status (系统维护 / 系统状态) ──────────────────────────

/// Available maintenance tasks with platform and privilege annotations.
#[tauri::command]
pub async fn get_maintenance_tasks() -> Result<MaintenanceReport, AppError> {
    Ok(maintenance::task_report())
}

/// Run one maintenance task from the closed registry. Privileged tasks make
/// the OS show its native authorization dialog.
#[tauri::command]
pub async fn run_maintenance_task(
    state: State<'_, AppState>,
    task: String,
) -> Result<MaintenanceOutcome, AppError> {
    database_task(state, move |database_path| {
        file_mutation(|| maintenance::run_task(database_path, &task))
    })
    .await
}

/// Compact CPU / memory / battery snapshot, parsed from platform tools.
#[tauri::command]
pub async fn get_system_status() -> Result<SystemStatus, AppError> {
    background_task(move || Ok(system_info::system_status())).await
}

// ── Tray performance monitor (CLN-403) ───────────────────────────────────────

/// Whether tray performance monitoring is switched on (persisted setting).
#[tauri::command]
pub async fn get_tray_monitor_enabled(state: State<'_, AppState>) -> Result<bool, AppError> {
    Ok(settings::load(&state.app_data_dir).tray_monitor_enabled)
}

/// Switch tray performance monitoring on or off: persists the choice,
/// updates the sampler and the tray menu, and notifies the frontend.
#[tauri::command]
pub async fn set_tray_monitor_enabled(app: AppHandle, enabled: bool) -> Result<(), AppError> {
    background_task(move || tray_monitor::apply_enabled(&app, enabled)).await
}

// ── Project slimming (项目瘦身) ────────────────────────────────────────────────

/// Discover development projects with rebuildable build artifacts under the
/// registered dev locations. On demand (user-triggered), budgeted.
#[tauri::command]
pub async fn discover_purge_projects(state: State<'_, AppState>) -> Result<PurgeReport, AppError> {
    database_task(state, |database_path| {
        purge::discover_projects(database_path)
    })
    .await
}

/// Trash confirmed artifact directories. Re-discovers first and verifies
/// every request against the fresh discovery (size, activity, whitelist).
#[tauri::command]
pub async fn execute_purge(
    state: State<'_, AppState>,
    items: Vec<CleanRequestItem>,
) -> Result<CleanOutcome, AppError> {
    database_task(state, move |database_path| {
        file_mutation(|| purge::execute_purge(database_path, &items))
    })
    .await
}

// ── Orphaned app data (应用残留清理) ───────────────────────────────────────────

/// Discover data left behind by already-removed apps under ~/Library.
/// Apps still installed never appear; read-only, user-triggered.
#[tauri::command]
pub async fn discover_orphan_app_data(
    state: State<'_, AppState>,
) -> Result<AppDataReport, AppError> {
    database_task(state, |database_path| {
        orphans::discover_orphans(database_path)
    })
    .await
}

/// Trash confirmed leftovers. Re-discovers first and verifies every request
/// against the fresh discovery (path + size, whitelist, protection).
#[tauri::command]
pub async fn execute_orphan_clean(
    state: State<'_, AppState>,
    items: Vec<CleanRequestItem>,
) -> Result<CleanOutcome, AppError> {
    database_task(state, move |database_path| {
        file_mutation(|| orphans::execute_orphan_clean(database_path, &items))
    })
    .await
}
