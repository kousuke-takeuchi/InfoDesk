use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;

use super::drive::{DriveError, DriveStore, RemoteFile};

#[async_trait]
pub trait AccessToken: Send + Sync {
    async fn get(&self) -> Result<String, DriveError>;
    async fn invalidate(&self);
}

pub struct GoogleDrive {
    http: reqwest::Client,
    base: String,
    token: Arc<dyn AccessToken>,
}

impl GoogleDrive {
    pub fn new(token: Arc<dyn AccessToken>) -> Self {
        Self::with_base("https://www.googleapis.com", token)
    }
    pub fn with_base(base: &str, token: Arc<dyn AccessToken>) -> Self {
        Self::with_timeouts(
            base,
            token,
            Duration::from_secs(10),
            Duration::from_secs(60),
        )
    }
    /// `connect` bounds connection setup; `read` bounds each wait for data, so a
    /// stalled connection cannot hang a sync.
    pub fn with_timeouts(
        base: &str,
        token: Arc<dyn AccessToken>,
        connect: Duration,
        read: Duration,
    ) -> Self {
        let http = reqwest::Client::builder()
            .connect_timeout(connect)
            .read_timeout(read)
            .build()
            .expect("reqwest client");
        Self {
            http,
            base: base.trim_end_matches('/').to_string(),
            token,
        }
    }
}

const FIELDS: &str = "id,name,modifiedTime,size";

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct ApiFile {
    id: String,
    name: String,
    modified_time: Option<String>,
    size: Option<String>,
}

impl TryFrom<ApiFile> for RemoteFile {
    type Error = DriveError;
    fn try_from(f: ApiFile) -> Result<Self, DriveError> {
        let modified_time = f
            .modified_time
            .ok_or_else(|| DriveError::Other(format!("file {} has no modifiedTime", f.id)))?;
        Ok(RemoteFile {
            id: f.id,
            name: f.name,
            modified_time,
            size: f.size.and_then(|s| s.parse().ok()),
        })
    }
}

fn transport(e: reqwest::Error) -> DriveError {
    DriveError::Retryable(e.to_string())
}

/// A short error description: Google's `error.message` when present, else the
/// body truncated to 200 characters.
fn error_text(body: &str) -> String {
    serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|v| v["error"]["message"].as_str().map(str::to_string))
        .unwrap_or_else(|| body.chars().take(200).collect())
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct ListResponse {
    next_page_token: Option<String>,
    #[serde(default)]
    files: Vec<ApiFile>,
}

fn with_query<K: AsRef<str>, V: AsRef<str>>(url: &str, params: &[(K, V)]) -> reqwest::Url {
    reqwest::Url::parse_with_params(url, params).expect("valid Drive URL")
}

fn is_rate_limit_body(body: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|v| {
            v["error"]["errors"].as_array().map(|errs| {
                errs.iter().any(|e| {
                    matches!(
                        e["reason"].as_str(),
                        Some("userRateLimitExceeded" | "rateLimitExceeded")
                    )
                })
            })
        })
        .unwrap_or(false)
}

fn multipart_related(boundary: &str, meta: &str, mime: &str, bytes: &[u8]) -> Vec<u8> {
    let mut body = Vec::with_capacity(bytes.len() + meta.len() + 256);
    body.extend_from_slice(
        format!(
            "--{boundary}\r\nContent-Type: application/json; charset=UTF-8\r\n\r\n{meta}\r\n--{boundary}\r\nContent-Type: {mime}\r\n\r\n"
        )
        .as_bytes(),
    );
    body.extend_from_slice(bytes);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    body
}

impl GoogleDrive {
    /// Send a request, retrying once with a fresh token on 401. Returns the response
    /// only when it is a success (2xx) or `allow_404` is set and the status is 404.
    async fn send(
        &self,
        allow_404: bool,
        build: impl Fn(&reqwest::Client, &str) -> reqwest::RequestBuilder,
    ) -> Result<reqwest::Response, DriveError> {
        for attempt in 0..2 {
            let token = self.token.get().await?;
            let resp = build(&self.http, &token)
                .bearer_auth(&token)
                .send()
                .await
                .map_err(transport)?;
            let status = resp.status();
            if status.is_success() || (allow_404 && status.as_u16() == 404) {
                return Ok(resp);
            }
            if status.as_u16() == 401 {
                if attempt == 0 {
                    self.token.invalidate().await;
                    continue;
                }
                return Err(DriveError::AuthExpired);
            }
            let code = status.as_u16();
            let body = resp.text().await.unwrap_or_default();
            return Err(
                if code == 429
                    || status.is_server_error()
                    || (code == 403 && is_rate_limit_body(&body))
                {
                    DriveError::Retryable(format!("HTTP {code}"))
                } else {
                    DriveError::Other(format!("HTTP {code}: {}", error_text(&body)))
                },
            );
        }
        Err(DriveError::AuthExpired)
    }

