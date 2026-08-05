mod commands;
mod database;
mod error;
mod models;
mod scanner;

use commands::AppState;
use std::{collections::HashMap, sync::Mutex};
use tauri::{menu::MenuBuilder, tray::TrayIconBuilder, Manager};
use tauri_plugin_updater::UpdaterExt;

const TRAY_ICON: tauri::image::Image<'_> = tauri::include_image!("./icons/tray-icon.png");

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(|app| {
            let tray_menu = MenuBuilder::new(app)
                .text("show", "显示 Luma")
                .separator()
                .text("quit", "退出 Luma")
                .build()?;
            TrayIconBuilder::with_id("main")
                .icon(TRAY_ICON)
                .icon_as_template(true)
                .tooltip("Luma · 本地空间观察")
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

            // 启动后台更新检查
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                match handle.updater() {
                    Ok(updater) => {
                        if let Ok(Some(update)) = updater.check().await {
                            let _ = update.download_and_install(|_chunk, _total| {}, || {}).await;
                        }
                    }
                    Err(_) => {}
                }
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
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
