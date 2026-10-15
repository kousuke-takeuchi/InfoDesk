//! アプリ内の Tailscale 端末 (libtailscale)。システムの VPN は使わず、NAS へはここだけを通す。
//!
//! 単体テストは純粋な部分 (status の JSON から状態を作る、プロキシ URL の組み立て、
//! HTTP 応答の分解) と `FakeTailnet` だけを使う。本物の端末は起動しない。

use std::io::{Read, Write};
use std::net::{IpAddr, TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
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
/// tsnet が鍵を置くファイル (tsnet.go の `Dir/tailscaled.state`)。あればログイン済みの端末がある。
const STATE_FILE: &str = "tailscaled.state";
/// `stop` が起動やログアウトの終わりを待つ上限。終了時 (メインスレッド) を長く止めない。
const STOP_LOCK_WAIT: Duration = Duration::from_secs(2);

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
    /// 裏で端末を起こす。ログインが要っても、ログイン画面は開かない (開くのは `login` だけ)。
    /// 古い起動処理がまだ残っていれば、その終了を待つ。メインスレッドから呼ばないこと。
    fn start(&self) -> AppResult<()>;
    /// 前に起動した端末の状態 (鍵) が残っているか。起動時の自動起動はこれがあるときだけ。
    fn has_saved_state(&self) -> bool;
    /// `host` が tailnet の端末 (peer) なら、その Tailscale の IP (IPv4 を優先) を返す。
    /// 端末の HostName、DNSName (末尾の点はあってもなくてもよい、最初の点より前の短い名前でもよい)、
    /// Tailscale の IP のどれかに一致したときだけ。`Running` でなければ `None`。
    /// LocalAPI に問い合わせるので、メインスレッドから呼ばないこと。
    fn peer_addr(&self, host: &str) -> Option<IpAddr>;
    /// `Running` のときだけ `Some`。NAS への通信はこの SOCKS5 プロキシを通す。
    fn proxy_url(&self) -> Option<String>;
    /// 端末を登録解除して状態ディレクトリを消す。端末が止まっていれば起動して待つので、
    /// 最長で 90 秒ほどブロックする。メインスレッドから呼ばないこと。
    /// 登録解除に失敗したらエラーを返し、ディレクトリは消さない。
    fn logout(&self) -> AppResult<()>;
    fn stop(&self);
    /// 「Tailscale にログイン」(利用者が押したときだけ)。ログイン待ちなら同じログイン URL を
    /// もう一度開く (端末は起こし直さない)。それ以外は端末を起こし、ログイン URL が分かったら開く。
    /// メインスレッドから呼ばないこと。
    fn login(&self) -> AppResult<()>;
    /// LocalAPI でのログアウトをせずに、端末を止めて状態ディレクトリを消す。起動が失敗し続けて
    /// ログアウトもできないときの手当て。管理画面には端末が残る。メインスレッドから呼ばないこと。
    fn forget(&self) -> AppResult<()>;
}

/// 環境変数の値が真か。Go の `strconv.ParseBool` (envknob が使う) が真とする値だけを受け付ける。
pub fn no_logs_env_is_true(v: Option<&std::ffi::OsStr>) -> bool {
    matches!(
        v.and_then(|v| v.to_str()),
        Some("1" | "t" | "T" | "TRUE" | "true" | "True")
    )
}

pub const BOOT_REFUSED_REASON: &str = "Tailscale のログ送信を止められなかったため起動しません";

/// ログの送信を止められなかった。このプロセスでは端末を起こさない。
static BOOT_REFUSED: AtomicBool = AtomicBool::new(false);

/// このプロセスで Tailscale の端末を起こさないようにする (戻せない)。
pub fn refuse_boot() {
    BOOT_REFUSED.store(true, Ordering::SeqCst);
}

/// テストだけが使う。拒否を他のテストに残さない。
#[cfg(test)]
pub fn reset_boot_refusal() {
    BOOT_REFUSED.store(false, Ordering::SeqCst);
}

/// 今の SIGPIPE の扱い (ハンドラ、フラグ、マスク) をそのまま写したもの。
#[cfg(unix)]
pub struct SigpipeDisposition(libc::sigaction);

/// 今の SIGPIPE の扱いを読む。`exec` は呼ぶ前に SIGPIPE を既定に戻すので、その前に取っておく。
/// Rust の実行時は無視にするが、Go のランタイム (c-archive) が自分のハンドラを入れていることもある。
#[cfg(unix)]
pub fn save_sigpipe() -> Option<SigpipeDisposition> {
    // SAFETY: 書き込み先は初期化済みの sigaction。新しい扱いは渡さない (読むだけ)。
    unsafe {
        let mut old: libc::sigaction = std::mem::zeroed();
        (libc::sigaction(libc::SIGPIPE, std::ptr::null(), &mut old) == 0)
            .then_some(SigpipeDisposition(old))
    }
}

