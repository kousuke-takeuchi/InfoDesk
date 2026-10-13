use std::collections::HashMap;
use std::time::Duration as StdDuration;

use chrono::{DateTime, Duration, FixedOffset, NaiveDate, NaiveTime, Utc, Weekday};
use infodesk_core::model::Period;
use infodesk_core::period::period_start;
use tauri::{AppHandle, Manager};

use crate::jobs;
use crate::state::AppState;

#[derive(Debug, Clone, PartialEq)]
pub struct ScheduleConfig {
    pub collect_interval: Duration,
    pub daily_time: NaiveTime,
    pub weekly_day: Weekday,
    pub weekly_time: NaiveTime,
    pub monthly_time: NaiveTime,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct LastRuns {
    pub collect_at: Option<DateTime<Utc>>,
    pub daily_for: Option<NaiveDate>,
    pub weekly_for: Option<NaiveDate>,
    pub monthly_for: Option<NaiveDate>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Job {
    Collect,
    /// 対象日 (前日)。
    Daily(NaiveDate),
    /// 対象週の月曜 (前週)。
    Weekly(NaiveDate),
    /// 対象月の1日 (前月)。
    Monthly(NaiveDate),
}

fn parse_time(s: Option<String>, default: (u32, u32)) -> NaiveTime {
    s.and_then(|v| NaiveTime::parse_from_str(v.trim(), "%H:%M").ok())
        .unwrap_or_else(|| NaiveTime::from_hms_opt(default.0, default.1, 0).expect("valid"))
}

fn parse_weekday(s: Option<String>) -> Weekday {
    s.and_then(|v| v.trim().to_lowercase().parse::<Weekday>().ok())
        .unwrap_or(Weekday::Mon)
}

impl ScheduleConfig {
    /// 設定から読む。壊れた値や未設定は既定値 (3時間、7:00、月曜 7:30、1日 8:00)。
    ///
    /// 収集間隔は `schedule.collect_interval_minutes` (1〜10080分) を優先し、なければ
    /// 旧設定 `schedule.collect_interval_hours` (1〜168時間) を分に直して使う。
    pub fn from_settings(get: impl Fn(&str) -> Option<String>) -> Self {
        let parse = |key: &str| get(key).and_then(|v| v.trim().parse::<i64>().ok());
        let minutes = parse("schedule.collect_interval_minutes")
            .or_else(|| parse("schedule.collect_interval_hours").map(|h| h.clamp(1, 24 * 7) * 60))
            .unwrap_or(180)
            .clamp(1, 24 * 7 * 60);
        Self {
            collect_interval: Duration::minutes(minutes),
            daily_time: parse_time(get("schedule.daily_time"), (7, 0)),
            weekly_day: parse_weekday(get("schedule.weekly_day")),
            weekly_time: parse_time(get("schedule.weekly_time"), (7, 30)),
            monthly_time: parse_time(get("schedule.monthly_time"), (8, 0)),
        }
    }
}

/// 次に収集する時刻。最後の収集がなければ今。
pub fn next_collect_at(
    now: DateTime<FixedOffset>,
    cfg: &ScheduleConfig,
    last: &LastRuns,
) -> DateTime<Utc> {
    let now = now.with_timezone(&Utc);
    match last.collect_at {
        Some(at) => (at + cfg.collect_interval).max(now),
        None => now,
    }
}

/// 今の時刻で実行すべきジョブを、依存順 (収集、日次、週次、月次) で返す。
///
/// 日次は「今日の daily_time 以降なら前日分」、まだなら「一昨日分まで」を対象にする。
/// 対象の開始日が最後に生成した日付より後なら実行する。実行は1回で、遅れても
/// 分単位に繰り返さない。週次と月次は、曜日や日にかかわらず、未実行の前期間を補う。
/// ただし当日が実行日の場合は、その時刻までは出さない。
pub fn due_jobs(now: DateTime<FixedOffset>, cfg: &ScheduleConfig, last: &LastRuns) -> Vec<Job> {
    let mut jobs = Vec::new();
    if next_collect_at(now, cfg, last) <= now.with_timezone(&Utc) {
        jobs.push(Job::Collect);
    }
    let today = now.date_naive();
    let time = now.time();

    // 日次: 対象は「最後に daily_time を迎えた日」の前日。
    let daily_anchor = if time >= cfg.daily_time {
        today
    } else {
        today - Duration::days(1)
    };
    let daily_target = daily_anchor - Duration::days(1);
    if last.daily_for.is_none_or(|d| d < daily_target) {
        jobs.push(Job::Daily(daily_target));
    }

    // 週次: 対象は現在の週の前の週。今週の実行日時をまだ迎えていなければ、前週の実行分
    // (つまり2週前) まで。
    let this_week = period_start(Period::Week, today);
    let weekly_run_day = this_week + Duration::days(cfg.weekly_day.num_days_from_monday() as i64);
    let weekly_reached =
        today > weekly_run_day || (today == weekly_run_day && time >= cfg.weekly_time);
    let weekly_target = if weekly_reached {
        this_week
    } else {
        this_week - Duration::days(7)
    } - Duration::days(7);
    if last.weekly_for.is_none_or(|d| d < weekly_target) {
        jobs.push(Job::Weekly(weekly_target));
    }

    // 月次: 対象は前月。今月1日の monthly_time をまだ迎えていなければ、前々月まで。
    let this_month = period_start(Period::Month, today);
    let monthly_reached = today > this_month || time >= cfg.monthly_time;
    let anchor = if monthly_reached {
        this_month
    } else {
        previous_month(this_month)
    };
    let monthly_target = previous_month(anchor);
    if last.monthly_for.is_none_or(|d| d < monthly_target) {
        jobs.push(Job::Monthly(monthly_target));
    }
    jobs
}

/// 同じジョブや再試行を繰り返さない間隔。
pub const RECENT_ATTEMPT: Duration = Duration::minutes(15);

/// 最後の試行からの待ち時間未満のジョブを除く。失敗し続けるジョブを繰り返さないため。
/// 待ち時間は `RECENT_ATTEMPT`。収集だけは収集間隔がそれより短ければ収集間隔にする。
pub fn filter_recent(
    due: Vec<Job>,
    attempts: &HashMap<Job, DateTime<Utc>>,
    now: DateTime<Utc>,
    cfg: &ScheduleConfig,
) -> Vec<Job> {
    due.into_iter()
        .filter(|job| {
            let window = match job {
                Job::Collect => RECENT_ATTEMPT.min(cfg.collect_interval),
                _ => RECENT_ATTEMPT,
            };
            attempts.get(job).is_none_or(|at| now - *at >= window)
        })
        .collect()
}

/// 保留の要約の再試行を今すべきか。前回から `RECENT_ATTEMPT` 以上空いていれば真。
pub fn retry_due(last_retry_at: Option<DateTime<Utc>>, now: DateTime<Utc>) -> bool {
    last_retry_at.is_none_or(|at| now - at >= RECENT_ATTEMPT)
}

/// `run_loop` の評価間隔。
const TICK: StdDuration = StdDuration::from_secs(15);

fn previous_month(first_of_month: NaiveDate) -> NaiveDate {
    period_start(Period::Month, first_of_month - Duration::days(1))
}

fn read_last_runs(state: &AppState) -> LastRuns {
    let get = |k: &str| state.with_store(|s| s.get_setting(k)).ok().flatten();
    let date = |k: &str| get(k).and_then(|v| NaiveDate::parse_from_str(&v, "%Y-%m-%d").ok());
    LastRuns {
        collect_at: get("schedule.last_collect_at")
            .and_then(|v| DateTime::parse_from_rfc3339(&v).ok())
            .map(|d| d.with_timezone(&Utc)),
        daily_for: date("schedule.last_daily_for"),
        weekly_for: date("schedule.last_weekly_for"),
        monthly_for: date("schedule.last_monthly_for"),
    }
}

fn record(state: &AppState, key: &str, value: &str) {
    let _ = state.with_store(|s| s.set_setting(key, value));
}

/// ジョブが実行する要約生成を順に返す (期間、対象日、作り直すか)。
///
/// 日次の後は今日を含む週次と月次を作り直す。週次と月次のジョブは期間ごとに1回だけ
/// 出るので、日次の作り直しで生成済みになった前期間も、最終日を含めて作り直す。
pub fn job_plan(job: &Job, today: NaiveDate) -> Vec<(Period, NaiveDate, bool)> {
    match job {
        Job::Collect => Vec::new(),
        Job::Daily(day) => vec![
            (Period::Day, *day, false),
            (Period::Week, today, true),
            (Period::Month, today, true),
        ],
        Job::Weekly(start) => vec![(Period::Week, *start, true)],
        Job::Monthly(start) => vec![(Period::Month, *start, true)],
    }
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
            state.update_status(|s| s.last_error = Some(e.message));
            return;
        }
    };
    // 週次と月次は作り直し (force) なので、保留の再試行では補えない。1件でも生成に
    // 失敗したら記録せず、次の評価で再実行する。日次の失敗は保留の再試行で補う。
    let generation_failed = outcomes
        .iter()
        .any(|(_, o)| o.as_ref().is_some_and(|o| o.error.is_some()));
    if matches!(job, Job::Weekly(_) | Job::Monthly(_)) && generation_failed {
        return;
    }
    let (key, value) = match job {
        Job::Daily(day) => ("schedule.last_daily_for", day),
        Job::Weekly(start) => ("schedule.last_weekly_for", start),
        Job::Monthly(start) => ("schedule.last_monthly_for", start),
        Job::Collect => unreachable!("handled above"),
    };
    record(state, key, &value.to_string());
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

