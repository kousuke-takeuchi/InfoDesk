//! アプリ内の Tailscale 端末 (libtailscale)。システムの VPN は使わず、NAS へはここだけを通す。
//!
//! 単体テストは純粋な部分 (status の JSON から状態を作る、プロキシ URL の組み立て、
//! HTTP 応答の分解) と `FakeTailnet` だけを使う。本物の端末は起動しない。

use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use base64::Engine;
use serde::Deserialize;

use crate::error::{AppError, AppResult};
use crate::state::{EVENT_SYNC, Notifier};

pub const HOSTNAME: &str = "infodesk-mac";
const POLL_INTERVAL: Duration = Duration::from_secs(2);
const LOCALAPI_TIMEOUT: Duration = Duration::from_secs(5);
/// ログアウトはコントロールサーバーとの通信を伴うので長めに待つ。
const LOGOUT_TIMEOUT: Duration = Duration::from_secs(30);
/// ログアウトのために端末を起動するとき、LocalAPI が使えるまで待つ上限。
const LOGOUT_BOOT_TIMEOUT: Duration = Duration::from_secs(60);
/// ログのアップロード (log.tailscale.com) を止める環境変数 (`envknob.NoLogsNoSupport`)。
pub const NO_LOGS_ENV: &str = "TS_NO_LOGS_NO_SUPPORT";
/// LocalAPI が要求する Host ヘッダー (`apitype.LocalAPIHost`)。
const LOCALAPI_HOST: &str = "local-tailscaled.sock";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TailnetState {
    Stopped,
    /// 起動に失敗した。`reason` は資格情報を含まない。
    Failed {
        reason: String,
    },
    NeedsLogin {
        url: String,
    },
    /// ログイン済みだが、管理画面での端末の承認待ち。
    NeedsApproval,
    Starting,
    Running {
        name: String,
        ip: String,
    },
}

pub trait Tailnet: Send + Sync {
    fn state(&self) -> TailnetState;
    /// 古い起動処理がまだ残っていれば、その終了を待つ。メインスレッドから呼ばないこと。
    fn start(&self) -> AppResult<()>;
    /// `Running` のときだけ `Some`。NAS への通信はこの SOCKS5 プロキシを通す。
    fn proxy_url(&self) -> Option<String>;
    /// 端末を登録解除して状態ディレクトリを消す。端末が止まっていれば起動して待つので、
    /// 最長で 90 秒ほどブロックする。メインスレッドから呼ばないこと。
    /// 登録解除に失敗したらエラーを返し、ディレクトリは消さない。
    fn logout(&self) -> AppResult<()>;
    fn stop(&self);
}

#[derive(Deserialize)]
struct StatusJson {
    #[serde(rename = "BackendState", default)]
    backend_state: String,
    #[serde(rename = "AuthURL", default)]
    auth_url: String,
    #[serde(rename = "TailscaleIPs", default)]
    ips: Vec<String>,
    #[serde(rename = "Self")]
    this: Option<SelfJson>,
}

#[derive(Deserialize)]
struct SelfJson {
    #[serde(rename = "DNSName", default)]
    dns_name: String,
    #[serde(rename = "TailscaleIPs", default)]
    ips: Vec<String>,
}

/// LocalAPI の `/localapi/v0/status?peers=false` の JSON から状態を作る。読めない JSON は `Starting`。
pub fn state_from_status(json: &str) -> TailnetState {
    let Ok(s) = serde_json::from_str::<StatusJson>(json) else {
        return TailnetState::Starting;
    };
    match s.backend_state.as_str() {
        "NeedsLogin" if !s.auth_url.is_empty() => TailnetState::NeedsLogin { url: s.auth_url },
        "Running" => {
            let (dns, ips) = match &s.this {
                Some(me) => (me.dns_name.as_str(), &me.ips),
                None => ("", &s.ips),
            };
            let ips = if ips.is_empty() { &s.ips } else { ips };
            let ip = ips
                .iter()
                .find(|ip| !ip.contains(':'))
                .or_else(|| ips.first())
                .cloned()
                .unwrap_or_default();
            TailnetState::Running {
                name: dns.trim_end_matches('.').to_string(),
                ip,
            }
        }
        "NeedsMachineAuth" => TailnetState::NeedsApproval,
        "Stopped" => TailnetState::Stopped,
        // NoState、Starting、URL がまだない NeedsLogin。
        _ => TailnetState::Starting,
    }
}

/// `loopback()` の戻りから SOCKS5 プロキシの URL を作る。名前解決も端末側で行う (`socks5h`)。
pub fn build_proxy_url(user: &str, credential: &str, address: &str) -> String {
    format!("socks5h://{user}:{credential}@{address}")
}