/// `exec` に失敗したとき、`save_sigpipe` で取っておいた扱いにそのまま戻す。
#[cfg(unix)]
pub fn restore_sigpipe(saved: &SigpipeDisposition) {
    // SAFETY: 同じプロセスで sigaction が返した値をそのまま設定し直すだけ。
    unsafe {
        libc::sigaction(libc::SIGPIPE, &saved.0, std::ptr::null_mut());
    }
}

/// Go は nil のスライスや map を `null` で書く (ログイン前の `"TailscaleIPs": null` など)。
/// `#[serde(default)]` は項目がないときにしか効かないので、`null` も既定値として読む。
fn null_as_default<'de, D, T>(d: D) -> Result<T, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Default + Deserialize<'de>,
{
    Ok(Option::<T>::deserialize(d)?.unwrap_or_default())
}

#[derive(Deserialize)]
struct StatusJson {
    #[serde(rename = "BackendState", default, deserialize_with = "null_as_default")]
    backend_state: String,
    #[serde(rename = "AuthURL", default, deserialize_with = "null_as_default")]
    auth_url: String,
    #[serde(rename = "TailscaleIPs", default, deserialize_with = "null_as_default")]
    ips: Vec<String>,
    #[serde(rename = "Self")]
    this: Option<SelfJson>,
}

#[derive(Deserialize)]
struct SelfJson {
    #[serde(rename = "DNSName", default, deserialize_with = "null_as_default")]
    dns_name: String,
    #[serde(rename = "TailscaleIPs", default, deserialize_with = "null_as_default")]
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

#[derive(Deserialize)]
struct PeersJson {
    #[serde(rename = "Peer", default)]
    peers: Option<std::collections::BTreeMap<String, PeerJson>>,
}

#[derive(Deserialize)]
struct PeerJson {
    #[serde(rename = "HostName", default, deserialize_with = "null_as_default")]
    host_name: String,
    #[serde(rename = "DNSName", default, deserialize_with = "null_as_default")]
    dns_name: String,
    #[serde(rename = "TailscaleIPs", default, deserialize_with = "null_as_default")]
    ips: Vec<String>,
}

impl PeerJson {
    fn ips(&self) -> Vec<IpAddr> {
        self.ips.iter().filter_map(|ip| ip.parse().ok()).collect()
    }

    /// 完全な名前か IP が一致する。
    fn matches_exactly(&self, host: &str, ip: Option<IpAddr>) -> bool {
        let fqdn = self.dns_name.trim_end_matches('.');
        (!fqdn.is_empty() && fqdn.eq_ignore_ascii_case(host))
            || ip.is_some_and(|ip| self.ips().contains(&ip))
    }

    /// 短い名前 (DNSName の最初の点より前) か HostName が一致する。
    fn matches_short(&self, host: &str) -> bool {
        let short = self.dns_name.split('.').next().unwrap_or_default();
        (!short.is_empty() && short.eq_ignore_ascii_case(host))
            || (!self.host_name.is_empty() && self.host_name.eq_ignore_ascii_case(host))
    }

