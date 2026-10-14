//! Google サインイン (PKCE + ループバック) と、更新用トークンからのアクセストークン取得。

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use infodesk_core::sync::drive::DriveError;
use infodesk_core::sync::google::AccessToken;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Manager};
use tauri_plugin_opener::OpenerExt;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

use crate::error::AppResult;
use crate::state::AppState;
use crate::sync::keychain::SecretStore;

pub const SCOPE: &str = "https://www.googleapis.com/auth/drive.appdata openid email";
pub const SETTING_ACCOUNT_EMAIL: &str = "sync.account_email";
const LOGIN_TIMEOUT: Duration = Duration::from_secs(300);
const EXPIRY_MARGIN: u64 = 60;
const HTTP_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const HTTP_TIMEOUT: Duration = Duration::from_secs(30);
const REVOKE_TIMEOUT: Duration = Duration::from_secs(10);
const READ_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Clone)]
pub struct OAuthConfig {
    pub client_id: String,
    pub client_secret: String,
    pub auth_url: String,
    pub token_url: String,
    pub revoke_url: String,
}

impl OAuthConfig {
    pub fn google(client_id: String, client_secret: String) -> Self {
        Self {
            client_id,
            client_secret,
            auth_url: "https://accounts.google.com/o/oauth2/v2/auth".into(),
            token_url: "https://oauth2.googleapis.com/token".into(),
            revoke_url: "https://oauth2.googleapis.com/revoke".into(),
        }
    }
}

/// ビルド時に `google-oauth.json` から埋め込んだ値。どちらかが無ければ `None` (同期は使えない)。
pub fn oauth_config() -> Option<OAuthConfig> {
    let id = option_env!("INFODESK_GOOGLE_CLIENT_ID")?;
    let secret = option_env!("INFODESK_GOOGLE_CLIENT_SECRET")?;
    Some(OAuthConfig::google(id.to_string(), secret.to_string()))
}

fn build_http(connect: Duration, total: Duration) -> reqwest::Client {
    reqwest::Client::builder()
        .connect_timeout(connect)
        .timeout(total)
        .build()
        .expect("reqwest client with timeouts builds")
}

/// OAuth 通信で共通に使うクライアント (接続 10 秒、全体 30 秒)。
pub fn http_client() -> reqwest::Client {
    build_http(HTTP_CONNECT_TIMEOUT, HTTP_TIMEOUT)
}

fn challenge_for(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

/// (verifier, S256 challenge)。verifier は 64 文字の 16 進 (RFC 7636 の 43〜128 文字)。
pub fn pkce_pair() -> (String, String) {
    let verifier = format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    );
    let challenge = challenge_for(&verifier);
    (verifier, challenge)
}

pub fn auth_url(cfg: &OAuthConfig, redirect: &str, challenge: &str, state: &str) -> String {
    let mut u = url::Url::parse(&cfg.auth_url).expect("auth_url is a valid URL");
    u.query_pairs_mut()
        .append_pair("client_id", &cfg.client_id)
        .append_pair("redirect_uri", redirect)
        .append_pair("response_type", "code")
        .append_pair("scope", SCOPE)
        .append_pair("access_type", "offline")
        .append_pair("prompt", "consent")
        .append_pair("code_challenge", challenge)
        .append_pair("code_challenge_method", "S256")
        .append_pair("state", state);
    u.into()
}

#[derive(Debug, PartialEq, Eq)]
pub enum CallbackOutcome {
    Code(String),
    Failed(String),
    /// state が合わない (別のページからの要求)。失敗ページを返して待ち続ける。
    StateMismatch,
    /// コールバックではない要求 (favicon など)。待ち続ける。
    Ignore,
}

