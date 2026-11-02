use std::collections::HashMap;

use chrono::{DateTime, NaiveDate, Utc};
use tauri::{AppHandle, Manager};

use infodesk_core::sync::mode::is_reader;

use crate::jobs;
use crate::state::{AppState, ErrorOrigin};
use crate::sync::SETTING_LAST_SYNC_AT;

pub use infodesk_core::schedule::{
    Job, LastRuns, RECENT_ATTEMPT, ScheduleConfig, TICK, due_jobs, filter_recent, job_plan,
    last_run_record, next_collect_at, retry_due, should_record,
};

fn read_last_runs(state: &AppState) -> LastRuns {
    LastRuns::from_settings(|k| state.with_store(|s| s.get_setting(k)).ok().flatten())
}

fn record(state: &AppState, key: &str, value: &str) {
    let _ = state.with_store(|s| s.set_setting(key, value));
}

/// 1つのジョブを実行する。`today` は `state.tz` での今日。
async fn run_job(state: &AppState, job: &Job, today: NaiveDate) {
    if let Job::Collect = job {
        // 収集のエラーは `run_collect` 自身が記録する。収集中の拒否は記録しない。
        let _ = jobs::run_collect(state).await;
        return;
    }
    let scopes = jobs::all_category_scopes();
    let mut plan = job_plan(job, today).into_iter();
    // 最初の生成がジョブ本体。成功したら記録し、残り (日次後の作り直し) の失敗は無視する。
    let Some((period, start, force)) = plan.next() else {
        return;
    };
    let outcomes = match jobs::run_digests(state, period, start, &scopes, force).await {
        Ok(outcomes) => outcomes,
        Err(e) => {
            state.update_status(|s| s.set_error(ErrorOrigin::Job, e.message));
            return;
        }
    };
    // 週次と月次は作り直し (force) なので、保留の再試行では補えない。1件でも生成に
    // 失敗したら記録せず、次の評価で再実行する。日次 (朝刊) も強制の作り直しなので同じで、
    // 失敗しても夕刊の本文は残り、15分後に再実行する。
    let generation_failed = outcomes
        .iter()
        .any(|(_, o)| o.as_ref().is_some_and(|o| o.error.is_some()));
    if !should_record(job, generation_failed) {
        return;
    }
    if let Some((key, value)) = last_run_record(job) {
        record(state, key, &value);
    }
    for (period, start, force) in plan {
        let _ = jobs::run_digests(state, period, start, &scopes, force).await;
    }
}

/// 読むだけのモードで同期を頼む間隔 (分)。サーバーは最短 5 分ごとに書く。
pub const READER_SYNC_MINUTES: i64 = 10;

/// 読むだけのモードで同期を頼むか。最後の成功と最後の依頼の遅いほうから 10 分たったら頼む
/// (NAS が落ちていても、15 秒ごとには頼まない)。どちらもなければ (起動直後) すぐ頼む。
pub fn reader_sync_due(
    last_sync: Option<DateTime<Utc>>,
    last_request: Option<DateTime<Utc>>,
    now: DateTime<Utc>,
) -> bool {
    last_sync
        .max(last_request)
        .is_none_or(|t| now - t >= chrono::Duration::minutes(READER_SYNC_MINUTES))
}

