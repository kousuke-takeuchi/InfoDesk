use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, BufReader};
use tokio::process::{Child, Command};

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum ChatEndpoint {
    Starting,
    Ready { url: String, token: String },
    Error { kind: String, message: String },
}

/// GUI から起動したアプリの PATH は短いので、Homebrew の場所も探す。
pub fn default_extra_path() -> Vec<PathBuf> {
    vec![
        PathBuf::from("/opt/homebrew/bin"),
        PathBuf::from("/usr/local/bin"),
    ]
}

pub fn parse_node_major(out: &str) -> Option<u32> {
    out.trim()
        .strip_prefix('v')?
        .split('.')
        .next()?
        .parse()
        .ok()
}

pub fn parse_ready_line(line: &str) -> Option<u16> {
    let json = line.trim().strip_prefix("INFODESK_AGENT_READY ")?;
    serde_json::from_str::<serde_json::Value>(json)
        .ok()?
        .get("port")?
        .as_u64()?
        .try_into()
        .ok()
}

fn is_executable(p: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(p).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

pub fn resolve_bin(name: &str, path_env: &str, extra: &[PathBuf]) -> Option<PathBuf> {
    if name.contains('/') {
        let p = PathBuf::from(name);
        return is_executable(&p).then_some(p);
    }
    std::env::split_paths(path_env)
        .chain(extra.iter().cloned())
        .map(|dir| dir.join(name))
        .find(|p| is_executable(p))
}

pub fn new_token() -> String {
    use std::io::Read;
    let mut bytes = [0u8; 16];
    std::fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut bytes))
        .expect("read /dev/urandom");
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

const MIN_NODE_MAJOR: u32 = 22;

pub struct LaunchConfig {
    pub node: String,
    pub agent: PathBuf,
    pub cli: PathBuf,
    pub db: PathBuf,
    pub token: String,
    pub extra_path: Vec<PathBuf>,
}

#[derive(Debug)]
pub struct LaunchError {
    pub kind: &'static str,
    pub message: String,
}

fn err(kind: &'static str, message: String) -> LaunchError {
    LaunchError { kind, message }
}

fn child_path(extra: &[PathBuf]) -> String {
    let current = std::env::var("PATH").unwrap_or_default();
    let mut dirs: Vec<PathBuf> = std::env::split_paths(&current).collect();
    for d in extra {
        if !dirs.contains(d) {
            dirs.push(d.clone());
        }
    }
    std::env::join_paths(dirs)
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or(current)
}

pub async fn launch(
    cfg: &LaunchConfig,
    ready_timeout: Duration,
) -> Result<(Child, u16), LaunchError> {
    let path_env = std::env::var("PATH").unwrap_or_default();
    let node = resolve_bin(&cfg.node, &path_env, &cfg.extra_path).ok_or_else(|| {
        err(
            "node_missing",
            format!(
                "Node が見つかりません ({})。チャットには Node {MIN_NODE_MAJOR} 以上が必要です。",
                cfg.node
            ),
        )
    })?;
    let version = tokio::time::timeout(
        Duration::from_secs(5),
        Command::new(&node).arg("--version").output(),
    )
    .await
    .map_err(|_| {
        err(
            "node_missing",
            format!("Node の版を確かめられません ({})", node.display()),
        )
    })?
    .map_err(|e| {
        err(
            "node_missing",
            format!("Node を実行できません ({}): {e}", node.display()),
        )
    })?;
    let text = String::from_utf8_lossy(&version.stdout).to_string();
    match parse_node_major(&text) {
        Some(m) if m >= MIN_NODE_MAJOR => {}
        Some(m) => {
            return Err(err(
                "node_too_old",
                format!(
                    "Node {m} は古すぎます ({})。チャットには Node {MIN_NODE_MAJOR} 以上が必要です。",
                    node.display()
                ),
            ));
        }
        None => {
            return Err(err(
                "node_missing",
                format!(
                    "Node の版を読めません ({}): {}",
                    node.display(),
                    text.trim()
                ),
            ));
        }
    }
    if !cfg.agent.is_file() {
        return Err(err(
            "agent_missing",
            format!(
                "エージェントのファイルがありません ({})。agent で npm run build を実行してください。",
                cfg.agent.display()
            ),
        ));
    }
    let mut child = Command::new(&node)
        .arg(&cfg.agent)
        .env("INFODESK_CLI", &cfg.cli)
        .env("INFODESK_CLI_ARG", "--cli")
        .env("INFODESK_DB", &cfg.db)
        .env("INFODESK_AGENT_TOKEN", &cfg.token)
        .env("INFODESK_AGENT_PORT", "0")
        .env("PATH", child_path(&cfg.extra_path))
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| err("start_failed", format!("エージェントを起動できません: {e}")))?;
    let stdout = child.stdout.take().expect("piped stdout");
    let mut lines = BufReader::new(stdout).lines();
    let wait_ready = async {
        while let Ok(Some(line)) = lines.next_line().await {
            if let Some(port) = parse_ready_line(&line) {
                return Some(port);
            }
        }
        None
    };
    match tokio::time::timeout(ready_timeout, wait_ready).await {
        Ok(Some(port)) => {
            // 準備完了後も出力を読み捨てて、パイプが詰まらないようにする。
            tokio::spawn(async move { while let Ok(Some(_)) = lines.next_line().await {} });
            if let Some(stderr) = child.stderr.take() {
                tokio::spawn(async move {
                    let mut lines = BufReader::new(stderr).lines();
                    while let Ok(Some(line)) = lines.next_line().await {
                        eprintln!("[agent] {line}");
                    }
                });
            }
            Ok((child, port))
        }
        Ok(None) => {
            let mut stderr = String::new();
            if let Some(mut e) = child.stderr.take() {
                let _ = tokio::time::timeout(Duration::from_secs(1), e.read_to_string(&mut stderr))
                    .await;
            }
            let tail: Vec<&str> = stderr
                .lines()
                .rev()
                .take(5)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect();
            Err(err(
                "start_failed",
                format!("エージェントが起動直後に終了しました: {}", tail.join(" / ")),
            ))
        }
        Err(_) => {
            let _ = child.kill().await;
            Err(err(
                "start_failed",
                "エージェントの起動が timed out しました".to_string(),
            ))
        }
    }
}

