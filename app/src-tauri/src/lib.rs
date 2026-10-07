mod commands;
mod error;
mod jobs;
mod scheduler;
mod state;
mod tray;

use std::sync::Arc;

use infodesk_core::Store;
use tauri::{Emitter, Manager, WindowEvent};

use crate::state::{AppState, EVENT_ITEMS, EVENT_STATUS};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec!["--hidden"]),
        ))
        .setup(|app| {
            let db_path = app.path().app_data_dir()?.join("infodesk.db");
            let store = Store::open(&db_path)?;
            infodesk_core::seed::seed_defaults(&store)?;
            let handle = app.handle().clone();
            let notify: state::Notifier = Arc::new(move |name: &str| {
                let _ = handle.emit(name, ());
                if name == EVENT_STATUS || name == EVENT_ITEMS {
                    tray::refresh(&handle);
                }
            });
            app.manage(AppState::new(store, notify));
            app.state::<AppState>().load_persisted();
            tray::install(app)?;
            // ウィンドウは非表示で作られる。ログイン時の自動起動 (`--hidden`) 以外は表示する。
            if !std::env::args().any(|a| a == "--hidden")
                && let Some(window) = app.get_webview_window("main")
            {
                let _ = window.show();
            }
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(scheduler::run_loop(handle));
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                let state = window.app_handle().state::<AppState>();
                if commands::settings::stay_resident(&state) {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::status::get_status,
            commands::settings::get_settings,
            commands::settings::set_setting,
            commands::settings::test_ollama,
            commands::settings::test_claude,
            commands::settings::set_launch_at_login,
            commands::settings::show_main_window,
            commands::items::list_items,
            commands::items::count_items,
            commands::items::get_item,
            commands::items::set_read,
            commands::items::set_starred,
            commands::items::mark_all_read,
            commands::items::open_item,
            commands::items::sidebar_counts,
            commands::sources::list_sources,
            commands::sources::add_rss_source,
            commands::sources::update_source,
            commands::sources::set_source_enabled,
            commands::sources::delete_source,
            commands::topics::list_topics,
            commands::topics::add_topic,
            commands::topics::update_topic,
            commands::topics::delete_topic,
            commands::digests::get_digests,
            commands::digests::regenerate_digest,
            commands::digests::collect_now,
            commands::digests::retry_pending_digests,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|_app, _event| {
            // Dock のアイコンを押したら、隠したウィンドウを表示する。
            #[cfg(target_os = "macos")]
            if let tauri::RunEvent::Reopen { .. } = _event {
                tray::show_main_window(_app);
            }
        });
}
