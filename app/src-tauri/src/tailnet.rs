//! アプリ内の Tailscale 端末 (libtailscale)。システムの VPN は使わず、NAS へはここだけを通す。
//!
//! 単体テストは純粋な部分 (status の JSON から状態を作る、プロキシ URL の組み立て、
//! HTTP 応答の分解) と `FakeTailnet` だけを使う。本物の端末は起動しない。

use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use base64::Engine;
use serde::Deserialize;

use crate::error::{AppError, AppResult};
use crate::state::{EVENT_SYNC, Notifier};

pub const HOSTNAME: &str = "infodesk-mac";
const POLL_INTERVAL: Duration = Duration::from_secs(2);
const LOCALAPI_TIMEOUT: Duration = Duration::from_secs(5);
/// LocalAPI が要求する Host ヘッダー (`apitype.LocalAPIHost`)。
const LOCALAPI_HOST: &str = "local-tailscaled.sock";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TailnetState {
    Stopped,
    NeedsLogin { url: String },
    Starting,
    Running { name: String, ip: String },
}

pub trait Tailnet: Send + Sync {
    fn state(&self) -> TailnetState;
    fn start(&self) -> AppResult<()>;
    /// `Running` のときだけ `Some`。NAS への通信はこの SOCKS5 プロキシを通す。
    fn proxy_url(&self) -> Option<String>;
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

/// LocalAPI の `/localapi/v0/status` の JSON から状態を作る。読めない JSON は `Starting`。
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
        "Stopped" => TailnetState::Stopped,
        // NoState、NeedsMachineAuth、Starting、URL がまだない NeedsLogin。
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

fn localapi(lb: &Loopback, method: &str, path: &str) -> Result<(u16, String), String> {
    let addr = lb
        .address
        .to_socket_addrs()
        .map_err(|e| e.to_string())?
        .next()
        .ok_or("no address")?;
    let mut s = TcpStream::connect_timeout(&addr, LOCALAPI_TIMEOUT).map_err(|e| e.to_string())?;
    s.set_read_timeout(Some(LOCALAPI_TIMEOUT)).ok();
    s.set_write_timeout(Some(LOCALAPI_TIMEOUT)).ok();
    let auth = base64::engine::general_purpose::STANDARD.encode(format!(":{}", lb.credential));
    let req = format!(
        "{method} {path} HTTP/1.0\r\nHost: {LOCALAPI_HOST}\r\nSec-Tailscale: localapi\r\nAuthorization: Basic {auth}\r\nContent-Length: 0\r\n\r\n"
    );
    s.write_all(req.as_bytes()).map_err(|e| e.to_string())?;
    let mut buf = Vec::new();
    let _ = s.read_to_end(&mut buf);
    split_http_response(&buf).ok_or_else(|| "bad LocalAPI response".to_string())
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
}

pub struct EmbeddedTailnet {
    notify: Notifier,
    config: Mutex<Option<Config>>,
    inner: Arc<Mutex<Inner>>,
    /// 起動、停止のたびに増やす。古い監視スレッドが自分で止まる。
    generation: Arc<AtomicU64>,
}

impl EmbeddedTailnet {
    /// 状態ディレクトリはまだ決まっていない。`configure` を呼ぶまで `start` は失敗する。
    pub fn new(notify: Notifier) -> Self {
        Self {
            notify,
            config: Mutex::new(None),
            inner: Arc::new(Mutex::new(Inner::default())),
            generation: Arc::new(AtomicU64::new(0)),
        }
    }

    pub fn configure(&self, dir: PathBuf, open: OpenUrl) {
        *self.config.lock().expect("tailnet config lock") = Some(Config { dir, open });
    }

    fn set_state(inner: &Mutex<Inner>, notify: &Notifier, next: TailnetState) {
        let changed = {
            let mut g = inner.lock().unwrap_or_else(|e| e.into_inner());
            let changed = g.state.as_ref() != Some(&next);
            g.state = Some(next);
            changed
        };
        if changed {
            notify(EVENT_SYNC);
        }
    }

    fn loopback(&self) -> Option<Arc<Loopback>> {
        self.inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .loopback
            .clone()
    }
}

fn boot(dir: &Path) -> Result<(libtailscale::Tailscale, Loopback), String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
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
        {
            let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
            if g.running {
                return Ok(());
            }
            g.running = true;
        }
        let gen_id = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        Self::set_state(&self.inner, &self.notify, TailnetState::Starting);
        let inner = self.inner.clone();
        let notify = self.notify.clone();
        let generation = self.generation.clone();
        std::thread::spawn(move || {
            let current = || generation.load(Ordering::SeqCst) == gen_id;
            let (ts, lb) = match boot(&dir) {
                Ok(v) => v,
                Err(_) => {
                    if current() {
                        inner.lock().unwrap_or_else(|e| e.into_inner()).running = false;
                        Self::set_state(&inner, &notify, TailnetState::Stopped);
                    }
                    return;
                }
            };
            let lb = Arc::new(lb);
            {
                let mut g = inner.lock().unwrap_or_else(|e| e.into_inner());
                if !current() {
                    return; // 起動中に stop された。ts はここで drop される。
                }
                g.node = Some(ts);
                g.loopback = Some(lb.clone());
            }
            let mut opened: Option<String> = None;
            while current() {
                if let Ok((200, body)) = localapi(&lb, "GET", "/localapi/v0/status") {
                    let next = state_from_status(&body);
                    if let TailnetState::NeedsLogin { url } = &next
                        && opened.as_deref() != Some(url.as_str())
                    {
                        open(url);
                        opened = Some(url.clone());
                    }
                    if current() {
                        Self::set_state(&inner, &notify, next);
                    }
                }
                std::thread::sleep(POLL_INTERVAL);
            }
        });
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
        if let Some(lb) = self.loopback() {
            // 失敗しても端末を止めて状態を消す (次の起動で新しくログインする)。
            let _ = localapi(&lb, "POST", "/localapi/v0/logout");
        }
        self.stop();
        let dir = self
            .config
            .lock()
            .expect("tailnet config lock")
            .as_ref()
            .map(|c| c.dir.clone());
        if let Some(dir) = dir
            && dir.exists()
        {
            std::fs::remove_dir_all(&dir).map_err(|e| AppError::from(e.to_string()))?;
        }
        Ok(())
    }

    fn stop(&self) {
        self.generation.fetch_add(1, Ordering::SeqCst);
        let node = {
            let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
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
}
