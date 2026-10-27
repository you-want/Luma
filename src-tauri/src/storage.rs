//! Bounded, rebuildable scan data. Undo records are deliberately never pruned here.
use crate::{database, error::AppError};
use rusqlite::{params, Connection};
use serde::Serialize;
use std::{path::Path, time::Duration};

pub const SCAN_BUDGET_BYTES: u64 = 256 * 1024 * 1024;
const COMPACT_MIN_BYTES: u64 = 8 * 1024 * 1024;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageUsage {
    pub database_bytes: u64,
    pub journal_bytes: u64,
    pub reclaimable_bytes: u64,
    pub scan_count: u64,
    pub indexed_files: u64,
    pub undo_records: u64,
    pub scan_budget_bytes: u64,
    pub maintenance_error: Option<String>,
}

pub fn used_bytes(connection: &Connection) -> Result<u64, AppError> {
    let pages: u64 = connection.query_row("PRAGMA page_count", [], |r| r.get(0))?;
    let free: u64 = connection.query_row("PRAGMA freelist_count", [], |r| r.get(0))?;
    let size: u64 = connection.query_row("PRAGMA page_size", [], |r| r.get(0))?;
    Ok(pages.saturating_sub(free) * size)
}

pub fn usage(path: &Path) -> Result<StorageUsage, AppError> {
    let connection = database::open(path)?;
    let free: u64 = connection.query_row("PRAGMA freelist_count", [], |r| r.get(0))?;
    let size: u64 = connection.query_row("PRAGMA page_size", [], |r| r.get(0))?;
    let journal = path.with_file_name(format!(
        "{}-wal",
        path.file_name().unwrap_or_default().to_string_lossy()
    ));
    Ok(StorageUsage {
        database_bytes: std::fs::metadata(path)?.len(),
        journal_bytes: std::fs::metadata(journal).map(|s| s.len()).unwrap_or(0),
        reclaimable_bytes: free * size,
        scan_count: connection.query_row("SELECT COUNT(*) FROM scan_runs", [], |r| r.get(0))?,
        indexed_files: connection.query_row("SELECT COUNT(*) FROM files", [], |r| r.get(0))?,
        undo_records: connection.query_row("SELECT COUNT(*) FROM file_operations WHERE undo_json IS NOT NULL AND status IN ('completed','partial','interrupted','undo_failed')", [], |r| r.get(0))?,
        scan_budget_bytes: SCAN_BUDGET_BYTES,
        maintenance_error: None,
    })
}

