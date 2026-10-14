pub mod keychain;
pub mod oauth;

use std::sync::Arc;
use std::sync::OnceLock;
use std::sync::atomic::Ordering;

use chrono::{DateTime, Utc};
use infodesk_core::sync::drive::{DriveError, DriveStore};
use infodesk_core::sync::engine::{SyncError, SyncInput, compaction_due, run_sync};
use infodesk_core::sync::google::GoogleDrive;
use serde::Serialize;

use crate::error::AppResult;
use crate::state::{AppState, EVENT_ITEMS, EVENT_SYNC};
use crate::tailnet::TailnetState;
use infodesk_core::sync::webdav::{WebDavConfig, WebDavStore};

pub const SETTING_ENABLED: &str = "sync.enabled";
pub const SETTING_DEVICE_NAME: &str = "sync.device_name";
pub const SETTING_LAST_SYNC_AT: &str = "sync.last_sync_at";
pub const SETTING_LAST_COMPACTION_AT: &str = "sync.last_compaction_at";
pub const SETTING_BACKEND: &str = "sync.backend";
pub const SETTING_NAS_URL: &str = "sync.nas.url";
pub const SETTING_NAS_USER: &str = "sync.nas.user";
/// パスワードそのものはキーチェーンにあり、ここには入れたかどうかだけを持つ。
pub const SETTING_NAS_PASSWORD_SET: &str = "sync.nas.password_set";
/// 開発中の DB に残っている古いキー。読んだときに新しいキーへ移す。
const LEGACY_NAS_KEYS: [(&str, &str); 3] = [
    (SETTING_NAS_URL, "sync.nas_url"),
    (SETTING_NAS_USER, "sync.nas_user"),
    (SETTING_NAS_PASSWORD_SET, "sync.nas_password_set"),
];
/// 保存先が変わった。次の同期の始めに、向こうのファイルの記録を捨てて最初から書き出す。
pub const SETTING_RESET_PENDING: &str = "sync.reset_pending";
/// 既定。`nas` 以外の値 (古い `google` も) はすべてこれとして読む。
pub const BACKEND_GDRIVE: &str = "gdrive";
pub const BACKEND_NAS: &str = "nas";
pub const NAS_URL_MESSAGE: &str =
    "NAS の URL を確かめてください (例: http://takeuchi-nas:5005/infodesk/)";
/// WebDAV クライアントを作れなかった。詳細 (プロキシの資格情報を含みうる) は出さない。
pub const NAS_PREPARE_MESSAGE: &str = "NAS への接続を準備できません";
pub const TAILNET_LOGIN_MESSAGE: &str = "Tailscale にログインしてください";
pub const TAILNET_APPROVAL_MESSAGE: &str = "Tailscale の管理画面でこの端末を承認してください";
pub const NAS_INCOMPLETE_MESSAGE: &str = "NAS の URL、ユーザー名、パスワードを入力してください";
pub const NAS_CHECK_OK: &str = "読み書きできました";
pub const FALLBACK_DEVICE_NAME: &str = "この Mac";
pub const REAUTH_MESSAGE: &str = "Google の再ログインが必要です";
/// 通信できなかった失敗 (`DriveError::Retryable`) に出す文。詳細は画面に出さない。
pub const NETWORK_MESSAGE: &str = "ネットワークに接続できません。次の同期で再試行します。";

/// 同期の状態。収集やダイジェストの `last_error` とは別に持つ。
#[derive(Debug, Clone, Default, Serialize)]
pub struct SyncStatus {
    /// このビルドに OAuth の設定が入っているか。
    pub available: bool,
    pub signed_in: bool,
    pub account_email: Option<String>,
    pub enabled: bool,
    pub syncing: bool,
    pub last_sync_at: Option<DateTime<Utc>>,
    pub error: Option<String>,
    pub device_name: String,
    /// "gdrive" か "nas"。
    pub backend: String,
    pub nas_url: String,
    pub nas_user: String,
    pub nas_password_set: bool,
    pub tailnet: TailnetStatusView,
}

/// 画面に出す Tailscale の状態。ログイン URL は含めない (ブラウザーで開く)。
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct TailnetStatusView {
    /// stopped | needs_login | needs_approval | starting | running | failed
    pub state: String,
    pub name: Option<String>,
    /// `failed` のときの理由。
    pub reason: Option<String>,
}

fn tailnet_view(state: &TailnetState) -> TailnetStatusView {
    let (name_, name, reason) = match state {
        TailnetState::Stopped => ("stopped", None, None),
        TailnetState::NeedsLogin { .. } => ("needs_login", None, None),
        TailnetState::NeedsApproval => ("needs_approval", None, None),
        TailnetState::Starting => ("starting", None, None),
        TailnetState::Running { name, .. } => ("running", Some(name.clone()), None),
        TailnetState::Failed { reason } => ("failed", None, Some(reason.clone())),
    };
    TailnetStatusView {
        state: name_.to_string(),
        name,
        reason,
    }
}

/// ホスト名の候補から端末名を選ぶ。空なら既定の「この Mac」。
pub fn pick_device_name(candidate: Option<String>) -> String {
    candidate
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| FALLBACK_DEVICE_NAME.to_string())
}

