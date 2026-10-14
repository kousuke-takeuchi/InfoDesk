use chrono::{DateTime, Utc};
use infodesk_core::Store;
use infodesk_core::collector::{apply_outcomes, fetch_all, images};
use infodesk_core::llm::LlmRole;
use infodesk_core::model::{Category, Digest, DigestScope, DigestStatus, Period};
use infodesk_core::period::{period_start, utc_range};
use infodesk_core::summarizer::{
    DigestOutcome, finish_digest, finish_item_summary, plan_digest, plan_item_summaries,
};
use serde::Serialize;

use crate::error::AppResult;
use crate::state::{AppState, EVENT_DIGESTS, EVENT_ITEMS, ErrorOrigin};

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct CollectSummary {
    pub new_items: usize,
    pub failed_sources: Vec<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ItemSummaryTotals {
    pub ok: usize,
    pub failed: usize,
    pub error: Option<String>,
}

pub fn all_category_scopes() -> Vec<DigestScope> {
    Category::ALL
        .into_iter()
        .map(DigestScope::Category)
        .collect()
}

fn now_utc() -> DateTime<Utc> {
    Utc::now()
}

/// ロックを短く2回取り、間で `await` する。
/// 1. 計画 (ロック内) 2. LLM (ロック外) 3. 保存 (ロック内)
pub async fn run_item_summaries(
    state: &AppState,
    since: DateTime<Utc>,
) -> AppResult<ItemSummaryTotals> {
    let limit: u32 = state
        .with_store(|s| s.get_setting_or("summary.top_n", "20"))?
        .parse()
        .unwrap_or(20);
    let llm = state.llm_provider(LlmRole::Items)?;
    let mut totals = ItemSummaryTotals {
        ok: 0,
        failed: 0,
        error: None,
    };
    state.update_status(|s| s.summarizing = true);
    // エラーでも必ずフラグを戻すため、本体を内側のブロックで実行する。
    let result: AppResult<()> = async {
        'categories: for category in Category::ALL {
            let jobs = state.with_store(|s| plan_item_summaries(s, category, &since, limit))?;
            for job in jobs {
                let output = llm.complete(&job.system, &job.prompt).await;
                match state.with_store(|s| finish_item_summary(s, job.item_id, output))? {
                    Ok(()) => totals.ok += 1,
                    Err(message) => {
                        totals.failed += 1;
                        totals.error.get_or_insert(message);
                        break 'categories;
                    }
                }
            }
        }
        Ok(())
    }
    .await;
    state.update_status(|s| s.summarizing = false);
    if totals.ok > 0 {
        state.notify(EVENT_ITEMS);
    }
    result.map(|()| totals)
}

/// 期間要約を順に生成する。各 scope の結果 (`None` は生成不要) を返す。
pub async fn run_digests(
    state: &AppState,
    period: Period,
    date: chrono::NaiveDate,
    scopes: &[DigestScope],
    force: bool,
) -> AppResult<Vec<(DigestScope, Option<DigestOutcome>)>> {
    let tz = state.tz;
    // 他の要約ジョブと重なってもよいよう、フラグではなくジョブ数で数える。
    let job_guard = state.begin_digest_job();
    let mut results = Vec::new();
    let mut first_error: Option<String> = None;
    // エラーでも必ず完了を通知するため、本体を内側のブロックで実行する。
    let result: AppResult<()> = async {
        let llm = state.llm_provider(LlmRole::Digest)?;
        for scope in scopes {
            let job = state.with_store(|s| plan_digest(s, period, date, scope, tz, force))?;
            let outcome = match job {
                None => None,
                Some(job) => {
                    let output = llm.complete(&job.system, &job.prompt).await;
                    let outcome = state
                        .with_store(|s| finish_digest(s, &job, &llm.name(), output, &now_utc()))?;
                    if let Some(e) = &outcome.error {
                        first_error.get_or_insert(e.clone());
                    }
                    Some(outcome)
                }
            };
            results.push((scope.clone(), outcome));
        }
        Ok(())
    }
    .await;
    if let Some(e) = first_error {
        state.update_status(|s| s.set_error(ErrorOrigin::Job, e));
    }
    // `digests-changed` を受けた画面が古い `digesting` を見ないよう、先にジョブ数を戻す。
    drop(job_guard);
    state.notify(EVENT_DIGESTS);
    if result.is_ok() {
        state.request_sync();
    }
    result.map(|()| results)
}

