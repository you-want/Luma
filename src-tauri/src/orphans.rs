// 孤儿应用数据 (Orphaned app data): find data left behind by apps the user
// has already removed — scoped to
// Luma's red lines:
//   1. The app itself is never touched. A bundle identifier only becomes a
//      candidate when no installed .app claims it — data of apps still in
//      /Applications, ~/Applications or /System/Applications never shows up.
//   2. Only bundle-id-shaped identifiers (com.company.app) are considered.
//      Plain-named folders in Application Support cannot be attributed
//      reliably and are deliberately left out (zero false positives first).
//   3. Risk tiers: caches/state/storage are "safe" and preselected;
//      Application Support / Preferences / Containers / Logs are "review"
//      and start unchecked — personal data is kept unless explicitly chosen.
//   4. Group Containers (shared across apps) and LaunchAgents (login items)
//      are out of scope for now.
//   5. Execution re-discovers and only trashes items present in the fresh
//      discovery with unchanged size; whitelist and protected paths apply;
//      everything routes to the OS trash.

use crate::cleaner::{
    begin_cleaner_operation, load_whitelist, modified_secs, overlaps_whitelist, to_store_path,
    CleanOutcome, CleanRequestItem,
};
use crate::error::AppError;
use crate::file_ops::{self, OpFailure};
use serde::Serialize;
use std::collections::HashMap;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};
use walkdir::WalkDir;

// ── Constants ────────────────────────────────────────────────────────────────

/// Wall-clock budget for one discovery pass (on-demand, user-triggered).
const DISCOVERY_BUDGET: Duration = Duration::from_secs(8);

/// ~/Library subpaths scanned for leftovers: (subpath, group, risk).
const LOCATIONS: &[(&str, &str, &str)] = &[
    ("Caches", "caches", "safe"),
    ("Saved Application State", "savedState", "safe"),
    ("WebKit", "webkit", "safe"),
    ("HTTPStorages", "httpStorages", "safe"),
    ("Application Support", "support", "review"),
    ("Preferences", "preferences", "review"),
    ("Containers", "containers", "review"),
    ("Group Containers", "groupContainers", "review"),
    ("Logs", "logs", "review"),
];

// ── Public types ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OrphanItem {
    /// Leftover path — the deletion unit.
    pub path: String,
    /// Directory/file name (e.g. `com.company.app`).
    pub name: String,
    /// The bundle identifier the leftover is attributed to.
    pub bundle_id: String,
    /// Stable group key: caches | savedState | webkit | httpStorages |
    /// support | preferences | containers | logs.
    pub group: String,
    /// "safe" (rebuildable, preselected) | "review" (personal data, opt-in).
    pub risk: String,
    pub size_bytes: u64,
    pub file_count: u64,
    pub modified_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppDataReport {
    pub items: Vec<OrphanItem>,
    /// Library roots that exist and were scanned.
    pub scanned_roots: Vec<String>,
    pub is_partial: bool,
    /// False on platforms without a filesystem-only uninstall model (Windows
    /// needs registry-based uninstalls — out of scope for now).
    pub platform_supported: bool,
}

// ── Identifier heuristics ────────────────────────────────────────────────────

/// Whether `name` looks like a bundle identifier: dot-separated segments of
/// letters/digits/hyphens, each starting with a letter. Shared group
/// containers ("group.*") are rejected — multiple apps may share them.
fn looks_like_bundle_id(name: &str) -> bool {
    if name.starts_with("group.") {
        return false;
    }
    let segments: Vec<&str> = name.split('.').collect();
    segments.len() >= 2
        && segments.iter().all(|segment| {
            !segment.is_empty()
                && segment
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-')
                && segment
                    .chars()
                    .next()
                    .is_some_and(|c| c.is_ascii_alphabetic())
        })
}

