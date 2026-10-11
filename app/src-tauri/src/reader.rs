use std::net::{IpAddr, Ipv4Addr};

use serde::Serialize;
use tauri::webview::{NewWindowResponse, PageLoadEvent, WebviewBuilder};
use tauri::{
    AppHandle, Emitter, EventTarget, LogicalPosition, LogicalSize, Manager, Runtime, Url, Webview,
    WebviewUrl, Window,
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

/// アプリ自身や手元のネットワークを指すホストか。localhost 系の名前と、
/// ループバック・プライベート・リンクローカル・未指定・マルチキャストの IP を含む。
/// これらはアプリのオリジン (tauri.localhost や開発サーバー) と重なり得るので、
/// 記事ページに読み込ませない (読み込むと IPC が使えてしまう)。
fn is_local_host(url: &Url) -> bool {
    let Some(host) = url.host_str().filter(|h| !h.is_empty()) else {
        return true;
    };
    // IPv6 は "[::1]" の形で返る
    let bare = host.trim_start_matches('[').trim_end_matches(']');
    match bare.parse::<IpAddr>() {
        Err(_) => {
            let d = host.trim_end_matches('.').to_ascii_lowercase();
            d == "localhost" || d.ends_with(".localhost")
        }
        Ok(IpAddr::V4(ip)) => is_local_ipv4(ip),
        Ok(IpAddr::V6(ip)) => {
            if let Some(v4) = ip.to_ipv4_mapped() {
                return is_local_ipv4(v4);
            }
            ip.is_loopback()
                || ip.is_unspecified()
                || ip.is_multicast()
                || ip.is_unicast_link_local()
                || ip.is_unique_local()
        }
    }
}

fn is_local_ipv4(ip: Ipv4Addr) -> bool {
    ip.is_loopback()
        || ip.is_private()
        || ip.is_link_local()
        || ip.is_unspecified()
        || ip.is_multicast()
        || ip.is_broadcast()
}

/// 閲覧ウィンドウの中で移動してよい URL (http/https で、手元のホストではないもの)。
pub fn allow_navigation(url: &Url) -> bool {
    matches!(url.scheme(), "http" | "https") && !is_local_host(url)
}

/// 閲覧ウィンドウで開いてよい URL かを確かめる。
pub fn ensure_reader_url(url: &str) -> AppResult<()> {
    ensure_http_url(url)?;
    let parsed = Url::parse(url).map_err(|e| e.to_string())?;
    if !allow_navigation(&parsed) {
        return Err(format!("このアドレスは閲覧ウィンドウで開けません: {url}").into());
    }
    Ok(())
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

/// 閲覧ウィンドウを開くときにすること。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReaderAction {
    /// 記事ページがあるので移動するだけ。
    Navigate,
    /// 窓はあるが記事ページが無い (前回の作成が途中で失敗した)。壊して作り直す。
    Rebuild,
    /// 何も無いので作る。
    Create,
}

pub fn reader_action(has_content: bool, has_window: bool) -> ReaderAction {
    match (has_content, has_window) {
        (true, _) => ReaderAction::Navigate,
        (false, true) => ReaderAction::Rebuild,
        (false, false) => ReaderAction::Create,
    }
}

/// 記事を閲覧ウィンドウで開く。窓が無ければ作り、あれば記事ページを移動して前面に出す。
pub fn open_in_reader<R: Runtime>(app: &AppHandle<R>, url: &str) -> AppResult<()> {
    ensure_reader_url(url)?;
    let target = Url::parse(url).map_err(|e| e.to_string())?;
    let state = app.state::<AppState>();
    // 2つの最初の open が同時に窓を作らないよう、確認から作成までを1つずつにする。
    // この中に .await は無い。
    let _guard = state.reader_lock.lock().unwrap_or_else(|e| e.into_inner());
    let content = app.get_webview(READER_CONTENT);
    let window = app.get_window(READER_WINDOW);
    match reader_action(content.is_some(), window.is_some()) {
        ReaderAction::Navigate => {
            if let Some(content) = content {
                set_title(&content, String::new());
                content.navigate(target)?;
            }
            if let Some(w) = window {
                let _ = w.unminimize();
                let _ = w.show();
                let _ = w.set_focus();
            }
            Ok(())
        }
        ReaderAction::Rebuild => {
            if let Some(w) = window {
                w.destroy()?;
            }
            create_reader(app, target)
        }
        ReaderAction::Create => create_reader(app, target),
    }
}

/// 閲覧ウィンドウとその中の2つの WebView を作る。途中で失敗したら窓を壊して残さない。
fn create_reader<R: Runtime>(app: &AppHandle<R>, target: Url) -> AppResult<()> {
    if let Ok(mut t) = app.state::<AppState>().reader_title.lock() {
        t.clear();
    }
    let window = tauri::window::WindowBuilder::new(app, READER_WINDOW)
        .title("閲覧")
        .inner_size(1100.0, 820.0)
        .min_inner_size(640.0, 400.0)
        .build()?;
    if let Err(e) = add_reader_children(app, &window, target) {
        let _ = window.destroy();
        return Err(e);
    }
    Ok(())
}

fn add_reader_children<R: Runtime>(
    app: &AppHandle<R>,
    window: &Window<R>,
    target: Url,
) -> AppResult<()> {
    let size = logical_size(window)?;
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
        .on_page_load(|webview, payload| {
            // 前のページの題名を残さない
            if let PageLoadEvent::Started = payload.event() {
                set_title(&webview, String::new());
            }
            emit_state(&webview);
        })
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
        if matches!(
            e,
            tauri::WindowEvent::Resized(_) | tauri::WindowEvent::ScaleFactorChanged { .. }
        ) {
            relayout(&app2, &window2);
        }
    });
    Ok(())
}

