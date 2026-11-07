// 托盘性能监测 (Tray performance monitor, CLN-403): the tray menu shows a
// live CPU / memory line, opt-in via an explicit switch.
//
// Product contract:
//   - OFF by default and persisted (settings.rs). Sampling only happens
//     while the app is open AND the user turned the switch on — there is no
//     background process beyond the tray the app already owns.
//   - The monitor thread samples `system_info::system_status()` on a fixed
//     interval (10 s macOS / 20 s Windows) and updates the menu line plus
//     the tray tooltip; every other poll it just sleeps, so an idle
//     monitored tray costs nothing between samples.
//   - The switch lives in two places that stay in sync: the tray menu
//     (CheckMenuItem) and the in-app system status card (via the
//     `tray-monitor-changed` event).

use crate::error::AppError;
use crate::i18n::TrayStrings;
use crate::settings::{self, AppSettings};
use crate::{commands::AppState, system_info};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tauri::menu::{CheckMenuItem, MenuItem};
use tauri::{AppHandle, Emitter, Manager, Wry};

/// How often the menu line refreshes while monitoring is on.
pub fn sample_interval() -> Duration {
    if cfg!(target_os = "windows") {
        // PowerShell CIM sampling is heavier; keep it gentle.
        Duration::from_secs(20)
    } else {
        Duration::from_secs(10)
    }
}

const POLL_INTERVAL: Duration = Duration::from_millis(500);

/// Handles to the menu items the monitor thread updates.
#[derive(Clone)]
pub struct TrayMenuHandles {
    pub performance_item: MenuItem<Wry>,
    pub monitor_toggle: CheckMenuItem<Wry>,
}

pub struct TrayMonitorState {
    pub enabled: Arc<AtomicBool>,
    pub handles: TrayMenuHandles,
    pub strings: TrayStrings,
}

/// Spawn the sampler thread. It starts with the persisted switch state; when
/// disabled it only polls the flag (no sampling at all).
pub fn spawn(
    app: AppHandle,
    strings: TrayStrings,
    handles: TrayMenuHandles,
    initial_enabled: bool,
) -> Arc<AtomicBool> {
    let enabled = Arc::new(AtomicBool::new(initial_enabled));
    let thread_enabled = enabled.clone();
    let interval = sample_interval();
    std::thread::Builder::new()
        .name("tray-monitor".into())
        .spawn(move || {
            // Start expired so enabling produces a sample on the first poll.
            let mut last_sample = Instant::now() - interval;
            let tray = app.tray_by_id("main");
            loop {
                std::thread::sleep(POLL_INTERVAL);
                if !thread_enabled.load(Ordering::Relaxed) {
                    continue;
                }
                if last_sample.elapsed() < interval {
                    continue;
                }
                last_sample = Instant::now();
                let status = system_info::system_status();
                let (Some(cpu), Some(total), Some(used)) = (
                    status.cpu_usage_percent,
                    status.memory_total_bytes,
                    status.memory_used_bytes,
                ) else {
                    continue;
                };
                let memory_percent = if total > 0 {
                    used as f32 / total as f32 * 100.0
                } else {
                    0.0
                };
                let _ = handles
                    .performance_item
                    .set_text(strings.performance_text(cpu, memory_percent));
                if let Some(tray) = &tray {
                    let _ =
                        tray.set_tooltip(Some(strings.performance_tooltip(cpu, memory_percent)));
                }
            }
        })
        .ok();
    enabled
}

/// Apply a new switch state: persist, update the sampler, sync the menu
/// checkbox and the performance line, and tell the frontend.
pub fn apply_enabled(app: &AppHandle, value: bool) -> Result<(), AppError> {
    let data_dir: PathBuf = app.state::<AppState>().app_data_dir.clone();
    settings::save(
        &data_dir,
        &AppSettings {
            tray_monitor_enabled: value,
        },
    )?;

    let state = app.state::<TrayMonitorState>();
    state.enabled.store(value, Ordering::Relaxed);
    let _ = state.handles.monitor_toggle.set_checked(value);
    if value {
        // The sampler picks this up on its next poll (≤ POLL_INTERVAL) and
        // replaces the placeholder with a real sample.
        let _ = state.handles.performance_item.set_enabled(true);
        let _ = state
            .handles
            .performance_item
            .set_text(state.strings.sampling);
    } else {
        let _ = state
            .handles
            .performance_item
            .set_text(state.strings.performance_off);
        let _ = state.handles.performance_item.set_enabled(false);
        if let Some(tray) = app.tray_by_id("main") {
            let _ = tray.set_tooltip(Some(state.strings.tooltip));
        }
    }
    let _ = app.emit("tray-monitor-changed", value);
    Ok(())
}