/// Strip known prefixes/suffixes so group-specific names reduce to the
/// bundle id they belong to.
fn extract_bundle_id(raw: &str, group: &str) -> Option<String> {
    let stripped = match group {
        "preferences" => raw.strip_suffix(".plist")?,
        "savedState" => raw.strip_suffix(".savedState")?,
        "httpStorages" => raw.strip_suffix(".binarycookies").unwrap_or(raw),
        // `group.com.x.y` user-level shared containers. Team-prefixed Apple
        // style entries (`TEAMID.group.…`) fail the prefix strip and drop.
        "groupContainers" => raw.strip_prefix("group.")?,
        "launchAgents" => raw.strip_suffix(".plist")?,
        _ => raw,
    };
    looks_like_bundle_id(stripped).then(|| stripped.to_string())
}

// ── Installed apps ───────────────────────────────────────────────────────────

/// Bundle identifiers of installed apps (lowercased). Reads each .app's
/// Info.plist via the system `plutil`; unreadable apps are skipped.
fn installed_bundle_ids(app_roots: &[PathBuf], deadline: Instant) -> HashSet<String> {
    let mut installed = HashSet::new();
    'roots: for root in app_roots {
        for entry in WalkDir::new(root)
            .follow_links(false)
            .max_depth(2)
            .into_iter()
            .filter_map(|value| value.ok())
        {
            if Instant::now() > deadline {
                break 'roots;
            }
            let path = entry.path();
            if !entry.file_type().is_dir() || !path.to_string_lossy().ends_with(".app") {
                continue;
            }
            let info_plist = path.join("Contents/Info.plist");
            if !info_plist.is_file() {
                continue;
            }
            if let Some(bundle_id) = read_bundle_identifier(&info_plist) {
                installed.insert(bundle_id.to_ascii_lowercase());
            }
        }
    }
    installed
}

fn read_bundle_identifier(info_plist: &Path) -> Option<String> {
    let output = Command::new("plutil")
        .args(["-extract", "CFBundleIdentifier", "raw"])
        .arg(info_plist)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let value = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!value.is_empty()).then_some(value)
}

// ── Discovery ────────────────────────────────────────────────────────────────

/// Discover orphaned app-data leftovers under the user's Library.
pub fn discover_orphans(db_path: &Path) -> Result<AppDataReport, AppError> {
    // Fail closed: the report drives cleanup suggestions.
    let _whitelist = load_whitelist(db_path)?;

    #[cfg(target_os = "macos")]
    {
        let Some(home) = std::env::var("HOME").ok().map(PathBuf::from) else {
            return Ok(AppDataReport {
                items: vec![],
                scanned_roots: vec![],
                is_partial: false,
                platform_supported: true,
            });
        };
        let library = home.join("Library");
        let app_roots = vec![
            PathBuf::from("/Applications"),
            home.join("Applications"),
            PathBuf::from("/System/Applications"),
        ];
        discover_in(&library, &app_roots, Instant::now() + DISCOVERY_BUDGET)
    }

    #[cfg(not(target_os = "macos"))]
    {
        let _ = db_path;
        Ok(AppDataReport {
            items: vec![],
            scanned_roots: vec![],
            is_partial: false,
            platform_supported: false,
        })
    }
}

/// Library-root-injected discovery core, shared with tests. Computes the
/// installed-bundle-id set from the given app roots (macOS `plutil`), then
/// delegates to `discover_with_installed`.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) fn discover_in(
    library_root: &Path,
    app_roots: &[PathBuf],
    deadline: Instant,
) -> Result<AppDataReport, AppError> {
    let installed = installed_bundle_ids(app_roots, deadline);
    discover_with_installed(library_root, &installed, deadline)
}

