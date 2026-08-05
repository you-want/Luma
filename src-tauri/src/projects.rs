use crate::error::AppError;
use rusqlite::{params, Connection};
use std::path::Path;

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectCandidate {
    pub path: String,
    pub name: String,
    pub kind: ProjectKind,
    pub size_bytes: u64,
    pub file_count: usize,
}

#[derive(Debug, serde::Serialize)]
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

/// 从数据库中识别开发项目目录
///
/// 识别规则：
/// - Node.js: 包含 package.json 且有 node_modules
/// - Rust: 包含 Cargo.toml 且有 target 目录
/// - Python: 包含 requirements.txt 或 pyproject.toml 且有 venv/__pycache__
/// - Git: 包含 .git 目录
/// - Xcode: 包含 .xcodeproj 或 .xcworkspace
/// - Maven: 包含 pom.xml 且有 target 目录
/// - Gradle: 包含 build.gradle 且有 build 目录
pub fn identify_projects(
    database_path: &Path,
    scan_id: &str,
) -> Result<Vec<ProjectCandidate>, AppError> {
    let connection = Connection::open(database_path)?;
    let mut projects = Vec::new();

    // Node.js 项目
    projects.extend(find_nodejs_projects(&connection, scan_id)?);

    // Rust 项目
    projects.extend(find_rust_projects(&connection, scan_id)?);

    // Python 项目
    projects.extend(find_python_projects(&connection, scan_id)?);

    // Git 仓库
    projects.extend(find_git_projects(&connection, scan_id)?);

    // Xcode 项目
    projects.extend(find_xcode_projects(&connection, scan_id)?);

    // Maven 项目
    projects.extend(find_maven_projects(&connection, scan_id)?);

    // Gradle 项目
    projects.extend(find_gradle_projects(&connection, scan_id)?);

    // 按大小降序排列
    projects.sort_by(|a, b| b.size_bytes.cmp(&a.size_bytes));

    Ok(projects)
}

fn find_nodejs_projects(
    connection: &Connection,
    scan_id: &str,
) -> Result<Vec<ProjectCandidate>, AppError> {
    let mut statement = connection.prepare(
        "SELECT DISTINCT
            substr(path, 1, length(path) - length(name) - 1) as project_path
         FROM files
         WHERE scan_id = ?1
           AND name = 'package.json'
           AND EXISTS (
             SELECT 1 FROM files f2
             WHERE f2.scan_id = ?1
               AND f2.path LIKE project_path || '/node_modules/%'
           )",
    )?;

    let rows = statement.query_map([scan_id], |row| row.get::<_, String>(0))?;
    let mut projects = Vec::new();

    for project_path in rows.flatten() {
        if let Some(candidate) =
            build_project_candidate(connection, scan_id, &project_path, ProjectKind::NodeJs)?
        {
            projects.push(candidate);
        }
    }

    Ok(projects)
}

fn find_rust_projects(
    connection: &Connection,
    scan_id: &str,
) -> Result<Vec<ProjectCandidate>, AppError> {
    let mut statement = connection.prepare(
        "SELECT DISTINCT
            substr(path, 1, length(path) - length(name) - 1) as project_path
         FROM files
         WHERE scan_id = ?1
           AND name = 'Cargo.toml'
           AND EXISTS (
             SELECT 1 FROM files f2
             WHERE f2.scan_id = ?1
               AND f2.path LIKE project_path || '/target/%'
           )",
    )?;

    let rows = statement.query_map([scan_id], |row| row.get::<_, String>(0))?;
    let mut projects = Vec::new();

    for project_path in rows.flatten() {
        if let Some(candidate) =
            build_project_candidate(connection, scan_id, &project_path, ProjectKind::Rust)?
        {
            projects.push(candidate);
        }
    }

    Ok(projects)
}

