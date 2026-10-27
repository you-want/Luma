// Smart file organizer — plan and execute batch moves based on a rule.
//
// Two-phase design:
//   1. `plan_organize` does a dry-run and returns the full move list so the
//      frontend can show a diff before anything touches the filesystem.
//   2. `execute_organize_plan` carries out the approved plan, emitting a
//      Tauri event after each file so the UI can show a live progress bar.
//
// Rules operate entirely on data already in the scan index (category,
// modified_at, extension). Planning checks destination metadata for conflicts
// but never reads file contents or creates directories.

use crate::{database, error::AppError, file_ops, file_ops::move_path, models::FileEntry};
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

// ── Rule ──────────────────────────────────────────────────────────────────────

/// The algorithm used to compute the destination subfolder for each file.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum OrganizeRule {
    #[serde(rename = "byPurpose")]
    Purpose,
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
            Self::Purpose => "桌面与下载整理",
            Self::Category => "按文件类型",
            Self::Year => "按年份",
            Self::YearMonth => "按年月",
            Self::Extension => "按扩展名",
        }
    }

    /// Compute the destination subfolder name for a single file.
    fn subfolder_for(&self, file: &FileEntry) -> String {
        match self {
            Self::Purpose => purpose_folder(file).to_owned(),
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

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct OrganizeOptions {
    pub recursive: bool,
    pub conflict_strategy: ConflictStrategy,
    pub language: String,
    pub folder_names: BTreeMap<String, String>,
    pub file_folders: BTreeMap<String, String>,
    pub keyword_rules: Vec<KeywordRule>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ConflictStrategy {
    #[default]
    Skip,
    Rename,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KeywordRule {
    pub keyword: String,
    pub folder: String,
}

fn purpose_folder(file: &FileEntry) -> &str {
    let name = file.name.to_lowercase();
    if file.category == "images"
        && [
            "screenshot",
            "screen shot",
            "screen capture",
            "截图",
            "截屏",
            "屏幕快照",
            "屏幕截图",
        ]
        .iter()
        .any(|word| name.contains(word))
    {
        return "截图";
    }
    match file
        .extension
        .as_deref()
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "xls" | "xlsx" | "xlsm" | "ods" | "csv" | "tsv" | "numbers" => "表格",
        "ppt" | "pptx" | "odp" | "key" => "演示文稿",
        "dmg" | "pkg" | "exe" | "msi" | "msix" | "appinstaller" => "安装包",
        _ => category_display(&file.category),
    }
}

fn localized_folder(folder: &str, language: &str) -> String {
    if !language.starts_with("en") {
        return folder.to_owned();
    }
    match folder {
        "截图" => "Screenshots",
        "表格" => "Spreadsheets",
        "演示文稿" => "Presentations",
        "安装包" => "Installers",
        "图片" => "Images",
        "视频" => "Videos",
        "音频" => "Audio",
        "文档" => "Documents",
        "代码" => "Code",
        "压缩包" => "Archives",
        "应用与安装包" => "Applications and installers",
        "其他" => "Other",
        _ => folder,
    }
    .to_owned()
}

// A single portable directory name, never a path or a Windows device name.
fn validate_folder_name(name: &str) -> Result<(), AppError> {
    let device = name.split('.').next().unwrap_or("").to_ascii_uppercase();
    if name.trim().is_empty()
        || name == "."
        || name == ".."
        || name.ends_with([' ', '.'])
        || name.len() > 200
        || name
            .chars()
            .any(|c| c.is_control() || "\\/<>:\"|?*".contains(c))
        || matches!(device.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (device.len() == 4
            && (device.starts_with("COM") || device.starts_with("LPT"))
            && matches!(device.as_bytes()[3], b'1'..=b'9'))
    {
        return Err(AppError::new(
            "INVALID_ORGANIZE_FOLDER",
            "分类名称必须是有效的单层文件夹名称。",
        ));
    }
    Ok(())
}

fn path_key(path: &str) -> String {
    // Conservative on case-sensitive disks too: portable plans never rely on
    // two destinations differing only in case.
    path.to_lowercase()
}

fn occupied(path: &str, reserved: &HashSet<String>) -> bool {
    reserved.contains(&path_key(path)) || std::fs::symlink_metadata(to_native(path)).is_ok()
}

fn numbered_path(dir: &str, name: &str, reserved: &HashSet<String>) -> String {
    let path = Path::new(name);
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or(name);
    let ext = path.extension().and_then(|s| s.to_str());
    let mut n = 2u64;
    loop {
        let suffix = ext.map(|e| format!(".{e}")).unwrap_or_default();
        let candidate = format!("{dir}/{stem} ({n}){suffix}");
        if !occupied(&candidate, reserved) {
            return candidate;
        }
        n += 1;
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
    pub group_key: String,
    /// The matching keyword, or None for metadata-based classification.
    pub matched_keyword: Option<String>,
    /// Canonical (slash-separated) source path from the scan index.
    pub from: String,
    pub from_name: String,
    /// Full canonical destination path.
    pub to: String,
    /// Destination subfolder that will be created (relative to `dest_dir`).
    pub subfolder: String,
    /// True if the target is occupied on disk or reserved by another move.
    pub conflict: bool,
    pub renamed: bool,
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
    pub preserved_files: u64,
}

/// Dry-run: compute a plan from the index and destination metadata without
/// creating directories or reading file contents. Existing subfolders are
/// preserved unless recursive organization was explicitly requested.
pub fn plan_organize(
    db_path: &Path,
    scan_id: &str,
    source_dir: &str,
    dest_dir: &str,
    rule: &OrganizeRule,
    options: &OrganizeOptions,
) -> Result<OrganizePlan, AppError> {
    #[cfg(windows)]
    let source_normalized = source_dir.replace('\\', "/");
    #[cfg(windows)]
    let dest_normalized = dest_dir.replace('\\', "/");
    #[cfg(windows)]
    let (source_dir, dest_dir) = (source_normalized.as_str(), dest_normalized.as_str());
    let source_dir = source_dir.trim_end_matches('/');
    let dest_dir = dest_dir.trim_end_matches('/');
    let native_source = file_ops::validate_write_path(source_dir)?;
    let native_dest = file_ops::validate_write_path(dest_dir)?;
    if !native_source.is_dir() || (native_dest.exists() && !native_dest.is_dir()) {
        return Err(AppError::invalid_path("请选择有效的源目录和目标目录。"));
    }
    for folder in options
        .folder_names
        .values()
        .chain(options.file_folders.values())
    {
        validate_folder_name(folder)?;
    }
    for rule in &options.keyword_rules {
        validate_folder_name(&rule.folder)?;
        if rule.keyword.trim().is_empty() {
            return Err(AppError::new(
                "INVALID_ORGANIZE_KEYWORD",
                "关键词不能为空。",
            ));
        }
    }
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
         WHERE scan_id = ?1 AND path LIKE ?2 ESCAPE '\\' ORDER BY path COLLATE BINARY",
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
    let mut preserved_files: u64 = 0;
    let mut reserved = HashSet::new();
    let source_prefix = format!("{source_dir}/");
    let dest_prefix = format!("{dest_dir}/");

    for file in &files {
        // SQLite LIKE is case-insensitive for ASCII. Do not let a sibling
        // whose name differs only by case leak into the selected source.
        let Some(relative) = file.path.strip_prefix(&source_prefix) else {
            continue;
        };
        if file.is_hidden
            || (!options.recursive && relative.contains('/'))
            || (dest_dir != source_dir && file.path.starts_with(&dest_prefix))
        {
            preserved_files += 1;
            continue;
        }
        let matched = options.keyword_rules.iter().find(|r| {
            file.name
                .to_lowercase()
                .contains(&r.keyword.trim().to_lowercase())
        });
        let group_key = matched
            .map(|r| format!("keyword:{}", r.keyword))
            .unwrap_or_else(|| rule.subfolder_for(file));
        let default_folder = matched
            .map(|r| r.folder.clone())
            .unwrap_or_else(|| localized_folder(&group_key, &options.language));
        let subfolder = options
            .file_folders
            .get(&file.path)
            .or_else(|| options.folder_names.get(&group_key))
            .cloned()
            .unwrap_or(default_folder);
        validate_folder_name(&subfolder)?;
        let target_dir = format!("{dest_dir}/{subfolder}");
        if to_native(&target_dir).exists() && !to_native(&target_dir).is_dir() {
            return Err(AppError::new(
                "INVALID_ORGANIZE_FOLDER",
                "分类名称已被普通文件占用，请更换名称。",
            ));
        }
        let mut dest_path = format!("{target_dir}/{}", file.name);
        file_ops::validate_write_path(&dest_path)?;

        // Skip files already sitting in their target location
        if file.path == dest_path {
            already_placed += 1;
            continue;
        }

        let mut conflict = occupied(&dest_path, &reserved);
        let mut renamed = false;
        if conflict && matches!(options.conflict_strategy, ConflictStrategy::Rename) {
            dest_path = numbered_path(&target_dir, &file.name, &reserved);
            conflict = false;
            renamed = true;
        }
        if conflict {
            conflict_count += 1;
        }
        reserved.insert(path_key(&dest_path));

        moves.push(OrganizeMove {
            group_key,
            matched_keyword: matched.map(|r| r.keyword.clone()),
            from: file.path.clone(),
            from_name: file.name.clone(),
            to: dest_path,
            subfolder,
            conflict,
            renamed,
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
        preserved_files,
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
        if std::fs::symlink_metadata(&native_dst).is_ok() {
            failed.push(crate::file_ops::OpFailure {
                path: m.from.clone(),
                reason: format!("目标路径已存在：{}", m.to),
            });
            done += 1;
            continue;
        }

        // Move without replacing an existing target, including a target that
        // appeared after the check above.
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
                    if move_path(&native_dst, &native_src).is_ok() {
                        let _ = update_db_path(db_path, scan_id, &m.to, &m.from);
                    }
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
    let metadata = std::fs::metadata(to_native(new_path))
        .map_err(|error| AppError::new("FILE_READ_ERROR", error.to_string()))?;
    let modified = metadata
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .and_then(|d| i64::try_from(d.as_secs()).ok());
    let new_name = new_path.rsplit('/').next().unwrap_or(new_path);
    conn.execute(
        "UPDATE files SET path = ?3, name = ?4, modified_at = ?5 WHERE scan_id = ?1 AND path = ?2",
        params![scan_id, old_path, new_path, new_name, modified],
    )?;
    Ok(())
}

// ── Path utilities ─────────────────────────────────────────────────────────────

fn to_native(canonical: &str) -> PathBuf {
    PathBuf::from(canonical.replace('/', std::path::MAIN_SEPARATOR_STR))
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

    struct Fixture {
        root: PathBuf,
        db: PathBuf,
        source: PathBuf,
        dest: PathBuf,
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    impl Fixture {
        fn new(files: &[&str]) -> Self {
            let root = std::env::current_dir()
                .unwrap()
                .join("target")
                .join(format!("luma-organize-test-{}", Uuid::new_v4()));
            let source = root.join("source");
            let dest = root.join("destination");
            fs::create_dir_all(&source).unwrap();
            fs::create_dir_all(&dest).unwrap();
            let db = root.join("test.sqlite3");
            database::initialize(&db).unwrap();
            database::create_scan_run(&db, "test", &canonical(&source), 1).unwrap();
            let mut connection = database::open(&db).unwrap();
            let entries: Vec<_> = files
                .iter()
                .map(|name| {
                    let path = source.join(name);
                    fs::create_dir_all(path.parent().unwrap()).unwrap();
                    fs::write(&path, name.as_bytes()).unwrap();
                    let metadata = fs::metadata(&path).unwrap();
                    let extension = path.extension().map(|e| e.to_string_lossy().to_lowercase());
                    FileEntry {
                        id: 0,
                        path: canonical(&path),
                        name: path.file_name().unwrap().to_string_lossy().into_owned(),
                        category: crate::scanner::classify_extension(extension.as_deref()).into(),
                        extension,
                        size_bytes: metadata.len(),
                        modified_at: metadata
                            .modified()
                            .ok()
                            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                            .map(|d| d.as_secs() as i64),
                        is_hidden: path.file_name().unwrap().to_string_lossy().starts_with('.'),
                        content_hash: None,
                    }
                })
                .collect();
            database::insert_file_batch(&mut connection, "test", &entries).unwrap();
            Self {
                root,
                db,
                source,
                dest,
            }
        }

        fn plan(&self, options: &super::OrganizeOptions) -> super::OrganizePlan {
            plan_organize(
                &self.db,
                "test",
                &canonical(&self.source),
                &canonical(&self.dest),
                &OrganizeRule::Purpose,
                options,
            )
            .unwrap()
        }

        fn execute(&self, moves: &[super::OrganizeMove]) -> super::OrganizeResult {
            let inputs: Vec<_> = moves
                .iter()
                .filter(|m| !m.conflict)
                .map(|m| OrganizeMoveInput {
                    from: m.from.clone(),
                    to: m.to.clone(),
                    expected_size_bytes: m.expected_size_bytes,
                    expected_modified_at: m.expected_modified_at,
                })
                .collect();
            execute_organize_plan(&self.db, "test", &inputs, |_| {}).unwrap()
        }
    }

    #[test]
    fn default_plan_preserves_existing_folders_and_hidden_files_without_writing() {
        let f = Fixture::new(&["report.pdf", "project/budget.xlsx", ".secret.txt"]);
        let plan = f.plan(&super::OrganizeOptions::default());
        assert_eq!(plan.moves.len(), 1);
        assert_eq!(plan.moves[0].from_name, "report.pdf");
        assert_eq!(plan.preserved_files, 2);
        assert_eq!(fs::read_dir(&f.dest).unwrap().count(), 0);
        assert!(f.source.join("project/budget.xlsx").exists());
        let recursive = f.plan(&super::OrganizeOptions {
            recursive: true,
            ..Default::default()
        });
        assert_eq!(recursive.moves.len(), 2);
        assert_eq!(recursive.preserved_files, 1);
    }

    #[test]
    fn purpose_groups_are_detailed_and_follow_the_selected_language() {
        let f = Fixture::new(&[
            "Screenshot 2026.png",
            "预算.xlsx",
            "slides.pptx",
            "Luma.dmg",
            "report.pdf",
        ]);
        let plan = f.plan(&super::OrganizeOptions {
            language: "en-US".into(),
            ..Default::default()
        });
        let folders: std::collections::HashSet<_> =
            plan.moves.iter().map(|m| m.subfolder.as_str()).collect();
        assert_eq!(
            folders,
            [
                "Screenshots",
                "Spreadsheets",
                "Presentations",
                "Installers",
                "Documents"
            ]
            .into()
        );
    }

    #[test]
    fn keywords_take_priority_and_custom_file_destinations_override_group_names() {
        let f = Fixture::new(&["CLIENT-a.pdf", "budget.xlsx"]);
        let source = canonical(&f.source.join("budget.xlsx"));
        let options = super::OrganizeOptions {
            language: "en-US".into(),
            keyword_rules: vec![
                super::KeywordRule {
                    keyword: "client".into(),
                    folder: "客户资料".into(),
                },
                super::KeywordRule {
                    keyword: "CLIENT-a".into(),
                    folder: "Second".into(),
                },
            ],
            folder_names: [("表格".into(), "财务".into())].into(),
            file_folders: [(source, "待审核".into())].into(),
            ..Default::default()
        };
        let plan = f.plan(&options);
        assert_eq!(plan.moves[0].subfolder, "客户资料");
        assert_eq!(plan.moves[0].matched_keyword.as_deref(), Some("client"));
        assert_eq!(plan.moves[1].subfolder, "待审核");
    }

    #[test]
    fn preview_detects_existing_and_batch_conflicts_and_renames_deterministically() {
        let f = Fixture::new(&["a/report.pdf", "b/report.pdf", "c/REPORT.PDF"]);
        fs::create_dir_all(f.dest.join("文档")).unwrap();
        fs::write(f.dest.join("文档/report.pdf"), b"existing").unwrap();
        let options = super::OrganizeOptions {
            recursive: true,
            ..Default::default()
        };
        assert_eq!(f.plan(&options).conflict_count, 3);
        let options = super::OrganizeOptions {
            conflict_strategy: super::ConflictStrategy::Rename,
            ..options
        };
        let plan = f.plan(&options);
        assert_eq!(plan.conflict_count, 0);
        assert!(plan.moves.iter().all(|m| m.renamed));
        assert!(plan.moves[0].to.ends_with("report (2).pdf"));
        assert!(plan.moves[1].to.ends_with("report (3).pdf"));
        assert!(plan.moves[2].to.ends_with("REPORT (4).PDF"));
        assert_eq!(
            plan.moves.iter().map(|m| &m.to).collect::<Vec<_>>(),
            f.plan(&options)
                .moves
                .iter()
                .map(|m| &m.to)
                .collect::<Vec<_>>()
        );
        let outcome = f.execute(&plan.moves);
        assert_eq!(outcome.succeeded, 3);
        assert_eq!(
            fs::read(f.dest.join("文档/report.pdf")).unwrap(),
            b"existing"
        );
    }

    #[test]
    fn batch_conflict_is_visible_even_when_destination_does_not_exist() {
        let f = Fixture::new(&["a/report.pdf", "b/report.pdf"]);
        let plan = f.plan(&super::OrganizeOptions {
            recursive: true,
            ..Default::default()
        });
        assert_eq!(plan.conflict_count, 1);
        assert!(!plan.moves[0].conflict);
        assert!(plan.moves[1].conflict);
    }

    #[test]
    fn selected_subset_is_moved_and_persisted_undo_restores_it() {
        let f = Fixture::new(&["keep.pdf", "move.xlsx", "project/config.json"]);
        let plan = f.plan(&super::OrganizeOptions::default());
        let outcome = f.execute(&plan.moves[1..]);
        assert_eq!(outcome.succeeded, 1);
        assert!(f.source.join("keep.pdf").exists());
        assert!(f.source.join("project/config.json").exists());
        assert!(!f.source.join("move.xlsx").exists());
        let operations = crate::file_ops::list_undoable_operations(&f.db, "test", 20).unwrap();
        assert_eq!(operations.len(), 1);
        let undo = crate::file_ops::undo_operation(&f.db, &operations[0].id, "test").unwrap();
        assert!(undo.failed.is_empty());
        assert_eq!(fs::read(f.source.join("move.xlsx")).unwrap(), b"move.xlsx");
    }

    #[test]
    fn repeated_in_place_organization_leaves_organized_files_alone() {
        let f = Fixture::new(&["report.pdf", "notes.txt"]);
        let options = super::OrganizeOptions::default();
        let plan = plan_organize(
            &f.db,
            "test",
            &canonical(&f.source),
            &canonical(&f.source),
            &OrganizeRule::Purpose,
            &options,
        )
        .unwrap();
        assert_eq!(f.execute(&plan.moves).succeeded, 2);
        let repeated = plan_organize(
            &f.db,
            "test",
            &canonical(&f.source),
            &canonical(&f.source),
            &OrganizeRule::Purpose,
            &options,
        )
        .unwrap();
        assert!(repeated.moves.is_empty());
        let recursive = super::OrganizeOptions {
            recursive: true,
            ..options
        };
        let repeated = plan_organize(
            &f.db,
            "test",
            &canonical(&f.source),
            &canonical(&f.source),
            &OrganizeRule::Purpose,
            &recursive,
        )
        .unwrap();
        assert!(repeated.moves.is_empty());
        assert_eq!(repeated.already_placed, 2);
    }

    #[test]
    fn custom_folders_cannot_escape_destination_or_use_reserved_names() {
        let f = Fixture::new(&["report.pdf"]);
        for invalid in [
            "../escape",
            "nested/folder",
            "",
            ".",
            "..",
            "NUL.txt",
            "COM1",
            "trailing.",
            "bad\\name",
        ] {
            let options = super::OrganizeOptions {
                folder_names: [("文档".into(), invalid.into())].into(),
                ..Default::default()
            };
            let error = plan_organize(
                &f.db,
                "test",
                &canonical(&f.source),
                &canonical(&f.dest),
                &OrganizeRule::Purpose,
                &options,
            )
            .unwrap_err();
            assert_eq!(error.code, "INVALID_ORGANIZE_FOLDER");
        }
        assert!(f.source.join("report.pdf").exists());
    }

    #[test]
    fn file_created_after_preview_is_not_overwritten_and_safe_move_is_atomic() {
        let f = Fixture::new(&["report.pdf"]);
        let plan = f.plan(&super::OrganizeOptions::default());
        let target = super::to_native(&plan.moves[0].to);
        fs::create_dir_all(target.parent().unwrap()).unwrap();
        fs::write(&target, b"do not overwrite").unwrap();
        assert_eq!(f.execute(&plan.moves).succeeded, 0);
        assert!(super::move_path(&f.source.join("report.pdf"), &target).is_err());
        assert_eq!(fs::read(&target).unwrap(), b"do not overwrite");
        assert_eq!(
            fs::read(f.source.join("report.pdf")).unwrap(),
            b"report.pdf"
        );
    }

    #[test]
    fn failed_log_write_rolls_back_the_file_and_its_index() {
        let f = Fixture::new(&["report.pdf"]);
        let plan = f.plan(&super::OrganizeOptions::default());
        database::open(&f.db)
            .unwrap()
            .execute_batch(
                "CREATE TRIGGER fail_organize_log BEFORE UPDATE OF undo_json ON file_operations
             BEGIN SELECT RAISE(ABORT, 'simulated log failure'); END;",
            )
            .unwrap();
        let outcome = f.execute(&plan.moves);
        assert_eq!(outcome.succeeded, 0);
        assert_eq!(outcome.failed.len(), 1);
        assert_eq!(
            fs::read(f.source.join("report.pdf")).unwrap(),
            b"report.pdf"
        );
        assert!(!super::to_native(&plan.moves[0].to).exists());
        crate::file_ops::validate_indexed_file(&f.db, "test", &plan.moves[0].from)
            .expect("rolled-back file must still match its index");
    }

    #[test]
    fn undo_preserves_a_new_file_at_the_original_location() {
        let f = Fixture::new(&["report.pdf"]);
        let plan = f.plan(&super::OrganizeOptions::default());
        let outcome = f.execute(&plan.moves);
        fs::write(f.source.join("report.pdf"), b"new file").unwrap();
        let result =
            crate::file_ops::undo_operation(&f.db, &outcome.operation_id.unwrap(), "test").unwrap();
        assert!(result.succeeded.is_empty());
        assert_eq!(result.failed.len(), 1);
        assert_eq!(fs::read(f.source.join("report.pdf")).unwrap(), b"new file");
        assert_eq!(
            fs::read(super::to_native(&plan.moves[0].to)).unwrap(),
            b"report.pdf"
        );
    }

    #[test]
    fn nested_destination_files_are_not_reorganized() {
        let f = Fixture::new(&["report.pdf", "organized/keep.xlsx"]);
        let plan = plan_organize(
            &f.db,
            "test",
            &canonical(&f.source),
            &canonical(&f.source.join("organized")),
            &OrganizeRule::Purpose,
            &super::OrganizeOptions {
                recursive: true,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(plan.moves.len(), 1);
        assert_eq!(plan.preserved_files, 1);
    }

    #[test]
    fn case_insensitive_sql_match_does_not_include_a_different_source_prefix() {
        let f = Fixture::new(&["report.pdf"]);
        let connection = database::open(&f.db).unwrap();
        connection.execute(
            "INSERT INTO files (scan_id, path, name, extension, category, size_bytes, modified_at, is_hidden)
             SELECT scan_id, ?1, 'sibling.pdf', extension, category, size_bytes, modified_at, is_hidden
             FROM files WHERE scan_id = 'test' LIMIT 1",
            [canonical(&f.source).replace("/source", "/SOURCE") + "/sibling.pdf"],
        ).unwrap();
        let plan = f.plan(&super::OrganizeOptions::default());
        assert_eq!(plan.moves.len(), 1);
        assert_eq!(plan.moves[0].from_name, "report.pdf");
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
            &super::OrganizeOptions::default(),
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