/// 起動直後と15秒ごとに `due_jobs` を評価する。
///
/// 同じジョブは `filter_recent` の待ち時間内に再実行しない。収集中に収集の時刻が来ても
/// 黙って見送る。保留の再試行は、ジョブの有無にかかわらず、実行したジョブの後に
/// `RECENT_ATTEMPT` に1回まで行う。
/// 読むだけのモードでは、ジョブを評価せず、`reader_sync_due` のときに同期だけ頼む。
pub async fn run_loop(app: AppHandle) {
    let mut ticker = tokio::time::interval(TICK);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut attempts: HashMap<Job, DateTime<Utc>> = HashMap::new();
    let mut last_retry_at: Option<DateTime<Utc>> = None;
    let mut last_reader_request: Option<DateTime<Utc>> = None;
    loop {
        ticker.tick().await;
        let state = app.state::<AppState>();
        if state.with_store(is_reader).unwrap_or(false) {
            // 読むだけのモード: 収集、要約、再試行はしない。10 分ごとに同期だけ頼む。
            state.update_status(|s| s.next_collect_at = None);
            let now_utc = Utc::now();
            let last_sync = state
                .with_store(|s| s.get_setting(SETTING_LAST_SYNC_AT))
                .ok()
                .flatten()
                .and_then(|v| DateTime::parse_from_rfc3339(&v).ok())
                .map(|d| d.with_timezone(&Utc));
            if reader_sync_due(last_sync, last_reader_request, now_utc) {
                last_reader_request = Some(now_utc);
                state.request_sync();
            }
            continue;
        }
        let cfg = ScheduleConfig::from_settings(|k| {
            state.with_store(|s| s.get_setting(k)).ok().flatten()
        });
        let last = read_last_runs(&state);
        let now = chrono::Local::now().with_timezone(&state.tz);
        state.update_status(|s| s.next_collect_at = Some(next_collect_at(now, &cfg, &last)));
        let now_utc = now.with_timezone(&Utc);
        attempts.retain(|_, at| now_utc - *at < RECENT_ATTEMPT);
        let due = filter_recent(due_jobs(now, &cfg, &last), &attempts, now_utc, &cfg);
        let today = now.date_naive();
        for job in &due {
            // 収集中の収集は黙って見送る。試行として記録せず、終わり次第次の評価で拾う。
            if *job == Job::Collect && jobs::is_collecting(&state) {
                continue;
            }
            attempts.insert(job.clone(), now_utc);
            run_job(&state, job, today).await;
        }
        if retry_due(last_retry_at, now_utc) {
            last_retry_at = Some(now_utc);
            let _ = jobs::retry_pending(&state).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    #[test]
    fn reader_sync_waits_ten_minutes_after_a_sync_or_a_request() {
        // Review Focus 5
        let t = |s: &str| s.parse::<DateTime<Utc>>().unwrap();
        let now = t("2026-11-01T10:00:00Z");
        assert!(reader_sync_due(None, None, now), "first tick after start");
        assert!(!reader_sync_due(Some(t("2026-11-01T09:51:00Z")), None, now));
        assert!(reader_sync_due(Some(t("2026-11-01T09:50:00Z")), None, now));
        // NAS down: the last success is old, but a request went out a minute ago.
        assert!(!reader_sync_due(
            Some(t("2026-11-01T08:00:00Z")),
            Some(t("2026-11-01T09:59:00Z")),
            now
        ));
        assert!(reader_sync_due(
            Some(t("2026-11-01T08:00:00Z")),
            Some(t("2026-11-01T09:50:00Z")),
            now
        ));
    }

    #[tokio::test]
    async fn the_evening_edition_is_rebuilt_by_the_next_daily_run() {
        use infodesk_core::model::{Category, DigestScope, NewItem, Period};
        let (mut state, _) = crate::state::test_state();
        let fetched: DateTime<Utc> = "2026-10-05T03:00:00Z".parse().unwrap();
        let day = fetched.with_timezone(&state.tz).date_naive();
        state
            .with_store(|s| {
                let src = s
                    .list_sources()?
                    .into_iter()
                    .find(|x| x.category == Category::Medcare)
                    .unwrap();
                let item = NewItem {
                    url: "https://e.com/1".into(),
                    title: "認知症の新薬".into(),
                    excerpt: String::new(),
                    author: None,
                    published_at: None,
                    score: None,
                    image_url: None,
                };
                s.upsert_items(&src, &[item], &fetched).map(|_| ())
            })
            .unwrap();
        let body = |state: &crate::state::AppState| {
            state
                .with_store(|s| {
                    s.get_digest(Period::Day, day, &DigestScope::Category(Category::Medcare))
                })
                .unwrap()
                .map(|d| d.body)
        };
        state.llm = std::sync::Arc::new(|_, _| {
            Ok(Box::new(FixedLlm(Ok("夕刊"))) as Box<dyn infodesk_core::llm::LlmProvider>)
        });
        run_job(&state, &Job::Evening(day), day).await;
        assert_eq!(body(&state).as_deref(), Some("夕刊"));
        assert_eq!(
            state
                .with_store(|s| s.get_setting("schedule.last_evening_for"))
                .unwrap(),
            Some(day.to_string())
        );

        state.llm = std::sync::Arc::new(|_, _| {
            Ok(Box::new(FixedLlm(Ok("朝刊"))) as Box<dyn infodesk_core::llm::LlmProvider>)
        });
        run_job(&state, &Job::Daily(day), day + chrono::Duration::days(1)).await;
        assert_eq!(body(&state).as_deref(), Some("朝刊"));
    }

    #[tokio::test]
    async fn a_job_in_reader_mode_makes_no_digest_and_records_no_run() {
        use infodesk_core::model::Period;
        let state = state_with_daily_in_previous_week(Ok("週次"));
        state.with_store(|s| s.switch_to_reader()).unwrap();
        run_job(&state, &Job::Weekly(d("2026-09-28")), d("2026-10-06")).await;
        assert_eq!(
            state
                .with_store(|s| s.get_setting("schedule.last_weekly_for"))
                .unwrap(),
            None
        );
        assert!(
            state
                .with_store(|s| s.list_digests(Period::Week, d("2026-09-28")))
                .unwrap()
                .is_empty()
        );
    }

    struct FixedLlm(Result<&'static str, &'static str>);

    #[async_trait::async_trait]
    impl infodesk_core::llm::LlmProvider for FixedLlm {
        fn name(&self) -> String {
            "fixed".into()
        }
        async fn complete(&self, _s: &str, _p: &str) -> infodesk_core::Result<String> {
            self.0
                .map(String::from)
                .map_err(|e| infodesk_core::Error::Llm(e.into()))
        }
    }

    /// 前週 (9/28〜) に日次要約が1件ある状態を作り、LLMの応答を固定する。
    fn state_with_daily_in_previous_week(
        reply: Result<&'static str, &'static str>,
    ) -> crate::state::AppState {
        use infodesk_core::model::{Category, DigestScope, Period};
        use infodesk_core::store::NewDigest;
        let (mut state, _) = crate::state::test_state();
        state
            .with_store(|s| {
                s.save_digest_done(
                    &NewDigest {
                        period: Period::Day,
                        period_start: d("2026-09-30"),
                        scope: DigestScope::Category(Category::Medcare),
                        body: "日次".into(),
                        item_ids: vec![],
                        provider: "mock".into(),
                    },
                    &"2026-09-30T00:00:00Z".parse().unwrap(),
                )
            })
            .unwrap();
        state.llm = std::sync::Arc::new(move |_, _| {
            Ok(Box::new(FixedLlm(reply)) as Box<dyn infodesk_core::llm::LlmProvider>)
        });
        state
    }

    #[tokio::test]
    async fn failed_weekly_run_is_not_recorded() {
        let state = state_with_daily_in_previous_week(Err("down"));
        run_job(&state, &Job::Weekly(d("2026-09-28")), d("2026-10-06")).await;
        assert_eq!(
            state
                .with_store(|s| s.get_setting("schedule.last_weekly_for"))
                .unwrap(),
            None
        );
        assert!(state.status().last_error.is_some());

        let state = state_with_daily_in_previous_week(Ok("週次"));
        run_job(&state, &Job::Weekly(d("2026-09-28")), d("2026-10-06")).await;
        assert_eq!(
            state
                .with_store(|s| s.get_setting("schedule.last_weekly_for"))
                .unwrap()
                .as_deref(),
            Some("2026-09-28")
        );
    }

    #[tokio::test]
    async fn rejected_collect_does_not_set_last_error() {
        let (state, _) = crate::state::test_state();
        state.update_status(|s| s.collecting = true);
        run_job(&state, &Job::Collect, d("2026-10-06")).await;
        assert_eq!(state.status().last_error, None);
    }
}