/// Keep only the latest successful full index. Previous completed scans retain
/// their category/totals summary, so comparisons need no duplicate file listings.
pub fn retain(connection: &mut Connection) -> Result<(), AppError> {
    let transaction = connection.transaction()?;
    let latest: Option<String> = transaction.query_row(
        "SELECT id FROM scan_runs WHERE status = 'completed' ORDER BY finished_at DESC, started_at DESC, rowid DESC LIMIT 1",
        [], |r| r.get(0)).optional()?;
    let ids = {
        let mut statement = transaction.prepare(
            "SELECT id FROM scan_runs WHERE status = 'completed' AND id != COALESCE(?1, '') AND id NOT IN (SELECT scan_id FROM archived_scan_summaries)")?;
        let ids = statement
            .query_map([&latest], |r| r.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        ids
    };
    for id in ids {
        let summary = database::scan_summary_with_connection(&transaction, &id)?;
        transaction.execute(
            "INSERT INTO archived_scan_summaries(scan_id, summary_json) VALUES (?1, ?2)",
            params![
                id,
                serde_json::to_string(&summary)
                    .map_err(|e| AppError::new("DATABASE_ERROR", e.to_string()))?
            ],
        )?;
    }
    let removed_files = transaction.execute("DELETE FROM files WHERE scan_id IN (SELECT scan_id FROM archived_scan_summaries) OR scan_id IN (SELECT id FROM scan_runs WHERE status IN ('cancelled','failed'))", [])?;
    database::prune_old_scans(&transaction)?;
    transaction.execute("DELETE FROM scan_runs WHERE id IN (SELECT id FROM scan_runs WHERE status != 'running' AND id != COALESCE(?1, '') ORDER BY COALESCE(finished_at, started_at) DESC, rowid DESC LIMIT -1 OFFSET ?2)", params![latest, if latest.is_some() {29} else {30}])?;
    transaction.commit()?;
    if removed_files > 0 {
        connection.execute("INSERT INTO files_fts(files_fts) VALUES ('optimize')", [])?;
    }
    Ok(())
}

use rusqlite::OptionalExtension;

pub fn compact(connection: &Connection, force: bool) -> Result<(), AppError> {
    connection.busy_timeout(Duration::from_secs(5))?;
    let pages: u64 = connection.query_row("PRAGMA page_count", [], |r| r.get(0))?;
    let free: u64 = connection.query_row("PRAGMA freelist_count", [], |r| r.get(0))?;
    let size: u64 = connection.query_row("PRAGMA page_size", [], |r| r.get(0))?;
    if force || (free * size >= COMPACT_MIN_BYTES && free * 4 >= pages) {
        // VACUUM is atomic; a failed compaction never just deletes the DB file.
        connection.execute_batch(
            "PRAGMA wal_checkpoint(TRUNCATE); VACUUM; PRAGMA wal_checkpoint(TRUNCATE);",
        )?;
    }
    Ok(())
}

pub fn maintain(path: &Path) -> Result<(), AppError> {
    let mut connection = database::open(path)?;
    retain(&mut connection)?;
    // Reclaim on the worker thread while scans/file writes are excluded. Failure
    // leaves reusable free pages visible in the storage panel for a manual retry.
    if let Err(error) = compact(&connection, false) {
        eprintln!("Luma index compaction: {error}");
    }
    Ok(())
}

pub fn clear(path: &Path) -> Result<StorageUsage, AppError> {
    let mut connection = database::open(path)?;
    let transaction = connection.transaction()?;
    // file_operations has no cascading FK to scans: undo survives and becomes
    // available again after rescanning the same root.
    transaction.execute("DELETE FROM scan_runs", [])?;
    // The external-content FTS table may retain deleted segments until merge.
    // With no scan rows left, discard those segments as well before VACUUM.
    transaction.execute("INSERT INTO files_fts(files_fts) VALUES ('delete-all')", [])?;
    transaction.commit()?;
    let compact_error = compact(&connection, true).err().map(|e| e.to_string());
    drop(connection);
    let mut result = usage(path)?;
    result.maintenance_error = compact_error;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        file_ops,
        models::{FileEntry, ScanStats, ScanStatus},
    };
    use uuid::Uuid;

    fn canonical(path: &Path) -> String {
        let value = path.to_string_lossy().into_owned();
        if cfg!(windows) {
            value.replace('\\', "/")
        } else {
            value
        }
    }

    struct Fixture {
        root: std::path::PathBuf,
        db: std::path::PathBuf,
    }
    impl Fixture {
        fn new() -> Self {
            let root = std::env::current_dir()
                .unwrap()
                .join("target")
                .join(format!("luma-storage-{}", Uuid::new_v4()));
            std::fs::create_dir_all(&root).unwrap();
            let db = root.join("index.sqlite3");
            database::initialize(&db).unwrap();
            Self { root, db }
        }
        fn scan(&self, id: &str, index: i64) {
            database::create_scan_run(&self.db, id, &canonical(&self.root), index).unwrap();
            let mut connection = database::open(&self.db).unwrap();
            database::insert_file_batch(
                &mut connection,
                id,
                &[FileEntry {
                    id: 0,
                    path: canonical(&self.root.join("original.txt")),
                    name: "original.txt".into(),
                    extension: Some("txt".into()),
                    category: "documents".into(),
                    size_bytes: 5,
                    modified_at: std::fs::metadata(self.root.join("original.txt"))
                        .ok()
                        .and_then(|m| m.modified().ok())
                        .and_then(|m| m.duration_since(std::time::UNIX_EPOCH).ok())
                        .map(|d| d.as_secs() as i64),
                    is_hidden: false,
                    content_hash: None,
                }],
            )
            .unwrap();
            database::finish_scan(
                &connection,
                id,
                ScanStatus::Completed,
                &ScanStats::default(),
                index,
            )
            .unwrap();
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn archives_file_lists_but_preserves_history_comparison_and_latest_index() {
        let f = Fixture::new();
        f.scan("first", 1);
        f.scan("second", 2);
        let before = database::compare_scans(&f.db, "first", "second").unwrap();
        maintain(&f.db).unwrap();
        let connection = database::open(&f.db).unwrap();
        let old: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM files WHERE scan_id='first'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(old, 0);
        assert_eq!(usage(&f.db).unwrap().indexed_files, 1);
        let after = database::compare_scans(&f.db, "first", "second").unwrap();
        assert_eq!(before.base.total_files, after.base.total_files);
        assert_eq!(before.base.total_bytes, after.base.total_bytes);
        assert_eq!(after.base.categories[0].size_bytes, 5);
        assert_eq!(after.total_bytes_delta, 0);
        // Reopening the migrated DB does not rebuild or lose archived summaries.
        database::initialize(&f.db).unwrap();
        assert_eq!(
            database::list_scan_history(&f.db, "second").unwrap().len(),
            2
        );
    }

    #[test]
    fn clearing_keeps_original_files_and_undo_works_after_rescanning() {
        let f = Fixture::new();
        std::fs::write(f.root.join("original.txt"), b"hello").unwrap();
        f.scan("before", 1);
        let mut connection = database::open(&f.db).unwrap();
        let extra = (0..2000)
            .map(|i| FileEntry {
                id: 0,
                path: format!("/fixture/{i}/{}", "long-path".repeat(200)),
                name: format!("record-{i}.txt"),
                extension: Some("txt".into()),
                category: "documents".into(),
                size_bytes: 1,
                modified_at: None,
                is_hidden: false,
                content_hash: None,
            })
            .collect::<Vec<_>>();
        database::insert_file_batch(&mut connection, "before", &extra).unwrap();
        drop(connection);
        let renamed = file_ops::rename_file(
            &f.db,
            "before",
            &canonical(&f.root.join("original.txt")),
            "renamed.txt",
        )
        .unwrap();
        let usage = clear(&f.db).unwrap();
        assert_eq!(usage.scan_count, 0);
        assert_eq!(usage.indexed_files, 0);
        assert_eq!(usage.undo_records, 1);
        assert!(usage.database_bytes + usage.journal_bytes < 1024 * 1024);
        assert_eq!(std::fs::read(f.root.join("renamed.txt")).unwrap(), b"hello");
        assert!(database::latest_scan(&f.db).unwrap().is_none());
        database::create_scan_run(&f.db, "after", &canonical(&f.root), 2).unwrap();
        let mut connection = database::open(&f.db).unwrap();
        let outcome = crate::scanner::scan_directory(
            &crate::models::StartScanRequest {
                root_path: f.root.to_string_lossy().into_owned(),
                include_hidden: false,
                stay_on_file_system: true,
            },
            "after",
            std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
            |files| database::insert_file_batch(&mut connection, "after", &files),
            |_| {},
        )
        .unwrap();
        database::finish_scan(&connection, "after", outcome.status, &outcome.stats, 3).unwrap();
        assert_eq!(
            file_ops::list_undoable_operations(&f.db, "after", 20)
                .unwrap()
                .len(),
            1
        );
        assert!(
            file_ops::undo_operation(&f.db, &renamed.operation_id, "after")
                .unwrap()
                .failed
                .is_empty()
        );
        assert_eq!(
            std::fs::read(f.root.join("original.txt")).unwrap(),
            b"hello"
        );
    }

    #[test]
    fn budget_rolls_back_batch_and_partial_failure_keeps_previous_success() {
        let f = Fixture::new();
        f.scan("good", 1);
        let mut connection = database::open(&f.db).unwrap();
        database::create_scan_run(&f.db, "oversize", &canonical(&f.root), 2).unwrap();
        database::insert_file_batch(
            &mut connection,
            "oversize",
            &[FileEntry {
                id: 0,
                path: "/partial.txt".into(),
                name: "partial.txt".into(),
                extension: Some("txt".into()),
                category: "documents".into(),
                size_bytes: 1,
                modified_at: None,
                is_hidden: false,
                content_hash: None,
            }],
        )
        .unwrap();
        let limit = used_bytes(&connection).unwrap();
        let files = (0..500)
            .map(|i| FileEntry {
                id: 0,
                path: format!("/{i}/{}", "a".repeat(1000)),
                name: format!("{i}.txt"),
                extension: Some("txt".into()),
                category: "documents".into(),
                size_bytes: 1,
                modified_at: None,
                is_hidden: false,
                content_hash: None,
            })
            .collect::<Vec<_>>();
        let error =
            database::insert_file_batch_with_limit(&mut connection, "oversize", &files, limit)
                .unwrap_err();
        assert_eq!(error.code, "INDEX_LIMIT");
        assert_eq!(
            connection
                .query_row(
                    "SELECT COUNT(*) FROM files WHERE scan_id='oversize'",
                    [],
                    |r| r.get::<_, i64>(0)
                )
                .unwrap(),
            1
        );
        database::mark_scan_failed(&f.db, "oversize", 3).unwrap();
        // Repeated cancellations must not evict the last successful scan.
        for i in 4..9 {
            database::create_scan_run(&f.db, &format!("cancel-{i}"), &canonical(&f.root), i)
                .unwrap();
            database::finish_scan(
                &connection,
                &format!("cancel-{i}"),
                ScanStatus::Cancelled,
                &ScanStats::default(),
                i,
            )
            .unwrap();
        }
        maintain(&f.db).unwrap();
        assert_eq!(
            database::latest_scan(&f.db).unwrap().unwrap().scan_id,
            "good"
        );
        assert_eq!(usage(&f.db).unwrap().indexed_files, 1);
        assert_eq!(
            connection
                .query_row(
                    "SELECT COUNT(*) FROM files WHERE scan_id='oversize'",
                    [],
                    |r| r.get::<_, i64>(0)
                )
                .unwrap(),
            0
        );
    }

    #[test]
    fn global_history_is_bounded_and_compaction_reclaims_disk_space() {
        let f = Fixture::new();
        for i in 0..35 {
            let id = format!("scan-{i}");
            database::create_scan_run(&f.db, &id, &format!("/root/{i}"), i).unwrap();
            let connection = database::open(&f.db).unwrap();
            database::finish_scan(
                &connection,
                &id,
                ScanStatus::Completed,
                &ScanStats::default(),
                i,
            )
            .unwrap();
        }
        let mut connection = database::open(&f.db).unwrap();
        // Model an existing bloated database, using an isolated test table.
        connection.execute_batch("CREATE TABLE ballast(data BLOB); INSERT INTO ballast VALUES (zeroblob(12582912)); DROP TABLE ballast;").unwrap();
        retain(&mut connection).unwrap();
        assert_eq!(usage(&f.db).unwrap().scan_count, 30);
        assert_eq!(
            database::latest_scan(&f.db).unwrap().unwrap().scan_id,
            "scan-34"
        );
        let before = usage(&f.db).unwrap();
        assert!(before.reclaimable_bytes >= 12 * 1024 * 1024);
        compact(&connection, false).unwrap();
        let after = usage(&f.db).unwrap();
        assert!(
            after.database_bytes + after.journal_bytes
                < before.database_bytes + before.journal_bytes
        );
        assert_eq!(after.reclaimable_bytes, 0);
    }
}