pub struct Sidecar {
    endpoint: Mutex<ChatEndpoint>,
    child: tokio::sync::Mutex<Option<Child>>,
}

impl Default for Sidecar {
    fn default() -> Self {
        Self {
            endpoint: Mutex::new(ChatEndpoint::Starting),
            child: tokio::sync::Mutex::new(None),
        }
    }
}

impl Sidecar {
    pub fn endpoint(&self) -> ChatEndpoint {
        self.endpoint.lock().expect("endpoint lock").clone()
    }

    fn set(&self, app: &AppHandle, e: ChatEndpoint) {
        *self.endpoint.lock().expect("endpoint lock") = e;
        let _ = app.emit(crate::state::EVENT_CHAT, ());
    }

    /// 動いているサイドカーを止めて、起動し直す。
    pub async fn restart(&self, app: &AppHandle) {
        let mut slot = self.child.lock().await;
        if let Some(mut old) = slot.take() {
            let _ = old.kill().await;
        }
        self.set(app, ChatEndpoint::Starting);
        let cfg = match launch_config(app) {
            Ok(c) => c,
            Err(e) => {
                return self.set(
                    app,
                    ChatEndpoint::Error {
                        kind: e.kind.into(),
                        message: e.message,
                    },
                );
            }
        };
        match launch(&cfg, Duration::from_secs(20)).await {
            Ok((child, port)) => {
                *slot = Some(child);
                self.set(
                    app,
                    ChatEndpoint::Ready {
                        url: format!("http://127.0.0.1:{port}"),
                        token: cfg.token,
                    },
                );
            }
            Err(e) => self.set(
                app,
                ChatEndpoint::Error {
                    kind: e.kind.into(),
                    message: e.message,
                },
            ),
        }
    }

    /// アプリの終了時に呼ぶ。
    pub fn stop(&self) {
        if let Ok(mut slot) = self.child.try_lock()
            && let Some(child) = slot.as_mut()
        {
            let _ = child.start_kill();
        }
    }
}

