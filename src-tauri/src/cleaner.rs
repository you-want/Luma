// 磁盘管家 (Disk Cleaner): surface reclaimable space at well-known locations.
//
// Implemented under Luma's product red lines:
//   1. Only registered fixed locations are ever listed or deleted. Luma never
//      walks arbitrary directories on its own; every candidate is explainable.
//   2. Deletion always routes to the OS trash (never permanent). The Trash
//      itself is listed for review but never emptied here — emptying the trash
//      is a permanent delete and stays out of product scope.
//   3. Every candidate is re-verified at the deletion boundary: still exists,
//      still inside a registered root, still the size we listed, not
//      whitelisted by the user.
//   4. The user whitelist excludes paths from listing and deletion. If the
//      whitelist cannot be read, cleaning refuses to run (fail closed) rather
//      than operating unprotected.
//   5. Every execution is logged to `file_operations` (kind "trash", no undo
//      payload — the OS trash is the recovery path, as with file-manager
//      trash operations).

use crate::{
    database,
    error::AppError,
    file_ops::{self, OpFailure},
};
use rusqlite::params;
use serde::{Deserialize, Serialize};
#[cfg(target_os = "macos")]
use std::process::Command;
use std::{
    collections::{HashMap, HashSet},
    fs,
    path::{Component, Path, PathBuf},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use uuid::Uuid;
use walkdir::WalkDir;

// ── Public types ─────────────────────────────────────────────────────────────

/// Default age cutoff for the `oldDownloads` kind, in days.
pub const DEFAULT_OLD_DOWNLOADS_DAYS: u32 = 180;

/// Wall-clock budget for one kind's stats walk. A slow/large location stops
/// counting early and reports `is_partial`, so sizes read "at least".
const STATS_BUDGET: Duration = Duration::from_secs(2);
/// Slightly larger budget when the user expands a kind and we must list units.
const LIST_BUDGET: Duration = Duration::from_secs(3);

/// Disk usage for one mounted volume. Best-effort: when the platform query
/// fails the frontend simply hides the overview bar.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VolumeInfo {
    pub name: String,
    pub mount_point: String,
    pub total_bytes: u64,
    pub used_bytes: u64,
    pub free_bytes: u64,
}

/// One deletable unit: a direct child of a cache-style root (file or
/// directory) or an individually matched file (installer, old download).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanerFile {
    pub path: String,
    pub name: String,
    pub size_bytes: u64,
    pub modified_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanerLocation {
    pub kind: String,
    /// "safe" (rebuilds automatically) | "caution" (close the app first) |
    /// "review" (look before you delete).
    pub risk: String,
    pub size_bytes: u64,
    pub file_count: u64,
    /// Existing roots backing this kind (canonical `/` separators).
    pub roots: Vec<String>,
    /// The stats walk stopped early at the time budget, so sizes are a floor.
    pub is_partial: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanerOverview {
    pub locations: Vec<CleanerLocation>,
    pub volumes: Vec<VolumeInfo>,
    pub generated_at: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanOutcome {
    pub operation_id: Option<String>,
    pub succeeded: Vec<String>,
    pub failed: Vec<OpFailure>,
}

/// A deletion request item echoed back from the file list the user confirmed.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanRequestItem {
    pub path: String,
    pub size_bytes: u64,
}

// ── Location registry ────────────────────────────────────────────────────────

/// How deletion units are collected for a kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CollectMode {
    /// Direct children of each root; each child is one deletion unit sized
    /// recursively (cache-style roots — the root itself is never deleted).
    ChildUnits,
    /// Top-level files under the root matching the extension filter.
    TopLevelFiles,
    /// Every file under the root, optionally older than a day cutoff.
    AllFiles { older_than_days: Option<u32> },
    /// Simulator devices whose runtime is gone (via `xcrun simctl`). The
    /// deletion units come from the simctl listing, not from a directory
    /// walk — valid devices never become candidates.
    SimulatorDevices,
}

pub(crate) struct KindDef {
    kind: &'static str,
    risk: &'static str,
    /// Whether `execute_clean` accepts this kind. The trash is review-only:
    /// emptying it is a permanent delete, which stays out of product scope.
    executable: bool,
    mode: CollectMode,
    /// Lowercase extension filter for `TopLevelFiles` kinds.
    extensions: &'static [&'static str],
    /// Execution must re-run discovery and match the request against the
    /// fresh result (needed when "everything under the root" is NOT
    /// cleanable — e.g. only unavailable simulator devices are).
    reverify: bool,
}

const KINDS: &[KindDef] = &[
    KindDef {
        kind: "userCaches",
        risk: "safe",
        executable: true,
        mode: CollectMode::ChildUnits,
        extensions: &[],
        reverify: false,
    },
    KindDef {
        kind: "appLogs",
        risk: "safe",
        executable: true,
        mode: CollectMode::ChildUnits,
        extensions: &[],
        reverify: false,
    },
    KindDef {
        kind: "browserCaches",
        risk: "caution",
        executable: true,
        mode: CollectMode::ChildUnits,
        extensions: &[],
        reverify: false,
    },
    KindDef {
        kind: "pkgManagerCaches",
        risk: "safe",
        executable: true,
        mode: CollectMode::ChildUnits,
        extensions: &[],
        reverify: false,
    },
    KindDef {
        kind: "devDerived",
        risk: "safe",
        executable: true,
        mode: CollectMode::ChildUnits,
        extensions: &[],
        reverify: false,
    },
    KindDef {
        kind: "simDevices",
        risk: "safe",
        executable: true,
        mode: CollectMode::SimulatorDevices,
        extensions: &[],
        reverify: true,
    },
    KindDef {
        kind: "installers",
        risk: "review",
        executable: true,
        mode: CollectMode::TopLevelFiles,
        extensions: &["dmg", "pkg", "mpkg", "iso", "xip"],
        reverify: false,
    },
    KindDef {
        kind: "oldDownloads",
        risk: "review",
        executable: true,
        mode: CollectMode::AllFiles {
            older_than_days: Some(DEFAULT_OLD_DOWNLOADS_DAYS),
        },
        extensions: &[],
        reverify: false,
    },
    KindDef {
        kind: "trash",
        risk: "review",
        executable: false,
        mode: CollectMode::AllFiles {
            older_than_days: None,
        },
        extensions: &[],
        reverify: false,
    },
];

pub(crate) fn find_kind(kind: &str) -> Option<&'static KindDef> {
    KINDS.iter().find(|def| def.kind == kind)
}

