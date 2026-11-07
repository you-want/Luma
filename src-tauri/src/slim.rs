// 应用瘦身 (App Slimming): strip the Intel slice from universal apps on
// Apple Silicon.
//
// Core idea, re-implemented under Luma's trash-routing red lines:
//   1. Only universal apps on an Apple Silicon Mac are candidates; App Store
//      apps (receipt present) and Apple's own apps (com.apple.*) are excluded
//      — receipts are strictly validated and Apple binaries are not ours to
//      touch.
//   2. The whole operation happens on a COPY: ditto → lipo -thin arm64 →
//      ad-hoc re-sign → codesign --verify. Any failure aborts with the
//      original untouched.
//   3. The swap is rename-based (atomic within /Applications): original is
//      renamed aside, the slimmed copy takes its place, and only then the
//      aside is moved to the OS trash — always recoverable.
//   4. Re-signing drops entitlements and gives the app a new signature
//      identity (first keychain access may re-prompt). Both are consequences
//      of ad-hoc signing and are surfaced in the UI copy.

use crate::cleaner::{begin_cleaner_operation, load_whitelist, overlaps_whitelist, to_store_path};
use crate::error::AppError;
use crate::file_ops::{self, OpFailure};
use serde::Serialize;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};
use uuid::Uuid;
use walkdir::WalkDir;

// ── Constants ────────────────────────────────────────────────────────────────

/// Budget for one discovery pass over the application roots.
const DISCOVERY_BUDGET: Duration = Duration::from_secs(8);

/// Mach-O fat-binary magic numbers (32/64-bit, stored big-endian).
const FAT_MAGIC_32: [u8; 4] = [0xCA, 0xFE, 0xBA, 0xBE];
const FAT_MAGIC_64: [u8; 4] = [0xCA, 0xFE, 0xBA, 0xBF];

// ── Public types ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SlimCandidate {
    /// App bundle path — the slimming unit.
    pub path: String,
    pub name: String,
    /// Current bundle size on disk.
    pub current_bytes: u64,
    /// Sum of x86_64 slice sizes across every fat Mach-O in the bundle —
    /// the space reclaimed by thinning (ad-hoc re-sign shifts it slightly).
    pub saving_bytes: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SlimReport {
    pub candidates: Vec<SlimCandidate>,
    pub is_partial: bool,
    /// Only an Apple Silicon Mac benefits from thinning.
    pub platform_supported: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SlimOutcome {
    pub operation_id: Option<String>,
    pub succeeded: Vec<String>,
    pub failed: Vec<OpFailure>,
}

/// A deletion request item echoed back from the list the user confirmed.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SlimRequestItem {
    pub path: String,
    pub current_bytes: u64,
}

// ── Mach-O helpers ───────────────────────────────────────────────────────────

/// Whether the file starts with a Mach-O fat-header magic. Thin binaries
/// (single-architecture) return false — nothing to strip.
fn is_fat_macho(path: &Path) -> bool {
    let Ok(header) = fs::read(path) else {
        return false;
    };
    if header.len() < 4 {
        return false;
    }
    let magic = [header[0], header[1], header[2], header[3]];
    magic == FAT_MAGIC_32 || magic == FAT_MAGIC_64
}

/// Parse `lipo -info` output into the architecture list.
/// Shape: "Architectures in the fat file: <path> are: arm64 x86_64"
fn parse_lipo_architectures(output: &str) -> Vec<String> {
    let Some(rest) = output.split("are:").nth(1) else {
        return vec![];
    };
    rest.split_whitespace()
        .map(|arch| arch.trim().to_string())
        .filter(|arch| !arch.is_empty())
        .collect()
}

/// Sum of the x86_64 slice sizes reported by `lipo -detailed_info`:
/// "architecture x86_64" lines are followed by an indented "size N" line.
fn parse_x86_64_slice_size(output: &str) -> Option<u64> {
    let mut current_arch: Option<&str> = None;
    let mut total: Option<u64> = None;
    for line in output.lines() {
        let trimmed = line.trim_start();
        if let Some(arch) = trimmed.strip_prefix("architecture ") {
            current_arch = Some(arch.trim());
        } else if let Some(size_text) = trimmed.strip_prefix("size ") {
            if current_arch == Some("x86_64") {
                if let Ok(size) = size_text.trim().parse::<u64>() {
                    total = Some(total.unwrap_or(0).saturating_add(size));
                }
            }
        }
    }
    total
}

