use crate::{database, error::AppError, models::FileEntry, scanner::classify_extension};
use rusqlite::{params, Connection};
use std::{
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};
use walkdir::WalkDir;

// ── Public types ─────────────────────────────────────────────────────────────

/// One actionable cleanup opportunity.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanupItem {
    /// Stable identifier used by the frontend to request the file list.
    /// DB-backed: "development" | "archives" | "installers" | "duplicatesEstimate"
    /// Filesystem: "trash" | "oldDownloads"
    pub kind: String,
    pub size_bytes: u64,
    pub file_count: u64,
}

/// All cleanup opportunities for one scan, sorted by wasted space descending.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanupSummary {
    pub items: Vec<CleanupItem>,
    pub total_bytes: u64,
}

// ── Entry points ──────────────────────────────────────────────────────────────

/// Build the full cleanup summary: DB items from the current scan + filesystem
/// items (Trash, old Downloads) that are always relevant regardless of which
/// directory was scanned.
pub fn build_cleanup_summary(
    db_path: &Path,
    scan_id: &str,
    old_downloads_days: u32,
) -> Result<CleanupSummary, AppError> {
    let connection = database::open(db_path)?;
    let mut items: Vec<CleanupItem> = Vec::new();

    // ── Items backed by the scan index ────────────────────────────────────────
    for (kind, sql) in DB_ITEMS {
        let (count, bytes) = aggregate(&connection, sql, scan_id)?;
        if count > 0 {
            items.push(CleanupItem {
                kind: kind.to_string(),
                size_bytes: bytes,
                file_count: count,
            });
        }
    }

    // Cheap duplicate estimate: sum (count-1)*size for size groups with ≥2 files
    // ≥1 MB. This is a fast over-estimate (same size ≠ same content) that gives
    // a useful lower-bound signal without reading any file bytes.
    let (dup_count, dup_bytes) = aggregate(
        &connection,
        "SELECT COALESCE(SUM(cnt - 1), 0), COALESCE(SUM((cnt - 1) * size_bytes), 0)
         FROM (
           SELECT size_bytes, COUNT(*) AS cnt
           FROM files
           WHERE scan_id = ?1 AND size_bytes >= 1048576
           GROUP BY size_bytes
           HAVING cnt >= 2
         )",
        scan_id,
    )?;
    if dup_count > 0 {
        items.push(CleanupItem {
            kind: "duplicatesEstimate".to_string(),
            size_bytes: dup_bytes,
            file_count: dup_count,
        });
    }
    drop(connection);

    // ── Filesystem items ──────────────────────────────────────────────────────
    let (trash_count, trash_bytes) = trash_stats();
    if trash_count > 0 {
        items.push(CleanupItem {
            kind: "trash".to_string(),
            size_bytes: trash_bytes,
            file_count: trash_count,
        });
    }

    let cutoff = now_secs().saturating_sub(u64::from(old_downloads_days) * 86_400);
    let (dl_count, dl_bytes) = old_downloads_stats(cutoff);
    if dl_count > 0 {
        items.push(CleanupItem {
            kind: "oldDownloads".to_string(),
            size_bytes: dl_bytes,
            file_count: dl_count,
        });
    }

    items.sort_by_key(|b| std::cmp::Reverse(b.size_bytes));
    let total_bytes = items.iter().map(|i| i.size_bytes).sum();
    Ok(CleanupSummary { items, total_bytes })
}