/// HTTP のリクエスト行 (`GET /?code=..&state=.. HTTP/1.1`) を解析する。
pub fn parse_callback(request_line: &str, expected_state: &str) -> CallbackOutcome {
    let Some(target) = request_line.split_whitespace().nth(1) else {
        return CallbackOutcome::Ignore;
    };
    let Ok(u) = url::Url::parse(&format!("http://127.0.0.1{target}")) else {
        return CallbackOutcome::Ignore;
    };
    if u.path() != "/" {
        return CallbackOutcome::Ignore;
    }
    let get = |k: &str| {
        u.query_pairs()
            .find(|(n, _)| n == k)
            .map(|(_, v)| v.into_owned())
    };
    let (code, error, state) = (get("code"), get("error"), get("state"));
    if code.is_none() && error.is_none() {
        return CallbackOutcome::Ignore;
    }
    if state.as_deref() != Some(expected_state) {
        return CallbackOutcome::StateMismatch;
    }
    if let Some(e) = error {
        return CallbackOutcome::Failed(format!("Google からエラーが返されました: {e}"));
    }
    match code {
        Some(c) if !c.is_empty() => CallbackOutcome::Code(c),
        _ => CallbackOutcome::Failed("認可コードが返されませんでした".into()),
    }
}

/// 表示用。署名は検証しない。
fn email_from_id_token(id_token: &str) -> Option<String> {
    let payload = id_token.split('.').nth(1)?;
    let bytes = URL_SAFE_NO_PAD.decode(payload.trim_end_matches('=')).ok()?;
    let v: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    v.get("email")?.as_str().map(str::to_string)
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: Option<String>,
    refresh_token: Option<String>,
    expires_in: Option<u64>,
    id_token: Option<String>,
}

async fn exchange_code(
    http: &reqwest::Client,
    cfg: &OAuthConfig,
    code: &str,
    verifier: &str,
    redirect: &str,
) -> Result<(String, Option<String>), String> {
    let resp = http
        .post(&cfg.token_url)
        .form(&[
            ("grant_type", "authorization_code"),
            ("code", code),
            ("code_verifier", verifier),
            ("redirect_uri", redirect),
            ("client_id", cfg.client_id.as_str()),
            ("client_secret", cfg.client_secret.as_str()),
        ])
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!(
            "Google のトークンを取得できませんでした (HTTP {})",
            resp.status().as_u16()
        ));
    }
    let t: TokenResponse = resp.json().await.map_err(|e| e.to_string())?;
    let rt = t
        .refresh_token
        .ok_or_else(|| "Google から更新用トークンが返されませんでした".to_string())?;
    Ok((rt, t.id_token.as_deref().and_then(email_from_id_token)))
}

/// 更新用トークンからアクセストークンを取り、期限の 60 秒前まで覚える。
pub struct TokenSource {
    cfg: OAuthConfig,
    secrets: Arc<dyn SecretStore>,
    http: reqwest::Client,
    cache: Mutex<Option<(String, Instant)>>,
}

impl TokenSource {
    pub fn new(cfg: OAuthConfig, secrets: Arc<dyn SecretStore>) -> Self {
        Self {
            cfg,
            secrets,
            http: http_client(),
            cache: Mutex::new(None),
        }
    }

    #[cfg(test)]
    fn with_client(cfg: OAuthConfig, secrets: Arc<dyn SecretStore>, http: reqwest::Client) -> Self {
        Self {
            cfg,
            secrets,
            http,
            cache: Mutex::new(None),
        }
    }
}

