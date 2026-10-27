use serde::Serialize;
use std::fmt::{Display, Formatter};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppError {
    pub code: &'static str,
    pub message: String,
}

impl AppError {
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    pub fn invalid_path(message: impl Into<String>) -> Self {
        Self::new("INVALID_PATH", message)
    }
}

impl Display for AppError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for AppError {}

impl From<std::io::Error> for AppError {
    fn from(error: std::io::Error) -> Self {
        let code = if error.kind() == std::io::ErrorKind::PermissionDenied {
            "PERMISSION_DENIED"
        } else {
            "SCAN_FAILED"
        };
        Self::new(code, error.to_string())
    }
}

impl From<rusqlite::Error> for AppError {
    fn from(error: rusqlite::Error) -> Self {
        if let rusqlite::Error::SqliteFailure(code, _) = &error {
            if code.code == rusqlite::ErrorCode::DiskFull {
                return Self::new(
                    "INDEX_DISK_FULL",
                    "磁盘空间不足，索引写入已停止。请释放空间后重试。原文件未被修改。",
                );
            }
        }
        Self::new("DATABASE_ERROR", error.to_string())
    }
}