    async fn parse_file(resp: reqwest::Response) -> Result<RemoteFile, DriveError> {
        resp.json::<ApiFile>().await.map_err(transport)?.try_into()
    }
}

#[async_trait]
impl DriveStore for GoogleDrive {
    async fn list(&self) -> Result<Vec<RemoteFile>, DriveError> {
        let mut out = Vec::new();
        let mut page: Option<String> = None;
        loop {
            let url = format!("{}/drive/v3/files", self.base);
            let page_ref = page.clone();
            let resp = self
                .send(false, |http, _| {
                    let mut q = vec![
                        ("spaces", "appDataFolder".to_string()),
                        (
                            "fields",
                            "nextPageToken,files(id,name,modifiedTime,size)".to_string(),
                        ),
                        ("pageSize", "1000".to_string()),
                    ];
                    if let Some(p) = &page_ref {
                        q.push(("pageToken", p.clone()));
                    }
                    http.get(with_query(&url, &q))
                })
                .await?;
            let parsed: ListResponse = resp.json().await.map_err(transport)?;
            for f in parsed.files {
                out.push(f.try_into()?);
            }
            match parsed.next_page_token {
                Some(t) if !t.is_empty() => page = Some(t),
                _ => return Ok(out),
            }
        }
    }

    async fn download(&self, id: &str) -> Result<Vec<u8>, DriveError> {
        let url = format!("{}/drive/v3/files/{id}", self.base);
        let resp = self
            .send(false, |http, _| {
                http.get(with_query(&url, &[("alt", "media")]))
            })
            .await?;
        resp.bytes().await.map(|b| b.to_vec()).map_err(transport)
    }

    async fn create(
        &self,
        name: &str,
        bytes: Vec<u8>,
        mime: &str,
    ) -> Result<RemoteFile, DriveError> {
        let url = format!("{}/upload/drive/v3/files", self.base);
        let meta = serde_json::json!({"name": name, "parents": ["appDataFolder"]}).to_string();
        let boundary = format!("infodesk-{}", uuid::Uuid::new_v4().simple());
        let body = multipart_related(&boundary, &meta, mime, &bytes);
        let ct = format!("multipart/related; boundary={boundary}");
        let resp = self
            .send(false, |http, _| {
                http.post(with_query(
                    &url,
                    &[("uploadType", "multipart"), ("fields", FIELDS)],
                ))
                .header("Content-Type", &ct)
                .body(body.clone())
            })
            .await?;
        Self::parse_file(resp).await
    }

    async fn update(&self, id: &str, bytes: Vec<u8>, mime: &str) -> Result<RemoteFile, DriveError> {
        let url = format!("{}/upload/drive/v3/files/{id}", self.base);
        let resp = self
            .send(false, |http, _| {
                http.patch(with_query(
                    &url,
                    &[("uploadType", "media"), ("fields", FIELDS)],
                ))
                .header("Content-Type", mime)
                .body(bytes.clone())
            })
            .await?;
        Self::parse_file(resp).await
    }