/// Discovery with an injected installed-app set — the testable core.
fn discover_with_installed(
    library_root: &Path,
    installed: &HashSet<String>,
    deadline: Instant,
) -> Result<AppDataReport, AppError> {
    let mut report = AppDataReport {
        items: vec![],
        scanned_roots: vec![],
        is_partial: false,
        platform_supported: true,
    };
    if !library_root.is_dir() {
        return Ok(report);
    }
    report.scanned_roots.push(to_store_path(library_root));

    let mut claimed: HashSet<String> = HashSet::new(); // one item per path

    'locations: for (sub, group, risk) in LOCATIONS {
        let dir = library_root.join(sub);
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            if Instant::now() > deadline {
                report.is_partial = true;
                break 'locations;
            }
            let name = entry.file_name().to_string_lossy().to_string();
            let Some(bundle_id) = extract_bundle_id(&name, group) else {
                continue;
            };
            if installed.contains(&bundle_id.to_ascii_lowercase()) {
                continue; // the app is still installed — not an orphan
            }
            let path = entry.path();
            let stored = to_store_path(&path);
            if !claimed.insert(stored.clone()) {
                continue;
            }
            let Ok(meta) = entry.metadata() else {
                continue;
            };
            let (size, files, partial) = size_of_entry(&path, &meta, deadline);
            report.is_partial |= partial;
            let modified = WalkDir::new(&path)
                .follow_links(false)
                .into_iter()
                .filter_map(|value| value.ok())
                .filter(|value| value.file_type().is_file())
                .filter_map(|value| value.metadata().ok())
                .filter_map(|value| modified_secs(&value))
                .max()
                .or_else(|| modified_secs(&meta));
            report.items.push(OrphanItem {
                path: stored,
                name,
                bundle_id,
                group: (*group).to_string(),
                risk: (*risk).to_string(),
                size_bytes: size,
                file_count: files,
                modified_at: modified,
            });
        }
    }

    // LaunchAgents plists need content inspection (plutil) — macOS only.
    #[cfg(target_os = "macos")]
    scan_launch_agents(library_root, installed, &mut claimed, &mut report, deadline);

    report
        .items
        .sort_by_key(|item| std::cmp::Reverse(item.size_bytes));
    Ok(report)
}

/// Size of one leftover: files are measured directly, directories walked
/// with the shared deadline.
fn size_of_entry(path: &Path, meta: &fs::Metadata, deadline: Instant) -> (u64, u64, bool) {
    if !meta.is_dir() {
        return (meta.len(), 1, false);
    }
    let mut bytes = 0u64;
    let mut files = 0u64;
    for entry in WalkDir::new(path)
        .follow_links(false)
        .into_iter()
        .filter_map(|value| value.ok())
    {
        if Instant::now() > deadline {
            return (bytes, files, true);
        }
        if !entry.file_type().is_file() {
            continue;
        }
        let Ok(entry_meta) = entry.metadata() else {
            continue;
        };
        bytes += entry_meta.len();
        files += 1;
    }
    (bytes, files, false)
}

// ── LaunchAgents (macOS) ─────────────────────────────────────────────────────

/// Scan `~/Library/LaunchAgents` for plists of removed apps. A plist becomes
/// a candidate when its name is bundle-id shaped, the id is not installed,
/// and the plist content references no existing app bundle (a helper agent
/// for an installed app under a different id stays untouched). Only user
/// LaunchAgents are scanned — system launchd configuration is never read for
/// cleanup candidates.
#[cfg(target_os = "macos")]
fn scan_launch_agents(
    library_root: &Path,
    installed: &HashSet<String>,
    claimed: &mut HashSet<String>,
    report: &mut AppDataReport,
    deadline: Instant,
) {
    let dir = library_root.join("LaunchAgents");
    let Ok(entries) = fs::read_dir(&dir) else {
        return;
    };
    for entry in entries.flatten() {
        if Instant::now() > deadline {
            report.is_partial = true;
            return;
        }
        let path = entry.path();
        if !path.to_string_lossy().ends_with(".plist") {
            continue;
        }
        let Ok(meta) = entry.metadata() else {
            continue;
        };
        if !meta.is_file() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        let Some(bundle_id) = extract_bundle_id(&name, "launchAgents") else {
            continue;
        };
        if installed.contains(&bundle_id.to_ascii_lowercase()) {
            continue;
        }
        // Content cross-check: a plist that still points into an existing app
        // bundle serves that app regardless of its own label.
        if launch_agent_serves_installed_app(&path) {
            continue;
        }
        let stored = to_store_path(&path);
        if !claimed.insert(stored.clone()) {
            continue;
        }
        report.items.push(OrphanItem {
            path: stored,
            name,
            bundle_id,
            group: "launchAgents".to_string(),
            risk: "review".to_string(),
            size_bytes: meta.len(),
            file_count: 1,
            modified_at: modified_secs(&meta),
        });
    }
}

