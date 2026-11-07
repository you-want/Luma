// 应用设置 (App settings): a tiny JSON store next to the SQLite index for
// backend-owned preferences that must survive restarts (currently only the
// tray performance-monitor switch, CLN-403).
//
// Fail-safe contract: any read problem (missing file, corrupt JSON, bad
// types) degrades to defaults — the app must never refuse to start because
// of a settings file. Writes are atomic via a temp-file rename.

use crate::error::AppError;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default, rename_all = "camelCase")]
pub struct AppSettings {
    /// Tray performance monitoring — OFF by default: sampling only runs
    /// while the app is open AND the user explicitly opted in.
    pub tray_monitor_enabled: bool,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            tray_monitor_enabled: false,
        }
    }
}

fn settings_path(data_dir: &Path) -> PathBuf {
    data_dir.join("app_settings.json")
}

pub fn load(data_dir: &Path) -> AppSettings {
    let Ok(text) = std::fs::read_to_string(settings_path(data_dir)) else {
        return AppSettings::default();
    };
    serde_json::from_str(&text).unwrap_or_default()
}

pub fn save(data_dir: &Path, settings: &AppSettings) -> Result<(), AppError> {
    let path = settings_path(data_dir);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| AppError::new("OP_FAILED", error.to_string()))?;
    }
    let text = serde_json::to_string_pretty(settings)
        .map_err(|error| AppError::new("OP_FAILED", error.to_string()))?;
    let temp = path.with_extension("json.tmp");
    std::fs::write(&temp, text).map_err(|error| AppError::new("OP_FAILED", error.to_string()))?;
    std::fs::rename(&temp, &path).map_err(|error| AppError::new("OP_FAILED", error.to_string()))?;
    Ok(())
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use uuid::Uuid;

    fn temp_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("luma-settings-{}", Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn round_trips_and_defaults_to_disabled() {
        let dir = temp_dir();
        // Missing file → defaults.
        assert!(!load(&dir).tray_monitor_enabled);

        let settings = AppSettings {
            tray_monitor_enabled: true,
        };
        save(&dir, &settings).unwrap();
        assert_eq!(load(&dir), settings);
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn corrupt_file_degrades_to_defaults() {
        let dir = temp_dir();
        fs::write(settings_path(&dir), "{ not json !!!").unwrap();
        assert!(!load(&dir).tray_monitor_enabled);
        // A save after corruption heals the file.
        save(
            &dir,
            &AppSettings {
                tray_monitor_enabled: true,
            },
        )
        .unwrap();
        assert!(load(&dir).tray_monitor_enabled);
        fs::remove_dir_all(dir).ok();
    }
}