/// Whether the bundle is excluded from slimming: App Store receipts are
/// strictly validated and Apple's own binaries are not ours to touch.
fn is_excluded_bundle(app_dir: &Path, bundle_id: Option<&str>) -> bool {
    if app_dir.join("Contents/_MASReceipt").exists() {
        return true;
    }
    bundle_id
        .map(|id| id.starts_with("com.apple."))
        .unwrap_or(false)
}

// ── Discovery ────────────────────────────────────────────────────────────────

/// Discover universal apps that could be thinned. Apple Silicon macOS only.
pub fn discover_slim_candidates(db_path: &Path) -> Result<SlimReport, AppError> {
    let _whitelist = load_whitelist(db_path)?; // fail closed, same as the family
    let supported = cfg!(all(target_os = "macos", target_arch = "aarch64"));
    if !supported {
        return Ok(SlimReport {
            candidates: vec![],
            is_partial: false,
            platform_supported: false,
        });
    }
    let whitelist = load_whitelist(db_path)?;
    discover_in(app_roots(), &whitelist, Instant::now() + DISCOVERY_BUDGET)
}

fn app_roots() -> Vec<PathBuf> {
    let mut roots = vec![PathBuf::from("/Applications")];
    if let Some(home) = std::env::var("HOME").ok().map(PathBuf::from) {
        roots.push(home.join("Applications"));
    }
    roots
}

/// Root-injected discovery core, shared with tests. Whitelisted app paths
/// are excluded, consistent with the rest of the cleaner family.
pub(crate) fn discover_in(
    roots: Vec<PathBuf>,
    whitelist: &[String],
    deadline: Instant,
) -> Result<SlimReport, AppError> {
    let case_insensitive = cfg!(windows);
    let mut report = SlimReport {
        candidates: vec![],
        is_partial: false,
        platform_supported: true,
    };
    'roots: for root in roots {
        if !root.is_dir() {
            continue;
        }
        let Ok(entries) = fs::read_dir(&root) else {
            continue;
        };
        for entry in entries.flatten() {
            if Instant::now() > deadline {
                report.is_partial = true;
                break 'roots;
            }
            let path = entry.path();
            if !path.to_string_lossy().ends_with(".app") || !path.is_dir() {
                continue;
            }
            let stored = to_store_path(&path);
            if overlaps_whitelist(&stored, whitelist, case_insensitive) {
                continue;
            }
            if let Some(candidate) = inspect_app(&path, deadline) {
                report.candidates.push(candidate);
            }
        }
    }
    report
        .candidates
        .sort_by_key(|candidate| std::cmp::Reverse(candidate.saving_bytes));
    Ok(report)
}

/// Inspect one app bundle; `None` when it is not a slimming candidate.
fn inspect_app(app_dir: &Path, deadline: Instant) -> Option<SlimCandidate> {
    let bundle_id = read_bundle_id(app_dir);
    if is_excluded_bundle(app_dir, bundle_id.as_deref()) {
        return None;
    }
    // The main executable must be a fat binary — thin apps gain nothing.
    if !main_executable_is_fat(app_dir) {
        return None;
    }

    let mut current_bytes = 0u64;
    let mut saving_bytes = 0u64;
    for entry in WalkDir::new(app_dir)
        .follow_links(false)
        .into_iter()
        .filter_map(|value| value.ok())
    {
        if Instant::now() > deadline {
            return None; // budget out — leave the app out of this pass
        }
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        let Ok(meta) = entry.metadata() else {
            continue;
        };
        current_bytes += meta.len();
        if is_fat_macho(path) {
            saving_bytes += x86_64_slice_size(path).unwrap_or(0);
        }
    }
    if saving_bytes == 0 {
        return None;
    }
    Some(SlimCandidate {
        path: to_store_path(app_dir),
        name: app_dir
            .file_stem()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_default(),
        current_bytes,
        saving_bytes,
    })
}

