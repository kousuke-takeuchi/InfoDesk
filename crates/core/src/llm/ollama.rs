use std::time::Duration;

use async_trait::async_trait;
use serde_json::json;

use super::{LlmProvider, clean_output};
use crate::error::{Error, Result};

/// 既定の4096では長い入力が切り捨てられるため、明示する。
const NUM_CTX: u32 = 16384;

pub struct OllamaProvider {
    base_url: String,
    model: String,
    http: reqwest::Client,
}

impl OllamaProvider {
    pub fn new(base_url: &str, model: &str) -> Result<Self> {
        Ok(Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            model: model.to_string(),
            // ローカルLLMの生成は長い。モデルの読み込みも含めて待つ。
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(600))
                .build()?,
        })
    }
}

/// モデル一覧の取得を待つ上限。設定画面を待たせないよう短くする。
pub const LIST_MODELS_TIMEOUT: Duration = Duration::from_secs(5);

/// インストール済みのモデル名を `GET {base_url}/api/tags` から取る。
pub async fn list_models(http: &reqwest::Client, base_url: &str) -> Result<Vec<String>> {
    list_models_within(http, base_url, LIST_MODELS_TIMEOUT).await
}

async fn list_models_within(
    http: &reqwest::Client,
    base_url: &str,
    timeout: Duration,
) -> Result<Vec<String>> {
    let url = format!("{}/api/tags", base_url.trim_end_matches('/'));
    let resp = http.get(&url).timeout(timeout).send().await?;
    let status = resp.status();
    if !status.is_success() {
        return Err(Error::Llm(format!("ollama {status}: {url}")));
    }
    let value: serde_json::Value = resp.json().await?;
    Ok(value["models"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|m| m["name"].as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default())
}

#[async_trait]
impl LlmProvider for OllamaProvider {
    fn name(&self) -> String {
        format!("ollama:{}", self.model)
    }

    async fn complete(&self, system: &str, prompt: &str) -> Result<String> {
        let body = json!({
            "model": self.model,
            "stream": false,
            "think": false,
            "options": {"num_ctx": NUM_CTX},
            "messages": [
                {"role": "system", "content": system},
                {"role": "user", "content": prompt}
            ]
        });
        let resp = self
            .http
            .post(format!("{}/api/chat", self.base_url))
            .json(&body)
            .send()
            .await?;
        let status = resp.status();
        let value: serde_json::Value = resp.json().await?;
        if !status.is_success() {
            let detail = value["error"].as_str().unwrap_or("unknown error");
            return Err(Error::Llm(format!("ollama {status}: {detail}")));
        }
        clean_output(value["message"]["content"].as_str().unwrap_or_default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{body_partial_json, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn sends_chat_request_and_returns_content() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/chat"))
            .and(body_partial_json(json!({
                "model": "m1",
                "stream": false,
                "think": false,
                "messages": [
                    {"role": "system", "content": "sys"},
                    {"role": "user", "content": "hello"}
                ]
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "message": {"role": "assistant", "content": "  返答です。\n"},
                "done": true
            })))
            .mount(&server)
            .await;
        let p = OllamaProvider::new(&format!("{}/", server.uri()), "m1").unwrap();
        assert_eq!(p.name(), "ollama:m1");
        assert_eq!(p.complete("sys", "hello").await.unwrap(), "返答です。");
    }

    #[tokio::test]
    async fn lists_installed_models() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/tags"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "models": [{"name": "gemma4:26b"}, {"name": "qwen3:8b"}]
            })))
            .mount(&server)
            .await;
        let http = reqwest::Client::new();
        let got = list_models(&http, &format!("{}/", server.uri()))
            .await
            .unwrap();
        assert_eq!(got, vec!["gemma4:26b", "qwen3:8b"]);

        assert!(list_models(&http, "http://127.0.0.1:1").await.is_err());

        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/tags"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;
        assert!(list_models(&http, &server.uri()).await.is_err());
    }

    #[tokio::test]
    async fn listing_models_gives_up_after_the_timeout() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/tags"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({"models": []}))
                    .set_delay(Duration::from_secs(5)),
            )
            .mount(&server)
            .await;
        let http = reqwest::Client::new();
        let started = std::time::Instant::now();
        assert!(
            list_models_within(&http, &server.uri(), Duration::from_millis(200))
                .await
                .is_err()
        );
        assert!(started.elapsed() < Duration::from_secs(2));
        assert!(LIST_MODELS_TIMEOUT <= Duration::from_secs(5));
    }

    #[tokio::test]
    async fn error_status_empty_content_and_unreachable_are_errors() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/chat"))
            .respond_with(
                ResponseTemplate::new(404).set_body_json(json!({"error": "model not found"})),
            )
            .mount(&server)
            .await;
        let p = OllamaProvider::new(&server.uri(), "missing").unwrap();
        let err = p.complete("s", "p").await.unwrap_err().to_string();
        assert!(err.contains("model not found"), "{err}");

        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/chat"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!({"message": {"content": "  "}})),
            )
            .mount(&server)
            .await;
        let p = OllamaProvider::new(&server.uri(), "m").unwrap();
        assert!(matches!(p.complete("s", "p").await, Err(Error::Llm(_))));

        let p = OllamaProvider::new("http://127.0.0.1:1", "m").unwrap();
        assert!(p.complete("s", "p").await.is_err());
    }
}