fn command_output(program: &str, args: &[&str]) -> Option<String> {
    let out = std::process::Command::new(program)
        .args(args)
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// macOS のコンピューター名。取れなければホスト名、それも無理なら「この Mac」。
/// 外部コマンドを呼ぶので、結果は1回だけ調べて覚える。
pub fn default_device_name() -> String {
    static NAME: OnceLock<String> = OnceLock::new();
    NAME.get_or_init(|| {
        pick_device_name(
            command_output("scutil", &["--get", "ComputerName"])
                .filter(|s| !s.is_empty())
                .or_else(|| command_output("hostname", &[])),
        )
    })
    .clone()
}

fn parse_time(v: Option<String>) -> Option<DateTime<Utc>> {
    v.and_then(|v| DateTime::parse_from_rfc3339(&v).ok())
        .map(|d| d.with_timezone(&Utc))
}

/// 設定から読める項目を `SyncStatus` に反映する。`signed_in`、`syncing`、`error` は触らない。
pub fn refresh_status(state: &AppState) {
    let read = state.with_store(|s| {
        Ok((
            s.get_setting(SETTING_ENABLED)?,
            s.get_setting(oauth::SETTING_ACCOUNT_EMAIL)?,
            s.get_setting(SETTING_DEVICE_NAME)?,
            s.get_setting(SETTING_LAST_SYNC_AT)?,
            read_nas(s)?,
        ))
    });
    let (enabled, email, name, last, nas) = match read {
        Ok(v) => v,
        Err(e) => {
            let mut st = state.sync.lock().unwrap_or_else(|e| e.into_inner());
            st.available = oauth::oauth_config().is_some();
            st.error = Some(e.message);
            return;
        }
    };
    let name = pick_device_name(name).to_string();
    let name = if name == FALLBACK_DEVICE_NAME {
        default_device_name()
    } else {
        name
    };
    let mut st = state.sync.lock().unwrap_or_else(|e| e.into_inner());
    st.available = oauth::oauth_config().is_some();
    st.enabled = enabled.as_deref() == Some("true");
    st.account_email = email.filter(|e| !e.is_empty());
    st.device_name = name;
    st.last_sync_at = parse_time(last);
    let tailnet = state.tailnet.state();
    st.backend = nas.backend.clone();
    st.nas_url = nas.url.clone();
    st.nas_user = nas.user.clone();
    st.nas_password_set = nas.password_set;
    let was_running = st.tailnet.state == "running";
    st.tailnet = tailnet_view(&tailnet);
    let running = matches!(tailnet, TailnetState::Running { .. });
    if nas.is_nas() {
        st.signed_in = nas.complete() && running;
    }
    // 前に写した状態と比べ、Tailscale がつながった時 (起動時もログイン後も) に1回だけ同期を頼む。
    let connected = !was_running && running && nas.is_nas() && st.enabled;
    drop(st);
    if connected {
        state.request_sync();
    }
}

/// Tailscale の端末が状態を知らせてきた。状態を写し、つながったなら同期を頼む。
pub fn on_tailnet_event(state: &AppState) {
    refresh_status(state);
}

/// NAS まわりの設定。
struct NasSettings {
    backend: String,
    url: String,
    user: String,
    password_set: bool,
}

impl NasSettings {
    fn is_nas(&self) -> bool {
        self.backend == BACKEND_NAS
    }

    fn complete(&self) -> bool {
        !self.url.is_empty() && !self.user.is_empty() && self.password_set
    }
}

/// 古いキーにだけ値があれば、新しいキーへ移す。
fn migrate_nas_keys(s: &infodesk_core::Store) -> infodesk_core::Result<()> {
    for (key, old) in LEGACY_NAS_KEYS {
        if s.get_setting(key)?.is_none()
            && let Some(v) = s.get_setting(old)?
        {
            s.set_setting(key, &v)?;
            s.delete_setting(old)?;
        }
    }
    Ok(())
}

fn read_nas(s: &infodesk_core::Store) -> infodesk_core::Result<NasSettings> {
    migrate_nas_keys(s)?;
    let text = |k: &str| -> infodesk_core::Result<String> {
        Ok(s.get_setting(k)?.unwrap_or_default().trim().to_string())
    };
    let backend = text(SETTING_BACKEND)?;
    Ok(NasSettings {
        backend: if backend == BACKEND_NAS {
            backend
        } else {
            BACKEND_GDRIVE.to_string()
        },
        url: text(SETTING_NAS_URL)?,
        user: text(SETTING_NAS_USER)?,
        password_set: text(SETTING_NAS_PASSWORD_SET)? == "true",
    })
}

/// 起動時に1回呼ぶ。サインイン済みかは、保存したメールアドレスの有無で決める。
pub fn init_status(state: &AppState) {
    refresh_status(state);
    let mut st = state.sync.lock().unwrap_or_else(|e| e.into_inner());
    if st.backend != BACKEND_NAS {
        st.signed_in = st.available && st.account_email.is_some();
    }
}

pub fn status_now(state: &AppState) -> SyncStatus {
    refresh_status(state);
    state.sync.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

fn update(state: &AppState, f: impl FnOnce(&mut SyncStatus)) {
    {
        let mut st = state.sync.lock().unwrap_or_else(|e| e.into_inner());
        f(&mut st);
    }
    state.notify(EVENT_SYNC);
}

/// 本物の Google ドライブクライアントを作る。キーチェーンを読むので、頻繁には呼ばない。
pub fn google_drive() -> AppResult<Arc<dyn DriveStore>> {
    Ok(Arc::new(GoogleDrive::new(Arc::new(oauth::token_source()?))))
}

#[cfg(test)]
fn make_drive(state: &AppState, generation: u64) -> AppResult<Arc<dyn DriveStore>> {
    let nas = state.with_store(read_nas)?;
    make_drive_for(state, &nas, generation)
}

/// 作ったクライアントを覚えておく。サインインやサインアウトで捨てる。
fn make_drive_for(
    state: &AppState,
    nas: &NasSettings,
    generation: u64,
) -> AppResult<Arc<dyn DriveStore>> {
    if nas.is_nas() {
        // プロキシの口は Tailscale を起動し直すと変わるので、NAS の保存先は覚えない。
        return nas_drive(state, nas);
    }
    let mut cache = state.drive_cache.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(d) = cache.as_ref() {
        return Ok(d.clone());
    }
    let drive = (state.drive_factory)()?;
    // 作っている間にサインインし直されていたら、古い認証のものは覚えない。
    if state.sync_generation.load(Ordering::SeqCst) == generation {
        *cache = Some(drive.clone());
    }
    Ok(drive)
}

/// NAS の保存先を作る。Tailscale が Running でなければ、直接つながずに止める。
fn nas_drive(state: &AppState, nas: &NasSettings) -> AppResult<Arc<dyn DriveStore>> {
    let proxy = match state.tailnet.state() {
        TailnetState::Running { .. } => state.tailnet.proxy_url(),
        TailnetState::NeedsApproval => return Err(TAILNET_APPROVAL_MESSAGE.to_string().into()),
        _ => None,
    };
    let Some(proxy) = proxy else {
        return Err(TAILNET_LOGIN_MESSAGE.to_string().into());
    };
    if !nas.complete() {
        return Err(NAS_INCOMPLETE_MESSAGE.to_string().into());
    }
    let password = state
        .nas_secrets
        .get()
        .map_err(|_| "NAS のパスワードを読めません".to_string())?
        .filter(|p| !p.is_empty());
    let Some(password) = password else {
        // キーチェーンから消えていた。画面の「入力済み」も戻す。
        let _ = state.with_store(|s| s.set_setting(SETTING_NAS_PASSWORD_SET, "false"));
        return Err(NAS_INCOMPLETE_MESSAGE.to_string().into());
    };
    (state.nas_drive_factory)(WebDavConfig {
        url: nas.url.clone(),
        user: nas.user.clone(),
        password,
        proxy: Some(proxy),
    })
}

/// 本物の WebDAV クライアントを作る。
/// 作れなかった理由は出さない (プロキシの URL には資格情報が入っている)。
pub fn webdav_drive(cfg: WebDavConfig) -> AppResult<Arc<dyn DriveStore>> {
    match WebDavStore::new(cfg) {
        Ok(d) => Ok(Arc::new(d)),
        Err(_) => Err(NAS_PREPARE_MESSAGE.to_string().into()),
    }
}

/// 保存先のエラーを画面に出す文にする。通信の失敗は詳細を出さない。
fn drive_message(e: &DriveError) -> String {
    match e {
        DriveError::Retryable(_) => NETWORK_MESSAGE.to_string(),
        other => other.to_string(),
    }
}

/// 認証が変わったので、覚えているクライアントと、実行中の同期の結果を捨てる。
fn reset_auth(state: &AppState) {
    state.sync_generation.fetch_add(1, Ordering::SeqCst);
    *state.drive_cache.lock().unwrap_or_else(|e| e.into_inner()) = None;
}

/// ログイン完了を反映する。エラー表示は消す。
pub fn signed_in(state: &AppState, email: &str) {
    reset_auth(state);
    update(state, |s| {
        s.signed_in = true;
        s.account_email = Some(email.to_string()).filter(|e| !e.is_empty());
        s.error = None;
    });
}

pub fn signed_out(state: &AppState) {
    reset_auth(state);
    update(state, |s| {
        s.signed_in = false;
        s.account_email = None;
        s.error = None;
    });
}

/// 有効にするときは、無効の間に残った未送信の操作を捨て、次の同期でスナップショットを作り直させる。
/// 印の状態は記事の `*_changed_at` が持っているので、そのスナップショットで届く。
pub fn set_enabled(state: &AppState, enabled: bool) -> AppResult<()> {
    state.with_store(|s| {
        if enabled {
            s.discard_pending_ops()?;
            s.set_setting(SETTING_LAST_COMPACTION_AT, "")?;
        }
        s.set_setting(SETTING_ENABLED, if enabled { "true" } else { "false" })
    })?;
    update(state, |s| {
        s.enabled = enabled;
        if !enabled {
            s.error = None;
        }
    });
    Ok(())
}

/// 保存先を切り替える。向こうのファイルの記録は、次の同期の始め (実行中の印を持った状態) で捨てる。
/// ここで捨てると、実行中の同期が前の保存先の記録を書き戻してしまう。同じ保存先なら何もしない。
/// 切り替えたら `true`。
pub fn set_backend(state: &AppState, backend: &str) -> AppResult<bool> {
    if backend != BACKEND_GDRIVE && backend != BACKEND_NAS {
        return Err(format!("保存先を選べません: {backend}").into());
    }
    let changed = state.with_store(|s| {
        if read_nas(s)?.backend == backend {
            return Ok(false);
        }
        s.set_setting(SETTING_BACKEND, backend)?;
        s.set_setting(SETTING_RESET_PENDING, "true")?;
        Ok(true)
    })?;
    if !changed {
        return Ok(false);
    }
    // 実行中の同期は古い保存先の結果になる。記録を残させない。
    reset_auth(state);
    state.nas_rejected.store(false, Ordering::SeqCst);
    update(state, |s| s.error = None);
    refresh_status(state);
    if backend == BACKEND_GDRIVE {
        let mut st = state.sync.lock().unwrap_or_else(|e| e.into_inner());
        st.signed_in = st.available && st.account_email.is_some();
    }
    state.notify(EVENT_SYNC);
    Ok(true)
}

/// 最初からやり直す予約があるか。
#[cfg(test)]
pub fn reset_pending(state: &AppState) -> bool {
    state
        .with_store(|s| s.get_setting(SETTING_RESET_PENDING))
        .ok()
        .flatten()
        .as_deref()
        == Some("true")
}

/// 予約があれば、向こうのファイルの記録を捨て、まとめ直しを求める。捨てたら `true`。
/// 途中で失敗しても予約は最後に消すので、次の同期でやり直せる。
fn take_reset(s: &infodesk_core::Store) -> infodesk_core::Result<bool> {
    if s.get_setting(SETTING_RESET_PENDING)?.as_deref() != Some("true") {
        return Ok(false);
    }
    s.clear_sync_files()?;
    s.set_setting(SETTING_LAST_COMPACTION_AT, "")?;
    s.set_setting(SETTING_RESET_PENDING, "")?;
    Ok(true)
}

/// NAS の URL として使えるか。http か https で、ホストがあり、URL に資格情報を含まない。
fn check_nas_url(url: &str) -> AppResult<()> {
    let ok = url::Url::parse(url).is_ok_and(|u| {
        matches!(u.scheme(), "http" | "https")
            && u.host_str().is_some_and(|h| !h.is_empty())
            && u.username().is_empty()
            && u.password().is_none()
    });
    if ok {
        Ok(())
    } else {
        Err(NAS_URL_MESSAGE.to_string().into())
    }
}

/// 値を戻す。もとが無ければ消す。
fn restore_setting(
    s: &infodesk_core::Store,
    key: &str,
    value: &Option<String>,
) -> infodesk_core::Result<()> {
    match value {
        Some(v) => s.set_setting(key, v),
        None => s.delete_setting(key),
    }
}

/// NAS の URL、ユーザー名、パスワードを保存する。`password` が `None` なら今のパスワードのまま、
/// 空なら消す。資格情報の拒否で止めていた同期は、ここで再開する。
/// NAS を選んでいて URL が変わったら、次の同期で最初から書き出す。
/// 設定を先に書き、キーチェーンに書けなければ設定を元に戻す。
pub fn set_nas(state: &AppState, url: &str, user: &str, password: Option<&str>) -> AppResult<()> {
    let (url, user) = (url.trim(), user.trim());
    check_nas_url(url)?;
    let keys = [
        SETTING_NAS_URL,
        SETTING_NAS_USER,
        SETTING_NAS_PASSWORD_SET,
        SETTING_RESET_PENDING,
    ];
    let before = state.with_store(|s| {
        let nas = read_nas(s)?;
        let before = keys
            .iter()
            .map(|k| s.get_setting(k))
            .collect::<infodesk_core::Result<Vec<_>>>()?;
        s.set_setting(SETTING_NAS_URL, url)?;
        s.set_setting(SETTING_NAS_USER, user)?;
        match password {
            Some("") => s.set_setting(SETTING_NAS_PASSWORD_SET, "false")?,
            Some(_) => s.set_setting(SETTING_NAS_PASSWORD_SET, "true")?,
            None => {}
        }
        if nas.is_nas() && nas.url != url {
            s.set_setting(SETTING_RESET_PENDING, "true")?;
        }
        Ok(before)
    })?;
    let saved = match password {
        Some("") => state.nas_secrets.delete(),
        Some(p) => state.nas_secrets.set(p),
        None => Ok(()),
    };
    if let Err(e) = saved {
        let _ = state.with_store(|s| {
            for (k, v) in keys.iter().zip(&before) {
                restore_setting(s, k, v)?;
            }
            Ok(())
        });
        return Err(neutral_keychain_error(e));
    }
    reset_auth(state);
    state.nas_rejected.store(false, Ordering::SeqCst);
    update(state, |s| s.error = None);
    refresh_status(state);
    Ok(())
}

fn neutral_keychain_error(_: String) -> crate::error::AppError {
    "NAS のパスワードをキーチェーンに保存できません"
        .to_string()
        .into()
}

/// NAS に一時ファイルを書き、読み、消して確かめる。同期が無効でも使える。結果は画面に出す文。
pub async fn test_nas(state: &AppState) -> String {
    let nas = match state.with_store(read_nas) {
        Ok(n) => n,
        Err(e) => return e.message,
    };
    let drive = match nas_drive(state, &nas) {
        Ok(d) => d,
        Err(e) => return e.message,
    };
    let name = format!("infodesk-check-{}.txt", uuid::Uuid::new_v4());
    let body = b"infodesk".to_vec();
    let written = match drive.create(&name, body.clone(), "text/plain").await {
        Ok(f) => f,
        Err(e) => return drive_message(&e),
    };
    let read = drive.download(&written.id).await;
    let removed = drive.delete(&written.id).await;
    match (read, removed) {
        (Err(e), _) | (_, Err(e)) => drive_message(&e),
        (Ok(got), Ok(())) if got == body => {
            // 読み書きできたので、資格情報の拒否で止めていた同期を再開する。
            if state.nas_rejected.swap(false, Ordering::SeqCst) {
                let rejected = DriveError::BadCredentials.to_string();
                update(state, |s| {
                    if s.error.as_deref() == Some(rejected.as_str()) {
                        s.error = None;
                    }
                });
            }
            NAS_CHECK_OK.to_string()
        }
        (Ok(_), Ok(())) => "NAS から読んだ内容が書いた内容と違います".to_string(),
    }
}

/// 起動時に Tailscale を始めるか。NAS を選び、同期が有効なとき。
pub fn should_start_tailnet(state: &AppState) -> bool {
    state
        .with_store(|s| Ok((read_nas(s)?, s.get_setting(SETTING_ENABLED)?)))
        .map(|(nas, enabled)| nas.is_nas() && enabled.as_deref() == Some("true"))
        .unwrap_or(false)
}

/// 条件に合えば Tailscale を別スレッドで始める (start は待たされることがある)。
pub fn start_tailnet_if_needed(state: &AppState) {
    if should_start_tailnet(state) {
        let tailnet = state.tailnet.clone();
        tauri::async_runtime::spawn_blocking(move || {
            if let Err(e) = tailnet.start() {
                eprintln!("tailnet: start failed: {}", e.message);
            }
        });
    }
}

/// NAS を選んでいるか。
pub fn backend_is_nas(state: &AppState) -> bool {
    state
        .with_store(read_nas)
        .map(|n| n.is_nas())
        .unwrap_or(false)
}

/// Tailscale を別スレッドで止める (ロックや通信で待たされうるので、呼び出し元を待たせない)。
pub fn stop_tailnet(state: &AppState) -> tauri::async_runtime::JoinHandle<()> {
    let tailnet = state.tailnet.clone();
    tauri::async_runtime::spawn_blocking(move || tailnet.stop())
}

/// Tailscale の状態が変わった (ログアウトなど)。覚えている接続を捨てて、画面に知らせる。
pub fn tailnet_changed(state: &AppState) {
    reset_auth(state);
    refresh_status(state);
    state.notify(EVENT_SYNC);
}

/// 端末名を保存する。空にすると既定 (ホスト名) に戻す。
pub fn set_device_name(state: &AppState, name: &str) -> AppResult<()> {
    let name = name.trim();
    state.with_store(|s| s.set_setting(SETTING_DEVICE_NAME, name))?;
    refresh_status(state);
    state.notify(EVENT_SYNC);
    Ok(())
}

/// 実行中の印。破棄すると `syncing` と実行中フラグを戻す (パニックでも戻る)。
struct RunGuard<'a>(&'a AppState);

impl<'a> RunGuard<'a> {
    fn begin(state: &'a AppState) -> Self {
        update(state, |s| s.syncing = true);
        Self(state)
    }
}

impl Drop for RunGuard<'_> {
    fn drop(&mut self) {
        // 先に状態を戻す。実行中フラグを先に戻すと、次の同期の `syncing` を上書きしうる。
        update(self.0, |s| s.syncing = false);
        self.0.sync_running.store(false, Ordering::SeqCst);
    }
}