fn find_python_projects(
    connection: &Connection,
    scan_id: &str,
) -> Result<Vec<ProjectCandidate>, AppError> {
    let mut statement = connection.prepare(
        "SELECT DISTINCT
            substr(path, 1, length(path) - length(name) - 1) as project_path
         FROM files
         WHERE scan_id = ?1
           AND (name = 'requirements.txt' OR name = 'pyproject.toml')
           AND EXISTS (
             SELECT 1 FROM files f2
             WHERE f2.scan_id = ?1
               AND (f2.path LIKE project_path || '/venv/%'
                    OR f2.path LIKE project_path || '/__pycache__/%'
                    OR f2.path LIKE project_path || '/.venv/%')
           )",
    )?;

    let rows = statement.query_map([scan_id], |row| row.get::<_, String>(0))?;
    let mut projects = Vec::new();

    for project_path in rows.flatten() {
        if let Some(candidate) =
            build_project_candidate(connection, scan_id, &project_path, ProjectKind::Python)?
        {
            projects.push(candidate);
        }
    }

    Ok(projects)
}

fn find_git_projects(
    connection: &Connection,
    scan_id: &str,
) -> Result<Vec<ProjectCandidate>, AppError> {
    let mut statement = connection.prepare(
        "SELECT DISTINCT
            substr(path, 1, length(path) - length(name) - 1) as project_path
         FROM files
         WHERE scan_id = ?1
           AND path LIKE '%/.git/%'
           AND path NOT LIKE '%/.git/%/%'",
    )?;

    let rows = statement.query_map([scan_id], |row| {
        let git_path: String = row.get(0)?;
        // 从 /some/path/.git 提取到 /some/path
        Ok(git_path.trim_end_matches("/.git").to_owned())
    })?;
    let mut projects = Vec::new();

    for project_path in rows.flatten() {
        if let Some(candidate) =
            build_project_candidate(connection, scan_id, &project_path, ProjectKind::Git)?
        {
            projects.push(candidate);
        }
    }

    Ok(projects)
}

fn find_xcode_projects(
    connection: &Connection,
    scan_id: &str,
) -> Result<Vec<ProjectCandidate>, AppError> {
    let mut statement = connection.prepare(
        "SELECT DISTINCT
            substr(path, 1, length(path) - length(name) - 1) as project_path
         FROM files
         WHERE scan_id = ?1
           AND (name LIKE '%.xcodeproj' OR name LIKE '%.xcworkspace')",
    )?;

    let rows = statement.query_map([scan_id], |row| row.get::<_, String>(0))?;
    let mut projects = Vec::new();

    for project_path in rows.flatten() {
        if let Some(candidate) =
            build_project_candidate(connection, scan_id, &project_path, ProjectKind::Xcode)?
        {
            projects.push(candidate);
        }
    }

    Ok(projects)
}

fn find_maven_projects(
    connection: &Connection,
    scan_id: &str,
) -> Result<Vec<ProjectCandidate>, AppError> {
    let mut statement = connection.prepare(
        "SELECT DISTINCT
            substr(path, 1, length(path) - length(name) - 1) as project_path
         FROM files
         WHERE scan_id = ?1
           AND name = 'pom.xml'
           AND EXISTS (
             SELECT 1 FROM files f2
             WHERE f2.scan_id = ?1
               AND f2.path LIKE project_path || '/target/%'
           )",
    )?;

    let rows = statement.query_map([scan_id], |row| row.get::<_, String>(0))?;
    let mut projects = Vec::new();

    for project_path in rows.flatten() {
        if let Some(candidate) =
            build_project_candidate(connection, scan_id, &project_path, ProjectKind::Maven)?
        {
            projects.push(candidate);
        }
    }

    Ok(projects)
}

fn find_gradle_projects(
    connection: &Connection,
    scan_id: &str,
) -> Result<Vec<ProjectCandidate>, AppError> {
    let mut statement = connection.prepare(
        "SELECT DISTINCT
            substr(path, 1, length(path) - length(name) - 1) as project_path
         FROM files
         WHERE scan_id = ?1
           AND (name = 'build.gradle' OR name = 'build.gradle.kts')
           AND EXISTS (
             SELECT 1 FROM files f2
             WHERE f2.scan_id = ?1
               AND f2.path LIKE project_path || '/build/%'
           )",
    )?;

    let rows = statement.query_map([scan_id], |row| row.get::<_, String>(0))?;
    let mut projects = Vec::new();

    for project_path in rows.flatten() {
        if let Some(candidate) =
            build_project_candidate(connection, scan_id, &project_path, ProjectKind::Gradle)?
        {
            projects.push(candidate);
        }
    }

    Ok(projects)
}