/// Return up to `limit` files for a given cleanup kind.
/// DB-backed kinds reuse the insight predicates; filesystem kinds walk the
/// relevant directory.
pub fn list_cleanup_files(
    db_path: &Path,
    scan_id: &str,
    kind: &str,
    limit: u32,
    old_downloads_days: u32,
) -> Result<Vec<FileEntry>, AppError> {
    match kind {
        "development" | "archives" | "installers" => {
            database::list_insight_files(db_path, scan_id, kind, u64::MAX, 0, limit)
        }
        "duplicatesEstimate" => {
            // Return the largest files that share their size with at least one
            // other file in the scan — a quick proxy for "likely duplicate" that
            // needs no hashing.
            let connection = database::open(db_path)?;
            let sql = format!(
                "SELECT id, path, name, extension, category, size_bytes, modified_at, is_hidden, content_hash
                 FROM files
                 WHERE scan_id = ?1
                   AND size_bytes >= 1048576
                   AND size_bytes IN (
                     SELECT size_bytes FROM files
                     WHERE scan_id = ?1 AND size_bytes >= 1048576
                     GROUP BY size_bytes HAVING COUNT(*) >= 2
                   )
                 ORDER BY size_bytes DESC
                 LIMIT {limit}"
            );
            let mut stmt = connection.prepare(&sql)?;
            let rows = stmt.query_map(params![scan_id], map_row)?;
            Ok(rows.collect::<Result<Vec<_>, _>>()?)
        }
        "trash" => Ok(filesystem_files(trash_path().as_deref(), limit, None)),
        "oldDownloads" => {
            let cutoff = now_secs().saturating_sub(u64::from(old_downloads_days) * 86_400);
            Ok(filesystem_files(
                downloads_path().as_deref(),
                limit,
                Some(cutoff),
            ))
        }
        _ => Err(AppError::new(
            "INVALID_CLEANUP_KIND",
            format!("未知的清理类型：{kind}。"),
        )),
    }
}

// ── DB helpers ────────────────────────────────────────────────────────────────

/// (kind, SQL) pairs for scan-index cleanup items. SQL must take `scan_id` as
/// `?1` and return (COUNT, SUM(size_bytes)).
const DB_ITEMS: &[(&str, &str)] = &[
    (
        "development",
        "SELECT COUNT(*), COALESCE(SUM(size_bytes), 0) FROM files
         WHERE scan_id = ?1 AND (
           path LIKE '%/node_modules/%' OR path LIKE '%/target/%'
           OR path LIKE '%/dist/%' OR path LIKE '%/.next/%'
         )",
    ),
    (
        "archives",
        "SELECT COUNT(*), COALESCE(SUM(size_bytes), 0) FROM files
         WHERE scan_id = ?1 AND category = 'archives'",
    ),
    (
        "installers",
        "SELECT COUNT(*), COALESCE(SUM(size_bytes), 0) FROM files
         WHERE scan_id = ?1 AND category = 'applications'",
    ),
];

fn aggregate(connection: &Connection, sql: &str, scan_id: &str) -> Result<(u64, u64), AppError> {
    Ok(connection.query_row(sql, params![scan_id], |row| {
        Ok((
            from_i64(row.get::<_, i64>(0)?),
            from_i64(row.get::<_, i64>(1)?),
        ))
    })?)
}

fn map_row(row: &rusqlite::Row) -> rusqlite::Result<FileEntry> {
    Ok(FileEntry {
        id: row.get(0)?,
        path: row.get(1)?,
        name: row.get(2)?,
        extension: row.get(3)?,
        category: row.get(4)?,
        size_bytes: from_i64(row.get::<_, i64>(5)?),
        modified_at: row.get(6)?,
        is_hidden: row.get::<_, i64>(7)? != 0,
        content_hash: row.get(8)?,
    })
}

// ── Filesystem helpers ────────────────────────────────────────────────────────

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn home_dir() -> Option<PathBuf> {
    std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .ok()
        .map(PathBuf::from)
}

fn trash_path() -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        Some(home_dir()?.join(".Trash"))
    }
    #[cfg(not(target_os = "macos"))]
    {
        // Windows $Recycle.Bin requires elevated access; skip for now.
        None
    }
}

fn downloads_path() -> Option<PathBuf> {
    home_dir().map(|h| h.join("Downloads"))
}

/// Walk `dir`, optionally filtering to files whose `modified_at < cutoff_secs`.
/// Returns (file_count, total_size_bytes).
fn walk_stats(dir: &Path, cutoff: Option<u64>) -> (u64, u64) {
    let mut count = 0u64;
    let mut bytes = 0u64;
    for entry in WalkDir::new(dir)
        .follow_links(false)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if !entry.file_type().is_file() {
            continue;
        }
        let Ok(meta) = entry.metadata() else { continue };
        if let Some(cutoff) = cutoff {
            let modified = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map(|d| d.as_secs())
                .unwrap_or(u64::MAX);
            if modified >= cutoff {
                continue;
            }
        }
        count += 1;
        bytes += meta.len();
    }
    (count, bytes)
}