/// 保留中 (`pending`) の要約を再試行する。成功した件数を返す。スケジューラーが使う。
pub async fn retry_pending(state: &AppState) -> AppResult<usize> {
    retry_from(state, Store::pending_digests).await
}

/// 保留中と失敗 (`failed`) の要約を再試行する。成功した件数を返す。手動の再試行で使う。
pub async fn retry_all(state: &AppState) -> AppResult<usize> {
    retry_from(state, Store::retryable_digests).await
}

async fn retry_from(
    state: &AppState,
    list: fn(&Store) -> infodesk_core::Result<Vec<Digest>>,
) -> AppResult<usize> {
    let pending = state.with_store(list)?;
    let had_pending = !pending.is_empty();
    let mut recovered = 0;
    for d in &pending {
        let results = run_digests(
            state,
            d.period,
            d.period_start,
            std::slice::from_ref(&d.scope),
            false,
        )
        .await?;
        if let Some((_, Some(outcome))) = results.first()
            && outcome.error.is_none()
        {
            recovered += 1;
        }
    }
    // 再試行した行がすべて生成済みになったときだけ `last_error` を消す。一部だけ
    // 復旧した場合や、保留から失敗に変わった場合は、エラーを残す。もともと何も
    // 保留していなければ、他のジョブのエラーを消さないよう `last_error` に触れない。
    let all_done = state.with_store(|s| {
        for d in &pending {
            let now = s.get_digest(d.period, d.period_start, &d.scope)?;
            if now.is_none_or(|n| n.status != DigestStatus::Done) {
                return Ok(false);
            }
        }
        Ok(true)
    })?;
    if had_pending && all_done {
        state.update_status(|s| s.last_error = None);
    }
    Ok(recovered)
}

/// 有効なソースがすべて取得に失敗したときの `last_error`。
pub const ALL_SOURCES_FAILED: &str = "全ソースの取得に失敗しました";

/// 全ソースが失敗したとき、次の収集までの待ち時間。
const RETRY_AFTER_ALL_FAILED: chrono::Duration = chrono::Duration::minutes(10);

/// 収集中に再度 `run_collect` を呼んだときのエラーメッセージ。
pub const COLLECT_ALREADY_RUNNING: &str = "collect already running";

pub fn is_collecting(state: &AppState) -> bool {
    state.status().collecting
}

/// 収集と記事要約。日次要約は作らない (スケジュールの日次ジョブが作る)。収集中なら拒否する (`last_error` は変えない)。
pub async fn run_collect(state: &AppState) -> AppResult<CollectSummary> {
    {
        let mut status = state.status.lock().expect("status lock");
        if status.collecting {
            return Err(COLLECT_ALREADY_RUNNING.to_string().into());
        }
        status.collecting = true;
    }
    state.notify(crate::state::EVENT_STATUS);

    let result = collect_inner(state).await;
    state.update_status(|s| {
        s.collecting = false;
        if let Err(e) = &result {
            s.set_error(ErrorOrigin::Collect, e.message.clone());
        }
    });
    if result.is_ok() {
        state.request_sync();
    }
    result
}

