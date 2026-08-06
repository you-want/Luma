use crate::{
    error::AppError,
    models::{FileEntry, ScanProgress, ScanStats, ScanStatus, StartScanRequest},
};
use std::{
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use walkdir::{DirEntry, WalkDir};

const BATCH_SIZE: usize = 500;
const PROGRESS_INTERVAL: Duration = Duration::from_millis(100);

pub struct ScanOutcome {
    pub status: ScanStatus,
    pub stats: ScanStats,
}

pub fn validate_root(path: &str) -> Result<PathBuf, AppError> {
    if path.trim().is_empty() {
        return Err(AppError::invalid_path("请选择一个要扫描的目录。"));
    }
    let root = PathBuf::from(path);
    let metadata = std::fs::metadata(&root).map_err(|error| {
        if error.kind() == std::io::ErrorKind::PermissionDenied {
            AppError::new("PERMISSION_DENIED", "没有权限读取所选目录。")
        } else {
            AppError::invalid_path("所选目录不存在或无法访问。")
        }
    })?;
    if !metadata.is_dir() {
        return Err(AppError::invalid_path("所选路径不是目录。"));
    }
    Ok(root)
}

pub fn scan_directory<Batch, Progress>(
    request: &StartScanRequest,
    scan_id: &str,
    cancel: Arc<AtomicBool>,
    mut write_batch: Batch,
    mut emit_progress: Progress,
) -> Result<ScanOutcome, AppError>
where
    Batch: FnMut(Vec<FileEntry>) -> Result<(), AppError>,
    Progress: FnMut(ScanProgress),
{
    let root = validate_root(&request.root_path)?;
    let mut walker = WalkDir::new(&root).follow_links(false);
    if request.stay_on_file_system {
        walker = walker.same_file_system(true);
    }
    let include_hidden = request.include_hidden;
    let root_for_filter = root.clone();
    let entries = walker.into_iter().filter_entry(move |entry| {
        include_hidden || entry.path() == root_for_filter || !is_hidden(entry, &root_for_filter)
    });

    let mut stats = ScanStats::default();
    let mut batch = Vec::with_capacity(BATCH_SIZE);
    let mut last_progress = Instant::now() - PROGRESS_INTERVAL;
    let mut current_path = None;

    for result in entries {
        if cancel.load(Ordering::Relaxed) {
            if !batch.is_empty() {
                write_batch(std::mem::take(&mut batch))?;
            }
            emit_progress(progress(
                scan_id,
                ScanStatus::Cancelled,
                &stats,
                current_path,
            ));
            return Ok(ScanOutcome {
                status: ScanStatus::Cancelled,
                stats,
            });
        }

        let entry = match result {
            Ok(entry) => entry,
            Err(error) => {
                stats.errors += 1;
                current_path = error.path().map(|path| path.to_string_lossy().into_owned());
                maybe_emit(
                    scan_id,
                    &stats,
                    &current_path,
                    &mut last_progress,
                    &mut emit_progress,
                );
                continue;
            }
        };
        current_path = Some(entry.path().to_string_lossy().into_owned());

        if entry.depth() == 0 || entry.file_type().is_symlink() {
            continue;
        }
        if entry.file_type().is_dir() {
            stats.directories_scanned += 1;
        } else if entry.file_type().is_file() {
            match file_entry(&entry, &root) {
                Ok(file) => {
                    stats.files_scanned += 1;
                    stats.bytes_scanned = stats.bytes_scanned.saturating_add(file.size_bytes);
                    batch.push(file);
                    if batch.len() >= BATCH_SIZE {
                        write_batch(std::mem::take(&mut batch))?;
                        batch = Vec::with_capacity(BATCH_SIZE);
                    }
                }
                Err(_) => stats.errors += 1,
            }
        }
        maybe_emit(
            scan_id,
            &stats,
            &current_path,
            &mut last_progress,
            &mut emit_progress,
        );
    }

    if !batch.is_empty() {
        write_batch(batch)?;
    }
    emit_progress(progress(
        scan_id,
        ScanStatus::Completed,
        &stats,
        current_path,
    ));
    Ok(ScanOutcome {
        status: ScanStatus::Completed,
        stats,
    })
}

fn maybe_emit<Progress: FnMut(ScanProgress)>(
    scan_id: &str,
    stats: &ScanStats,
    current_path: &Option<String>,
    last_progress: &mut Instant,
    emit_progress: &mut Progress,
) {
    if last_progress.elapsed() >= PROGRESS_INTERVAL {
        emit_progress(progress(
            scan_id,
            ScanStatus::Running,
            stats,
            current_path.clone(),
        ));
        *last_progress = Instant::now();
    }
}

fn progress(
    scan_id: &str,
    status: ScanStatus,
    stats: &ScanStats,
    current_path: Option<String>,
) -> ScanProgress {
    ScanProgress {
        scan_id: scan_id.to_owned(),
        status,
        files_scanned: stats.files_scanned,
        directories_scanned: stats.directories_scanned,
        bytes_scanned: stats.bytes_scanned,
        errors: stats.errors,
        current_path,
    }
}

fn file_entry(entry: &DirEntry, root: &Path) -> Result<FileEntry, AppError> {
    let metadata = entry
        .metadata()
        .map_err(|error| AppError::new("SCAN_FAILED", error.to_string()))?;
    let path = entry.path();
    let name = entry.file_name().to_string_lossy().into_owned();
    let extension = path
        .extension()
        .map(|value| value.to_string_lossy().to_ascii_lowercase());
    let modified_at = metadata.modified().ok().and_then(system_time_seconds);

    Ok(FileEntry {
        id: 0, // Assigned by database on insert
        // Hidden = the Unix dotfile convention (cross-platform, since dev tools
        // use dotfiles on Windows too) OR the Windows hidden/system attribute.
        // Runs on the native `path`; the stored string is separator-normalized
        // so all downstream SQL uses a single `/` form.
        is_hidden: has_hidden_attribute(&metadata) || is_hidden_path(path, root),
        path: normalize_separators(path.to_string_lossy().into_owned()),
        name,
        category: classify_extension(extension.as_deref()).to_owned(),
        extension,
        size_bytes: metadata.len(),
        modified_at,
        content_hash: None,
    })
}

/// Canonicalize a path string for storage so every downstream SQL query and
/// name-extraction uses a single separator (`/`). On Windows `walkdir` yields
/// `\` separators; rewriting them to `/` makes `LIKE '%/target/%'`-style
/// queries and `rsplit('/')` name extraction behave identically on both
/// platforms. On Unix `\` is a legal filename character, so paths are stored
/// verbatim.
#[cfg(windows)]
fn normalize_separators(path: String) -> String {
    path.replace('\\', "/")
}

#[cfg(not(windows))]
fn normalize_separators(path: String) -> String {
    path
}

fn system_time_seconds(time: SystemTime) -> Option<i64> {
    time.duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|duration| i64::try_from(duration.as_secs()).ok())
}