/// Registered root paths for a kind on this platform, resolved from the
/// user's home. Returns `None` for kinds that do not exist on this platform.
fn location_roots(kind: &str) -> Option<Vec<PathBuf>> {
    #[cfg(target_os = "macos")]
    {
        let home = home_dir()?;
        let roots = match kind {
            "userCaches" => vec![home.join("Library/Caches")],
            "appLogs" => vec![home.join("Library/Logs")],
            "browserCaches" => vec![
                home.join("Library/Caches/com.apple.Safari"),
                home.join("Library/Caches/Google/Chrome"),
                home.join("Library/Caches/Firefox"),
            ],
            "pkgManagerCaches" => vec![
                home.join(".npm/_cacache"),
                home.join("Library/Caches/pnpm"),
                home.join("Library/Caches/pip"),
                home.join("Library/Caches/uv"),
            ],
            "devDerived" => vec![
                home.join("Library/Developer/Xcode/DerivedData"),
                home.join("Library/Developer/CoreSimulator/Caches"),
                // Rebuilt automatically the next time a device is attached.
                home.join("Library/Developer/Xcode/iOS DeviceSupport"),
            ],
            "simDevices" => vec![home.join("Library/Developer/CoreSimulator/Devices")],
            "installers" => vec![home.join("Downloads"), home.join("Desktop")],
            "oldDownloads" => vec![home.join("Downloads")],
            "trash" => vec![home.join(".Trash")],
            _ => return None,
        };
        Some(roots)
    }

    #[cfg(target_os = "windows")]
    {
        let local = std::env::var("LOCALAPPDATA")
            .ok()
            .map(PathBuf::from)
            .or_else(|| home_dir().map(|h| h.join("AppData/Local")))?;
        let profile = home_dir()?;
        let roots = match kind {
            "userCaches" => vec![local.join("Temp")],
            "appLogs" => vec![local.join("CrashDumps")],
            "browserCaches" => vec![
                local.join("Google/Chrome/User Data/Default/Cache"),
                local.join("Microsoft/Edge/User Data/Default/Cache"),
            ],
            "pkgManagerCaches" => vec![local.join("npm-cache"), local.join("pip/Cache")],
            "installers" => vec![profile.join("Downloads"), profile.join("Desktop")],
            "oldDownloads" => vec![profile.join("Downloads")],
            // The Recycle Bin requires elevated access; skip like cleanup.rs.
            "trash" => return None,
            _ => return None,
        };
        Some(roots)
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = kind;
        None
    }
}

fn home_dir() -> Option<PathBuf> {
    std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .ok()
        .map(PathBuf::from)
}

pub(crate) fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Store paths with `/` separators on every platform, matching the scan index
/// convention so whitelist matching and prefix checks are uniform.
pub(crate) fn to_store_path(path: &Path) -> String {
    let raw = path.to_string_lossy().to_string();
    #[cfg(target_os = "windows")]
    let raw = raw.replace('\\', "/");
    raw
}

// ── Whitelist ────────────────────────────────────────────────────────────────

/// One whitelisted path, for the management UI.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanerWhitelistEntry {
    pub path: String,
    pub created_at: i64,
}

/// Raw whitelist paths used for filtering (fail-closed caller side).
pub fn load_whitelist(db_path: &Path) -> Result<Vec<String>, AppError> {
    Ok(list_whitelist_entries(db_path)?
        .into_iter()
        .map(|entry| entry.path)
        .collect())
}

pub fn list_whitelist_entries(db_path: &Path) -> Result<Vec<CleanerWhitelistEntry>, AppError> {
    let connection = database::open(db_path)?;
    let mut statement = connection
        .prepare("SELECT path, created_at FROM cleaner_whitelist ORDER BY created_at ASC, path ASC")
        .map_err(|error| AppError::new("DATABASE_ERROR", error.to_string()))?;
    let rows = statement
        .query_map([], |row| {
            Ok(CleanerWhitelistEntry {
                path: row.get(0)?,
                created_at: row.get(1)?,
            })
        })
        .map_err(|error| AppError::new("DATABASE_ERROR", error.to_string()))?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|error| AppError::new("DATABASE_ERROR", error.to_string()))
}

/// One cleaner-family run, for the history view. Read-only: cleanup runs are
/// already logged to `file_operations` with the sentinel `scan_id='cleaner'`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanerOperation {
    pub id: String,
    pub label: String,
    pub status: String,
    pub created_at: i64,
    pub finished_at: Option<i64>,
}

/// Recent cleaner-family operations, newest first. Covers 磁盘管家清理 /
/// 项目瘦身清理 / 应用残留清理 (all logged under scan_id 'cleaner').
pub fn list_operations(db_path: &Path, limit: u32) -> Result<Vec<CleanerOperation>, AppError> {
    let connection = database::open(db_path)?;
    let mut statement = connection
        .prepare(
            "SELECT id, label, status, created_at, finished_at FROM file_operations
             WHERE scan_id = 'cleaner'
             ORDER BY created_at DESC, id DESC
             LIMIT ?1",
        )
        .map_err(|error| AppError::new("DATABASE_ERROR", error.to_string()))?;
    let rows = statement
        .query_map(params![i64::from(limit.clamp(1, 50))], |row| {
            Ok(CleanerOperation {
                id: row.get(0)?,
                label: row.get(1)?,
                status: row.get(2)?,
                created_at: row.get(3)?,
                finished_at: row.get(4)?,
            })
        })
        .map_err(|error| AppError::new("DATABASE_ERROR", error.to_string()))?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|error| AppError::new("DATABASE_ERROR", error.to_string()))
}

pub fn add_whitelist_entry(
    db_path: &Path,
    raw_path: &str,
) -> Result<Vec<CleanerWhitelistEntry>, AppError> {
    let canonical = normalize_input_path(raw_path)?;
    database::open(db_path)?
        .execute(
            "INSERT OR IGNORE INTO cleaner_whitelist (path, created_at) VALUES (?1, ?2)",
            params![canonical, now_secs() as i64],
        )
        .map_err(|error| AppError::new("DATABASE_ERROR", error.to_string()))?;
    list_whitelist_entries(db_path)
}

pub fn remove_whitelist_entry(
    db_path: &Path,
    raw_path: &str,
) -> Result<Vec<CleanerWhitelistEntry>, AppError> {
    let canonical = normalize_input_path(raw_path)?;
    database::open(db_path)?
        .execute(
            "DELETE FROM cleaner_whitelist WHERE path = ?1",
            params![canonical],
        )
        .map_err(|error| AppError::new("DATABASE_ERROR", error.to_string()))?;
    list_whitelist_entries(db_path)
}

/// Canonicalize a user-provided path to the stored form: absolute, `/`
/// separators, no trailing slash, symlinks resolved when the path exists.
fn normalize_input_path(raw: &str) -> Result<String, AppError> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(AppError::invalid_path("路径不能为空。"));
    }
    #[cfg(target_os = "windows")]
    let candidate = trimmed.replace('\\', "/");
    #[cfg(not(target_os = "windows"))]
    let candidate = trimmed.to_string();

    let native = PathBuf::from(candidate.replace('/', std::path::MAIN_SEPARATOR_STR));
    if !native.is_absolute() {
        return Err(AppError::invalid_path("白名单只接受绝对路径。"));
    }
    if native
        .components()
        .any(|component| matches!(component, Component::ParentDir))
    {
        return Err(AppError::invalid_path("路径不能包含上级目录跳转。"));
    }
    let stored = match native.canonicalize() {
        // Strip Windows verbatim prefixes, or the stored form can never match
        // the plain paths later sent back for deletion.
        Ok(resolved) => to_store_path(&file_ops::strip_windows_verbatim_prefix(resolved)),
        Err(_) => {
            let trimmed_stored = candidate.trim_end_matches('/');
            if trimmed_stored.is_empty() {
                "/".to_string()
            } else {
                trimmed_stored.to_string()
            }
        }
    };
    Ok(stored)
}

