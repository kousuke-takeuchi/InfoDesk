use std::time::Duration;

use async_trait::async_trait;
use serde_json::{Value, json};

use super::claude::is_valid_model_name;
use super::{LlmProvider, LlmRole, clean_output};
use crate::error::{Error, Result};
use crate::store::Store;

/// API キーを読む環境変数。キーは DB に保存しない。
pub const API_KEY_ENV: &str = "ANTHROPIC_API_KEY";
const BASE_URL: &str = "https://api.anthropic.com";
const API_VERSION: &str = "2026-10-21";
/// 安全機能が断った依頼を、勧められたモデルでやり直す (`fallbacks: "default"`)。
const FALLBACK_BETA: &str = "server-side-fallback-2026-07-01";
const MAX_TOKENS: u32 = 16000;
const TIMEOUT: Duration = Duration::from_secs(300);
/// `output_config.effort` に使える値。空は API の既定。
pub const EFFORTS: [&str; 5] = ["low", "medium", "high", "xhigh", "max"];

pub struct ClaudeApiProvider {
    api_key: String,
    model: String,
    pub(crate) effort: String,
    base_url: String,
    http: reqwest::Client,
}

impl std::fmt::Debug for ClaudeApiProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ClaudeApiProvider")
            .field("api_key", &"<redacted>")
            .field("model", &self.model)
            .field("effort", &self.effort)
            .field("base_url", &self.base_url)
            .finish()
    }
}

impl ClaudeApiProvider {
    pub fn new(api_key: String, model: &str, effort: &str) -> Result<Self> {
        let model = model.trim();
        if !is_valid_model_name(model) {
            return Err(Error::Invalid(format!(
                "claude_api.model: invalid model name '{model}'"
            )));
        }
        let effort = effort.trim();
        if !effort.is_empty() && !EFFORTS.contains(&effort) {
            return Err(Error::Invalid(format!(
                "claude_api effort: '{effort}' is not one of {}",
                EFFORTS.join(", ")
            )));
        }
        Ok(Self {
            api_key,
            model: model.to_string(),
            effort: effort.to_string(),
            base_url: BASE_URL.to_string(),
            http: reqwest::Client::builder().timeout(TIMEOUT).build()?,
        })
    }

    /// テスト用。接続先を差し替える。
    pub fn with_base_url(mut self, base_url: &str) -> Self {
        self.base_url = base_url.trim_end_matches('/').to_string();
        self
    }
}

/// 設定と API キーから作る。用途ごとの effort は `claude_api.items_effort` / `claude_api.digest_effort`。
pub fn from_settings(
    store: &Store,
    role: LlmRole,
    api_key: Option<String>,
) -> Result<ClaudeApiProvider> {
    let api_key = api_key
        .map(|k| k.trim().to_string())
        .filter(|k| !k.is_empty())
        .ok_or_else(|| Error::Invalid(format!("{API_KEY_ENV} is not set")))?;
    let effort_key = match role {
        LlmRole::Items => "claude_api.items_effort",
        LlmRole::Digest => "claude_api.digest_effort",
    };
    ClaudeApiProvider::new(
        api_key,
        &store.get_setting_or("claude_api.model", "claude-sonnet-5-5")?,
        &store.get_setting_or(effort_key, "")?,
    )
}

/// API のエラー応答を文にする。認証の失敗では本文を使わない (キーが含まれうる)。
fn api_error(status: reqwest::StatusCode, body: &Value) -> String {
    if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
        return format!("claude_api {status}: the API key was rejected");
    }
    let message = body["error"]["message"].as_str().unwrap_or("unknown error");
    format!("claude_api {status}: {message}")
}

#[async_trait]
impl LlmProvider for ClaudeApiProvider {
    fn name(&self) -> String {
        format!("claude_api:{}", self.model)
    }