async fn collect_inner(state: &AppState) -> AppResult<CollectSummary> {
    let sources = state.with_store(|s| s.list_sources())?;
    let mut outcomes = fetch_all(&state.http, &sources).await;
    let urls: Vec<String> = outcomes
        .iter()
        .filter_map(|o| o.result.as_ref().ok())
        .flatten()
        .map(|i| i.url.clone())
        .collect();
    let known = state.with_store(|s| s.existing_urls(&urls))?;
    images::fill_page_images(&mut outcomes, &known).await;
    let now = now_utc();
    let reports = state.with_store(|s| apply_outcomes(s, &outcomes, &now))?;
    let summary = CollectSummary {
        new_items: reports.iter().map(|r| r.new_items).sum(),
        failed_sources: reports
            .iter()
            .filter(|r| r.error.is_some())
            .map(|r| r.name.clone())
            .collect(),
    };
    if !reports.is_empty() && reports.iter().all(|r| r.error.is_some()) {
        // 有効なソースがすべて失敗した。ソースごとのエラーは行に残っている。
        // 最終収集時刻を前にずらし、スケジューラーが約10分後 (間隔がそれより短ければ間隔どおり)
        // に再試行するようにする。ずらした時刻が未来になることはない。
        // 新しい記事がないので、要約は作らない。
        let interval = crate::scheduler::ScheduleConfig::from_settings(|k| {
            state.with_store(|s| s.get_setting(k)).ok().flatten()
        })
        .collect_interval;
        let retry_base = (now - interval + RETRY_AFTER_ALL_FAILED.min(interval)).min(now);
        state
            .with_store(|s| s.set_setting("schedule.last_collect_at", &retry_base.to_rfc3339()))?;
        state.update_status(|s| s.set_error(ErrorOrigin::Collect, ALL_SOURCES_FAILED));
        return Ok(summary);
    }
    state.with_store(|s| s.set_setting("schedule.last_collect_at", &now.to_rfc3339()))?;
    state.update_status(|s| {
        s.last_collect_at = Some(now);
        // 要約やスケジュールのジョブのエラーは、それぞれの成功まで残す。
        s.clear_collect_error();
    });
    state.notify(EVENT_ITEMS);

    let today = now.with_timezone(&state.tz).date_naive();
    let (since, _) = utc_range(Period::Day, period_start(Period::Day, today), state.tz);
    let totals = run_item_summaries(state, since).await?;
    if let Some(e) = &totals.error {
        state.update_status(|s| s.set_error(ErrorOrigin::Collect, e.clone()));
    }
    Ok(summary)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::test_state;
    use async_trait::async_trait;
    use chrono::Local;
    use infodesk_core::llm::LlmProvider;
    use infodesk_core::model::{ItemFilter, SourceKind};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    struct Mock1(Result<String, String>);

    #[async_trait]
    impl LlmProvider for Mock1 {
        fn name(&self) -> String {
            "mock".into()
        }
        async fn complete(&self, _s: &str, _p: &str) -> infodesk_core::Result<String> {
            self.0.clone().map_err(infodesk_core::Error::Llm)
        }
    }

    fn use_mock(state: &mut AppState, reply: Result<&str, &str>) {
        let reply = reply.map(String::from).map_err(String::from);
        state.llm =
            Arc::new(move |_, _| Ok(Box::new(Mock1(reply.clone())) as Box<dyn LlmProvider>));
    }

    /// 収集の後に当日の日次要約を作る。収集自体は要約を作らないので、保留の要約を用意する。
    async fn collect_with_digest(state: &AppState) {
        run_collect(state).await.unwrap();
        let today = Local::now().date_naive();
        let _ = run_digests(state, Period::Day, today, &all_category_scopes(), true).await;
    }

    /// 全ソースを無効にし、wiremock のRSS 1件だけ有効にする。
    async fn one_rss_source(state: &AppState, server: &MockServer, status: u16) -> i64 {
        Mock::given(method("GET")).and(path("/feed"))
            .respond_with(ResponseTemplate::new(status).set_body_string(
                r#"<rss version="2.0"><channel><title>T</title><item><title>認知症の記事</title><link>https://e.com/1</link><description>本文</description></item></channel></rss>"#,
            ))
            .mount(server).await;
        state
            .with_store(|s| {
                for src in s.list_sources()? {
                    s.set_source_enabled(src.id, false)?;
                }
                s.add_source(
                    SourceKind::Rss,
                    "Mock",
                    Category::Medcare,
                    &format!("{}/feed", server.uri()),
                )
            })
            .unwrap()
    }

    #[tokio::test]
    async fn collect_fetches_and_summarizes_items_without_building_a_digest() {
        let server = MockServer::start().await;
        let (mut state, events) = test_state();
        one_rss_source(&state, &server, 200).await;
        use_mock(&mut state, Ok("日本語の要約。"));

        let summary = run_collect(&state).await.unwrap();
        assert_eq!(summary.new_items, 1);
        assert!(summary.failed_sources.is_empty());

        let items = state
            .with_store(|s| s.query_items(&ItemFilter::default()))
            .unwrap();
        assert_eq!(items[0].ai_summary.as_deref(), Some("日本語の要約。"));
        let today = Local::now().date_naive();
        for period in [Period::Day, Period::Week, Period::Month] {
            assert!(
                state
                    .with_store(|s| s.get_digest(
                        period,
                        today,
                        &DigestScope::Category(Category::Medcare)
                    ))
                    .unwrap()
                    .is_none()
            );
        }
        assert!(
            state
                .with_store(|s| s.pending_digests())
                .unwrap()
                .is_empty()
        );
        let st = state.status();
        assert!(!st.collecting && !st.summarizing && !st.digesting);
        assert!(st.last_collect_at.is_some());
        assert_eq!(st.last_error, None);
        let ev = events.lock().unwrap();
        assert!(ev.contains(&EVENT_ITEMS.to_string()));
        assert!(!ev.contains(&EVENT_DIGESTS.to_string()));
    }

    #[tokio::test]
    async fn collect_with_llm_down_leaves_pending_and_records_error() {
        // Review Focus 2
        let server = MockServer::start().await;
        let (mut state, _) = test_state();
        one_rss_source(&state, &server, 200).await;
        use_mock(&mut state, Err("connection refused"));

        let summary = run_collect(&state).await.unwrap();
        assert_eq!(summary.new_items, 1);
        let items = state
            .with_store(|s| s.query_items(&ItemFilter::default()))
            .unwrap();
        assert_eq!(items[0].ai_summary, None);
        // 収集は要約を作らないので、保留の要約は日次の生成で作る。
        assert!(
            state
                .with_store(|s| s.pending_digests())
                .unwrap()
                .is_empty()
        );
        let today = Local::now().date_naive();
        let _ = run_digests(&state, Period::Day, today, &all_category_scopes(), true).await;
        let pending = state.with_store(|s| s.pending_digests()).unwrap();
        assert_eq!(pending.len(), 1);
        assert!(
            pending[0]
                .last_error
                .as_deref()
                .unwrap()
                .contains("connection refused")
        );
        assert!(
            state
                .status()
                .last_error
                .unwrap()
                .contains("connection refused")
        );
    }

    #[tokio::test]
    async fn collect_records_failed_sources_without_stopping() {
        let server = MockServer::start().await;
        let (mut state, _) = test_state();
        one_rss_source(&state, &server, 500).await;
        use_mock(&mut state, Ok("x"));
        let summary = run_collect(&state).await.unwrap();
        assert_eq!(summary.new_items, 0);
        assert_eq!(summary.failed_sources, vec!["Mock".to_string()]);
    }

    #[tokio::test]
    async fn collect_with_every_source_failing_records_error_and_retries_soon() {
        let server = MockServer::start().await;
        let (mut state, _) = test_state();
        one_rss_source(&state, &server, 500).await;
        use_mock(&mut state, Ok("x"));
        collect_with_digest(&state).await;
        assert_eq!(
            state.status().last_error.as_deref(),
            Some(ALL_SOURCES_FAILED)
        );
        // ソースごとのエラーは行に残る。
        let src = state
            .with_store(|s| s.list_sources())
            .unwrap()
            .into_iter()
            .find(|s| s.name == "Mock")
            .unwrap();
        assert!(src.last_error.is_some());
        // 次の収集が約10分後になるよう、最終収集時刻を前にずらして記録する。
        let stored = state
            .with_store(|s| s.get_setting("schedule.last_collect_at"))
            .unwrap()
            .unwrap();
        let stored = DateTime::parse_from_rfc3339(&stored)
            .unwrap()
            .with_timezone(&Utc);
        let expected = now_utc() - chrono::Duration::hours(3) + chrono::Duration::minutes(10);
        assert!(
            (stored - expected).num_seconds().abs() < 60,
            "stored {stored}, expected about {expected}"
        );
    }

    fn stored_last_collect_at(state: &AppState) -> DateTime<Utc> {
        let stored = state
            .with_store(|s| s.get_setting("schedule.last_collect_at"))
            .unwrap()
            .unwrap();
        DateTime::parse_from_rfc3339(&stored)
            .unwrap()
            .with_timezone(&Utc)
    }

    #[tokio::test]
    async fn every_source_failing_at_a_short_interval_retries_at_the_normal_cadence() {
        let server = MockServer::start().await;
        let (mut state, _) = test_state();
        one_rss_source(&state, &server, 500).await;
        use_mock(&mut state, Ok("x"));
        state
            .with_store(|s| s.set_setting("schedule.collect_interval_minutes", "1"))
            .unwrap();
        let before = now_utc();
        run_collect(&state).await.unwrap();
        let after = now_utc();
        assert_eq!(
            state.status().last_error.as_deref(),
            Some(ALL_SOURCES_FAILED)
        );
        // 間隔が10分より短いときは、間隔どおりに再試行する。最終収集時刻は未来にならない。
        let stored = stored_last_collect_at(&state);
        assert!(stored <= after, "stored {stored} is after {after}");
        assert!(stored >= before, "stored {stored} is before {before}");
    }

    #[tokio::test]
    async fn every_source_failing_never_stores_a_future_collect_time() {
        let server = MockServer::start().await;
        let (mut state, _) = test_state();
        one_rss_source(&state, &server, 500).await;
        use_mock(&mut state, Ok("x"));
        for minutes in ["1", "5", "10", "15"] {
            state
                .with_store(|s| s.set_setting("schedule.collect_interval_minutes", minutes))
                .unwrap();
            run_collect(&state).await.unwrap();
            let stored = stored_last_collect_at(&state);
            assert!(stored <= now_utc(), "interval {minutes}: stored {stored}");
        }
        // 15分間隔なら 10分後に再試行する (最終収集時刻は5分前)。
        let expected = now_utc() - chrono::Duration::minutes(5);
        let stored = stored_last_collect_at(&state);
        assert!(
            (stored - expected).num_seconds().abs() < 60,
            "stored {stored}"
        );
    }

    #[tokio::test]
    async fn successful_collect_clears_an_earlier_collect_error() {
        let server = MockServer::start().await;
        let (mut state, _) = test_state();
        // 最初の取得だけ失敗させる。
        Mock::given(method("GET"))
            .and(path("/feed"))
            .respond_with(ResponseTemplate::new(500))
            .up_to_n_times(1)
            .mount(&server)
            .await;
        one_rss_source(&state, &server, 200).await;
        use_mock(&mut state, Ok("要約。"));
        run_collect(&state).await.unwrap();
        assert_eq!(
            state.status().last_error.as_deref(),
            Some(ALL_SOURCES_FAILED)
        );
        run_collect(&state).await.unwrap();
        assert_eq!(state.status().last_error, None);
    }

    #[tokio::test]
    async fn successful_collect_clears_an_item_summary_error_from_collect() {
        let server = MockServer::start().await;
        let (mut state, _) = test_state();
        one_rss_source(&state, &server, 200).await;
        use_mock(&mut state, Err("connection refused"));
        run_collect(&state).await.unwrap();
        assert!(
            state
                .status()
                .last_error
                .unwrap()
                .contains("connection refused")
        );
        use_mock(&mut state, Ok("要約。"));
        run_collect(&state).await.unwrap();
        assert_eq!(state.status().last_error, None);
    }

    #[tokio::test]
    async fn successful_collect_keeps_a_digest_error() {
        let server = MockServer::start().await;
        let (mut state, _) = test_state();
        one_rss_source(&state, &server, 200).await;
        use_mock(&mut state, Ok("要約。"));
        run_collect(&state).await.unwrap();
        // 日次要約の生成に失敗する。
        use_mock(&mut state, Err("model not found"));
        let today = Local::now().date_naive();
        let _ = run_digests(&state, Period::Day, today, &all_category_scopes(), true).await;
        assert!(
            state
                .status()
                .last_error
                .unwrap()
                .contains("model not found")
        );
        // 次の収集が成功しても、要約のエラーは要約の成功まで残す。
        use_mock(&mut state, Ok("要約。"));
        run_collect(&state).await.unwrap();
        assert!(
            state
                .status()
                .last_error
                .unwrap()
                .contains("model not found")
        );
        assert_eq!(retry_pending(&state).await.unwrap(), 1);
        assert_eq!(state.status().last_error, None);
    }

    #[tokio::test]
    async fn successful_collect_keeps_a_job_error() {
        let server = MockServer::start().await;
        let (mut state, _) = test_state();
        one_rss_source(&state, &server, 200).await;
        use_mock(&mut state, Ok("要約。"));
        state.update_status(|s| s.set_error(ErrorOrigin::Job, "weekly failed"));
        run_collect(&state).await.unwrap();
        assert_eq!(state.status().last_error.as_deref(), Some("weekly failed"));
        // 収集のエラーで上書きされたら、それは次の成功で消える。
        state.update_status(|s| s.set_error(ErrorOrigin::Collect, "collect failed"));
        run_collect(&state).await.unwrap();
        assert_eq!(state.status().last_error, None);
    }

    #[tokio::test]
    async fn collect_and_digests_request_a_sync_but_a_rejected_collect_does_not() {
        let (mut state, _) = test_state();
        use_mock(&mut state, Ok("要約"));
        let calls = Arc::new(AtomicUsize::new(0));
        let c = calls.clone();
        state.set_sync_launcher(Arc::new(move || {
            c.fetch_add(1, Ordering::SeqCst);
        }));
        run_collect(&state).await.unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        let today = Local::now().date_naive();
        run_digests(&state, Period::Day, today, &all_category_scopes(), true)
            .await
            .unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        state.update_status(|s| s.collecting = true);
        assert!(run_collect(&state).await.is_err());
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn second_collect_while_running_is_rejected() {
        // Review Focus 1
        let (state, _) = test_state();
        state.update_status(|s| s.collecting = true);
        let err = run_collect(&state).await.unwrap_err();
        assert!(err.message.contains("already running"), "{}", err.message);
    }

    #[tokio::test]
    async fn retry_pending_regenerates_when_llm_is_back() {
        let server = MockServer::start().await;
        let (mut state, _) = test_state();
        one_rss_source(&state, &server, 200).await;
        use_mock(&mut state, Err("down"));
        collect_with_digest(&state).await;
        assert_eq!(state.with_store(|s| s.pending_digests()).unwrap().len(), 1);

        use_mock(&mut state, Ok("復旧"));
        assert_eq!(retry_pending(&state).await.unwrap(), 1);
        assert!(
            state
                .with_store(|s| s.pending_digests())
                .unwrap()
                .is_empty()
        );
        assert_eq!(state.status().last_error, None);
    }

    /// 最初の `ok_first` 回だけ成功し、以降は失敗する。呼び出し回数は全インスタンスで共有する。
    struct SeqMock {
        calls: Arc<AtomicUsize>,
        ok_first: usize,
    }

    #[async_trait]
    impl LlmProvider for SeqMock {
        fn name(&self) -> String {
            "seq".into()
        }
        async fn complete(&self, _s: &str, _p: &str) -> infodesk_core::Result<String> {
            if self.calls.fetch_add(1, Ordering::SeqCst) < self.ok_first {
                Ok("復旧".into())
            } else {
                Err(infodesk_core::Error::Llm("still down".into()))
            }
        }
    }

    fn use_seq_mock(state: &mut AppState, ok_first: usize) {
        let calls = Arc::new(AtomicUsize::new(0));
        state.llm = Arc::new(move |_, _| {
            Ok(Box::new(SeqMock {
                calls: calls.clone(),
                ok_first,
            }) as Box<dyn LlmProvider>)
        });
    }

    /// 全ソースを無効にし、カテゴリーの異なる wiremock のRSS 2件を有効にする。
    async fn two_rss_sources(state: &AppState, server: &MockServer) {
        for (p, link) in [("/feed1", "https://e.com/a"), ("/feed2", "https://e.com/b")] {
            Mock::given(method("GET"))
                .and(path(p))
                .respond_with(ResponseTemplate::new(200).set_body_string(format!(
                    r#"<rss version="2.0"><channel><title>T</title><item><title>記事</title><link>{link}</link><description>本文</description></item></channel></rss>"#
                )))
                .mount(server)
                .await;
        }
        state
            .with_store(|s| {
                for src in s.list_sources()? {
                    s.set_source_enabled(src.id, false)?;
                }
                s.add_source(
                    SourceKind::Rss,
                    "Mock1",
                    Category::Medcare,
                    &format!("{}/feed1", server.uri()),
                )?;
                s.add_source(
                    SourceKind::Rss,
                    "Mock2",
                    Category::TechJp,
                    &format!("{}/feed2", server.uri()),
                )
            })
            .unwrap();
    }

    async fn two_pending(state: &mut AppState) -> MockServer {
        let server = MockServer::start().await;
        two_rss_sources(state, &server).await;
        use_mock(state, Err("down"));
        collect_with_digest(state).await;
        assert_eq!(state.with_store(|s| s.pending_digests()).unwrap().len(), 2);
        assert!(state.status().last_error.is_some());
        server
    }

    #[tokio::test]
    async fn retry_pending_full_recovery_clears_last_error() {
        let (mut state, _) = test_state();
        let _server = two_pending(&mut state).await;
        use_mock(&mut state, Ok("復旧"));
        assert_eq!(retry_pending(&state).await.unwrap(), 2);
        assert!(
            state
                .with_store(|s| s.pending_digests())
                .unwrap()
                .is_empty()
        );
        assert_eq!(state.status().last_error, None);
    }

    #[tokio::test]
    async fn retry_pending_still_failing_keeps_last_error() {
        let (mut state, _) = test_state();
        let _server = two_pending(&mut state).await;
        use_mock(&mut state, Err("still down"));
        assert_eq!(retry_pending(&state).await.unwrap(), 0);
        assert_eq!(state.with_store(|s| s.pending_digests()).unwrap().len(), 2);
        assert!(state.status().last_error.is_some());
    }

    #[tokio::test]
    async fn retry_pending_partial_recovery_keeps_last_error() {
        let (mut state, _) = test_state();
        let _server = two_pending(&mut state).await;
        use_seq_mock(&mut state, 1);
        assert_eq!(retry_pending(&state).await.unwrap(), 1);
        assert_eq!(state.with_store(|s| s.pending_digests()).unwrap().len(), 1);
        assert!(
            state
                .status()
                .last_error
                .as_deref()
                .unwrap()
                .contains("still down")
        );
    }

    #[tokio::test]
    async fn retry_all_includes_failed_rows_but_retry_pending_does_not() {
        let server = MockServer::start().await;
        let (mut state, _) = test_state();
        one_rss_source(&state, &server, 200).await;
        use_mock(&mut state, Err("down"));
        collect_with_digest(&state).await;
        // 3回目の失敗で failed になる。
        retry_pending(&state).await.unwrap();
        retry_pending(&state).await.unwrap();
        assert!(
            state
                .with_store(|s| s.pending_digests())
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            state.with_store(|s| s.retryable_digests()).unwrap().len(),
            1
        );

        use_mock(&mut state, Ok("復旧"));
        assert_eq!(retry_pending(&state).await.unwrap(), 0);
        assert!(state.status().last_error.is_some());
        assert_eq!(retry_all(&state).await.unwrap(), 1);
        assert!(
            state
                .with_store(|s| s.retryable_digests())
                .unwrap()
                .is_empty()
        );
        assert_eq!(state.status().last_error, None);
    }

    #[tokio::test]
    async fn retry_pending_with_nothing_pending_keeps_last_error() {
        let (mut state, _) = test_state();
        use_mock(&mut state, Ok("x"));
        state.update_status(|s| s.last_error = Some("x".into()));
        assert_eq!(retry_pending(&state).await.unwrap(), 0);
        assert_eq!(state.status().last_error.as_deref(), Some("x"));
    }

    #[tokio::test]
    async fn run_digests_error_resets_digesting_flag() {
        let (mut state, _) = test_state();
        use_mock(&mut state, Ok("x"));
        let today = Local::now().date_naive();
        let result = run_digests(
            &state,
            Period::Day,
            today,
            &[DigestScope::Topic(9999)],
            true,
        )
        .await;
        assert!(result.is_err());
        assert!(!state.status().digesting);
    }

    #[tokio::test]
    async fn run_digests_error_leaves_other_running_job_counted() {
        let (mut state, _) = test_state();
        use_mock(&mut state, Ok("x"));
        let today = Local::now().date_naive();
        let other = state.begin_digest_job();
        let result = run_digests(
            &state,
            Period::Day,
            today,
            &[DigestScope::Topic(9999)],
            true,
        )
        .await;
        assert!(result.is_err());
        // 失敗したジョブは自分の分だけ戻す。もう一方が動いている間は digesting のまま。
        assert!(state.status().digesting);
        drop(other);
        assert!(!state.status().digesting);
    }

    #[tokio::test]
    async fn run_digests_without_provider_still_notifies_and_resets() {
        let (mut state, events) = test_state();
        state.llm = Arc::new(|_, _| Err(infodesk_core::Error::Invalid("no provider".into())));
        let today = Local::now().date_naive();
        let result = run_digests(&state, Period::Day, today, &all_category_scopes(), true).await;
        assert!(result.is_err());
        assert!(!state.status().digesting);
        // 画面の「生成中」を解除できるよう、失敗しても完了を通知する。
        assert!(events.lock().unwrap().contains(&EVENT_DIGESTS.to_string()));
    }

    #[tokio::test]
    async fn item_summaries_with_bad_top_n_resets_summarizing_flag() {
        let (mut state, _) = test_state();
        use_mock(&mut state, Ok("x"));
        state
            .with_store(|s| s.set_setting("summary.top_n", "abc"))
            .unwrap();
        let totals = run_item_summaries(&state, now_utc()).await.unwrap();
        assert_eq!(totals.failed, 0);
        assert!(!state.status().summarizing);
    }

    #[tokio::test]
    async fn rejected_collect_leaves_last_error_untouched() {
        let (state, _) = test_state();
        assert!(!is_collecting(&state));
        state.update_status(|s| s.collecting = true);
        assert!(is_collecting(&state));
        let err = run_collect(&state).await.unwrap_err();
        assert_eq!(err.message, COLLECT_ALREADY_RUNNING);
        assert_eq!(state.status().last_error, None);
        assert!(is_collecting(&state));
    }

    #[tokio::test]
    async fn run_digests_returns_outcome_per_scope() {
        let server = MockServer::start().await;
        let (mut state, _) = test_state();
        one_rss_source(&state, &server, 200).await;
        use_mock(&mut state, Ok("要約"));
        collect_with_digest(&state).await;
        let today = Local::now().date_naive();
        let results = run_digests(&state, Period::Day, today, &all_category_scopes(), false)
            .await
            .unwrap();
        assert_eq!(results.len(), 7);
        // medcare は直前の生成で作成済みなので None (再生成しない)。他は記事なしで None。
        assert!(results.iter().all(|(_, o)| o.is_none()));
        let results = run_digests(
            &state,
            Period::Day,
            today,
            &[DigestScope::Category(Category::Medcare)],
            true,
        )
        .await
        .unwrap();
        assert_eq!(results[0].1.as_ref().unwrap().error, None);
    }
}
