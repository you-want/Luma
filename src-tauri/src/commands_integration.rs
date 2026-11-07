// Command-layer integration tests.
//
// These drive the same module functions `commands.rs` invokes, through the
// root-injected seams the existing unit tests use, so the full validation and
// fail-closed behavior is covered end-to-end without spawning a Tauri app or
// touching the OS trash, `~/Library`, or real user data.
//
// Coverage:
//   - Disk cleaner: execution boundary with fake deleter, whitelist rejection,
//     out-of-scope path rejection, fail-closed when whitelist table is missing
//   - Project slimming: stale-project success path + active-project rejection
//   - Orphan app data: discovery + execution with an injected library root
//   - App slimming: per-item rejection of paths not in the discovery result
//   - Maintenance: closed-registry validation + real (harmless) QuickLook
//     cache refresh with operation logging
//   - Scan-root validation: empty and missing paths rejected before any I/O
//
// The real async command wrappers only add a worker-thread hop
// (`spawn_blocking`) and the process-wide `DATABASE_BUSY` guard (already
// covered in `commands.rs`'s own tests), so testing the underlying functions
// here is equivalent for logic coverage.

#[cfg(test)]
mod tests {
    use crate::cleaner::{
        add_whitelist_entry, execute_clean_with_roots, find_kind, list_operations, to_store_path,
        CleanOutcome, CleanRequestItem,
    };
    use crate::database;
    use crate::file_ops::OpFailure;
    use crate::maintenance::{run_task, task_report};
    use crate::orphans::{discover_in, execute_orphan_clean_in};
    use crate::purge::{discover_projects_in, execute_purge_with_roots};
    use crate::scanner::validate_root;
    use crate::slim::execute_slim;
    use std::cell::Cell;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::time::{Duration, Instant, SystemTime};
    use uuid::Uuid;
    use walkdir::WalkDir;

    /// Fixtures live under `target/` — `std::env::temp_dir()` on macOS is
    /// `/var/folders/…`, which `validate_write_path` rejects as a protected
    /// `/var` root, so nothing real can be trashed from here.
    fn temp_dir(tag: &str) -> PathBuf {
        let base = std::env::current_dir()
            .expect("cwd")
            .join(format!("target/luma-cmd-{}", tag));
        let dir = base.join(Uuid::new_v4().to_string());
        fs::create_dir_all(&dir).expect("create dir");
        dir
    }

