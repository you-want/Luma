use crate::{
    error::AppError,
    models::{
        CategoryDelta, CategorySummary, FileEntry, InsightSummary, ScanComparison, ScanStats,
        ScanStatus, ScanSummary, SearchRequest, SearchResponse,
    },
};
use rusqlite::{params, Connection, OptionalExtension};
use std::path::Path;

pub fn initialize(path: &Path) -> Result<(), AppError> {
    let connection = open(path)?;
    let schema_version =
        connection.query_row("PRAGMA user_version", [], |row| row.get::<_, u32>(0))?;
    if schema_version > 3 {
        return Err(AppError::new(
            "DATABASE_ERROR",
            format!("数据库版本 {schema_version} 高于当前应用支持的版本。"),
        ));
    }
    connection.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS scan_runs (
          id TEXT PRIMARY KEY,
          root_path TEXT NOT NULL,
          status TEXT NOT NULL,
          started_at INTEGER NOT NULL,
          finished_at INTEGER,
          total_files INTEGER NOT NULL DEFAULT 0,
          total_directories INTEGER NOT NULL DEFAULT 0,
          total_bytes INTEGER NOT NULL DEFAULT 0,
          error_count INTEGER NOT NULL DEFAULT 0
        );

        CREATE TABLE IF NOT EXISTS files (
          id INTEGER PRIMARY KEY AUTOINCREMENT,
          scan_id TEXT NOT NULL,
          path TEXT NOT NULL,
          name TEXT NOT NULL,
          extension TEXT,
          category TEXT NOT NULL,
          size_bytes INTEGER NOT NULL,
          modified_at INTEGER,
          is_hidden INTEGER NOT NULL DEFAULT 0,
          content_hash TEXT,
          FOREIGN KEY(scan_id) REFERENCES scan_runs(id) ON DELETE CASCADE
        );

        CREATE INDEX IF NOT EXISTS idx_files_scan_size ON files(scan_id, size_bytes DESC);
        CREATE INDEX IF NOT EXISTS idx_files_scan_category ON files(scan_id, category);
        CREATE INDEX IF NOT EXISTS idx_files_scan_modified ON files(scan_id, modified_at);
        CREATE INDEX IF NOT EXISTS idx_files_scan_name ON files(scan_id, name);

        CREATE TABLE IF NOT EXISTS file_operations (
          id TEXT PRIMARY KEY,
          scan_id TEXT NOT NULL,
          root_path TEXT NOT NULL DEFAULT '',
          kind TEXT NOT NULL,
          label TEXT NOT NULL,
          status TEXT NOT NULL,
          undo_json TEXT,
          created_at INTEGER NOT NULL,
          finished_at INTEGER,
          error_message TEXT
        );

        CREATE INDEX IF NOT EXISTS idx_file_operations_scan_created
          ON file_operations(scan_id, created_at DESC);
",
    )?;

    // Older databases predate duplicate detection and do not have content_hash.
    // Migrate them in place before creating the index or running any queries
    // that select the new column. The PRAGMA check keeps this idempotent.
    let has_content_hash = connection
        .prepare("PRAGMA table_info(files)")?
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<Result<Vec<_>, _>>()?
        .iter()
        .any(|name| name == "content_hash");
    if !has_content_hash {
        connection.execute("ALTER TABLE files ADD COLUMN content_hash TEXT", [])?;
    }
    connection.execute(
        "CREATE INDEX IF NOT EXISTS idx_files_size_hash ON files(scan_id, size_bytes, content_hash)",
        [],
    )?;

    let operation_columns = connection
        .prepare("PRAGMA table_info(file_operations)")?
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<Result<Vec<_>, _>>()?;
    if !operation_columns.iter().any(|name| name == "root_path") {
        connection.execute(
            "ALTER TABLE file_operations ADD COLUMN root_path TEXT NOT NULL DEFAULT ''",
            [],
        )?;
    }
    connection.execute(
        "UPDATE file_operations
         SET root_path = COALESCE(
           (SELECT root_path FROM scan_runs WHERE scan_runs.id = file_operations.scan_id),
           root_path
         )
         WHERE root_path = ''",
        [],
    )?;
    connection.execute(
        "CREATE INDEX IF NOT EXISTS idx_file_operations_root_created
         ON file_operations(root_path, created_at DESC)",
        [],
    )?;

    connection.execute_batch(
        "
        UPDATE scan_runs
        SET status = 'failed', finished_at = unixepoch()
        WHERE status = 'running';

        UPDATE file_operations
        SET status = 'interrupted', finished_at = unixepoch(),
            error_message = COALESCE(error_message, '应用在操作完成前退出。')
        WHERE status = 'running';
        ",
    )?;

    if schema_version < 3 {
        connection.execute("PRAGMA user_version = 3", [])?;
    }
    Ok(())
}

pub fn open(path: &Path) -> Result<Connection, AppError> {
    let connection = Connection::open(path)?;
    connection.execute_batch("PRAGMA foreign_keys = ON; PRAGMA journal_mode = WAL;")?;
    Ok(connection)
}

pub fn create_scan_run(
    path: &Path,
    scan_id: &str,
    root_path: &str,
    started_at: i64,
) -> Result<(), AppError> {
    open(path)?.execute(
        "INSERT INTO scan_runs (id, root_path, status, started_at) VALUES (?1, ?2, 'running', ?3)",
        params![scan_id, root_path, started_at],
    )?;
    Ok(())
}

