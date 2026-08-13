// Smart file organizer — plan and execute batch moves based on a rule.
//
// Two-phase design:
//   1. `plan_organize` does a dry-run and returns the full move list so the
//      frontend can show a diff before anything touches the filesystem.
//   2. `execute_organize_plan` carries out the approved plan, emitting a
//      Tauri event after each file so the UI can show a live progress bar.
//
// Rules operate entirely on data already in the scan index (category,
// modified_at, extension) — no extra filesystem reads during planning.

use crate::{database, error::AppError, models::FileEntry};
use rusqlite::params;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

// ── Rule ──────────────────────────────────────────────────────────────────────

/// The algorithm used to compute the destination subfolder for each file.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum OrganizeRule {
    /// Group by Luma category  →  `Images/`, `Videos/`, `Documents/`, …
    ByCategory,
    /// Group by modification year  →  `2024/`, `2025/`, `Unknown/`
    ByYear,
    /// Group by modification year-month  →  `2024-03/`, `2025-01/`, …
    ByYearMonth,
    /// Group by lowercase extension  →  `pdf/`, `jpg/`, `no-extension/`
    ByExtension,
}

/// Human-readable display name for a rule, used in plan metadata.
impl OrganizeRule {
    pub fn display_name(&self) -> &'static str {
        match self {
            Self::ByCategory  => "按文件类型",
            Self::ByYear      => "按年份",
            Self::ByYearMonth => "按年月",
            Self::ByExtension => "按扩展名",
        }
    }

    /// Compute the destination subfolder name for a single file.
    fn subfolder_for(&self, file: &FileEntry) -> String {
        match self {
            Self::ByCategory => category_display(&file.category).to_owned(),

            Self::ByYear => file
                .modified_at
                .and_then(|ts| {
                    let dt = chrono_year_month(ts);
                    dt.map(|(y, _)| y.to_string())
                })
                .unwrap_or_else(|| "Unknown".to_owned()),

            Self::ByYearMonth => file
                .modified_at
                .and_then(|ts| {
                    chrono_year_month(ts).map(|(y, m)| format!("{y}-{m:02}"))
                })
                .unwrap_or_else(|| "Unknown".to_owned()),

            Self::ByExtension => file
                .extension
                .as_deref()
                .map(|e| e.to_ascii_lowercase())
                .unwrap_or_else(|| "no-extension".to_owned()),
        }
    }
}

fn category_display(category: &str) -> &str {
    match category {
        "images"       => "图片",
        "videos"       => "视频",
        "audio"        => "音频",
        "documents"    => "文档",
        "code"         => "代码",
        "archives"     => "压缩包",
        "applications" => "应用与安装包",
        _              => "其他",
    }
}

/// Decompose a Unix timestamp into (year, month) without pulling in chrono.
fn chrono_year_month(ts: i64) -> Option<(i32, u32)> {
    if ts < 0 {
        return None;
    }
    // Days since Unix epoch
    let days = ts / 86400;
    // Gregorian algorithm (works for 1970–2099)
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if m <= 2 { y + 1 } else { y };
    Some((year as i32, m as u32))
}

// ── Plan ──────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OrganizeMove {
    /// Canonical (slash-separated) source path from the scan index.
    pub from: String,
    pub from_name: String,
    /// Full canonical destination path.
    pub to: String,
    /// Destination subfolder that will be created (relative to `dest_dir`).
    pub subfolder: String,
    /// True if the destination file already exists on disk.
    pub conflict: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OrganizePlan {
    pub rule_name: String,
    pub source_dir: String,
    pub dest_dir: String,
    pub moves: Vec<OrganizeMove>,
    pub conflict_count: u64,
    /// Files skipped because they are already in their target subfolder.
    pub already_placed: u64,
}

