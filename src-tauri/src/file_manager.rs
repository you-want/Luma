use crate::{database, error::AppError, models::FileEntry};
use rusqlite::params;
use serde::Serialize;
use std::path::Path;

/// A directory node derived from the scan index path data.
/// Counts are aggregated over all descendant files (not just direct children).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DirNode {
    pub path: String,
    pub name: String,
    /// Total files anywhere under this directory.
    pub file_count: u64,
    /// Aggregated size of all descendant files.
    pub size_bytes: u64,
    /// Whether this directory has at least one subdirectory under it.
    pub has_children: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DirectoryListing {
    pub dirs: Vec<DirNode>,
    pub files: Vec<FileEntry>,
    pub total_files: u64,
}

/// Returns the direct subdirectories of `parent_path` for the given scan,
/// derived from path prefixes stored in the files table.
pub fn get_directory_nodes(
    db_path: &Path,
    scan_id: &str,
    parent_path: &str,
) -> Result<Vec<DirNode>, AppError> {
    let connection = database::open(db_path)?;

    // `parent_path` has no trailing slash. The pattern matches any file
    // that is *inside* (not just at) the parent directory.
    let glob = format!("{parent_path}/%");
    // SQLite SUBSTR is 1-based; skip `parent_path` + the following '/'.
    let prefix_len = i64::try_from(parent_path.len()).unwrap_or(0) + 2;

    // Extract the first path segment after the parent prefix.
    // Files sitting directly in the parent have no '/' in their relative path
    // and are excluded by the `instr(...) > 0` guard.
    let sql = "
        SELECT
            ?1 || '/' || dir_name                           AS path,
            dir_name                                        AS name,
            COUNT(*)                                        AS file_count,
            COALESCE(SUM(size_bytes), 0)                    AS size_bytes,
            MAX(CASE WHEN instr(rel_after_dir, '/') > 0
                     THEN 1 ELSE 0 END)                     AS has_children
        FROM (
            SELECT
                size_bytes,
                SUBSTR(
                    SUBSTR(path, ?2),
                    1,
                    instr(SUBSTR(path, ?2), '/') - 1
                )                                           AS dir_name,
                SUBSTR(
                    SUBSTR(path, ?2),
                    instr(SUBSTR(path, ?2), '/') + 1
                )                                           AS rel_after_dir
            FROM files
            WHERE scan_id = ?3
              AND path LIKE ?4 ESCAPE '\\'
              AND instr(SUBSTR(path, ?2), '/') > 0
        )
        WHERE dir_name != ''
        GROUP BY dir_name
        ORDER BY size_bytes DESC
    ";

    let mut stmt = connection.prepare(sql)?;
    let rows = stmt.query_map(
        params![parent_path, prefix_len, scan_id, glob],
        |row| {
            Ok(DirNode {
                path: row.get(0)?,
                name: row.get(1)?,
                file_count: from_i64(row.get(2)?),
                size_bytes: from_i64(row.get(3)?),
                has_children: row.get::<_, i64>(4)? != 0,
            })
        },
    )?;

    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

/// Returns the files sitting directly inside `dir_path` (non-recursive),
/// together with the total count for pagination.
pub fn list_directory_files(
    db_path: &Path,
    scan_id: &str,
    dir_path: &str,
    include_hidden: bool,
    sort_order: &str,
    limit: u32,
    offset: u32,
) -> Result<(Vec<FileEntry>, u64), AppError> {
    let connection = database::open(db_path)?;

    let glob = format!("{dir_path}/%");
    let prefix_len = i64::try_from(dir_path.len()).unwrap_or(0) + 2;

    let hidden_clause = if include_hidden { "" } else { " AND is_hidden = 0" };

    // A direct child has no '/' in its relative path segment.
    let where_clause = format!(
        "scan_id = ?1 AND path LIKE ?2 ESCAPE '\\' AND instr(SUBSTR(path, ?3), '/') = 0{hidden_clause}"
    );

    let count_sql = format!("SELECT COUNT(*) FROM files WHERE {where_clause}");
    let total: i64 =
        connection.query_row(&count_sql, params![scan_id, glob, prefix_len], |row| {
            row.get(0)
        })?;

    let page_sql = format!(
        "SELECT id, path, name, extension, category, size_bytes, modified_at, is_hidden, content_hash
         FROM files WHERE {where_clause}
         ORDER BY {sort_order}
         LIMIT ?4 OFFSET ?5"
    );
    let mut stmt = connection.prepare(&page_sql)?;
    let rows = stmt.query_map(
        params![scan_id, glob, prefix_len, limit, offset],
        |row| {
            Ok(FileEntry {
                id: row.get(0)?,
                path: row.get(1)?,
                name: row.get(2)?,
                extension: row.get(3)?,
                category: row.get(4)?,
                size_bytes: from_i64(row.get(5)?),
                modified_at: row.get(6)?,
                is_hidden: row.get::<_, i64>(7)? != 0,
                content_hash: row.get(8)?,
            })
        },
    )?;

    let files = rows.collect::<Result<Vec<_>, _>>()?;
    Ok((files, from_i64(total)))
}

fn from_i64(value: i64) -> u64 {
    u64::try_from(value).unwrap_or_default()
}
