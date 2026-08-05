use crate::{
    database,
    error::AppError,
    models::{FileEntry, InsightSummary, ScanFinished, ScanStatus, ScanSummary, StartScanRequest},
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
    database::list_insights(
        &state.database_path,
        &scan_id,
        threshold,
        stale_before,
        days,
    )
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

fn now_seconds() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|duration| i64::try_from(duration.as_secs()).ok())
        .unwrap_or_default()
}