/// 同期を投げて待たない。失敗は `sync_once` が `SyncStatus.error` に記録済み。
pub fn spawn_sync(app: &tauri::AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        use tauri::Manager;
        if let Err(e) = sync_once(&app.state::<AppState>()).await {
            eprintln!("sync failed: {}", e.message);
        }
    });
}

fn record_error(state: &AppState, message: &str) {
    update(state, |s| s.error = Some(message.to_string()));
}

/// 同期を1回行う。無効、未ログインならすぐ戻る。実行中なら、終わったあとにもう1回走らせる依頼を残す
/// (何度頼まれても1回に束ねる)。失敗は `SyncStatus.error` にだけ記録する。
pub async fn sync_once(state: &AppState) -> AppResult<()> {
    loop {
        let read = state.with_store(|s| Ok((s.get_setting(SETTING_ENABLED)?, read_nas(s)?)));
        let (enabled, nas) = match read {
            Ok((v, nas)) => (v.as_deref() == Some("true"), nas),
            Err(e) => {
                record_error(state, &e.message);
                return Err(e);
            }
        };
        let ready = if nas.is_nas() {
            nas.complete()
                && matches!(state.tailnet.state(), TailnetState::Running { .. })
                && !state.nas_rejected.load(Ordering::SeqCst)
        } else {
            state
                .sync
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .signed_in
        };
        if !enabled || !ready {
            return Ok(());
        }
        if state
            .sync_running
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            state.sync_rerun.store(true, Ordering::SeqCst);
            return Ok(());
        }
        let result = {
            let _guard = RunGuard::begin(state);
            run_once(state).await
        };
        if !state.sync_rerun.swap(false, Ordering::SeqCst) {
            return result;
        }
    }
}