fn agent_path(app: &AppHandle) -> PathBuf {
    // バンドルでは resources/agent/agent.mjs。開発時はリポジトリの agent/dist/agent.mjs。
    if let Ok(p) = app
        .path()
        .resolve("agent/agent.mjs", tauri::path::BaseDirectory::Resource)
        && p.is_file()
    {
        return p;
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../agent/dist/agent.mjs")
}

fn launch_config(app: &AppHandle) -> Result<LaunchConfig, LaunchError> {
    let state = app.state::<crate::state::AppState>();
    let node = state
        .with_store(|s| s.get_setting_or("node.bin", "node"))
        .map_err(|e| err("start_failed", e.message))?;
    let db = app
        .path()
        .app_data_dir()
        .map_err(|e| err("start_failed", e.to_string()))?
        .join("infodesk.db");
    let cli = std::env::current_exe().map_err(|e| err("start_failed", e.to_string()))?;
    Ok(LaunchConfig {
        node,
        agent: agent_path(app),
        cli,
        db,
        token: new_token(),
        extra_path: default_extra_path(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    use std::path::{Path, PathBuf};
    use std::time::Duration;

    #[test]
    fn parses_node_versions() {
        assert_eq!(parse_node_major("v26.5.0\n"), Some(26));
        assert_eq!(parse_node_major("v22.0.0"), Some(22));
        assert_eq!(parse_node_major("node: command not found"), None);
    }

    #[test]
    fn parses_the_ready_line() {
        assert_eq!(
            parse_ready_line(r#"INFODESK_AGENT_READY {"port":4321}"#),
            Some(4321)
        );
        assert_eq!(parse_ready_line("INFODESK_AGENT_READY {}"), None);
        assert_eq!(parse_ready_line("some log line"), None);
    }

    #[test]
    fn resolves_bare_names_on_path_and_extra_dirs() {
        let a = tempfile::tempdir().unwrap();
        let b = tempfile::tempdir().unwrap();
        let node = b.path().join("node");
        std::fs::write(&node, "").unwrap();
        std::fs::set_permissions(&node, std::fs::Permissions::from_mode(0o755)).unwrap();
        let path_env = a.path().display().to_string();
        assert_eq!(
            resolve_bin("node", &path_env, &[b.path().to_path_buf()]),
            Some(node.clone())
        );
        assert_eq!(resolve_bin("node", &path_env, &[]), None);
        assert_eq!(
            resolve_bin(&node.display().to_string(), "", &[]),
            Some(node.clone())
        );
        assert_eq!(resolve_bin("/no/such/node", "", &[]), None);
    }

    #[test]
    fn tokens_are_random_hex() {
        let a = new_token();
        assert_eq!(a.len(), 32);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(a, new_token());
    }

    fn fake_node(dir: &Path, body: &str) -> String {
        let p = dir.join("node");
        std::fs::write(&p, format!("#!/bin/sh\nif [ \"$1\" = \"--version\" ]; then echo \"${{FAKE_VERSION:-v26.5.0}}\"; exit 0; fi\n{body}\n")).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        p.display().to_string()
    }

    fn config(dir: &Path, node: String) -> LaunchConfig {
        let agent = dir.join("agent.mjs");
        std::fs::write(&agent, "").unwrap();
        LaunchConfig {
            node,
            agent,
            cli: PathBuf::from("/Applications/InfoDesk.app/Contents/MacOS/infodesk-app"),
            db: dir.join("t.db"),
            token: "abc".into(),
            extra_path: vec![],
        }
    }

    #[tokio::test]
    async fn launch_waits_for_the_ready_line_and_passes_the_environment() {
        let dir = tempfile::tempdir().unwrap();
        let node = fake_node(
            dir.path(),
            r#"
[ "$INFODESK_AGENT_TOKEN" = "abc" ] && [ "$INFODESK_CLI_ARG" = "--cli" ] && [ -n "$INFODESK_DB" ] || exit 9
echo "starting"
echo 'INFODESK_AGENT_READY {"port":4321}'
cat > /dev/null"#,
        );
        let (mut child, port) = launch(&config(dir.path(), node), Duration::from_secs(5))
            .await
            .unwrap();
        assert_eq!(port, 4321);
        assert!(
            child.try_wait().unwrap().is_none(),
            "サイドカーは動き続ける"
        );
        child.kill().await.unwrap();
    }

    #[tokio::test]
    async fn launch_reports_missing_and_old_node() {
        let dir = tempfile::tempdir().unwrap();
        let err = launch(
            &config(dir.path(), "/no/such/node".into()),
            Duration::from_secs(1),
        )
        .await
        .unwrap_err();
        assert_eq!(err.kind, "node_missing");
        assert!(err.message.contains("/no/such/node"));
        let node = fake_node(dir.path(), "exit 0");
        let old = std::fs::read_to_string(&node)
            .unwrap()
            .replace("v26.5.0", "v20.11.1");
        std::fs::write(&node, old).unwrap();
        let err = launch(&config(dir.path(), node), Duration::from_secs(1))
            .await
            .unwrap_err();
        assert_eq!(err.kind, "node_too_old");
        assert!(err.message.contains("20"), "{}", err.message);
    }

    #[tokio::test]
    async fn launch_reports_a_missing_agent_bundle() {
        let dir = tempfile::tempdir().unwrap();
        let mut cfg = config(dir.path(), fake_node(dir.path(), "exit 0"));
        cfg.agent = dir.path().join("missing.mjs");
        let err = launch(&cfg, Duration::from_secs(1)).await.unwrap_err();
        assert_eq!(err.kind, "agent_missing");
    }

    #[tokio::test]
    async fn launch_reports_an_early_exit_with_stderr_and_a_timeout() {
        let dir = tempfile::tempdir().unwrap();
        let node = fake_node(
            dir.path(),
            "echo 'Error: Cannot find module zod' >&2\nexit 1",
        );
        let err = launch(&config(dir.path(), node), Duration::from_secs(5))
            .await
            .unwrap_err();
        assert_eq!(err.kind, "start_failed");
        assert!(
            err.message.contains("Cannot find module zod"),
            "{}",
            err.message
        );
        let node = fake_node(dir.path(), "sleep 30");
        let err = launch(&config(dir.path(), node), Duration::from_millis(300))
            .await
            .unwrap_err();
        assert_eq!(err.kind, "start_failed");
        assert!(err.message.contains("timed out"), "{}", err.message);
    }
}