#[async_trait]
impl AccessToken for TokenSource {
    async fn get(&self) -> Result<String, DriveError> {
        if let Some((tok, until)) = self.cache.lock().unwrap().as_ref()
            && Instant::now() < *until
        {
            return Ok(tok.clone());
        }
        let refresh = self
            .secrets
            .get()
            .map_err(DriveError::Other)?
            .ok_or(DriveError::AuthExpired)?;
        let resp = self
            .http
            .post(&self.cfg.token_url)
            .form(&[
                ("grant_type", "refresh_token"),
                ("refresh_token", refresh.as_str()),
                ("client_id", self.cfg.client_id.as_str()),
                ("client_secret", self.cfg.client_secret.as_str()),
            ])
            .send()
            .await
            .map_err(|e| DriveError::Retryable(e.to_string()))?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            if status.is_client_error()
                && serde_json::from_str::<serde_json::Value>(&body)
                    .ok()
                    .and_then(|v| v.get("error")?.as_str().map(|e| e == "invalid_grant"))
                    == Some(true)
            {
                return Err(DriveError::AuthExpired);
            }
            return Err(if status.is_server_error() || status.as_u16() == 429 {
                DriveError::Retryable(format!("token refresh: HTTP {status}"))
            } else {
                DriveError::Other(format!("token refresh: HTTP {status}"))
            });
        }
        let t: TokenResponse = resp.json().await.map_err(|e| {
            if e.is_timeout() {
                DriveError::Retryable(e.to_string())
            } else {
                DriveError::Other(e.to_string())
            }
        })?;
        let tok = t
            .access_token
            .ok_or_else(|| DriveError::Other("no access_token in response".into()))?;
        let life = t.expires_in.unwrap_or(0).saturating_sub(EXPIRY_MARGIN);
        *self.cache.lock().unwrap() =
            Some((tok.clone(), Instant::now() + Duration::from_secs(life)));
        Ok(tok)
    }

    async fn invalidate(&self) {
        *self.cache.lock().unwrap() = None;
    }
}

const PAGE_OK: &str = "<!doctype html><meta charset=\"utf-8\"><title>InfoDesk</title><p>ログインしました。InfoDesk に戻ってください。</p>";
const PAGE_NG: &str = "<!doctype html><meta charset=\"utf-8\"><title>InfoDesk</title><p>ログインに失敗しました。InfoDesk に戻ってやり直してください。</p>";

async fn respond(stream: &mut tokio::net::TcpStream, status: &str, body: &str) {
    let msg = format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.write_all(msg.as_bytes()).await;
    let _ = stream.shutdown().await;
}

async fn read_request_line(stream: &mut tokio::net::TcpStream) -> Option<String> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 1024];
    while buf.len() < 8192 && !buf.windows(2).any(|w| w == b"\r\n") {
        let n = stream.read(&mut chunk).await.ok()?;
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&chunk[..n]);
    }
    let text = String::from_utf8_lossy(&buf);
    text.lines().next().map(str::to_string)
}

/// コールバックを待つ。接続ごとに読み取りを `read_timeout` で打ち切り、
/// 無言の接続・別の要求・state 違いは捨てて待ち続ける。
async fn serve_callback(
    listener: TcpListener,
    state: &str,
    read_timeout: Duration,
) -> Result<String, String> {
    loop {
        let (mut stream, _) = listener.accept().await.map_err(|e| e.to_string())?;
        let line = match tokio::time::timeout(read_timeout, read_request_line(&mut stream)).await {
            Ok(Some(line)) => line,
            _ => continue,
        };
        match parse_callback(&line, state) {
            CallbackOutcome::Code(c) => {
                respond(&mut stream, "200 OK", PAGE_OK).await;
                return Ok(c);
            }
            CallbackOutcome::Failed(e) => {
                respond(&mut stream, "400 Bad Request", PAGE_NG).await;
                return Err(e);
            }
            CallbackOutcome::StateMismatch => {
                respond(&mut stream, "400 Bad Request", PAGE_NG).await;
            }
            CallbackOutcome::Ignore => {
                respond(&mut stream, "404 Not Found", "").await;
            }
        }
    }
}

/// コールバックを待つ。5分で打ち切る。
async fn wait_for_code(listener: TcpListener, state: &str) -> Result<String, String> {
    tokio::time::timeout(LOGIN_TIMEOUT, serve_callback(listener, state, READ_TIMEOUT))
        .await
        .map_err(|_| "Google のログインがタイムアウトしました".to_string())?
}

/// 更新用トークンの失効を試みる。失敗・タイムアウトは無視する。
async fn revoke_best_effort(http: &reqwest::Client, cfg: &OAuthConfig, token: &str) {
    let _ = tokio::time::timeout(
        REVOKE_TIMEOUT,
        http.post(&cfg.revoke_url).form(&[("token", token)]).send(),
    )
    .await;
}

fn config_or_err() -> AppResult<OAuthConfig> {
    oauth_config().ok_or_else(|| "このビルドでは同期を使えません".to_string().into())
}

