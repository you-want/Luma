mod cleaner;
mod cleanup;
mod commands;
#[cfg(test)]
mod commands_integration;
mod database;
mod duplicates;
mod error;
mod file_manager;
mod file_ops;
mod i18n;
mod maintenance;
mod models;
mod organizer;
mod orphans;
mod projects;
mod purge;
mod scanner;
mod search;
mod sessions;
mod settings;
mod slim;
mod storage;
mod system_info;
mod tray_monitor;

use commands::AppState;
use std::{collections::HashMap, sync::Mutex};
use tauri::menu::{CheckMenuItem, MenuBuilder, MenuItem};
use tauri::{tray::TrayIconBuilder, Manager};

const TRAY_ICON: tauri::image::Image<'_> = tauri::include_image!("./icons/tray-icon.png");

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(|app| {
            // The tray is built once at startup, so it follows the system
            // language. See `i18n.rs` for why a live switch is out of scope.
            let app_data_dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&app_data_dir)?;
            let settings = settings::load(&app_data_dir);

            let tray = i18n::tray_strings(i18n::Language::detect());
            // Performance line: disabled-looking until monitoring is on.
            let performance_item = MenuItem::with_id(
                app,
                "performance",
                tray.performance_off,
                settings.tray_monitor_enabled,
                None::<&str>,
            )?;
            let monitor_toggle = CheckMenuItem::with_id(
                app,
                "toggle_monitor",
                tray.monitor_toggle,
                true,
                settings.tray_monitor_enabled,
                None::<&str>,
            )?;
            let tray_menu = MenuBuilder::new(app)
                .text("show", tray.show)
                .separator()
                .item(&performance_item)
                .item(&monitor_toggle)
                .separator()
                .text("quit", tray.quit)
                .build()?;
            TrayIconBuilder::with_id("main")
                .icon(TRAY_ICON)
                .icon_as_template(true)
                .tooltip(tray.tooltip)
                .menu(&tray_menu)
                .show_menu_on_left_click(true)
                .on_menu_event(|app, event| match event.id().as_ref() {
                    "show" => {
                        if let Some(window) = app.get_webview_window("main") {
                            let _ = window.show();
                            let _ = window.set_focus();
                        }
                    }
                    "toggle_monitor" => {
                        let current = app
                            .state::<tray_monitor::TrayMonitorState>()
                            .enabled
                            .load(std::sync::atomic::Ordering::Relaxed);
                        let _ = tray_monitor::apply_enabled(app, !current);
                    }
                    "quit" => app.exit(0),
                    _ => {}
                })
                .build(app)?;

            let database_path = app_data_dir.join("luma.sqlite3");
            database::initialize(&database_path)?;
            app.manage(AppState {
                app_data_dir: app_data_dir.clone(),
                database_path,
                cancellations: Mutex::new(HashMap::new()),
            });

            // Tray performance sampler (CLN-403): starts with the persisted
            // switch state; the flag alone decides whether it ever samples.
            let handles = tray_monitor::TrayMenuHandles {
                performance_item,
                monitor_toggle,
            };
            let enabled = tray_monitor::spawn(
                app.handle().clone(),
                tray.clone(),
                handles.clone(),
                settings.tray_monitor_enabled,
            );
            app.manage(tray_monitor::TrayMonitorState {
                enabled,
                handles,
                strings: tray,
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::start_scan,
            commands::get_index_storage,
            commands::compact_index_storage,
            commands::clear_scan_index,
            commands::cancel_scan,
            commands::get_latest_scan,
            commands::get_scan_summary,
            commands::list_large_files,
            commands::list_insights,
            commands::list_insight_files,
            commands::find_duplicates,
            commands::list_projects,
            commands::list_scan_history,
            commands::compare_scans,
            commands::search_files,
            commands::reveal_path,
            commands::get_cleanup_summary,
            commands::list_cleanup_files,
            commands::get_cleaner_overview,
            commands::list_cleaner_files,
            commands::execute_cleaner,
            commands::get_cleaner_whitelist,
            commands::add_cleaner_whitelist,
            commands::remove_cleaner_whitelist,
            commands::list_cleaner_operations,
            commands::discover_slim_candidates,
            commands::execute_slim,
            commands::get_maintenance_tasks,
            commands::run_maintenance_task,
            commands::get_system_status,
            commands::get_tray_monitor_enabled,
            commands::set_tray_monitor_enabled,
            commands::discover_purge_projects,
            commands::execute_purge,
            commands::discover_orphan_app_data,
            commands::execute_orphan_clean,
            commands::discover_agent_sessions,
            commands::trash_agent_sessions,
            commands::get_directory_nodes,
            commands::list_directory_files,
            commands::open_path,
            commands::read_text_preview,
            commands::read_binary_preview,
            commands::trash_files,
            commands::rename_file,
            commands::move_files,
            commands::copy_files,
            commands::list_undoable_operations,
            commands::undo_file_operation,
            commands::plan_organize,
            commands::execute_organize_plan,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
