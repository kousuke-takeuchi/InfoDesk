use chrono::{FixedOffset, Local};
use infodesk_core::model::{Category, DigestScope, ItemFilter, Period};
use infodesk_core::period::period_start;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{App, AppHandle, Manager, Wry};

use crate::state::{AppState, AppStatus};

pub struct TrayMenu {
    pub status_line: MenuItem<Wry>,
    pub unread_line: MenuItem<Wry>,
}

/// メニューの状態行。1行目は収集、2行目は未読と日次要約。
pub fn status_lines(
    status: &AppStatus,
    unread: i64,
    daily_done: bool,
    tz: FixedOffset,
) -> (String, String) {
    let first = if status.collecting {
        "収集中…".to_string()
    } else {
        match status.last_collect_at {
            Some(at) => format!("最終収集 {}", at.with_timezone(&tz).format("%H:%M")),
            None => "未収集".to_string(),
        }
    };
    let digest = if status.digesting || status.summarizing {
        "要約を生成中…".to_string()
    } else if let Some(e) = &status.last_error {
        format!("エラー: {e}")
    } else if daily_done {
        "日次要約 生成済み".to_string()
    } else {
        "日次要約 未生成".to_string()
    };
    (first, format!("未読 {unread}件 · {digest}"))
}

fn daily_done_today(state: &AppState) -> bool {
    let today = period_start(Period::Day, Local::now().date_naive());
    state
        .with_store(|s| {
            for c in Category::ALL {
                if s.get_digest(Period::Day, today, &DigestScope::Category(c))?
                    .is_some()
                {
                    return Ok(true);
                }
            }
            Ok(false)
        })
        .unwrap_or(false)
}

fn unread_total(state: &AppState) -> i64 {
    state
        .with_store(|s| {
            s.count_items(&ItemFilter {
                unread_only: true,
                ..Default::default()
            })
        })
        .unwrap_or(0)
}

/// 状態行を更新する。`status-changed` と `items-changed` のたびに呼ぶ。
pub fn refresh(app: &AppHandle) {
    let Some(menu) = app.try_state::<TrayMenu>() else {
        return;
    };
    let state = app.state::<AppState>();
    let (first, second) = status_lines(
        &state.status(),
        unread_total(&state),
        daily_done_today(&state),
        state.tz,
    );
    let _ = menu.status_line.set_text(first);
    let _ = menu.unread_line.set_text(second);
}

pub fn show_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

pub fn install(app: &App) -> tauri::Result<()> {
    let status_line = MenuItem::with_id(app, "status", "未収集", false, None::<&str>)?;
    let unread_line = MenuItem::with_id(app, "unread", "未読 0件", false, None::<&str>)?;
    let collect = MenuItem::with_id(app, "collect", "今すぐ収集", true, None::<&str>)?;
    let open = MenuItem::with_id(app, "open", "ウィンドウを開く", true, Some("CmdOrCtrl+O"))?;
    let settings = MenuItem::with_id(app, "settings", "設定…", true, Some("CmdOrCtrl+,"))?;
    let quit = MenuItem::with_id(app, "quit", "InfoDesk を終了", true, Some("CmdOrCtrl+Q"))?;
    let menu = Menu::with_items(
        app,
        &[
            &status_line,
            &unread_line,
            &PredefinedMenuItem::separator(app)?,
            &collect,
            &open,
            &PredefinedMenuItem::separator(app)?,
            &settings,
            &PredefinedMenuItem::separator(app)?,
            &quit,
        ],
    )?;
    app.manage(TrayMenu {
        status_line,
        unread_line,
    });
    TrayIconBuilder::with_id("main")
        .icon(app.default_window_icon().expect("app icon").clone())
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "collect" => {
                let _ = crate::commands::digests::collect_now(app.clone());
            }
            "open" => show_main_window(app),
            "settings" => {
                show_main_window(app);
                let _ = tauri::Emitter::emit(app, "open-settings", ());
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .build(app)?;
    refresh(app.handle());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn jst() -> FixedOffset {
        FixedOffset::east_opt(9 * 3600).unwrap()
    }

    #[test]
    fn status_lines_describe_collect_and_digest_state() {
        let mut st = AppStatus::default();
        assert_eq!(
            status_lines(&st, 0, false, jst()),
            (
                "未収集".to_string(),
                "未読 0件 · 日次要約 未生成".to_string()
            )
        );
        st.last_collect_at = Some("2026-10-06T00:00:00Z".parse().unwrap());
        assert_eq!(status_lines(&st, 61, true, jst()).0, "最終収集 09:00");
        assert_eq!(
            status_lines(&st, 61, true, jst()).1,
            "未読 61件 · 日次要約 生成済み"
        );
        st.collecting = true;
        assert_eq!(status_lines(&st, 61, true, jst()).0, "収集中…");
        st.collecting = false;
        st.digesting = true;
        assert_eq!(
            status_lines(&st, 61, false, jst()).1,
            "未読 61件 · 要約を生成中…"
        );
        st.digesting = false;
        st.last_error = Some("ollama 404: model not found".into());
        assert_eq!(
            status_lines(&st, 61, false, jst()).1,
            "未読 61件 · エラー: ollama 404: model not found"
        );
    }
}
