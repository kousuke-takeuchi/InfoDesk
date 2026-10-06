// 後続タスク (収集、要約、ダイジェスト) が使うまで未使用の項目がある。
#![allow(dead_code)]

use std::sync::{Arc, Mutex};

use chrono::{DateTime, FixedOffset, Local, Utc};
use infodesk_core::Store;
use infodesk_core::llm::{LlmProvider, LlmRole, provider_from_settings};
use serde::Serialize;

use crate::error::{AppError, AppResult};

pub const EVENT_STATUS: &str = "status-changed";
pub const EVENT_ITEMS: &str = "items-changed";
pub const EVENT_DIGESTS: &str = "digests-changed";

#[derive(Debug, Clone, Default, Serialize)]
pub struct AppStatus {
    pub collecting: bool,
    pub summarizing: bool,
    pub digesting: bool,
    pub last_collect_at: Option<DateTime<Utc>>,
    pub next_collect_at: Option<DateTime<Utc>>,
    pub last_error: Option<String>,
}

pub type Notifier = Arc<dyn Fn(&str) + Send + Sync>;
pub type LlmFactory =
    Arc<dyn Fn(&Store, LlmRole) -> infodesk_core::Result<Box<dyn LlmProvider>> + Send + Sync>;

pub struct AppState {
    pub store: Mutex<Store>,
    pub http: reqwest::Client,
    pub status: Mutex<AppStatus>,
    pub tz: FixedOffset,
    pub notify: Notifier,
    pub llm: LlmFactory,
}

impl AppState {
    pub fn new(store: Store, notify: Notifier) -> Self {
        Self {
            store: Mutex::new(store),
            http: infodesk_core::collector::http_client().expect("http client"),
            status: Mutex::new(AppStatus::default()),
            tz: *Local::now().offset(),
            notify,
            llm: Arc::new(provider_from_settings),
        }
    }

    /// DBをロックして同期処理を行う。ロックは関数の間だけ保持する。
    pub fn with_store<T>(
        &self,
        f: impl FnOnce(&Store) -> infodesk_core::Result<T>,
    ) -> AppResult<T> {
        let store = self
            .store
            .lock()
            .map_err(|_| AppError::from("database lock poisoned".to_string()))?;
        Ok(f(&store)?)
    }

    pub fn status(&self) -> AppStatus {
        self.status.lock().expect("status lock").clone()
    }

    pub fn update_status(&self, f: impl FnOnce(&mut AppStatus)) {
        {
            let mut status = self.status.lock().expect("status lock");
            f(&mut status);
        }
        (self.notify)(EVENT_STATUS);
    }

    pub fn notify(&self, event: &str) {
        (self.notify)(event);
    }

    /// 設定からプロバイダーを作る。戻り値はロックに依存しないので、`await` をまたいで使える。
    pub fn llm_provider(&self, role: LlmRole) -> AppResult<Box<dyn LlmProvider>> {
        let factory = self.llm.clone();
        self.with_store(|store| factory(store, role))
    }
}

#[cfg(test)]
pub(crate) fn test_state() -> (AppState, Arc<Mutex<Vec<String>>>) {
    let events = Arc::new(Mutex::new(Vec::new()));
    let sink = events.clone();
    let store = Store::open_in_memory().unwrap();
    infodesk_core::seed::seed_defaults(&store).unwrap();
    let state = AppState::new(
        store,
        Arc::new(move |name: &str| sink.lock().unwrap().push(name.to_string())),
    );
    (state, events)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn with_store_maps_core_errors() {
        let (state, _) = test_state();
        let n = state.with_store(|s| Ok(s.list_sources()?.len())).unwrap();
        assert_eq!(n, 22);
        let err = state
            .with_store(|_| Err::<(), _>(infodesk_core::Error::Invalid("x".into())))
            .unwrap_err();
        assert_eq!(err.message, "invalid value: x");
    }

    #[test]
    fn update_status_notifies() {
        let (state, events) = test_state();
        state.update_status(|s| s.collecting = true);
        assert!(state.status().collecting);
        assert_eq!(events.lock().unwrap().as_slice(), [EVENT_STATUS]);
    }

    #[test]
    fn llm_provider_reads_settings() {
        let (state, _) = test_state();
        assert_eq!(
            state.llm_provider(LlmRole::Digest).unwrap().name(),
            "ollama:gemma4:26b"
        );
        state
            .with_store(|s| s.set_setting("llm.digest_provider", "claude"))
            .unwrap();
        assert_eq!(
            state.llm_provider(LlmRole::Digest).unwrap().name(),
            "claude"
        );
    }
}