/// Whether `path` equals `root` or lives underneath it. A root of `/` covers
/// every absolute path; trailing slashes are ignored.
fn is_under_root(path: &str, root: &str, case_insensitive: bool) -> bool {
    let (path, root) = if case_insensitive {
        (path.to_ascii_lowercase(), root.to_ascii_lowercase())
    } else {
        (path.to_string(), root.to_string())
    };
    let trimmed = root.trim_end_matches('/');
    if trimmed.is_empty() {
        return path.starts_with('/');
    }
    path == trimmed || path.starts_with(&format!("{trimmed}/"))
}

/// Whether a deletion unit overlaps the whitelist in either direction: the
/// unit inside a protected entry, or a protected entry inside the unit (the
/// unit would swallow something the user chose to protect).
/// Whether a deletion unit overlaps the whitelist in either direction: the
/// unit inside a protected entry, or a protected entry inside the unit (the
/// unit would swallow something the user chose to protect). Shared with the
/// project-slimming module (`purge.rs`).
pub(crate) fn overlaps_whitelist(path: &str, whitelist: &[String], case_insensitive: bool) -> bool {
    whitelist.iter().any(|entry| {
        is_under_root(path, entry, case_insensitive) || is_under_root(entry, path, case_insensitive)
    })
}

// ── Collection (stats + listing share one primitive) ─────────────────────────

#[derive(Debug, Default)]
struct Collected {
    units: Vec<CleanerFile>,
    total_bytes: u64,
    file_count: u64,
    is_partial: bool,
}

/// Collect deletion units for one kind. `deadline` bounds the walk; once it
/// passes, collection stops early and `is_partial` is set.
fn collect_units(def: &KindDef, whitelist: &[String], deadline: Instant) -> Collected {
    let roots = location_roots(def.kind).unwrap_or_default();
    collect_units_in(def, roots, whitelist, deadline)
}

/// Root-injected variant used by tests so filesystem walks never touch the
/// real registered locations.
fn collect_units_in(
    def: &KindDef,
    roots: Vec<PathBuf>,
    whitelist: &[String],
    deadline: Instant,
) -> Collected {
    let mut collected = Collected::default();
    let case_insensitive = cfg!(windows);
    let cutoff = match def.mode {
        CollectMode::AllFiles {
            older_than_days: Some(days),
        } => Some(now_secs().saturating_sub(u64::from(days) * 86_400)),
        _ => None,
    };

    'roots: for root in roots {
        if !root.is_dir() {
            continue;
        }
        match def.mode {
            CollectMode::ChildUnits => {
                let Ok(entries) = fs::read_dir(&root) else {
                    continue;
                };
                for entry in entries.flatten() {
                    if Instant::now() > deadline {
                        collected.is_partial = true;
                        break 'roots;
                    }
                    let path = entry.path();
                    let stored = to_store_path(&path);
                    if overlaps_whitelist(&stored, whitelist, case_insensitive) {
                        continue;
                    }
                    let Ok(meta) = entry.metadata() else {
                        continue;
                    };
                    let (size, files, partial) = size_of_path(&path, &meta, deadline);
                    collected.is_partial |= partial;
                    collected.units.push(CleanerFile {
                        path: stored,
                        name: entry.file_name().to_string_lossy().to_string(),
                        size_bytes: size,
                        modified_at: modified_secs(&meta),
                    });
                    collected.total_bytes += size;
                    collected.file_count += files;
                }
            }
            CollectMode::TopLevelFiles => {
                let Ok(entries) = fs::read_dir(&root) else {
                    continue;
                };
                for entry in entries.flatten() {
                    if Instant::now() > deadline {
                        collected.is_partial = true;
                        break 'roots;
                    }
                    let Ok(meta) = entry.metadata() else {
                        continue;
                    };
                    if !meta.is_file() {
                        continue;
                    }
                    let stored = to_store_path(&entry.path());
                    if overlaps_whitelist(&stored, whitelist, case_insensitive) {
                        continue;
                    }
                    let extension = entry
                        .path()
                        .extension()
                        .map(|value| value.to_string_lossy().to_ascii_lowercase());
                    let matches = extension
                        .as_deref()
                        .map(|value| def.extensions.contains(&value))
                        .unwrap_or(false);
                    if !matches {
                        continue;
                    }
                    collected.units.push(CleanerFile {
                        path: stored,
                        name: entry.file_name().to_string_lossy().to_string(),
                        size_bytes: meta.len(),
                        modified_at: modified_secs(&meta),
                    });
                    collected.total_bytes += meta.len();
                    collected.file_count += 1;
                }
            }
            CollectMode::AllFiles { .. } => {
                for entry in WalkDir::new(&root)
                    .follow_links(false)
                    .into_iter()
                    .filter_map(|value| value.ok())
                {
                    if Instant::now() > deadline {
                        collected.is_partial = true;
                        break 'roots;
                    }
                    if !entry.file_type().is_file() {
                        continue;
                    }
                    let Ok(meta) = entry.metadata() else {
                        continue;
                    };
                    if let Some(cutoff) = cutoff {
                        let modified = modified_secs(&meta).unwrap_or(u64::MAX as i64) as u64;
                        if modified >= cutoff {
                            continue;
                        }
                    }
                    let stored = to_store_path(entry.path());
                    if overlaps_whitelist(&stored, whitelist, case_insensitive) {
                        continue;
                    }
                    collected.units.push(CleanerFile {
                        path: stored,
                        name: entry.file_name().to_string_lossy().to_string(),
                        size_bytes: meta.len(),
                        modified_at: modified_secs(&meta),
                    });
                    collected.total_bytes += meta.len();
                    collected.file_count += 1;
                }
            }
            CollectMode::SimulatorDevices => {
                // Units come from the simctl listing (macOS only), not from
                // walking the root — valid devices never become candidates.
                #[cfg(target_os = "macos")]
                for device in simulator_unavailable_devices(deadline) {
                    if Instant::now() > deadline {
                        collected.is_partial = true;
                        break 'roots;
                    }
                    let stored = to_store_path(&device.path);
                    if overlaps_whitelist(&stored, whitelist, case_insensitive) {
                        continue;
                    }
                    let Ok(meta) = fs::metadata(&device.path) else {
                        continue;
                    };
                    let (size, files, partial) = size_of_path(&device.path, &meta, deadline);
                    collected.is_partial |= partial;
                    collected.units.push(CleanerFile {
                        path: stored,
                        name: device.label,
                        size_bytes: size,
                        modified_at: modified_secs(&meta),
                    });
                    collected.total_bytes += size;
                    collected.file_count += files;
                }
            }
        }
    }
    collected
}

/// Total size and file count of one deletion unit. Files are measured
/// directly; directories are walked with the same deadline.
fn size_of_path(path: &Path, meta: &fs::Metadata, deadline: Instant) -> (u64, u64, bool) {
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

pub(crate) fn modified_secs(meta: &fs::Metadata) -> Option<i64> {
    meta.modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_secs() as i64)
}

// ── Entry points ─────────────────────────────────────────────────────────────

