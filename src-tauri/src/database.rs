use crate::{
    error::AppError,
    models::{CategorySummary, FileEntry, InsightSummary, ScanStats, ScanStatus, ScanSummary},
};
use rusqlite::{params, Connection, OptionalExtension};
use std::path::Path;

pub fn initialize(path: &Path) -> Result<(), AppError> {
    let connection = open(path)?;
    let schema_version =
        connection.query_row("PRAGMA user_version", [], |row| row.get::<_, u32>(0))?;
    if schema_version > 2 {
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
        CREATE INDEX IF NOT EXISTS idx_files_size_hash ON files(scan_id, size_bytes, content_hash);

        UPDATE scan_runs
        SET status = 'failed', finished_at = unixepoch()
        WHERE status = 'running';
        ",
    )?;

    if schema_version < 2 {
        connection.execute("PRAGMA user_version = 2", [])?;
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
        "SELECT path, name, extension, category, size_bytes, modified_at, is_hidden, content_hash
         FROM files WHERE scan_id = ?1 ORDER BY size_bytes DESC LIMIT ?2 OFFSET ?3",
    )?;
    let rows = statement.query_map(params![scan_id, limit, offset], |row| {
        Ok(FileEntry {
            path: row.get(0)?,
            name: row.get(1)?,
            extension: row.get(2)?,
            category: row.get(3)?,
            size_bytes: from_i64(row.get(4)?),
            modified_at: row.get(5)?,
            is_hidden: row.get::<_, i64>(6)? != 0,
            content_hash: row.get(7)?,
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
        "development" => (
            "(path LIKE '%/node_modules/%' OR path LIKE '%/target/%' \
             OR path LIKE '%/dist/%' OR path LIKE '%/.next/%')",
            None,
        ),
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
        "SELECT path, name, extension, category, size_bytes, modified_at, is_hidden, content_hash
         FROM files WHERE scan_id = ?1 AND {predicate}
         ORDER BY size_bytes DESC LIMIT {limit}"
    );
    let mut statement = connection.prepare(&sql)?;
    let map_row = |row: &rusqlite::Row| {
        Ok(FileEntry {
            path: row.get(0)?,
            name: row.get(1)?,
            extension: row.get(2)?,
            category: row.get(3)?,
            size_bytes: from_i64(row.get(4)?),
            modified_at: row.get(5)?,
            is_hidden: row.get::<_, i64>(6)? != 0,
            content_hash: row.get(7)?,
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
    stale_days: u32,
) -> Result<Vec<InsightSummary>, AppError> {
    let connection = open(path)?;
    let mut insights = Vec::new();

    push_insight(
        &mut insights,
        "largeFiles",
        aggregate(
            &connection,
            "SELECT COUNT(*), COALESCE(SUM(size_bytes), 0) FROM files
             WHERE scan_id = ?1 AND size_bytes >= ?2",
            params![scan_id, to_i64(large_file_threshold)],
        )?,
        format!("判定依据：单个文件至少 {} 字节。", large_file_threshold),
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
        format!("判定依据：修改时间超过 {stale_days} 天。"),
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
        "判定依据：路径位于 node_modules、target、dist 或 .next 中。".to_owned(),
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
        "判定依据：文件扩展名属于常见压缩格式。".to_owned(),
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
        "判定依据：文件扩展名属于应用或安装包格式。".to_owned(),
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
    basis: String,
) {
    if file_count > 0 {
        insights.push(InsightSummary {
            kind: kind.to_owned(),
            file_count,
            size_bytes,
            basis,
        });
    }
}

pub fn prune_old_scans(connection: &Connection) -> Result<(), AppError> {
    connection.execute(
        "DELETE FROM scan_runs WHERE id IN (
           SELECT id FROM scan_runs WHERE status != 'running'
           ORDER BY COALESCE(finished_at, started_at) DESC LIMIT -1 OFFSET 3
         )",
        [],
    )?;
    Ok(())
}

fn parse_status(value: &str) -> ScanStatus {
    match value {
        "completed" => ScanStatus::Completed,
        "cancelled" => ScanStatus::Cancelled,
        "failed" => ScanStatus::Failed,
        _ => ScanStatus::Running,
    }
}

fn to_i64(value: u64) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

fn from_i64(value: i64) -> u64 {
    u64::try_from(value).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::{
        create_scan_run, finish_scan, initialize, insert_file_batch, latest_scan, list_insight_files,
        list_insights, list_large_files, open, prune_old_scans,
    };
    use crate::models::{FileEntry, ScanStats, ScanStatus};
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

        let insights =
            list_insights(&database_path, "scan-1", 300, 50, 180).expect("query insights");
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
            file("/fixture/big-b.mp4", "big-b.mp4", "videos", 3000, Some(1000)),
            file(
                "/fixture/node_modules/pkg/lib.js",
                "lib.js",
                "code",
                100,
                Some(1000),
            ),
            file("/fixture/old.txt", "old.txt", "documents", 50, Some(5)),
            file("/fixture/data.zip", "data.zip", "archives", 800, Some(1000)),
            file("/fixture/app.dmg", "app.dmg", "applications", 700, Some(1000)),
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
        assert_eq!(stale.iter().map(|f| f.name.as_str()).collect::<Vec<_>>(), vec!["old.txt"]);

        let development =
            list_insight_files(&database_path, "scan-1", "development", 1000, 100, 10)
                .expect("query development files");
        assert_eq!(
            development.iter().map(|f| f.name.as_str()).collect::<Vec<_>>(),
            vec!["lib.js"]
        );

        let archives = list_insight_files(&database_path, "scan-1", "archives", 1000, 100, 10)
            .expect("query archive files");
        assert_eq!(archives.iter().map(|f| f.name.as_str()).collect::<Vec<_>>(), vec!["data.zip"]);

        let installers = list_insight_files(&database_path, "scan-1", "installers", 1000, 100, 10)
            .expect("query installer files");
        assert_eq!(
            installers.iter().map(|f| f.name.as_str()).collect::<Vec<_>>(),
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
        assert_eq!(version, 2);
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
}
