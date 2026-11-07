// 项目瘦身 (Project Slimming): find rebuildable build artifacts inside
// development projects, grouped per project, with activity protection.
//
// Implemented under Luma's stricter rules:
//   1. Discovery is bounded: only registered dev-directory locations under the
//      user's home, limited depth — never a whole-home walk.
//   2. Marker-file gating: a directory only counts as build output when the
//      enclosing project carries the matching marker file (package.json for
//      node_modules, Cargo.toml for target, pom.xml for Maven's target, …).
//      A stray folder literally named `target` or `build` is never touched.
//   3. Activity protection: a project modified within the last 7 days is
//      active — its artifacts stay out of cleanup, and the executor refuses
//      them even if a stale client request asks for them.
//   4. Execution re-discovers projects and only trashes artifacts that appear
//      in the fresh discovery with an unchanged size (deletion-boundary
//      re-verification). Whitelisted or system-protected paths are rejected,
//      and every deletion routes to the OS trash.

use crate::cleaner::{
    begin_cleaner_operation, load_whitelist, modified_secs, now_secs, overlaps_whitelist,
    to_store_path, CleanOutcome, CleanRequestItem,
};
use crate::error::AppError;
use crate::file_ops::{self, OpFailure};
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};
use walkdir::WalkDir;

// ── Constants ────────────────────────────────────────────────────────────────

/// Projects touched within this window are considered active and protected.
pub const ACTIVE_WINDOW_DAYS: u64 = 7;

/// Wall-clock budget for one full discovery pass. Discovery runs on demand
/// (the user opens the panel or confirms a cleanup), never in the background.
const DISCOVERY_BUDGET: Duration = Duration::from_secs(10);

/// A project directory sits at most this many levels below its dev root.
const PROJECT_MAX_DEPTH: usize = 3;
/// Artifact directories are matched at most this deep inside a project
/// (covers monorepo layouts like `apps/web/node_modules`).
const ARTIFACT_MAX_DEPTH: usize = 2;

// ── Public types ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactGroup {
    /// Stable group key for frontend labels and rebuild hints.
    pub group: String,
    /// Artifact directory path — the deletion unit.
    pub path: String,
    pub size_bytes: u64,
    pub file_count: u64,
    /// The project has a .git repository and git reports files under this
    /// artifact as tracked. Tracked artifacts are excluded from cleanup:
    /// deleting them would dirty the working tree and they may be committed
    /// intentionally. A project with
    /// .git whose git invocation fails is treated as tracked (fail closed).
    pub git_tracked: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PurgeProject {
    pub path: String,
    pub name: String,
    /// Total size of the project directory (may include nested projects'
    /// artifacts; display only — execution works per artifact path).
    pub total_bytes: u64,
    pub last_active_at: Option<i64>,
    /// Modified within `ACTIVE_WINDOW_DAYS` — artifacts are not cleanable.
    pub is_active: bool,
    pub artifacts: Vec<ArtifactGroup>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PurgeReport {
    pub projects: Vec<PurgeProject>,
    /// Dev roots that exist and were scanned (canonical `/` separators).
    pub scanned_roots: Vec<String>,
    /// The discovery budget ran out; results are a floor.
    pub is_partial: bool,
}

// ── Registry ─────────────────────────────────────────────────────────────────

/// (marker file, artifact directory name, group key). An artifact directory is
/// only recognized when the project directly contains the marker file.
const ARTIFACT_RULES: &[(&str, &str, &str)] = &[
    ("package.json", "node_modules", "nodeModules"),
    ("package.json", ".next", "nextBuild"),
    ("package.json", "dist", "distBuild"),
    ("package.json", "build", "distBuild"),
    ("Cargo.toml", "target", "rustTarget"),
    ("pom.xml", "target", "mavenTarget"),
    ("build.gradle", "build", "gradleBuild"),
    ("build.gradle", ".gradle", "gradleBuild"),
    ("pyproject.toml", ".venv", "pyVenv"),
    ("pyproject.toml", "venv", "pyVenv"),
    ("requirements.txt", ".venv", "pyVenv"),
    ("requirements.txt", "venv", "pyVenv"),
    ("setup.py", ".venv", "pyVenv"),
    ("setup.py", "venv", "pyVenv"),
];

/// Marker file names that turn a directory into a project candidate.
fn is_marker_file(name: &str) -> bool {
    ARTIFACT_RULES.iter().any(|(marker, _, _)| *marker == name)
}

/// The artifact group for `dir_name`, gated by the project's marker set.
fn artifact_group_for(dir_name: &str, markers: &HashSet<&str>) -> Option<&'static str> {
    ARTIFACT_RULES
        .iter()
        .find(|(marker, artifact, _)| *artifact == dir_name && markers.contains(marker))
        .map(|(_, _, group)| *group)
}