    fn address(&self) -> Option<IpAddr> {
        let ips = self.ips();
        ips.iter()
            .find(|ip| ip.is_ipv4())
            .or_else(|| ips.first())
            .copied()
    }
}

/// `/localapi/v0/status` (peers 付き) の JSON から、`host` に当たる端末の IP を探す。
/// 完全な名前か IP の一致を、短い名前の一致より先に見る。どれにも当たらなければ `None`。
pub fn peer_addr_in_status(json: &str, host: &str) -> Option<IpAddr> {
    let host = host
        .trim()
        .trim_start_matches('[')
        .trim_end_matches(']')
        .trim_end_matches('.');
    if host.is_empty() {
        return None;
    }
    let ip = host.parse::<IpAddr>().ok();
    let status: PeersJson = serde_json::from_str(json).ok()?;
    let peers: Vec<PeerJson> = status.peers.unwrap_or_default().into_values().collect();
    let found = peers
        .iter()
        .find(|p| p.matches_exactly(host, ip))
        .or_else(|| {
            // IP の形をした名前は、名前としては比べない。
            ip.is_none()
                .then(|| peers.iter().find(|p| p.matches_short(host)))
                .flatten()
        })?;
    found.address()
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
    /// 端末そのもの。手放す (drop) と libtailscale が閉じる。テストでは偽物。
    node: Option<Node>,
    running: bool,
    /// 利用者がログインを押した。ログイン URL が分かったら開く。`stop` で下ろす。
    wants_login: bool,
    /// 直近に開いたログイン URL。`login` と起動スレッドで共有し、同じ URL を二重に開かない。
    /// ログインを押し直したとき、`stop` のときに下ろす。
    opened_login: Option<String>,
    /// 起動、停止のたびに増やす。状態の書き込みと同じロックの中で比べる。
    generation: u64,
    /// 直近の起動スレッド。次の起動はこれの終了を待ってから始める。
    thread: Option<JoinHandle<()>>,
}

/// 起動した端末。手放すと閉じる。
type Node = Box<dyn Send>;
type BootFn = Arc<dyn Fn(&Path) -> Result<(Node, Loopback), String> + Send + Sync>;

pub struct EmbeddedTailnet {
    notify: Notifier,
    /// 端末を起こす処理。テストでは本物の端末を起こさないものに差し替える。
    boot: BootFn,
    config: Mutex<Option<Config>>,
    inner: Arc<Mutex<Inner>>,
    /// `start`、`login`、`logout`、`forget`、`stop` を1つずつにする (状態ディレクトリを
    /// 消している間に別の起動が始まらないように)。`inner` より先に取る。
    /// メインスレッドでは、これを持ったまま libtailscale (Go) を呼ばない。メインスレッドから
    /// 呼ばれうるのは終了時の `stop` だけで、`stop` は状態を書き換える間だけこれを持ち、
    /// 端末を閉じる (`drop`、Go の呼び出し) のは手放した後。待つのも `STOP_LOCK_WAIT` まで
    /// (ログアウト中でも終了を止めない。取れなくても世代の確認で安全に止まる)。
    /// 端末を起こす Go の呼び出しは、いつも起動スレッドで行う。`logout` と `forget` は
    /// メインスレッド以外で呼ばれ、端末を閉じ終えるまでこれを持つ。
    lifecycle: Mutex<()>,
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
            boot: Arc::new(boot),
            config: Mutex::new(None),
            inner: Arc::new(Mutex::new(Inner::default())),
            lifecycle: Mutex::new(()),
        }
    }

    fn lock_lifecycle(&self) -> std::sync::MutexGuard<'_, ()> {
        self.lifecycle.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// `stop` 用。長く待たずに取れなければ `None` (それでも止めてよい)。
    fn try_lock_lifecycle(&self, wait: Duration) -> Option<std::sync::MutexGuard<'_, ()>> {
        let deadline = Instant::now() + wait;
        loop {
            match self.lifecycle.try_lock() {
                Ok(g) => return Some(g),
                Err(std::sync::TryLockError::Poisoned(e)) => return Some(e.into_inner()),
                Err(std::sync::TryLockError::WouldBlock) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(20));
                }
                Err(std::sync::TryLockError::WouldBlock) => return None,
            }
        }
    }

    /// 端末を起こす (`lifecycle` を持って呼ぶ)。`interactive` なら、ログイン URL が分かったら開く。
    fn start_locked(&self, interactive: bool) -> AppResult<()> {
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
                // 起動中なら、ログイン URL が分かったときに開くよう頼むだけ。
                g.wants_login |= interactive;
                if interactive {
                    g.opened_login = None;
                }
                return Ok(());
            }
            g.running = true;
            g.wants_login = interactive;
            g.opened_login = None;
            g.generation += 1;
            g.state = Some(TailnetState::Starting);
            (g.generation, g.thread.take())
        };
        (self.notify)(EVENT_SYNC);
        // 同じ状態ディレクトリで 2 つの端末を起こさない。古い世代は自分で終わる。
        if let Some(previous) = previous {
            let _ = previous.join();
        }
        // 待つ間に stop や新しい start があれば、この起動はもう古い。起こさず、新しい方の
        // ハンドルも上書きしない。起動と記録は同じロックの中で行う (スレッドは最初にこのロックを待つ)。
        let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if g.generation != gen_id {
            return Ok(());
        }
        let inner = self.inner.clone();
        let notify = self.notify.clone();
        let boot = self.boot.clone();
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
                        // 利用者がログインを押したときだけ開く。裏の起動では開かない。
                        // `login` が開いた URL は、ここで開き直さない。
                        let to_open = url.filter(|url| {
                            let mut g = inner.lock().unwrap_or_else(|e| e.into_inner());
                            let fresh = g.wants_login && g.opened_login.as_deref() != Some(url);
                            if fresh {
                                g.opened_login = Some(url.clone());
                            }
                            fresh
                        });
                        if let Some(url) = to_open {
                            open(&url);
                        }
                    }
                }
                sleep_while_current(&inner, gen_id);
            }
        });
        g.thread = Some(handle);
        Ok(())
    }

    /// 止める (`lifecycle` を持って呼ぶか、持てなかったとき)。端末は呼び出し元が閉じる。
    fn stop_state(&self) -> Option<Node> {
        let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        g.generation += 1;
        g.running = false;
        g.wants_login = false;
        g.opened_login = None;
        g.loopback = None;
        g.state = Some(TailnetState::Stopped);
        g.node.take()
    }

    /// `stop` の中身。端末を閉じる (Go を呼ぶ) のは、呼び出し元が `lifecycle` を手放した後。
    fn stop_locked(&self) {
        let node = self.stop_state();
        drop(node);
        (self.notify)(EVENT_SYNC);
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

fn boot(dir: &Path) -> Result<(Node, Loopback), String> {
    if BOOT_REFUSED.load(Ordering::SeqCst) {
        return Err(BOOT_REFUSED_REASON.to_string());
    }
    prepare_dir(dir).map_err(|e| e.to_string())?;
    let mut ts = libtailscale::Tailscale::new();
    ts.set_dir(&dir.to_string_lossy())?;
    ts.set_hostname(HOSTNAME)?;
    ts.set_logfd(-1)?;
    ts.start()?;
    let lb = ts.loopback()?;
    Ok((
        Box::new(ts),
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
        let _life = self.lock_lifecycle();
        self.start_locked(false)
    }

    fn has_saved_state(&self) -> bool {
        self.dir().is_some_and(|d| d.join(STATE_FILE).is_file())
    }

    fn peer_addr(&self, host: &str) -> Option<IpAddr> {
        if !matches!(self.state(), TailnetState::Running { .. }) {
            return None;
        }
        let lb = self.loopback()?;
        match localapi(&lb, "GET", "/localapi/v0/status", LOCALAPI_TIMEOUT) {
            Ok((200, body)) => peer_addr_in_status(&body, host),
            _ => None,
        }
    }

    fn login(&self) -> AppResult<()> {
        let open = self
            .config
            .lock()
            .expect("tailnet config lock")
            .as_ref()
            .map(|c| c.open.clone())
            .ok_or_else(|| AppError::from("Tailscale is not configured".to_string()))?;
        let pending = {
            let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
            match (&g.state, g.running) {
                (Some(TailnetState::NeedsLogin { url }), true) => {
                    let url = url.clone();
                    g.wants_login = true;
                    g.opened_login = Some(url.clone());
                    Some(url)
                }
                _ => None,
            }
        };
        // 起動スレッドは同じ URL を 1 回しか開かない。ボタンを押し直したら、ここで開き直す。
        // 端末も世代もそのまま (実行中の同期を止めない)。
        if let Some(url) = pending {
            open(&url);
            return Ok(());
        }
        let _life = self.lock_lifecycle();
        self.start_locked(true)
    }

    fn forget(&self) -> AppResult<()> {
        let dir = self
            .dir()
            .ok_or_else(|| AppError::from("Tailscale is not configured".to_string()))?;
        let _life = self.lock_lifecycle();
        let node = self.stop_state();
        // 起動中のスレッドが状態ディレクトリに書き終えるのを待ってから消す。新しい start は
        // lifecycle を待つので、ここで消している間に起動は始まらない。
        let thread = self
            .inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .thread
            .take();
        if let Some(thread) = thread {
            let _ = thread.join();
        }
        drop(node);
        (self.notify)(EVENT_SYNC);
        if dir.exists() {
            std::fs::remove_dir_all(&dir).map_err(|e| AppError::from(e.to_string()))?;
        }
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
        let _life = self.lock_lifecycle();
        // 状態ディレクトリがなければ、登録されたものもない。
        if dir.exists() {
            let lb = match self.loopback() {
                Some(lb) => lb,
                None => {
                    self.start_locked(false)?;
                    self.wait_for_loopback(LOGOUT_BOOT_TIMEOUT)
                        .inspect_err(|_| {
                            self.stop_locked();
                        })?
                }
            };
            let res = localapi(&lb, "POST", "/localapi/v0/logout", LOGOUT_TIMEOUT);
            check_logout(res)?;
        }
        self.stop_locked();
        if dir.exists() {
            std::fs::remove_dir_all(&dir).map_err(|e| AppError::from(e.to_string()))?;
        }
        Ok(())
    }

    fn stop(&self) {
        let node = {
            let _life = self.try_lock_lifecycle(STOP_LOCK_WAIT);
            self.stop_state()
        };
        // drop が libtailscale の端末を閉じる。どのロックも持たずに行う。
        drop(node);
        (self.notify)(EVENT_SYNC);
    }
}

/// テスト用の tailnet の status。NAS が1台 (HostName `takeuchi-nas`)、ほかに端末が1台。
#[cfg(test)]
pub const FAKE_STATUS: &str = r#"{"BackendState":"Running","Peer":{
  "nodekey:aa":{"HostName":"takeuchi-nas","DNSName":"takeuchi-nas.tail1234.ts.net.",
    "TailscaleIPs":["100.100.1.5","fd7a:115c:a1e0::5"]},
  "nodekey:bb":{"HostName":"phone","DNSName":"phone.tail1234.ts.net.",
    "TailscaleIPs":["100.100.1.9"]}}}"#;

