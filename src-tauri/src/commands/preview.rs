use super::{background_task, database_task, AppState};
use crate::{
    error::AppError,
    file_manager::{self, DirNode, DirectoryListing},
};
use std::path::PathBuf;
use tauri::{AppHandle, State};
use tauri_plugin_opener::OpenerExt;

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
