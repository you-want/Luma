mod cleanup;
mod commands;
mod database;
mod duplicates;
mod error;
mod file_manager;
mod file_ops;
mod i18n;
mod models;
mod organizer;
mod projects;
mod scanner;

use commands::AppState;
use std::{collections::HashMap, sync::Mutex};
use tauri::{menu::MenuBuilder, tray::TrayIconBuilder, Manager};

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
            let tray = i18n::tray_strings(i18n::Language::detect());
            let tray_menu = MenuBuilder::new(app)
                .text("show", tray.show)
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
                    "quit" => app.exit(0),
                    _ => {}
                })
                .build(app)?;

            let app_data_dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&app_data_dir)?;
            let database_path = app_data_dir.join("luma.sqlite3");
            database::initialize(&database_path)?;
            app.manage(AppState {
                database_path,
                cancellations: Mutex::new(HashMap::new()),
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::start_scan,
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
            commands::get_directory_nodes,
            commands::list_directory_files,
            commands::open_path,
            commands::read_text_preview,
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