pub fn insert_file_batch(
    connection: &mut Connection,
    scan_id: &str,
    files: &[FileEntry],
) -> Result<(), AppError> {
    let transaction = connection.transaction()?;
    {
        let mut statement = transaction.prepare(
            "INSERT INTO files
             (scan_id, path, name, extension, category, size_bytes, modified_at, is_hidden, content_hash)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        )?;
        for file in files {
            statement.execute(params![
                scan_id,
                file.path,
                file.name,
                file.extension,
                file.category,
                to_i64(file.size_bytes),
                file.modified_at,
                i64::from(file.is_hidden),
                file.content_hash,
            ])?;
        }
    }
    transaction.commit()?;
    Ok(())
}

pub fn finish_scan(
    connection: &Connection,
    scan_id: &str,
    status: ScanStatus,
    stats: &ScanStats,
    finished_at: i64,
) -> Result<(), AppError> {
    connection.execute(
        "UPDATE scan_runs SET status = ?2, finished_at = ?3, total_files = ?4,
         total_directories = ?5, total_bytes = ?6, error_count = ?7 WHERE id = ?1",
        params![
            scan_id,
            status.as_str(),
            finished_at,
            to_i64(stats.files_scanned),
            to_i64(stats.directories_scanned),
            to_i64(stats.bytes_scanned),
            to_i64(stats.errors),
        ],
    )?;
    Ok(())
}

pub fn mark_scan_failed(path: &Path, scan_id: &str, finished_at: i64) -> Result<(), AppError> {
    open(path)?.execute(
        "UPDATE scan_runs SET status = 'failed', finished_at = ?2 WHERE id = ?1",
        params![scan_id, finished_at],
    )?;
    Ok(())
}

pub fn latest_scan(path: &Path) -> Result<Option<ScanSummary>, AppError> {
    let connection = open(path)?;
    let scan_id = connection
        .query_row(
            "SELECT id FROM scan_runs WHERE status = 'completed' ORDER BY finished_at DESC LIMIT 1",
            [],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    scan_id
        .map(|id| scan_summary_with_connection(&connection, &id))
        .transpose()
}

pub fn scan_summary(path: &Path, scan_id: &str) -> Result<Option<ScanSummary>, AppError> {
    let connection = open(path)?;
    let exists = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM scan_runs WHERE id = ?1)",
        [scan_id],
        |row| row.get::<_, bool>(0),
    )?;
    if exists {
        Ok(Some(scan_summary_with_connection(&connection, scan_id)?))
    } else {
        Ok(None)
    }
}