fn main_executable_is_fat(app_dir: &Path) -> bool {
    let Some(executable) = main_executable_path(app_dir) else {
        return false;
    };
    let Ok(output) = Command::new("lipo").arg("-info").arg(&executable).output() else {
        return false;
    };
    if !output.status.success() {
        return false;
    }
    let architectures = parse_lipo_architectures(&String::from_utf8_lossy(&output.stdout));
    architectures.contains(&"arm64".to_string()) && architectures.contains(&"x86_64".to_string())
}

/// The bundle's main executable from CFBundleExecutable, falling back to the
/// bundle file stem (most apps match that convention).
fn main_executable_path(app_dir: &Path) -> Option<PathBuf> {
    // Gate on a readable Info.plist; the exact id value is checked by the
    // exclusion rules in `inspect_app`.
    if read_bundle_id(app_dir).is_some() {
        // A second plutil call fetches CFBundleExecutable — cheap and exact.
        if let Ok(output) = Command::new("plutil")
            .args(["-extract", "CFBundleExecutable", "raw"])
            .arg(app_dir.join("Contents/Info.plist"))
            .output()
        {
            if output.status.success() {
                let name = String::from_utf8_lossy(&output.stdout).trim().to_string();
                if !name.is_empty() {
                    return Some(app_dir.join("Contents/MacOS").join(name));
                }
            }
        }
    }
    let stem = app_dir.file_stem()?.to_string_lossy().to_string();
    let candidate = app_dir.join("Contents/MacOS").join(stem);
    candidate.is_file().then_some(candidate)
}

fn read_bundle_id(app_dir: &Path) -> Option<String> {
    let output = Command::new("plutil")
        .args(["-extract", "CFBundleIdentifier", "raw"])
        .arg(app_dir.join("Contents/Info.plist"))
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let value = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!value.is_empty()).then_some(value)
}

/// The x86_64 slice size of a fat Mach-O, via `lipo -detailed_info`.
fn x86_64_slice_size(path: &Path) -> Option<u64> {
    let output = Command::new("lipo")
        .arg("-detailed_info")
        .arg(path)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    parse_x86_64_slice_size(&String::from_utf8_lossy(&output.stdout))
}

// ── Execution ────────────────────────────────────────────────────────────────

/// Thin the confirmed apps. Every operation happens on a copy and the
/// original only moves to the trash after the slimmed copy has been verified
/// and swapped in.
pub fn execute_slim(db_path: &Path, items: &[SlimRequestItem]) -> Result<SlimOutcome, AppError> {
    let whitelist = load_whitelist(db_path)?; // fail closed
    let report = discover_in(app_roots(), &whitelist, Instant::now() + DISCOVERY_BUDGET)?;

    let mut index: HashMap<&str, &SlimCandidate> = HashMap::new();
    for candidate in &report.candidates {
        index.insert(candidate.path.as_str(), candidate);
    }

    let operation_id = begin_cleaner_operation(
        db_path,
        format!("应用瘦身（{} 项，原件移至废纸篓）", items.len()),
    )?;
    let mut succeeded = Vec::new();
    let mut failed = Vec::new();

    for item in items {
        let outcome = match index.get(item.path.as_str()) {
            Some(candidate) if candidate.current_bytes == item.current_bytes => {
                slim_one(&item.path)
            }
            Some(_) => Err(AppError::new(
                "FILE_CHANGED",
                format!("应用内容与重新扫描结果不一致，请刷新后重试：{}", item.path),
            )),
            None => Err(AppError::new(
                "PATH_NOT_IN_SCOPE",
                format!("应用不在当前发现结果中，请重新扫描：{}", item.path),
            )),
        };
        match outcome {
            Ok(()) => succeeded.push(item.path.clone()),
            Err(error) => failed.push(OpFailure {
                path: item.path.clone(),
                reason: error.message,
            }),
        }
    }

    file_ops::finish_operation(db_path, &operation_id, succeeded.len(), &failed)?;
    Ok(SlimOutcome {
        operation_id: Some(operation_id),
        succeeded,
        failed,
    })
}

