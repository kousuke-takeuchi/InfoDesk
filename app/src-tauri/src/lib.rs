mod commands;
mod error;
mod jobs;
mod scheduler;
mod state;

use std::sync::Arc;

use infodesk_core::Store;
use tauri::{Emitter, Manager};

use crate::state::AppState;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let db_path = app.path().app_data_dir()?.join("infodesk.db");
            let store = Store::open(&db_path)?;
            infodesk_core::seed::seed_defaults(&store)?;
            let handle = app.handle().clone();
            let notify: state::Notifier = Arc::new(move |name: &str| {
                let _ = handle.emit(name, ());
            });
            app.manage(AppState::new(store, notify));
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(scheduler::run_loop(handle));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::status::get_status,
            commands::settings::get_settings,
            commands::settings::set_setting,
            commands::settings::test_ollama,
            commands::settings::test_claude,
            commands::items::list_items,
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
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
