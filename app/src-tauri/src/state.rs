use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use chrono::{DateTime, FixedOffset, Local, Utc};
use infodesk_core::Store;
use infodesk_core::llm::{LlmProvider, LlmRole, provider_from_settings};
use serde::Serialize;

use crate::error::{AppError, AppResult};

pub const EVENT_STATUS: &str = "status-changed";
pub const EVENT_ITEMS: &str = "items-changed";
pub const EVENT_DIGESTS: &str = "digests-changed";
pub const EVENT_CHAT: &str = "chat-endpoint-changed";

#[derive(Debug, Clone, Default, Serialize)]
pub struct AppStatus {
    pub collecting: bool,
    pub summarizing: bool,
    pub digesting: bool,
    pub last_collect_at: Option<DateTime<Utc>>,
    pub next_collect_at: Option<DateTime<Utc>>,
    pub last_error: Option<String>,
    /// `last_error` を記録した処理。収集の成功は収集のエラーだけを消す。
    #[serde(skip)]
    pub error_origin: ErrorOrigin,
}

/// `last_error` の出どころ。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ErrorOrigin {
    /// 収集 (取得の失敗、全ソース失敗、収集中の記事要約の失敗)。次の収集の成功で消える。
    Collect,
    /// 期間要約やスケジュールのジョブなど、収集以外。それぞれの成功で消える。
    #[default]
    Job,
}

impl AppStatus {
    /// エラーを出どころと一緒に記録する。`last_error` はこの関数で設定する。
    pub fn set_error(&mut self, origin: ErrorOrigin, message: impl Into<String>) {
        self.last_error = Some(message.into());
        self.error_origin = origin;
    }

    /// 収集が成功したときに呼ぶ。収集が記録したエラーだけを消し、要約やジョブのエラーは残す。
    pub fn clear_collect_error(&mut self) {
        if self.error_origin == ErrorOrigin::Collect {
            self.last_error = None;
        }
    }
}

pub type Notifier = Arc<dyn Fn(&str) + Send + Sync>;
pub type LlmFactory =
    Arc<dyn Fn(&Store, LlmRole) -> infodesk_core::Result<Box<dyn LlmProvider>> + Send + Sync>;

pub struct AppState {
    pub store: Mutex<Store>,
    pub http: reqwest::Client,
    pub status: Mutex<AppStatus>,
    /// 実行中の期間要約ジョブの数。`status` のロック中にだけ書き換える。
    digest_jobs: AtomicU32,
    pub tz: FixedOffset,
    pub notify: Notifier,
    pub llm: LlmFactory,
    pub chat: crate::sidecar::Sidecar,
    /// 閲覧ウィンドウの記事ページのタイトル。
    pub reader_title: Mutex<String>,
    /// 閲覧ウィンドウの作成を1つずつにする。中で `.await` しないこと。
    pub reader_lock: Mutex<()>,
}

impl AppState {
    pub fn new(store: Store, notify: Notifier) -> Self {
        Self {
            store: Mutex::new(store),
            http: infodesk_core::collector::http_client().expect("http client"),
            status: Mutex::new(AppStatus::default()),
            digest_jobs: AtomicU32::new(0),
            tz: *Local::now().offset(),
            notify,
            llm: Arc::new(provider_from_settings),
            chat: crate::sidecar::Sidecar::default(),
            reader_title: Mutex::new(String::new()),
            reader_lock: Mutex::new(()),
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

    /// 前回起動時までに保存した状態を読み込む。起動直後に1回呼ぶ。通知はしない。
    pub fn load_persisted(&self) {
        let last = self
            .with_store(|s| s.get_setting("schedule.last_collect_at"))
            .ok()
            .flatten()
            .and_then(|v| DateTime::parse_from_rfc3339(&v).ok())
            .map(|d| d.with_timezone(&Utc));
        if last.is_some() {
            self.status.lock().expect("status lock").last_collect_at = last;
        }
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

    /// 期間要約ジョブの開始を記録する。戻り値を破棄した時点で終了とみなす。
    /// ジョブが重なっても、`digesting` は最後の1つが終わるまで `true` のまま。
    pub fn begin_digest_job(&self) -> DigestJob<'_> {
        self.count_digest_job(|n| n + 1);
        DigestJob { state: self }
    }

    /// ジョブ数を更新し、`digesting` が切り替わったときだけ通知する。
    fn count_digest_job(&self, change: impl FnOnce(u32) -> u32) {
        let flipped = {
            // 破棄中にも呼ぶので、ロックが汚染されていても続行する。
            let mut status = self.status.lock().unwrap_or_else(|e| e.into_inner());
            let jobs = change(self.digest_jobs.load(Ordering::SeqCst));
            self.digest_jobs.store(jobs, Ordering::SeqCst);
            let digesting = jobs > 0;
            let flipped = status.digesting != digesting;
            status.digesting = digesting;
            flipped
        };
        if flipped {
            (self.notify)(EVENT_STATUS);
        }
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

/// 実行中の期間要約ジョブ1つ分。破棄するとジョブ数を戻す (エラーや中断でも戻る)。
pub struct DigestJob<'a> {
    state: &'a AppState,
}

impl Drop for DigestJob<'_> {
    fn drop(&mut self) {
        self.state.count_digest_job(|n| n.saturating_sub(1));
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
    fn overlapping_digest_jobs_keep_digesting_until_all_finish() {
        let (state, events) = test_state();
        let first = state.begin_digest_job();
        let second = state.begin_digest_job();
        assert!(state.status().digesting);
        drop(first);
        assert!(state.status().digesting, "one job is still running");
        drop(second);
        assert!(!state.status().digesting);
        // 通知は切り替わったときだけ (開始1回、終了1回)。
        assert_eq!(
            events.lock().unwrap().as_slice(),
            [EVENT_STATUS, EVENT_STATUS]
        );
    }

    #[test]
    fn load_persisted_restores_last_collect_at() {
        let (state, _) = test_state();
        state.load_persisted();
        assert_eq!(state.status().last_collect_at, None);
        state
            .with_store(|s| s.set_setting("schedule.last_collect_at", "2026-10-06T09:30:00+09:00"))
            .unwrap();
        state.load_persisted();
        assert_eq!(
            state.status().last_collect_at,
            Some("2026-10-06T00:30:00Z".parse().unwrap())
        );
        // 壊れた値は無視する。
        state
            .with_store(|s| s.set_setting("schedule.last_collect_at", "garbage"))
            .unwrap();
        state.update_status(|s| s.last_collect_at = None);
        state.load_persisted();
        assert_eq!(state.status().last_collect_at, None);
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