    async fn complete(&self, system: &str, prompt: &str) -> Result<String> {
        let mut body = json!({
            "model": self.model,
            "max_tokens": MAX_TOKENS,
            "system": system,
            "messages": [{"role": "user", "content": prompt}],
            "fallbacks": "default"
        });
        if !self.effort.is_empty() {
            body["output_config"] = json!({"effort": self.effort});
        }
        let resp = self
            .http
            .post(format!("{}/v1/messages", self.base_url))
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", API_VERSION)
            .header("anthropic-beta", FALLBACK_BETA)
            .json(&body)
            .send()
            .await?;
        let status = resp.status();
        let value: Value = resp
            .json()
            .await
            .map_err(|_| Error::Llm(format!("claude_api {status}: unreadable response")))?;
        if !status.is_success() {
            return Err(Error::Llm(api_error(status, &value)));
        }
        match value["stop_reason"].as_str() {
            Some("refusal") => {
                let category = value["stop_details"]["category"]
                    .as_str()
                    .unwrap_or("unknown");
                return Err(Error::Llm(format!("claude_api: refused ({category})")));
            }
            Some("max_tokens") => {
                return Err(Error::Llm("claude_api: truncated (max_tokens)".to_string()));
            }
            // 最後まで書き終えたときだけ成功とする。
            Some("end_turn" | "stop_sequence") => {}
            // 文脈の上限や pause_turn などで止まった途中の文は使わない。stop_reason がないのも同じ。
            other => {
                let reason = other.unwrap_or("none");
                return Err(Error::Llm(format!("claude_api: stopped ({reason})")));
            }
        }
        // thinking や fallback の区切りなど、text 以外のブロックは読まない。
        let text: String = value["content"]
            .as_array()
            .map(|blocks| {
                blocks
                    .iter()
                    .filter(|b| b["type"] == "text")
                    .filter_map(|b| b["text"].as_str())
                    .collect()
            })
            .unwrap_or_default();
        clean_output(&text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use wiremock::matchers::{body_partial_json, header, method, path};
    use wiremock::{Mock, MockServer, Request, ResponseTemplate};

    fn provider(server: &MockServer, effort: &str) -> ClaudeApiProvider {
        ClaudeApiProvider::new("sk-test-secret".into(), "claude-sonnet-5-5", effort)
            .unwrap()
            .with_base_url(&server.uri())
    }

    fn ok_body(content: serde_json::Value) -> serde_json::Value {
        json!({"type": "message", "stop_reason": "end_turn", "content": content})
    }

    #[tokio::test]
    async fn sends_a_messages_request_and_joins_text_blocks() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/messages"))
            .and(header("x-api-key", "sk-test-secret"))
            .and(header("anthropic-version", "2026-10-21"))
            .and(header("anthropic-beta", "server-side-fallback-2026-07-01"))
            .and(body_partial_json(json!({
                "model": "claude-sonnet-5-5",
                "max_tokens": 16000,
                "system": "sys",
                "messages": [{"role": "user", "content": "hello"}],
                "output_config": {"effort": "low"},
                "fallbacks": "default"
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(ok_body(json!([
                {"type": "thinking", "thinking": ""},
                {"type": "text", "text": "  要約"},
                {"type": "fallback", "from": {"model": "a"}, "to": {"model": "b"}},
                {"type": "text", "text": "です。\n"}
            ]))))
            .mount(&server)
            .await;
        let p = provider(&server, "low");
        assert_eq!(p.name(), "claude_api:claude-sonnet-5-5");
        assert_eq!(p.complete("sys", "hello").await.unwrap(), "要約です。");
    }

    #[tokio::test]
    async fn sends_no_sampling_thinking_or_empty_effort() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(ok_body(json!([
                {"type": "text", "text": "ok"}
            ]))))
            .mount(&server)
            .await;
        provider(&server, "").complete("s", "p").await.unwrap();
        let reqs: Vec<Request> = server.received_requests().await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&reqs[0].body).unwrap();
        for key in ["temperature", "top_p", "top_k", "thinking", "output_config"] {
            assert!(body.get(key).is_none(), "{key} must not be sent: {body}");
        }
    }

    #[tokio::test]
    async fn refusal_and_truncation_are_errors() {
        for (stop, want) in [("refusal", "refused (cyber)"), ("max_tokens", "truncated")] {
            let server = MockServer::start().await;
            Mock::given(method("POST"))
                .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                    "stop_reason": stop,
                    "stop_details": {"type": "refusal", "category": "cyber"},
                    "content": [{"type": "text", "text": "partial"}]
                })))
                .mount(&server)
                .await;
            let err = provider(&server, "low")
                .complete("s", "p")
                .await
                .unwrap_err();
            assert!(matches!(&err, Error::Llm(m) if m.contains(want)), "{err}");
        }
    }

    #[tokio::test]
    async fn only_end_turn_and_stop_sequence_are_success() {
        let reply = |stop: Option<&str>| {
            let mut body = json!({"content": [{"type": "text", "text": "partial"}]});
            if let Some(stop) = stop {
                body["stop_reason"] = json!(stop);
            }
            body
        };
        for (stop, want) in [
            (
                Some("model_context_window_exceeded"),
                Err("claude_api: stopped (model_context_window_exceeded)"),
            ),
            (Some("pause_turn"), Err("claude_api: stopped (pause_turn)")),
            (None, Err("claude_api: stopped (none)")),
            (Some("end_turn"), Ok("partial")),
            (Some("stop_sequence"), Ok("partial")),
        ] {
            let server = MockServer::start().await;
            Mock::given(method("POST"))
                .respond_with(ResponseTemplate::new(200).set_body_json(reply(stop)))
                .mount(&server)
                .await;
            let got = provider(&server, "low").complete("s", "p").await;
            match (got, want) {
                (Ok(text), Ok(want)) => assert_eq!(text, want, "{stop:?}"),
                (Err(Error::Llm(m)), Err(want)) => assert_eq!(m, want, "{stop:?}"),
                (got, want) => panic!("{stop:?}: {got:?} vs {want:?}"),
            }
        }
    }

    #[tokio::test]
    async fn rejected_key_is_not_echoed() {
        // Review Focus 5
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(401).set_body_json(json!({
                "type": "error",
                "error": {"type": "authentication_error", "message": "invalid x-api-key sk-test-secret"}
            })))
            .mount(&server)
            .await;
        let err = provider(&server, "low")
            .complete("s", "p")
            .await
            .unwrap_err();
        let text = err.to_string();
        assert!(!text.contains("sk-test-secret"), "{text}");
        assert!(text.contains("401"), "{text}");
    }

    #[tokio::test]
    async fn other_api_errors_carry_the_status_and_message() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(529).set_body_json(json!({
                "type": "error", "error": {"type": "overloaded_error", "message": "Overloaded"}
            })))
            .mount(&server)
            .await;
        let err = provider(&server, "low")
            .complete("s", "p")
            .await
            .unwrap_err();
        assert!(
            matches!(&err, Error::Llm(m) if m.contains("529") && m.contains("Overloaded")),
            "{err}"
        );
    }

    #[test]
    fn invalid_effort_or_model_is_rejected_at_once() {
        assert!(matches!(
            ClaudeApiProvider::new("k".into(), "claude-sonnet-5-5", "lowest"),
            Err(Error::Invalid(_))
        ));
        assert!(matches!(
            ClaudeApiProvider::new("k".into(), "-bad", "low"),
            Err(Error::Invalid(_))
        ));
    }

    #[test]
    fn missing_key_is_a_clear_error() {
        // Review Focus 2
        let store = Store::open_in_memory().unwrap();
        for key in [None, Some(String::new()), Some("  ".into())] {
            let err = from_settings(&store, LlmRole::Items, key).unwrap_err();
            assert!(
                matches!(&err, Error::Invalid(m) if m == "ANTHROPIC_API_KEY is not set"),
                "{err}"
            );
        }
    }

    #[test]
    fn effort_follows_the_role_and_debug_hides_the_key() {
        let store = Store::open_in_memory().unwrap();
        crate::seed::seed_defaults(&store).unwrap();
        let items = from_settings(&store, LlmRole::Items, Some("sk-x".into())).unwrap();
        let digest = from_settings(&store, LlmRole::Digest, Some("sk-x".into())).unwrap();
        assert_eq!(items.effort, "low");
        assert_eq!(digest.effort, "");
        assert_eq!(items.name(), "claude_api:claude-sonnet-5-5");
        assert!(!format!("{items:?}").contains("sk-x"));
    }
}