fn is_hidden(entry: &DirEntry, root: &Path) -> bool {
    // Attribute lookup needs metadata; if it is unavailable, fall back to the
    // path-based dotfile rule alone rather than failing the whole entry.
    let attribute_hidden = entry
        .metadata()
        .map(|metadata| has_hidden_attribute(&metadata))
        .unwrap_or(false);
    attribute_hidden || is_hidden_path(entry.path(), root)
}

fn is_hidden_path(path: &Path, root: &Path) -> bool {
    path.strip_prefix(root)
        .unwrap_or(path)
        .components()
        .any(|component| component.as_os_str().to_string_lossy().starts_with('.'))
}

/// Whether the filesystem marks this entry hidden or system. This is the native
/// Windows notion of "hidden" (`FILE_ATTRIBUTE_HIDDEN` / `FILE_ATTRIBUTE_SYSTEM`);
/// Unix has no such attribute, so it is always `false` there and only the
/// dotfile convention applies.
#[cfg(windows)]
fn has_hidden_attribute(metadata: &std::fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    const FILE_ATTRIBUTE_HIDDEN: u32 = 0x2;
    const FILE_ATTRIBUTE_SYSTEM: u32 = 0x4;
    metadata.file_attributes() & (FILE_ATTRIBUTE_HIDDEN | FILE_ATTRIBUTE_SYSTEM) != 0
}

#[cfg(not(windows))]
fn has_hidden_attribute(_metadata: &std::fs::Metadata) -> bool {
    false
}

pub fn classify_extension(extension: Option<&str>) -> &'static str {
    match extension.unwrap_or_default().to_ascii_lowercase().as_str() {
        "jpg" | "jpeg" | "png" | "gif" | "webp" | "heic" | "tif" | "tiff" | "svg" => "images",
        "mp4" | "mov" | "mkv" | "avi" | "webm" | "m4v" => "videos",
        "mp3" | "wav" | "flac" | "aac" | "m4a" | "ogg" => "audio",
        "pdf" | "doc" | "docx" | "xls" | "xlsx" | "ppt" | "pptx" | "txt" | "md" | "rtf"
        | "pages" | "numbers" | "key" => "documents",
        "zip" | "rar" | "7z" | "tar" | "gz" | "bz2" | "xz" => "archives",
        "rs" | "ts" | "tsx" | "js" | "jsx" | "vue" | "py" | "go" | "java" | "kt" | "swift"
        | "c" | "cc" | "cpp" | "h" | "css" | "scss" | "html" | "json" | "toml" | "yaml" | "yml" => {
            "code"
        }
        "app" | "dmg" | "pkg" | "iso" | "exe" | "msi" | "apk" => "applications",
        _ => "other",
    }
}

