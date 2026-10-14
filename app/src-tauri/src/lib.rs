mod commands;
mod error;
mod jobs;
mod reader;
mod scheduler;
mod sidecar;
mod state;
mod sync;
// Task 3 wires this into sync; until then most of it is unused.
#[allow(dead_code)]
mod tailnet;
mod tray;

use std::sync::Arc;

use infodesk_core::Store;
use tauri::{Emitter, Manager, WindowEvent};

use crate::state::{AppState, EVENT_ITEMS, EVENT_STATUS};

/// tsnet は log.tailscale.com へログを送る。止めるのは環境変数 `TS_NO_LOGS_NO_SUPPORT`
/// (tailscale.com/envknob/envknob.go の `NoLogsNoSupport`、logpolicy.go:700 と :874)。
/// ただし Go のランタイムは起動時 (libtailscale を読み込んだとき) に環境を写し取るので、
/// 後から `set_var` しても Go からは見えない。そのため、未設定なら変数を付けて自分自身を
/// `exec` し直す (同じプロセスのまま。この時点ではスレッドも tsnet の端末もない)。
/// `set_var` を使わないので Rust 2024 の unsafe も要らない。
fn ensure_no_tailscale_logs() {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        if std::env::var_os(tailnet::NO_LOGS_ENV).is_some() {
            return;
        }
        if let Ok(exe) = std::env::current_exe() {
            let err = std::process::Command::new(exe)
                .args(std::env::args_os().skip(1))
                .env(tailnet::NO_LOGS_ENV, "true")
                .exec();
            eprintln!("tailnet: could not re-exec to disable log uploads: {err}");
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    ensure_no_tailscale_logs();
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
            let tailnet = Arc::new(tailnet::EmbeddedTailnet::new(notify.clone()));
            let opener = app.handle().clone();
            tailnet.configure(
                app.path().app_data_dir()?.join("tailscale"),
                Arc::new(move |url: &str| {
                    let _ =
                        tauri_plugin_opener::OpenerExt::opener(&opener).open_url(url, None::<&str>);
                }),
            );
            let mut state = AppState::new(store, notify);
            state.tailnet = tailnet;
            app.manage(state);
            app.state::<AppState>().load_persisted();
            sync::init_status(&app.state::<AppState>());
            // 保存済みの状態があるので、ログインは要らない。待たされるので別スレッドで始める。
            sync::start_tailnet_if_needed(&app.state::<AppState>());
            let handle = app.handle().clone();
            app.state::<AppState>()
                .set_sync_launcher(Arc::new(move || sync::spawn_sync(&handle)));
            tray::install(app)?;
            // ウィンドウは非表示で作られる。ログイン時の自動起動 (`--hidden`) 以外は表示する。
            if !std::env::args().any(|a| a == "--hidden")
                && let Some(window) = app.get_webview_window("main")
            {
                let _ = window.show();
            }
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(scheduler::run_loop(handle));
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                handle.state::<AppState>().chat.restart(&handle).await;
            });
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event
                && window.label() != reader::READER_WINDOW
            {
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
            commands::items::open_link,
            reader::command::open_in_reader,
            reader::reader_back,
            reader::reader_forward,
            reader::reader_reload,
            reader::reader_open_external,
            reader::reader_close,
            reader::reader_state,
            commands::items::sidebar_counts,
            commands::sources::list_sources,
            commands::sources::add_rss_source,
            commands::sources::update_source,
            commands::sources::set_source_enabled,
            commands::sources::delete_source,
            commands::sources::source_item_counts,
            commands::topics::list_topics,
            commands::chat::list_chat_threads,
            commands::chat::create_chat_thread,
            commands::chat::rename_chat_thread,
            commands::chat::delete_chat_thread,
            commands::chat::get_chat_messages,
            commands::chat::save_chat_messages,
            commands::chat_endpoint::chat_endpoint,
            commands::chat_endpoint::restart_chat,
            commands::topics::add_topic,
            commands::topics::update_topic,
            commands::topics::delete_topic,
            commands::digests::get_digests,
            commands::digests::digest_stats,
            commands::digests::regenerate_digest,
            commands::digests::collect_now,
            commands::digests::retry_pending_digests,
            commands::sync::sync_status,
            commands::sync::sync_login,
            commands::sync::sync_logout,
            commands::sync::sync_set_enabled,
            commands::sync::sync_set_device_name,
            commands::sync::sync_now,
            commands::sync::sync_set_backend,
            commands::sync::sync_set_nas,
            commands::sync::tailnet_login,
            commands::sync::tailnet_logout,
            commands::sync::sync_test_nas,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| {
            // Dock のアイコンを押したら、隠したウィンドウを表示する。
            #[cfg(target_os = "macos")]
            if let tauri::RunEvent::Reopen { .. } = event {
                tray::show_main_window(app);
            }
            if let tauri::RunEvent::Exit = event {
                app.state::<AppState>().chat.stop();
                app.state::<AppState>().tailnet.stop();
            }
        });
}