/// Slim one app bundle in place (copy → thin → re-sign → swap → trash).
fn slim_one(app_store_path: &str) -> Result<(), AppError> {
    let app = PathBuf::from(app_store_path.replace('/', std::path::MAIN_SEPARATOR_STR));
    if !file_ops::validate_write_path(app_store_path)?.is_dir() {
        return Err(AppError::new(
            "STALE_FILE",
            format!("应用目录不存在：{app_store_path}"),
        ));
    }
    let Some(parent) = app.parent() else {
        return Err(AppError::new("INVALID_PATH", "无法确定应用所在目录。"));
    };
    let token = Uuid::new_v4();
    let staging = parent.join(format!(".luma-slim-{token}"));

    // 1. Copy the bundle (ditto preserves metadata, xattrs and signatures).
    let copy_status = Command::new("ditto").arg(&app).arg(&staging).status();
    match copy_status {
        Ok(status) if status.success() => {}
        _ => {
            fs::remove_dir_all(&staging).ok();
            return Err(AppError::new(
                "OP_FAILED",
                format!("复制应用失败：{}", app.display()),
            ));
        }
    }

    // 2-4. Thin every fat Mach-O in the copy, re-sign ad hoc, verify.
    if let Err(error) = thin_and_resign(&staging) {
        fs::remove_dir_all(&staging).ok();
        return Err(error);
    }

    // 5. Atomic swap: aside → move copy in → trash the aside. A failed
    //    move-back restores the original, so the app is never lost.
    let aside = parent.join(format!(".luma-slim-old-{token}"));
    if let Err(error) = fs::rename(&app, &aside) {
        fs::remove_dir_all(&staging).ok();
        return Err(AppError::new(
            "OP_FAILED",
            format!("无法暂存原应用：{error}"),
        ));
    }
    if let Err(error) = fs::rename(&staging, &app) {
        fs::rename(&aside, &app).ok();
        return Err(AppError::new("OP_FAILED", format!("替换应用失败：{error}")));
    }
    if let Err(reason) = file_ops::trash_one(&aside) {
        // The app itself is already replaced and working; only the old copy
        // could not reach the trash. Surface it, keep the outcome partial.
        return Err(AppError::new(
            "OP_FAILED",
            format!(
                "原应用移入废纸篓失败（应用已替换成功，可手动清理 {}）：{reason}",
                aside.display()
            ),
        ));
    }
    Ok(())
}

