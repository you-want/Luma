use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartScanRequest {
    pub root_path: String,
    pub include_hidden: bool,
    pub stay_on_file_system: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ScanStatus {
    Running,
    Completed,
    Cancelled,
    Failed,
}

impl ScanStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Completed => "completed",
            Self::Cancelled => "cancelled",
            Self::Failed => "failed",
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct ScanStats {
    pub files_scanned: u64,
    pub directories_scanned: u64,
    pub bytes_scanned: u64,
    pub errors: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanProgress {
    pub scan_id: String,
    pub status: ScanStatus,
    pub files_scanned: u64,
    pub directories_scanned: u64,
    pub bytes_scanned: u64,
    pub errors: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_path: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileEntry {
    pub id: i64,
    pub path: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extension: Option<String>,
    pub category: String,
    pub size_bytes: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub modified_at: Option<i64>,
    pub is_hidden: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_hash: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CategorySummary {
    pub category: String,
    pub file_count: u64,
    pub size_bytes: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanSummary {
    pub scan_id: String,
    pub root_path: String,
    pub status: ScanStatus,
    pub started_at: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finished_at: Option<i64>,
    pub total_files: u64,
    pub total_directories: u64,
    pub total_bytes: u64,
    pub error_count: u64,
    pub categories: Vec<CategorySummary>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InsightSummary {
    pub kind: String,
    pub file_count: u64,
    pub size_bytes: u64,
}

/// How search results are ordered. A closed enum (not a raw column string from
/// the client) so the backend never interpolates untrusted text into `ORDER BY`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum SearchSort {
    NameAsc,
    NameDesc,
    SizeAsc,
    #[default]
    SizeDesc,
    ModifiedAsc,
    ModifiedDesc,
}

impl SearchSort {
    /// The `ORDER BY` clause for this sort. Every arm is a compile-time constant,
    /// so this is safe to embed in SQL. A stable `id` tiebreaker keeps pagination
    /// deterministic when the primary key ties.
    pub fn order_by(self) -> &'static str {
        match self {
            Self::NameAsc => "name COLLATE NOCASE ASC, id ASC",
            Self::NameDesc => "name COLLATE NOCASE DESC, id ASC",
            Self::SizeAsc => "size_bytes ASC, id ASC",
            Self::SizeDesc => "size_bytes DESC, id ASC",
            Self::ModifiedAsc => "modified_at ASC, id ASC",
            Self::ModifiedDesc => "modified_at DESC, id ASC",
        }
    }
}

/// A search over one scan snapshot. All filters are optional and combine with
/// AND; `query` matches name or path. Only the current scan's indexed rows are
/// searched — no filesystem access, no file contents.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchRequest {
    pub scan_id: String,
    #[serde(default)]
    pub query: String,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub extension: Option<String>,
    #[serde(default)]
    pub min_size: Option<u64>,
    #[serde(default)]
    pub max_size: Option<u64>,
    #[serde(default)]
    pub modified_after: Option<i64>,
    #[serde(default)]
    pub modified_before: Option<i64>,
    #[serde(default)]
    pub include_hidden: bool,
    #[serde(default)]
    pub sort: SearchSort,
    #[serde(default)]
    pub limit: Option<u32>,
    #[serde(default)]
    pub offset: Option<u32>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchResponse {
    /// Rows for the requested page.
    pub files: Vec<FileEntry>,
    /// Total rows matching the filters across all pages, so the UI can show
    /// "N results" and drive pagination without loading every row.
    pub total: u64,
    pub limit: u32,
    pub offset: u32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CategoryDelta {
    pub category: String,
    pub base_size_bytes: u64,
    pub target_size_bytes: u64,
    pub base_file_count: u64,
    pub target_file_count: u64,
    pub size_delta: i64,
    pub file_count_delta: i64,
}

impl CategoryDelta {
    pub fn empty(category: &str) -> Self {
        Self {
            category: category.to_owned(),
            base_size_bytes: 0,
            target_size_bytes: 0,
            base_file_count: 0,
            target_file_count: 0,
            size_delta: 0,
            file_count_delta: 0,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanComparison {
    pub base: ScanSummary,
    pub target: ScanSummary,
    pub total_bytes_delta: i64,
    pub total_files_delta: i64,
    pub categories: Vec<CategoryDelta>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanFinished {
    pub scan_id: String,
    pub status: ScanStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<ScanSummary>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<crate::error::AppError>,
}
