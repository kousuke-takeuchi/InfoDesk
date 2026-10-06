use std::time::Duration;

use async_trait::async_trait;
use serde_json::json;

use super::{clean_output, LlmProvider};
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
    async fn error_status_empty_content_and_unreachable_are_errors() {
        let server = MockServer::start().await;
        Mock::given(method("POST")).and(path("/api/chat"))
            .respond_with(ResponseTemplate::new(404).set_body_json(json!({"error": "model not found"})))
            .mount(&server).await;
        let p = OllamaProvider::new(&server.uri(), "missing").unwrap();
        let err = p.complete("s", "p").await.unwrap_err().to_string();
        assert!(err.contains("model not found"), "{err}");

        let server = MockServer::start().await;
        Mock::given(method("POST")).and(path("/api/chat"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"message": {"content": "  "}})))
            .mount(&server).await;
        let p = OllamaProvider::new(&server.uri(), "m").unwrap();
        assert!(matches!(p.complete("s", "p").await, Err(Error::Llm(_))));

        let p = OllamaProvider::new("http://127.0.0.1:1", "m").unwrap();
        assert!(p.complete("s", "p").await.is_err());
    }
}
