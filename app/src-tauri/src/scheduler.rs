use std::collections::HashMap;

use chrono::{DateTime, NaiveDate, Utc};
use tauri::{AppHandle, Manager};

use crate::jobs;
use crate::state::{AppState, ErrorOrigin};

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
    // 失敗したら記録せず、次の評価で再実行する。日次の失敗は保留の再試行で補う。
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

/// 起動直後と15秒ごとに `due_jobs` を評価する。
///
/// 同じジョブは `filter_recent` の待ち時間内に再実行しない。収集中に収集の時刻が来ても
/// 黙って見送る。保留の再試行は、ジョブの有無にかかわらず、実行したジョブの後に
/// `RECENT_ATTEMPT` に1回まで行う。
pub async fn run_loop(app: AppHandle) {
    let mut ticker = tokio::time::interval(TICK);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut attempts: HashMap<Job, DateTime<Utc>> = HashMap::new();
    let mut last_retry_at: Option<DateTime<Utc>> = None;
    loop {
        ticker.tick().await;
        let state = app.state::<AppState>();
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