/// HTTP/1.0 の応答を (ステータス, 本文) に分ける。
fn split_http_response(raw: &[u8]) -> Option<(u16, String)> {
    let text = String::from_utf8_lossy(raw);
    let (head, body) = text.split_once("\r\n\r\n")?;
    let status = head
        .lines()
        .next()?
        .split_whitespace()
        .nth(1)?
        .parse()
        .ok()?;
    Some((status, body.to_string()))
}

struct Loopback {
    address: String,
    credential: String,
    proxy_user: String,
    proxy_credential: String,
}

fn localapi(
    lb: &Loopback,
    method: &str,
    path: &str,
    timeout: Duration,
) -> Result<(u16, String), String> {
    let addr = lb
        .address
        .to_socket_addrs()
        .map_err(|e| e.to_string())?
        .next()
        .ok_or("no address")?;
    let mut s = TcpStream::connect_timeout(&addr, timeout).map_err(|e| e.to_string())?;
    s.set_read_timeout(Some(timeout)).ok();
    s.set_write_timeout(Some(timeout)).ok();
    let auth = base64::engine::general_purpose::STANDARD.encode(format!(":{}", lb.credential));
    let req = format!(
        "{method} {path} HTTP/1.0\r\nHost: {LOCALAPI_HOST}\r\nSec-Tailscale: localapi\r\nAuthorization: Basic {auth}\r\nContent-Length: 0\r\n\r\n"
    );
    s.write_all(req.as_bytes()).map_err(|e| e.to_string())?;
    let mut buf = Vec::new();
    let _ = s.read_to_end(&mut buf);
    split_http_response(&buf).ok_or_else(|| "bad LocalAPI response".to_string())
}

/// ログアウトの応答を調べる。成功は 204 だけ。エラーには本文も資格情報も入れない。
fn check_logout(response: Result<(u16, String), String>) -> AppResult<()> {
    match response {
        Ok((204, _)) => Ok(()),
        Ok((status, _)) => Err(AppError::from(format!(
            "Tailscale logout failed (HTTP {status})"
        ))),
        Err(_) => Err(AppError::from("Tailscale logout failed".to_string())),
    }
}

pub type OpenUrl = Arc<dyn Fn(&str) + Send + Sync>;

struct Config {
    dir: PathBuf,
    open: OpenUrl,
}

#[derive(Default)]
struct Inner {
    state: Option<TailnetState>,
    loopback: Option<Arc<Loopback>>,
    /// 端末そのもの。手放す (drop) と libtailscale が閉じる。
    node: Option<libtailscale::Tailscale>,
    running: bool,
    /// 起動、停止のたびに増やす。状態の書き込みと同じロックの中で比べる。
    generation: u64,
    /// 直近の起動スレッド。次の起動はこれの終了を待ってから始める。
    thread: Option<JoinHandle<()>>,
}

pub struct EmbeddedTailnet {
    notify: Notifier,
    config: Mutex<Option<Config>>,
    inner: Arc<Mutex<Inner>>,
}

/// 世代が `gen_id` のままなら状態を書く。古い世代 (stop や再起動の後) なら何もせず `None`。
/// 世代の確認と書き込みを同じロックの中で行うので、stop の後に状態が戻ることはない。
/// `Some(changed)` を返したときだけ、呼び出し側は URL を開くなどの副作用を起こしてよい。
/// `finished` が真なら、起動スレッドが終わる (`running` を下ろす) ことも同時に記録する。
fn commit(inner: &Mutex<Inner>, gen_id: u64, next: TailnetState, finished: bool) -> Option<bool> {
    let mut g = inner.lock().unwrap_or_else(|e| e.into_inner());
    if g.generation != gen_id {
        return None;
    }
    if finished {
        g.running = false;
    }
    let changed = g.state.as_ref() != Some(&next);
    g.state = Some(next);
    Some(changed)
}

fn current(inner: &Mutex<Inner>, gen_id: u64) -> bool {
    inner.lock().unwrap_or_else(|e| e.into_inner()).generation == gen_id
}

impl EmbeddedTailnet {
    /// 状態ディレクトリはまだ決まっていない。`configure` を呼ぶまで `start` は失敗する。
    pub fn new(notify: Notifier) -> Self {
        Self {
            notify,
            config: Mutex::new(None),
            inner: Arc::new(Mutex::new(Inner::default())),
        }
    }

    pub fn configure(&self, dir: PathBuf, open: OpenUrl) {
        *self.config.lock().expect("tailnet config lock") = Some(Config { dir, open });
    }