/// Registered development locations under the user's home, most common first.
fn dev_roots() -> Vec<PathBuf> {
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    {
        let home = match std::env::var("HOME")
            .or_else(|_| std::env::var("USERPROFILE"))
            .ok()
            .map(PathBuf::from)
        {
            Some(home) => home,
            None => return vec![],
        };
        [
            "Projects",
            "Developer",
            "Code",
            "code",
            "dev",
            "workspace",
            "workspaces",
            "src",
            "source",
            "Documents",
            "Desktop",
        ]
        .iter()
        .map(|name| home.join(name))
        .collect()
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        vec![]
    }
}

// ── Discovery ────────────────────────────────────────────────────────────────

pub fn discover_projects(db_path: &Path) -> Result<PurgeReport, AppError> {
    // Fail closed: the report drives cleanup suggestions, so an unreadable
    // whitelist stops the whole feature, mirroring `cleaner.rs`.
    let _whitelist = load_whitelist(db_path)?;
    let roots = dev_roots();
    discover_projects_in(roots, Instant::now() + DISCOVERY_BUDGET)
}

/// Root-injected discovery core, shared with tests.
pub(crate) fn discover_projects_in(
    roots: Vec<PathBuf>,
    deadline: Instant,
) -> Result<PurgeReport, AppError> {
    let mut report = PurgeReport {
        projects: Vec::new(),
        scanned_roots: Vec::new(),
        is_partial: false,
    };

    for root in roots {
        if !root.is_dir() {
            continue;
        }
        report.scanned_roots.push(to_store_path(&root));

        // Phase 1: find project directories and their marker files.
        let Some(projects) = find_projects(&root, deadline) else {
            report.is_partial = true;
            break;
        };
        if projects.is_empty() {
            continue;
        }

        // Phase 2: per project, measure size/activity and match artifacts.
        for (project_path, markers) in projects {
            if Instant::now() > deadline {
                report.is_partial = true;
                break;
            }
            report
                .projects
                .push(measure_project(project_path, markers, deadline));
        }
        if report.is_partial {
            break;
        }
    }

    report
        .projects
        .sort_by_key(|project| std::cmp::Reverse(project.total_bytes));
    Ok(report)
}

