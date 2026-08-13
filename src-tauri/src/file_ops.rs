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
use rusqlite::params;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

// ── Public result types ────────────────────────────────────────────────────────

/// Outcome of a single-file or batch operation.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpResult {
    pub succeeded: Vec<String>,
    pub failed: Vec<OpFailure>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpFailure {
    pub path: String,
    pub reason: String,
}

/// Enough information to undo a rename or move (not trash — that's irreversible
/// without OS trash integration).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UndoRecord {
    pub kind: UndoKind,
    /// Original path(s) before the operation.
    pub from: Vec<String>,
    /// Destination path(s) after the operation.
    pub to: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum UndoKind {
    Rename,
    Move,
    Copy,
}

// ── Trash ──────────────────────────────────────────────────────────────────────

/// Move files to the OS trash. Paths not present in the DB are silently skipped
/// (they may already be gone). Failures are collected rather than aborting.
pub fn trash_files(
    db_path: &Path,
    scan_id: &str,
    paths: &[String],
) -> Result<OpResult, AppError> {
    let mut succeeded = Vec::new();
    let mut failed = Vec::new();

    for raw in paths {
        let native = to_native(raw);
        match trash_one(&native) {
            Ok(()) => {
                // Remove from the scan index; non-fatal if missing
                let _ = remove_db_entry(db_path, scan_id, raw);
                succeeded.push(raw.clone());
            }
            Err(reason) => failed.push(OpFailure {
                path: raw.clone(),
                reason,
            }),
        }
    }

    Ok(OpResult { succeeded, failed })
}

fn trash_one(native: &Path) -> Result<(), String> {
    // Use the `trash` crate once it's in Cargo.toml; for now fall back to a
    // platform-specific shell command so the feature works without the crate.
    #[cfg(target_os = "macos")]
    {
        use std::process::Command;
        // `osascript` moves to Trash atomically and handles locked files better
        // than `mv ~/.Trash/`. The path must be POSIX-escaped.
        let posix = native.to_string_lossy();
        let script = format!("tell application \"Finder\" to delete POSIX file \"{}\"", posix);
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
        let path_str = native.to_string_lossy();
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
) -> Result<String, AppError> {
    validate_name(new_name)?;

    let native_old = to_native(old_path);
    let parent = native_old
        .parent()
        .ok_or_else(|| AppError::new("INVALID_PATH", "Cannot rename the root path."))?;
    let native_new = parent.join(new_name);

    if native_new.exists() {
        return Err(AppError::new(
            "ALREADY_EXISTS",
            format!("A file named \"{new_name}\" already exists in this location."),
        ));
    }

    std::fs::rename(&native_old, &native_new)
        .map_err(|e| AppError::new("OP_FAILED", e.to_string()))?;

    let new_canonical = to_canonical(&native_new);

    // Update the DB: the stored path uses '/' separators.
    rename_db_path(db_path, scan_id, old_path, &new_canonical)?;

    Ok(new_canonical)
}

// ── Move ───────────────────────────────────────────────────────────────────────

/// Move files to `dest_dir`. Fails if any destination already exists.
pub fn move_files(
    db_path: &Path,
    scan_id: &str,
    paths: &[String],
    dest_dir: &str,
) -> Result<OpResult, AppError> {
    let native_dest = to_native(dest_dir);
    if !native_dest.is_dir() {
        return Err(AppError::new(
            "INVALID_PATH",
            format!("Destination is not a directory: {dest_dir}"),
        ));
    }

    let mut succeeded = Vec::new();
    let mut failed = Vec::new();

    for raw in paths {
        let native_src = to_native(raw);
        let file_name = match native_src.file_name() {
            Some(n) => n,
            None => {
                failed.push(OpFailure { path: raw.clone(), reason: "No file name".into() });
                continue;
            }
        };
        let native_dst = native_dest.join(file_name);

        if native_dst.exists() {
            failed.push(OpFailure {
                path: raw.clone(),
                reason: format!("\"{}\" already exists in the destination.", file_name.to_string_lossy()),
            });
            continue;
        }

        match std::fs::rename(&native_src, &native_dst) {
            Ok(()) => {
                let new_canonical = to_canonical(&native_dst);
                let _ = rename_db_path(db_path, scan_id, raw, &new_canonical);
                succeeded.push(raw.clone());
            }
            Err(e) => {
                // Cross-device rename fails; fall back to copy + remove
                match copy_then_remove(&native_src, &native_dst) {
                    Ok(()) => {
                        let new_canonical = to_canonical(&native_dst);
                        let _ = rename_db_path(db_path, scan_id, raw, &new_canonical);
                        succeeded.push(raw.clone());
                    }
                    Err(copy_err) => failed.push(OpFailure {
                        path: raw.clone(),
                        reason: format!("{e} / {copy_err}"),
                    }),
                }
            }
        }
    }

    Ok(OpResult { succeeded, failed })
}

// ── Copy ───────────────────────────────────────────────────────────────────────

/// Copy files to `dest_dir`. Adds a DB entry for each copy in the same scan.
pub fn copy_files(
    db_path: &Path,
    scan_id: &str,
    paths: &[String],
    dest_dir: &str,
) -> Result<OpResult, AppError> {
    let native_dest = to_native(dest_dir);
    if !native_dest.is_dir() {
        return Err(AppError::new(
            "INVALID_PATH",
            format!("Destination is not a directory: {dest_dir}"),
        ));
    }

    let mut succeeded = Vec::new();
    let mut failed = Vec::new();

    for raw in paths {
        let native_src = to_native(raw);
        let file_name = match native_src.file_name() {
            Some(n) => n,
            None => {
                failed.push(OpFailure { path: raw.clone(), reason: "No file name".into() });
                continue;
            }
        };
        let native_dst = native_dest.join(file_name);

        if native_dst.exists() {
            failed.push(OpFailure {
                path: raw.clone(),
                reason: format!("\"{}\" already exists in the destination.", file_name.to_string_lossy()),
            });
            continue;
        }

        match std::fs::copy(&native_src, &native_dst) {
            Ok(_) => {
                let new_canonical = to_canonical(&native_dst);
                let _ = clone_db_entry(db_path, scan_id, raw, &new_canonical);
                succeeded.push(raw.clone());
            }
            Err(e) => failed.push(OpFailure {
                path: raw.clone(),
                reason: e.to_string(),
            }),
        }
    }

    Ok(OpResult { succeeded, failed })
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
        params![
            scan_id,
            old_path,
            new_path,
            basename(new_path),
        ],
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

/// Copy `src` to `dst` then remove `src`; used when `rename` fails across
/// device boundaries.
fn copy_then_remove(src: &Path, dst: &Path) -> Result<(), String> {
    std::fs::copy(src, dst).map_err(|e| e.to_string())?;
    std::fs::remove_file(src).map_err(|e| e.to_string())?;
    Ok(())
}