    fn loopback(&self) -> Option<Arc<Loopback>> {
        self.inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .loopback
            .clone()
    }

    fn dir(&self) -> Option<PathBuf> {
        self.config
            .lock()
            .expect("tailnet config lock")
            .as_ref()
            .map(|c| c.dir.clone())
    }

    /// 起動済みの端末の LocalAPI が使えるようになるまで待つ。
    fn wait_for_loopback(&self, timeout: Duration) -> AppResult<Arc<Loopback>> {
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(lb) = self.loopback() {
                return Ok(lb);
            }
            if let TailnetState::Failed { reason } = self.state() {
                return Err(AppError::from(format!(
                    "Tailscale could not start: {reason}"
                )));
            }
            if Instant::now() >= deadline {
                return Err(AppError::from(
                    "Tailscale did not become ready in time".to_string(),
                ));
            }
            std::thread::sleep(Duration::from_millis(200));
        }
    }
}

/// 状態ディレクトリを作り、既にあっても権限を 0o700 にする (鍵が入る)。
fn prepare_dir(dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

fn boot(dir: &Path) -> Result<(libtailscale::Tailscale, Loopback), String> {
    prepare_dir(dir).map_err(|e| e.to_string())?;
    let mut ts = libtailscale::Tailscale::new();
    ts.set_dir(&dir.to_string_lossy())?;
    ts.set_hostname(HOSTNAME)?;
    ts.set_logfd(-1)?;
    ts.start()?;
    let lb = ts.loopback()?;
    Ok((
        ts,
        Loopback {
            address: lb.address,
            credential: lb.credential,
            proxy_user: lb.proxy_username.to_string(),
            proxy_credential: lb.proxy_credential,
        },
    ))
}

/// `POLL_INTERVAL` だけ待つ。世代が変わったら早めに戻る。
fn sleep_while_current(inner: &Mutex<Inner>, gen_id: u64) {
    let until = Instant::now() + POLL_INTERVAL;
    while Instant::now() < until && current(inner, gen_id) {
        std::thread::sleep(Duration::from_millis(100));
    }
}

impl Tailnet for EmbeddedTailnet {
    fn state(&self) -> TailnetState {
        self.inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .state
            .clone()
            .unwrap_or(TailnetState::Stopped)
    }

    fn start(&self) -> AppResult<()> {
        let (dir, open) = {
            let cfg = self.config.lock().expect("tailnet config lock");
            let cfg = cfg
                .as_ref()
                .ok_or_else(|| AppError::from("Tailscale is not configured".to_string()))?;
            (cfg.dir.clone(), cfg.open.clone())
        };
        let (gen_id, previous) = {
            let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
            if g.running {
                return Ok(());
            }
            g.running = true;
            g.generation += 1;
            g.state = Some(TailnetState::Starting);
            (g.generation, g.thread.take())
        };
        (self.notify)(EVENT_SYNC);
        // 同じ状態ディレクトリで 2 つの端末を起こさない。古い世代は自分で終わる。
        if let Some(previous) = previous {
            let _ = previous.join();
        }
        let inner = self.inner.clone();
        let notify = self.notify.clone();
        let handle = std::thread::spawn(move || {
            if !current(&inner, gen_id) {
                return;
            }
            let (ts, lb) = match boot(&dir) {
                Ok(v) => v,
                Err(reason) => {
                    // 資格情報はここまで届かない。エラー文だけを残す。
                    eprintln!("tailnet: boot failed: {reason}");
                    if commit(&inner, gen_id, TailnetState::Failed { reason }, true) == Some(true) {
                        notify(EVENT_SYNC);
                    }
                    return;
                }
            };
            let lb = Arc::new(lb);
            {
                let mut g = inner.lock().unwrap_or_else(|e| e.into_inner());
                if g.generation != gen_id {
                    return; // 起動中に stop された。ts はここで drop される。
                }
                g.node = Some(ts);
                g.loopback = Some(lb.clone());
            }
            let mut opened: Option<String> = None;
            while current(&inner, gen_id) {
                if let Ok((200, body)) = localapi(
                    &lb,
                    "GET",
                    "/localapi/v0/status?peers=false",
                    LOCALAPI_TIMEOUT,
                ) {
                    let next = state_from_status(&body);
                    let url = match &next {
                        TailnetState::NeedsLogin { url } => Some(url.clone()),
                        _ => None,
                    };
                    // 世代の確認が通ったときだけ、通知とブラウザを開く処理に進む。
                    if let Some(changed) = commit(&inner, gen_id, next, false) {
                        if changed {
                            notify(EVENT_SYNC);
                        }
                        if let Some(url) = url
                            && opened.as_deref() != Some(url.as_str())
                        {
                            open(&url);
                            opened = Some(url);
                        }
                    }
                }
                sleep_while_current(&inner, gen_id);
            }
        });
        self.inner.lock().unwrap_or_else(|e| e.into_inner()).thread = Some(handle);
        Ok(())
    }

    fn proxy_url(&self) -> Option<String> {
        if !matches!(self.state(), TailnetState::Running { .. }) {
            return None;
        }
        let lb = self.loopback()?;
        Some(build_proxy_url(
            &lb.proxy_user,
            &lb.proxy_credential,
            &lb.address,
        ))
    }

    fn logout(&self) -> AppResult<()> {
        let dir = self
            .dir()
            .ok_or_else(|| AppError::from("Tailscale is not configured".to_string()))?;
        // 状態ディレクトリがなければ、登録されたものもない。
        if dir.exists() {
            let lb = match self.loopback() {
                Some(lb) => lb,
                None => {
                    self.start()?;
                    self.wait_for_loopback(LOGOUT_BOOT_TIMEOUT)
                        .inspect_err(|_| {
                            self.stop();
                        })?
                }
            };
            let res = localapi(&lb, "POST", "/localapi/v0/logout", LOGOUT_TIMEOUT);
            check_logout(res)?;
        }
        self.stop();
        if dir.exists() {
            std::fs::remove_dir_all(&dir).map_err(|e| AppError::from(e.to_string()))?;
        }
        Ok(())
    }

    fn stop(&self) {
        let node = {
            let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
            g.generation += 1;
            g.running = false;
            g.loopback = None;
            g.state = Some(TailnetState::Stopped);
            g.node.take()
        };
        // drop が libtailscale の端末を閉じる。ロックの外で行う。
        drop(node);
        (self.notify)(EVENT_SYNC);
    }
}

/// テスト用。状態を外から変えられる。
pub struct FakeTailnet {
    state: Mutex<TailnetState>,
    proxy: Mutex<String>,
}

impl FakeTailnet {
    pub fn new(state: TailnetState) -> Self {
        Self {
            state: Mutex::new(state),
            proxy: Mutex::new("socks5h://tsnet:fake@127.0.0.1:1".to_string()),
        }
    }

    pub fn set_state(&self, state: TailnetState) {
        *self.state.lock().unwrap() = state;
    }
}

impl Tailnet for FakeTailnet {
    fn state(&self) -> TailnetState {
        self.state.lock().unwrap().clone()
    }
    fn start(&self) -> AppResult<()> {
        Ok(())
    }
    fn proxy_url(&self) -> Option<String> {
        matches!(self.state(), TailnetState::Running { .. })
            .then(|| self.proxy.lock().unwrap().clone())
    }
    fn logout(&self) -> AppResult<()> {
        self.set_state(TailnetState::Stopped);
        Ok(())
    }
    fn stop(&self) {
        self.set_state(TailnetState::Stopped);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn needs_login_carries_the_auth_url() {
        let j = r#"{"BackendState":"NeedsLogin","AuthURL":"https://login.tailscale.com/a/abc","Self":null}"#;
        assert_eq!(
            state_from_status(j),
            TailnetState::NeedsLogin {
                url: "https://login.tailscale.com/a/abc".into()
            }
        );
    }

    #[test]
    fn needs_login_without_url_is_still_starting() {
        let j = r#"{"BackendState":"NeedsLogin","AuthURL":""}"#;
        assert_eq!(state_from_status(j), TailnetState::Starting);
    }

    #[test]
    fn running_uses_self_name_and_ipv4() {
        let j = r#"{"BackendState":"Running","AuthURL":"","TailscaleIPs":["100.1.2.3","fd7a::1"],
            "Self":{"DNSName":"infodesk-mac.tail1234.ts.net.","TailscaleIPs":["fd7a::1","100.1.2.3"]}}"#;
        assert_eq!(
            state_from_status(j),
            TailnetState::Running {
                name: "infodesk-mac.tail1234.ts.net".into(),
                ip: "100.1.2.3".into()
            }
        );
    }

    #[test]
    fn running_falls_back_to_top_level_ips() {
        let j = r#"{"BackendState":"Running","TailscaleIPs":["100.9.9.9"],"Self":{"DNSName":"a.b.ts.net."}}"#;
        assert_eq!(
            state_from_status(j),
            TailnetState::Running {
                name: "a.b.ts.net".into(),
                ip: "100.9.9.9".into()
            }
        );
    }

    #[test]
    fn stopped_starting_and_garbage() {
        assert_eq!(
            state_from_status(r#"{"BackendState":"Stopped"}"#),
            TailnetState::Stopped
        );
        assert_eq!(
            state_from_status(r#"{"BackendState":"Starting"}"#),
            TailnetState::Starting
        );
        assert_eq!(
            state_from_status(r#"{"BackendState":"NoState"}"#),
            TailnetState::Starting
        );
        assert_eq!(state_from_status("not json"), TailnetState::Starting);
    }

    #[test]
    fn proxy_url_is_socks5h_with_the_tsnet_user() {
        assert_eq!(
            build_proxy_url("tsnet", "abc123", "127.0.0.1:54321"),
            "socks5h://tsnet:abc123@127.0.0.1:54321"
        );
    }

    #[test]
    fn splits_a_http_response() {
        let raw = b"HTTP/1.0 200 OK\r\nContent-Type: application/json\r\n\r\n{\"a\":1}";
        assert_eq!(split_http_response(raw), Some((200, "{\"a\":1}".into())));
        assert_eq!(split_http_response(b"garbage"), None);
    }

    #[test]
    fn fake_has_no_proxy_until_running() {
        let fake = FakeTailnet::new(TailnetState::NeedsLogin { url: "u".into() });
        assert_eq!(fake.proxy_url(), None);
        fake.set_state(TailnetState::Running {
            name: "n".into(),
            ip: "100.1.1.1".into(),
        });
        assert!(fake.proxy_url().unwrap().starts_with("socks5h://"));
        fake.stop();
        assert_eq!(fake.proxy_url(), None);
    }

    #[test]
    fn embedded_without_configuration_does_not_start() {
        let t = EmbeddedTailnet::new(Arc::new(|_| {}));
        assert!(t.start().is_err());
        assert_eq!(t.state(), TailnetState::Stopped);
        assert_eq!(t.proxy_url(), None);
    }

    fn embedded() -> EmbeddedTailnet {
        EmbeddedTailnet::new(Arc::new(|_| {}))
    }

    #[test]
    fn a_stale_generation_cannot_overwrite_stopped() {
        let t = embedded();
        let gen_id = {
            let mut g = t.inner.lock().unwrap();
            g.generation = 1;
            g.running = true;
            g.generation
        };
        let running = TailnetState::Running {
            name: "n".into(),
            ip: "100.1.1.1".into(),
        };
        assert_eq!(commit(&t.inner, gen_id, running.clone(), false), Some(true));
        t.stop();
        assert_eq!(t.state(), TailnetState::Stopped);
        // stop の前に取った世代での更新は捨てられる。
        assert_eq!(commit(&t.inner, gen_id, running, false), None);
        let login = TailnetState::NeedsLogin { url: "u".into() };
        assert_eq!(commit(&t.inner, gen_id, login, false), None);
        assert_eq!(t.state(), TailnetState::Stopped);
    }

    #[test]
    fn a_stale_boot_failure_does_not_clear_running() {
        let t = embedded();
        {
            let mut g = t.inner.lock().unwrap();
            g.generation = 2;
            g.running = true;
        }
        let failed = TailnetState::Failed { reason: "x".into() };
        assert_eq!(commit(&t.inner, 1, failed.clone(), true), None);
        assert!(t.inner.lock().unwrap().running);
        assert_eq!(commit(&t.inner, 2, failed.clone(), true), Some(true));
        assert!(!t.inner.lock().unwrap().running);
        assert_eq!(t.state(), failed);
    }

    #[test]
    fn needs_machine_auth_is_its_own_state() {
        assert_eq!(
            state_from_status(r#"{"BackendState":"NeedsMachineAuth"}"#),
            TailnetState::NeedsApproval
        );
    }

    #[test]
    fn logout_requires_204_and_hides_the_body() {
        assert!(check_logout(Ok((204, String::new()))).is_ok());
        let err = check_logout(Ok((500, "secret-token".into()))).unwrap_err();
        let text = format!("{err:?}");
        assert!(text.contains("500"));
        assert!(!text.contains("secret-token"));
        assert!(check_logout(Err("refused".into())).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn state_dir_is_private_even_if_it_already_exists() {
        use std::os::unix::fs::PermissionsExt;
        let base = std::env::temp_dir().join(format!("infodesk-ts-{}", std::process::id()));
        let dir = base.join("tailscale");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
        prepare_dir(&dir).unwrap();
        let mode = std::fs::metadata(&dir).unwrap().permissions().mode() & 0o777;
        std::fs::remove_dir_all(&base).unwrap();
        assert_eq!(mode, 0o700);
    }

    #[test]
    fn logout_without_configuration_fails() {
        assert!(embedded().logout().is_err());
    }
}