/// Whether the plist references at least one app bundle that still exists.
/// Unreadable plists count as "served" — skip rather than risk deleting a
/// launchd configuration we cannot inspect.
#[cfg(target_os = "macos")]
fn launch_agent_serves_installed_app(plist: &Path) -> bool {
    let Ok(output) = Command::new("plutil")
        .args(["-convert", "xml1", "-o", "-"])
        .arg(plist)
        .output()
    else {
        return true;
    };
    if !output.status.success() {
        return true;
    }
    let xml = String::from_utf8_lossy(&output.stdout);
    for value in extract_string_values(&xml) {
        if !value.starts_with('/') {
            continue;
        }
        if let Some(end) = value.find(".app/") {
            if Path::new(&value[..end + 4]).is_dir() {
                return true;
            }
        } else if value.ends_with(".app") && Path::new(&value).is_dir() {
            return true;
        }
    }
    false
}

/// Naive `<string>…</string>` extraction from a normalized XML plist.
#[cfg(target_os = "macos")]
fn extract_string_values(xml: &str) -> Vec<String> {
    let mut values = Vec::new();
    let mut rest = xml;
    while let Some(start) = rest.find("<string>") {
        let after = &rest[start + "<string>".len()..];
        let Some(end) = after.find("</string>") else {
            break;
        };
        values.push(after[..end].to_string());
        rest = &after[end + "</string>".len()..];
    }
    values
}

// ── Execution ────────────────────────────────────────────────────────────────

type Deleter<'a> = dyn Fn(&Path) -> Result<(), String> + 'a;

/// Trash the confirmed leftovers. Re-discovers first and verifies every
/// request against the fresh discovery (path + size, whitelist, protection).
pub fn execute_orphan_clean(
    db_path: &Path,
    items: &[CleanRequestItem],
) -> Result<CleanOutcome, AppError> {
    let library = home_library();
    execute_orphan_clean_in(
        db_path,
        library.as_deref(),
        &default_app_roots(),
        items,
        &|native| file_ops::trash_one(native),
    )
}

fn home_library() -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        std::env::var("HOME")
            .ok()
            .map(|home| PathBuf::from(home).join("Library"))
    }
    #[cfg(not(target_os = "macos"))]
    {
        None
    }
}

fn default_app_roots() -> Vec<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        let mut roots = vec![
            PathBuf::from("/Applications"),
            PathBuf::from("/System/Applications"),
        ];
        if let Some(home) = std::env::var("HOME").ok().map(PathBuf::from) {
            roots.push(home.join("Applications"));
        }
        roots
    }
    #[cfg(not(target_os = "macos"))]
    {
        vec![]
    }
}

/// Library-root-injected execution core, shared with tests. Computes the
/// installed-bundle-id set from the given app roots, then delegates.
pub(crate) fn execute_orphan_clean_in(
    db_path: &Path,
    library_root: Option<&Path>,
    app_roots: &[PathBuf],
    items: &[CleanRequestItem],
    deleter: &Deleter<'_>,
) -> Result<CleanOutcome, AppError> {
    let installed = match library_root {
        Some(library) if library.is_dir() => {
            installed_bundle_ids(app_roots, Instant::now() + DISCOVERY_BUDGET)
        }
        _ => HashSet::new(),
    };
    execute_orphan_clean_with_installed(db_path, library_root, &installed, items, deleter)
}

