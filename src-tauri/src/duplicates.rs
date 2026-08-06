use crate::{error::AppError, models::FileEntry};
use rusqlite::{params, Connection};
use std::{
    collections::HashMap,
    fs::File,
    io::{BufReader, Read},
    path::Path,
};

/// 查找重复文件候选：先按大小分组，再对同组文件计算内容哈希
pub fn find_duplicate_candidates(
    database_path: &Path,
    scan_id: &str,
    min_size: u64,
) -> Result<Vec<DuplicateGroup>, AppError> {
    let connection = Connection::open(database_path)?;

    // 第一步：按大小分组，只关注至少有 2 个文件且大小 >= min_size 的组
    let mut statement = connection.prepare(
        "SELECT size_bytes, COUNT(*) as count
         FROM files
         WHERE scan_id = ?1 AND size_bytes >= ?2
         GROUP BY size_bytes
         HAVING count >= 2
         ORDER BY size_bytes DESC",
    )?;

    let size_groups = statement
        .query_map(params![scan_id, to_i64(min_size)], |row| {
            Ok((from_i64(row.get(0)?), from_i64(row.get::<_, i64>(1)?)))
        })?
        .collect::<Result<Vec<(u64, u64)>, _>>()?;

    let mut duplicate_groups = Vec::new();

    // 第二步：对每个大小组，获取文件列表并计算哈希
    for (size_bytes, _count) in size_groups {
        let mut size_statement = connection.prepare(
            "SELECT id, path, name, extension, category, size_bytes, modified_at, is_hidden, content_hash
             FROM files
             WHERE scan_id = ?1 AND size_bytes = ?2
             ORDER BY path",
        )?;

        let files = size_statement
            .query_map(params![scan_id, to_i64(size_bytes)], |row| {
                Ok(FileEntry {
                    id: row.get(0)?,
                    path: row.get(1)?,
                    name: row.get(2)?,
                    extension: row.get(3)?,
                    category: row.get(4)?,
                    size_bytes: from_i64(row.get(5)?),
                    modified_at: row.get(6)?,
                    is_hidden: row.get::<_, i64>(7)? != 0,
                    content_hash: row.get(8)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        // 按内容哈希分组
        let hash_groups = group_by_content_hash(&files)?;

        // 只保留真正重复的组（至少 2 个文件有相同哈希）
        for (hash, group_files) in hash_groups {
            if group_files.len() >= 2 {
                let wasted_bytes = size_bytes * (group_files.len() as u64 - 1);
                duplicate_groups.push(DuplicateGroup {
                    content_hash: hash,
                    size_bytes,
                    file_count: group_files.len() as u64,
                    wasted_bytes,
                    files: group_files,
                });
            }
        }
    }

    // 按浪费空间降序排列
    duplicate_groups.sort_by(|a, b| b.wasted_bytes.cmp(&a.wasted_bytes));

    Ok(duplicate_groups)
}

/// 按内容哈希分组文件
fn group_by_content_hash(files: &[FileEntry]) -> Result<HashMap<String, Vec<FileEntry>>, AppError> {
    let mut groups: HashMap<String, Vec<FileEntry>> = HashMap::new();

    for file in files {
        let hash = match &file.content_hash {
            Some(h) => h.clone(),
            None => {
                // 如果数据库中没有哈希，现场计算
                compute_file_hash(&file.path)?
            }
        };

        groups.entry(hash).or_default().push(file.clone());
    }

    Ok(groups)
}

/// 计算文件的 BLAKE3 哈希（快速且安全）
fn compute_file_hash(path: &str) -> Result<String, AppError> {
    let file = File::open(path).map_err(|error| {
        AppError::new(
            "FILE_READ_ERROR",
            format!("无法读取文件 {}: {}", path, error),
        )
    })?;

    let mut reader = BufReader::new(file);
    let mut hasher = blake3::Hasher::new();
    let mut buffer = [0u8; 65536]; // 64KB 缓冲区

    loop {
        let count = reader.read(&mut buffer).map_err(|error| {
            AppError::new(
                "FILE_READ_ERROR",
                format!("读取文件内容失败 {}: {}", path, error),
            )
        })?;

        if count == 0 {
            break;
        }

        hasher.update(&buffer[..count]);
    }

    Ok(hasher.finalize().to_hex().to_string())
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DuplicateGroup {
    pub content_hash: String,
    pub size_bytes: u64,
    pub file_count: u64,
    pub wasted_bytes: u64,
    pub files: Vec<FileEntry>,
}

fn to_i64(value: u64) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

fn from_i64(value: i64) -> u64 {
    u64::try_from(value).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::{create_scan_run, initialize, insert_file_batch, open};
    use std::fs;
    use uuid::Uuid;

    #[test]
    fn identifies_duplicates_by_size_and_content() {
        let temp_dir = std::env::temp_dir().join(format!("luma-dup-{}", Uuid::new_v4()));
        fs::create_dir_all(&temp_dir).expect("create temp dir");

        // 创建测试文件
        let file_a = temp_dir.join("a.txt");
        let file_b = temp_dir.join("b.txt");
        let file_c = temp_dir.join("c.txt");

        fs::write(&file_a, b"duplicate content").expect("write a");
        fs::write(&file_b, b"duplicate content").expect("write b");
        fs::write(&file_c, b"unique content!!!").expect("write c");

        // 创建数据库并插入文件记录
        let db_path = temp_dir.join("test.db");
        initialize(&db_path).expect("init db");
        create_scan_run(&db_path, "scan-1", "/test", 100).expect("create scan");

        let mut connection = open(&db_path).expect("open db");
        let files = vec![
            FileEntry {
                id: 0,
                path: file_a.to_string_lossy().into_owned(),
                name: "a.txt".to_owned(),
                extension: Some("txt".to_owned()),
                category: "documents".to_owned(),
                size_bytes: 17,
                modified_at: Some(100),
                is_hidden: false,
                content_hash: None,
            },
            FileEntry {
                id: 0,
                path: file_b.to_string_lossy().into_owned(),
                name: "b.txt".to_owned(),
                extension: Some("txt".to_owned()),
                category: "documents".to_owned(),
                size_bytes: 17,
                modified_at: Some(100),
                is_hidden: false,
                content_hash: None,
            },
            FileEntry {
                id: 0,
                path: file_c.to_string_lossy().into_owned(),
                name: "c.txt".to_owned(),
                extension: Some("txt".to_owned()),
                category: "documents".to_owned(),
                size_bytes: 17,
                modified_at: Some(100),
                is_hidden: false,
                content_hash: None,
            },
        ];
        insert_file_batch(&mut connection, "scan-1", &files).expect("insert files");
        drop(connection);

        // 查找重复
        let duplicates = find_duplicate_candidates(&db_path, "scan-1", 1).expect("find duplicates");

        assert_eq!(duplicates.len(), 1);
        assert_eq!(duplicates[0].file_count, 2);
        assert_eq!(duplicates[0].size_bytes, 17);
        assert_eq!(duplicates[0].wasted_bytes, 17);

        fs::remove_dir_all(temp_dir).expect("cleanup");
    }
}