#[cfg(target_os = "macos")]
fn keychain() -> Arc<dyn SecretStore> {
    Arc::new(crate::sync::keychain::KeychainStore)
}

#[cfg(not(target_os = "macos"))]
fn keychain() -> Arc<dyn SecretStore> {
    Arc::new(crate::sync::keychain::MemorySecretStore::default())
}

/// 本番用の `TokenSource` (キーチェーンの更新用トークンを使う)。
pub fn token_source() -> AppResult<TokenSource> {
    Ok(TokenSource::new(config_or_err()?, keychain()))
}

/// ブラウザーでサインインし、更新用トークンをキーチェーンへ、メールアドレスを設定へ保存する。
pub async fn login(app: &AppHandle) -> AppResult<String> {
    let cfg = config_or_err()?;
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|e| e.to_string())?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    let redirect = format!("http://127.0.0.1:{port}");
    let (verifier, challenge) = pkce_pair();
    let state = uuid::Uuid::new_v4().simple().to_string();
    app.opener()
        .open_url(auth_url(&cfg, &redirect, &challenge, &state), None::<&str>)
        .map_err(|e| e.to_string())?;
    let code = wait_for_code(listener, &state).await?;
    let http = http_client();
    let (refresh, email) = exchange_code(&http, &cfg, &code, &verifier, &redirect).await?;
    if keychain().set(&refresh).is_err() {
        revoke_best_effort(&http, &cfg, &refresh).await;
        return Err("キーチェーンに保存できませんでした".to_string().into());
    }
    let email = email.unwrap_or_default();
    app.state::<AppState>()
        .with_store(|s| s.set_setting(SETTING_ACCOUNT_EMAIL, &email))?;
    Ok(email)
}

