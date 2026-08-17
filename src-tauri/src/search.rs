use crate::{
    database,
    error::AppError,
    models::{SearchRequest, SearchResponse},
};
use std::path::Path;

pub trait SearchProvider {
    fn search(
        &self,
        database_path: &Path,
        request: &SearchRequest,
    ) -> Result<SearchResponse, AppError>;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct LocalSqliteProvider;

impl SearchProvider for LocalSqliteProvider {
    fn search(
        &self,
        database_path: &Path,
        request: &SearchRequest,
    ) -> Result<SearchResponse, AppError> {
        database::search_files(database_path, request)
    }
}

pub fn search_files(
    database_path: &Path,
    request: &SearchRequest,
) -> Result<SearchResponse, AppError> {
    LocalSqliteProvider.search(database_path, request)
}
