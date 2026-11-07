// 系统维护 (System Maintenance): the disk-relevant maintenance tasks a GUI
// app can do honestly:
//   - QuickLook cache refresh (`qlmanage -r cache`) — no privileges, reclaims
//     stale thumbnail space and fixes broken previews.
//   - DNS cache flush — requires administrator privileges, which the OS
//     surfaces as its native authorization dialog (Touch ID where the Mac
//     has it configured). One-off privileged tasks go through the system
//     prompt rather than a standing privileged helper.
// Every run is logged to `file_operations` (scan_id 'cleaner') so the
// cleanup history stays complete.

use crate::error::AppError;
use rusqlite::params;
use serde::Serialize;
use std::process::Command;

// ── Public types ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MaintenanceTask {
    /// Stable task id: quicklookCache | dnsFlush.
    pub id: String,
    /// Whether the task can run on this platform.
    pub available: bool,
    /// Whether the OS will ask for administrator authorization.
    pub needs_admin: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MaintenanceReport {
    pub tasks: Vec<MaintenanceTask>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MaintenanceOutcome {
    pub operation_id: Option<String>,
    pub detail: String,
}

// ── Task registry ────────────────────────────────────────────────────────────

fn task_definitions() -> Vec<MaintenanceTask> {
    vec![
        MaintenanceTask {
            id: "quicklookCache".to_string(),
            available: cfg!(target_os = "macos"),
            needs_admin: false,
        },
        MaintenanceTask {
            id: "dnsFlush".to_string(),
            available: cfg!(target_os = "macos"),
            needs_admin: true,
        },
    ]
}

pub fn task_report() -> MaintenanceReport {
    MaintenanceReport {
        tasks: task_definitions(),
    }
}

/// Run one maintenance task. `task` is a closed enum from the registry —
/// anything else is rejected.
pub fn run_task(db_path: &std::path::Path, task: &str) -> Result<MaintenanceOutcome, AppError> {
    let detail = match task {
        "quicklookCache" => {
            if !cfg!(target_os = "macos") {
                return Err(AppError::new(
                    "PLATFORM_UNSUPPORTED",
                    "此维护任务仅支持 macOS。",
                ));
            }
            let status = Command::new("qlmanage")
                .args(["-r", "cache"])
                .status()
                .map_err(|error| AppError::new("OP_FAILED", error.to_string()))?;
            if !status.success() {
                return Err(AppError::new("OP_FAILED", "QuickLook 缓存刷新未完成。"));
            }
            "QuickLook 缓存已刷新。".to_string()
        }
        "dnsFlush" => {
            if !cfg!(target_os = "macos") {
                return Err(AppError::new(
                    "PLATFORM_UNSUPPORTED",
                    "此维护任务仅支持 macOS。",
                ));
            }
            // The OS shows its native authorization dialog (password or
            // Touch ID when configured). A decline surfaces as a non-zero
            // exit with error 128 on stderr — reported, not retried.
            let script = "do shell script \"dscacheutil -flushcache; killall -HUP mDNSResponder\" with administrator privileges";
            let status = Command::new("osascript")
                .args(["-e", script])
                .status()
                .map_err(|error| AppError::new("OP_FAILED", error.to_string()))?;
            if !status.success() {
                return Err(AppError::new(
                    "OP_FAILED",
                    "DNS 缓存刷新未完成（需要管理员授权）。",
                ));
            }
            "DNS 缓存已刷新。".to_string()
        }
        _ => {
            return Err(AppError::new(
                "INVALID_MAINTENANCE_TASK",
                format!("未知的维护任务：{task}。"),
            ))
        }
    };

    let operation_id = begin_maintenance_operation(db_path, task)?;
    // Maintenance runs are all-or-nothing: log completion directly.
    crate::file_ops::finish_operation(db_path, &operation_id, 1, &[])?;
    Ok(MaintenanceOutcome {
        operation_id: Some(operation_id),
        detail,
    })
}

/// Log a maintenance run with its own kind, reusing the cleaner sentinel so
/// the cleanup history shows it.
fn begin_maintenance_operation(db_path: &std::path::Path, task: &str) -> Result<String, AppError> {
    let operation_id = uuid::Uuid::new_v4().to_string();
    let label = match task {
        "quicklookCache" => "系统维护：QuickLook 缓存刷新",
        "dnsFlush" => "系统维护：DNS 缓存刷新",
        other => other,
    };
    crate::database::open(db_path)?
        .execute(
            "INSERT INTO file_operations (id, scan_id, root_path, kind, label, status, created_at)
             VALUES (?1, 'cleaner', '', 'maintenance', ?2, 'running', ?3)",
            params![
                operation_id,
                label,
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs() as i64)
                    .unwrap_or(0)
            ],
        )
        .map_err(|error| AppError::new("DATABASE_ERROR", error.to_string()))?;
    Ok(operation_id)
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::initialize;
    use std::fs;
    use uuid::Uuid;

    #[test]
    fn registry_is_closed_and_platform_annotated() {
        let report = task_report();
        assert_eq!(report.tasks.len(), 2);
        let ids: Vec<&str> = report.tasks.iter().map(|task| task.id.as_str()).collect();
        assert_eq!(ids, vec!["quicklookCache", "dnsFlush"]);
        for task in &report.tasks {
            assert_eq!(task.available, cfg!(target_os = "macos"));
        }
        let dns = report.tasks.iter().find(|t| t.id == "dnsFlush").unwrap();
        assert!(dns.needs_admin);
    }

    #[test]
    fn unknown_task_is_rejected() {
        let dir = std::env::temp_dir().join(format!("luma-maint-{}", Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        let db = dir.join("test.db");
        initialize(&db).unwrap();
        let error = run_task(&db, "reboot").expect_err("unknown tasks must be rejected");
        assert_eq!(error.code, "INVALID_MAINTENANCE_TASK");
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn quicklook_task_runs_and_logs() {
        let dir = std::env::temp_dir().join(format!("luma-maint-{}", Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        let db = dir.join("test.db");
        initialize(&db).unwrap();
        let outcome = run_task(&db, "quicklookCache").expect("qlmanage is always present on macOS");
        assert!(outcome.detail.contains("QuickLook"));
        // The run shows up in the cleaner history.
        let operations = crate::cleaner::list_operations(&db, 10).unwrap();
        assert!(operations
            .iter()
            .any(|op| op.label.contains("QuickLook") && op.status == "completed"));
        fs::remove_dir_all(dir).ok();
    }
}
