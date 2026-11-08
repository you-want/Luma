// Tauri command layer, split by product domain. Each submodule wraps the
// matching backend module in async commands that run blocking work on the
// worker pool (`background_task` / `database_task`) behind the process-wide
// `DATABASE_BUSY` guard (`file_mutation`). `lib.rs` registers every command
// from the `commands::` namespace via the re-exports below — submodule
// organization is internal and must not change registration.

mod cleaner;
mod files;
mod organize;
mod preview;
mod scan;
mod sessions;
mod storage;

pub use cleaner::*;
pub use files::*;
pub use organize::*;
pub use preview::*;
pub use scan::*;
pub use sessions::*;
pub use storage::*;

use crate::error::AppError;
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::{SystemTime, UNIX_EPOCH},
};
use tauri::State;

pub struct AppState {
    pub app_data_dir: PathBuf,
    pub database_path: PathBuf,
    pub cancellations: Mutex<HashMap<String, Arc<AtomicBool>>>,
}

// Synchronous SQLite, filesystem and OS calls must never run on the WebView's
// event thread. Keep the command wrappers asynchronous and use the blocking pool.
async fn background_task<T: Send + 'static>(
    task: impl FnOnce() -> Result<T, AppError> + Send + 'static,
) -> Result<T, AppError> {
    tauri::async_runtime::spawn_blocking(task)
        .await
        .map_err(|error| AppError::new("OP_FAILED", error.to_string()))?
}

async fn database_task<T: Send + 'static>(
    state: State<'_, AppState>,
    task: impl FnOnce(&std::path::Path) -> Result<T, AppError> + Send + 'static,
) -> Result<T, AppError> {
    let database_path = state.database_path.clone();
    background_task(move || task(&database_path)).await
}

static DATABASE_BUSY: AtomicBool = AtomicBool::new(false);

struct DatabaseActivity;
impl DatabaseActivity {
    fn acquire() -> Result<Self, AppError> {
        DATABASE_BUSY
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| AppError::new("OP_BUSY", "扫描、文件操作或索引维护仍在进行。"))?;
        Ok(Self)
    }
}
impl Drop for DatabaseActivity {
    fn drop(&mut self) {
        DATABASE_BUSY.store(false, Ordering::Release);
    }
}

/// Serialize every mutating file/organization operation: while one is in
/// flight any other write is rejected with `OP_BUSY` instead of racing.
fn file_mutation<T>(task: impl FnOnce() -> Result<T, AppError>) -> Result<T, AppError> {
    let _activity = DatabaseActivity::acquire()?;
    task()
}

fn now_seconds() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|duration| i64::try_from(duration.as_secs()).ok())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocking_io_runs_on_a_worker_and_preserves_results_and_errors() {
        let caller = std::thread::current().id();
        let worker =
            tauri::async_runtime::block_on(background_task(|| Ok(std::thread::current().id())))
                .unwrap();
        assert_ne!(caller, worker);

        let error = tauri::async_runtime::block_on(background_task(|| {
            Err::<(), _>(AppError::new("TEST_IO", "original error"))
        }))
        .unwrap_err();
        assert_eq!(error.code, "TEST_IO");
        assert_eq!(error.message, "original error");
    }

    #[test]
    fn overlapping_file_writes_are_rejected_without_executing_and_unlock_after_failure() {
        let guard = DatabaseActivity::acquire().unwrap();
        let mut executed = false;
        let result = file_mutation(|| {
            executed = true;
            Ok(())
        });
        assert_eq!(result.unwrap_err().code, "OP_BUSY");
        assert!(!executed);
        drop(guard);
        assert!(file_mutation(|| Err::<(), _>(AppError::new("TEST_IO", "failure"))).is_err());
        assert_eq!(file_mutation(|| Ok(42)).unwrap(), 42);
    }
}
