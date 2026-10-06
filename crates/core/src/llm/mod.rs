pub mod claude;
pub mod ollama;

use async_trait::async_trait;

use crate::error::{Error, Result};
use crate::store::Store;

#[async_trait]
pub trait LlmProvider: Send + Sync {
    /// 期間要約の `provider` 列に記録する名前。
    fn name(&self) -> String;
    async fn complete(&self, system: &str, prompt: &str) -> Result<String>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LlmRole {
    Items,
    Digest,
}

/// LLMの出力を整える。空の出力は失敗として扱う。
pub fn clean_output(raw: &str) -> Result<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(Error::Llm("empty response".into()));
    }
    Ok(trimmed.to_string())
}

/// 設定から、用途に対応するプロバイダーを作る。
/// 設定キー: `llm.items_provider` / `llm.digest_provider` (`ollama` または `claude`)、
/// `ollama.base_url`、`ollama.model`、`claude.bin`。
pub fn provider_from_settings(store: &Store, role: LlmRole) -> Result<Box<dyn LlmProvider>> {
    let key = match role {
        LlmRole::Items => "llm.items_provider",
        LlmRole::Digest => "llm.digest_provider",
    };
    match store.get_setting_or(key, "ollama")?.as_str() {
        "ollama" => Ok(Box::new(ollama::OllamaProvider::new(
            &store.get_setting_or("ollama.base_url", "http://localhost:11434")?,
            &store.get_setting_or("ollama.model", "gemma4:26b")?,
        )?)),
        "claude" => Ok(Box::new(claude::ClaudeProvider::new(
            store.get_setting_or("claude.bin", "claude")?,
        ))),
        other => Err(Error::Invalid(format!("{key}: unknown provider '{other}'"))),
    }
}

#[cfg(test)]
pub(crate) mod mock {
    use std::sync::Mutex;

    use super::*;

    pub struct MockProvider {
        reply: std::result::Result<String, String>,
        calls: Mutex<Vec<(String, String)>>,
    }

    impl MockProvider {
        pub fn ok(reply: &str) -> Self {
            Self { reply: Ok(reply.to_string()), calls: Mutex::new(Vec::new()) }
        }

        pub fn failing(msg: &str) -> Self {
            Self { reply: Err(msg.to_string()), calls: Mutex::new(Vec::new()) }
        }

        pub fn calls(&self) -> Vec<(String, String)> {
            self.calls.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl LlmProvider for MockProvider {
        fn name(&self) -> String {
            "mock".to_string()
        }

        async fn complete(&self, system: &str, prompt: &str) -> Result<String> {
            self.calls
                .lock()
                .unwrap()
                .push((system.to_string(), prompt.to_string()));
            self.reply.clone().map_err(Error::Llm)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_output_trims_and_rejects_empty() {
        // Review Focus 5
        assert_eq!(clean_output("  要約です。\n").unwrap(), "要約です。");
        assert!(matches!(clean_output(""), Err(Error::Llm(_))));
        assert!(matches!(clean_output(" \n\t "), Err(Error::Llm(_))));
    }

    #[test]
    fn provider_from_settings_selects_by_role() {
        let store = Store::open_in_memory().unwrap();
        // 設定がなければ既定のOllama。
        assert_eq!(
            provider_from_settings(&store, LlmRole::Items).unwrap().name(),
            "ollama:gemma4:26b"
        );
        store.set_setting("ollama.model", "other:1b").unwrap();
        store.set_setting("llm.digest_provider", "claude").unwrap();
        assert_eq!(
            provider_from_settings(&store, LlmRole::Items).unwrap().name(),
            "ollama:other:1b"
        );
        assert_eq!(provider_from_settings(&store, LlmRole::Digest).unwrap().name(), "claude");

        store.set_setting("llm.items_provider", "gpt").unwrap();
        assert!(matches!(
            provider_from_settings(&store, LlmRole::Items),
            Err(Error::Invalid(_))
        ));
    }
}