    fn jst(s: &str) -> DateTime<FixedOffset> {
        DateTime::parse_from_rfc3339(s).unwrap()
    }

    fn d(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    fn cfg() -> ScheduleConfig {
        ScheduleConfig::from_settings(|_| None)
    }

    #[test]
    fn config_defaults_and_tolerates_garbage() {
        // Review Focus 4
        let c = cfg();
        assert_eq!(c.collect_interval, Duration::hours(3));
        assert_eq!(c.daily_time, NaiveTime::from_hms_opt(7, 0, 0).unwrap());
        assert_eq!(c.weekly_day, Weekday::Mon);
        assert_eq!(c.weekly_time, NaiveTime::from_hms_opt(7, 30, 0).unwrap());
        assert_eq!(c.monthly_time, NaiveTime::from_hms_opt(8, 0, 0).unwrap());

        let c = ScheduleConfig::from_settings(|k| {
            Some(
                match k {
                    "schedule.collect_interval_hours" => "abc",
                    "schedule.daily_time" => "25:99",
                    "schedule.weekly_day" => "funday",
                    "schedule.weekly_time" => "",
                    "schedule.monthly_time" => "09:15",
                    _ => return None,
                }
                .to_string(),
            )
        });
        assert_eq!(c.collect_interval, Duration::hours(3));
        assert_eq!(c.daily_time, NaiveTime::from_hms_opt(7, 0, 0).unwrap());
        assert_eq!(c.weekly_day, Weekday::Mon);
        assert_eq!(c.monthly_time, NaiveTime::from_hms_opt(9, 15, 0).unwrap());

        let c = ScheduleConfig::from_settings(|k| match k {
            "schedule.collect_interval_hours" => Some("0".into()),
            "schedule.weekly_day" => Some("sun".into()),
            _ => None,
        });
        assert_eq!(
            c.collect_interval,
            Duration::hours(1),
            "0時間は1時間に切り上げる"
        );
        assert_eq!(c.weekly_day, Weekday::Sun);
    }

    #[test]
    fn first_start_collects_and_backfills_previous_periods() {
        // 2026-10-06 (火) 06:00 JST。日次の7:00より前なので、日次の対象は一昨日 (10-04)。
        // 週次と月次は前期間を補う。記事がなければ生成はされない (plan_digest が None)。
        let now = jst("2026-10-06T06:00:00+09:00");
        let jobs = due_jobs(now, &cfg(), &LastRuns::default());
        assert_eq!(
            jobs,
            vec![
                Job::Collect,
                Job::Daily(d("2026-10-04")),
                Job::Weekly(d("2026-09-28")),
                Job::Monthly(d("2026-09-01"))
            ]
        );
    }

    #[test]
    fn collect_is_due_after_interval_only() {
        let now = jst("2026-10-06T06:00:00+09:00");
        let last = LastRuns {
            collect_at: Some(now.with_timezone(&Utc) - Duration::hours(2)),
            ..Default::default()
        };
        assert!(!due_jobs(now, &cfg(), &last).contains(&Job::Collect));
        let last = LastRuns {
            collect_at: Some(now.with_timezone(&Utc) - Duration::hours(3)),
            ..Default::default()
        };
        assert!(due_jobs(now, &cfg(), &last).contains(&Job::Collect));
        assert_eq!(next_collect_at(now, &cfg(), &last), now.with_timezone(&Utc));
    }

    #[test]
    fn daily_runs_once_after_its_time_even_if_late() {
        // Review Focus 3: 7:00 を過ぎてスリープから復帰 (13:42)。前日分を1回だけ。
        let now = jst("2026-10-06T13:42:00+09:00");
        let last = LastRuns {
            collect_at: Some(now.with_timezone(&Utc)),
            weekly_for: Some(d("2026-09-28")),
            monthly_for: Some(d("2026-09-01")),
            ..Default::default()
        };
        let jobs = due_jobs(now, &cfg(), &last);
        assert_eq!(jobs, vec![Job::Daily(d("2026-10-05"))]);

        let done = LastRuns {
            daily_for: Some(d("2026-10-05")),
            ..last.clone()
        };
        assert!(due_jobs(now, &cfg(), &done).is_empty());
        // 翌日の 7:00 前はまだ出ない。
        // 収集は別ジョブなので、各時刻で収集済みにして日次だけを見る。
        let tomorrow_early = jst("2026-10-07T06:59:00+09:00");
        let done_early = LastRuns {
            collect_at: Some(tomorrow_early.with_timezone(&Utc)),
            ..done.clone()
        };
        assert!(due_jobs(tomorrow_early, &cfg(), &done_early).is_empty());
        let tomorrow = jst("2026-10-07T07:00:00+09:00");
        let done_tomorrow = LastRuns {
            collect_at: Some(tomorrow.with_timezone(&Utc)),
            ..done.clone()
        };
        assert_eq!(
            due_jobs(tomorrow, &cfg(), &done_tomorrow),
            vec![Job::Daily(d("2026-10-06"))]
        );
    }

    #[test]
    fn weekly_and_monthly_target_previous_periods() {
        // 2026-10-05 (月) 09:00: 前週 (9/28〜) の週次。10/1 以降なので前月 (9月) の月次も。
        let now = jst("2026-10-05T09:00:00+09:00");
        let last = LastRuns {
            collect_at: Some(now.with_timezone(&Utc)),
            daily_for: Some(d("2026-10-04")),
            ..Default::default()
        };
        let jobs = due_jobs(now, &cfg(), &last);
        assert_eq!(
            jobs,
            vec![Job::Weekly(d("2026-09-28")), Job::Monthly(d("2026-09-01"))]
        );

        // 火曜日は週次の対象外だが、未実行なら補う (前週分)。
        let tue = jst("2026-10-06T09:00:00+09:00");
        let last = LastRuns {
            daily_for: Some(d("2026-10-05")),
            monthly_for: Some(d("2026-09-01")),
            collect_at: Some(tue.with_timezone(&Utc)),
            ..Default::default()
        };
        assert_eq!(
            due_jobs(tue, &cfg(), &last),
            vec![Job::Weekly(d("2026-09-28"))]
        );

        // 月曜 7:00 (週次の 7:30 より前) は週次を出さない。
        let early = jst("2026-10-05T07:00:00+09:00");
        let last = LastRuns {
            daily_for: Some(d("2026-10-04")),
            weekly_for: Some(d("2026-09-21")),
            monthly_for: Some(d("2026-09-01")),
            collect_at: Some(early.with_timezone(&Utc)),
        };
        assert!(due_jobs(early, &cfg(), &last).is_empty());
    }

    #[test]
    fn job_plan_lists_digest_runs_in_order() {
        let today = d("2026-10-06");
        assert!(job_plan(&Job::Collect, today).is_empty());
        assert_eq!(
            job_plan(&Job::Daily(d("2026-10-05")), today),
            vec![
                (Period::Day, d("2026-10-05"), false),
                (Period::Week, today, true),
                (Period::Month, today, true),
            ]
        );
        // 週次と月次は期間ごとに1回だけ出るので、最終日を含めて作り直す (force)。
        assert_eq!(
            job_plan(&Job::Weekly(d("2026-09-28")), today),
            vec![(Period::Week, d("2026-09-28"), true)]
        );
        assert_eq!(
            job_plan(&Job::Monthly(d("2026-09-01")), today),
            vec![(Period::Month, d("2026-09-01"), true)]
        );
    }

    #[test]
    fn filter_recent_skips_jobs_attempted_within_15_minutes() {
        let now: DateTime<Utc> = "2026-10-06T00:00:00Z".parse().unwrap();
        let mut attempts = HashMap::new();
        attempts.insert(Job::Collect, now - Duration::minutes(14));
        attempts.insert(Job::Daily(d("2026-10-05")), now - Duration::minutes(15));
        attempts.insert(Job::Weekly(d("2026-09-21")), now);
        let due = vec![
            Job::Collect,
            Job::Daily(d("2026-10-05")),
            Job::Weekly(d("2026-09-28")),
            Job::Monthly(d("2026-09-01")),
        ];
        assert_eq!(
            filter_recent(due, &attempts, now, &cfg()),
            vec![
                Job::Daily(d("2026-10-05")),
                Job::Weekly(d("2026-09-28")),
                Job::Monthly(d("2026-09-01")),
            ]
        );
        assert!(filter_recent(Vec::new(), &attempts, now, &cfg()).is_empty());
    }

    #[test]
    fn collect_recent_window_is_capped_by_the_interval() {
        let now: DateTime<Utc> = "2026-10-06T00:00:00Z".parse().unwrap();
        let one_min = ScheduleConfig::from_settings(|k| match k {
            "schedule.collect_interval_minutes" => Some("1".into()),
            _ => None,
        });
        let mut attempts = HashMap::new();
        attempts.insert(Job::Collect, now - Duration::seconds(61));
        attempts.insert(Job::Daily(d("2026-10-05")), now - Duration::seconds(61));
        let due = vec![Job::Collect, Job::Daily(d("2026-10-05"))];
        // 1分間隔なら収集は1分後に再実行でき、他のジョブは15分のまま。
        assert_eq!(
            filter_recent(due.clone(), &attempts, now, &one_min),
            vec![Job::Collect]
        );
        attempts.insert(Job::Collect, now - Duration::seconds(30));
        assert!(filter_recent(due, &attempts, now, &one_min).is_empty());
        // 3時間間隔なら従来どおり15分。
        let mut attempts = HashMap::new();
        attempts.insert(Job::Collect, now - Duration::minutes(14));
        assert!(filter_recent(vec![Job::Collect], &attempts, now, &cfg()).is_empty());
    }

    #[test]
    fn pending_retry_runs_at_most_once_per_window() {
        let now: DateTime<Utc> = "2026-10-06T00:00:00Z".parse().unwrap();
        assert!(retry_due(None, now));
        assert!(!retry_due(Some(now - Duration::minutes(14)), now));
        assert!(retry_due(Some(now - Duration::minutes(15)), now));
    }

    #[test]
    fn minutes_setting_wins_over_hours_and_falls_back() {
        let with = |minutes: Option<&'static str>, hours: Option<&'static str>| {
            ScheduleConfig::from_settings(|k| match k {
                "schedule.collect_interval_minutes" => minutes.map(String::from),
                "schedule.collect_interval_hours" => hours.map(String::from),
                _ => None,
            })
            .collect_interval
        };
        assert_eq!(with(Some("5"), Some("6")), Duration::minutes(5));
        assert_eq!(with(None, Some("6")), Duration::hours(6));
        assert_eq!(with(Some("abc"), Some("6")), Duration::hours(6));
        assert_eq!(with(Some(""), Some("2")), Duration::hours(2));
        assert_eq!(with(Some("0"), Some("6")), Duration::minutes(1));
        assert_eq!(with(Some("999999"), None), Duration::minutes(10080));
        assert_eq!(with(None, None), Duration::minutes(180));
        assert_eq!(with(Some("x"), Some("y")), Duration::minutes(180));
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
        use infodesk_core::model::{Category, DigestScope};
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

    #[test]
    fn monthly_waits_for_its_time_on_the_first_and_rolls_over_the_year() {
        // 10/1 の 8:00 より前は前月 (9月) を出さない。前々月 (8月) は済み。
        let early = jst("2026-10-01T07:59:00+09:00");
        let last = LastRuns {
            collect_at: Some(early.with_timezone(&Utc)),
            daily_for: Some(d("2026-09-30")),
            weekly_for: Some(d("2026-09-21")),
            monthly_for: Some(d("2026-08-01")),
        };
        assert!(due_jobs(early, &cfg(), &last).is_empty());
        let at_time = jst("2026-10-01T08:00:00+09:00");
        let last = LastRuns {
            collect_at: Some(at_time.with_timezone(&Utc)),
            ..last
        };
        assert_eq!(
            due_jobs(at_time, &cfg(), &last),
            vec![Job::Monthly(d("2026-09-01"))]
        );

        // 1月は前年12月が対象。
        let jan = jst("2027-01-05T09:00:00+09:00");
        let last = LastRuns {
            collect_at: Some(jan.with_timezone(&Utc)),
            daily_for: Some(d("2027-01-04")),
            weekly_for: Some(d("2026-12-28")),
            monthly_for: Some(d("2026-11-01")),
        };
        assert_eq!(
            due_jobs(jan, &cfg(), &last),
            vec![Job::Monthly(d("2026-12-01"))]
        );
    }

    #[test]
    fn weekly_on_sunday_waits_for_weekly_time() {
        let sun_cfg = ScheduleConfig::from_settings(|k| match k {
            "schedule.weekly_day" => Some("sun".into()),
            _ => None,
        });
        // 2026-10-11 は日曜。今週 (10/5〜) の実行日。7:30 より前は前週分 (9/28〜) を出さない。
        let before = jst("2026-10-11T07:29:00+09:00");
        let last = LastRuns {
            collect_at: Some(before.with_timezone(&Utc)),
            daily_for: Some(d("2026-10-10")),
            weekly_for: Some(d("2026-09-21")),
            monthly_for: Some(d("2026-09-01")),
        };
        assert!(due_jobs(before, &sun_cfg, &last).is_empty());
        let after = jst("2026-10-11T07:30:00+09:00");
        let last = LastRuns {
            collect_at: Some(after.with_timezone(&Utc)),
            ..last
        };
        assert_eq!(
            due_jobs(after, &sun_cfg, &last),
            vec![Job::Weekly(d("2026-09-28"))]
        );
    }

    #[tokio::test]
    async fn rejected_collect_does_not_set_last_error() {
        let (state, _) = crate::state::test_state();
        state.update_status(|s| s.collecting = true);
        run_job(&state, &Job::Collect, d("2026-10-06")).await;
        assert_eq!(state.status().last_error, None);
    }

    #[test]
    fn jobs_come_in_dependency_order() {
        let now = jst("2026-10-05T09:00:00+09:00");
        let jobs = due_jobs(now, &cfg(), &LastRuns::default());
        assert_eq!(
            jobs,
            vec![
                Job::Collect,
                Job::Daily(d("2026-10-04")),
                Job::Weekly(d("2026-09-28")),
                Job::Monthly(d("2026-09-01"))
            ]
        );
    }
}