/// Build the disk-cleaner overview: per-kind reclaimable sizes at registered
/// locations plus best-effort volume capacity. Fails closed when the
/// whitelist cannot be read.
pub fn build_cleaner_overview(
    db_path: &Path,
    old_downloads_days: u32,
) -> Result<CleanerOverview, AppError> {
    let _ = old_downloads_days; // the registry carries its own cutoff
    let whitelist = load_whitelist(db_path)?;
    let mut locations = Vec::new();
    for def in KINDS {
        let roots = match location_roots(def.kind) {
            Some(roots) => roots,
            None => continue,
        };
        let present: Vec<PathBuf> = roots.into_iter().filter(|root| root.exists()).collect();
        if present.is_empty() {
            continue;
        }
        let collected = collect_units(def, &whitelist, Instant::now() + STATS_BUDGET);
        locations.push(CleanerLocation {
            kind: def.kind.to_string(),
            risk: def.risk.to_string(),
            size_bytes: collected.total_bytes,
            file_count: collected.file_count,
            roots: present.iter().map(|root| to_store_path(root)).collect(),
            is_partial: collected.is_partial,
        });
    }
    locations.sort_by_key(|location| std::cmp::Reverse(location.size_bytes));
    Ok(CleanerOverview {
        locations,
        volumes: query_volumes(),
        generated_at: now_secs() as i64,
    })
}

/// List deletion units for one kind, largest first, whitelist-filtered.
pub fn list_cleaner_files(
    db_path: &Path,
    kind: &str,
    limit: u32,
) -> Result<Vec<CleanerFile>, AppError> {
    let def = find_kind(kind)
        .ok_or_else(|| AppError::new("INVALID_CLEAN_KIND", format!("未知的清理类别：{kind}。")))?;
    let whitelist = load_whitelist(db_path)?;
    let mut collected = collect_units(def, &whitelist, Instant::now() + LIST_BUDGET);
    collected.units.sort_by(|a, b| {
        b.size_bytes
            .cmp(&a.size_bytes)
            .then_with(|| a.path.cmp(&b.path))
    });
    collected.units.truncate(limit as usize);
    Ok(collected.units)
}

type Deleter<'a> = dyn Fn(&Path) -> Result<(), String> + 'a;

/// Trash the confirmed units. Every path is re-verified at the deletion
/// boundary; failures are collected instead of aborting the batch.
pub fn execute_clean(
    db_path: &Path,
    kind: &str,
    items: &[CleanRequestItem],
) -> Result<CleanOutcome, AppError> {
    execute_clean_with(db_path, kind, items, &|native| file_ops::trash_one(native))
}

fn execute_clean_with(
    db_path: &Path,
    kind: &str,
    items: &[CleanRequestItem],
    deleter: &Deleter<'_>,
) -> Result<CleanOutcome, AppError> {
    let def = find_kind(kind)
        .ok_or_else(|| AppError::new("INVALID_CLEAN_KIND", format!("未知的清理类别：{kind}。")))?;
    if !def.executable {
        return Err(AppError::new(
            "INVALID_CLEAN_KIND",
            "该类别仅支持查看，不支持在此清理。",
        ));
    }
    let roots = location_roots(kind)
        .ok_or_else(|| AppError::new("INVALID_CLEAN_KIND", "此类别在当前平台不可用。"))?;
    let roots: Vec<String> = roots.iter().map(|root| to_store_path(root)).collect();
    execute_clean_with_roots(db_path, def, &roots, items, deleter)
}

