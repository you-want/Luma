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
    // that is *inside* (not just at) the parent directory. `ESCAPE '\'`
    // means the user-supplied prefix must have its own LIKE wildcards
    // (`%`, `_`) and the escape character (`\`) backslashed, otherwise a
    // directory literally named e.g. "100%" or "a_b" would match stray files
    // that merely *look* alike.
    let glob = format!("{}/%", escape_like(parent_path));
    // `SUBSTR()` operates on *characters* while `String::len()` is in
    // bytes, so a multi-byte (e.g. Chinese) prefix would silently over-slice
    // if we passed the byte length. The char count of the known prefix is
    // deterministic, so compute it here instead of in SQL.
    let prefix_chars = i64::try_from(parent_path.chars().count()).unwrap_or(0) + 2;

    // Extract the first path segment after the parent prefix.
    // `prefix_chars` is a character offset (SUBSTR is 1-based); `?3` is the
    // character position just past `parent_path` + '/' so the relative path
    // after the first subdirectory starts at char 1.
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
                    SUBSTR(path, ?3),
                    instr(SUBSTR(path, ?3), '/') + 1
                )                                           AS rel_after_dir
            FROM files
            WHERE scan_id = ?4
              AND path LIKE ?5 ESCAPE '\\'
              AND instr(SUBSTR(path, ?2), '/') > 0
        )
        WHERE dir_name != ''
        GROUP BY dir_name
        ORDER BY size_bytes DESC
    ";

    let mut stmt = connection.prepare(sql)?;
    let rows = stmt.query_map(
        params![parent_path, prefix_chars, prefix_chars + 1, scan_id, glob],
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

    let glob = format!("{}/%", escape_like(dir_path));
    // `SUBSTR()` operates on *characters* while `String::len()` is in bytes,
    // so a multi-byte (e.g. Chinese) prefix would silently over-slice if we
    // passed the byte length. The char count of the known prefix is
    // deterministic, so compute it here instead of in SQL.
    // +2 = skip the trailing '/' of the prefix too (SUBSTR is 1-based).
    let prefix_chars = i64::try_from(dir_path.chars().count()).unwrap_or(0) + 2;

    let hidden_clause = if include_hidden {
        ""
    } else {
        " AND is_hidden = 0"
    };

    // A direct child has no '/' in its relative path segment.
    let where_clause = format!(
        "scan_id = ?1 AND path LIKE ?2 ESCAPE '\\' AND instr(SUBSTR(path, ?3), '/') = 0{hidden_clause}"
    );

    let count_sql = format!("SELECT COUNT(*) FROM files WHERE {where_clause}");
    let total: i64 =
        connection.query_row(&count_sql, params![scan_id, glob, prefix_chars], |row| {
            row.get(0)
        })?;

    let page_sql = format!(
        "SELECT id, path, name, extension, category, size_bytes, modified_at, is_hidden, content_hash
         FROM files WHERE {where_clause}
         ORDER BY {sort_order}
         LIMIT ?4 OFFSET ?5"
    );
    let mut stmt = connection.prepare(&page_sql)?;
    let rows = stmt.query_map(params![scan_id, glob, prefix_chars, limit, offset], |row| {
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
    })?;

    let files = rows.collect::<Result<Vec<_>, _>>()?;
    Ok((files, from_i64(total)))
}

/// Backslash-escape the LIKE wildcards (`%`, `_`) and the escape character
/// itself (`\`) in a user-supplied directory path so that a `LIKE … ESCAPE '\'`
/// pattern matches the literal prefix exactly and never strays into other
/// directories that merely look alike.
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

