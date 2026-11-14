use super::{database_task, file_mutation, AppState};
use crate::{database, error::AppError, storage};
use tauri::State;

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