/// Execution with an injected installed-app set — the testable core.
fn execute_orphan_clean_with_installed(
    db_path: &Path,
    library_root: Option<&Path>,
    installed: &HashSet<String>,
    items: &[CleanRequestItem],
    deleter: &Deleter<'_>,
) -> Result<CleanOutcome, AppError> {
    // Fail closed on the whitelist before touching anything.
    let whitelist = load_whitelist(db_path)?;
    let report = match library_root {
        Some(library) => {
            discover_with_installed(library, installed, Instant::now() + DISCOVERY_BUDGET)?
        }
        None => AppDataReport {
            items: vec![],
            scanned_roots: vec![],
            is_partial: false,
            platform_supported: false,
        },
    };

    let mut index: HashMap<&str, &OrphanItem> = HashMap::new();
    for item in &report.items {
        index.insert(item.path.as_str(), item);
    }

    let operation_id = begin_cleaner_operation(
        db_path,
        format!("应用残留清理（{} 项，移至废纸篓）", items.len()),
    )?;
    let case_insensitive = cfg!(windows);
    let mut succeeded = Vec::new();
    let mut failed = Vec::new();

    for item in items {
        let outcome = match index.get(item.path.as_str()) {
            Some(orphan) => verify_orphan(orphan, item, &whitelist, case_insensitive),
            None => Err(AppError::new(
                "PATH_NOT_IN_SCOPE",
                format!("路径不在当前残留发现结果中，请重新扫描：{}", item.path),
            )),
        };
        match outcome {
            Ok(native) => match deleter(&native) {
                Ok(()) => succeeded.push(item.path.clone()),
                Err(reason) => failed.push(OpFailure {
                    path: item.path.clone(),
                    reason,
                }),
            },
            Err(error) => failed.push(OpFailure {
                path: item.path.clone(),
                reason: error.message,
            }),
        }
    }

    file_ops::finish_operation(db_path, &operation_id, succeeded.len(), &failed)?;
    Ok(CleanOutcome {
        operation_id: Some(operation_id),
        succeeded,
        failed,
    })
}

