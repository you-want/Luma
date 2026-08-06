use crate::{database, error::AppError};
use std::collections::HashSet;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ProjectKind {
    NodeJs,
    Rust,
    Python,
    Git,
    Xcode,
    Maven,
    Gradle,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectCandidate {
    pub path: String,
    pub name: String,
    pub kind: ProjectKind,
    pub file_count: usize,
    pub size_bytes: u64,
}

/// 识别开发项目目录。
///
/// 性能策略（方案 A：纯内存计算）：只做**一次** `SELECT path, size_bytes` 把整个
/// 扫描的文件表载入内存，然后在 Rust 侧完成全部分析——单遍扫描收集"标记文件"
/// （如 package.json）与"构建产物目录"（如 node_modules），再用排序 + 二分查找
/// 统计每个项目子树的文件数与大小。
///
/// 这取代了早期实现的 O(项目数 × 全表) 关联子查询：在 33 万文件、数千个
/// package.json 的真实库上，旧实现要分钟级且会冻死 UI；本实现是 O(N log N)，
/// 亚秒级完成。见 `tests::handles_large_scan_without_quadratic_blowup`。
pub fn identify_projects(
    database_path: &Path,
    scan_id: &str,
) -> Result<Vec<ProjectCandidate>, AppError> {
    let connection = database::open(database_path)?;

    // 1. 一次载入全部 (path, size)。这是唯一一次数据库查询。
    let mut rows: Vec<(String, u64)> = {
        let mut stmt =
            connection.prepare("SELECT path, size_bytes FROM files WHERE scan_id = ?1")?;
        let collected = stmt
            .query_map([scan_id], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?.max(0) as u64,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        collected
    };

    // 2. 单遍扫描：收集每类项目的"标记文件所在目录"和"含产物目录的根"。
    //    标记 = 项目声明文件（package.json 等）的父目录。
    //    产物 = 路径中出现 node_modules/target 等目录时，其前缀根目录。
    let mut markers = Markers::default();
    let mut artifacts = Artifacts::default();

    for (path, _) in &rows {
        let basename = path.rsplit('/').next().unwrap_or(path.as_str());

        // 标记文件（同一目录下多个标记只记一次由后续 dedup 兜底）。
        match basename {
            "package.json" => push_parent(path, &mut markers.nodejs),
            "Cargo.toml" => push_parent(path, &mut markers.rust),
            "setup.py" | "pyproject.toml" | "requirements.txt" => {
                push_parent(path, &mut markers.python)
            }
            "pom.xml" => push_parent(path, &mut markers.maven),
            "build.gradle" | "build.gradle.kts" | "settings.gradle" | "settings.gradle.kts" => {
                push_parent(path, &mut markers.gradle)
            }
            _ => {}
        }

        // 产物目录：路径中包含 `/<artifact>/` 时，取其之前的部分为项目根。
        if let Some(root) = artifact_root(path, "node_modules") {
            artifacts.node_modules.insert(root);
        }
        if let Some(root) = artifact_root(path, "target") {
            artifacts.target.insert(root);
        }
        for dir in ["venv", ".venv", "__pycache__"] {
            if let Some(root) = artifact_root(path, dir) {
                artifacts.python.insert(root);
            }
        }
        for dir in ["build", ".gradle"] {
            if let Some(root) = artifact_root(path, dir) {
                artifacts.gradle.insert(root);
            }
        }
        if let Some(root) = artifact_root(path, ".git") {
            artifacts.git.insert(root);
        }
        if let Some(root) = xcode_root(path) {
            artifacts.xcode.insert(root);
        }
    }

    // 3. 匹配：标记目录必须同时拥有对应产物才算项目。Git/Xcode 目录本身即项目。
    //    过滤掉位于 node_modules 内部的伪项目（依赖包自带的 package.json）。
    let mut candidates: Vec<(String, ProjectKind)> = Vec::new();
    collect_matched(
        &markers.nodejs,
        &artifacts.node_modules,
        ProjectKind::NodeJs,
        &mut candidates,
    );
    collect_matched(
        &markers.rust,
        &artifacts.target,
        ProjectKind::Rust,
        &mut candidates,
    );
    collect_matched(
        &markers.python,
        &artifacts.python,
        ProjectKind::Python,
        &mut candidates,
    );
    collect_matched(
        &markers.maven,
        &artifacts.target,
        ProjectKind::Maven,
        &mut candidates,
    );
    collect_matched(
        &markers.gradle,
        &artifacts.gradle,
        ProjectKind::Gradle,
        &mut candidates,
    );
    for root in &artifacts.git {
        candidates.push((root.clone(), ProjectKind::Git));
    }
    for root in &artifacts.xcode {
        candidates.push((root.clone(), ProjectKind::Xcode));
    }

    // 去重：同一路径只保留一个项目。先按 (路径, 类型) 排序——ProjectKind 的
    // 枚举顺序即优先级（NodeJs/Rust/… 在 Git 之前），故同一目录既是 Git 仓库又是
    // Node 项目时，保留信息量更大的语言类型，而非笼统的 Git。
    candidates.sort();
    candidates.dedup_by(|a, b| a.0 == b.0);

    // 4. 统计每个项目子树的文件数与大小。排序后同前缀路径是连续区间，用二分定位。
    rows.sort_by(|a, b| a.0.cmp(&b.0));
    let mut projects = Vec::with_capacity(candidates.len());
    for (root, kind) in candidates {
        let (file_count, size_bytes) = subtree_totals(&rows, &root);
        if file_count == 0 {
            continue;
        }
        let name = root.rsplit('/').next().unwrap_or(root.as_str()).to_owned();
        projects.push(ProjectCandidate {
            path: root,
            name,
            kind,
            file_count,
            size_bytes,
        });
    }

    // 大项目优先。
    projects.sort_by(|a, b| b.size_bytes.cmp(&a.size_bytes));
    Ok(projects)
}

#[derive(Default)]
struct Markers {
    nodejs: Vec<String>,
    rust: Vec<String>,
    python: Vec<String>,
    maven: Vec<String>,
    gradle: Vec<String>,
}

#[derive(Default)]
struct Artifacts {
    node_modules: HashSet<String>,
    target: HashSet<String>,
    python: HashSet<String>,
    gradle: HashSet<String>,
    git: HashSet<String>,
    xcode: HashSet<String>,
}

/// 把标记文件的父目录记为候选项目根。跳过位于依赖目录内部的标记
/// （如 node_modules 里每个包自带的 package.json），避免伪项目噪声。
fn push_parent(path: &str, out: &mut Vec<String>) {
    if let Some((parent, _)) = path.rsplit_once('/') {
        if parent.is_empty() || parent.contains("/node_modules/") || parent.contains("/.git/") {
            return;
        }
        out.push(parent.to_owned());
    }
}

/// 若路径中包含 `/<artifact>/`，返回它之前的部分作为项目根。
fn artifact_root(path: &str, artifact: &str) -> Option<String> {
    let needle = format!("/{artifact}/");
    let index = path.find(&needle)?;
    if index == 0 {
        return None;
    }
    Some(path[..index].to_owned())
}

/// Xcode 项目：路径含 `.xcodeproj`/`.xcworkspace` 时，取该目录的父目录为根。
fn xcode_root(path: &str) -> Option<String> {
    let index = path
        .find(".xcodeproj")
        .or_else(|| path.find(".xcworkspace"))?;
    // path[..index] 形如 "/a/b/Foo"；父目录 "/a/b" 即项目根。
    let up_to = &path[..index];
    up_to.rsplit_once('/').and_then(|(parent, _)| {
        if parent.is_empty() {
            None
        } else {
            Some(parent.to_owned())
        }
    })
}

/// 标记目录集合中，凡产物集合里存在同一根的，即为已识别项目。
fn collect_matched(
    marker_roots: &[String],
    artifact_roots: &HashSet<String>,
    kind: ProjectKind,
    out: &mut Vec<(String, ProjectKind)>,
) {
    for root in marker_roots {
        if artifact_roots.contains(root) {
            out.push((root.clone(), kind));
        }
    }
}

/// 统计 `root/` 前缀下所有文件的数量与总大小。`rows` 必须已按路径升序排序：
/// 同前缀路径是连续区间，用两次 `partition_point` 二分定位区间端点。
///
/// 上界用 `root` 拼上 `'0'`（`'/'` 之后的一个字符）：任何以 `root/` 开头的路径
/// 都满足 `root/ <= p < root0`，因为 `'/'`(0x2F) < `'0'`(0x30)。这样 `/a/app`
/// 不会误配 `/a/application/...`。
fn subtree_totals(rows: &[(String, u64)], root: &str) -> (usize, u64) {
    let prefix = format!("{root}/");
    let prefix_end = format!("{root}0");
    let start = rows.partition_point(|(p, _)| p.as_str() < prefix.as_str());
    let end = rows.partition_point(|(p, _)| p.as_str() < prefix_end.as_str());
    let slice = &rows[start..end];
    let size_bytes = slice.iter().map(|(_, size)| *size).sum();
    (slice.len(), size_bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::{create_scan_run, initialize, insert_file_batch, open};
    use crate::models::FileEntry;
    use std::fs;
    use std::time::Instant;
    use uuid::Uuid;

    fn file(path: &str, name: &str, size: u64) -> FileEntry {
        FileEntry {
            id: 0,
            path: path.to_owned(),
            name: name.to_owned(),
            extension: None,
            category: "code".to_owned(),
            size_bytes: size,
            modified_at: Some(1),
            is_hidden: false,
            content_hash: None,
        }
    }

    fn setup(files: &[FileEntry]) -> (std::path::PathBuf, String) {
        let db = std::env::temp_dir().join(format!("luma-proj-{}.sqlite3", Uuid::new_v4()));
        initialize(&db).expect("initialize");
        create_scan_run(&db, "scan-1", "/test", 0).expect("create scan run");
        let mut connection = open(&db).expect("open");
        insert_file_batch(&mut connection, "scan-1", files).expect("insert files");
        (db, "scan-1".to_owned())
    }

    fn cleanup(db: &std::path::Path) {
        fs::remove_file(db).ok();
        fs::remove_file(db.with_extension("sqlite3-shm")).ok();
        fs::remove_file(db.with_extension("sqlite3-wal")).ok();
    }

    #[test]
    fn identifies_nodejs_project_with_node_modules() {
        let (db, scan_id) = setup(&[
            file("/test/app/package.json", "package.json", 100),
            file("/test/app/node_modules/lib/index.js", "index.js", 5000),
        ]);

        let projects = identify_projects(&db, &scan_id).expect("identify");
        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0].name, "app");
        assert_eq!(projects[0].path, "/test/app");
        assert!(matches!(projects[0].kind, ProjectKind::NodeJs));
        assert_eq!(projects[0].file_count, 2);
        assert_eq!(projects[0].size_bytes, 5100);

        cleanup(&db);
    }

    #[test]
    fn ignores_package_json_without_node_modules() {
        let (db, scan_id) = setup(&[
            file("/test/app/package.json", "package.json", 100),
            file("/test/app/src/index.js", "index.js", 5000),
        ]);

        let projects = identify_projects(&db, &scan_id).expect("identify");
        assert!(projects.is_empty());

        cleanup(&db);
    }

    #[test]
    fn identifies_rust_project() {
        let (db, scan_id) = setup(&[
            file("/test/crate/Cargo.toml", "Cargo.toml", 50),
            file("/test/crate/target/debug/app", "app", 2_000_000),
        ]);

        let projects = identify_projects(&db, &scan_id).expect("identify");
        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0].name, "crate");
        assert!(matches!(projects[0].kind, ProjectKind::Rust));

        cleanup(&db);
    }

    #[test]
    fn does_not_report_nested_dependency_package_json_as_project() {
        // A dependency inside node_modules has its own package.json; it must not
        // surface as a separate project even if it nests another node_modules.
        let (db, scan_id) = setup(&[
            file("/test/app/package.json", "package.json", 100),
            file(
                "/test/app/node_modules/dep/package.json",
                "package.json",
                40,
            ),
            file(
                "/test/app/node_modules/dep/node_modules/sub/index.js",
                "index.js",
                10,
            ),
        ]);

        let projects = identify_projects(&db, &scan_id).expect("identify");
        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0].path, "/test/app");

        cleanup(&db);
    }

    #[test]
    fn sibling_prefix_does_not_leak_into_subtree_totals() {
        // `/test/app` must not absorb files under the sibling `/test/application`.
        let (db, scan_id) = setup(&[
            file("/test/app/package.json", "package.json", 100),
            file("/test/app/node_modules/lib/index.js", "index.js", 200),
            file("/test/application/huge.bin", "huge.bin", 9_000_000),
        ]);

        let projects = identify_projects(&db, &scan_id).expect("identify");
        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0].path, "/test/app");
        assert_eq!(projects[0].file_count, 2);
        assert_eq!(projects[0].size_bytes, 300);

        cleanup(&db);
    }

    #[test]
    fn handles_large_scan_without_quadratic_blowup() {
        // Regression guard for the O(projects × full-table) freeze: many markers
        // over many files must finish fast. The old correlated-subquery version
        // took minutes here; the in-memory version is well under a second.
        let mut files = Vec::new();
        for p in 0..80 {
            files.push(file(
                &format!("/repo/proj{p}/package.json"),
                "package.json",
                10,
            ));
            for n in 0..300 {
                files.push(file(
                    &format!("/repo/proj{p}/node_modules/dep/file{n}.js"),
                    &format!("file{n}.js"),
                    10,
                ));
            }
        }
        let total_files = files.len();
        let (db, scan_id) = setup(&files);

        let started = Instant::now();
        let projects = identify_projects(&db, &scan_id).expect("identify");
        let elapsed = started.elapsed();

        assert_eq!(projects.len(), 80);
        assert!(
            elapsed.as_secs() < 5,
            "identify_projects on {total_files} files took {elapsed:?}, expected < 5s"
        );

        cleanup(&db);
    }
}
