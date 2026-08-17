// File operations: trash, rename, move, copy.
//
// Safety model:
//   1. Every operation is atomic from the user's perspective:
//      FS change succeeds → DB updated. If the FS step fails, DB is untouched.
//   2. Delete always sends to the OS trash (never permanent).
//   3. Path conflicts (destination already exists) are reported, not silently
//      overwritten.
//   4. All path inputs are validated to be absolute before any I/O.

use crate::{database, error::AppError};
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::{
    ffi::OsString,
    path::Component,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};
use uuid::Uuid;

// ── Public result types ────────────────────────────────────────────────────────

/// Outcome of a single-file or batch operation.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpResult {
    pub operation_id: Option<String>,
    pub succeeded: Vec<String>,
    pub failed: Vec<OpFailure>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpFailure {
    pub path: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum UndoKind {
    Rename,
    Move,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationRecord {
    pub id: String,
    pub kind: String,
    pub label: String,
    pub status: String,
    pub created_at: i64,
    pub can_undo: bool,
}

#[derive(Debug, Clone)]
pub struct RenameOutcome {
    pub new_path: String,
    pub operation_id: String,
}

#[derive(Debug, Clone)]
pub(crate) struct FileSnapshot {
    pub size_bytes: u64,
    pub modified_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LoggedMove {
    from: String,
    to: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct UndoPayload {
    kind: UndoKind,
    moves: Vec<LoggedMove>,
}

// ── Trash ──────────────────────────────────────────────────────────────────────

/// Move indexed files to the OS trash. Failures are collected rather than
/// aborting the remaining batch.
pub fn trash_files(db_path: &Path, scan_id: &str, paths: &[String]) -> Result<OpResult, AppError> {
    let operation_id = begin_operation(
        db_path,
        scan_id,
        "trash",
        &format!("移至废纸篓（{} 项）", paths.len()),
        None,
    )?;
    let mut succeeded = Vec::new();
    let mut failed = Vec::new();

    for raw in paths {
        let native = match validate_indexed_file(db_path, scan_id, raw) {
            Ok(_) => to_native(raw),
            Err(error) => {
                failed.push(OpFailure {
                    path: raw.clone(),
                    reason: error.message,
                });
                continue;
            }
        };
        match trash_one(&native) {
            Ok(()) => {
                if let Err(error) = remove_db_entry(db_path, scan_id, raw) {
                    failed.push(OpFailure {
                        path: raw.clone(),
                        reason: format!(
                            "文件已移至废纸篓，但本地索引更新失败，请重新扫描：{}",
                            error.message
                        ),
                    });
                }
                succeeded.push(raw.clone());
            }
            Err(reason) => failed.push(OpFailure {
                path: raw.clone(),
                reason,
            }),
        }
    }

    finish_operation(db_path, &operation_id, succeeded.len(), &failed)?;
    Ok(OpResult {
        operation_id: Some(operation_id),
        succeeded,
        failed,
    })
}

fn trash_one(native: &Path) -> Result<(), String> {
    // Use the `trash` crate once it's in Cargo.toml; for now fall back to a
    // platform-specific shell command so the feature works without the crate.
    #[cfg(target_os = "macos")]
    {
        use std::process::Command;
        // `osascript` moves to Trash atomically and handles locked files better
        // than `mv ~/.Trash/`. The path must be POSIX-escaped.
        let posix = escape_applescript_string(&native.to_string_lossy());
        let script = format!(
            "tell application \"Finder\" to delete POSIX file \"{}\"",
            posix
        );
        let status = Command::new("osascript")
            .args(["-e", &script])
            .status()
            .map_err(|e| e.to_string())?;
        if status.success() {
            Ok(())
        } else {
            Err(format!("osascript exited with status {status}"))
        }
    }

    #[cfg(target_os = "windows")]
    {
        use std::process::Command;
        // PowerShell's Recycle-Item (Windows 10+)
        let path_str = native.to_string_lossy().replace('\'', "''");
        let ps = format!("Add-Type -AssemblyName Microsoft.VisualBasic; [Microsoft.VisualBasic.FileIO.FileSystem]::DeleteFile('{}', 'OnlyErrorDialogs', 'SendToRecycleBin')", path_str);
        let status = Command::new("powershell")
            .args(["-NoProfile", "-Command", &ps])
            .status()
            .map_err(|e| e.to_string())?;
        if status.success() {
            Ok(())
        } else {
            Err(format!("PowerShell exited with status {status}"))
        }
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        // Linux: move to ~/.local/share/Trash/files/ (FreeDesktop spec)
        let trash_dir = dirs::data_dir()
            .map(|d| d.join("Trash/files"))
            .ok_or_else(|| "Cannot locate trash directory".to_owned())?;
        std::fs::create_dir_all(&trash_dir).map_err(|e| e.to_string())?;
        let dest = trash_dir.join(
            native
                .file_name()
                .ok_or_else(|| "No file name".to_owned())?,
        );
        // Rename stays on the same filesystem when possible; fall back to copy+rm.
        if std::fs::rename(native, &dest).is_ok() {
            Ok(())
        } else {
            std::fs::copy(native, &dest).map_err(|e| e.to_string())?;
            std::fs::remove_file(native).map_err(|e| e.to_string())?;
            Ok(())
        }
    }
}

// ── Rename ─────────────────────────────────────────────────────────────────────

/// Rename a single file or directory in-place (same parent, new name).
/// Returns the new canonical path on success.
pub fn rename_file(
    db_path: &Path,
    scan_id: &str,
    old_path: &str,
    new_name: &str,
) -> Result<RenameOutcome, AppError> {
    validate_name(new_name)?;
    validate_indexed_file(db_path, scan_id, old_path)?;

    let native_old = to_native(old_path);
    let parent = native_old
        .parent()
        .ok_or_else(|| AppError::new("INVALID_PATH", "Cannot rename the root path."))?;
    let native_new = parent.join(new_name);
    validate_write_path(&to_canonical(&native_new))?;

    if native_new.exists() {
        return Err(AppError::new(
            "ALREADY_EXISTS",
            format!("A file named \"{new_name}\" already exists in this location."),
        ));
    }

    let new_canonical = to_canonical(&native_new);
    let operation_id = begin_operation(
        db_path,
        scan_id,
        "rename",
        &format!("重命名为 {new_name}"),
        Some(UndoKind::Rename),
    )?;

    if let Err(error) = std::fs::rename(&native_old, &native_new) {
        let failure = OpFailure {
            path: old_path.to_owned(),
            reason: error.to_string(),
        };
        finish_operation(db_path, &operation_id, 0, std::slice::from_ref(&failure))?;
        return Err(AppError::new("OP_FAILED", failure.reason));
    }

    // Update the DB: the stored path uses '/' separators.
    if let Err(error) = rename_db_path(db_path, scan_id, old_path, &new_canonical) {
        let _ = std::fs::rename(&native_new, &native_old);
        let failure = OpFailure {
            path: old_path.to_owned(),
            reason: error.message.clone(),
        };
        finish_operation(db_path, &operation_id, 0, std::slice::from_ref(&failure))?;
        return Err(error);
    }
    if let Err(error) = append_operation_move(db_path, &operation_id, old_path, &new_canonical) {
        let _ = rename_db_path(db_path, scan_id, &new_canonical, old_path);
        let _ = std::fs::rename(&native_new, &native_old);
        let failure = OpFailure {
            path: old_path.to_owned(),
            reason: error.message.clone(),
        };
        finish_operation(db_path, &operation_id, 0, std::slice::from_ref(&failure))?;
        return Err(error);
    }
    finish_operation(db_path, &operation_id, 1, &[])?;

    Ok(RenameOutcome {
        new_path: new_canonical,
        operation_id,
    })
}

// ── Move ───────────────────────────────────────────────────────────────────────

/// Move files to `dest_dir`. Fails if any destination already exists.
pub fn move_files(
    db_path: &Path,
    scan_id: &str,
    paths: &[String],
    dest_dir: &str,
) -> Result<OpResult, AppError> {
    let native_dest = validate_write_path(dest_dir)?;
    if !native_dest.is_dir() {
        return Err(AppError::new(
            "INVALID_PATH",
            format!("Destination is not a directory: {dest_dir}"),
        ));
    }

    let operation_id = begin_operation(
        db_path,
        scan_id,
        "move",
        &format!("移动文件（{} 项）", paths.len()),
        Some(UndoKind::Move),
    )?;
    let mut succeeded = Vec::new();
    let mut failed = Vec::new();

    for raw in paths {
        if let Err(error) = validate_indexed_file(db_path, scan_id, raw) {
            failed.push(OpFailure {
                path: raw.clone(),
                reason: error.message,
            });
            continue;
        }
        let native_src = to_native(raw);
        let file_name = match native_src.file_name() {
            Some(n) => n,
            None => {
                failed.push(OpFailure {
                    path: raw.clone(),
                    reason: "No file name".into(),
                });
                continue;
            }
        };
        let native_dst = native_dest.join(file_name);
        if let Err(error) = validate_write_path(&to_canonical(&native_dst)) {
            failed.push(OpFailure {
                path: raw.clone(),
                reason: error.message,
            });
            continue;
        }

        if native_dst.exists() {
            failed.push(OpFailure {
                path: raw.clone(),
                reason: format!(
                    "\"{}\" already exists in the destination.",
                    file_name.to_string_lossy()
                ),
            });
            continue;
        }

        match std::fs::rename(&native_src, &native_dst) {
            Ok(()) => {
                let new_canonical = to_canonical(&native_dst);
                if let Err(error) = rename_db_path(db_path, scan_id, raw, &new_canonical) {
                    let _ = std::fs::rename(&native_dst, &native_src);
                    failed.push(OpFailure {
                        path: raw.clone(),
                        reason: error.message,
                    });
                } else {
                    if let Err(error) =
                        append_operation_move(db_path, &operation_id, raw, &new_canonical)
                    {
                        let _ = rename_db_path(db_path, scan_id, &new_canonical, raw);
                        let _ = move_path(&native_dst, &native_src);
                        failed.push(OpFailure {
                            path: raw.clone(),
                            reason: error.message,
                        });
                    } else {
                        succeeded.push(raw.clone());
                    }
                }
            }
            Err(e) => {
                // Cross-device rename fails; fall back to copy + remove
                match copy_then_remove(&native_src, &native_dst) {
                    Ok(()) => {
                        let new_canonical = to_canonical(&native_dst);
                        if let Err(error) = rename_db_path(db_path, scan_id, raw, &new_canonical) {
                            let _ = copy_then_remove(&native_dst, &native_src);
                            failed.push(OpFailure {
                                path: raw.clone(),
                                reason: error.message,
                            });
                        } else {
                            if let Err(error) =
                                append_operation_move(db_path, &operation_id, raw, &new_canonical)
                            {
                                let _ = rename_db_path(db_path, scan_id, &new_canonical, raw);
                                let _ = move_path(&native_dst, &native_src);
                                failed.push(OpFailure {
                                    path: raw.clone(),
                                    reason: error.message,
                                });
                            } else {
                                succeeded.push(raw.clone());
                            }
                        }
                    }
                    Err(copy_err) => failed.push(OpFailure {
                        path: raw.clone(),
                        reason: format!("{e} / {copy_err}"),
                    }),
                }
            }
        }
    }

    finish_operation(db_path, &operation_id, succeeded.len(), &failed)?;
    Ok(OpResult {
        operation_id: Some(operation_id),
        succeeded,
        failed,
    })
}

// ── Copy ───────────────────────────────────────────────────────────────────────

/// Copy files to `dest_dir`. Adds a DB entry for each copy in the same scan.
pub fn copy_files(
    db_path: &Path,
    scan_id: &str,
    paths: &[String],
    dest_dir: &str,
) -> Result<OpResult, AppError> {
    let native_dest = validate_write_path(dest_dir)?;
    if !native_dest.is_dir() {
        return Err(AppError::new(
            "INVALID_PATH",
            format!("Destination is not a directory: {dest_dir}"),
        ));
    }

    let operation_id = begin_operation(
        db_path,
        scan_id,
        "copy",
        &format!("复制文件（{} 项）", paths.len()),
        None,
    )?;
    let mut succeeded = Vec::new();
    let mut failed = Vec::new();

    for raw in paths {
        if let Err(error) = validate_indexed_file(db_path, scan_id, raw) {
            failed.push(OpFailure {
                path: raw.clone(),
                reason: error.message,
            });
            continue;
        }
        let native_src = to_native(raw);
        let file_name = match native_src.file_name() {
            Some(n) => n,
            None => {
                failed.push(OpFailure {
                    path: raw.clone(),
                    reason: "No file name".into(),
                });
                continue;
            }
        };
        let native_dst = native_dest.join(file_name);
        if let Err(error) = validate_write_path(&to_canonical(&native_dst)) {
            failed.push(OpFailure {
                path: raw.clone(),
                reason: error.message,
            });
            continue;
        }

        if native_dst.exists() {
            failed.push(OpFailure {
                path: raw.clone(),
                reason: format!(
                    "\"{}\" already exists in the destination.",
                    file_name.to_string_lossy()
                ),
            });
            continue;
        }

        match std::fs::copy(&native_src, &native_dst) {
            Ok(_) => {
                let new_canonical = to_canonical(&native_dst);
                if let Err(error) = clone_db_entry(db_path, scan_id, raw, &new_canonical) {
                    let _ = std::fs::remove_file(&native_dst);
                    failed.push(OpFailure {
                        path: raw.clone(),
                        reason: error.message,
                    });
                } else {
                    succeeded.push(raw.clone());
                }
            }
            Err(e) => failed.push(OpFailure {
                path: raw.clone(),
                reason: e.to_string(),
            }),
        }
    }

    finish_operation(db_path, &operation_id, succeeded.len(), &failed)?;
    Ok(OpResult {
        operation_id: Some(operation_id),
        succeeded,
        failed,
    })
}

// ── Persistent operation log + undo ─────────────────────────────────────────

pub fn list_undoable_operations(
    db_path: &Path,
    scan_id: &str,
    limit: u32,
) -> Result<Vec<OperationRecord>, AppError> {
    let connection = database::open(db_path)?;
    let root_path = connection
        .query_row(
            "SELECT root_path FROM scan_runs WHERE id = ?1",
            [scan_id],
            |row| row.get::<_, String>(0),
        )
        .optional()?
        .ok_or_else(|| AppError::new("SCAN_NOT_FOUND", "找不到当前扫描记录。"))?;
    let mut statement = connection.prepare(
        "SELECT id, kind, label, status, created_at, undo_json
         FROM file_operations
         WHERE root_path = ?1
           AND status IN ('completed', 'partial', 'interrupted', 'undo_failed')
           AND undo_json IS NOT NULL
         ORDER BY created_at DESC
         LIMIT ?2",
    )?;
    let rows = statement.query_map(params![root_path, i64::from(limit.clamp(1, 50))], |row| {
        let undo_json: String = row.get(5)?;
        let can_undo = serde_json::from_str::<UndoPayload>(&undo_json)
            .map(|payload| !payload.moves.is_empty())
            .unwrap_or(false);
        Ok(OperationRecord {
            id: row.get(0)?,
            kind: row.get(1)?,
            label: row.get(2)?,
            status: row.get(3)?,
            created_at: row.get(4)?,
            can_undo,
        })
    })?;
    Ok(rows
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .filter(|operation| operation.can_undo)
        .collect())
}

pub fn undo_operation(
    db_path: &Path,
    operation_id: &str,
    active_scan_id: &str,
) -> Result<OpResult, AppError> {
    let connection = database::open(db_path)?;
    let operation = connection
        .query_row(
            "SELECT root_path, status, undo_json FROM file_operations WHERE id = ?1",
            [operation_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                ))
            },
        )
        .optional()?
        .ok_or_else(|| AppError::new("OP_NOT_FOUND", "找不到可撤销的操作记录。"))?;
    let active_root = connection
        .query_row(
            "SELECT root_path FROM scan_runs WHERE id = ?1",
            [active_scan_id],
            |row| row.get::<_, String>(0),
        )
        .optional()?
        .ok_or_else(|| AppError::new("SCAN_NOT_FOUND", "找不到当前扫描记录。"))?;
    drop(connection);
    if active_root != operation.0 {
        return Err(AppError::new(
            "SCAN_MISMATCH",
            "该撤销记录不属于当前扫描目录。",
        ));
    }

    if operation.1 == "running" || operation.1 == "undoing" {
        return Err(AppError::new("OP_BUSY", "该操作仍在执行，暂时无法撤销。"));
    }
    if operation.1 == "undone" {
        return Err(AppError::new("OP_ALREADY_UNDONE", "该操作已经撤销。"));
    }

    let mut payload = serde_json::from_str::<UndoPayload>(
        operation
            .2
            .as_deref()
            .ok_or_else(|| AppError::new("OP_NOT_UNDOABLE", "该操作不支持撤销。"))?,
    )
    .map_err(|error| AppError::new("DATABASE_ERROR", error.to_string()))?;
    if payload.moves.is_empty() {
        return Err(AppError::new(
            "OP_NOT_UNDOABLE",
            "该操作没有可撤销的成功项。",
        ));
    }

    database::open(db_path)?.execute(
        "UPDATE file_operations SET status = 'undoing', error_message = NULL WHERE id = ?1",
        [operation_id],
    )?;

    let mut succeeded = Vec::new();
    let mut failed = Vec::new();
    let mut remaining = Vec::new();

    for movement in payload.moves.iter().rev() {
        let current = movement.to.as_str();
        let original = movement.from.as_str();
        let native_current = match validate_indexed_file(db_path, active_scan_id, current) {
            Ok(_) => to_native(current),
            Err(error) => {
                failed.push(OpFailure {
                    path: current.to_owned(),
                    reason: error.message,
                });
                remaining.push(movement.clone());
                continue;
            }
        };
        let native_original = match validate_write_path(original) {
            Ok(path) => path,
            Err(error) => {
                failed.push(OpFailure {
                    path: current.to_owned(),
                    reason: error.message,
                });
                remaining.push(movement.clone());
                continue;
            }
        };
        if native_original.exists() {
            failed.push(OpFailure {
                path: current.to_owned(),
                reason: format!("原路径已被占用：{original}"),
            });
            remaining.push(movement.clone());
            continue;
        }
        if let Some(parent) = native_original.parent() {
            if let Err(error) = std::fs::create_dir_all(parent) {
                failed.push(OpFailure {
                    path: current.to_owned(),
                    reason: error.to_string(),
                });
                remaining.push(movement.clone());
                continue;
            }
        }

        if let Err(error) = move_path(&native_current, &native_original) {
            failed.push(OpFailure {
                path: current.to_owned(),
                reason: error,
            });
            remaining.push(movement.clone());
            continue;
        }
        if let Err(error) = rename_db_path(db_path, active_scan_id, current, original) {
            let _ = move_path(&native_original, &native_current);
            failed.push(OpFailure {
                path: current.to_owned(),
                reason: error.message,
            });
            remaining.push(movement.clone());
            continue;
        }
        succeeded.push(current.to_owned());
    }

    remaining.reverse();
    payload.moves = remaining;
    let status = if failed.is_empty() {
        "undone"
    } else {
        "undo_failed"
    };
    let undo_json = if payload.moves.is_empty() {
        None
    } else {
        Some(
            serde_json::to_string(&payload)
                .map_err(|error| AppError::new("DATABASE_ERROR", error.to_string()))?,
        )
    };
    let error_message = if failed.is_empty() {
        None
    } else {
        Some(
            failed
                .iter()
                .map(|failure| format!("{}: {}", failure.path, failure.reason))
                .collect::<Vec<_>>()
                .join("\n"),
        )
    };
    database::open(db_path)?.execute(
        "UPDATE file_operations
         SET status = ?2, undo_json = ?3, finished_at = ?4, error_message = ?5
         WHERE id = ?1",
        params![
            operation_id,
            status,
            undo_json,
            now_seconds(),
            error_message
        ],
    )?;

    Ok(OpResult {
        operation_id: Some(operation_id.to_owned()),
        succeeded,
        failed,
    })
}

pub(crate) fn begin_operation(
    db_path: &Path,
    scan_id: &str,
    kind: &str,
    label: &str,
    undo_kind: Option<UndoKind>,
) -> Result<String, AppError> {
    let operation_id = Uuid::new_v4().to_string();
    let undo_json = undo_kind
        .map(|kind| UndoPayload {
            kind,
            moves: Vec::new(),
        })
        .map(|payload| serde_json::to_string(&payload))
        .transpose()
        .map_err(|error| AppError::new("DATABASE_ERROR", error.to_string()))?;
    let inserted = database::open(db_path)?.execute(
        "INSERT INTO file_operations
         (id, scan_id, root_path, kind, label, status, undo_json, created_at)
         SELECT ?1, ?2, root_path, ?3, ?4, 'running', ?5, ?6
         FROM scan_runs WHERE id = ?2",
        params![operation_id, scan_id, kind, label, undo_json, now_seconds()],
    )?;
    if inserted != 1 {
        return Err(AppError::new("SCAN_NOT_FOUND", "找不到当前扫描记录。"));
    }
    Ok(operation_id)
}

pub(crate) fn append_operation_move(
    db_path: &Path,
    operation_id: &str,
    from: &str,
    to: &str,
) -> Result<(), AppError> {
    let connection = database::open(db_path)?;
    let undo_json: String = connection.query_row(
        "SELECT undo_json FROM file_operations WHERE id = ?1",
        [operation_id],
        |row| row.get(0),
    )?;
    let mut payload = serde_json::from_str::<UndoPayload>(&undo_json)
        .map_err(|error| AppError::new("DATABASE_ERROR", error.to_string()))?;
    payload.moves.push(LoggedMove {
        from: from.to_owned(),
        to: to.to_owned(),
    });
    let updated = serde_json::to_string(&payload)
        .map_err(|error| AppError::new("DATABASE_ERROR", error.to_string()))?;
    connection.execute(
        "UPDATE file_operations SET undo_json = ?2 WHERE id = ?1",
        params![operation_id, updated],
    )?;
    Ok(())
}

pub(crate) fn finish_operation(
    db_path: &Path,
    operation_id: &str,
    succeeded: usize,
    failed: &[OpFailure],
) -> Result<(), AppError> {
    let status = match (succeeded, failed.is_empty()) {
        (_, true) => "completed",
        (0, false) => "failed",
        _ => "partial",
    };
    let error_message = if failed.is_empty() {
        None
    } else {
        Some(
            failed
                .iter()
                .map(|failure| format!("{}: {}", failure.path, failure.reason))
                .collect::<Vec<_>>()
                .join("\n"),
        )
    };
    database::open(db_path)?.execute(
        "UPDATE file_operations
         SET status = ?2, finished_at = ?3, error_message = ?4
         WHERE id = ?1",
        params![operation_id, status, now_seconds(), error_message],
    )?;
    Ok(())
}

pub(crate) fn validate_indexed_file(
    db_path: &Path,
    scan_id: &str,
    canonical_path: &str,
) -> Result<FileSnapshot, AppError> {
    let native_path = validate_write_path(canonical_path)?;
    let snapshot = database::open(db_path)?
        .query_row(
            "SELECT size_bytes, modified_at FROM files WHERE scan_id = ?1 AND path = ?2 LIMIT 1",
            params![scan_id, canonical_path],
            |row| {
                Ok(FileSnapshot {
                    size_bytes: crate::database::row_u64(row, 0),
                    modified_at: row.get(1)?,
                })
            },
        )
        .optional()?
        .ok_or_else(|| AppError::new("STALE_INDEX", "文件已不在当前扫描索引中，请重新扫描。"))?;
    validate_snapshot_on_disk(&native_path, &snapshot)?;
    Ok(snapshot)
}

pub(crate) fn validate_snapshot_on_disk(
    native_path: &Path,
    expected: &FileSnapshot,
) -> Result<(), AppError> {
    let metadata = std::fs::metadata(native_path)
        .map_err(|error| AppError::new("STALE_FILE", error.to_string()))?;
    if !metadata.is_file() {
        return Err(AppError::new(
            "UNSUPPORTED_PATH",
            "当前只允许操作普通文件。",
        ));
    }
    let actual_modified = metadata.modified().ok().and_then(system_time_seconds);
    if metadata.len() != expected.size_bytes || actual_modified != expected.modified_at {
        return Err(AppError::new(
            "FILE_CHANGED",
            "文件在扫描后发生变化，请重新扫描并再次确认。",
        ));
    }
    Ok(())
}

pub(crate) fn validate_write_path(canonical_path: &str) -> Result<PathBuf, AppError> {
    let native_path = to_native(canonical_path);
    if !native_path.is_absolute() {
        return Err(AppError::invalid_path("文件操作只接受绝对路径。"));
    }
    if native_path
        .components()
        .any(|component| matches!(component, Component::ParentDir))
    {
        return Err(AppError::invalid_path("文件操作路径不能包含上级目录跳转。"));
    }
    let protection_path = resolve_for_protection(&native_path);
    if is_protected_path(&protection_path) {
        return Err(AppError::new(
            "PROTECTED_PATH",
            format!("为保护系统和应用数据，Luma 不允许修改此路径：{canonical_path}"),
        ));
    }
    Ok(native_path)
}

// ── DB helpers ─────────────────────────────────────────────────────────────────

fn remove_db_entry(db_path: &Path, scan_id: &str, path: &str) -> Result<(), AppError> {
    let conn = database::open(db_path)?;
    conn.execute(
        "DELETE FROM files WHERE scan_id = ?1 AND path = ?2",
        params![scan_id, path],
    )?;
    Ok(())
}

/// Update the path of every DB row whose stored path starts with `old_path`.
/// Handles single files (exact match) and directory moves (prefix match).
fn rename_db_path(
    db_path: &Path,
    scan_id: &str,
    old_path: &str,
    new_path: &str,
) -> Result<(), AppError> {
    let conn = database::open(db_path)?;

    // Exact match: single file
    conn.execute(
        "UPDATE files SET path = ?3, name = ?4
         WHERE scan_id = ?1 AND path = ?2",
        params![scan_id, old_path, new_path, basename(new_path),],
    )?;

    // Prefix match: directory rename — rewrite all descendant paths
    let prefix_old = format!("{old_path}/");
    let prefix_new = format!("{new_path}/");
    // SQLite doesn't have string replace on UPDATE directly, but we can build it:
    let prefix_len = i64::try_from(prefix_old.len()).unwrap_or(0) + 1; // 1-based
    conn.execute(
        "UPDATE files
         SET path = ?3 || SUBSTR(path, ?4)
         WHERE scan_id = ?1 AND path LIKE ?2 ESCAPE '\\'",
        params![
            scan_id,
            format!("{}%", escape_like(&prefix_old)),
            prefix_new,
            prefix_len,
        ],
    )?;

    Ok(())
}

/// Insert a new DB row for the copied file, derived from the original's row.
fn clone_db_entry(
    db_path: &Path,
    scan_id: &str,
    src_path: &str,
    dst_path: &str,
) -> Result<(), AppError> {
    let conn = database::open(db_path)?;
    // Copy metadata from the source row; update path and name.
    conn.execute(
        "INSERT INTO files (scan_id, path, name, extension, category, size_bytes, modified_at, is_hidden, content_hash)
         SELECT scan_id, ?3, ?4, extension, category, size_bytes, modified_at, is_hidden, content_hash
         FROM files WHERE scan_id = ?1 AND path = ?2
         LIMIT 1",
        params![scan_id, src_path, dst_path, basename(dst_path)],
    )?;
    Ok(())
}

// ── Path utilities ─────────────────────────────────────────────────────────────

/// Convert a stored canonical path (`/`-separated) to a native `PathBuf`.
fn to_native(canonical: &str) -> PathBuf {
    PathBuf::from(canonical.replace('/', std::path::MAIN_SEPARATOR_STR))
}

/// Convert a native `PathBuf` back to the canonical `/`-separated string stored
/// in the DB.
fn to_canonical(native: &Path) -> String {
    native
        .to_string_lossy()
        .replace(std::path::MAIN_SEPARATOR, "/")
}

fn basename(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

fn validate_name(name: &str) -> Result<(), AppError> {
    if name.is_empty() {
        return Err(AppError::new("INVALID_NAME", "File name cannot be empty."));
    }
    // Reject path separators and null bytes inside a name.
    if name.contains(['/', '\\', '\0']) {
        return Err(AppError::new(
            "INVALID_NAME",
            "File name contains invalid characters.",
        ));
    }
    Ok(())
}

fn escape_like(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        if matches!(ch, '\\' | '%' | '_') {
            out.push('\\');
        }
        out.push(ch);
    }
    out
}

fn is_protected_path(path: &Path) -> bool {
    if path.parent().is_none() {
        return true;
    }

    #[cfg(target_os = "windows")]
    {
        let normalized = path
            .to_string_lossy()
            .replace('/', "\\")
            .to_ascii_lowercase();
        let protected = [
            r"c:\windows",
            r"c:\program files",
            r"c:\program files (x86)",
            r"c:\programdata",
        ];
        return protected
            .iter()
            .any(|root| normalized == *root || normalized.starts_with(&format!("{root}\\")));
    }

    #[cfg(not(target_os = "windows"))]
    {
        let protected = [
            Path::new("/System"),
            Path::new("/Library"),
            Path::new("/Applications"),
            Path::new("/bin"),
            Path::new("/sbin"),
            Path::new("/usr"),
            Path::new("/private"),
            Path::new("/etc"),
            Path::new("/var"),
        ];
        protected
            .iter()
            .any(|root| path == *root || path.starts_with(root))
    }
}

fn escape_applescript_string(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

fn resolve_for_protection(path: &Path) -> PathBuf {
    let mut existing = path;
    let mut suffix: Vec<OsString> = Vec::new();
    while !existing.exists() {
        let Some(name) = existing.file_name() else {
            break;
        };
        suffix.push(name.to_os_string());
        let Some(parent) = existing.parent() else {
            break;
        };
        existing = parent;
    }
    let mut resolved = std::fs::canonicalize(existing).unwrap_or_else(|_| existing.to_path_buf());
    for segment in suffix.iter().rev() {
        resolved.push(segment);
    }
    resolved
}

fn move_path(src: &Path, dst: &Path) -> Result<(), String> {
    match std::fs::rename(src, dst) {
        Ok(()) => Ok(()),
        Err(rename_error) => copy_then_remove(src, dst)
            .map_err(|copy_error| format!("{rename_error} / {copy_error}")),
    }
}

fn system_time_seconds(time: SystemTime) -> Option<i64> {
    time.duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|duration| i64::try_from(duration.as_secs()).ok())
}

fn now_seconds() -> i64 {
    system_time_seconds(SystemTime::now()).unwrap_or_default()
}

/// Copy `src` to `dst` then remove `src`; used when `rename` fails across
/// device boundaries.
fn copy_then_remove(src: &Path, dst: &Path) -> Result<(), String> {
    std::fs::copy(src, dst).map_err(|e| e.to_string())?;
    if let Err(error) = std::fs::remove_file(src) {
        let cleanup_error = std::fs::remove_file(dst).err();
        return Err(match cleanup_error {
            Some(cleanup) => {
                format!("复制完成但删除源文件失败：{error}；回滚目标副本也失败：{cleanup}")
            }
            None => format!("复制完成但删除源文件失败，已移除目标副本：{error}"),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        list_undoable_operations, move_files, system_time_seconds, to_canonical, undo_operation,
        validate_write_path,
    };
    use crate::{database, models::FileEntry};
    use std::{fs, path::PathBuf};
    use uuid::Uuid;

    struct Fixture {
        root: PathBuf,
        database_path: PathBuf,
        source_path: PathBuf,
        destination_dir: PathBuf,
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    fn fixture() -> Fixture {
        let root = std::env::current_dir()
            .expect("current directory")
            .join("target")
            .join(format!("luma-file-ops-{}", Uuid::new_v4()));
        let source_dir = root.join("source");
        let destination_dir = root.join("destination");
        fs::create_dir_all(&source_dir).expect("create source directory");
        fs::create_dir_all(&destination_dir).expect("create destination directory");
        let source_path = source_dir.join("report.txt");
        fs::write(&source_path, b"hello").expect("write source file");

        let database_path = root.join("luma.sqlite3");
        database::initialize(&database_path).expect("initialize database");
        let root_canonical = to_canonical(&root);
        database::create_scan_run(&database_path, "scan-1", &root_canonical, 1)
            .expect("create scan");
        let metadata = fs::metadata(&source_path).expect("source metadata");
        let entry = FileEntry {
            id: 0,
            path: to_canonical(&source_path),
            name: "report.txt".to_owned(),
            extension: Some("txt".to_owned()),
            category: "documents".to_owned(),
            size_bytes: metadata.len(),
            modified_at: metadata.modified().ok().and_then(system_time_seconds),
            is_hidden: false,
            content_hash: None,
        };
        let mut connection = database::open(&database_path).expect("open database");
        database::insert_file_batch(&mut connection, "scan-1", &[entry])
            .expect("insert source file");
        drop(connection);

        Fixture {
            root,
            database_path,
            source_path,
            destination_dir,
        }
    }

    #[test]
    fn move_is_logged_and_can_be_undone_after_reload() {
        let fixture = fixture();
        let source = to_canonical(&fixture.source_path);
        let destination = to_canonical(&fixture.destination_dir);
        let moved_path = fixture.destination_dir.join("report.txt");

        let result = move_files(
            &fixture.database_path,
            "scan-1",
            std::slice::from_ref(&source),
            &destination,
        )
        .expect("move file");
        assert_eq!(result.succeeded, vec![source.clone()]);
        assert!(moved_path.exists());
        assert!(!fixture.source_path.exists());

        // Simulate a new scan of the same root before undoing. The operation
        // remains discoverable, and undo updates the active scan index.
        database::create_scan_run(
            &fixture.database_path,
            "scan-2",
            &to_canonical(&fixture.root),
            2,
        )
        .expect("create second scan");
        let moved_metadata = fs::metadata(&moved_path).expect("moved metadata");
        let mut connection = database::open(&fixture.database_path).expect("open second scan");
        database::insert_file_batch(
            &mut connection,
            "scan-2",
            &[FileEntry {
                id: 0,
                path: to_canonical(&moved_path),
                name: "report.txt".to_owned(),
                extension: Some("txt".to_owned()),
                category: "documents".to_owned(),
                size_bytes: moved_metadata.len(),
                modified_at: moved_metadata.modified().ok().and_then(system_time_seconds),
                is_hidden: false,
                content_hash: None,
            }],
        )
        .expect("insert moved file into second scan");
        drop(connection);

        let operations = list_undoable_operations(&fixture.database_path, "scan-2", 20)
            .expect("list undoable operations");
        assert_eq!(operations.len(), 1);
        assert_eq!(operations[0].id, result.operation_id.expect("operation id"));

        let undo = undo_operation(&fixture.database_path, &operations[0].id, "scan-2")
            .expect("undo persisted operation");
        assert!(undo.failed.is_empty());
        assert!(fixture.source_path.exists());
        assert!(!moved_path.exists());
        assert!(
            list_undoable_operations(&fixture.database_path, "scan-2", 20)
                .expect("reload undoable operations")
                .is_empty()
        );
        let active_path: String = database::open(&fixture.database_path)
            .expect("open active scan")
            .query_row(
                "SELECT path FROM files WHERE scan_id = 'scan-2'",
                [],
                |row| row.get(0),
            )
            .expect("query active path");
        assert_eq!(active_path, source);
    }

    #[test]
    fn changed_file_is_rejected_before_move() {
        let fixture = fixture();
        fs::write(&fixture.source_path, b"changed after scan").expect("change source file");
        let result = move_files(
            &fixture.database_path,
            "scan-1",
            &[to_canonical(&fixture.source_path)],
            &to_canonical(&fixture.destination_dir),
        )
        .expect("return per-file failure");
        assert!(result.succeeded.is_empty());
        assert_eq!(result.failed.len(), 1);
        assert!(result.failed[0].reason.contains("重新扫描"));
        assert!(fixture.source_path.exists());
    }

    #[test]
    fn system_locations_are_protected() {
        #[cfg(target_os = "windows")]
        let protected = r"C:/Windows/System32/example.dll";
        #[cfg(not(target_os = "windows"))]
        let protected = "/System/Library/example";

        let error = validate_write_path(protected).expect_err("protected path must fail");
        assert_eq!(error.code, "PROTECTED_PATH");

        #[cfg(target_os = "windows")]
        let traversal = r"C:/Users/example/../Windows/System32/example.dll";
        #[cfg(not(target_os = "windows"))]
        let traversal = "/Users/example/../System/Library/example";
        let error = validate_write_path(traversal).expect_err("parent traversal must fail");
        assert_eq!(error.code, "INVALID_PATH");
    }

    #[test]
    fn escapes_applescript_file_names() {
        assert_eq!(
            super::escape_applescript_string("quote\" and \\ slash"),
            "quote\\\" and \\\\ slash"
        );
    }
}
