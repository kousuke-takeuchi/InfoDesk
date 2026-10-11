use serde::Serialize;
use tauri::webview::{NewWindowResponse, WebviewBuilder};
use tauri::{
    AppHandle, Emitter, EventTarget, LogicalPosition, LogicalSize, Manager, Runtime, State, Url,
    Webview, WebviewUrl, Window,
};
use tauri_plugin_opener::OpenerExt;

use crate::commands::items::ensure_http_url;
use crate::error::AppResult;
use crate::state::AppState;

pub const READER_WINDOW: &str = "reader";
pub const READER_TOOLBAR: &str = "reader-toolbar";
pub const READER_CONTENT: &str = "reader-content";
pub const TOOLBAR_HEIGHT: f64 = 44.0;
pub const EVENT_READER: &str = "reader-state";

/// x, y, 幅, 高さ。
type Rect = (f64, f64, f64, f64);

/// ツールバーと記事ページの位置と大きさ。高さが足りなくても負にしない。
pub fn reader_layout(width: f64, height: f64) -> (Rect, Rect) {
    let bar_height = TOOLBAR_HEIGHT.min(height.max(0.0));
    let content_height = (height - TOOLBAR_HEIGHT).max(0.0);
    (
        (0.0, 0.0, width, bar_height),
        (0.0, bar_height, width, content_height),
    )
}

/// 閲覧ウィンドウの中で移動してよい URL (http/https だけ)。
pub fn allow_navigation(url: &Url) -> bool {
    matches!(url.scheme(), "http" | "https")
}

/// ツールバーに送る、表示中のページの状態。
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ReaderState {
    pub url: String,
    pub title: String,
}

fn current_state<R: Runtime>(webview: &Webview<R>) -> ReaderState {
    let url = webview.url().map(|u| u.to_string()).unwrap_or_default();
    let title = webview
        .state::<AppState>()
        .reader_title
        .lock()
        .map(|t| t.clone())
        .unwrap_or_default();
    ReaderState { url, title }
}

fn emit_state<R: Runtime>(webview: &Webview<R>) {
    let _ = webview.emit_to(
        EventTarget::webview(READER_TOOLBAR),
        EVENT_READER,
        current_state(webview),
    );
}

fn set_title<R: Runtime>(webview: &Webview<R>, title: String) {
    if let Ok(mut t) = webview.state::<AppState>().reader_title.lock() {
        *t = title;
    }
}

fn logical_size<R: Runtime>(window: &Window<R>) -> tauri::Result<LogicalSize<f64>> {
    Ok(window
        .inner_size()?
        .to_logical::<f64>(window.scale_factor()?))
}

/// 窓の大きさに合わせて2つの WebView を並べ直す。
fn relayout<R: Runtime>(app: &AppHandle<R>, window: &Window<R>) {
    let Ok(size) = logical_size(window) else {
        return;
    };
    let (bar, content) = reader_layout(size.width, size.height);
    for (label, r) in [(READER_TOOLBAR, bar), (READER_CONTENT, content)] {
        if let Some(wv) = app.get_webview(label) {
            let _ = wv.set_position(LogicalPosition::new(r.0, r.1));
            let _ = wv.set_size(LogicalSize::new(r.2, r.3));
        }
    }
}

/// 記事を閲覧ウィンドウで開く。窓が無ければ作り、あれば記事ページを移動して前面に出す。
pub fn open_in_reader<R: Runtime>(app: &AppHandle<R>, url: &str) -> AppResult<()> {
    ensure_http_url(url)?;
    let target = Url::parse(url).map_err(|e| e.to_string())?;
    if let Some(content) = app.get_webview(READER_CONTENT) {
        set_title(&content, String::new());
        content.navigate(target)?;
        if let Some(w) = app.get_window(READER_WINDOW) {
            let _ = w.show();
            let _ = w.set_focus();
        }
        return Ok(());
    }
    if let Ok(mut t) = app.state::<AppState>().reader_title.lock() {
        t.clear();
    }
    let window = tauri::window::WindowBuilder::new(app, READER_WINDOW)
        .title("閲覧")
        .inner_size(1100.0, 820.0)
        .min_inner_size(640.0, 400.0)
        .build()?;
    let size = logical_size(&window)?;
    let (bar, content) = reader_layout(size.width, size.height);
    let handle = app.clone();
    let toolbar = WebviewBuilder::new(READER_TOOLBAR, WebviewUrl::App("reader.html".into()));
    window.add_child(
        toolbar,
        LogicalPosition::new(bar.0, bar.1),
        LogicalSize::new(bar.2, bar.3),
    )?;
    let page = WebviewBuilder::new(READER_CONTENT, WebviewUrl::External(target))
        .on_navigation(allow_navigation)
        .on_new_window(move |url, _features| {
            // target="_blank" は同じ記事ページで開く
            if allow_navigation(&url)
                && let Some(c) = handle.get_webview(READER_CONTENT)
            {
                let _ = c.navigate(url);
            }
            NewWindowResponse::Deny
        })
        .on_page_load(|webview, _payload| emit_state(&webview))
        .on_document_title_changed(|webview, title| {
            set_title(&webview, title);
            emit_state(&webview);
        });
    window.add_child(
        page,
        LogicalPosition::new(content.0, content.1),
        LogicalSize::new(content.2, content.3),
    )?;
    let (app2, window2) = (app.clone(), window.clone());
    window.on_window_event(move |e| {
        if let tauri::WindowEvent::Resized(_) = e {
            relayout(&app2, &window2);
        }
    });
    Ok(())
}