type ProjectHit = (PathBuf, HashSet<&'static str>);

/// Walk a dev root looking for directories that directly contain marker
/// files. Returns `None` when the deadline expired (partial discovery).
fn find_projects(root: &Path, deadline: Instant) -> Option<Vec<ProjectHit>> {
    let mut markers_by_dir: HashMap<PathBuf, HashSet<&'static str>> = HashMap::new();
    for entry in WalkDir::new(root)
        .follow_links(false)
        .max_depth(PROJECT_MAX_DEPTH + 1)
        .into_iter()
        .filter_map(|value| value.ok())
    {
        if Instant::now() > deadline {
            return None;
        }
        if !entry.file_type().is_file() {
            continue;
        }
        let name = entry.file_name().to_string_lossy();
        if !is_marker_file(&name) {
            continue;
        }
        let Some(parent) = entry.path().parent() else {
            continue;
        };
        let marker = ARTIFACT_RULES
            .iter()
            .find(|(marker_name, _, _)| *marker_name == name.as_ref())
            .map(|(marker_name, _, _)| *marker_name);
        if let Some(marker) = marker {
            markers_by_dir
                .entry(parent.to_path_buf())
                .or_default()
                .insert(marker);
        }
    }
    let mut projects: Vec<ProjectHit> = markers_by_dir.into_iter().collect();
    projects.sort_by(|a, b| a.0.cmp(&b.0));
    Some(projects)
}

/// Accumulator for the artifact directory currently being walked into:
/// entries inside it contribute size/count but never new artifact hits.
struct ArtifactAcc {
    prefix: PathBuf,
    depth: usize,
    index: usize,
    size: u64,
    files: u64,
}

/// Measure one project: total size, latest modification time and matching
/// artifact directories (marker-gated, depth-bounded).
fn measure_project(
    project_path: PathBuf,
    markers: HashSet<&'static str>,
    deadline: Instant,
) -> PurgeProject {
    let mut total_bytes = 0u64;
    let mut last_active: Option<i64> = None;
    let mut artifacts: Vec<ArtifactGroup> = Vec::new();
    let mut active: Option<ArtifactAcc> = None;
    let mut has_git = false;

    for entry in WalkDir::new(&project_path)
        .follow_links(false)
        .into_iter()
        .filter_map(|value| value.ok())
    {
        if Instant::now() > deadline {
            break;
        }
        let path = entry.path();
        let Ok(meta) = entry.metadata() else {
            continue;
        };

        // A .git entry (directory or worktree file) at the project root
        // enables the git-tracked check for this project's artifacts.
        if entry.depth() == 1 && entry.file_name() == std::ffi::OsStr::new(".git") {
            has_git = true;
        }

        // Close the active artifact once the walk leaves its subtree.
        let should_close = matches!(&active, Some(acc)
            if entry.depth() <= acc.depth || !path.starts_with(&acc.prefix));
        if should_close {
            if let Some(acc) = active.take() {
                if let Some(artifact) = artifacts.get_mut(acc.index) {
                    artifact.size_bytes = acc.size;
                    artifact.file_count = acc.files;
                }
            }
        }

        if meta.is_file() {
            total_bytes += meta.len();
            // Activity is judged by *file* mtimes only, excluding the .git
            // subtree: git-internal churn (IDE auto-fetch, index refresh)
            // does not mean the user is working on the code, and directory
            // mtimes would mask genuinely stale projects after a fixture-
            // style bulk copy.
            let in_git_dir = path.starts_with(project_path.join(".git"));
            if !in_git_dir {
                if let Some(modified) = modified_secs(&meta) {
                    last_active =
                        Some(last_active.map_or(modified, |current| current.max(modified)));
                }
            }
        }

        // Inside the active artifact: accumulate only.
        let inside_active = matches!(&active, Some(acc) if path.starts_with(&acc.prefix));
        if inside_active {
            if let Some(acc) = &mut active {
                if meta.is_file() {
                    acc.size += meta.len();
                    acc.files += 1;
                }
            }
            continue;
        }

        if meta.is_dir() && entry.depth() >= 1 && entry.depth() <= ARTIFACT_MAX_DEPTH {
            let name = entry.file_name().to_string_lossy().to_string();
            if let Some(group) = artifact_group_for(&name, &markers) {
                artifacts.push(ArtifactGroup {
                    group: group.to_string(),
                    path: to_store_path(path),
                    size_bytes: 0,
                    file_count: 0,
                    git_tracked: false,
                });
                active = Some(ArtifactAcc {
                    prefix: path.to_path_buf(),
                    depth: entry.depth(),
                    index: artifacts.len() - 1,
                    size: 0,
                    files: 0,
                });
            }
        }
    }

    // Flush an artifact still open at the end of the walk.
    if let Some(acc) = active {
        if let Some(artifact) = artifacts.get_mut(acc.index) {
            artifact.size_bytes = acc.size;
            artifact.file_count = acc.files;
        }
    }

    // Git-tracked check runs once per artifact after the walk (tracked
    // artifacts are skipped; see ArtifactGroup::git_tracked).
    if has_git {
        for artifact in &mut artifacts {
            artifact.git_tracked = git_tracks(&project_path, &artifact.path, deadline);
        }
    }

    let cutoff = now_secs().saturating_sub(ACTIVE_WINDOW_DAYS * 86_400);
    let is_active = last_active.is_some_and(|modified| modified as u64 >= cutoff);
    PurgeProject {
        path: to_store_path(&project_path),
        name: project_path
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_default(),
        total_bytes,
        last_active_at: last_active,
        is_active,
        artifacts,
    }
}

// ── Execution ────────────────────────────────────────────────────────────────

type Deleter<'a> = dyn Fn(&Path) -> Result<(), String> + 'a;

/// Whether git reports files under the artifact as tracked. `artifact_path`
/// is the stored `/`-separated path; it is mapped back to a native path and
/// made relative to the project for `git ls-files`. Any failure (git missing,
/// broken repository, expired budget) counts as tracked — fail closed rather
/// than offer deletion inside a repository we cannot inspect.
fn git_tracks(project: &Path, artifact_path: &str, deadline: Instant) -> bool {
    if Instant::now() > deadline {
        return true;
    }
    let native = PathBuf::from(artifact_path.replace('/', std::path::MAIN_SEPARATOR_STR));
    let Ok(relative) = native.strip_prefix(project) else {
        return true;
    };
    let relative = relative
        .components()
        .map(|component| component.as_os_str().to_string_lossy().to_string())
        .collect::<Vec<_>>()
        .join("/");
    match Command::new("git")
        .arg("-C")
        .arg(project)
        .args(["ls-files", "-z", "--"])
        .arg(&relative)
        .output()
    {
        Ok(output) if output.status.success() => !output.stdout.is_empty(),
        _ => true,
    }
}

/// Trash the confirmed artifact directories. Re-discovers projects first and
/// only accepts paths that appear in the fresh discovery with an unchanged
/// size, belonging to a non-active project (deletion-boundary re-verification).
pub fn execute_purge(db_path: &Path, items: &[CleanRequestItem]) -> Result<CleanOutcome, AppError> {
    execute_purge_with_roots(db_path, &dev_roots(), items, &|native| {
        file_ops::trash_one(native)
    })
}

/// Root-injected execution core, shared with tests.
pub(crate) fn execute_purge_with_roots(
    db_path: &Path,
    roots: &[PathBuf],
    items: &[CleanRequestItem],
    deleter: &Deleter<'_>,
) -> Result<CleanOutcome, AppError> {
    // Fail closed on the whitelist before touching anything.
    let whitelist = load_whitelist(db_path)?;
    let report = discover_projects_in(roots.to_vec(), Instant::now() + DISCOVERY_BUDGET)?;

    let mut index: HashMap<&str, (&ArtifactGroup, &PurgeProject)> = HashMap::new();
    for project in &report.projects {
        for artifact in &project.artifacts {
            index.insert(artifact.path.as_str(), (artifact, project));
        }
    }

    let operation_id = begin_cleaner_operation(
        db_path,
        format!("项目瘦身清理（{} 项，移至废纸篓）", items.len()),
    )?;
    let case_insensitive = cfg!(windows);
    let mut succeeded = Vec::new();
    let mut failed = Vec::new();

    for item in items {
        let outcome = match index.get(item.path.as_str()) {
            Some((artifact, project)) => {
                verify_artifact(artifact, project, item, &whitelist, case_insensitive)
            }
            None => Err(AppError::new(
                "PATH_NOT_IN_SCOPE",
                format!("路径不在当前项目发现结果中，请重新扫描：{}", item.path),
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

/// Deletion-boundary checks for one artifact against the fresh discovery.
fn verify_artifact(
    artifact: &ArtifactGroup,
    project: &PurgeProject,
    item: &CleanRequestItem,
    whitelist: &[String],
    case_insensitive: bool,
) -> Result<PathBuf, AppError> {
    // Absolute, no parent-dir jumps, outside system-protected locations.
    let native = file_ops::validate_write_path(&item.path)?;
    if item.size_bytes != artifact.size_bytes {
        return Err(AppError::new(
            "FILE_CHANGED",
            format!("产物大小与重新扫描结果不一致，请刷新后重试：{}", item.path),
        ));
    }
    if project.is_active {
        return Err(AppError::new(
            "PROJECT_ACTIVE",
            format!("项目最近 7 天内有修改，暂不清理：{}", project.path),
        ));
    }
    if artifact.git_tracked {
        return Err(AppError::new(
            "GIT_TRACKED",
            format!("产物已被 git 跟踪，为避免影响仓库状态已跳过：{}", item.path),
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
        // Under target/ — outside the `/var` protected prefix on macOS.
        let base = std::env::current_dir()
            .expect("current dir")
            .join("target/luma-purge-tests");
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
        let dir = temp_root("db");
        let db = dir.join("test.db");
        initialize(&db).unwrap();
        db
    }

    fn remove_dir(dir: &Path) {
        fs::remove_dir_all(dir).ok();
    }

    // ── Registry ─────────────────────────────────────────────────────────────

    #[test]
    fn artifact_rules_gate_on_marker_files() {
        // node_modules requires package.json.
        let with_marker: HashSet<&str> = ["package.json"].into_iter().collect();
        assert_eq!(
            artifact_group_for("node_modules", &with_marker),
            Some("nodeModules")
        );
        assert_eq!(artifact_group_for("target", &with_marker), None);

        // The same directory name maps to different groups per marker.
        let cargo: HashSet<&str> = ["Cargo.toml"].into_iter().collect();
        assert_eq!(artifact_group_for("target", &cargo), Some("rustTarget"));
        let maven: HashSet<&str> = ["pom.xml"].into_iter().collect();
        assert_eq!(artifact_group_for("target", &maven), Some("mavenTarget"));

        // No markers at all — nothing matches.
        let empty: HashSet<&str> = HashSet::new();
        assert!(ARTIFACT_RULES
            .iter()
            .all(|(_, artifact, _)| artifact_group_for(artifact, &empty).is_none()));
    }

    #[test]
    fn marker_names_cover_all_rules() {
        for (marker, _, _) in ARTIFACT_RULES {
            assert!(ARTIFACT_RULES.iter().any(|(m, _, _)| m == marker));
            assert!(!marker.contains('/'), "markers are direct children only");
        }
    }

    // ── Discovery ────────────────────────────────────────────────────────────

    #[test]
    fn discovers_project_and_groups_artifacts() {
        let root = temp_root("discover");
        let project = root.join("app");
        write_file(&project.join("package.json"), 10);
        write_file(&project.join("node_modules/pkg/index.js"), 1_000);
        write_file(&project.join("node_modules/pkg/sub/util.js"), 500);
        write_file(&project.join("src/main.js"), 200);

        let report =
            discover_projects_in(vec![root.clone()], Instant::now() + DISCOVERY_BUDGET).unwrap();
        assert_eq!(report.scanned_roots, vec![to_store_path(&root)]);
        assert!(!report.is_partial);
        assert_eq!(report.projects.len(), 1);
        let project = &report.projects[0];
        assert_eq!(project.name, "app");
        assert_eq!(project.total_bytes, 1_710);
        assert_eq!(project.artifacts.len(), 1);
        let artifact = &project.artifacts[0];
        assert_eq!(artifact.group, "nodeModules");
        assert_eq!(artifact.size_bytes, 1_500);
        assert_eq!(artifact.file_count, 2);
        assert!(project.is_active, "fresh fixture counts as active");
        remove_dir(&root);
    }

    #[test]
    fn stray_named_directory_without_marker_is_ignored() {
        let root = temp_root("stray");
        // A folder literally named `target` with no Cargo.toml: not a project,
        // not an artifact.
        write_file(&root.join("notes/target/keep.txt"), 100);

        let report =
            discover_projects_in(vec![root.clone()], Instant::now() + DISCOVERY_BUDGET).unwrap();
        assert!(report.projects.is_empty());
        remove_dir(&root);
    }

    #[test]
    fn marker_gating_distinguishes_same_directory_name() {
        let root = temp_root("gating");
        // Rust project: target counts.
        let rust = root.join("rust-app");
        write_file(&rust.join("Cargo.toml"), 10);
        write_file(&rust.join("target/debug/app"), 800);
        // Plain directory with `target` but no marker: skipped entirely.
        write_file(&root.join("plain/target/blob"), 900);

        let report =
            discover_projects_in(vec![root.clone()], Instant::now() + DISCOVERY_BUDGET).unwrap();
        assert_eq!(report.projects.len(), 1);
        assert_eq!(report.projects[0].name, "rust-app");
        assert_eq!(report.projects[0].artifacts[0].group, "rustTarget");
        assert_eq!(report.projects[0].artifacts[0].size_bytes, 800);
        remove_dir(&root);
    }

    #[test]
    fn monorepo_subpackages_are_discovered_within_depth() {
        let root = temp_root("monorepo");
        // Root package (depth 1) + subpackage (depth 3, within the limit).
        write_file(&root.join("app/package.json"), 10);
        write_file(&root.join("app/packages/web/package.json"), 10);
        write_file(&root.join("app/packages/web/node_modules/lib.js"), 300);
        write_file(&root.join("app/node_modules/root-pkg.js"), 700);

        let report =
            discover_projects_in(vec![root.clone()], Instant::now() + DISCOVERY_BUDGET).unwrap();
        assert_eq!(report.projects.len(), 2, "root and subpackage both match");
        // Artifacts carry per-path sizes even when projects overlap.
        let artifact_sizes: Vec<u64> = report
            .projects
            .iter()
            .flat_map(|project| project.artifacts.iter().map(|artifact| artifact.size_bytes))
            .collect();
        assert!(artifact_sizes.contains(&700));
        assert!(artifact_sizes.contains(&300));
        remove_dir(&root);
    }

    #[test]
    fn projects_beyond_depth_limit_are_not_discovered() {
        let root = temp_root("deep");
        write_file(&root.join("a/b/c/d/e/package.json"), 10);
        write_file(&root.join("a/b/c/d/e/node_modules/x.js"), 100);

        let report =
            discover_projects_in(vec![root.clone()], Instant::now() + DISCOVERY_BUDGET).unwrap();
        assert!(report.projects.is_empty(), "depth 5 exceeds the limit");
        remove_dir(&root);
    }

    #[test]
    fn stale_projects_are_not_active() {
        let root = temp_root("stale");
        let project = root.join("old-app");
        write_file(&project.join("package.json"), 10);
        write_file(&project.join("node_modules/pkg.js"), 400);
        make_stale(&project);

        let report =
            discover_projects_in(vec![root.clone()], Instant::now() + DISCOVERY_BUDGET).unwrap();
        assert_eq!(report.projects.len(), 1);
        assert!(
            !report.projects[0].is_active,
            "400-day-old project is stale"
        );
        assert!(report.projects[0].last_active_at.is_some());
        remove_dir(&root);
    }

    #[test]
    fn budget_exhaustion_marks_partial() {
        let root = temp_root("partial");
        write_file(&root.join("app/package.json"), 10);
        write_file(&root.join("app/node_modules/x.js"), 100);

        let report = discover_projects_in(vec![root], Instant::now()).unwrap();
        assert!(report.is_partial, "an expired deadline must flag partial");
    }

    // ── Execution ────────────────────────────────────────────────────────────

    fn make_stale(path: &Path) {
        // Stamp every fixture *file* 400 days into the past (directories
        // cannot be opened for writing).
        for entry in WalkDir::new(path)
            .into_iter()
            .filter_map(|v| v.ok())
            .filter(|entry| entry.file_type().is_file())
        {
            // Git object files are read-only and cannot be re-stamped —
            // skip them (git-internal mtimes no longer affect activity).
            let Ok(file) = fs::File::options().write(true).open(entry.path()) else {
                continue;
            };
            file.set_times(
                fs::FileTimes::new()
                    .set_modified(std::time::SystemTime::now() - Duration::from_secs(400 * 86_400)),
            )
            .unwrap();
        }
    }

    fn fixture_project(tag: &str) -> (PathBuf, PathBuf, String, u64) {
        // Returns (dev_root, project_dir, artifact_store_path, artifact_size).
        let root = temp_root(tag);
        let project = root.join("old-app");
        write_file(&project.join("package.json"), 10);
        write_file(&project.join("node_modules/pkg/index.js"), 1_000);
        write_file(&project.join("src/main.js"), 200);
        make_stale(&project);

        let report =
            discover_projects_in(vec![root.clone()], Instant::now() + DISCOVERY_BUDGET).unwrap();
        let artifact = report.projects[0]
            .artifacts
            .iter()
            .find(|artifact| artifact.group == "nodeModules")
            .expect("nodeModules artifact")
            .clone();
        (root, project, artifact.path.clone(), artifact.size_bytes)
    }

    #[test]
    fn execute_trashes_verified_artifact_and_logs_operation() {
        let (root, project, artifact_path, artifact_size) = fixture_project("exec");
        let db = whitelist_db();

        let moved = project.join("node_modules.gone");
        let moved_for_closure = moved.clone();
        let outcome = execute_purge_with_roots(
            &db,
            std::slice::from_ref(&root),
            &[CleanRequestItem {
                path: artifact_path.clone(),
                size_bytes: artifact_size,
            }],
            &move |native: &Path| {
                std::fs::rename(native, &moved_for_closure).map_err(|error| error.to_string())
            },
        )
        .unwrap();

        assert_eq!(outcome.succeeded, vec![artifact_path]);
        assert!(outcome.failed.is_empty());
        assert!(!project.join("node_modules").exists());
        assert!(moved.exists());
        remove_dir(&root);
    }

    #[test]
    fn execute_rejects_stale_requests_and_active_projects() {
        // Fresh (active) project: even a perfectly matching request is refused.
        let root = temp_root("active");
        let project = root.join("hot-app");
        write_file(&project.join("package.json"), 10);
        write_file(&project.join("node_modules/x.js"), 500);
        let report =
            discover_projects_in(vec![root.clone()], Instant::now() + DISCOVERY_BUDGET).unwrap();
        let artifact = report.projects[0].artifacts[0].clone();
        let db = whitelist_db();

        let outcome = execute_purge_with_roots(
            &db,
            std::slice::from_ref(&root),
            &[CleanRequestItem {
                path: artifact.path.clone(),
                size_bytes: artifact.size_bytes,
            }],
            &|_| Ok(()),
        )
        .unwrap();
        assert!(outcome.succeeded.is_empty());
        assert!(outcome.failed[0].reason.contains("7 天"));

        // Size drift on a stale project: refused.
        let (root2, project2, artifact_path2, artifact_size2) = fixture_project("drift");
        write_file(&project2.join("node_modules/extra.js"), 123);
        let outcome = execute_purge_with_roots(
            &db,
            std::slice::from_ref(&root2),
            &[CleanRequestItem {
                path: artifact_path2,
                size_bytes: artifact_size2,
            }],
            &|_| Ok(()),
        )
        .unwrap();
        assert!(outcome.succeeded.is_empty());
        assert!(outcome.failed[0].reason.contains("不一致"));

        // Path that no longer appears in discovery: refused.
        let outcome = execute_purge_with_roots(
            &db,
            std::slice::from_ref(&root2),
            &[CleanRequestItem {
                path: "/definitely/not/discovered/node_modules".to_string(),
                size_bytes: 1,
            }],
            &|_| Ok(()),
        )
        .unwrap();
        assert!(outcome.succeeded.is_empty());
        assert!(outcome.failed[0].reason.contains("重新扫描"));
        remove_dir(&root);
        remove_dir(&root2);
    }

    #[test]
    fn execute_refuses_whitelisted_artifacts() {
        let (root, _project, artifact_path, artifact_size) = fixture_project("whitelist");
        let db = whitelist_db();
        crate::cleaner::add_whitelist_entry(&db, &artifact_path).unwrap();

        let outcome = execute_purge_with_roots(
            &db,
            &[root],
            &[CleanRequestItem {
                path: artifact_path,
                size_bytes: artifact_size,
            }],
            &|_| Ok(()),
        )
        .unwrap();
        assert!(outcome.succeeded.is_empty());
        assert!(outcome.failed[0].reason.contains("白名单"));
    }

    #[test]
    fn execute_fails_closed_when_whitelist_unreadable() {
        let (root, _project, artifact_path, artifact_size) = fixture_project("fail-closed");
        let db_dir = temp_root("fail-closed-db");
        let db = db_dir.join("broken.db");
        initialize(&db).unwrap();
        {
            let connection = crate::database::open(&db).unwrap();
            connection
                .execute("DROP TABLE cleaner_whitelist", [])
                .unwrap();
        }
        let error = execute_purge_with_roots(
            &db,
            std::slice::from_ref(&root),
            &[CleanRequestItem {
                path: artifact_path,
                size_bytes: artifact_size,
            }],
            &|_| Ok(()),
        )
        .expect_err("unreadable whitelist must refuse execution");
        assert_eq!(error.code, "DATABASE_ERROR");
        remove_dir(&root);
        fs::remove_dir_all(db_dir).ok();
    }

    #[test]
    fn discover_fails_closed_when_whitelist_unreadable() {
        let db_dir = temp_root("discover-db");
        let db = db_dir.join("broken.db");
        initialize(&db).unwrap();
        {
            let connection = crate::database::open(&db).unwrap();
            connection
                .execute("DROP TABLE cleaner_whitelist", [])
                .unwrap();
        }
        let error = discover_projects(&db).expect_err("must fail closed");
        assert_eq!(error.code, "DATABASE_ERROR");
        fs::remove_dir_all(db_dir).ok();
    }

    // ── Git-tracked detection (CLN-102) ─────────────────────────────────────

    fn git_available() -> bool {
        Command::new("git")
            .arg("--version")
            .output()
            .map(|output| output.status.success())
            .unwrap_or(false)
    }

    fn run_git(dir: &Path, args: &[&str]) {
        let status = Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(["-c", "user.email=luma@test", "-c", "user.name=luma"])
            .args(args)
            .status()
            .expect("run git");
        assert!(status.success(), "git {args:?} failed");
    }

    #[test]
    fn git_tracked_artifacts_are_flagged_and_refused() {
        if !git_available() {
            return;
        }
        let root = temp_root("git");
        let project = root.join("repo");
        write_file(&project.join("package.json"), 10);
        write_file(&project.join("dist/bundle.js"), 800); // committed
        write_file(&project.join("node_modules/pkg.js"), 400); // untracked
        run_git(&project, &["init"]);
        run_git(&project, &["add", "dist"]);
        run_git(&project, &["commit", "-m", "init"]);
        make_stale(&project);

        let report =
            discover_projects_in(vec![root.clone()], Instant::now() + DISCOVERY_BUDGET).unwrap();
        assert_eq!(report.projects.len(), 1);
        let dist = report.projects[0]
            .artifacts
            .iter()
            .find(|artifact| artifact.group == "distBuild")
            .expect("dist artifact");
        let node = report.projects[0]
            .artifacts
            .iter()
            .find(|artifact| artifact.group == "nodeModules")
            .expect("node_modules artifact");
        assert!(dist.git_tracked, "committed dist is tracked");
        assert!(!node.git_tracked, "node_modules is not tracked");

        // Tracked artifact is refused at the execution boundary.
        let db = whitelist_db();
        let outcome = execute_purge_with_roots(
            &db,
            std::slice::from_ref(&root),
            &[CleanRequestItem {
                path: dist.path.clone(),
                size_bytes: dist.size_bytes,
            }],
            &|_| Ok(()),
        )
        .unwrap();
        assert!(outcome.succeeded.is_empty());
        assert!(outcome.failed[0].reason.contains("git"));

        // Untracked artifact passes and is deleted by the injected deleter.
        let outcome = execute_purge_with_roots(
            &db,
            std::slice::from_ref(&root),
            &[CleanRequestItem {
                path: node.path.clone(),
                size_bytes: node.size_bytes,
            }],
            &|native: &Path| std::fs::remove_dir_all(native).map_err(|error| error.to_string()),
        )
        .unwrap();
        assert_eq!(outcome.succeeded.len(), 1);
        remove_dir(&root);
    }
}