fn content_webview<R: Runtime>(app: &AppHandle<R>) -> AppResult<Webview<R>> {
    Ok(app
        .get_webview(READER_CONTENT)
        .ok_or_else(|| "閲覧ウィンドウが開いていません".to_string())?)
}

/// フロントエンドから呼ぶ `open_in_reader` コマンド。関数名の衝突を避けるため別モジュールに置く。
pub mod command {
    use super::*;

    /// 窓を作るので async にする (同期コマンドはメインスレッドで動き、Windows で固まる)。
    #[tauri::command]
    pub async fn open_in_reader(app: AppHandle, url: String) -> AppResult<()> {
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
        // close() と違い、すぐ続く open_in_reader の前に確実に消える
        w.destroy()?;
    }
    Ok(())
}

/// ツールバーが読み込まれた直後に最新の状態を取る。
#[tauri::command]
pub fn reader_state(app: AppHandle) -> AppResult<ReaderState> {
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
    fn reader_action_navigates_rebuilds_or_creates() {
        assert_eq!(reader_action(true, true), ReaderAction::Navigate);
        assert_eq!(reader_action(false, true), ReaderAction::Rebuild);
        assert_eq!(reader_action(false, false), ReaderAction::Create);
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

    const LOCAL_URLS: [&str; 12] = [
        "http://tauri.localhost/",
        "https://tauri.localhost/x",
        "http://localhost:1420/",
        "http://LOCALHOST./",
        "http://127.0.0.1:8080/",
        "http://[::1]/",
        "http://10.0.0.1/",
        "http://192.168.1.2/",
        "http://169.254.169.254/",
        "http://0.0.0.0/",
        "http://[::ffff:127.0.0.1]/",
        "http://[fe80::1]/",
    ];

    #[test]
    fn navigation_refuses_local_and_private_hosts() {
        for bad in LOCAL_URLS {
            assert!(!allow_navigation(&Url::parse(bad).unwrap()), "{bad}");
        }
        assert!(allow_navigation(
            &Url::parse("https://news.example.com/a").unwrap()
        ));
        assert!(allow_navigation(&Url::parse("http://8.8.8.8/").unwrap()));
    }

    #[test]
    fn open_in_reader_refuses_local_and_private_hosts() {
        for bad in LOCAL_URLS {
            let err = ensure_reader_url(bad).unwrap_err().to_string();
            assert!(err.contains("閲覧ウィンドウで開けません"), "{bad}: {err}");
        }
        assert!(ensure_reader_url("https://news.example.com/a").is_ok());
        assert!(ensure_reader_url("javascript:alert(1)").is_err());
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