async fn run_once(state: &AppState) -> AppResult<()> {
    let generation = state.sync_generation.load(Ordering::SeqCst);
    let stale = || state.sync_generation.load(Ordering::SeqCst) != generation;
    // 保存先の切り替えのやり直しは、実行中の印を持ったここで行う。前の同期が記録を書き戻すことはない。
    // 保存先も同じロックの中で読み、やり直しと食い違わないようにする。
    let nas = match state.with_store(|s| Ok((take_reset(s)?, read_nas(s)?))) {
        Ok((reset, nas)) => {
            if reset {
                *state.drive_cache.lock().unwrap_or_else(|e| e.into_inner()) = None;
            }
            nas
        }
        Err(e) => {
            record_error(state, &e.message);
            return Err(e);
        }
    };
    let drive = match make_drive_for(state, &nas, generation) {
        Ok(d) => d,
        Err(e) => {
            if !stale() {
                record_error(state, &e.message);
            }
            return Ok(());
        }
    };
    if stale() {
        return Ok(());
    }
    let now = Utc::now();
    let prepared = state.with_store(|s| {
        let (d, b) = s.ensure_sync_ids()?;
        Ok((
            d,
            b,
            s.get_setting(SETTING_LAST_COMPACTION_AT)?,
            s.get_setting("schedule.daily_time")?,
        ))
    });
    let (device_id, db_id, last_compaction, daily) = match prepared {
        Ok(v) => v,
        Err(e) => {
            record_error(state, &e.message);
            return Err(e);
        }
    };
    let daily_time = crate::scheduler::ScheduleConfig::from_settings(|k| {
        if k == "schedule.daily_time" {
            daily.clone()
        } else {
            None
        }
    })
    .daily_time;
    // まだ一度もまとめていなければ、時刻を待たずにまとめて、今の印を届ける。
    let last_compaction = parse_time(last_compaction);
    let compact =
        last_compaction.is_none() || compaction_due(last_compaction, now, state.tz, daily_time);
    let writer = state
        .sync
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .device_name
        .clone();
    let input = SyncInput {
        device_id,
        db_id,
        writer,
        now,
        tz: state.tz,
        compact,
    };
    match run_sync(&state.store, drive.as_ref(), &input).await {
        Ok(report) => {
            let stamp = now.to_rfc3339();
            // 途中で保存先が変わった (やり直しの予約がある) か、認証が変わった同期は古い。
            // 時刻を残すと、次の同期のまとめ直しを飛ばしてしまう。予約の確認と書き込みは同じロックの中。
            let recorded = state.with_store(|s| {
                if stale() || s.get_setting(SETTING_RESET_PENDING)?.as_deref() == Some("true") {
                    return Ok(false);
                }
                s.set_setting(SETTING_LAST_SYNC_AT, &stamp)?;
                if report.compacted {
                    s.set_setting(SETTING_LAST_COMPACTION_AT, &stamp)?;
                }
                Ok(true)
            });
            if recorded.unwrap_or(false) {
                update(state, |s| {
                    s.last_sync_at = Some(now);
                    s.error = None;
                });
            }
            if report.applied_ops > 0 {
                state.notify(EVENT_ITEMS);
            }
        }
        Err(err) => {
            // 取り込みのあとで失敗しても、取り込み済みの変更は画面に出す必要がある。
            state.notify(EVENT_ITEMS);
            {
                // 古さの確認と書き換えを同じロックの中で行い、間に入ったサインインを消さない。
                let mut st = state.sync.lock().unwrap_or_else(|e| e.into_inner());
                if stale() {
                    // 実行中にサインインし直された。古い認証の結果は捨てる。
                    return Ok(());
                }
                match err {
                    SyncError::Drive(DriveError::AuthExpired) => {
                        reset_auth(state);
                        let _ =
                            state.with_store(|s| s.set_setting(oauth::SETTING_ACCOUNT_EMAIL, ""));
                        st.signed_in = false;
                        st.account_email = None;
                        st.error = Some(REAUTH_MESSAGE.to_string());
                    }
                    SyncError::Drive(DriveError::BadCredentials) => {
                        // 設定が変わるまで再試行しない。signed_in は変えない。
                        state.nas_rejected.store(true, Ordering::SeqCst);
                        st.error = Some(DriveError::BadCredentials.to_string());
                    }
                    SyncError::Drive(DriveError::Retryable(_)) => {
                        st.error = Some(NETWORK_MESSAGE.to_string());
                    }
                    other => st.error = Some(other.to_string()),
                }
            }
            state.notify(EVENT_SYNC);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{EVENT_STATUS, test_state};
    use infodesk_core::sync::drive::{MemoryDrive, RemoteFile};
    use std::sync::Mutex;
    use std::sync::atomic::AtomicUsize;

    /// 差し込んだ Drive を使うようにして、同期を有効・ログイン済みにする。作った回数を返す。
    fn use_drive(state: &mut AppState, drive: Arc<dyn DriveStore>) -> Arc<AtomicUsize> {
        let made = Arc::new(AtomicUsize::new(0));
        let counter = made.clone();
        state.drive_factory = Arc::new(move || {
            counter.fetch_add(1, Ordering::SeqCst);
            Ok(drive.clone())
        });
        state
            .with_store(|s| s.set_setting(SETTING_ENABLED, "true"))
            .unwrap();
        state.sync.lock().unwrap().signed_in = true;
        made
    }

    fn ready(state: &mut AppState) -> Arc<MemoryDrive> {
        let drive = Arc::new(MemoryDrive::new());
        use_drive(state, drive.clone());
        drive
    }

    /// `list` で1回譲る Drive。他の処理が割り込める。呼ばれた回数を数え、仕込んだ失敗を返す。
    #[derive(Default)]
    struct Slow {
        inner: MemoryDrive,
        lists: AtomicUsize,
        fail: Mutex<Option<DriveError>>,
    }

    #[async_trait::async_trait]
    impl DriveStore for Slow {
        async fn list(&self) -> Result<Vec<RemoteFile>, DriveError> {
            self.lists.fetch_add(1, Ordering::SeqCst);
            tokio::task::yield_now().await;
            if let Some(e) = self.fail.lock().unwrap().take() {
                return Err(e);
            }
            self.inner.list().await
        }
        async fn download(&self, id: &str) -> Result<Vec<u8>, DriveError> {
            self.inner.download(id).await
        }
        async fn create(&self, n: &str, b: Vec<u8>, m: &str) -> Result<RemoteFile, DriveError> {
            self.inner.create(n, b, m).await
        }
        async fn update(&self, id: &str, b: Vec<u8>, m: &str) -> Result<RemoteFile, DriveError> {
            self.inner.update(id, b, m).await
        }
        async fn delete(&self, id: &str) -> Result<(), DriveError> {
            self.inner.delete(id).await
        }
    }

    #[tokio::test]
    async fn the_drive_client_is_made_once_and_again_after_auth_changes() {
        let (mut state, _) = test_state();
        let drive = Arc::new(MemoryDrive::new());
        let made = use_drive(&mut state, drive.clone());
        sync_once(&state).await.unwrap();
        sync_once(&state).await.unwrap();
        assert_eq!(made.load(Ordering::SeqCst), 1);
        signed_out(&state);
        signed_in(&state, "a@b.c");
        sync_once(&state).await.unwrap();
        assert_eq!(made.load(Ordering::SeqCst), 2);
        *drive.fail_next.lock().unwrap() = Some(DriveError::AuthExpired);
        sync_once(&state).await.unwrap();
        signed_in(&state, "a@b.c");
        sync_once(&state).await.unwrap();
        assert_eq!(made.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn failure_after_import_still_emits_items_changed() {
        let (mut state, events) = test_state();
        let drive = ready(&mut state);
        phone_stars_an_item(&state, &drive);
        *drive.fail_on_name.lock().unwrap() = Some("meta".into());
        sync_once(&state).await.unwrap();
        assert!(state.sync.lock().unwrap().error.is_some());
        assert!(events.lock().unwrap().contains(&EVENT_ITEMS.to_string()));
    }

    #[tokio::test]
    async fn store_errors_reach_the_sync_error() {
        let (mut state, _) = test_state();
        ready(&mut state);
        let store = &state.store;
        let _ = std::thread::scope(|sc| {
            sc.spawn(|| {
                let _held = store.lock().unwrap();
                panic!("poison");
            })
            .join()
        });
        assert!(sync_once(&state).await.is_err());
        let err = state.sync.lock().unwrap().error.clone();
        assert!(err.unwrap().contains("poisoned"));
        let mut state2 = test_state().0;
        ready(&mut state2);
        let _ = std::thread::scope(|sc| {
            sc.spawn(|| {
                let _held = state2.store.lock().unwrap();
                panic!("poison");
            })
            .join()
        });
        refresh_status(&state2);
        assert!(state2.sync.lock().unwrap().error.is_some());
    }

    #[test]
    fn a_panic_does_not_leave_the_status_syncing() {
        let (state, events) = test_state();
        state.sync_running.store(true, Ordering::SeqCst);
        let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = RunGuard::begin(&state);
            assert!(state.sync.lock().unwrap().syncing);
            panic!("pass blew up");
        }));
        assert!(caught.is_err());
        assert!(!state.sync.lock().unwrap().syncing);
        assert!(!state.sync_running.load(Ordering::SeqCst));
        assert!(events.lock().unwrap().contains(&EVENT_SYNC.to_string()));
    }

    #[tokio::test]
    async fn requests_during_a_pass_queue_exactly_one_more() {
        let (mut state, _) = test_state();
        let drive = Arc::new(Slow::default());
        use_drive(&mut state, drive.clone());
        let (a, b, c) = tokio::join!(sync_once(&state), sync_once(&state), sync_once(&state));
        a.unwrap();
        b.unwrap();
        c.unwrap();
        assert_eq!(drive.lists.load(Ordering::SeqCst), 2);
        assert!(!state.sync_rerun.load(Ordering::SeqCst));
        assert!(!state.sync_running.load(Ordering::SeqCst));
        sync_once(&state).await.unwrap();
        assert_eq!(drive.lists.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn a_pass_that_outlives_a_sign_out_discards_its_auth_error() {
        let (mut state, _) = test_state();
        let drive = Arc::new(Slow::default());
        use_drive(&mut state, drive.clone());
        *drive.fail.lock().unwrap() = Some(DriveError::AuthExpired);
        let relogin = async {
            signed_out(&state);
            signed_in(&state, "new@b.c");
        };
        let (r, ()) = tokio::join!(sync_once(&state), relogin);
        r.unwrap();
        let st = state.sync.lock().unwrap().clone();
        assert!(st.signed_in, "the new sign-in survives");
        assert_eq!(st.error, None);
        assert_eq!(st.account_email.as_deref(), Some("new@b.c"));
    }

    #[test]
    fn device_name_is_looked_up_once() {
        assert_eq!(default_device_name(), default_device_name());
    }

    fn phone_stars_an_item(state: &AppState, drive: &MemoryDrive) -> i64 {
        use infodesk_core::model::{ItemFilter, NewItem};
        use infodesk_core::sync::format::{Field, Op, OpsFile, ops_file_name};
        let id = state
            .with_store(|s| {
                let src = s.list_sources()?.remove(0);
                let item = NewItem {
                    url: "https://e.com/1".into(),
                    title: "t".into(),
                    excerpt: String::new(),
                    author: None,
                    published_at: None,
                    score: None,
                    image_url: None,
                };
                s.upsert_items(&src, &[item], &Utc::now())?;
                Ok(s.query_items(&ItemFilter::default())?[0].id)
            })
            .unwrap();
        let file = OpsFile {
            device: "phone".into(),
            ops: vec![Op {
                item_id: id,
                field: Field::Starred,
                value: true,
                at: "2999-01-01T00:00:00.000Z".into(),
                device: "phone".into(),
            }],
        };
        drive.put(
            &ops_file_name("phone", Utc::now(), 1),
            serde_json::to_vec(&file).unwrap(),
        );
        id
    }

    #[test]
    fn device_name_falls_back() {
        assert_eq!(pick_device_name(None), "この Mac");
        assert_eq!(pick_device_name(Some("  ".into())), "この Mac");
        assert_eq!(pick_device_name(Some(" Studio \n".into())), "Studio");
    }

    #[test]
    fn commands_update_status_and_settings() {
        let (state, events) = test_state();
        set_enabled(&state, true).unwrap();
        assert!(state.sync.lock().unwrap().enabled);
        assert_eq!(
            state
                .with_store(|s| s.get_setting(SETTING_ENABLED))
                .unwrap()
                .as_deref(),
            Some("true")
        );
        set_device_name(&state, "  Studio ").unwrap();
        assert_eq!(status_now(&state).device_name, "Studio");
        signed_in(&state, "a@b.c");
        assert_eq!(
            state.sync.lock().unwrap().account_email.as_deref(),
            Some("a@b.c")
        );
        signed_out(&state);
        assert!(!state.sync.lock().unwrap().signed_in);
        assert!(events.lock().unwrap().contains(&EVENT_SYNC.to_string()));
    }

    #[tokio::test]
    async fn successful_sync_records_time_and_clears_error() {
        let (mut state, events) = test_state();
        let drive = ready(&mut state);
        state.sync.lock().unwrap().error = Some("old".into());
        sync_once(&state).await.unwrap();
        let st = state.sync.lock().unwrap().clone();
        assert!(st.last_sync_at.is_some());
        assert_eq!(st.error, None);
        assert!(!st.syncing);
        assert!(drive.names().contains(&"manifest.json".to_string()));
        let stored = state
            .with_store(|s| s.get_setting(SETTING_LAST_SYNC_AT))
            .unwrap();
        assert!(stored.is_some());
        assert!(events.lock().unwrap().contains(&EVENT_SYNC.to_string()));
    }

    #[tokio::test]
    async fn sync_errors_do_not_touch_last_error() {
        let (mut state, events) = test_state();
        let drive = ready(&mut state);
        *drive.fail_next.lock().unwrap() = Some(DriveError::Other("boom".into()));
        state.update_status(|s| s.set_error(crate::state::ErrorOrigin::Job, "digest failed"));
        events.lock().unwrap().clear();
        sync_once(&state).await.unwrap();
        assert_eq!(state.status().last_error.as_deref(), Some("digest failed"));
        let err = state.sync.lock().unwrap().error.clone().unwrap();
        assert!(err.contains("boom"), "{err}");
        assert!(!events.lock().unwrap().contains(&EVENT_STATUS.to_string()));
    }

    #[tokio::test]
    async fn auth_expired_signs_out_and_says_so() {
        let (mut state, _) = test_state();
        let drive = ready(&mut state);
        state
            .with_store(|s| s.set_setting(oauth::SETTING_ACCOUNT_EMAIL, "a@b.c"))
            .unwrap();
        *drive.fail_next.lock().unwrap() = Some(DriveError::AuthExpired);
        sync_once(&state).await.unwrap();
        let st = state.sync.lock().unwrap().clone();
        assert!(!st.signed_in);
        assert_eq!(st.error.as_deref(), Some("Google の再ログインが必要です"));
        assert_eq!(state.status().last_error, None);
    }

    #[tokio::test]
    async fn sync_is_skipped_when_disabled_or_signed_out() {
        let (mut state, _) = test_state();
        let drive = ready(&mut state);
        *drive.fail_next.lock().unwrap() = Some(DriveError::Other("boom".into()));
        state
            .with_store(|s| s.set_setting(SETTING_ENABLED, "false"))
            .unwrap();
        sync_once(&state).await.unwrap();
        state
            .with_store(|s| s.set_setting(SETTING_ENABLED, "true"))
            .unwrap();
        state.sync.lock().unwrap().signed_in = false;
        sync_once(&state).await.unwrap();
        // どちらも Drive に触れていないので、仕込んだ失敗がそのまま残る。
        assert!(drive.fail_next.lock().unwrap().is_some());
        assert_eq!(state.sync.lock().unwrap().error, None);
        assert_eq!(state.sync.lock().unwrap().last_sync_at, None);
    }

    #[tokio::test]
    async fn a_running_sync_is_not_started_twice() {
        let (mut state, _) = test_state();
        let drive = ready(&mut state);
        *drive.fail_next.lock().unwrap() = Some(DriveError::Other("boom".into()));
        state.sync_running.store(true, Ordering::SeqCst);
        sync_once(&state).await.unwrap();
        assert!(drive.fail_next.lock().unwrap().is_some());
        assert!(
            state.sync_running.load(Ordering::SeqCst),
            "not ours to reset"
        );
        // 終わった後は、次の同期が走る。
        state.sync_running.store(false, Ordering::SeqCst);
        sync_once(&state).await.unwrap();
        assert!(drive.fail_next.lock().unwrap().is_none());
        assert!(!state.sync_running.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn network_errors_show_a_fixed_message_without_detail() {
        let (mut state, _) = test_state();
        let drive = ready(&mut state);
        *drive.fail_next.lock().unwrap() = Some(DriveError::Retryable(
            "error sending request: tcp connect".into(),
        ));
        sync_once(&state).await.unwrap();
        let err = state.sync.lock().unwrap().error.clone();
        assert_eq!(
            err.as_deref(),
            Some("ネットワークに接続できません。次の同期で再試行します。")
        );
    }

    /// 記事を1件足し、同期が無効の間に既読にする。
    fn read_while_sync_is_off(state: &AppState) -> i64 {
        use infodesk_core::model::{ItemFilter, NewItem};
        state
            .with_store(|s| {
                s.set_setting(SETTING_ENABLED, "false")?;
                let src = s.list_sources()?.remove(0);
                let item = NewItem {
                    url: "https://e.com/old".into(),
                    title: "t".into(),
                    excerpt: String::new(),
                    author: None,
                    published_at: None,
                    score: None,
                    image_url: None,
                };
                s.upsert_items(&src, &[item], &Utc::now())?;
                let id = s.query_items(&ItemFilter::default())?[0].id;
                s.set_read(id, true)?;
                Ok(id)
            })
            .unwrap()
    }

    #[tokio::test]
    async fn the_first_sync_publishes_marks_even_before_the_daily_time() {
        use infodesk_core::sync::format::{StateSnapshot, gunzip_json, gzip_json};
        let (mut state, _) = test_state();
        let drive = ready(&mut state);
        let id = read_while_sync_is_off(&state);
        state
            .with_store(|s| {
                s.set_setting(SETTING_ENABLED, "true")?;
                s.set_setting("schedule.daily_time", "23:59")
            })
            .unwrap();
        // Drive にはもう (空の) スナップショットがある。
        let empty = StateSnapshot {
            covers: Vec::new(),
            state: Default::default(),
        };
        drive.put("state-snapshot.json.gz", gzip_json(&empty).unwrap());
        sync_once(&state).await.unwrap();
        let f = drive
            .list()
            .await
            .unwrap()
            .into_iter()
            .find(|f| f.name == "state-snapshot.json.gz")
            .unwrap();
        let snap: StateSnapshot = gunzip_json(&drive.download(&f.id).await.unwrap()).unwrap();
        assert!(snap.state[&id].read.as_ref().expect("published").value);
        let last = state
            .with_store(|s| s.get_setting(SETTING_LAST_COMPACTION_AT))
            .unwrap();
        assert!(parse_time(last).is_some());
    }

    #[test]
    fn enabling_sync_drops_unsent_ops_and_asks_for_a_fresh_snapshot() {
        let (state, _) = test_state();
        let id = read_while_sync_is_off(&state);
        state
            .with_store(|s| {
                s.set_setting(SETTING_ENABLED, "true")?;
                s.set_starred(id, true)?;
                s.set_setting(SETTING_ENABLED, "false")?;
                s.set_setting(SETTING_LAST_COMPACTION_AT, &Utc::now().to_rfc3339())
            })
            .unwrap();
        let (device, _) = state.with_store(|s| s.ensure_sync_ids()).unwrap();
        assert_eq!(
            state
                .with_store(|s| s.pending_local_ops(&device))
                .unwrap()
                .len(),
            1
        );
        set_enabled(&state, true).unwrap();
        assert!(
            state
                .with_store(|s| s.pending_local_ops(&device))
                .unwrap()
                .is_empty()
        );
        let last = state
            .with_store(|s| s.get_setting(SETTING_LAST_COMPACTION_AT))
            .unwrap();
        assert_eq!(parse_time(last), None);
        // 無効にするときは何も消さない。
        state.with_store(|s| s.set_read(id, false)).unwrap();
        set_enabled(&state, false).unwrap();
        assert_eq!(
            state
                .with_store(|s| s.pending_local_ops(&device))
                .unwrap()
                .len(),
            1
        );
    }

    #[tokio::test]
    async fn imported_changes_emit_items_changed() {
        let (mut state, events) = test_state();
        let drive = ready(&mut state);
        // 何も変わらない同期では記事の通知を出さない。
        sync_once(&state).await.unwrap();
        assert!(!events.lock().unwrap().contains(&EVENT_ITEMS.to_string()));
        let id = phone_stars_an_item(&state, &drive);
        sync_once(&state).await.unwrap();
        assert!(events.lock().unwrap().contains(&EVENT_ITEMS.to_string()));
        assert!(
            state
                .with_store(|s| Ok(s.get_item(id)?.unwrap().starred))
                .unwrap()
        );
    }

    // ---- NAS ----

    use crate::sync::keychain::SecretStore;
    use crate::tailnet::{FakeTailnet, Tailnet, TailnetState};
    use infodesk_core::sync::webdav::WebDavConfig;

    const NAS_URL: &str = "https://nas.ts.net:5006/sync";

    fn running() -> TailnetState {
        TailnetState::Running {
            name: "mac".into(),
            ip: "100.1.2.3".into(),
        }
    }

    type Seen = Arc<Mutex<Vec<WebDavConfig>>>;

    /// 偽の Tailscale と NAS の保存先 (`drive`) を差し込み、NAS の設定をそろえる。保存先は選ばない。
    fn nas_parts(
        state: &mut AppState,
        tail: TailnetState,
        drive: Arc<dyn DriveStore>,
    ) -> (Arc<FakeTailnet>, Seen) {
        let fake = Arc::new(FakeTailnet::new(tail));
        state.tailnet = fake.clone();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let log = seen.clone();
        state.nas_drive_factory = Arc::new(move |cfg| {
            log.lock().unwrap().push(cfg);
            Ok(drive.clone())
        });
        set_nas(state, NAS_URL, "kou", Some("pw")).unwrap();
        (fake, seen)
    }

    /// NAS を選び、設定をそろえ、偽の Tailscale と MemoryDrive を差し込む。同期は有効。
    fn nas_ready(
        state: &mut AppState,
        tail: TailnetState,
    ) -> (Arc<FakeTailnet>, Arc<MemoryDrive>, Seen) {
        let drive = Arc::new(MemoryDrive::new());
        let (fake, seen) = nas_parts(state, tail, drive.clone());
        set_backend(state, "nas").unwrap();
        state
            .with_store(|s| s.set_setting(SETTING_ENABLED, "true"))
            .unwrap();
        (fake, drive, seen)
    }

    fn setting(state: &AppState, key: &str) -> Option<String> {
        state.with_store(|s| s.get_setting(key)).unwrap()
    }

    #[test]
    fn the_backend_is_gdrive_unless_it_is_nas() {
        let (state, _) = test_state();
        assert_eq!(BACKEND_GDRIVE, "gdrive");
        assert_eq!(status_now(&state).backend, "gdrive");
        for stored in ["google", "dropbox", ""] {
            state
                .with_store(|s| s.set_setting("sync.backend", stored))
                .unwrap();
            assert_eq!(status_now(&state).backend, "gdrive", "{stored:?}");
        }
        set_backend(&state, "nas").unwrap();
        assert_eq!(setting(&state, "sync.backend").as_deref(), Some("nas"));
        set_backend(&state, "gdrive").unwrap();
        assert_eq!(setting(&state, "sync.backend").as_deref(), Some("gdrive"));
        assert_eq!(status_now(&state).backend, "gdrive");
    }

    #[test]
    fn nas_settings_use_dotted_keys_and_old_keys_move_over() {
        let (state, _) = test_state();
        set_nas(&state, "http://nas:5005/a/", "kou", None).unwrap();
        assert_eq!(
            setting(&state, "sync.nas.url").as_deref(),
            Some("http://nas:5005/a/")
        );
        assert_eq!(setting(&state, "sync.nas.user").as_deref(), Some("kou"));
        // 開発中の DB に残った古いキーは、読んだときに新しいキーへ移す。
        let (state, _) = test_state();
        state
            .with_store(|s| {
                s.set_setting("sync.nas_url", "http://old:5005/x/")?;
                s.set_setting("sync.nas_user", "ken")
            })
            .unwrap();
        let st = status_now(&state);
        assert_eq!(st.nas_url, "http://old:5005/x/");
        assert_eq!(st.nas_user, "ken");
        assert_eq!(
            setting(&state, "sync.nas.url").as_deref(),
            Some("http://old:5005/x/")
        );
        assert_eq!(setting(&state, "sync.nas.user").as_deref(), Some("ken"));
        assert_eq!(setting(&state, "sync.nas_url"), None);
        assert_eq!(setting(&state, "sync.nas_user"), None);
    }

    #[test]
    fn switching_backend_asks_the_next_pass_to_start_over() {
        use infodesk_core::store::SyncFileRow;
        let (mut state, events) = test_state();
        ready(&mut state);
        state
            .with_store(|s| {
                s.put_sync_file(&SyncFileRow {
                    name: "a".into(),
                    kind: "out".into(),
                    drive_id: Some("d".into()),
                    sha256: None,
                    updated_at: "2026-10-13T00:00:00Z".into(),
                    fingerprint: None,
                })?;
                s.set_setting(SETTING_LAST_COMPACTION_AT, &Utc::now().to_rfc3339())
            })
            .unwrap();
        make_drive(&state, 0).unwrap();
        assert!(state.drive_cache.lock().unwrap().is_some());
        events.lock().unwrap().clear();
        assert!(set_backend(&state, "nas").unwrap());
        assert!(reset_pending(&state));
        assert!(state.drive_cache.lock().unwrap().is_none());
        assert_eq!(status_now(&state).backend, "nas");
        assert!(events.lock().unwrap().contains(&EVENT_SYNC.to_string()));
        assert!(set_backend(&state, "dropbox").is_err());
        assert_eq!(status_now(&state).backend, "nas");
    }

    #[tokio::test]
    async fn the_next_pass_forgets_remote_files_and_writes_a_snapshot() {
        use infodesk_core::store::SyncFileRow;
        let (mut state, _) = test_state();
        let (_, drive, _) = nas_ready(&mut state, running());
        state
            .with_store(|s| {
                s.put_sync_file(&SyncFileRow {
                    name: "a".into(),
                    kind: "out".into(),
                    drive_id: Some("d".into()),
                    sha256: None,
                    updated_at: "2026-10-13T00:00:00Z".into(),
                    fingerprint: None,
                })?;
                s.set_setting(SETTING_LAST_COMPACTION_AT, &Utc::now().to_rfc3339())
            })
            .unwrap();
        sync_once(&state).await.unwrap();
        assert!(!reset_pending(&state));
        let rows = state.with_store(|s| s.sync_files("out")).unwrap();
        assert!(rows.iter().all(|r| r.name != "a"), "{rows:?}");
        assert!(
            drive
                .names()
                .contains(&"state-snapshot.json.gz".to_string())
        );
    }

    #[tokio::test]
    async fn a_backend_switch_during_a_pass_does_not_undo_the_reset() {
        let (mut state, _) = test_state();
        let old = Arc::new(Slow::default());
        use_drive(&mut state, old.clone());
        let nas = Arc::new(MemoryDrive::new());
        nas_parts(&mut state, running(), nas.clone());
        // 前の保存先への同期が list で譲った間に、保存先を切り替える。
        let switch = async {
            assert!(set_backend(&state, "nas").unwrap());
        };
        let (r, ()) = tokio::join!(sync_once(&state), switch);
        r.unwrap();
        assert_eq!(old.lists.load(Ordering::SeqCst), 1, "the old pass ran");
        assert!(reset_pending(&state), "the old pass kept the reset");
        assert_eq!(setting(&state, SETTING_LAST_SYNC_AT), None, "stale pass");
        assert_eq!(status_now(&state).last_sync_at, None);
        let compacted = setting(&state, SETTING_LAST_COMPACTION_AT);
        assert!(parse_time(compacted).is_none(), "stale pass");
        sync_once(&state).await.unwrap();
        assert!(!reset_pending(&state));
        let names = nas.names();
        assert!(names.contains(&"manifest.json".to_string()), "{names:?}");
        assert!(
            names.contains(&"state-snapshot.json.gz".to_string()),
            "{names:?}"
        );
        assert_eq!(state.sync.lock().unwrap().error, None);
    }

    #[tokio::test]
    async fn changing_the_nas_url_asks_for_a_fresh_start() {
        let (mut state, _) = test_state();
        nas_ready(&mut state, running());
        sync_once(&state).await.unwrap();
        assert!(!reset_pending(&state));
        set_nas(&state, NAS_URL, "kou", Some("other")).unwrap();
        assert!(!reset_pending(&state), "same URL");
        set_nas(&state, "https://nas.ts.net:5006/other", "kou", None).unwrap();
        assert!(reset_pending(&state));
        sync_once(&state).await.unwrap();
        assert!(!reset_pending(&state));
    }

    #[test]
    fn choosing_the_same_backend_again_does_nothing() {
        let (mut state, events) = test_state();
        ready(&mut state);
        make_drive(&state, 0).unwrap();
        events.lock().unwrap().clear();
        let generation = state.sync_generation.load(Ordering::SeqCst);
        assert!(!set_backend(&state, "gdrive").unwrap());
        assert!(!reset_pending(&state));
        assert!(state.drive_cache.lock().unwrap().is_some());
        assert_eq!(state.sync_generation.load(Ordering::SeqCst), generation);
        assert!(events.lock().unwrap().is_empty());
        // 古い値 "google" も gdrive と同じに扱う。
        state
            .with_store(|s| s.set_setting("sync.backend", "google"))
            .unwrap();
        assert!(!set_backend(&state, "gdrive").unwrap());
        assert!(!reset_pending(&state));
    }

    #[test]
    fn the_nas_url_is_checked_before_anything_is_saved() {
        let (state, _) = test_state();
        for bad in [
            "",
            "   ",
            "takeuchi-nas:5005/infodesk/",
            "ftp://nas/infodesk/",
            "file:///tmp/x",
            "http://",
            "http://kou:pw@nas:5005/infodesk/",
            "http://kou@nas:5005/infodesk/",
            "not a url",
        ] {
            let err = set_nas(&state, bad, "kou", Some("pw")).unwrap_err();
            assert_eq!(
                err.message,
                "NAS の URL を確かめてください (例: http://takeuchi-nas:5005/infodesk/)",
                "{bad:?}"
            );
            assert_eq!(state.nas_secrets.get().unwrap(), None, "{bad:?}");
            assert_eq!(setting(&state, "sync.nas.url"), None, "{bad:?}");
            assert_eq!(setting(&state, SETTING_NAS_PASSWORD_SET), None, "{bad:?}");
        }
        set_nas(
            &state,
            " http://takeuchi-nas:5005/infodesk/ ",
            "kou",
            Some("pw"),
        )
        .unwrap();
        set_nas(&state, "https://nas.ts.net:5006/sync", "kou", None).unwrap();
        assert_eq!(state.nas_secrets.get().unwrap().as_deref(), Some("pw"));
    }

    #[test]
    fn the_tailnet_turning_running_requests_one_sync() {
        let (mut state, _) = test_state();
        let (fake, _, _) = nas_ready(&mut state, TailnetState::Stopped);
        let asked = Arc::new(AtomicUsize::new(0));
        let counter = asked.clone();
        state.set_sync_launcher(Arc::new(move || {
            counter.fetch_add(1, Ordering::SeqCst);
        }));
        let count = || asked.load(Ordering::SeqCst);
        // 起動時: Stopped -> Starting -> Running。
        on_tailnet_event(&state);
        fake.set_state(TailnetState::Starting);
        on_tailnet_event(&state);
        assert_eq!(count(), 0);
        fake.set_state(running());
        on_tailnet_event(&state);
        assert_eq!(count(), 1);
        on_tailnet_event(&state);
        status_now(&state);
        assert_eq!(count(), 1, "only on the transition");
        // ログイン: NeedsLogin -> Running。
        fake.set_state(TailnetState::NeedsLogin { url: "u".into() });
        on_tailnet_event(&state);
        fake.set_state(running());
        on_tailnet_event(&state);
        assert_eq!(count(), 2);
        // 同期が無効、または Google ドライブなら頼まない。
        set_enabled(&state, false).unwrap();
        fake.set_state(TailnetState::Stopped);
        on_tailnet_event(&state);
        fake.set_state(running());
        on_tailnet_event(&state);
        assert_eq!(count(), 2);
        set_enabled(&state, true).unwrap();
        set_backend(&state, "gdrive").unwrap();
        fake.set_state(TailnetState::Stopped);
        on_tailnet_event(&state);
        fake.set_state(running());
        on_tailnet_event(&state);
        assert_eq!(count(), 2);
    }

    #[test]
    fn a_missing_keychain_password_clears_the_flag() {
        let (mut state, _) = test_state();
        nas_ready(&mut state, running());
        assert!(status_now(&state).nas_password_set);
        state.nas_secrets.delete().unwrap();
        let err = make_drive(&state, 0).err().unwrap();
        assert_eq!(err.message, NAS_INCOMPLETE_MESSAGE);
        assert!(!status_now(&state).nas_password_set);
    }

    /// 書けないキーチェーン。
    struct BrokenKeychain;

    impl SecretStore for BrokenKeychain {
        fn get(&self) -> Result<Option<String>, String> {
            Ok(Some("pw".into()))
        }
        fn set(&self, _: &str) -> Result<(), String> {
            Err("denied".into())
        }
        fn delete(&self) -> Result<(), String> {
            Err("denied".into())
        }
    }

    #[test]
    fn a_keychain_failure_leaves_the_settings_as_they_were() {
        let (mut state, _) = test_state();
        set_backend(&state, "nas").unwrap();
        set_nas(&state, "http://nas/a/", "kou", Some("pw")).unwrap();
        state
            .with_store(|s| s.set_setting(SETTING_RESET_PENDING, ""))
            .unwrap();
        state.nas_secrets = Arc::new(BrokenKeychain);
        for password in [Some("new"), Some("")] {
            let err = set_nas(&state, "http://nas/b/", "ken", password).unwrap_err();
            assert_eq!(
                err.message,
                "NAS のパスワードをキーチェーンに保存できません"
            );
            let st = status_now(&state);
            assert_eq!(st.nas_url, "http://nas/a/");
            assert_eq!(st.nas_user, "kou");
            assert!(st.nas_password_set);
            assert!(!reset_pending(&state));
        }
    }

    /// 取得 (GET) だけ失敗する保存先。
    struct FailingGet(MemoryDrive);

    #[async_trait::async_trait]
    impl DriveStore for FailingGet {
        async fn list(&self) -> Result<Vec<RemoteFile>, DriveError> {
            self.0.list().await
        }
        async fn download(&self, _: &str) -> Result<Vec<u8>, DriveError> {
            Err(DriveError::Retryable("read timed out".into()))
        }
        async fn create(&self, n: &str, b: Vec<u8>, m: &str) -> Result<RemoteFile, DriveError> {
            self.0.create(n, b, m).await
        }
        async fn update(&self, id: &str, b: Vec<u8>, m: &str) -> Result<RemoteFile, DriveError> {
            self.0.update(id, b, m).await
        }
        async fn delete(&self, id: &str) -> Result<(), DriveError> {
            self.0.delete(id).await
        }
    }

    #[tokio::test]
    async fn the_nas_check_removes_its_file_when_the_read_fails() {
        let (mut state, _) = test_state();
        let drive = Arc::new(FailingGet(MemoryDrive::new()));
        nas_parts(&mut state, running(), drive.clone());
        assert_eq!(test_nas(&state).await, NETWORK_MESSAGE);
        assert!(drive.0.names().is_empty(), "the temporary file is removed");
    }

    #[test]
    fn webdav_construction_errors_hide_the_details() {
        let cfg = |url: &str, proxy: &str| WebDavConfig {
            url: url.into(),
            user: "kou".into(),
            password: "pw".into(),
            proxy: Some(proxy.into()),
        };
        for c in [
            cfg("http://nas/x/", "socks5h://tsnet:SECRET@127.0.0.1:notaport"),
            cfg("http://[bad", "socks5h://tsnet:SECRET@127.0.0.1:1"),
            cfg("mailto:x@y", "socks5h://tsnet:SECRET@127.0.0.1:1"),
        ] {
            let err = webdav_drive(c).err().unwrap();
            assert_eq!(err.message, "NAS への接続を準備できません");
        }
    }

    #[tokio::test]
    async fn stopping_the_tailnet_runs_off_the_caller() {
        let (mut state, _) = test_state();
        let (fake, _, _) = nas_ready(&mut state, running());
        stop_tailnet(&state).await.unwrap();
        assert_eq!(fake.state(), TailnetState::Stopped);
    }

    #[tokio::test]
    async fn a_successful_nas_check_lifts_the_credentials_stop() {
        let (mut state, _) = test_state();
        let (_, drive, _) = nas_ready(&mut state, running());
        *drive.fail_next.lock().unwrap() = Some(DriveError::BadCredentials);
        sync_once(&state).await.unwrap();
        assert!(state.nas_rejected.load(Ordering::SeqCst));
        assert_eq!(test_nas(&state).await, NAS_CHECK_OK);
        assert!(!state.nas_rejected.load(Ordering::SeqCst));
        assert_eq!(state.sync.lock().unwrap().error, None);
        sync_once(&state).await.unwrap();
        assert!(drive.names().contains(&"manifest.json".to_string()));
    }

    #[test]
    fn nas_settings_are_saved_and_the_password_goes_to_the_keychain() {
        let (state, _) = test_state();
        set_nas(&state, " https://nas/x ", " kou ", Some("secret")).unwrap();
        let st = status_now(&state);
        assert_eq!(st.nas_url, "https://nas/x");
        assert_eq!(st.nas_user, "kou");
        assert!(st.nas_password_set);
        assert!(!serde_json::to_string(&st).unwrap().contains("secret"));
        assert_eq!(state.nas_secrets.get().unwrap().as_deref(), Some("secret"));
        assert_eq!(setting(&state, "sync.nas_password"), None);
        assert_eq!(setting(&state, "sync.nas.password"), None);
        set_nas(&state, "https://nas/y", "kou", None).unwrap();
        assert_eq!(state.nas_secrets.get().unwrap().as_deref(), Some("secret"));
        assert!(status_now(&state).nas_password_set);
        set_nas(&state, "https://nas/y", "kou", Some("")).unwrap();
        assert_eq!(state.nas_secrets.get().unwrap(), None);
        assert!(!status_now(&state).nas_password_set);
    }

    #[test]
    fn nas_needs_tailscale_and_says_what_to_do() {
        let (mut state, _) = test_state();
        let (fake, _, seen) = nas_ready(&mut state, TailnetState::Stopped);
        let msg = |s: &AppState| make_drive(s, 0).err().unwrap().message;
        assert_eq!(msg(&state), "Tailscale にログインしてください");
        fake.set_state(TailnetState::NeedsLogin { url: "u".into() });
        assert_eq!(msg(&state), "Tailscale にログインしてください");
        fake.set_state(TailnetState::Starting);
        assert_eq!(msg(&state), "Tailscale にログインしてください");
        fake.set_state(TailnetState::NeedsApproval);
        assert_eq!(
            msg(&state),
            "Tailscale の管理画面でこの端末を承認してください"
        );
        assert!(seen.lock().unwrap().is_empty(), "never connects directly");
        fake.set_state(running());
        make_drive(&state, 0).unwrap();
        let cfg = seen.lock().unwrap()[0].clone();
        assert_eq!(cfg.url, NAS_URL);
        assert_eq!((cfg.user.as_str(), cfg.password.as_str()), ("kou", "pw"));
        assert_eq!(cfg.proxy, fake.proxy_url());
    }

    #[test]
    fn nas_incomplete_settings_stop_with_a_message() {
        let (mut state, _) = test_state();
        nas_ready(&mut state, running());
        set_nas(&state, NAS_URL, "", None).unwrap();
        assert_eq!(
            make_drive(&state, 0).err().unwrap().message,
            "NAS の URL、ユーザー名、パスワードを入力してください"
        );
    }

    #[tokio::test]
    async fn nas_sync_runs_only_when_tailscale_is_running_and_settings_are_complete() {
        let (mut state, _) = test_state();
        let (fake, drive, _) = nas_ready(&mut state, TailnetState::Stopped);
        sync_once(&state).await.unwrap();
        assert!(drive.names().is_empty());
        assert_eq!(state.sync.lock().unwrap().error, None);
        fake.set_state(running());
        sync_once(&state).await.unwrap();
        assert!(drive.names().contains(&"manifest.json".to_string()));
        assert!(status_now(&state).signed_in);
        fake.set_state(TailnetState::Stopped);
        assert!(!status_now(&state).signed_in);
    }

    #[tokio::test]
    async fn bad_credentials_show_a_message_and_stop_retries_until_settings_change() {
        let (mut state, _) = test_state();
        let (_, drive, _) = nas_ready(&mut state, running());
        *drive.fail_next.lock().unwrap() = Some(DriveError::BadCredentials);
        sync_once(&state).await.unwrap();
        let st = state.sync.lock().unwrap().clone();
        assert_eq!(
            st.error.as_deref(),
            Some("NAS のユーザー名かパスワードが違います")
        );
        assert!(status_now(&state).signed_in, "signed_in is not touched");
        *drive.fail_next.lock().unwrap() = Some(DriveError::Other("again".into()));
        sync_once(&state).await.unwrap();
        assert!(drive.fail_next.lock().unwrap().is_some(), "no retry");
        assert_eq!(
            state.sync.lock().unwrap().error.as_deref(),
            Some("NAS のユーザー名かパスワードが違います")
        );
        set_nas(&state, NAS_URL, "kou", Some("new")).unwrap();
        assert_eq!(state.sync.lock().unwrap().error, None);
        sync_once(&state).await.unwrap();
        assert!(drive.fail_next.lock().unwrap().is_none(), "retried");
    }

    #[tokio::test]
    async fn the_nas_check_reads_and_writes_even_when_sync_is_off() {
        let (mut state, _) = test_state();
        let (_, drive, _) = nas_ready(&mut state, running());
        state
            .with_store(|s| s.set_setting(SETTING_ENABLED, "false"))
            .unwrap();
        assert_eq!(test_nas(&state).await, "読み書きできました");
        assert!(drive.names().is_empty(), "the temporary file is removed");
        *drive.fail_next.lock().unwrap() = Some(DriveError::BadCredentials);
        assert_eq!(
            test_nas(&state).await,
            "NAS のユーザー名かパスワードが違います"
        );
        *drive.fail_next.lock().unwrap() = Some(DriveError::Retryable("tcp: secret".into()));
        assert_eq!(
            test_nas(&state).await,
            "ネットワークに接続できません。次の同期で再試行します。"
        );
    }

    #[tokio::test]
    async fn the_nas_check_needs_tailscale_and_settings() {
        let (mut state, _) = test_state();
        let (fake, _, seen) = nas_ready(&mut state, TailnetState::Stopped);
        assert_eq!(test_nas(&state).await, "Tailscale にログインしてください");
        fake.set_state(running());
        set_nas(&state, "https://nas/x", "", None).unwrap();
        assert_eq!(test_nas(&state).await, NAS_INCOMPLETE_MESSAGE);
        assert!(seen.lock().unwrap().is_empty());
    }

    #[test]
    fn the_tailnet_state_is_mirrored_into_the_status() {
        let (mut state, _) = test_state();
        let (fake, _, _) = nas_ready(&mut state, TailnetState::Stopped);
        let view = |s: &AppState| serde_json::to_value(status_now(s).tailnet).unwrap();
        assert_eq!(view(&state)["state"], "stopped");
        fake.set_state(TailnetState::NeedsLogin { url: "u".into() });
        assert_eq!(view(&state)["state"], "needs_login");
        fake.set_state(TailnetState::NeedsApproval);
        assert_eq!(view(&state)["state"], "needs_approval");
        fake.set_state(TailnetState::Starting);
        assert_eq!(view(&state)["state"], "starting");
        fake.set_state(running());
        assert_eq!(view(&state)["state"], "running");
        assert_eq!(view(&state)["name"], "mac");
        fake.set_state(TailnetState::Failed {
            reason: "boom".into(),
        });
        assert_eq!(view(&state)["state"], "failed");
        assert_eq!(view(&state)["reason"], "boom");
    }

    #[test]
    fn the_tailnet_starts_at_launch_only_for_an_enabled_nas() {
        let (mut state, _) = test_state();
        nas_ready(&mut state, TailnetState::Stopped);
        assert!(should_start_tailnet(&state));
        set_enabled(&state, false).unwrap();
        assert!(!should_start_tailnet(&state));
        set_enabled(&state, true).unwrap();
        set_backend(&state, "gdrive").unwrap();
        assert!(!should_start_tailnet(&state));
    }
}