fn trash_stats() -> (u64, u64) {
    match trash_path() {
        Some(p) if p.exists() => walk_stats(&p, None),
        _ => (0, 0),
    }
}

fn old_downloads_stats(cutoff_secs: u64) -> (u64, u64) {
    match downloads_path() {
        Some(p) if p.exists() => walk_stats(&p, Some(cutoff_secs)),
        _ => (0, 0),
    }
}

/// Return up to `limit` `FileEntry` values from `dir`, filtered by optional
/// modification-time cutoff. Sorted by size descending.
fn filesystem_files(dir: Option<&Path>, limit: u32, cutoff: Option<u64>) -> Vec<FileEntry> {
    let Some(dir) = dir else { return vec![] };
    if !dir.exists() {
        return vec![];
    }

    let mut files: Vec<FileEntry> = WalkDir::new(dir)
        .follow_links(false)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .filter_map(|e| {
            let meta = e.metadata().ok()?;
            let modified_secs = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map(|d| d.as_secs());
            if let (Some(cutoff), Some(m)) = (cutoff, modified_secs) {
                if m >= cutoff {
                    return None;
                }
            }
            let path = e.path().to_string_lossy().to_string();
            let name = e.file_name().to_string_lossy().to_string();
            let ext = e
                .path()
                .extension()
                .map(|x| x.to_string_lossy().to_string());
            let category = classify_extension(ext.as_deref()).to_owned();
            Some(FileEntry {
                id: 0,
                path,
                name,
                extension: ext,
                category,
                size_bytes: meta.len(),
                modified_at: modified_secs.map(|s| s as i64),
                is_hidden: false,
                content_hash: None,
            })
        })
        .collect();

    files.sort_by_key(|b| std::cmp::Reverse(b.size_bytes));
    files.truncate(limit as usize);
    files
}

fn from_i64(v: i64) -> u64 {
    u64::try_from(v).unwrap_or(0)
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::{create_scan_run, initialize, insert_file_batch, open};
    use std::fs;
    use uuid::Uuid;

    fn make_entry(path: &str, name: &str, size: u64, ext: Option<&str>) -> FileEntry {
        FileEntry {
            id: 0,
            path: path.to_owned(),
            name: name.to_owned(),
            extension: ext.map(str::to_owned),
            category: classify_extension(ext).to_owned(),
            size_bytes: size,
            modified_at: None,
            is_hidden: false,
            content_hash: None,
        }
    }

    #[test]
    fn cleanup_summary_aggregates_db_items() {
        let tmp = std::env::temp_dir().join(format!("luma-cleanup-{}", Uuid::new_v4()));
        fs::create_dir_all(&tmp).unwrap();
        let db = tmp.join("test.db");
        initialize(&db).unwrap();
        create_scan_run(&db, "s1", "/root", 0).unwrap();

        let mut conn = open(&db).unwrap();
        insert_file_batch(
            &mut conn,
            "s1",
            &[
                make_entry(
                    "/root/proj/node_modules/x.js",
                    "x.js",
                    5_000_000,
                    Some("js"),
                ),
                make_entry("/root/archive.zip", "archive.zip", 2_000_000, Some("zip")),
                make_entry("/root/app.dmg", "app.dmg", 3_000_000, Some("dmg")),
                // two files with the same size → duplicate estimate
                make_entry("/root/a.mp4", "a.mp4", 10_000_000, Some("mp4")),
                make_entry("/root/b.mp4", "b.mp4", 10_000_000, Some("mp4")),
            ],
        )
        .unwrap();
        drop(conn);

        let summary = build_cleanup_summary(&db, "s1", 180).unwrap();
        let kinds: Vec<&str> = summary.items.iter().map(|i| i.kind.as_str()).collect();
        assert!(kinds.contains(&"development"), "expected development item");
        assert!(kinds.contains(&"archives"), "expected archives item");
        assert!(kinds.contains(&"installers"), "expected installers item");
        assert!(
            kinds.contains(&"duplicatesEstimate"),
            "expected duplicate estimate"
        );
        assert!(summary.total_bytes > 0);

        fs::remove_dir_all(tmp).ok();
    }
}