/// Root-injected core of the execution path, shared with tests. Fail closed:
/// refuse to clean anything when the whitelist is unreadable.
pub(crate) fn execute_clean_with_roots(
    db_path: &Path,
    def: &KindDef,
    roots: &[String],
    items: &[CleanRequestItem],
    deleter: &Deleter<'_>,
) -> Result<CleanOutcome, AppError> {
    let whitelist = load_whitelist(db_path)?;
    let case_insensitive = cfg!(windows);
    let cutoff = match def.mode {
        CollectMode::AllFiles {
            older_than_days: Some(days),
        } => Some(now_secs().saturating_sub(u64::from(days) * 86_400)),
        _ => None,
    };
    // Kinds flagged `reverify` (simulator devices) re-run discovery and only
    // accept paths present in the fresh result with an unchanged size —
    // living under the root does not make every child cleanable there.
    let fresh: HashMap<String, u64> = if def.reverify {
        collect_units(def, &whitelist, Instant::now() + LIST_BUDGET)
            .units
            .into_iter()
            .map(|unit| (unit.path, unit.size_bytes))
            .collect()
    } else {
        HashMap::new()
    };

    let operation_id = begin_cleaner_operation(
        db_path,
        format!("磁盘管家清理（{} 项，移至废纸篓）", items.len()),
    )?;
    let mut succeeded = Vec::new();
    let mut failed = Vec::new();

    for item in items {
        let outcome = if def.reverify {
            match fresh.get(&item.path) {
                Some(size) if *size == item.size_bytes => file_ops::validate_write_path(&item.path),
                Some(_) => Err(AppError::new(
                    "FILE_CHANGED",
                    format!("内容与重新扫描结果不一致，请刷新后重试：{}", item.path),
                )),
                None => Err(AppError::new(
                    "PATH_NOT_IN_SCOPE",
                    format!("路径不在当前发现结果中，请重新扫描：{}", item.path),
                )),
            }
        } else {
            validate_candidate(
                def,
                roots,
                &item.path,
                item.size_bytes,
                &whitelist,
                case_insensitive,
                cutoff,
            )
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

/// Deletion-boundary re-verification for one candidate path.
fn validate_candidate(
    def: &KindDef,
    roots: &[String],
    raw: &str,
    expected_size: u64,
    whitelist: &[String],
    case_insensitive: bool,
    cutoff: Option<u64>,
) -> Result<PathBuf, AppError> {
    if raw.trim().is_empty() {
        return Err(AppError::invalid_path("路径不能为空。"));
    }
    // Absolute, no parent-dir jumps, outside system-protected locations.
    let native = file_ops::validate_write_path(raw)?;
    // Only paths inside one of this kind's registered roots are eligible —
    // anything else is rejected no matter what the client sent.
    if !roots
        .iter()
        .any(|root| is_under_root(raw, root, case_insensitive))
    {
        return Err(AppError::new(
            "PATH_NOT_IN_SCOPE",
            format!("路径不在清理范围内：{raw}"),
        ));
    }

    match def.mode {
        CollectMode::SimulatorDevices => {
            // Simulator devices are verified against a fresh simctl listing
            // (KindDef::reverify) and never reach this per-path validator.
            let _ = cutoff;
            let _ = whitelist;
            return Err(AppError::new(
                "INVALID_CLEAN_KIND",
                "模拟器设备清理必须通过重新扫描校验。",
            ));
        }
        CollectMode::ChildUnits => {
            // The unit must be a direct child of a registered root — the root
            // itself is never deleted.
            let parent = raw.rsplit_once('/').map(|(parent, _)| parent).unwrap_or("");
            if !roots
                .iter()
                .any(|root| parent == root.trim_end_matches('/'))
            {
                return Err(AppError::new(
                    "PATH_NOT_IN_SCOPE",
                    format!("清理目标必须是注册位置的一级子项：{raw}"),
                ));
            }
            let meta = fs::metadata(&native)
                .map_err(|error| AppError::new("STALE_FILE", error.to_string()))?;
            if meta.is_file() && meta.len() != expected_size {
                return Err(AppError::new(
                    "FILE_CHANGED",
                    "文件在确认后发生了变化，请刷新后重试。",
                ));
            }
            // Both directions: the unit must not be protected, and must not
            // swallow a protected path inside it.
            if overlaps_whitelist(raw, whitelist, case_insensitive) {
                return Err(AppError::new(
                    "WHITELISTED_PATH",
                    format!("路径受白名单保护：{raw}"),
                ));
            }
        }
        CollectMode::TopLevelFiles | CollectMode::AllFiles { .. } => {
            let meta = fs::metadata(&native)
                .map_err(|error| AppError::new("STALE_FILE", error.to_string()))?;
            if !meta.is_file() {
                return Err(AppError::new(
                    "FILE_CHANGED",
                    "目标已不是普通文件，请刷新后重试。",
                ));
            }
            if meta.len() != expected_size {
                return Err(AppError::new(
                    "FILE_CHANGED",
                    "文件在确认后发生了变化，请刷新后重试。",
                ));
            }
            if !def.extensions.is_empty() {
                let matches = native
                    .extension()
                    .map(|value| {
                        let value = value.to_string_lossy().to_ascii_lowercase();
                        def.extensions.contains(&value.as_str())
                    })
                    .unwrap_or(false);
                if !matches {
                    return Err(AppError::new(
                        "FILE_CHANGED",
                        format!("扩展名不符合该清理类别：{raw}"),
                    ));
                }
            }
            if let Some(cutoff) = cutoff {
                let modified = modified_secs(&meta).unwrap_or(u64::MAX as i64) as u64;
                if modified >= cutoff {
                    return Err(AppError::new(
                        "FILE_CHANGED",
                        format!("文件仍在时间范围内，已不再符合清理条件：{raw}"),
                    ));
                }
            }
            if overlaps_whitelist(raw, whitelist, case_insensitive) {
                return Err(AppError::new(
                    "WHITELISTED_PATH",
                    format!("路径受白名单保护：{raw}"),
                ));
            }
        }
    }
    Ok(native)
}

/// Log a cleaner-family run. Reuses the `file_operations` table with the
/// sentinel `scan_id = 'cleaner'` and no undo payload — the OS trash is the
/// recovery path, same as file-manager trash operations.
pub(crate) fn begin_cleaner_operation(db_path: &Path, label: String) -> Result<String, AppError> {
    let operation_id = Uuid::new_v4().to_string();
    database::open(db_path)?
        .execute(
            "INSERT INTO file_operations (id, scan_id, root_path, kind, label, status, created_at)
             VALUES (?1, 'cleaner', '', 'trash', ?2, 'running', ?3)",
            params![operation_id, label, now_secs() as i64],
        )
        .map_err(|error| AppError::new("DATABASE_ERROR", error.to_string()))?;
    Ok(operation_id)
}

// ── Simulator devices (macOS) ────────────────────────────────────────────────

/// A simulator device whose runtime is gone, ready to be offered for cleanup.
#[cfg(target_os = "macos")]
struct SimulatorDevice {
    /// On-disk device directory (the deletion unit).
    path: PathBuf,
    /// Human label: "<device name> (<runtime>)".
    label: String,
}

/// Unavailable simulator devices from `xcrun simctl list devices -j`. Only
/// devices the system explicitly reports as unavailable are ever returned —
/// anything unparsable, ambiguous or missing a data path is skipped (fail
/// closed), so a valid device can never be offered for deletion.
#[cfg(target_os = "macos")]
fn simulator_unavailable_devices(deadline: Instant) -> Vec<SimulatorDevice> {
    if Instant::now() > deadline {
        return vec![];
    }
    let Ok(output) = Command::new("xcrun")
        .args(["simctl", "list", "devices", "-j"])
        .output()
    else {
        return vec![];
    };
    if !output.status.success() {
        return vec![];
    }
    let Ok(value) = serde_json::from_slice::<serde_json::Value>(&output.stdout) else {
        return vec![];
    };
    parse_simctl_devices(&value)
}

/// Pure parser shared with tests: keeps only entries with `isAvailable`
/// explicitly false, mapping each to its on-disk device directory.
#[cfg(target_os = "macos")]
fn parse_simctl_devices(value: &serde_json::Value) -> Vec<SimulatorDevice> {
    let mut devices = Vec::new();
    let Some(runtimes) = value.get("devices").and_then(|v| v.as_object()) else {
        return devices;
    };
    for (runtime, list) in runtimes {
        let Some(entries) = list.as_array() else {
            continue;
        };
        let runtime_name = runtime
            .strip_prefix("com.apple.CoreSimulator.SimRuntime.")
            .unwrap_or(runtime);
        for entry in entries {
            // `isAvailable` absent means unknown — skip, never guess.
            if entry.get("isAvailable").and_then(|v| v.as_bool()) != Some(false) {
                continue;
            }
            let Some(data_path) = entry.get("dataPath").and_then(|v| v.as_str()) else {
                continue;
            };
            // dataPath points at <device>/data; the deletion unit is <device>.
            let Some(device_dir) = Path::new(data_path).parent() else {
                continue;
            };
            let name = entry
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("Simulator");
            devices.push(SimulatorDevice {
                path: device_dir.to_path_buf(),
                label: format!("{name} ({runtime_name})"),
            });
        }
    }
    devices
}

// ── Volumes ──────────────────────────────────────────────────────────────────

/// Best-effort disk capacity query. Returns an empty list when the platform
/// query is unavailable — the frontend hides the section then.
fn query_volumes() -> Vec<VolumeInfo> {
    #[cfg(target_os = "macos")]
    {
        let Ok(output) = std::process::Command::new("df").arg("-k").output() else {
            return vec![];
        };
        if !output.status.success() {
            return vec![];
        }
        parse_df_output(&String::from_utf8_lossy(&output.stdout))
    }

    #[cfg(target_os = "windows")]
    {
        let script = "Get-PSDrive -PSProvider FileSystem | ForEach-Object { '{0}|{1}|{2}|{3}' -f $_.Name,$_.Root,$_.Used,$_.Free }";
        let Ok(output) = std::process::Command::new("powershell")
            .args(["-NoProfile", "-Command", script])
            .output()
        else {
            return vec![];
        };
        if !output.status.success() {
            return vec![];
        }
        parse_powershell_drives(&String::from_utf8_lossy(&output.stdout))
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        vec![]
    }
}

/// Parse `df -k` output (1 KiB blocks) into local volumes: the root volume
/// and `/Volumes/*` mounts, excluding Time Machine snapshots.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn parse_df_output(text: &str) -> Vec<VolumeInfo> {
    let mut volumes = Vec::new();
    let mut seen = HashSet::new();
    for line in text.lines().skip(1) {
        let Some(info) = parse_df_line(line) else {
            continue;
        };
        if seen.contains(&info.mount_point) {
            continue;
        }
        seen.insert(info.mount_point.clone());
        volumes.push(info);
    }
    volumes
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn parse_df_line(line: &str) -> Option<VolumeInfo> {
    let fields: Vec<&str> = line.split_whitespace().collect();
    if fields.len() < 9 {
        return None;
    }
    let device = fields[0];
    if device == "devfs" || device.starts_with("map") {
        return None;
    }
    let total_kib: u64 = fields[1].parse().ok()?;
    let free_kib: u64 = fields[3].parse().ok()?;
    let mount_point = fields[8..].join(" ");
    let last_segment = mount_point.rsplit('/').next().unwrap_or("");
    let relevant = mount_point == "/"
        || (mount_point.starts_with("/Volumes/") && !last_segment.starts_with("com.apple."));
    if !relevant {
        return None;
    }
    let total_bytes = total_kib.saturating_mul(1024);
    let free_bytes = free_kib.saturating_mul(1024);
    let used_bytes = total_bytes.saturating_sub(free_bytes);
    Some(VolumeInfo {
        name: last_segment.to_string(),
        mount_point,
        total_bytes,
        used_bytes,
        free_bytes,
    })
}

/// Parse the `Name|Root|Used|Free` lines produced by the Windows query.
/// Drives without size info (empty optical drives) fail to parse and drop.
#[cfg(target_os = "windows")]
fn parse_powershell_drives(text: &str) -> Vec<VolumeInfo> {
    let mut volumes = Vec::new();
    for line in text.lines() {
        let fields: Vec<&str> = line.split('|').collect();
        if fields.len() != 4 {
            continue;
        }
        let name = fields[0].trim();
        let mount_point = fields[1].trim();
        if name.is_empty() || mount_point.is_empty() {
            continue;
        }
        let Ok(used_bytes) = fields[2].trim().parse::<u64>() else {
            continue;
        };
        let Ok(free_bytes) = fields[3].trim().parse::<u64>() else {
            continue;
        };
        volumes.push(VolumeInfo {
            name: name.to_string(),
            mount_point: mount_point.to_string(),
            total_bytes: used_bytes.saturating_add(free_bytes),
            used_bytes,
            free_bytes,
        });
    }
    volumes
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::initialize;
    use std::fs;
    use std::time::Duration;
    use uuid::Uuid;

    fn temp_dir(tag: &str) -> PathBuf {
        // std::env::temp_dir() resolves to /var/folders/... on macOS, which
        // sits under the protected `/var` prefix and would rightly be rejected
        // by validate_write_path. Build fixtures under target/ instead —
        // outside the protected roots and never committed.
        let base = std::env::current_dir()
            .expect("current dir")
            .join("target/luma-cleaner-tests");
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

    fn whitelist_db() -> PathBuf {
        let dir = temp_dir("db");
        let db = dir.join("test.db");
        initialize(&db).unwrap();
        db
    }

    // ── Registry ─────────────────────────────────────────────────────────────

    #[test]
    fn registry_kinds_are_unique_and_classified() {
        let mut seen = HashSet::new();
        for def in KINDS {
            assert!(seen.insert(def.kind), "duplicate kind id: {}", def.kind);
            assert!(
                matches!(def.risk, "safe" | "caution" | "review"),
                "unknown risk level: {}",
                def.risk
            );
            assert!(!def.kind.is_empty());
        }
        assert!(KINDS.len() >= 8);
    }

    #[test]
    fn registry_kinds_resolve_roots_on_this_platform() {
        for def in KINDS {
            match location_roots(def.kind) {
                Some(roots) => {
                    assert!(!roots.is_empty(), "{} must have roots", def.kind);
                    for root in roots {
                        assert!(root.is_absolute(), "{} root must be absolute", def.kind);
                    }
                }
                // Acceptable platform gaps on Windows: the Recycle Bin (needs
                // elevated access) and Xcode concepts (simulators, build
                // artifacts like DerivedData).
                None => assert!(
                    cfg!(target_os = "windows")
                        && matches!(def.kind, "trash" | "simDevices" | "devDerived"),
                    "{} must resolve roots on this platform",
                    def.kind
                ),
            }
        }
    }

    #[test]
    fn trash_kind_is_never_executable() {
        let def = find_kind("trash").expect("trash kind registered");
        assert!(!def.executable);
        let error = execute_clean_with(&whitelist_db(), "trash", &[], &|_| Ok(()))
            .expect_err("trash must refuse execution");
        assert_eq!(error.code, "INVALID_CLEAN_KIND");
    }

    // ── Path matching ────────────────────────────────────────────────────────

    #[test]
    fn is_under_root_respects_boundaries() {
        assert!(is_under_root("/foo/bar", "/foo", false));
        assert!(is_under_root("/foo", "/foo", false));
        assert!(is_under_root("/foo/bar", "/foo/bar", false));
        assert!(!is_under_root("/foobar", "/foo", false));
        assert!(!is_under_root("/foo/baz", "/foo/bar", false));
        assert!(!is_under_root("/foo/bar/two", "/foo/bar/baz", false));
        // A root of "/" covers every absolute path.
        assert!(is_under_root("/anything", "/", false));
        // Trailing slashes on the root are ignored.
        assert!(is_under_root("/foo/bar", "/foo/", false));
    }

    #[test]
    fn is_under_root_supports_case_insensitive_matching() {
        assert!(!is_under_root("C:/Users/A", "c:/users/a", false));
        assert!(is_under_root("C:/Users/A", "c:/users/a", true));
        // Drive roots keep their slash semantics after trimming.
        assert!(is_under_root("C:/Users/A", "C:/", true));
        assert!(!is_under_root("D:/Users/A", "C:/", true));
    }

    #[test]
    fn overlaps_whitelist_checks_both_directions() {
        let whitelist = vec!["/Users/me/Library/Caches/app/keep".to_string()];
        // Unit inside a protected entry.
        assert!(overlaps_whitelist(
            "/Users/me/Library/Caches/app/keep/deep.bin",
            &whitelist,
            false
        ));
        // Protected entry inside the unit — the unit would swallow it.
        assert!(overlaps_whitelist(
            "/Users/me/Library/Caches/app",
            &whitelist,
            false
        ));
        // Sibling of a protected entry is fine.
        assert!(!overlaps_whitelist(
            "/Users/me/Library/Caches/app/other",
            &whitelist,
            false
        ));
    }

    // ── Whitelist storage ────────────────────────────────────────────────────

    #[test]
    fn whitelist_crud_round_trip() {
        let db = whitelist_db();
        // A native absolute path (not a hardcoded POSIX one): the store form
        // is platform-specific, so assert against what normalize returns.
        let keep = to_store_path(&temp_dir("crud").join("Keep Me"));
        let expected = keep.trim_end_matches('/').to_string();
        let entries = add_whitelist_entry(&db, &format!("{keep}/")).unwrap();
        assert!(entries.iter().any(|entry| entry.path == expected));
        // Duplicate insert is ignored, not an error.
        let entries = add_whitelist_entry(&db, &keep).unwrap();
        assert_eq!(
            entries
                .iter()
                .filter(|entry| entry.path == expected)
                .count(),
            1
        );
        let entries = remove_whitelist_entry(&db, &expected).unwrap();
        assert!(!entries.iter().any(|entry| entry.path == expected));
    }

    #[test]
    fn whitelist_rejects_relative_and_empty_paths() {
        let db = whitelist_db();
        assert!(add_whitelist_entry(&db, "relative/path").is_err());
        assert!(add_whitelist_entry(&db, "").is_err());
        assert!(add_whitelist_entry(&db, "  ").is_err());
        assert!(add_whitelist_entry(&db, "/a/../b").is_err());
    }

    // ── Collection ───────────────────────────────────────────────────────────

    /// Build a fixture tree and return its root:
    /// `cache/` has a sized child dir plus a stray file; `downloads/` has a
    /// top-level installer, a nested one, a text file and old/fresh zips.
    fn fixture(tag: &str) -> PathBuf {
        let dir = temp_dir(tag);
        write_file(&dir.join("cache/child-a/blob.bin"), 1_000);
        write_file(&dir.join("cache/child-a/nested/more.bin"), 500);
        write_file(&dir.join("cache/loose.tmp"), 100);
        write_file(&dir.join("downloads/App.dmg"), 2_000);
        write_file(&dir.join("downloads/sub/Inner.iso"), 3_000);
        write_file(&dir.join("downloads/notes.txt"), 10);
        write_file(&dir.join("downloads/old.zip"), 4_000);
        write_file(&dir.join("downloads/new.zip"), 5_000);
        dir
    }

    fn remove_fixture(dir: &Path) {
        fs::remove_dir_all(dir).ok();
    }

    /// Point a kind's roots at the fixture for one call. The registry itself
    /// stays untouched — roots are injected via `collect_units_in`.
    fn collect_with_roots(
        def: &'static KindDef,
        roots: Vec<PathBuf>,
        whitelist: &[String],
        budget: Duration,
    ) -> Collected {
        collect_units_in(def, roots, whitelist, Instant::now() + budget)
    }

    #[test]
    fn child_units_collect_direct_children_with_recursive_sizes() {
        let fix = fixture("child-units");
        let cache_root = fix.join("cache");
        let def = find_kind("userCaches").unwrap();
        let collected = collect_with_roots(def, vec![cache_root.clone()], &[], STATS_BUDGET);
        assert_eq!(collected.units.len(), 2, "child-a dir + loose.tmp");
        let unit = collected
            .units
            .iter()
            .find(|unit| unit.name == "child-a")
            .unwrap();
        assert_eq!(unit.size_bytes, 1_500, "recursive size of the child dir");
        assert_eq!(collected.total_bytes, 1_600);
        assert_eq!(collected.file_count, 3);
        assert!(!collected.is_partial);
        remove_fixture(&fix);
    }

    #[test]
    fn installer_units_match_extension_at_top_level_only() {
        let fix = fixture("installers");
        let downloads = fix.join("downloads");
        let def = find_kind("installers").unwrap();
        let collected = collect_with_roots(def, vec![downloads], &[], STATS_BUDGET);
        let names: Vec<&str> = collected
            .units
            .iter()
            .map(|unit| unit.name.as_str())
            .collect();
        assert_eq!(names, vec!["App.dmg"], "nested .iso and .txt are excluded");
        assert_eq!(collected.total_bytes, 2_000);
        remove_fixture(&fix);
    }

    #[test]
    fn old_download_units_respect_cutoff() {
        let fix = fixture("old-downloads");
        let downloads = fix.join("downloads");
        // Stamp one file 400 days in the past.
        let old_path = fix.join("downloads/old.zip");
        let file = fs::File::options().write(true).open(&old_path).unwrap();
        file.set_times(
            std::fs::FileTimes::new()
                .set_modified(SystemTime::now() - Duration::from_secs(400 * 86_400)),
        )
        .unwrap();
        drop(file);

        let def = KindDef {
            kind: "oldDownloadsTest",
            risk: "review",
            executable: true,
            mode: CollectMode::AllFiles {
                older_than_days: Some(180),
            },
            extensions: &[],
            reverify: false,
        };
        let collected = collect_units_in(&def, vec![downloads], &[], Instant::now() + STATS_BUDGET);
        let names: Vec<&str> = collected
            .units
            .iter()
            .map(|unit| unit.name.as_str())
            .collect();
        assert_eq!(names, vec!["old.zip"], "fresh files stay out of the cutoff");
        remove_fixture(&fix);
    }

    #[test]
    fn budget_exhaustion_marks_partial() {
        let fix = fixture("partial");
        let cache_root = fix.join("cache");
        let def = find_kind("userCaches").unwrap();
        let collected = collect_with_roots(def, vec![cache_root], &[], Duration::ZERO);
        assert!(collected.is_partial, "an expired budget must flag partial");
        remove_fixture(&fix);
    }

    #[test]
    fn whitelisted_units_are_filtered_from_listing() {
        let fix = fixture("whitelist");
        let cache_root = fix.join("cache");
        let def = find_kind("userCaches").unwrap();
        let whitelist = vec![to_store_path(&cache_root.join("loose.tmp"))];
        let collected = collect_with_roots(def, vec![cache_root], &whitelist, STATS_BUDGET);
        let names: Vec<&str> = collected
            .units
            .iter()
            .map(|unit| unit.name.as_str())
            .collect();
        assert!(!names.contains(&"loose.tmp"), "whitelisted unit is hidden");
        assert!(names.contains(&"child-a"));
        remove_fixture(&fix);
    }

    // ── Execution safety ─────────────────────────────────────────────────────

    fn clean_item(path: &Path, size: u64) -> CleanRequestItem {
        CleanRequestItem {
            path: to_store_path(path),
            size_bytes: size,
        }
    }

    #[test]
    fn execute_rejects_paths_outside_registered_roots() {
        let fix = fixture("outside");
        let db = whitelist_db();
        let downloads = fix.join("downloads");
        let def_roots = vec![to_store_path(&downloads)];
        let def = find_kind("installers").unwrap();

        let outside = fix.join("elsewhere/App.dmg");
        write_file(&outside, 2_000);
        let outcome = execute_clean_with_roots(
            &db,
            def,
            &def_roots,
            &[clean_item(&outside, 2_000)],
            &|_| Ok(()),
        )
        .unwrap();
        assert!(outcome.succeeded.is_empty());
        assert_eq!(outcome.failed.len(), 1);
        assert!(outcome.failed[0].reason.contains("清理范围"));
        remove_fixture(&fix);
    }

    #[test]
    fn execute_rejects_non_direct_children_for_cache_kinds() {
        let fix = fixture("deep");
        let db = whitelist_db();
        let cache_root = fix.join("cache");
        let def_roots = vec![to_store_path(&cache_root)];
        let def = find_kind("userCaches").unwrap();

        let nested = fix.join("cache/child-a/nested");
        let outcome =
            execute_clean_with_roots(&db, def, &def_roots, &[clean_item(&nested, 500)], &|_| {
                Ok(())
            })
            .unwrap();
        assert!(outcome.succeeded.is_empty());
        assert!(outcome.failed[0].reason.contains("一级子项"));
        remove_fixture(&fix);
    }

    #[test]
    fn execute_refuses_whitelisted_units() {
        let fix = fixture("refuse-whitelist");
        let db = whitelist_db();
        let cache_root = fix.join("cache");
        let target = cache_root.join("loose.tmp");
        add_whitelist_entry(&db, &to_store_path(&target)).unwrap();

        let def_roots = vec![to_store_path(&cache_root)];
        let def = find_kind("userCaches").unwrap();
        let outcome =
            execute_clean_with_roots(&db, def, &def_roots, &[clean_item(&target, 100)], &|_| {
                Ok(())
            })
            .unwrap();
        assert!(outcome.succeeded.is_empty());
        assert_eq!(outcome.failed.len(), 1);
        remove_fixture(&fix);
    }

    #[test]
    fn execute_fails_closed_when_whitelist_unreadable() {
        let fix = fixture("fail-closed");
        let dir = temp_dir("fail-closed-db");
        let db = dir.join("broken.db");
        initialize(&db).unwrap();
        // Drop the whitelist table: loading must now fail, and cleaning must
        // refuse to run rather than operate unprotected.
        {
            let connection = database::open(&db).unwrap();
            connection
                .execute("DROP TABLE cleaner_whitelist", [])
                .unwrap();
        }
        let cache_root = fix.join("cache");
        let def_roots = vec![to_store_path(&cache_root)];
        let def = find_kind("userCaches").unwrap();
        let error = execute_clean_with_roots(
            &db,
            def,
            &def_roots,
            &[clean_item(&cache_root.join("loose.tmp"), 100)],
            &|_| Ok(()),
        )
        .expect_err("unreadable whitelist must refuse execution");
        assert_eq!(error.code, "DATABASE_ERROR");
        fs::remove_dir_all(dir).ok();
        remove_fixture(&fix);
    }

    #[test]
    fn execute_rejects_size_drift_and_missing_files() {
        let fix = fixture("drift");
        let db = whitelist_db();
        let downloads = fix.join("downloads");
        let def_roots = vec![to_store_path(&downloads)];
        let def = find_kind("installers").unwrap();

        // Size no longer matches what was listed.
        let drifted = fix.join("downloads/App.dmg");
        let outcome =
            execute_clean_with_roots(&db, def, &def_roots, &[clean_item(&drifted, 999)], &|_| {
                Ok(())
            })
            .unwrap();
        assert_eq!(outcome.failed.len(), 1);

        // File disappeared after listing.
        let missing = fix.join("downloads/Ghost.dmg");
        let outcome = execute_clean_with_roots(
            &db,
            def,
            &def_roots,
            &[clean_item(&missing, 1_000)],
            &|_| Ok(()),
        )
        .unwrap();
        assert_eq!(outcome.failed.len(), 1);
        assert!(outcome.succeeded.is_empty());
        remove_fixture(&fix);
    }

    #[test]
    fn execute_trashes_confirmed_units_and_logs_operation() {
        let fix = fixture("execute");
        let db = whitelist_db();
        let cache_root = fix.join("cache");
        let target = cache_root.join("loose.tmp");
        let def_roots = vec![to_store_path(&cache_root)];
        let def = find_kind("userCaches").unwrap();

        // Fake deleter: rename instead of touching the real OS trash.
        let moved = target.with_extension("tmp.gone");
        let moved_for_closure = moved.clone();
        let outcome = execute_clean_with_roots(
            &db,
            def,
            &def_roots,
            &[clean_item(&target, 100)],
            &move |native: &Path| {
                let destination = moved_for_closure.clone();
                std::fs::rename(native, destination).map_err(|error| error.to_string())
            },
        )
        .unwrap();

        assert_eq!(outcome.succeeded, vec![to_store_path(&target)]);
        assert!(outcome.failed.is_empty());
        assert!(!target.exists());
        assert!(moved.exists());
        // The run is visible in the operation log.
        let connection = database::open(&db).unwrap();
        let label: String = connection
            .query_row(
                "SELECT label FROM file_operations WHERE id = ?1",
                [outcome.operation_id.as_deref().unwrap()],
                |row| row.get(0),
            )
            .unwrap();
        assert!(label.contains("磁盘管家"));
        remove_fixture(&fix);
    }

    #[test]
    fn unknown_kind_is_rejected() {
        let db = whitelist_db();
        let error =
            execute_clean_with(&db, "nope", &[], &|_| Ok(())).expect_err("unknown kind must fail");
        assert_eq!(error.code, "INVALID_CLEAN_KIND");
    }

    // ── Simulator device parsing (CLN-301) ───────────────────────────────────

    #[test]
    #[cfg(target_os = "macos")]
    fn simctl_json_parses_unavailable_devices_only() {
        let value = serde_json::json!({
            "devices": {
                "com.apple.CoreSimulator.SimRuntime.iOS-17-5": [
                    { "udid": "A", "name": "iPhone 15", "isAvailable": false,
                      "dataPath": "/sim/Devices/A/data" },
                    { "udid": "B", "name": "iPhone 15", "isAvailable": true,
                      "dataPath": "/sim/Devices/B/data" },
                    // No isAvailable flag — unknown, must be skipped.
                    { "udid": "C", "name": "Mystery", "dataPath": "/sim/Devices/C/data" },
                    // No dataPath — cannot map to a deletion unit, skipped.
                    { "udid": "D", "name": "Ghost", "isAvailable": false }
                ]
            }
        });
        let devices = parse_simctl_devices(&value);
        assert_eq!(devices.len(), 1, "only explicitly-unavailable devices pass");
        assert_eq!(devices[0].path, PathBuf::from("/sim/Devices/A"));
        assert_eq!(devices[0].label, "iPhone 15 (iOS-17-5)");
    }

    // ── Volume parsing ───────────────────────────────────────────────────────

    #[test]
    #[cfg(target_os = "macos")]
    fn df_output_parses_local_volumes() {
        let text = concat!(
            "Filesystem   1024-blocks      Used Available Capacity iused      ifree %iused  Mounted on\n",
            "/dev/disk3s1s1   494384472 229948400 241121072    49% 1576121 4822421460    0%   /\n",
            "/dev/disk3s6     494384472  22994840 241121072     9%       0 4822421460    0%   /System/Volumes/VM\n",
            "devfs                 380        380       0   100%     652          0  100%   /dev\n",
            "map auto_home           0          0       0   100%       0          0  100%   /System/Volumes/Data/home\n",
            "/dev/disk5s1      15656960   9642128  6014832    62%       5 15656955    0%   /Volumes/USB DRIVE\n",
            "/dev/disk4s1      15656960   9642128  6014832    62%       5 15656955    0%   /Volumes/com.apple.TimeMachine.localsnapshots\n"
        );
        let volumes = parse_df_output(text);
        let mounts: Vec<&str> = volumes
            .iter()
            .map(|volume| volume.mount_point.as_str())
            .collect();
        assert_eq!(mounts, vec!["/", "/Volumes/USB DRIVE"]);
        let root = &volumes[0];
        assert_eq!(root.total_bytes, 494_384_472 * 1024);
        assert_eq!(root.free_bytes, 241_121_072 * 1024);
        assert_eq!(root.used_bytes, (494_384_472 - 241_121_072) * 1024);
    }

    #[test]
    #[cfg(target_os = "windows")]
    fn powershell_drive_output_parses() {
        let text = "C|C:\\|123456|654321\nD|D:\\|not-a-number|100\n";
        let volumes = parse_powershell_drives(text);
        assert_eq!(volumes.len(), 1);
        assert_eq!(volumes[0].name, "C");
        assert_eq!(volumes[0].total_bytes, 123_456 + 654_321);
    }

    // ── Overview ─────────────────────────────────────────────────────────────

    #[test]
    fn overview_only_lists_kinds_with_existing_roots() {
        // Runs against the real home: roots that do not exist must be absent,
        // and the overview must always build (read-only walk, budgeted).
        let overview = build_cleaner_overview(&whitelist_db(), 180).unwrap();
        for location in &overview.locations {
            assert!(!location.roots.is_empty());
            assert!(matches!(
                location.risk.as_str(),
                "safe" | "caution" | "review"
            ));
        }
    }

    #[test]
    fn overview_fails_closed_when_whitelist_unreadable() {
        let dir = temp_dir("overview-db");
        let db = dir.join("broken.db");
        initialize(&db).unwrap();
        {
            let connection = database::open(&db).unwrap();
            connection
                .execute("DROP TABLE cleaner_whitelist", [])
                .unwrap();
        }
        let error = build_cleaner_overview(&db, 180).expect_err("must fail closed");
        assert_eq!(error.code, "DATABASE_ERROR");
        fs::remove_dir_all(dir).ok();
    }
}
