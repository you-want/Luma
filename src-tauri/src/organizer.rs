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

use crate::{database, error::AppError, file_ops, models::FileEntry};
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

// ── Rule ──────────────────────────────────────────────────────────────────────

/// The algorithm used to compute the destination subfolder for each file.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum OrganizeRule {
    /// Group by Luma category  →  `Images/`, `Videos/`, `Documents/`, …
    #[serde(rename = "byCategory")]
    Category,
    /// Group by modification year  →  `2024/`, `2025/`, `Unknown/`
    #[serde(rename = "byYear")]
    Year,
    /// Group by modification year-month  →  `2024-03/`, `2025-01/`, …
    #[serde(rename = "byYearMonth")]
    YearMonth,
    /// Group by lowercase extension  →  `pdf/`, `jpg/`, `no-extension/`
    #[serde(rename = "byExtension")]
    Extension,
}

/// Human-readable display name for a rule, used in plan metadata.
impl OrganizeRule {
    pub fn display_name(&self) -> &'static str {
        match self {
            Self::Category => "按文件类型",
            Self::Year => "按年份",
            Self::YearMonth => "按年月",
            Self::Extension => "按扩展名",
        }
    }

    /// Compute the destination subfolder name for a single file.
    fn subfolder_for(&self, file: &FileEntry) -> String {
        match self {
            Self::Category => category_display(&file.category).to_owned(),

            Self::Year => file
                .modified_at
                .and_then(|ts| {
                    let dt = chrono_year_month(ts);
                    dt.map(|(y, _)| y.to_string())
                })
                .unwrap_or_else(|| "Unknown".to_owned()),

            Self::YearMonth => file
                .modified_at
                .and_then(|ts| chrono_year_month(ts).map(|(y, m)| format!("{y}-{m:02}")))
                .unwrap_or_else(|| "Unknown".to_owned()),

            Self::Extension => file
                .extension
                .as_deref()
                .map(|e| e.to_ascii_lowercase())
                .unwrap_or_else(|| "no-extension".to_owned()),
        }
    }
}

fn category_display(category: &str) -> &str {
    match category {
        "images" => "图片",
        "videos" => "视频",
        "audio" => "音频",
        "documents" => "文档",
        "code" => "代码",
        "archives" => "压缩包",
        "applications" => "应用与安装包",
        _ => "其他",
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
    /// Snapshot values used to reject stale plans at execution time.
    pub expected_size_bytes: u64,
    pub expected_modified_at: Option<i64>,
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
    let native_source = file_ops::validate_write_path(source_dir)?;
    file_ops::validate_write_path(dest_dir)?;
    let connection = database::open(db_path)?;
    let root_path = connection
        .query_row(
            "SELECT root_path FROM scan_runs WHERE id = ?1",
            [scan_id],
            |row| row.get::<_, String>(0),
        )
        .optional()?
        .ok_or_else(|| AppError::new("SCAN_NOT_FOUND", "找不到当前扫描记录。"))?;
    let native_root = PathBuf::from(root_path.replace('/', std::path::MAIN_SEPARATOR_STR));
    if native_source != native_root && !native_source.starts_with(&native_root) {
        return Err(AppError::new(
            "OUTSIDE_SCAN_ROOT",
            "整理源目录必须位于当前扫描目录内。",
        ));
    }

    let glob = format!("{}/%", escape_like(source_dir));
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

        let native_dest = PathBuf::from(dest_path.replace('/', std::path::MAIN_SEPARATOR_STR));
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
            expected_size_bytes: file.size_bytes,
            expected_modified_at: file.modified_at,
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
    pub operation_id: Option<String>,
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
    let operation_id = file_ops::begin_operation(
        db_path,
        scan_id,
        "organize",
        &format!("规则整理（{} 项）", moves.len()),
        Some(file_ops::UndoKind::Move),
    )?;
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
        let native_dst = match file_ops::validate_write_path(&m.to) {
            Ok(path) => path,
            Err(error) => {
                failed.push(crate::file_ops::OpFailure {
                    path: m.from.clone(),
                    reason: error.message,
                });
                done += 1;
                continue;
            }
        };

        let indexed = match file_ops::validate_indexed_file(db_path, scan_id, &m.from) {
            Ok(snapshot) => snapshot,
            Err(error) => {
                failed.push(crate::file_ops::OpFailure {
                    path: m.from.clone(),
                    reason: error.message,
                });
                done += 1;
                continue;
            }
        };
        if indexed.size_bytes != m.expected_size_bytes
            || indexed.modified_at != m.expected_modified_at
        {
            failed.push(crate::file_ops::OpFailure {
                path: m.from.clone(),
                reason: "整理计划已过期，请重新生成预览。".to_owned(),
            });
            done += 1;
            continue;
        }

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

        // Move: try rename first; fall back to rollback-safe copy+remove.
        if let Err(reason) = move_path(&native_src, &native_dst) {
            failed.push(crate::file_ops::OpFailure {
                path: m.from.clone(),
                reason,
            });
        } else {
            // Update the DB path
            if let Err(error) = update_db_path(db_path, scan_id, &m.from, &m.to) {
                let _ = move_path(&native_dst, &native_src);
                failed.push(crate::file_ops::OpFailure {
                    path: m.from.clone(),
                    reason: error.message,
                });
            } else {
                if let Err(error) =
                    file_ops::append_operation_move(db_path, &operation_id, &m.from, &m.to)
                {
                    let _ = update_db_path(db_path, scan_id, &m.to, &m.from);
                    let _ = move_path(&native_dst, &native_src);
                    failed.push(crate::file_ops::OpFailure {
                        path: m.from.clone(),
                        reason: error.message,
                    });
                } else {
                    succeeded += 1;
                }
            }
        }

        done += 1;
    }

    // Emit final progress
    on_progress(OrganizeProgress {
        done,
        total,
        current_from: String::new(),
    });

    file_ops::finish_operation(db_path, &operation_id, succeeded as usize, &failed)?;
    Ok(OrganizeResult {
        operation_id: Some(operation_id),
        succeeded,
        failed,
    })
}