fn build_project_candidate(
    connection: &Connection,
    scan_id: &str,
    project_path: &str,
    kind: ProjectKind,
) -> Result<Option<ProjectCandidate>, AppError> {
    let mut statement = connection.prepare(
        "SELECT COUNT(*), COALESCE(SUM(size_bytes), 0)
         FROM files
         WHERE scan_id = ?1 AND path LIKE ?2",
    )?;

    let pattern = format!("{}/%", project_path);
    let (file_count, size_bytes): (usize, i64) = statement
        .query_row(params![scan_id, pattern], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })?;

    if file_count == 0 {
        return Ok(None);
    }

    let name = project_path
        .rsplit('/')
        .next()
        .unwrap_or(project_path)
        .to_owned();

    Ok(Some(ProjectCandidate {
        path: project_path.to_owned(),
        name,
        kind,
        size_bytes: size_bytes.max(0) as u64,
        file_count,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::{create_scan_run, initialize, insert_file_batch, open};
    use crate::models::FileEntry;
    use std::fs;
    use uuid::Uuid;

    #[test]
    fn serializes_project_candidate_with_camel_case_fields() {
        let project = ProjectCandidate {
            path: "/test/my-app".to_owned(),
            name: "my-app".to_owned(),
            kind: ProjectKind::NodeJs,
            size_bytes: 51_024,
            file_count: 2,
        };

        let value = serde_json::to_value(project).expect("serialize project candidate");

        assert_eq!(value["sizeBytes"], 51_024);
        assert_eq!(value["fileCount"], 2);
        assert!(value.get("size_bytes").is_none());
        assert!(value.get("file_count").is_none());
    }

    #[test]
    fn identifies_nodejs_project() {
        let database_path =
            std::env::temp_dir().join(format!("luma-projects-{}.sqlite3", Uuid::new_v4()));
        initialize(&database_path).unwrap();
        let scan_id = "scan-1";
        create_scan_run(&database_path, scan_id, "/test", 0).unwrap();

        let files = vec![
            FileEntry {
                path: "/test/my-app/package.json".to_owned(),
                name: "package.json".to_owned(),
                extension: Some("json".to_owned()),
                category: "code".to_owned(),
                size_bytes: 1024,
                modified_at: Some(100),
                is_hidden: false,
                content_hash: None,
            },
            FileEntry {
                path: "/test/my-app/node_modules/lib/index.js".to_owned(),
                name: "index.js".to_owned(),
                extension: Some("js".to_owned()),
                category: "code".to_owned(),
                size_bytes: 50000,
                modified_at: Some(100),
                is_hidden: false,
                content_hash: None,
            },
        ];

        let mut connection = open(&database_path).unwrap();
        insert_file_batch(&mut connection, scan_id, &files).unwrap();

        let projects = identify_projects(&database_path, scan_id).unwrap();
        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0].name, "my-app");
        assert!(matches!(projects[0].kind, ProjectKind::NodeJs));
        assert_eq!(projects[0].file_count, 2);

        fs::remove_file(database_path).unwrap();
    }

    #[test]
    fn identifies_rust_project() {
        let database_path =
            std::env::temp_dir().join(format!("luma-projects-{}.sqlite3", Uuid::new_v4()));
        initialize(&database_path).unwrap();
        let scan_id = "scan-1";
        create_scan_run(&database_path, scan_id, "/test", 0).unwrap();

        let files = vec![
            FileEntry {
                path: "/test/my-crate/Cargo.toml".to_owned(),
                name: "Cargo.toml".to_owned(),
                extension: Some("toml".to_owned()),
                category: "code".to_owned(),
                size_bytes: 512,
                modified_at: Some(100),
                is_hidden: false,
                content_hash: None,
            },
            FileEntry {
                path: "/test/my-crate/target/debug/my-crate".to_owned(),
                name: "my-crate".to_owned(),
                extension: None,
                category: "applications".to_owned(),
                size_bytes: 2_000_000,
                modified_at: Some(100),
                is_hidden: false,
                content_hash: None,
            },
        ];

        let mut connection = open(&database_path).unwrap();
        insert_file_batch(&mut connection, scan_id, &files).unwrap();

        let projects = identify_projects(&database_path, scan_id).unwrap();
        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0].name, "my-crate");
        assert!(matches!(projects[0].kind, ProjectKind::Rust));

        fs::remove_file(database_path).unwrap();
    }
}