#[cfg(test)]
mod tests {
    use super::{classify_extension, normalize_separators, scan_directory};
    use crate::models::{ScanStatus, StartScanRequest};
    use crate::{
        database::{
            compare_scans, create_scan_run, finish_scan, initialize, insert_file_batch,
            list_scan_history, open,
        },
        duplicates::find_duplicate_candidates,
        projects::identify_projects,
    };
    use std::{
        fs,
        sync::{atomic::AtomicBool, Arc},
    };
    use uuid::Uuid;

    #[test]
    fn scans_real_fixture_for_history_duplicates_and_projects() {
        let root = std::env::temp_dir().join(format!("luma-regression-{}", Uuid::new_v4()));
        let database_path = root.with_extension("sqlite3");
        fs::create_dir_all(root.join("sample-project/node_modules/demo")).expect("create fixture");
        fs::create_dir_all(root.join("sample-project/src")).expect("create source directory");
        fs::write(
            root.join("sample-project/package.json"),
            b"{\"name\":\"sample\"}",
        )
        .expect("write package");
        fs::write(
            root.join("sample-project/node_modules/demo/index.js"),
            b"module",
        )
        .expect("write module");
        fs::write(root.join("sample-project/src/copy-a.txt"), b"same content")
            .expect("write copy a");
        fs::write(root.join("sample-project/src/copy-b.txt"), b"same content")
            .expect("write copy b");
        initialize(&database_path).expect("initialize database");

        let request = StartScanRequest {
            root_path: root.to_string_lossy().into_owned(),
            include_hidden: false,
            stay_on_file_system: true,
        };

        let run = |scan_id: &str, finished_at: i64| {
            create_scan_run(&database_path, scan_id, &request.root_path, finished_at)
                .expect("create scan run");
            let mut connection = open(&database_path).expect("open database");
            let outcome = scan_directory(
                &request,
                scan_id,
                Arc::new(AtomicBool::new(false)),
                |batch| insert_file_batch(&mut connection, scan_id, &batch),
                |_| {},
            )
            .expect("scan fixture");
            finish_scan(
                &connection,
                scan_id,
                outcome.status,
                &outcome.stats,
                finished_at,
            )
            .expect("finish scan");
        };

        run("first", 100);
        fs::write(root.join("sample-project/src/new.txt"), b"new file")
            .expect("write changed file");
        run("second", 200);

        let history = list_scan_history(&database_path, "second").expect("list scan history");
        assert_eq!(history.len(), 2);
        let comparison = compare_scans(&database_path, "first", "second").expect("compare scans");
        assert_eq!(comparison.total_files_delta, 1);

        let duplicates =
            find_duplicate_candidates(&database_path, "second", 1).expect("find duplicates");
        assert_eq!(duplicates.len(), 1);
        assert_eq!(duplicates[0].file_count, 2);

        let projects = identify_projects(&database_path, "second").expect("identify projects");
        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0].name, "sample-project");
        assert_eq!(projects[0].file_count, 5);

        fs::remove_dir_all(&root).expect("remove fixture directory");
        fs::remove_file(&database_path).expect("remove fixture database");
        fs::remove_file(database_path.with_extension("sqlite3-shm")).ok();
        fs::remove_file(database_path.with_extension("sqlite3-wal")).ok();
    }

    #[test]
    fn classifies_common_extensions_case_insensitively() {
        assert_eq!(classify_extension(Some("JPG")), "images");
        assert_eq!(classify_extension(Some("mkv")), "videos");
        assert_eq!(classify_extension(Some("tsx")), "code");
        assert_eq!(classify_extension(None), "other");
    }

    #[test]
    fn scans_nested_files_and_skips_hidden_entries() {
        let root = std::env::temp_dir().join(format!("luma-scan-{}", Uuid::new_v4()));
        fs::create_dir_all(root.join("nested")).expect("create fixture directory");
        fs::write(root.join("nested/report.pdf"), b"data").expect("write visible fixture");
        fs::write(root.join(".hidden.txt"), b"hidden").expect("write hidden fixture");

        let request = StartScanRequest {
            root_path: root.to_string_lossy().into_owned(),
            include_hidden: false,
            stay_on_file_system: true,
        };
        let mut files = Vec::new();
        let outcome = scan_directory(
            &request,
            "test-scan",
            Arc::new(AtomicBool::new(false)),
            |batch| {
                files.extend(batch);
                Ok(())
            },
            |_| {},
        )
        .expect("scan fixture");

        assert_eq!(outcome.status, ScanStatus::Completed);
        assert_eq!(outcome.stats.files_scanned, 1);
        assert_eq!(outcome.stats.directories_scanned, 1);
        assert_eq!(outcome.stats.bytes_scanned, 4);
        assert_eq!(files[0].category, "documents");
        fs::remove_dir_all(root).expect("remove fixture directory");
    }

    #[test]
    fn honours_a_preexisting_cancellation() {
        let root = std::env::temp_dir().join(format!("luma-cancel-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).expect("create fixture directory");
        fs::write(root.join("file.txt"), b"data").expect("write fixture");
        let cancel = Arc::new(AtomicBool::new(true));
        let request = StartScanRequest {
            root_path: root.to_string_lossy().into_owned(),
            include_hidden: true,
            stay_on_file_system: true,
        };

        let outcome =
            scan_directory(&request, "cancelled", cancel, |_| Ok(()), |_| {}).expect("cancel scan");

        assert_eq!(outcome.status, ScanStatus::Cancelled);
        assert_eq!(outcome.stats.files_scanned, 0);
        fs::remove_dir_all(root).expect("remove fixture directory");
    }

    #[test]
    fn scans_an_empty_directory() {
        let root = std::env::temp_dir().join(format!("luma-empty-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).expect("create empty fixture directory");
        let request = StartScanRequest {
            root_path: root.to_string_lossy().into_owned(),
            include_hidden: false,
            stay_on_file_system: true,
        };

        let outcome = scan_directory(
            &request,
            "empty",
            Arc::new(AtomicBool::new(false)),
            |_| Ok(()),
            |_| {},
        )
        .expect("scan empty fixture");

        assert_eq!(outcome.status, ScanStatus::Completed);
        assert_eq!(outcome.stats.files_scanned, 0);
        assert_eq!(outcome.stats.directories_scanned, 0);
        fs::remove_dir_all(root).expect("remove fixture directory");
    }

    #[test]
    fn includes_hidden_files_when_requested() {
        let root = std::env::temp_dir().join(format!("luma-hidden-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).expect("create fixture directory");
        fs::write(root.join(".hidden.txt"), b"hidden").expect("write hidden fixture");
        let request = StartScanRequest {
            root_path: root.to_string_lossy().into_owned(),
            include_hidden: true,
            stay_on_file_system: true,
        };
        let mut files = Vec::new();

        let outcome = scan_directory(
            &request,
            "hidden",
            Arc::new(AtomicBool::new(false)),
            |batch| {
                files.extend(batch);
                Ok(())
            },
            |_| {},
        )
        .expect("scan hidden fixture");

        assert_eq!(outcome.stats.files_scanned, 1);
        assert!(files[0].is_hidden);
        fs::remove_dir_all(root).expect("remove fixture directory");
    }

    #[cfg(unix)]
    #[test]
    fn does_not_follow_symbolic_links() {
        use std::os::unix::fs::symlink;

        let root = std::env::temp_dir().join(format!("luma-link-{}", Uuid::new_v4()));
        fs::create_dir_all(root.join("real")).expect("create fixture directory");
        fs::write(root.join("real/file.txt"), b"data").expect("write fixture");
        symlink(root.join("real"), root.join("linked")).expect("create symlink");
        let request = StartScanRequest {
            root_path: root.to_string_lossy().into_owned(),
            include_hidden: true,
            stay_on_file_system: true,
        };

        let outcome = scan_directory(
            &request,
            "links",
            Arc::new(AtomicBool::new(false)),
            |_| Ok(()),
            |_| {},
        )
        .expect("scan symlink fixture");

        assert_eq!(outcome.stats.files_scanned, 1);
        assert_eq!(outcome.stats.directories_scanned, 1);
        fs::remove_dir_all(root).expect("remove fixture directory");
    }

    #[cfg(windows)]
    #[test]
    fn normalizes_backslashes_to_forward_slashes_on_windows() {
        assert_eq!(
            normalize_separators("C:\\Users\\me\\project\\node_modules".to_owned()),
            "C:/Users/me/project/node_modules"
        );
    }

    #[cfg(not(windows))]
    #[test]
    fn preserves_paths_verbatim_on_unix() {
        // On Unix a backslash is a legal filename character, so it must survive.
        assert_eq!(
            normalize_separators("/home/me/weird\\name".to_owned()),
            "/home/me/weird\\name"
        );
    }
}