/// テスト用。状態を外から変えられる。起動とログイン画面を開いた回数を記録する。
#[cfg(test)]
pub struct FakeTailnet {
    state: Mutex<TailnetState>,
    proxy: Mutex<String>,
    pub status: Mutex<String>,
    pub saved_state: AtomicBool,
    pub starts: std::sync::atomic::AtomicUsize,
    pub stops: std::sync::atomic::AtomicUsize,
    pub opened: Mutex<Vec<String>>,
}

#[cfg(test)]
impl FakeTailnet {
    pub fn new(state: TailnetState) -> Self {
        Self {
            state: Mutex::new(state),
            proxy: Mutex::new("socks5h://tsnet:fake@127.0.0.1:1".to_string()),
            status: Mutex::new(FAKE_STATUS.to_string()),
            saved_state: AtomicBool::new(false),
            starts: Default::default(),
            stops: Default::default(),
            opened: Mutex::new(Vec::new()),
        }
    }

    pub fn set_state(&self, state: TailnetState) {
        *self.state.lock().unwrap() = state;
    }
}

#[cfg(test)]
impl Tailnet for FakeTailnet {
    fn state(&self) -> TailnetState {
        self.state.lock().unwrap().clone()
    }
    /// 裏の起動。ログイン画面は開かない。
    fn start(&self) -> AppResult<()> {
        self.starts.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
    fn has_saved_state(&self) -> bool {
        self.saved_state.load(Ordering::SeqCst)
    }
    fn peer_addr(&self, host: &str) -> Option<IpAddr> {
        if !matches!(self.state(), TailnetState::Running { .. }) {
            return None;
        }
        peer_addr_in_status(&self.status.lock().unwrap(), host)
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
        self.stops.fetch_add(1, Ordering::SeqCst);
        self.set_state(TailnetState::Stopped);
    }
    fn login(&self) -> AppResult<()> {
        if let TailnetState::NeedsLogin { url } = self.state() {
            self.opened.lock().unwrap().push(url);
            return Ok(());
        }
        self.start()
    }
    fn forget(&self) -> AppResult<()> {
        self.set_state(TailnetState::Stopped);
        Ok(())
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

    /// 本物の LocalAPI (libtailscale 0.2.2) が返した NeedsLogin の status (値は伏せた)。
    /// Go の nil スライスは `null` になる (`"TailscaleIPs": null`、`"Peer": null`)。
    const REAL_NEEDS_LOGIN: &str = include_str!("testdata/status_needs_login.json");
    const REAL_NEEDS_LOGIN_NO_URL: &str = include_str!("testdata/status_needs_login_no_url.json");

    #[test]
    fn the_real_needs_login_status_carries_the_auth_url() {
        assert_eq!(
            state_from_status(REAL_NEEDS_LOGIN),
            TailnetState::NeedsLogin {
                url: "https://login.tailscale.com/a/xyz".into()
            }
        );
        assert_eq!(
            state_from_status(REAL_NEEDS_LOGIN_NO_URL),
            TailnetState::Starting
        );
    }

    #[test]
    fn a_peer_with_null_ips_does_not_hide_the_others() {
        let j = r#"{"BackendState":"Running","Peer":{
          "nodekey:aa":{"HostName":"takeuchi-nas","DNSName":"takeuchi-nas.tail1234.ts.net.",
            "TailscaleIPs":["100.100.1.5"]},
          "nodekey:cc":{"HostName":"old","DNSName":"","TailscaleIPs":null}}}"#;
        assert_eq!(
            peer_addr_in_status(j, "takeuchi-nas"),
            Some("100.100.1.5".parse().unwrap())
        );
    }