// ── Input type (frontend sends back a subset of OrganizeMove) ────────────────

/// Moves sent by the frontend to execute; excludes computed fields.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OrganizeMoveInput {
    pub from: String,
    pub to: String,
    pub expected_size_bytes: u64,
    pub expected_modified_at: Option<i64>,
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

fn move_path(src: &Path, dst: &Path) -> Result<(), String> {
    match std::fs::rename(src, dst) {
        Ok(()) => Ok(()),
        Err(rename_error) => {
            std::fs::copy(src, dst)
                .map_err(|copy_error| format!("{rename_error} / {copy_error}"))?;
            if let Err(remove_error) = std::fs::remove_file(src) {
                let cleanup_error = std::fs::remove_file(dst).err();
                return Err(match cleanup_error {
                    Some(cleanup) => format!(
                        "{rename_error} / 删除源文件失败：{remove_error}；回滚副本失败：{cleanup}"
                    ),
                    None => {
                        format!("{rename_error} / 删除源文件失败，已移除目标副本：{remove_error}")
                    }
                });
            }
            Ok(())
        }
    }
}

fn escape_like(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        if matches!(character, '\\' | '%' | '_') {
            escaped.push('\\');
        }
        escaped.push(character);
    }
    escaped
}

#[cfg(test)]
mod tests {
    use super::{execute_organize_plan, plan_organize, OrganizeMoveInput, OrganizeRule};
    use crate::{database, models::FileEntry};
    use std::{
        fs,
        path::{Path, PathBuf},
        time::UNIX_EPOCH,
    };
    use uuid::Uuid;

    fn canonical(path: &Path) -> String {
        path.to_string_lossy()
            .replace(std::path::MAIN_SEPARATOR, "/")
    }

    #[test]
    fn execution_rejects_a_file_changed_after_preview() {
        let root = std::env::current_dir()
            .expect("current directory")
            .join("target")
            .join(format!("luma-organizer-{}", Uuid::new_v4()));
        let source_dir = root.join("source");
        let destination_dir = root.join("organized");
        fs::create_dir_all(&source_dir).expect("create source directory");
        fs::create_dir_all(&destination_dir).expect("create destination directory");
        let source_path = source_dir.join("report.txt");
        fs::write(&source_path, b"initial").expect("write source file");

        let database_path = root.join("luma.sqlite3");
        database::initialize(&database_path).expect("initialize database");
        database::create_scan_run(&database_path, "scan-1", &canonical(&root), 1)
            .expect("create scan");
        let metadata = fs::metadata(&source_path).expect("source metadata");
        let modified_at = metadata
            .modified()
            .ok()
            .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
            .and_then(|duration| i64::try_from(duration.as_secs()).ok());
        let mut connection = database::open(&database_path).expect("open database");
        database::insert_file_batch(
            &mut connection,
            "scan-1",
            &[FileEntry {
                id: 0,
                path: canonical(&source_path),
                name: "report.txt".to_owned(),
                extension: Some("txt".to_owned()),
                category: "documents".to_owned(),
                size_bytes: metadata.len(),
                modified_at,
                is_hidden: false,
                content_hash: None,
            }],
        )
        .expect("insert source file");
        drop(connection);

        let plan = plan_organize(
            &database_path,
            "scan-1",
            &canonical(&source_dir),
            &canonical(&destination_dir),
            &OrganizeRule::Category,
        )
        .expect("build organize plan");
        assert_eq!(plan.moves.len(), 1);

        fs::write(&source_path, b"changed after preview").expect("change source file");
        let movement = &plan.moves[0];
        let result = execute_organize_plan(
            &database_path,
            "scan-1",
            &[OrganizeMoveInput {
                from: movement.from.clone(),
                to: movement.to.clone(),
                expected_size_bytes: movement.expected_size_bytes,
                expected_modified_at: movement.expected_modified_at,
            }],
            |_| {},
        )
        .expect("execute stale plan");

        assert_eq!(result.succeeded, 0);
        assert_eq!(result.failed.len(), 1);
        assert!(result.failed[0].reason.contains("重新扫描"));
        assert!(source_path.exists());
        assert!(!PathBuf::from(&movement.to).exists());
        fs::remove_dir_all(root).expect("remove fixture");
    }
}