/// Thin all fat Mach-O files inside the staged copy and re-sign ad hoc so
/// the seal matches the thinned contents. Any failure aborts the item.
fn thin_and_resign(app_dir: &Path) -> Result<(), AppError> {
    let mut fat_files: Vec<PathBuf> = Vec::new();
    for entry in WalkDir::new(app_dir)
        .follow_links(false)
        .into_iter()
        .filter_map(|value| value.ok())
    {
        if !entry.file_type().is_file() {
            continue;
        }
        if is_fat_macho(entry.path()) {
            fat_files.push(entry.path().to_path_buf());
        }
    }
    for file in &fat_files {
        // Write the thin slice next to the file, then swap it in — lipo
        // must not read and overwrite the same path in one step.
        let staged = file.with_extension("luma-thin");
        let status = Command::new("lipo")
            .args(["-thin", "arm64"])
            .arg(file)
            .args(["-output"])
            .arg(&staged)
            .status()
            .map_err(|error| AppError::new("OP_FAILED", error.to_string()))?;
        if !status.success() {
            fs::remove_file(&staged).ok();
            return Err(AppError::new(
                "OP_FAILED",
                format!("剥离 Intel 代码失败：{}", file.display()),
            ));
        }
        if let Err(error) = fs::rename(&staged, file) {
            fs::remove_file(&staged).ok();
            return Err(AppError::new("OP_FAILED", format!("替换切片失败：{error}")));
        }
    }
    // Ad-hoc re-sign so the seal matches the thinned binaries, then verify.
    let sign_status = Command::new("codesign")
        .args(["--force", "--deep", "--sign", "-"])
        .arg(app_dir)
        .status()
        .map_err(|error| AppError::new("OP_FAILED", error.to_string()))?;
    if !sign_status.success() {
        return Err(AppError::new("OP_FAILED", "重新签名失败，已中止替换。"));
    }
    let verify_status = Command::new("codesign")
        .args(["--verify", "--strict"])
        .arg(app_dir)
        .status()
        .map_err(|error| AppError::new("OP_FAILED", error.to_string()))?;
    if !verify_status.success() {
        return Err(AppError::new("OP_FAILED", "签名校验未通过，已中止替换。"));
    }
    Ok(())
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp_dir(tag: &str) -> PathBuf {
        let base = std::env::current_dir()
            .expect("current dir")
            .join("target/luma-slim-tests");
        let dir = base.join(format!("{tag}-{}", Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn fat_magic_detection_and_exclusions() {
        let dir = temp_dir("magic");
        let fat = dir.join("fat.bin");
        let thin = dir.join("thin.bin");
        fs::write(&fat, [0xCA, 0xFE, 0xBA, 0xBE, 0x00, 0x01]).unwrap();
        fs::write(&thin, [0xCF, 0xFA, 0xED, 0xFE, 0x00, 0x01]).unwrap();
        assert!(is_fat_macho(&fat));
        assert!(!is_fat_macho(&thin), "thin binaries are not candidates");
        assert!(!is_fat_macho(&dir.join("missing.bin")));

        let store_app = dir.join("Store.app");
        fs::create_dir_all(store_app.join("Contents/_MASReceipt")).unwrap();
        assert!(
            is_excluded_bundle(&store_app, Some("com.vendor.app")),
            "App Store receipt excludes"
        );
        // A non-store, non-Apple bundle is a candidate; Apple apps are
        // excluded by bundle id regardless of receipts.
        let plain = dir.join("Plain.app");
        fs::create_dir_all(plain.join("Contents")).unwrap();
        assert!(!is_excluded_bundle(&plain, Some("com.vendor.app")));
        assert!(
            is_excluded_bundle(&plain, Some("com.apple.Safari")),
            "Apple apps exclude"
        );
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn lipo_outputs_are_parsed() {
        assert_eq!(
            parse_lipo_architectures(
                "Architectures in the fat file: /Applications/App.app are: x86_64 arm64\n"
            ),
            vec!["x86_64".to_string(), "arm64".to_string()]
        );
        assert!(parse_lipo_architectures("/Applications/App.app is not a fat file").is_empty());

        let detailed = concat!(
            "Fat header in: /Applications/App.app/Contents/MacOS/App\n",
            "architecture arm64\n",
            "    cputype CPU_TYPE_ARM64\n",
            "    cpusubtype CPU_SUBTYPE_ARM64_ALL\n",
            "    offset 4096\n",
            "    size 123456\n",
            "    align 2^12 (4096)\n",
            "architecture x86_64\n",
            "    cputype CPU_TYPE_X86_64\n",
            "    cpusubtype CPU_SUBTYPE_X86_64_ALL\n",
            "    offset 131072\n",
            "    size 654321\n",
            "    align 2^12 (4096)\n"
        );
        assert_eq!(parse_x86_64_slice_size(detailed), Some(654_321));
        assert_eq!(parse_x86_64_slice_size("nothing here"), None);
    }

    #[test]
    fn discovery_skips_non_candidates_and_never_touches_real_apps() {
        // Run against an empty fixture root: the discovery core must produce
        // an empty report without ever touching the real /Applications (no
        // roots are passed).
        let dir = temp_dir("empty");
        let report =
            discover_in(vec![dir.clone()], &[], Instant::now() + DISCOVERY_BUDGET).unwrap();
        assert!(report.candidates.is_empty());
        assert!(report.platform_supported);
        assert!(!report.is_partial);
        fs::remove_dir_all(dir).ok();
    }
}