    #[test]
    fn the_poll_reaches_needs_login_with_the_real_status() {
        let (_dir, t, _) = fake_node(REAL_NEEDS_LOGIN);
        t.start().unwrap();
        wait_until("needs login from the real status", || {
            matches!(t.state(), TailnetState::NeedsLogin { .. })
        });
        t.stop();
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

    fn peer(host: &str) -> Option<IpAddr> {
        peer_addr_in_status(FAKE_STATUS, host)
    }

    #[test]
    fn a_peer_is_found_by_name_or_tailscale_ip() {
        let nas: IpAddr = "100.100.1.5".parse().unwrap();
        for host in [
            "takeuchi-nas",
            "TAKEUCHI-NAS",
            "takeuchi-nas.tail1234.ts.net",
            "takeuchi-nas.tail1234.ts.net.",
            "100.100.1.5",
            "fd7a:115c:a1e0::5",
            "[fd7a:115c:a1e0::5]",
        ] {
            assert_eq!(peer(host), Some(nas), "{host}");
        }
        assert_eq!(peer("phone"), Some("100.100.1.9".parse().unwrap()));
    }

    #[test]
    fn hosts_outside_the_tailnet_are_refused() {
        for host in [
            "192.168.1.10",
            "10.0.0.5",
            "100.100.1.99",
            "nas.local",
            "takeuchi-nas.example.com",
            "takeuchi-nas.tail9999.ts.net",
            "tail1234",
            "",
        ] {
            assert_eq!(peer(host), None, "{host:?}");
        }
        assert_eq!(peer_addr_in_status("not json", "takeuchi-nas"), None);
        assert_eq!(
            peer_addr_in_status(r#"{"BackendState":"Running"}"#, "takeuchi-nas"),
            None
        );
    }

    #[test]
    fn the_fake_never_opens_the_login_page_on_a_background_start() {
        let fake = FakeTailnet::new(TailnetState::NeedsLogin { url: "u".into() });
        fake.start().unwrap();
        assert!(fake.opened.lock().unwrap().is_empty());
        fake.login().unwrap();
        assert_eq!(fake.opened.lock().unwrap().as_slice(), ["u"]);
    }

    #[test]
    fn saved_state_means_the_state_file_exists() {
        let base = tempfile::tempdir().unwrap();
        let dir = base.path().join("tailscale");
        let (t, _, _) = fake_booted(dir.clone());
        assert!(!t.has_saved_state());
        std::fs::create_dir_all(&dir).unwrap();
        assert!(!t.has_saved_state(), "an empty directory is not a login");
        std::fs::write(dir.join("tailscaled.state"), b"keys").unwrap();
        assert!(t.has_saved_state());
        assert!(!embedded().has_saved_state(), "not configured");
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

    type Opened = Arc<Mutex<Vec<String>>>;

    /// 本物の端末を起こさない `EmbeddedTailnet`。起動は呼ばれたスレッドを記録して失敗する。
    fn fake_booted(
        dir: PathBuf,
    ) -> (
        Arc<EmbeddedTailnet>,
        Opened,
        Arc<Mutex<Vec<std::thread::ThreadId>>>,
    ) {
        let booted = Arc::new(Mutex::new(Vec::new()));
        let log = booted.clone();
        let mut t = embedded();
        t.boot = Arc::new(move |_: &Path| {
            log.lock().unwrap().push(std::thread::current().id());
            Err("no node in tests".to_string())
        });
        let opened: Opened = Arc::new(Mutex::new(Vec::new()));
        let sink = opened.clone();
        t.configure(
            dir,
            Arc::new(move |u: &str| sink.lock().unwrap().push(u.to_string())),
        );
        (Arc::new(t), opened, booted)
    }

    fn wait_until(what: &str, mut f: impl FnMut() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while !f() {
            assert!(Instant::now() < deadline, "timed out waiting for {what}");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    /// LocalAPI の偽物。どの要求にも `status` の JSON を返す。本物の端末は起こさない。
    fn fake_localapi(status: &'static str) -> String {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap().to_string();
        std::thread::spawn(move || {
            for conn in listener.incoming() {
                let Ok(mut conn) = conn else { return };
                let mut buf = [0u8; 4096];
                let _ = conn.read(&mut buf);
                let _ = write!(conn, "HTTP/1.0 200 OK\r\n\r\n{status}");
            }
        });
        address
    }

    const NEEDS_LOGIN: &str =
        r#"{"BackendState":"NeedsLogin","AuthURL":"https://login.tailscale.com/a/xyz"}"#;

    /// 偽の端末と LocalAPI で「起動」する `EmbeddedTailnet`。開いた URL を記録する。
    fn fake_node(status: &'static str) -> (tempfile::TempDir, Arc<EmbeddedTailnet>, Opened) {
        let address = fake_localapi(status);
        let dir = tempfile::tempdir().unwrap();
        let mut t = embedded();
        t.boot = Arc::new(move |_: &Path| {
            let node: Node = Box::new(());
            Ok((
                node,
                Loopback {
                    address: address.clone(),
                    credential: "c".into(),
                    proxy_user: "tsnet".into(),
                    proxy_credential: "p".into(),
                },
            ))
        });
        let opened: Opened = Arc::new(Mutex::new(Vec::new()));
        let sink = opened.clone();
        t.configure(
            dir.path().join("tailscale"),
            Arc::new(move |u: &str| sink.lock().unwrap().push(u.to_string())),
        );
        (dir, Arc::new(t), opened)
    }

    #[test]
    fn a_background_start_never_opens_the_login_page() {
        let (_dir, t, opened) = fake_node(NEEDS_LOGIN);
        t.start().unwrap();
        wait_until("needs login", || {
            matches!(t.state(), TailnetState::NeedsLogin { .. })
        });
        // 次の問い合わせ (2 秒ごと) を越えて待っても開かない。
        std::thread::sleep(POLL_INTERVAL + Duration::from_millis(300));
        assert!(opened.lock().unwrap().is_empty(), "{opened:?}");
        // 押されたときだけ開く。開き直しでは端末を起こし直さない。
        let generation = t.inner.lock().unwrap().generation;
        t.login().unwrap();
        assert_eq!(
            opened.lock().unwrap().as_slice(),
            ["https://login.tailscale.com/a/xyz"]
        );
        assert_eq!(t.inner.lock().unwrap().generation, generation);
        // 次の問い合わせが同じ URL をもう一度開かない。
        std::thread::sleep(POLL_INTERVAL + Duration::from_millis(300));
        assert_eq!(opened.lock().unwrap().len(), 1, "opened once");
        t.stop();
    }

    #[test]
    fn login_while_starting_opens_the_page_once_it_is_known() {
        let (_dir, t, opened) = fake_node(NEEDS_LOGIN);
        t.login().unwrap();
        wait_until("the login page", || opened.lock().unwrap().len() == 1);
        std::thread::sleep(POLL_INTERVAL + Duration::from_millis(300));
        assert_eq!(opened.lock().unwrap().len(), 1, "opened once");
        t.stop();
    }

    #[test]
    fn login_opens_a_pending_login_url_again() {
        let dir = tempfile::tempdir().unwrap();
        let (t, opened, booted) = fake_booted(dir.path().join("tailscale"));
        let url = "https://login.tailscale.com/a/abc";
        {
            let mut g = t.inner.lock().unwrap();
            g.running = true;
            g.generation = 1;
            g.state = Some(TailnetState::NeedsLogin { url: url.into() });
        }
        t.login().unwrap();
        assert_eq!(opened.lock().unwrap().as_slice(), [url]);
        assert_eq!(t.inner.lock().unwrap().generation, 1, "no new boot");
        assert!(booted.lock().unwrap().is_empty());
    }

    #[test]
    fn a_late_start_does_not_replace_a_newer_boot_thread() {
        let dir = tempfile::tempdir().unwrap();
        let (t, _, booted) = fake_booted(dir.path().join("tailscale"));
        // 前の起動スレッドが終わらないうちに、2 つ目の start を走らせる。
        let (release, wait) = std::sync::mpsc::channel::<()>();
        t.inner.lock().unwrap().thread = Some(std::thread::spawn(move || {
            let _ = wait.recv();
        }));
        let late = {
            let t = t.clone();
            std::thread::spawn(move || t.start().unwrap())
        };
        wait_until("the late start to take the old thread", || {
            let g = t.inner.lock().unwrap();
            g.generation == 1 && g.thread.is_none()
        });
        // その間に止める (終了時と同じく、待ちきれなくても止まる)。遅れた start は起こさない。
        t.stop();
        release.send(()).unwrap();
        late.join().unwrap();
        assert!(
            booted.lock().unwrap().is_empty(),
            "the late start never booted"
        );
        assert!(t.inner.lock().unwrap().thread.is_none());
        // 新しい start は、遅れた start が終わってから起こす。
        t.start().unwrap();
        wait_until("the new boot", || booted.lock().unwrap().len() == 1);
        let newest = booted.lock().unwrap()[0];
        let stored = t
            .inner
            .lock()
            .unwrap()
            .thread
            .as_ref()
            .map(|h| h.thread().id());
        assert_eq!(stored, Some(newest));
    }

    #[test]
    fn start_waits_for_a_forget_in_progress() {
        let base = tempfile::tempdir().unwrap();
        let dir = base.path().join("tailscale");
        std::fs::create_dir_all(&dir).unwrap();
        let (t, _, booted) = fake_booted(dir.clone());
        let held = t.lock_lifecycle();
        let starter = {
            let t = t.clone();
            std::thread::spawn(move || t.start().unwrap())
        };
        std::thread::sleep(Duration::from_millis(100));
        assert_eq!(t.inner.lock().unwrap().generation, 0, "start waits");
        drop(held);
        starter.join().unwrap();
        wait_until("the boot", || booted.lock().unwrap().len() == 1);
    }

    #[test]
    fn the_no_logs_switch_parses_like_go_parse_bool() {
        use std::ffi::OsStr;
        for yes in ["1", "t", "T", "TRUE", "true", "True"] {
            assert!(no_logs_env_is_true(Some(OsStr::new(yes))), "{yes}");
        }
        for no in [
            "", "0", "f", "false", "FALSE", "yes", "on", " true", "tRUE", "2",
        ] {
            assert!(!no_logs_env_is_true(Some(OsStr::new(no))), "{no}");
        }
        assert!(!no_logs_env_is_true(None));
    }

    #[test]
    fn boot_refuses_when_log_uploads_could_not_be_disabled() {
        /// パニックしても、拒否を他のテストに残さない。
        struct Reset;
        impl Drop for Reset {
            fn drop(&mut self) {
                reset_boot_refusal();
            }
        }
        let _reset = Reset;
        refuse_boot();
        // 状態ディレクトリは作れない場所。拒否が先なら、そこまで進まない。
        match boot(Path::new("/dev/null/tailscale")) {
            Err(reason) => assert_eq!(reason, BOOT_REFUSED_REASON),
            Ok(_) => panic!("booted"),
        }
        let t = embedded();
        t.configure(PathBuf::from("/dev/null/tailscale"), Arc::new(|_: &str| {}));
        t.start().unwrap();
        wait_until("the refusal", || {
            matches!(t.state(), TailnetState::Failed { .. })
        });
        assert_eq!(
            t.state(),
            TailnetState::Failed {
                reason: BOOT_REFUSED_REASON.into()
            }
        );
    }

    #[cfg(unix)]
    #[test]
    fn the_exact_sigpipe_disposition_comes_back_after_a_failed_exec() {
        extern "C" fn handler(_: libc::c_int) {}
        let original = save_sigpipe().unwrap();
        // Go のランタイムが入れるような、SIG_IGN ではないハンドラとフラグ。
        unsafe {
            let mut custom: libc::sigaction = std::mem::zeroed();
            custom.sa_sigaction = handler as *const () as usize;
            custom.sa_flags = libc::SA_ONSTACK | libc::SA_RESTART;
            libc::sigaction(libc::SIGPIPE, &custom, std::ptr::null_mut());
        }
        let saved = save_sigpipe().unwrap();
        // `exec` が呼ぶ前にするのと同じ: 既定に戻す。
        unsafe { libc::signal(libc::SIGPIPE, libc::SIG_DFL) };
        restore_sigpipe(&saved);
        let now = save_sigpipe().unwrap();
        restore_sigpipe(&original);
        assert_eq!(now.0.sa_sigaction, handler as *const () as usize);
        assert_eq!(
            now.0.sa_flags & (libc::SA_ONSTACK | libc::SA_RESTART),
            libc::SA_ONSTACK | libc::SA_RESTART
        );
    }

    #[test]
    fn forget_stops_and_removes_the_state_dir_without_logging_out() {
        let base = tempfile::tempdir().unwrap();
        let dir = base.path().join("tailscale");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("tailscaled.state"), b"keys").unwrap();
        let (t, _, _) = fake_booted(dir.clone());
        {
            let mut g = t.inner.lock().unwrap();
            g.running = true;
            g.generation = 1;
            g.state = Some(TailnetState::Failed { reason: "x".into() });
        }
        t.forget().unwrap();
        assert!(!dir.exists());
        assert_eq!(t.state(), TailnetState::Stopped);
        assert!(!t.inner.lock().unwrap().running);
        // もう何もなくても成功する。
        t.forget().unwrap();
        assert!(embedded().forget().is_err(), "not configured");
    }
}