    fn write_file(path: &Path, size: u64) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create parent");
        }
        use std::io::Write;
        let mut file = fs::File::create(path).expect("create file");
        file.write_all(&vec![0u8; size as usize])
            .expect("write zeros");
    }

    fn temp_db(tag: &str) -> (PathBuf, PathBuf) {
        let dir = temp_dir(tag);
        let db = dir.join("test.db");
        database::initialize(&db).expect("init db");
        (dir, db)
    }

    /// Stamp every fixture *file* 400 days into the past so the containing
    /// project falls outside the 7-day active window (directories cannot be
    /// opened for writing). Mirrors the existing purge test harness.
    fn make_stale(root: &Path) {
        for entry in WalkDir::new(root)
            .into_iter()
            .filter_map(|v| v.ok())
            .filter(|entry| entry.file_type().is_file())
        {
            let Ok(file) = fs::File::options().write(true).open(entry.path()) else {
                continue;
            };
            file.set_times(
                fs::FileTimes::new()
                    .set_modified(SystemTime::now() - Duration::from_secs(400 * 86_400)),
            )
            .unwrap();
        }
    }

    fn build_node_project(dir: &str, tag: &str, stale: bool) -> (PathBuf, PathBuf, PathBuf) {
        let dir = temp_dir(dir);
        let root = dir.join("Projects");
        let project = root.join(tag);
        write_file(&project.join("package.json"), 10);
        write_file(&project.join("node_modules").join("lib.js"), 500);
        if stale {
            make_stale(&project);
        }
        (dir, root, project)
    }

    /// A fake deleter that records each path handed to it (in a `Cell`)
    /// instead of moving it to the OS trash. `Cell` keeps the closure a
    /// plain `Fn` so it coerces to the seams' `&Deleter`.
    fn recording_deleter(
        recorded: &Cell<Vec<String>>,
    ) -> impl Fn(&Path) -> Result<(), String> + '_ {
        move |native: &Path| {
            let mut v = recorded.take();
            v.push(native.to_string_lossy().into_owned());
            recorded.set(v);
            Ok(())
        }
    }

    // ── Disk cleaner: execution boundary with injected roots ───────────────

    #[test]
    fn execute_clean_trashes_in_scope_units_and_logs_operation() {
        let (dir, db) = temp_db("cleaner-exec");
        let cache_root = dir.join("Caches");
        let target = cache_root.join("com.test.app");
        write_file(&target.join("file"), 200);

        let def = find_kind("userCaches").expect("kind defined");
        let roots = vec![to_store_path(&cache_root)];
        let recorded = Cell::new(Vec::new());
        let outcome: CleanOutcome = execute_clean_with_roots(
            &db,
            def,
            &roots,
            &[CleanRequestItem {
                path: to_store_path(&target),
                size_bytes: 200,
            }],
            &recording_deleter(&recorded),
        )
        .expect("execute");

        let recorded = recorded.take();
        assert_eq!(outcome.succeeded.len(), 1);
        assert!(outcome.operation_id.is_some());
        assert_eq!(recorded.len(), 1, "fake deleter ran exactly once");

        let ops = list_operations(&db, 5).expect("list ops");
        assert_eq!(ops.len(), 1);
    }

    #[test]
    fn execute_clean_rejects_whitelisted_paths() {
        let (dir, db) = temp_db("cleaner-whitelist");
        let cache_root = dir.join("Caches");
        let target = cache_root.join("com.protected.app");
        write_file(&target.join("f"), 100);

        let def = find_kind("userCaches").expect("kind");
        let roots = vec![to_store_path(&cache_root)];
        let protected = to_store_path(&target);
        add_whitelist_entry(&db, &protected).expect("add whitelist");

        let recorded = Cell::new(Vec::new());
        let outcome = execute_clean_with_roots(
            &db,
            def,
            &roots,
            &[CleanRequestItem {
                path: protected,
                size_bytes: 100,
            }],
            &recording_deleter(&recorded),
        )
        .expect("whitelist rejection is per-item, not fatal");

        assert!(outcome.succeeded.is_empty());
        assert_eq!(outcome.failed.len(), 1);
        assert!(
            outcome.failed[0].reason.contains("白名单"),
            "got {:?}",
            outcome.failed[0].reason
        );
        assert!(
            recorded.take().is_empty(),
            "deleter must not run for whitelisted path"
        );
    }

    #[test]
    fn execute_clean_fails_closed_when_whitelist_unreadable() {
        let (dir, db) = temp_db("cleaner-failclosed");
        let cache_root = dir.join("Caches");
        let target = cache_root.join("x");
        write_file(&target.join("f"), 50);

        // Simulate an unreadable/missing whitelist table.
        let conn = database::open(&db).expect("open");
        conn.execute_batch("DROP TABLE cleaner_whitelist;")
            .expect("drop table");
        drop(conn);

        let def = find_kind("userCaches").expect("kind");
        let recorded = Cell::new(Vec::new());
        let err = execute_clean_with_roots(
            &db,
            def,
            &[to_store_path(&cache_root)],
            &[CleanRequestItem {
                path: to_store_path(&target),
                size_bytes: 50,
            }],
            &recording_deleter(&recorded),
        )
        .expect_err("must refuse when whitelist unreadable");
        assert_eq!(err.code, "DATABASE_ERROR");
        assert!(recorded.take().is_empty());
    }

    #[test]
    fn execute_clean_rejects_paths_outside_injected_roots() {
        let (dir, db) = temp_db("cleaner-oots");
        let cache_root = dir.join("Caches");
        let outside = dir.join("somewhere-else");
        write_file(&outside.join("f"), 80);

        let def = find_kind("userCaches").expect("kind");
        let roots = vec![to_store_path(&cache_root)];
        let recorded = Cell::new(Vec::new());
        let outcome = execute_clean_with_roots(
            &db,
            def,
            &roots,
            &[CleanRequestItem {
                path: to_store_path(&outside),
                size_bytes: 80,
            }],
            &recording_deleter(&recorded),
        )
        .expect("out-of-scope rejection is per-item");
        assert!(outcome.succeeded.is_empty());
        assert!(
            outcome
                .failed
                .iter()
                .any(|f: &OpFailure| f.reason.contains("清理范围内")),
            "got {:?}",
            outcome.failed
        );
        assert!(recorded.take().is_empty());
    }

    // ── Project slimming: discovery + execution, injected roots ────────────

    #[test]
    fn purge_discovers_and_trashes_stale_fixture_artifact() {
        let (_dir, root, _project) = build_node_project("purge-exec", "myapp", true);
        let (_db_dir, db) = temp_db("purge-db");

        let report =
            discover_projects_in(vec![root.clone()], Instant::now() + Duration::from_secs(5))
                .expect("discover");
        let project = report.projects.first().expect("one fixture project");
        assert!(!project.is_active, "stamped project must be inactive");
        let artifact = project
            .artifacts
            .iter()
            .find(|a| a.group == "nodeModules")
            .expect("node_modules artifact")
            .clone();

        let moved_dir = temp_dir("purge-moved");
        let moved_to = moved_dir.join("moved");
        let outcome = execute_purge_with_roots(
            &db,
            std::slice::from_ref(&root),
            &[CleanRequestItem {
                path: artifact.path.clone(),
                size_bytes: artifact.size_bytes,
            }],
            &|native: &Path| std::fs::rename(native, &moved_to).map_err(|e| e.to_string()),
        )
        .expect("purge execute");
        assert_eq!(outcome.succeeded, vec![artifact.path]);
        assert!(moved_to.exists(), "artifact renamed by fake deleter");
    }

    #[test]
    fn purge_rejects_active_project_artifacts() {
        let (_dir, root, _project) = build_node_project("purge-active", "active", false);
        let (_db_dir, db) = temp_db("purge-active-db");

        let report =
            discover_projects_in(vec![root.clone()], Instant::now() + Duration::from_secs(5))
                .expect("discover");
        let active = report
            .projects
            .iter()
            .find(|p| p.is_active && !p.artifacts.is_empty())
            .expect("fresh fixture project must be marked active with an artifact");

        let recorded = Cell::new(Vec::new());
        let outcome = execute_purge_with_roots(
            &db,
            std::slice::from_ref(&root),
            &[CleanRequestItem {
                path: active.artifacts[0].path.clone(),
                size_bytes: active.artifacts[0].size_bytes,
            }],
            &recording_deleter(&recorded),
        )
        .expect("active rejection is per-item");
        assert!(outcome.succeeded.is_empty());
        assert!(
            outcome
                .failed
                .iter()
                .any(|f: &OpFailure| f.reason.contains("7 天")),
            "active-project message expected, got {:?}",
            outcome.failed
        );
        assert!(
            recorded.take().is_empty(),
            "deleter must not run for an active project"
        );
    }

    // ── Orphan app data: discovery + execution, injected library root ──────

    #[test]
    fn orphans_execute_trashes_verified_leftover() {
        let dir = temp_dir("orphans-exec");
        let library = dir.join("Library");
        let caches = library.join("Caches").join("com.removed.app");
        write_file(&caches.join("cache.bin"), 300);

        let (_db_dir, db) = temp_db("orphans-db");

        // No installed apps (empty app roots) → the leftover is a candidate.
        let report =
            discover_in(&library, &[], Instant::now() + Duration::from_secs(5)).expect("discover");
        assert!(
            report
                .items
                .iter()
                .any(|i| i.path.contains("com.removed.app")),
            "expected orphan candidate, got {:?}",
            report.items
        );

        let moved_to = dir.join("moved");
        let outcome = execute_orphan_clean_in(
            &db,
            Some(&library),
            &[],
            &[CleanRequestItem {
                path: to_store_path(&caches),
                size_bytes: 300,
            }],
            &|native: &Path| std::fs::rename(native, &moved_to).map_err(|e| e.to_string()),
        )
        .expect("execute");
        assert_eq!(outcome.succeeded.len(), 1);
        assert!(moved_to.exists());
    }

    // ── Maintenance: closed registry + fail-closed on unknown task ─────────

    #[test]
    fn maintenance_registry_is_closed_and_known_tasks_are_listed() {
        let report = task_report();
        let ids: Vec<&str> = report.tasks.iter().map(|t| t.id.as_str()).collect();
        assert!(ids.contains(&"quicklookCache"));
        assert!(ids.contains(&"dnsFlush"));
        assert_eq!(report.tasks.len(), 2);
    }

    #[test]
    fn maintenance_rejects_unknown_task() {
        let (_dir, db) = temp_db("maintenance-db");
        let err = run_task(&db, "not-a-task").expect_err("unknown task must fail");
        assert_eq!(err.code, "INVALID_MAINTENANCE_TASK");
    }

    #[test]
    fn maintenance_quicklook_cache_refresh_succeeds_on_macos() {
        // `qlmanage` is always present on macOS and needs no privileges.
        // This actually refreshes the QuickLook cache on the test machine,
        // which is harmless (it is exactly what the shipped feature does).
        let (_dir, db) = temp_db("maintenance-ql");
        let outcome = run_task(&db, "quicklookCache").expect("quicklook cache refresh");
        assert!(outcome.operation_id.is_some());
        assert!(outcome.detail.contains("QuickLook"));

        // The run is logged to the cleanup history.
        let ops = list_operations(&db, 5).expect("list ops");
        assert!(
            ops.iter().any(|op| op.label.contains("QuickLook")),
            "maintenance run should be logged, got {:?}",
            ops
        );
    }

    // ── Slim: execution boundary with an out-of-scope path ─────────────────

    #[test]
    fn slim_execute_rejects_paths_not_in_discovery_result() {
        let (_dir, db) = temp_db("slim-unknown");
        let outcome = execute_slim(
            &db,
            &[crate::slim::SlimRequestItem {
                path: "/not/a/real/app".into(),
                current_bytes: 100,
            }],
        )
        .expect("out-of-scope rejection is per-item, not fatal");
        assert!(outcome.succeeded.is_empty());
        assert!(
            outcome
                .failed
                .iter()
                .any(|f: &OpFailure| f.reason.contains("不在当前发现结果中")),
            "got {:?}",
            outcome.failed
        );
    }

    #[test]
    fn slim_execute_empty_request_is_no_op() {
        let (_dir, db) = temp_db("slim-db");
        let outcome = execute_slim(&db, &[]).expect("empty request");
        assert!(outcome.succeeded.is_empty());
        assert!(outcome.failed.is_empty());
    }

    // ── Scan-root validation ────────────────────────────────────────────────

    #[test]
    fn scanner_validate_root_rejects_empty_and_missing_paths() {
        let err = validate_root("").expect_err("empty root");
        assert_eq!(err.code, "INVALID_PATH");

        let dir = temp_dir("scanner-db");
        let missing = dir.join("does-not-exist");
        let err2 = validate_root(missing.to_string_lossy().as_ref()).expect_err("missing root");
        assert_eq!(err2.code, "INVALID_PATH");

        let valid = validate_root(dir.to_string_lossy().as_ref()).expect("valid root");
        assert_eq!(valid, dir);
    }

    /// Placeholder so this file is not empty even if feature-gated helpers
    /// shift; the real coverage above is the point.
    #[test]
    fn start_scan_request_shape_is_constructible() {
        let _req = crate::models::StartScanRequest {
            root_path: "/tmp".to_string(),
            include_hidden: false,
            stay_on_file_system: true,
        };
    }
}