/// Deletion-boundary checks for one leftover against the fresh discovery.
fn verify_orphan(
    orphan: &OrphanItem,
    item: &CleanRequestItem,
    whitelist: &[String],
    case_insensitive: bool,
) -> Result<PathBuf, AppError> {
    let native = file_ops::validate_write_path(&item.path)?;
    if item.size_bytes != orphan.size_bytes {
        return Err(AppError::new(
            "FILE_CHANGED",
            format!("残留大小与重新扫描结果不一致，请刷新后重试：{}", item.path),
        ));
    }
    if overlaps_whitelist(&item.path, whitelist, case_insensitive) {
        return Err(AppError::new(
            "WHITELISTED_PATH",
            format!("路径受白名单保护：{}", item.path),
        ));
    }
    Ok(native)
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::initialize;
    use std::fs;
    use uuid::Uuid;

    fn temp_root(tag: &str) -> PathBuf {
        let base = std::env::current_dir()
            .expect("current dir")
            .join("target/luma-orphans-tests");
        let dir = base.join(format!("{tag}-{}", Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write_file(path: &Path, size: usize) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, vec![0u8; size]).unwrap();
    }

    /// Create a fake installed app whose Info.plist carries `bundle_id`.
    // Only the macOS-gated plutil discovery test installs apps.
    #[cfg(target_os = "macos")]
    fn install_app(app_roots: &Path, bundle_id: &str) {
        let info = app_roots.join(format!("App-{bundle_id}.app/Contents/Info.plist"));
        fs::create_dir_all(info.parent().unwrap()).unwrap();
        fs::write(
            &info,
            format!(
                "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
                 <plist version=\"1.0\"><dict>\n\
                 <key>CFBundleIdentifier</key><string>{bundle_id}</string>\n\
                 </dict></plist>\n"
            ),
        )
        .unwrap();
    }

    fn remove_dir(dir: &Path) {
        fs::remove_dir_all(dir).ok();
    }

    // ── Identifier heuristics ────────────────────────────────────────────────

    #[test]
    fn bundle_id_heuristic_matches_typical_identifiers() {
        assert!(looks_like_bundle_id("com.company.app"));
        assert!(looks_like_bundle_id("io.github.some-tool"));
        assert!(looks_like_bundle_id("com.foo"));
        assert!(
            !looks_like_bundle_id("group.com.company.app"),
            "shared containers are excluded"
        );
        assert!(!looks_like_bundle_id("Google"));
        assert!(!looks_like_bundle_id("com pany app"), "spaces are rejected");
        assert!(
            !looks_like_bundle_id("2ch.pipe"),
            "segments must start with a letter"
        );
        assert!(!looks_like_bundle_id("com."));
        assert!(!looks_like_bundle_id(""));
    }

    #[test]
    fn extract_bundle_id_strips_group_suffixes() {
        assert_eq!(
            extract_bundle_id("com.foo.bar.plist", "preferences").as_deref(),
            Some("com.foo.bar")
        );
        assert_eq!(
            extract_bundle_id("com.foo.bar.savedState", "savedState").as_deref(),
            Some("com.foo.bar")
        );
        assert_eq!(
            extract_bundle_id("com.foo.bar.binarycookies", "httpStorages").as_deref(),
            Some("com.foo.bar")
        );
        assert_eq!(extract_bundle_id("plain-thing.plist", "preferences"), None);
    }

    // ── Discovery ────────────────────────────────────────────────────────────

    #[test]
    // Reads real Info.plists via `plutil`, which only exists on macOS.
    #[cfg(target_os = "macos")]
    fn orphan_discovery_excludes_installed_apps() {
        let dir = temp_root("discover");
        let library = dir.join("Library");
        let apps = dir.join("Applications");
        install_app(&apps, "com.still.here");

        write_file(&library.join("Caches/com.still.here/cache.bin"), 1_000);
        write_file(&library.join("Caches/com.gone.app/cache.bin"), 2_000);
        write_file(
            &library.join("Application Support/com.gone.app/data.db"),
            3_000,
        );
        write_file(
            &library.join("Application Support/PlainFolder/stuff"),
            5_000,
        );
        write_file(&library.join("Preferences/com.gone.app.plist"), 100);
        write_file(
            &library.join("Saved Application State/com.gone.app.savedState/window"),
            50,
        );
        write_file(
            &library.join("Containers/group.com.gone.app/container"),
            4_000,
        );

        let report = discover_in(
            &library,
            std::slice::from_ref(&apps),
            Instant::now() + DISCOVERY_BUDGET,
        )
        .unwrap();
        let paths: Vec<&str> = report
            .items
            .iter()
            .map(|item| item.bundle_id.as_str())
            .collect();
        assert!(
            !paths.contains(&"com.still.here"),
            "installed app is excluded"
        );
        assert!(
            !paths.contains(&"group.com.gone.app"),
            "group containers are skipped"
        );
        assert!(!report.is_partial);

        // Orphans from bundle-id-shaped leftovers only.
        let support = report
            .items
            .iter()
            .find(|item| item.group == "support")
            .expect("Application Support orphan");
        assert_eq!(support.bundle_id, "com.gone.app");
        assert_eq!(support.size_bytes, 3_000);
        assert_eq!(support.risk, "review");

        let caches: Vec<&OrphanItem> = report
            .items
            .iter()
            .filter(|item| item.group == "caches")
            .collect();
        assert_eq!(caches.len(), 1, "only the removed app's caches");
        assert_eq!(caches[0].risk, "safe");
        assert_eq!(caches[0].size_bytes, 2_000);

        let prefs = report
            .items
            .iter()
            .find(|item| item.group == "preferences")
            .expect("preferences plist orphan");
        assert_eq!(prefs.size_bytes, 100, "single plist file size");
        remove_dir(&dir);
    }

    #[test]
    fn discovery_is_sorted_by_size_and_budget_aware() {
        let dir = temp_root("sort");
        let library = dir.join("Library");
        write_file(&library.join("Caches/com.small.one/x"), 10);
        write_file(&library.join("WebKit/com.big.one/data"), 9_999);

        let report = discover_in(&library, &[], Instant::now() + DISCOVERY_BUDGET).unwrap();
        assert_eq!(report.items.len(), 2);
        assert!(report.items[0].size_bytes >= report.items[1].size_bytes);

        // An expired deadline stops the walk and flags partial results.
        let report = discover_in(&library, &[], Instant::now()).unwrap();
        assert!(report.is_partial);
        remove_dir(&dir);
    }

    #[test]
    fn missing_library_yields_empty_report() {
        let dir = temp_root("missing");
        let report =
            discover_in(&dir.join("Nope"), &[], Instant::now() + DISCOVERY_BUDGET).unwrap();
        assert!(report.items.is_empty());
        assert!(!report.is_partial);
        assert!(report.platform_supported);
        remove_dir(&dir);
    }

    #[test]
    fn discover_fails_closed_when_whitelist_unreadable() {
        let db_dir = temp_root("db");
        let db = db_dir.join("broken.db");
        initialize(&db).unwrap();
        {
            let connection = crate::database::open(&db).unwrap();
            connection
                .execute("DROP TABLE cleaner_whitelist", [])
                .unwrap();
        }
        let error = discover_orphans(&db).expect_err("must fail closed");
        assert_eq!(error.code, "DATABASE_ERROR");
        fs::remove_dir_all(db_dir).ok();
    }

    // ── Execution ────────────────────────────────────────────────────────────

    fn orphan_fixture(tag: &str) -> (PathBuf, PathBuf, String, u64) {
        // (dir, library, leftover store path, leftover size)
        let dir = temp_root(tag);
        let library = dir.join("Library");
        write_file(&library.join("Caches/com.gone.app/a.bin"), 1_000);
        write_file(&library.join("Caches/com.gone.app/b.bin"), 500);
        let leftover = to_store_path(&library.join("Caches/com.gone.app"));
        (dir, library, leftover, 1_500)
    }

    #[test]
    fn execute_trashes_verified_leftover_and_logs_operation() {
        let (dir, library, leftover, size) = orphan_fixture("exec");
        let db = {
            let db_path = dir.join("test.db");
            initialize(&db_path).unwrap();
            db_path
        };
        let gone_marker = library.join("Caches/com.gone.app.gone");
        let marker_for_closure = gone_marker.clone();

        let outcome = execute_orphan_clean_in(
            &db,
            Some(&library),
            &[],
            &[CleanRequestItem {
                path: leftover.clone(),
                size_bytes: size,
            }],
            &move |native: &Path| {
                std::fs::rename(native, &marker_for_closure).map_err(|error| error.to_string())
            },
        )
        .unwrap();

        assert_eq!(outcome.succeeded, vec![leftover]);
        assert!(outcome.failed.is_empty());
        assert!(gone_marker.exists());
        remove_dir(&dir);
    }

    #[test]
    fn execute_rejects_size_drift_and_unknown_paths() {
        let (dir, library, leftover, size) = orphan_fixture("drift");
        let db = {
            let db_path = dir.join("test.db");
            initialize(&db_path).unwrap();
            db_path
        };
        // Size drift: an extra file appears before execution.
        write_file(&library.join("Caches/com.gone.app/new.bin"), 100);
        let outcome = execute_orphan_clean_in(
            &db,
            Some(&library),
            &[],
            &[CleanRequestItem {
                path: leftover.clone(),
                size_bytes: size,
            }],
            &|_| Ok(()),
        )
        .unwrap();
        assert!(outcome.succeeded.is_empty());
        assert!(outcome.failed[0].reason.contains("不一致"));

        // Unknown path: refused outright.
        let outcome = execute_orphan_clean_in(
            &db,
            Some(&library),
            &[],
            &[CleanRequestItem {
                path: "/elsewhere/com.ghost.app".to_string(),
                size_bytes: 1,
            }],
            &|_| Ok(()),
        )
        .unwrap();
        assert!(outcome.succeeded.is_empty());
        assert!(outcome.failed[0].reason.contains("重新扫描"));
        remove_dir(&dir);
    }

    #[test]
    fn execute_refuses_whitelisted_leftovers() {
        let (dir, library, leftover, size) = orphan_fixture("whitelist");
        let db = {
            let db_path = dir.join("test.db");
            initialize(&db_path).unwrap();
            db_path
        };
        crate::cleaner::add_whitelist_entry(&db, &leftover).unwrap();
        let outcome = execute_orphan_clean_in(
            &db,
            Some(&library),
            &[],
            &[CleanRequestItem {
                path: leftover,
                size_bytes: size,
            }],
            &|_| Ok(()),
        )
        .unwrap();
        assert!(outcome.succeeded.is_empty());
        assert!(outcome.failed[0].reason.contains("白名单"));
        remove_dir(&dir);
    }

    #[test]
    fn execute_fails_closed_when_whitelist_unreadable() {
        let (dir, library, leftover, size) = orphan_fixture("fail-closed");
        let db_dir = dir.join("dbdir");
        fs::create_dir_all(&db_dir).unwrap();
        let db = db_dir.join("broken.db");
        initialize(&db).unwrap();
        {
            let connection = crate::database::open(&db).unwrap();
            connection
                .execute("DROP TABLE cleaner_whitelist", [])
                .unwrap();
        }
        let error = execute_orphan_clean_in(
            &db,
            Some(&library),
            &[],
            &[CleanRequestItem {
                path: leftover,
                size_bytes: size,
            }],
            &|_| Ok(()),
        )
        .expect_err("unreadable whitelist must refuse execution");
        assert_eq!(error.code, "DATABASE_ERROR");
        remove_dir(&dir);
    }

    // ── Group Containers & LaunchAgents (CLN-202) ───────────────────────────

    #[test]
    fn group_containers_are_attributed_via_group_prefix() {
        let dir = temp_root("group-containers");
        let library = dir.join("Library");
        write_file(
            &library.join("Group Containers/group.com.gone.app/data"),
            2_000,
        );
        write_file(
            &library.join("Group Containers/TEAM1234.group.com.apple.ubq/x"),
            100,
        );
        write_file(
            &library.join("Group Containers/group.com.still.here/y"),
            300,
        );
        let installed: HashSet<String> = ["com.still.here".to_string()].into_iter().collect();

        let report =
            discover_with_installed(&library, &installed, Instant::now() + DISCOVERY_BUDGET)
                .unwrap();
        let group_items: Vec<&OrphanItem> = report
            .items
            .iter()
            .filter(|item| item.group == "groupContainers")
            .collect();
        assert_eq!(
            group_items.len(),
            1,
            "team-prefixed and installed are skipped"
        );
        assert_eq!(group_items[0].bundle_id, "com.gone.app");
        assert_eq!(group_items[0].risk, "review");
        remove_dir(&dir);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn launch_agents_of_removed_apps_are_listed() {
        let dir = temp_root("launch-agents");
        let library = dir.join("Library");
        let agents = library.join("LaunchAgents");
        // References an app path that does not exist → orphan.
        fs::create_dir_all(&agents).unwrap();
        fs::write(
            agents.join("com.gone.app.plist"),
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
             <plist version=\"1.0\"><dict>\n\
             <key>Label</key><string>com.gone.app</string>\n\
             <key>ProgramArguments</key><array>\n\
             <string>/Applications/Gone.app/Contents/MacOS/helper</string>\n\
             </array></dict></plist>\n",
        )
        .unwrap();
        // References an app path that exists → served by an installed app.
        let apps = dir.join("Applications");
        write_file(&apps.join("There.app/Contents/MacOS/main"), 10);
        fs::write(
            agents.join("com.served.app.plist"),
            format!(
                "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
                 <plist version=\"1.0\"><dict>\n\
                 <key>ProgramArguments</key><array>\n\
                 <string>{}/There.app/Contents/MacOS/main</string>\n\
                 </array></dict></plist>\n",
                to_store_path(&apps).replace('/', std::path::MAIN_SEPARATOR_STR)
            ),
        )
        .unwrap();
        // Not bundle-id shaped → never considered.
        fs::write(agents.join("my-agent.plist"), "<plist/>").unwrap();

        let report =
            discover_with_installed(&library, &HashSet::new(), Instant::now() + DISCOVERY_BUDGET)
                .unwrap();
        let agents_items: Vec<&OrphanItem> = report
            .items
            .iter()
            .filter(|item| item.group == "launchAgents")
            .collect();
        assert_eq!(
            agents_items.len(),
            1,
            "served and non-bundle-id plists are skipped"
        );
        assert_eq!(agents_items[0].bundle_id, "com.gone.app");
        assert_eq!(agents_items[0].risk, "review");
        remove_dir(&dir);
    }
}