fn content_webview<R: Runtime>(app: &AppHandle<R>) -> AppResult<Webview<R>> {
    Ok(app
        .get_webview(READER_CONTENT)
        .ok_or_else(|| "reader is not open".to_string())?)
}

/// フロントエンドから呼ぶ `open_in_reader` コマンド。関数名の衝突を避けるため別モジュールに置く。
pub mod command {
    use super::*;

    #[tauri::command]
    pub fn open_in_reader(app: AppHandle, url: String) -> AppResult<()> {
        super::open_in_reader(&app, &url)
    }
}

#[tauri::command]
pub fn reader_back(app: AppHandle) -> AppResult<()> {
    Ok(content_webview(&app)?.eval("history.back()")?)
}

#[tauri::command]
pub fn reader_forward(app: AppHandle) -> AppResult<()> {
    Ok(content_webview(&app)?.eval("history.forward()")?)
}

#[tauri::command]
pub fn reader_reload(app: AppHandle) -> AppResult<()> {
    Ok(content_webview(&app)?.reload()?)
}

/// 表示中の URL を既定のブラウザーで開く。
#[tauri::command]
pub fn reader_open_external(app: AppHandle) -> AppResult<()> {
    let url = content_webview(&app)?.url()?.to_string();
    ensure_http_url(&url)?;
    app.opener()
        .open_url(url, None::<&str>)
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn reader_close(app: AppHandle) -> AppResult<()> {
    if let Some(w) = app.get_window(READER_WINDOW) {
        w.close()?;
    }
    Ok(())
}

/// ツールバーが読み込まれた直後に最新の状態を取る。
#[tauri::command]
pub fn reader_state(app: AppHandle, _state: State<'_, AppState>) -> AppResult<ReaderState> {
    Ok(current_state(&content_webview(&app)?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_splits_toolbar_and_content() {
        let (bar, content) = reader_layout(1000.0, 800.0);
        assert_eq!(bar, (0.0, 0.0, 1000.0, TOOLBAR_HEIGHT));
        assert_eq!(
            content,
            (0.0, TOOLBAR_HEIGHT, 1000.0, 800.0 - TOOLBAR_HEIGHT)
        );
        let (_, tiny) = reader_layout(300.0, 20.0);
        assert!(tiny.3 >= 0.0, "高さが足りなくても負にしない");
    }

    #[test]
    fn navigation_allows_only_http_and_https() {
        for ok in ["https://e.com/a", "http://e.com/", "HTTPS://E.COM/"] {
            assert!(allow_navigation(&Url::parse(ok).unwrap()), "{ok}");
        }
        for bad in [
            "javascript:alert(1)",
            "file:///etc/passwd",
            "data:text/html,x",
            "tauri://localhost",
            "mailto:a@b.c",
        ] {
            assert!(!allow_navigation(&Url::parse(bad).unwrap()), "{bad}");
        }
    }

    #[test]
    fn capability_never_grants_the_content_webview() {
        let cap: serde_json::Value =
            serde_json::from_str(include_str!("../capabilities/default.json")).unwrap();
        let text = cap.to_string();
        assert!(!text.contains(READER_CONTENT), "記事ページに権限を与えない");
        assert!(
            cap.get("remote").is_none(),
            "リモートの URL に IPC を許さない"
        );
        assert!(
            cap["webviews"]
                .as_array()
                .unwrap()
                .iter()
                .any(|w| w == READER_TOOLBAR)
        );
    }
}