/// Dry-run: compute the full move plan without touching the filesystem.
/// Queries all files recursively under `source_dir` from the scan index.
pub fn plan_organize(
    db_path: &Path,
    scan_id: &str,
    source_dir: &str,
    dest_dir: &str,
    rule: &OrganizeRule,
) -> Result<OrganizePlan, AppError> {
    let connection = database::open(db_path)?;

    let glob = format!("{source_dir}/%");
    let mut stmt = connection.prepare(
        "SELECT id, path, name, extension, category, size_bytes, modified_at, is_hidden, content_hash
         FROM files
         WHERE scan_id = ?1 AND path LIKE ?2 ESCAPE '\\'",
    )?;
    let files: Vec<FileEntry> = stmt
        .query_map(params![scan_id, glob], |row| {
            Ok(FileEntry {
                id: row.get(0)?,
                path: row.get(1)?,
                name: row.get(2)?,
                extension: row.get(3)?,
                category: row.get(4)?,
                size_bytes: crate::database::row_u64(row, 5),
                modified_at: row.get(6)?,
                is_hidden: row.get::<_, i64>(7)? != 0,
                content_hash: row.get(8)?,
            })
        })?
        .collect::<Result<_, _>>()?;

    let mut moves = Vec::with_capacity(files.len());
    let mut conflict_count: u64 = 0;
    let mut already_placed: u64 = 0;

    for file in &files {
        let subfolder = rule.subfolder_for(file);
        let dest_path = format!("{dest_dir}/{subfolder}/{}", file.name);

        // Skip files already sitting in their target location
        if file.path == dest_path {
            already_placed += 1;
            continue;
        }

        let native_dest = PathBuf::from(
            dest_path.replace('/', std::path::MAIN_SEPARATOR_STR),
        );
        let conflict = native_dest.exists();
        if conflict {
            conflict_count += 1;
        }

        moves.push(OrganizeMove {
            from: file.path.clone(),
            from_name: file.name.clone(),
            to: dest_path,
            subfolder,
            conflict,
        });
    }

    Ok(OrganizePlan {
        rule_name: rule.display_name().to_owned(),
        source_dir: source_dir.to_owned(),
        dest_dir: dest_dir.to_owned(),
        moves,
        conflict_count,
        already_placed,
    })
}

// ── Execute ───────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OrganizeProgress {
    pub done: u64,
    pub total: u64,
    pub current_from: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OrganizeResult {
    pub succeeded: u64,
    pub failed: Vec<crate::file_ops::OpFailure>,
}

/// Execute the approved plan. `on_progress` is called after every file move.
pub fn execute_organize_plan(
    db_path: &Path,
    scan_id: &str,
    moves: &[OrganizeMoveInput],
    on_progress: impl Fn(OrganizeProgress),
) -> Result<OrganizeResult, AppError> {
    let total = moves.len() as u64;
    let mut done: u64 = 0;
    let mut succeeded: u64 = 0;
    let mut failed: Vec<crate::file_ops::OpFailure> = Vec::new();

    for m in moves {
        on_progress(OrganizeProgress {
            done,
            total,
            current_from: m.from.clone(),
        });

        let native_src = to_native(&m.from);
        let native_dst = to_native(&m.to);

        // Create destination parent directory if needed
        if let Some(parent) = native_dst.parent() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                failed.push(crate::file_ops::OpFailure {
                    path: m.from.clone(),
                    reason: e.to_string(),
                });
                done += 1;
                continue;
            }
        }

        // Skip if destination already occupied (conflict was flagged in plan)
        if native_dst.exists() {
            failed.push(crate::file_ops::OpFailure {
                path: m.from.clone(),
                reason: format!("目标路径已存在：{}", m.to),
            });
            done += 1;
            continue;
        }

        // Move: try rename first; fall back to copy+remove for cross-device
        let moved = if std::fs::rename(&native_src, &native_dst).is_ok() {
            true
        } else {
            std::fs::copy(&native_src, &native_dst)
                .and_then(|_| std::fs::remove_file(&native_src))
                .is_ok()
        };

        if moved {
            // Update the DB path
            let _ = update_db_path(db_path, scan_id, &m.from, &m.to);
            succeeded += 1;
        } else {
            failed.push(crate::file_ops::OpFailure {
                path: m.from.clone(),
                reason: "移动文件失败".to_owned(),
            });
        }

        done += 1;
    }

    // Emit final progress
    on_progress(OrganizeProgress {
        done,
        total,
        current_from: String::new(),
    });

    Ok(OrganizeResult { succeeded, failed })
}

// ── Input type (frontend sends back a subset of OrganizeMove) ────────────────

/// Moves sent by the frontend to execute; excludes computed fields.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OrganizeMoveInput {
    pub from: String,
    pub to: String,
}

// ── DB helper ─────────────────────────────────────────────────────────────────

fn update_db_path(
    db_path: &Path,
    scan_id: &str,
    old_path: &str,
    new_path: &str,
) -> Result<(), AppError> {
    let conn = database::open(db_path)?;
    let new_name = new_path.rsplit('/').next().unwrap_or(new_path);
    conn.execute(
        "UPDATE files SET path = ?3, name = ?4 WHERE scan_id = ?1 AND path = ?2",
        params![scan_id, old_path, new_path, new_name],
    )?;
    Ok(())
}

// ── Path utilities ─────────────────────────────────────────────────────────────

fn to_native(canonical: &str) -> PathBuf {
    PathBuf::from(canonical.replace('/', std::path::MAIN_SEPARATOR_STR))
}