    async fn delete(&self, id: &str) -> Result<(), DriveError> {
        let url = format!("{}/drive/v3/files/{id}", self.base);
        self.send(true, |http, _| http.delete(&url))
            .await
            .map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use wiremock::matchers::{method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    struct Tok {
        gets: AtomicUsize,
        invalidations: AtomicUsize,
    }
    impl Tok {
        fn new() -> Arc<Self> {
            Arc::new(Self {
                gets: AtomicUsize::new(0),
                invalidations: AtomicUsize::new(0),
            })
        }
    }
    #[async_trait]
    impl AccessToken for Tok {
        async fn get(&self) -> Result<String, DriveError> {
            let n = self.gets.fetch_add(1, Ordering::SeqCst);
            Ok(format!("tok{n}"))
        }
        async fn invalidate(&self) {
            self.invalidations.fetch_add(1, Ordering::SeqCst);
        }
    }

    fn file_json(id: &str, name: &str) -> serde_json::Value {
        serde_json::json!({"id": id, "name": name, "modifiedTime": "2026-01-01T00:00:00.000Z", "size": "5"})
    }

    #[tokio::test]
    async fn list_follows_page_tokens() {
        let s = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/drive/v3/files"))
            .and(query_param("spaces", "appDataFolder"))
            .and(query_param("pageSize", "1000"))
            .and(query_param("pageToken", "p2"))
            .and(query_param(
                "fields",
                "nextPageToken,files(id,name,modifiedTime,size)",
            ))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"files":[file_json("c","c.json")]})),
            )
            .mount(&s)
            .await;
        Mock::given(method("GET"))
            .and(path("/drive/v3/files"))
            .and(query_param("spaces", "appDataFolder"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "nextPageToken": "p2",
                "files":[file_json("a","a.json"), file_json("b","b.json")]})))
            .mount(&s)
            .await;
        let d = GoogleDrive::with_base(&s.uri(), Tok::new());
        let files = d.list().await.unwrap();
        let ids: Vec<_> = files.iter().map(|f| f.id.as_str()).collect();
        assert_eq!(ids, vec!["a", "b", "c"]);
        assert_eq!(files[0].size, Some(5));
    }

    #[tokio::test]
    async fn create_sends_multipart_with_app_data_parent() {
        let s = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/upload/drive/v3/files"))
            .and(query_param("uploadType", "multipart"))
            .and(query_param("fields", "id,name,modifiedTime,size"))
            .respond_with(ResponseTemplate::new(200).set_body_json(file_json("n", "x.bin")))
            .mount(&s)
            .await;
        let d = GoogleDrive::with_base(&s.uri(), Tok::new());
        let body = vec![0u8, 1, 2, 255, 254];
        let f = d
            .create("x.bin", body.clone(), "application/gzip")
            .await
            .unwrap();
        assert_eq!(f.id, "n");
        let reqs = s.received_requests().await.unwrap();
        let req = &reqs[0];
        let ct = req
            .headers
            .get("content-type")
            .unwrap()
            .to_str()
            .unwrap()
            .to_string();
        assert!(ct.starts_with("multipart/related; boundary="));
        assert_eq!(
            req.headers.get("authorization").unwrap().to_str().unwrap(),
            "Bearer tok0"
        );
        let text = String::from_utf8_lossy(&req.body);
        assert!(text.contains(r#""parents":["appDataFolder"]"#));
        assert!(text.contains(r#""name":"x.bin""#));
        assert!(req.body.windows(body.len()).any(|w| w == body.as_slice()));
    }

    #[tokio::test]
    async fn update_patches_media_and_download_gets_media() {
        let s = MockServer::start().await;
        Mock::given(method("PATCH"))
            .and(path("/upload/drive/v3/files/abc"))
            .and(query_param("uploadType", "media"))
            .respond_with(ResponseTemplate::new(200).set_body_json(file_json("abc", "x")))
            .mount(&s)
            .await;
        Mock::given(method("GET"))
            .and(path("/drive/v3/files/abc"))
            .and(query_param("alt", "media"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(vec![9u8, 8]))
            .mount(&s)
            .await;
        let d = GoogleDrive::with_base(&s.uri(), Tok::new());
        assert_eq!(
            d.update("abc", vec![1], "application/json")
                .await
                .unwrap()
                .id,
            "abc"
        );
        assert_eq!(d.download("abc").await.unwrap(), vec![9u8, 8]);
    }

    #[tokio::test]
    async fn unauthorized_retries_once_with_a_new_token_then_reports_auth_expired() {
        let s = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/drive/v3/files/abc"))
            .respond_with(ResponseTemplate::new(401))
            .mount(&s)
            .await;
        let t = Tok::new();
        let d = GoogleDrive::with_base(&s.uri(), t.clone());
        assert!(matches!(
            d.download("abc").await,
            Err(DriveError::AuthExpired)
        ));
        assert_eq!(t.invalidations.load(Ordering::SeqCst), 1);

        let s2 = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/drive/v3/files/abc"))
            .respond_with(ResponseTemplate::new(401))
            .up_to_n_times(1)
            .with_priority(1)
            .mount(&s2)
            .await;
        Mock::given(method("GET"))
            .and(path("/drive/v3/files/abc"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(vec![7u8]))
            .with_priority(2)
            .mount(&s2)
            .await;
        let t2 = Tok::new();
        let d2 = GoogleDrive::with_base(&s2.uri(), t2.clone());
        assert_eq!(d2.download("abc").await.unwrap(), vec![7u8]);
        assert_eq!(t2.invalidations.load(Ordering::SeqCst), 1);
        let reqs = s2.received_requests().await.unwrap();
        assert_eq!(
            reqs[1]
                .headers
                .get("authorization")
                .unwrap()
                .to_str()
                .unwrap(),
            "Bearer tok1"
        );
    }

    #[tokio::test]
    async fn rate_limit_is_retryable() {
        for (status, body) in [
            (
                403,
                r#"{"error":{"errors":[{"reason":"userRateLimitExceeded"}]}}"#,
            ),
            (
                403,
                r#"{"error":{"errors":[{"reason":"rateLimitExceeded"}]}}"#,
            ),
            (429, ""),
            (503, ""),
        ] {
            let s = MockServer::start().await;
            Mock::given(method("GET"))
                .respond_with(ResponseTemplate::new(status).set_body_string(body))
                .mount(&s)
                .await;
            let d = GoogleDrive::with_base(&s.uri(), Tok::new());
            assert!(
                matches!(d.download("a").await, Err(DriveError::Retryable(_))),
                "{status} {body}"
            );
        }
    }

    #[tokio::test]
    async fn other_errors_and_connection_failures() {
        let s = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(
                ResponseTemplate::new(403)
                    .set_body_string(r#"{"error":{"errors":[{"reason":"forbidden"}]}}"#),
            )
            .mount(&s)
            .await;
        let d = GoogleDrive::with_base(&s.uri(), Tok::new());
        assert!(matches!(d.download("a").await, Err(DriveError::Other(_))));
        let dead = GoogleDrive::with_base("http://127.0.0.1:1", Tok::new());
        assert!(matches!(
            dead.download("a").await,
            Err(DriveError::Retryable(_))
        ));
    }

    #[tokio::test]
    async fn stalled_response_times_out_as_retryable() {
        let s = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_delay(Duration::from_secs(3)))
            .mount(&s)
            .await;
        let d = GoogleDrive::with_timeouts(
            &s.uri(),
            Tok::new(),
            Duration::from_secs(1),
            Duration::from_millis(200),
        );
        assert!(matches!(
            d.download("a").await,
            Err(DriveError::Retryable(_))
        ));
        assert!(matches!(d.list().await, Err(DriveError::Retryable(_))));
    }

    #[tokio::test]
    async fn bad_json_is_retryable_and_missing_modified_time_is_other() {
        let s = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/drive/v3/files"))
            .respond_with(ResponseTemplate::new(200).set_body_string("not json"))
            .mount(&s)
            .await;
        Mock::given(method("PATCH"))
            .and(path("/upload/drive/v3/files/abc"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"id":"abc","name":"x"})),
            )
            .mount(&s)
            .await;
        let d = GoogleDrive::with_base(&s.uri(), Tok::new());
        assert!(matches!(d.list().await, Err(DriveError::Retryable(_))));
        assert!(matches!(
            d.update("abc", vec![1], "application/json").await,
            Err(DriveError::Other(m)) if m.contains("modifiedTime")
        ));
    }

    #[test]
    fn error_text_prefers_message_and_truncates() {
        assert_eq!(error_text(r#"{"error":{"message":"nope"}}"#), "nope");
        let long = "x".repeat(500);
        assert_eq!(error_text(&long).chars().count(), 200);
    }

    #[tokio::test]
    async fn delete_treats_404_as_done() {
        let s = MockServer::start().await;
        Mock::given(method("DELETE"))
            .and(path("/drive/v3/files/gone"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&s)
            .await;
        Mock::given(method("DELETE"))
            .and(path("/drive/v3/files/here"))
            .respond_with(ResponseTemplate::new(204))
            .mount(&s)
            .await;
        let d = GoogleDrive::with_base(&s.uri(), Tok::new());
        d.delete("gone").await.unwrap();
        d.delete("here").await.unwrap();
    }
}