fn scan_summary_with_connection(
    connection: &Connection,
    scan_id: &str,
) -> Result<ScanSummary, AppError> {
    let mut summary = connection.query_row(
        "SELECT id, root_path, status, started_at, finished_at, total_files,
         total_directories, total_bytes, error_count FROM scan_runs WHERE id = ?1",
        [scan_id],
        |row| {
            Ok(ScanSummary {
                scan_id: row.get(0)?,
                root_path: row.get(1)?,
                status: parse_status(&row.get::<_, String>(2)?),
                started_at: row.get(3)?,
                finished_at: row.get(4)?,
                total_files: from_i64(row.get(5)?),
                total_directories: from_i64(row.get(6)?),
                total_bytes: from_i64(row.get(7)?),
                error_count: from_i64(row.get(8)?),
                categories: Vec::new(),
            })
        },
    )?;

    let mut statement = connection.prepare(
        "SELECT category, COUNT(*), COALESCE(SUM(size_bytes), 0)
         FROM files WHERE scan_id = ?1 GROUP BY category ORDER BY SUM(size_bytes) DESC",
    )?;
    summary.categories = statement
        .query_map([scan_id], |row| {
            Ok(CategorySummary {
                category: row.get(0)?,
                file_count: from_i64(row.get(1)?),
                size_bytes: from_i64(row.get(2)?),
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(summary)
}

pub fn list_large_files(
    path: &Path,
    scan_id: &str,
    limit: u32,
    offset: u32,
) -> Result<Vec<FileEntry>, AppError> {
    let connection = open(path)?;
    let mut statement = connection.prepare(
        "SELECT id, path, name, extension, category, size_bytes, modified_at, is_hidden, content_hash
         FROM files WHERE scan_id = ?1 ORDER BY size_bytes DESC LIMIT ?2 OFFSET ?3",
    )?;
    let rows = statement.query_map(params![scan_id, limit, offset], |row| {
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
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

/// Returns the largest files backing a single insight, reusing the same
/// per-kind predicates as `list_insights` so the detail view can never
/// disagree with the summary counts.
pub fn list_insight_files(
    path: &Path,
    scan_id: &str,
    kind: &str,
    large_file_threshold: u64,
    stale_before: i64,
    limit: u32,
) -> Result<Vec<FileEntry>, AppError> {
    let connection = open(path)?;
    let (predicate, extra_param): (&str, Option<i64>) = match kind {
        "largeFiles" => ("size_bytes >= ?2", Some(to_i64(large_file_threshold))),
        "staleFiles" => (
            "modified_at IS NOT NULL AND modified_at < ?2",
            Some(stale_before),
        ),
        "development" => {
            // Paths are normalized to `/` on all platforms (see scanner.rs normalize_separators),
            // so SQL LIKE patterns always use `/` regardless of the host OS.
            (
                "(path LIKE '%/node_modules/%' OR path LIKE '%/target/%' \
                 OR path LIKE '%/dist/%' OR path LIKE '%/.next/%')",
                None,
            )
        }
        "archives" => ("category = 'archives'", None),
        "installers" => ("category = 'applications'", None),
        _ => {
            return Err(AppError::new(
                "INVALID_INSIGHT_KIND",
                format!("未知的发现类型：{kind}。"),
            ))
        }
    };

    let sql = format!(
        "SELECT id, path, name, extension, category, size_bytes, modified_at, is_hidden, content_hash
         FROM files WHERE scan_id = ?1 AND {predicate}
         ORDER BY size_bytes DESC LIMIT {limit}"
    );
    let mut statement = connection.prepare(&sql)?;
    let map_row = |row: &rusqlite::Row| {
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
    };
    let rows = match extra_param {
        Some(value) => statement.query_map(params![scan_id, value], map_row)?,
        None => statement.query_map(params![scan_id], map_row)?,
    };
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

pub fn list_insights(
    path: &Path,
    scan_id: &str,
    large_file_threshold: u64,
    stale_before: i64,
) -> Result<Vec<InsightSummary>, AppError> {
    let connection = open(path)?;
    let mut insights = Vec::new();

    // The human-readable rule ("basis") is no longer produced here: the frontend
    // rebuilds it from `kind` plus the thresholds it already holds, so the text
    // is translatable instead of a fixed backend sentence.
    push_insight(
        &mut insights,
        "largeFiles",
        aggregate(
            &connection,
            "SELECT COUNT(*), COALESCE(SUM(size_bytes), 0) FROM files
             WHERE scan_id = ?1 AND size_bytes >= ?2",
            params![scan_id, to_i64(large_file_threshold)],
        )?,
    );
    push_insight(
        &mut insights,
        "staleFiles",
        aggregate(
            &connection,
            "SELECT COUNT(*), COALESCE(SUM(size_bytes), 0) FROM files
             WHERE scan_id = ?1 AND modified_at IS NOT NULL AND modified_at < ?2",
            params![scan_id, stale_before],
        )?,
    );
    push_insight(
        &mut insights,
        "development",
        aggregate(
            &connection,
            "SELECT COUNT(*), COALESCE(SUM(size_bytes), 0) FROM files
             WHERE scan_id = ?1 AND (
               path LIKE '%/node_modules/%' OR path LIKE '%/target/%'
               OR path LIKE '%/dist/%' OR path LIKE '%/.next/%'
             )",
            [scan_id],
        )?,
    );
    push_insight(
        &mut insights,
        "archives",
        aggregate(
            &connection,
            "SELECT COUNT(*), COALESCE(SUM(size_bytes), 0) FROM files
             WHERE scan_id = ?1 AND category = 'archives'",
            [scan_id],
        )?,
    );
    push_insight(
        &mut insights,
        "installers",
        aggregate(
            &connection,
            "SELECT COUNT(*), COALESCE(SUM(size_bytes), 0) FROM files
             WHERE scan_id = ?1 AND category = 'applications'",
            [scan_id],
        )?,
    );

    Ok(insights)
}

fn aggregate<P: rusqlite::Params>(
    connection: &Connection,
    sql: &str,
    parameters: P,
) -> Result<(u64, u64), AppError> {
    Ok(connection.query_row(sql, parameters, |row| {
        Ok((from_i64(row.get(0)?), from_i64(row.get(1)?)))
    })?)
}

fn push_insight(
    insights: &mut Vec<InsightSummary>,
    kind: &str,
    (file_count, size_bytes): (u64, u64),
) {
    if file_count > 0 {
        insights.push(InsightSummary {
            kind: kind.to_owned(),
            file_count,
            size_bytes,
        });
    }
}

pub fn prune_old_scans(connection: &Connection) -> Result<(), AppError> {
    // Retain the three most recent terminal runs *per root path* so each
    // scanned directory keeps its own history for comparison, instead of
    // evicting one directory's runs when another directory is scanned.
    connection.execute(
        "DELETE FROM scan_runs WHERE id IN (
           SELECT id FROM (
             SELECT id, ROW_NUMBER() OVER (
               PARTITION BY root_path
               ORDER BY COALESCE(finished_at, started_at) DESC
             ) AS rn
             FROM scan_runs WHERE status != 'running'
           ) WHERE rn > 3
         )",
        [],
    )?;
    Ok(())
}

/// Returns completed scans for the same root path as `scan_id`, newest first,
/// so the UI can offer earlier runs to compare against.
pub fn list_scan_history(path: &Path, scan_id: &str) -> Result<Vec<ScanSummary>, AppError> {
    let connection = open(path)?;
    let root_path = connection
        .query_row(
            "SELECT root_path FROM scan_runs WHERE id = ?1",
            [scan_id],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    let Some(root_path) = root_path else {
        return Ok(Vec::new());
    };

    let mut statement = connection.prepare(
        "SELECT id FROM scan_runs
         WHERE root_path = ?1 AND status = 'completed'
         ORDER BY COALESCE(finished_at, started_at) DESC",
    )?;
    let ids = statement
        .query_map([root_path], |row| row.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    ids.iter()
        .map(|id| scan_summary_with_connection(&connection, id))
        .collect()
}

/// Compares two completed scans and reports the per-category and total
/// deltas between them. `base` is the earlier reference; `target` is the
/// scan being examined.
pub fn compare_scans(
    path: &Path,
    base_scan_id: &str,
    target_scan_id: &str,
) -> Result<ScanComparison, AppError> {
    let connection = open(path)?;
    let base = scan_summary_with_connection(&connection, base_scan_id)?;
    let target = scan_summary_with_connection(&connection, target_scan_id)?;

    let mut categories: std::collections::BTreeMap<String, CategoryDelta> =
        std::collections::BTreeMap::new();
    for category in &base.categories {
        let entry = categories
            .entry(category.category.clone())
            .or_insert_with(|| CategoryDelta::empty(&category.category));
        entry.base_size_bytes = category.size_bytes;
        entry.base_file_count = category.file_count;
    }
    for category in &target.categories {
        let entry = categories
            .entry(category.category.clone())
            .or_insert_with(|| CategoryDelta::empty(&category.category));
        entry.target_size_bytes = category.size_bytes;
        entry.target_file_count = category.file_count;
    }

    let mut category_deltas: Vec<CategoryDelta> = categories
        .into_values()
        .map(|mut delta| {
            delta.size_delta = i128_delta(delta.target_size_bytes, delta.base_size_bytes);
            delta.file_count_delta = i128_delta(delta.target_file_count, delta.base_file_count);
            delta
        })
        .collect();
    // Largest absolute size change first so the biggest movers surface on top.
    category_deltas.sort_by_key(|delta| std::cmp::Reverse(delta.size_delta.abs()));

    Ok(ScanComparison {
        total_bytes_delta: i128_delta(target.total_bytes, base.total_bytes),
        total_files_delta: i128_delta(target.total_files, base.total_files),
        categories: category_deltas,
        base,
        target,
    })
}

/// Signed difference between two unsigned counters, saturating into i64 so a
/// very large swing can never wrap.
fn i128_delta(target: u64, base: u64) -> i64 {
    let delta = i128::from(target) - i128::from(base);
    delta.clamp(i128::from(i64::MIN), i128::from(i64::MAX)) as i64
}

fn parse_status(value: &str) -> ScanStatus {
    match value {
        "completed" => ScanStatus::Completed,
        "cancelled" => ScanStatus::Cancelled,
        "failed" => ScanStatus::Failed,
        _ => ScanStatus::Running,
    }
}

/// Escape a user query for a `LIKE` pattern so `%`, `_`, and the escape
/// character itself are matched literally rather than as wildcards. Paired with
/// `ESCAPE '\'` in the SQL. Without this, a query like "50%" would match far
/// more than intended.
fn escape_like(query: &str) -> String {
    let mut escaped = String::with_capacity(query.len());
    for ch in query.chars() {
        if matches!(ch, '\\' | '%' | '_') {
            escaped.push('\\');
        }
        escaped.push(ch);
    }
    escaped
}

/// Search one scan's indexed rows. Name/path matching, optional filters, sort,
/// and pagination all run in SQLite; the frontend never loads the full table.
/// Returns the requested page plus the total match count for pagination.
pub fn search_files(path: &Path, request: &SearchRequest) -> Result<SearchResponse, AppError> {
    let connection = open(path)?;

    // Build the WHERE clause incrementally. Every dynamic value is a bound
    // parameter; only the closed-set ORDER BY and these static fragments are
    // interpolated, so the query is not injectable.
    let mut clauses: Vec<String> = vec!["scan_id = ?1".to_owned()];
    let mut params: Vec<rusqlite::types::Value> = vec![request.scan_id.clone().into()];

    let trimmed = request.query.trim();
    if !trimmed.is_empty() {
        params.push(format!("%{}%", escape_like(trimmed)).into());
        let idx = params.len();
        // Match either the file name or its full path, escaped LIKE.
        clauses.push(format!(
            "(name LIKE ?{idx} ESCAPE '\\' OR path LIKE ?{idx} ESCAPE '\\')"
        ));
    }
    if let Some(category) = request.category.as_deref().filter(|c| !c.is_empty()) {
        params.push(category.to_owned().into());
        clauses.push(format!("category = ?{}", params.len()));
    }
    if let Some(extension) = request.extension.as_deref().filter(|e| !e.is_empty()) {
        params.push(extension.to_ascii_lowercase().into());
        clauses.push(format!("extension = ?{}", params.len()));
    }
    if let Some(min_size) = request.min_size {
        params.push(to_i64(min_size).into());
        clauses.push(format!("size_bytes >= ?{}", params.len()));
    }
    if let Some(max_size) = request.max_size {
        params.push(to_i64(max_size).into());
        clauses.push(format!("size_bytes <= ?{}", params.len()));
    }
    if let Some(after) = request.modified_after {
        params.push(after.into());
        clauses.push(format!(
            "modified_at IS NOT NULL AND modified_at >= ?{}",
            params.len()
        ));
    }
    if let Some(before) = request.modified_before {
        params.push(before.into());
        clauses.push(format!(
            "modified_at IS NOT NULL AND modified_at <= ?{}",
            params.len()
        ));
    }
    if !request.include_hidden {
        clauses.push("is_hidden = 0".to_owned());
    }
    let where_clause = clauses.join(" AND ");

    // Total match count first, so the UI can page without loading every row.
    let count_sql = format!("SELECT COUNT(*) FROM files WHERE {where_clause}");
    let total: i64 = connection.query_row(
        &count_sql,
        rusqlite::params_from_iter(params.iter()),
        |row| row.get(0),
    )?;

    let limit = request.limit.unwrap_or(50).clamp(1, 200);
    let offset = request.offset.unwrap_or(0);
    let sql = format!(
        "SELECT id, path, name, extension, category, size_bytes, modified_at, is_hidden, content_hash
         FROM files WHERE {where_clause}
         ORDER BY {} LIMIT ?{} OFFSET ?{}",
        request.sort.order_by(),
        params.len() + 1,
        params.len() + 2,
    );
    let mut page_params = params.clone();
    page_params.push(i64::from(limit).into());
    page_params.push(i64::from(offset).into());

    let mut statement = connection.prepare(&sql)?;
    let rows = statement.query_map(rusqlite::params_from_iter(page_params.iter()), |row| {
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

    Ok(SearchResponse {
        files,
        total: from_i64(total),
        limit,
        offset,
    })
}

fn to_i64(value: u64) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

fn from_i64(value: i64) -> u64 {
    u64::try_from(value).unwrap_or_default()
}

/// Public helper so sibling modules (organizer, file_manager) can reuse the
/// same u64 extraction without duplicating the conversion.
pub fn row_u64(row: &rusqlite::Row, idx: usize) -> u64 {
    row.get::<_, i64>(idx).map(from_i64).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::{
        compare_scans, create_scan_run, escape_like, finish_scan, initialize, insert_file_batch,
        latest_scan, list_insight_files, list_insights, list_large_files, list_scan_history, open,
        prune_old_scans, search_files,
    };
    use crate::models::{FileEntry, ScanStats, ScanStatus, SearchRequest, SearchSort};
    use std::fs;
    use uuid::Uuid;

    fn file(
        path: &str,
        name: &str,
        category: &str,
        size_bytes: u64,
        modified_at: Option<i64>,
    ) -> FileEntry {
        FileEntry {
            id: 0, // Test helper; real id comes from database
            path: path.to_owned(),
            name: name.to_owned(),
            extension: None,
            category: category.to_owned(),
            size_bytes,
            modified_at,
            is_hidden: false,
            content_hash: None,
        }
    }

    #[test]
    fn persists_and_queries_a_completed_scan() {
        let database_path =
            std::env::temp_dir().join(format!("luma-db-{}.sqlite3", Uuid::new_v4()));
        initialize(&database_path).expect("initialize database");
        create_scan_run(&database_path, "scan-1", "/fixture", 100).expect("create scan run");
        let mut connection = open(&database_path).expect("open database");
        let files = vec![
            FileEntry {
                id: 0,
                path: "/fixture/node_modules/package/archive.zip".to_owned(),
                name: "archive.zip".to_owned(),
                extension: Some("zip".to_owned()),
                category: "archives".to_owned(),
                size_bytes: 400,
                modified_at: Some(10),
                is_hidden: false,
                content_hash: None,
            },
            FileEntry {
                id: 0,
                path: "/fixture/installer.dmg".to_owned(),
                name: "installer.dmg".to_owned(),
                extension: Some("dmg".to_owned()),
                category: "applications".to_owned(),
                size_bytes: 200,
                modified_at: Some(90),
                is_hidden: false,
                content_hash: None,
            },
        ];
        insert_file_batch(&mut connection, "scan-1", &files).expect("insert files");
        finish_scan(
            &connection,
            "scan-1",
            ScanStatus::Completed,
            &ScanStats {
                files_scanned: 2,
                directories_scanned: 2,
                bytes_scanned: 600,
                errors: 0,
            },
            200,
        )
        .expect("finish scan");
        drop(connection);

        let summary = latest_scan(&database_path)
            .expect("query latest scan")
            .expect("completed scan exists");
        assert_eq!(summary.total_files, 2);
        assert_eq!(summary.total_bytes, 600);
        assert_eq!(summary.categories[0].size_bytes, 400);

        let large_files =
            list_large_files(&database_path, "scan-1", 20, 0).expect("query large files");
        assert_eq!(large_files[0].name, "archive.zip");

        let insights = list_insights(&database_path, "scan-1", 300, 50).expect("query insights");
        let kinds = insights
            .iter()
            .map(|insight| insight.kind.as_str())
            .collect::<Vec<_>>();
        assert!(kinds.contains(&"largeFiles"));
        assert!(kinds.contains(&"staleFiles"));
        assert!(kinds.contains(&"development"));
        assert!(kinds.contains(&"archives"));
        assert!(kinds.contains(&"installers"));

        fs::remove_file(database_path).expect("remove fixture database");
    }

    #[test]
    fn lists_insight_files_per_kind_ordered_by_size() {
        let database_path =
            std::env::temp_dir().join(format!("luma-insight-files-{}.sqlite3", Uuid::new_v4()));
        initialize(&database_path).expect("initialize database");
        create_scan_run(&database_path, "scan-1", "/fixture", 100).expect("create scan run");
        let mut connection = open(&database_path).expect("open database");
        let files = vec![
            file("/fixture/big-a.iso", "big-a.iso", "other", 5000, Some(1000)),
            file(
                "/fixture/big-b.mp4",
                "big-b.mp4",
                "videos",
                3000,
                Some(1000),
            ),
            file(
                "/fixture/node_modules/pkg/lib.js",
                "lib.js",
                "code",
                100,
                Some(1000),
            ),
            file("/fixture/old.txt", "old.txt", "documents", 50, Some(5)),
            file("/fixture/data.zip", "data.zip", "archives", 800, Some(1000)),
            file(
                "/fixture/app.dmg",
                "app.dmg",
                "applications",
                700,
                Some(1000),
            ),
        ];
        insert_file_batch(&mut connection, "scan-1", &files).expect("insert files");
        drop(connection);

        // Large files: only entries at or above the threshold, largest first.
        let large = list_insight_files(&database_path, "scan-1", "largeFiles", 1000, 100, 10)
            .expect("query large files");
        let large_names = large.iter().map(|f| f.name.as_str()).collect::<Vec<_>>();
        assert_eq!(large_names, vec!["big-a.iso", "big-b.mp4"]);

        // The limit caps the result while preserving the size-desc order.
        let capped = list_insight_files(&database_path, "scan-1", "largeFiles", 1000, 100, 1)
            .expect("query capped large files");
        assert_eq!(capped.len(), 1);
        assert_eq!(capped[0].name, "big-a.iso");

        // Each remaining predicate selects exactly its matching fixture file.
        let stale = list_insight_files(&database_path, "scan-1", "staleFiles", 1000, 100, 10)
            .expect("query stale files");
        assert_eq!(
            stale.iter().map(|f| f.name.as_str()).collect::<Vec<_>>(),
            vec!["old.txt"]
        );

        let development =
            list_insight_files(&database_path, "scan-1", "development", 1000, 100, 10)
                .expect("query development files");
        assert_eq!(
            development
                .iter()
                .map(|f| f.name.as_str())
                .collect::<Vec<_>>(),
            vec!["lib.js"]
        );

        let archives = list_insight_files(&database_path, "scan-1", "archives", 1000, 100, 10)
            .expect("query archive files");
        assert_eq!(
            archives.iter().map(|f| f.name.as_str()).collect::<Vec<_>>(),
            vec!["data.zip"]
        );

        let installers = list_insight_files(&database_path, "scan-1", "installers", 1000, 100, 10)
            .expect("query installer files");
        assert_eq!(
            installers
                .iter()
                .map(|f| f.name.as_str())
                .collect::<Vec<_>>(),
            vec!["app.dmg"]
        );

        // An unknown kind is rejected rather than silently returning nothing.
        let error = list_insight_files(&database_path, "scan-1", "mystery", 1000, 100, 10)
            .expect_err("unknown kind must error");
        assert_eq!(error.code, "INVALID_INSIGHT_KIND");

        fs::remove_file(database_path).expect("remove fixture database");
    }

    #[test]
    fn marks_interrupted_runs_failed_on_initialization() {
        let database_path =
            std::env::temp_dir().join(format!("luma-interrupted-{}.sqlite3", Uuid::new_v4()));
        initialize(&database_path).expect("initialize database");
        create_scan_run(&database_path, "interrupted", "/fixture", 100)
            .expect("create running scan");

        initialize(&database_path).expect("reinitialize database");
        let connection = open(&database_path).expect("open database");
        let status: String = connection
            .query_row(
                "SELECT status FROM scan_runs WHERE id = 'interrupted'",
                [],
                |row| row.get(0),
            )
            .expect("query interrupted scan");
        let version: u32 = connection
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .expect("query schema version");

        assert_eq!(status, "failed");
        assert_eq!(version, 3);
        drop(connection);
        fs::remove_file(database_path).expect("remove fixture database");
    }

    #[test]
    fn migrates_version_one_database_without_losing_files() {
        let database_path =
            std::env::temp_dir().join(format!("luma-v1-migration-{}.sqlite3", Uuid::new_v4()));
        let connection = open(&database_path).expect("open legacy database");
        connection
            .execute_batch(
                "
                CREATE TABLE scan_runs (
                  id TEXT PRIMARY KEY,
                  root_path TEXT NOT NULL,
                  status TEXT NOT NULL,
                  started_at INTEGER NOT NULL,
                  finished_at INTEGER,
                  total_files INTEGER NOT NULL DEFAULT 0,
                  total_directories INTEGER NOT NULL DEFAULT 0,
                  total_bytes INTEGER NOT NULL DEFAULT 0,
                  error_count INTEGER NOT NULL DEFAULT 0
                );
                CREATE TABLE files (
                  id INTEGER PRIMARY KEY AUTOINCREMENT,
                  scan_id TEXT NOT NULL,
                  path TEXT NOT NULL,
                  name TEXT NOT NULL,
                  extension TEXT,
                  category TEXT NOT NULL,
                  size_bytes INTEGER NOT NULL,
                  modified_at INTEGER,
                  is_hidden INTEGER NOT NULL DEFAULT 0,
                  FOREIGN KEY(scan_id) REFERENCES scan_runs(id) ON DELETE CASCADE
                );
                INSERT INTO scan_runs (
                  id, root_path, status, started_at, finished_at, total_files, total_bytes
                ) VALUES ('legacy', '/fixture', 'completed', 100, 200, 1, 42);
                INSERT INTO files (
                  scan_id, path, name, category, size_bytes
                ) VALUES ('legacy', '/fixture/keep.txt', 'keep.txt', 'documents', 42);
                PRAGMA user_version = 1;
                ",
            )
            .expect("create legacy schema");
        drop(connection);

        initialize(&database_path).expect("migrate legacy database");
        let connection = open(&database_path).expect("reopen migrated database");
        let version: u32 = connection
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .expect("query schema version");
        let columns = connection
            .prepare("PRAGMA table_info(files)")
            .expect("prepare column query")
            .query_map([], |row| row.get::<_, String>(1))
            .expect("query columns")
            .collect::<Result<Vec<_>, _>>()
            .expect("collect columns");
        let retained_path: String = connection
            .query_row(
                "SELECT path FROM files WHERE scan_id = 'legacy'",
                [],
                |row| row.get(0),
            )
            .expect("query retained file");

        assert_eq!(version, 3);
        assert!(columns.iter().any(|column| column == "content_hash"));
        let operation_table: String = connection
            .query_row(
                "SELECT name FROM sqlite_master WHERE type = 'table' AND name = 'file_operations'",
                [],
                |row| row.get(0),
            )
            .expect("query operation table");
        assert_eq!(operation_table, "file_operations");
        assert_eq!(retained_path, "/fixture/keep.txt");
        drop(connection);
        fs::remove_file(database_path).expect("remove fixture database");
    }

    #[test]
    fn retains_only_the_three_most_recent_terminal_runs() {
        let database_path =
            std::env::temp_dir().join(format!("luma-retention-{}.sqlite3", Uuid::new_v4()));
        initialize(&database_path).expect("initialize database");
        let connection = open(&database_path).expect("open database");
        for index in 0..5 {
            connection
                .execute(
                    "INSERT INTO scan_runs (id, root_path, status, started_at, finished_at)
                     VALUES (?1, '/fixture', 'cancelled', ?2, ?2)",
                    rusqlite::params![format!("scan-{index}"), index],
                )
                .expect("insert terminal scan");
        }

        prune_old_scans(&connection).expect("prune old scans");
        let remaining: i64 = connection
            .query_row("SELECT COUNT(*) FROM scan_runs", [], |row| row.get(0))
            .expect("count retained scans");
        assert_eq!(remaining, 3);
        drop(connection);
        fs::remove_file(database_path).expect("remove fixture database");
    }

    #[test]
    fn retains_history_per_root_path() {
        // Two directories each scanned four times: pruning must keep three runs
        // *per directory*, not three across the whole table.
        let database_path =
            std::env::temp_dir().join(format!("luma-per-root-{}.sqlite3", Uuid::new_v4()));
        initialize(&database_path).expect("initialize database");
        let connection = open(&database_path).expect("open database");
        for root in ["/alpha", "/beta"] {
            for index in 0..4 {
                connection
                    .execute(
                        "INSERT INTO scan_runs (id, root_path, status, started_at, finished_at)
                         VALUES (?1, ?2, 'completed', ?3, ?3)",
                        rusqlite::params![format!("{root}-{index}"), root, index],
                    )
                    .expect("insert terminal scan");
            }
        }

        prune_old_scans(&connection).expect("prune old scans");
        for root in ["/alpha", "/beta"] {
            let kept: i64 = connection
                .query_row(
                    "SELECT COUNT(*) FROM scan_runs WHERE root_path = ?1",
                    [root],
                    |row| row.get(0),
                )
                .expect("count retained scans");
            assert_eq!(kept, 3, "each root path keeps its three newest runs");
        }
        drop(connection);
        fs::remove_file(database_path).expect("remove fixture database");
    }

    #[test]
    fn compares_two_scans_of_the_same_directory() {
        let database_path =
            std::env::temp_dir().join(format!("luma-compare-{}.sqlite3", Uuid::new_v4()));
        initialize(&database_path).expect("initialize database");

        // Base scan: one 100-byte document.
        create_scan_run(&database_path, "base", "/dir", 10).expect("create base run");
        let mut connection = open(&database_path).expect("open database");
        insert_file_batch(
            &mut connection,
            "base",
            &[file("/dir/a.txt", "a.txt", "documents", 100, Some(10))],
        )
        .expect("insert base files");
        finish_scan(
            &connection,
            "base",
            ScanStatus::Completed,
            &ScanStats {
                files_scanned: 1,
                directories_scanned: 0,
                bytes_scanned: 100,
                errors: 0,
            },
            20,
        )
        .expect("finish base scan");

        // Target scan: the document grew and a new video appeared.
        create_scan_run(&database_path, "target", "/dir", 30).expect("create target run");
        insert_file_batch(
            &mut connection,
            "target",
            &[
                file("/dir/a.txt", "a.txt", "documents", 300, Some(30)),
                file("/dir/b.mp4", "b.mp4", "videos", 5000, Some(30)),
            ],
        )
        .expect("insert target files");
        finish_scan(
            &connection,
            "target",
            ScanStatus::Completed,
            &ScanStats {
                files_scanned: 2,
                directories_scanned: 0,
                bytes_scanned: 5300,
                errors: 0,
            },
            40,
        )
        .expect("finish target scan");
        drop(connection);

        // History lists both completed runs of /dir, newest first.
        let history = list_scan_history(&database_path, "target").expect("list history");
        assert_eq!(
            history
                .iter()
                .map(|s| s.scan_id.as_str())
                .collect::<Vec<_>>(),
            vec!["target", "base"]
        );

        let comparison = compare_scans(&database_path, "base", "target").expect("compare scans");
        assert_eq!(comparison.total_bytes_delta, 5200);
        assert_eq!(comparison.total_files_delta, 1);

        // The newly added video is the biggest mover, so it sorts first.
        let videos = comparison
            .categories
            .iter()
            .find(|c| c.category == "videos")
            .expect("videos delta present");
        assert_eq!(videos.size_delta, 5000);
        assert_eq!(videos.base_size_bytes, 0);
        assert_eq!(videos.target_size_bytes, 5000);

        let documents = comparison
            .categories
            .iter()
            .find(|c| c.category == "documents")
            .expect("documents delta present");
        assert_eq!(documents.size_delta, 200);
        assert_eq!(documents.file_count_delta, 0);

        fs::remove_file(database_path).expect("remove fixture database");
    }

    #[test]
    fn test_escape_like() {
        assert_eq!(escape_like("normal"), "normal");
        assert_eq!(escape_like("50%"), "50\\%");
        assert_eq!(escape_like("file_name"), "file\\_name");
        assert_eq!(escape_like("back\\slash"), "back\\\\slash");
        assert_eq!(escape_like("_%\\all"), "\\_\\%\\\\all");
    }

    #[test]
    fn test_search_files() {
        let database_path =
            std::env::temp_dir().join(format!("luma-search-{}.sqlite3", Uuid::new_v4()));
        initialize(&database_path).expect("initialize");
        let scan_id = "scan-search";
        create_scan_run(&database_path, scan_id, "/test", 1000).expect("create scan");

        let files = vec![
            file("docs/readme.md", "readme.md", "documents", 1024, Some(2000)),
            file("photos/beach.jpg", "beach.jpg", "images", 5000, Some(3000)),
            file(
                "photos/mountain.jpg",
                "mountain.jpg",
                "images",
                6000,
                Some(4000),
            ),
            file("videos/clip.mp4", "clip.mp4", "videos", 50000, Some(5000)),
            FileEntry {
                id: 0,
                path: ".hidden/secret.txt".to_owned(),
                name: "secret.txt".to_owned(),
                extension: Some("txt".to_owned()),
                category: "documents".to_owned(),
                size_bytes: 100,
                modified_at: Some(1500),
                is_hidden: true,
                content_hash: None,
            },
        ];
        let mut connection = open(&database_path).expect("open");
        insert_file_batch(&mut connection, scan_id, &files).expect("insert batch");
        finish_scan(
            &connection,
            scan_id,
            ScanStatus::Completed,
            &ScanStats {
                files_scanned: 5,
                directories_scanned: 3,
                bytes_scanned: 62124,
                errors: 0,
            },
            2000,
        )
        .expect("finish scan");

        // Basic query: name match
        let response = search_files(
            &database_path,
            &SearchRequest {
                scan_id: scan_id.to_owned(),
                query: "mountain".to_owned(),
                category: None,
                extension: None,
                min_size: None,
                max_size: None,
                modified_after: None,
                modified_before: None,
                include_hidden: false,
                sort: SearchSort::default(),
                limit: None,
                offset: None,
            },
        )
        .expect("search by name");
        assert_eq!(response.total, 1);
        assert_eq!(response.files.len(), 1);
        assert_eq!(response.files[0].name, "mountain.jpg");

        // Category filter
        let response = search_files(
            &database_path,
            &SearchRequest {
                scan_id: scan_id.to_owned(),
                query: String::new(),
                category: Some("images".to_owned()),
                extension: None,
                min_size: None,
                max_size: None,
                modified_after: None,
                modified_before: None,
                include_hidden: false,
                sort: SearchSort::NameAsc,
                limit: None,
                offset: None,
            },
        )
        .expect("search by category");
        assert_eq!(response.total, 2);
        assert_eq!(response.files[0].name, "beach.jpg");
        assert_eq!(response.files[1].name, "mountain.jpg");

        // Size range
        let response = search_files(
            &database_path,
            &SearchRequest {
                scan_id: scan_id.to_owned(),
                query: String::new(),
                category: None,
                extension: None,
                min_size: Some(5000),
                max_size: Some(10000),
                modified_after: None,
                modified_before: None,
                include_hidden: false,
                sort: SearchSort::SizeDesc,
                limit: None,
                offset: None,
            },
        )
        .expect("search by size");
        assert_eq!(response.total, 2);
        assert_eq!(response.files[0].size_bytes, 6000);
        assert_eq!(response.files[1].size_bytes, 5000);

        // Hidden exclusion (default)
        let response = search_files(
            &database_path,
            &SearchRequest {
                scan_id: scan_id.to_owned(),
                query: "secret".to_owned(),
                category: None,
                extension: None,
                min_size: None,
                max_size: None,
                modified_after: None,
                modified_before: None,
                include_hidden: false,
                sort: SearchSort::default(),
                limit: None,
                offset: None,
            },
        )
        .expect("search exclude hidden");
        assert_eq!(response.total, 0);

        // Include hidden
        let response = search_files(
            &database_path,
            &SearchRequest {
                scan_id: scan_id.to_owned(),
                query: "secret".to_owned(),
                category: None,
                extension: None,
                min_size: None,
                max_size: None,
                modified_after: None,
                modified_before: None,
                include_hidden: true,
                sort: SearchSort::default(),
                limit: None,
                offset: None,
            },
        )
        .expect("search include hidden");
        assert_eq!(response.total, 1);
        assert_eq!(response.files[0].name, "secret.txt");

        // Pagination
        let response = search_files(
            &database_path,
            &SearchRequest {
                scan_id: scan_id.to_owned(),
                query: String::new(),
                category: None,
                extension: None,
                min_size: None,
                max_size: None,
                modified_after: None,
                modified_before: None,
                include_hidden: false,
                sort: SearchSort::NameAsc,
                limit: Some(2),
                offset: Some(0),
            },
        )
        .expect("search page 1");
        assert_eq!(response.total, 4);
        assert_eq!(response.files.len(), 2);
        assert_eq!(response.limit, 2);
        assert_eq!(response.offset, 0);

        let response = search_files(
            &database_path,
            &SearchRequest {
                scan_id: scan_id.to_owned(),
                query: String::new(),
                category: None,
                extension: None,
                min_size: None,
                max_size: None,
                modified_after: None,
                modified_before: None,
                include_hidden: false,
                sort: SearchSort::NameAsc,
                limit: Some(2),
                offset: Some(2),
            },
        )
        .expect("search page 2");
        assert_eq!(response.total, 4);
        assert_eq!(response.files.len(), 2);

        // Windows keeps SQLite files locked while a connection is alive.
        drop(connection);
        fs::remove_file(database_path).expect("remove fixture database");
    }
}