/// 失効 (revoke) を試み (失敗しても続ける)、キーチェーンとメールアドレスを消す。
pub async fn logout(app: &AppHandle) {
    let secrets = keychain();
    if let (Some(cfg), Ok(Some(token))) = (oauth_config(), secrets.get()) {
        revoke_best_effort(&http_client(), &cfg, &token).await;
    }
    let _ = secrets.delete();
    let _ = app
        .state::<AppState>()
        .with_store(|s| s.set_setting(SETTING_ACCOUNT_EMAIL, ""));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sync::keychain::MemorySecretStore;
    use wiremock::matchers::{body_string_contains, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn cfg_for(server: &MockServer) -> OAuthConfig {
        OAuthConfig {
            client_id: "cid".into(),
            client_secret: "sec".into(),
            token_url: format!("{}/token", server.uri()),
            revoke_url: format!("{}/revoke", server.uri()),
            ..OAuthConfig::google("cid".into(), "sec".into())
        }
    }

    fn source(server: &MockServer) -> TokenSource {
        let secrets = Arc::new(MemorySecretStore::default());
        secrets.set("rt-1").unwrap();
        TokenSource::new(cfg_for(server), secrets)
    }

    #[test]
    fn pkce_challenge_matches_rfc7636_example() {
        assert_eq!(
            challenge_for("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
        let (v, c) = pkce_pair();
        assert!((43..=128).contains(&v.len()));
        assert_eq!(challenge_for(&v), c);
    }

    #[test]
    fn auth_url_has_offline_access_and_only_app_data_scope() {
        let cfg = OAuthConfig::google("cid".into(), "sec".into());
        let u = url::Url::parse(&auth_url(&cfg, "http://127.0.0.1:5555", "CH", "ST")).unwrap();
        let q: std::collections::HashMap<_, _> = u.query_pairs().into_owned().collect();
        assert_eq!(u.host_str(), Some("accounts.google.com"));
        assert_eq!(
            q["scope"],
            "https://www.googleapis.com/auth/drive.appdata openid email"
        );
        assert_eq!(q["access_type"], "offline");
        assert_eq!(q["prompt"], "consent");
        assert_eq!(q["code_challenge_method"], "S256");
        assert_eq!(q["code_challenge"], "CH");
        assert_eq!(q["response_type"], "code");
        assert_eq!(q["state"], "ST");
        assert_eq!(q["client_id"], "cid");
        assert_eq!(q["redirect_uri"], "http://127.0.0.1:5555");
    }

    #[test]
    fn callback_parser_rejects_wrong_state() {
        assert_eq!(
            parse_callback("GET /?code=abc&state=ok HTTP/1.1", "ok"),
            CallbackOutcome::Code("abc".into())
        );
        assert_eq!(
            parse_callback("GET /?code=abc&state=bad HTTP/1.1", "ok"),
            CallbackOutcome::StateMismatch
        );
        assert!(matches!(
            parse_callback("GET /?error=access_denied&state=ok HTTP/1.1", "ok"),
            CallbackOutcome::Failed(_)
        ));
        assert_eq!(
            parse_callback("GET /favicon.ico HTTP/1.1", "ok"),
            CallbackOutcome::Ignore
        );
    }

    #[test]
    fn email_is_read_from_id_token() {
        use base64::Engine;
        let payload =
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(r#"{"email":"a@example.com"}"#);
        assert_eq!(
            email_from_id_token(&format!("h.{payload}.s")).as_deref(),
            Some("a@example.com")
        );
        assert_eq!(email_from_id_token("garbage"), None);
    }

    #[tokio::test]
    async fn token_source_refreshes_and_caches() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/token"))
            .and(body_string_contains("grant_type=refresh_token"))
            .and(body_string_contains("refresh_token=rt-1"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"access_token": "at-1", "expires_in": 3600})),
            )
            .expect(1)
            .mount(&server)
            .await;
        let ts = source(&server);
        assert_eq!(ts.get().await.unwrap(), "at-1");
        assert_eq!(ts.get().await.unwrap(), "at-1");
        server.verify().await;
    }

    #[tokio::test]
    async fn expired_token_is_refreshed_again_and_invalidate_clears() {
        let server = MockServer::start().await;
        // expires_in 60 minus the 60 s margin leaves no lifetime.
        Mock::given(method("POST"))
            .and(path("/token"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"access_token": "at", "expires_in": 60})),
            )
            .expect(2)
            .mount(&server)
            .await;
        let ts = source(&server);
        ts.get().await.unwrap();
        ts.get().await.unwrap();
        server.verify().await;

        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/token"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"access_token": "at", "expires_in": 3600})),
            )
            .expect(2)
            .mount(&server)
            .await;
        let ts = source(&server);
        ts.get().await.unwrap();
        ts.invalidate().await;
        ts.get().await.unwrap();
        server.verify().await;
    }

    #[tokio::test]
    async fn invalid_grant_means_auth_expired() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/token"))
            .respond_with(
                ResponseTemplate::new(400)
                    .set_body_json(serde_json::json!({"error": "invalid_grant"})),
            )
            .mount(&server)
            .await;
        let ts = source(&server);
        assert!(matches!(ts.get().await, Err(DriveError::AuthExpired)));
    }

    #[tokio::test]
    async fn missing_refresh_token_means_auth_expired_and_5xx_is_retryable() {
        let server = MockServer::start().await;
        let ts = TokenSource::new(cfg_for(&server), Arc::new(MemorySecretStore::default()));
        assert!(matches!(ts.get().await, Err(DriveError::AuthExpired)));

        Mock::given(method("POST"))
            .and(path("/token"))
            .respond_with(ResponseTemplate::new(503))
            .mount(&server)
            .await;
        let ts = source(&server);
        assert!(matches!(ts.get().await, Err(DriveError::Retryable(_))));
    }

    #[tokio::test]
    async fn code_exchange_returns_refresh_token_and_email() {
        use base64::Engine;
        let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(r#"{"email":"me@example.com"}"#);
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/token"))
            .and(body_string_contains("grant_type=authorization_code"))
            .and(body_string_contains("code_verifier=V"))
            .and(body_string_contains(
                "redirect_uri=http%3A%2F%2F127.0.0.1%3A1",
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": "at", "refresh_token": "rt", "expires_in": 3600,
                "id_token": format!("h.{payload}.s")
            })))
            .mount(&server)
            .await;
        let (rt, email) = exchange_code(
            &http_client(),
            &cfg_for(&server),
            "code",
            "V",
            "http://127.0.0.1:1",
        )
        .await
        .unwrap();
        assert_eq!(rt, "rt");
        assert_eq!(email.as_deref(), Some("me@example.com"));
    }

    #[tokio::test]
    async fn slow_token_endpoint_is_retryable() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/token"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"access_token": "at", "expires_in": 3600}))
                    .set_delay(Duration::from_secs(2)),
            )
            .mount(&server)
            .await;
        let secrets = Arc::new(MemorySecretStore::default());
        secrets.set("rt-1").unwrap();
        let http = build_http(Duration::from_millis(200), Duration::from_millis(200));
        let ts = TokenSource::with_client(cfg_for(&server), secrets, http);
        assert!(matches!(ts.get().await, Err(DriveError::Retryable(_))));
    }

    #[tokio::test]
    async fn revoke_posts_the_token_and_gives_up_quietly() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/revoke"))
            .and(body_string_contains("token=rt-9"))
            .respond_with(ResponseTemplate::new(200))
            .expect(1)
            .mount(&server)
            .await;
        revoke_best_effort(&http_client(), &cfg_for(&server), "rt-9").await;
        server.verify().await;

        // An unreachable server must not panic or hang.
        let mut cfg = cfg_for(&server);
        cfg.revoke_url = "http://127.0.0.1:1/revoke".into();
        revoke_best_effort(&http_client(), &cfg, "rt-9").await;
    }

    async fn spawn_listener(
        read_timeout: Duration,
    ) -> (u16, tokio::task::JoinHandle<Result<String, String>>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let h = tokio::spawn(async move { serve_callback(listener, "ST", read_timeout).await });
        (port, h)
    }

    async fn get_path(port: u16, target: &str) -> String {
        use tokio::io::AsyncReadExt;
        let mut c = tokio::net::TcpStream::connect(("127.0.0.1", port))
            .await
            .unwrap();
        c.write_all(format!("GET {target} HTTP/1.1\r\nHost: x\r\n\r\n").as_bytes())
            .await
            .unwrap();
        let mut out = String::new();
        let _ = c.read_to_string(&mut out).await;
        out
    }

    #[tokio::test]
    async fn silent_connection_does_not_block_the_callback() {
        let (port, h) = spawn_listener(Duration::from_millis(200)).await;
        let _silent = tokio::net::TcpStream::connect(("127.0.0.1", port))
            .await
            .unwrap();
        let page = get_path(port, "/?code=abc&state=ST").await;
        assert!(page.starts_with("HTTP/1.1 200"));
        assert_eq!(h.await.unwrap(), Ok("abc".to_string()));
    }

    #[tokio::test]
    async fn favicon_then_callback_returns_the_code() {
        let (port, h) = spawn_listener(Duration::from_secs(5)).await;
        let page = get_path(port, "/favicon.ico").await;
        assert!(page.starts_with("HTTP/1.1 404"));
        get_path(port, "/?code=abc&state=ST").await;
        assert_eq!(h.await.unwrap(), Ok("abc".to_string()));
    }

    #[tokio::test]
    async fn wrong_state_gets_failure_page_and_listener_keeps_waiting() {
        let (port, h) = spawn_listener(Duration::from_secs(5)).await;
        let page = get_path(port, "/?code=evil&state=BAD").await;
        assert!(page.starts_with("HTTP/1.1 400"));
        assert!(!h.is_finished());
        get_path(port, "/?code=abc&state=ST").await;
        assert_eq!(h.await.unwrap(), Ok("abc".to_string()));
    }

    #[tokio::test]
    async fn google_error_with_right_state_ends_the_flow() {
        let (port, h) = spawn_listener(Duration::from_secs(5)).await;
        get_path(port, "/?error=access_denied&state=ST").await;
        let err = h.await.unwrap().unwrap_err();
        assert!(err.contains("access_denied"));
    }
}