fn from_i64(value: i64) -> u64 {
    u64::try_from(value).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::{create_scan_run, initialize, insert_file_batch, open};
    use std::fs;
    use uuid::Uuid;

    /// Regression test: multi-byte (UTF-8) directory names must not break the
    /// byte-vs-char slice arithmetic in `get_directory_nodes` /
    /// `list_directory_files`. The previous code passed a byte offset to
    /// `SUBSTR()`, which SQLite treats as a character index — so a Chinese
    /// parent directory would silently mis-slice its own path.
    #[test]
    fn multi_byte_directory_names_slice_correctly() {
        let tmp = std::env::temp_dir().join(format!("luma-multibyte-{}", Uuid::new_v4()));
        fs::create_dir_all(&tmp).unwrap();
        let db = tmp.join("test.db");
        initialize(&db).unwrap();
        create_scan_run(&db, "s1", "/root", 0).unwrap();

        // "文档" is 2 chars × 3 bytes each = 6 bytes; the old code would have
        // sliced at byte 6 + 2 = 8, but char position 8 of "/root/文档/笔记.txt"
        // (which is 13 chars) is inside the second char of "档".
        let parent = "/root/文档";
        let entries = vec![
            FileEntry {
                id: 0,
                path: "/root/文档/笔记.txt".into(),
                name: "笔记.txt".into(),
                extension: Some("txt".into()),
                category: "documents".into(),
                size_bytes: 100,
                modified_at: None,
                is_hidden: false,
                content_hash: None,
            },
            FileEntry {
                id: 0,
                path: "/root/文档/子目录/图片.png".into(),
                name: "图片.png".into(),
                extension: Some("png".into()),
                category: "images".into(),
                size_bytes: 200,
                modified_at: None,
                is_hidden: false,
                content_hash: None,
            },
            FileEntry {
                id: 0,
                path: "/root/文档/photo.jpg".into(),
                name: "photo.jpg".into(),
                extension: Some("jpg".into()),
                category: "images".into(),
                size_bytes: 300,
                modified_at: None,
                is_hidden: false,
                content_hash: None,
            },
        ];
        let mut conn = open(&db).unwrap();
        insert_file_batch(&mut conn, "s1", &entries).unwrap();
        drop(conn);

        // list_directory_files: dir = "/root/文档" → 2 direct files, names intact
        let (files, total) = list_directory_files(
            &db,
            "s1",
            parent,
            false,
            "name COLLATE NOCASE ASC, id ASC",
            100,
            0,
        )
        .unwrap();
        assert_eq!(total, 2);
        assert_eq!(
            files.iter().map(|f| f.name.as_str()).collect::<Vec<_>>(),
            vec!["photo.jpg", "笔记.txt"],
            "names must be the bare file names, not a mangled slice"
        );

        // get_directory_nodes: parent = "/root" → one child dir "文档"
        let dirs = get_directory_nodes(&db, "s1", "/root").unwrap();
        assert_eq!(dirs.len(), 1, "exactly one subdirectory expected");
        assert_eq!(
            dirs[0].name, "文档",
            "dir name must be the full segment, not a partial slice"
        );
        assert_eq!(
            dirs[0].file_count, 3,
            "aggregated count over all descendants"
        );
        assert_eq!(dirs[0].size_bytes, 600);
        assert!(dirs[0].has_children, "子目录 exists below 文档");

        // get_directory_nodes: parent = "/root/文档" → one child dir "子目录"
        let sub = get_directory_nodes(&db, "s1", parent).unwrap();
        assert_eq!(sub.len(), 1);
        assert_eq!(sub[0].name, "子目录");
        assert_eq!(sub[0].file_count, 1);

        fs::remove_dir_all(&tmp).ok();
    }

    /// Regression test: LIKE wildcards in a directory name (`%`, `_`, `\`)
    /// must be escaped so they match only that exact directory, not every
    /// file that merely *looks* like it (e.g. a dir named `100%` must not
    /// pull in unrelated `100abc` paths).
    #[test]
    fn like_wildcards_in_directory_names_are_escaped() {
        let tmp = std::env::temp_dir().join(format!("luma-like-esc-{}", Uuid::new_v4()));
        fs::create_dir_all(&tmp).unwrap();
        let db = tmp.join("test.db");
        initialize(&db).unwrap();
        create_scan_run(&db, "s1", "/root", 0).unwrap();

        let entries = vec![
            // A directory literally named "100%" with a file inside it.
            FileEntry {
                id: 0,
                path: "/root/100%/inside.txt".into(),
                name: "inside.txt".into(),
                extension: Some("txt".into()),
                category: "documents".into(),
                size_bytes: 10,
                modified_at: None,
                is_hidden: false,
                content_hash: None,
            },
            // An unrelated path that the *unescaped* pattern "100%/%" would
            // also match (since "%" is a wildcard), proving the fix.
            FileEntry {
                id: 0,
                path: "/root/100abc/decoy.txt".into(),
                name: "decoy.txt".into(),
                extension: Some("txt".into()),
                category: "documents".into(),
                size_bytes: 99,
                modified_at: None,
                is_hidden: false,
                content_hash: None,
            },
            // A directory named "a_b" and a decoy "axb" that an unescaped
            // "a_b" pattern would match.
            FileEntry {
                id: 0,
                path: "/root/a_b/file.dat".into(),
                name: "file.dat".into(),
                extension: Some("dat".into()),
                category: "documents".into(),
                size_bytes: 20,
                modified_at: None,
                is_hidden: false,
                content_hash: None,
            },
            FileEntry {
                id: 0,
                path: "/root/axb/decoy2.dat".into(),
                name: "decoy2.dat".into(),
                extension: Some("dat".into()),
                category: "documents".into(),
                size_bytes: 88,
                modified_at: None,
                is_hidden: false,
                content_hash: None,
            },
        ];
        let mut conn = open(&db).unwrap();
        insert_file_batch(&mut conn, "s1", &entries).unwrap();
        drop(conn);

        // list_directory_files("/root/100%") must return exactly its own file,
        // not the "100abc" decoy.
        let (files, total) = list_directory_files(
            &db,
            "s1",
            "/root/100%",
            false,
            "name COLLATE NOCASE ASC, id ASC",
            100,
            0,
        )
        .unwrap();
        assert_eq!(total, 1, "only the file inside the literal '100%' dir");
        assert_eq!(files[0].name, "inside.txt");

        // get_directory_nodes("/root") reports each distinct subdirectory. The
        // escape fix guarantees the *file listing* inside "100%" / "a_b" is
        // exact; the directory nodes themselves legitimately include the decoy
        // dirs too (they are real separate directories in the scan).
        let dirs = get_directory_nodes(&db, "s1", "/root").unwrap();
        let names: Vec<String> = dirs.iter().map(|d| d.name.clone()).collect();
        assert!(names.contains(&"100%".to_string()), "names = {names:?}");
        assert!(names.contains(&"a_b".to_string()), "names = {names:?}");
        // The "100%" node must aggregate exactly its own file, not the decoy.
        let pct = dirs.iter().find(|d| d.name == "100%").expect("100% node");
        assert_eq!(
            pct.file_count, 1,
            "'100%' node must count only its own file"
        );
        assert_eq!(pct.size_bytes, 10);

        // And the "a_b" dir lists only its own file.
        let (files2, total2) = list_directory_files(
            &db,
            "s1",
            "/root/a_b",
            false,
            "name COLLATE NOCASE ASC, id ASC",
            100,
            0,
        )
        .unwrap();
        assert_eq!(total2, 1);
        assert_eq!(files2[0].name, "file.dat");

        fs::remove_dir_all(&tmp).ok();
    }
}
