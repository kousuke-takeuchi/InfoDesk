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

#[derive(Debug, Clone, PartialEq, Eq)]
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
    pub fn from_settings(get: impl Fn(&str) -> Option<String>) -> Self {
        let hours = get("schedule.collect_interval_hours")
            .and_then(|v| v.trim().parse::<i64>().ok())
            .unwrap_or(3)
            .clamp(1, 24 * 7);
        Self {
            collect_interval: Duration::hours(hours),
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

/// 1つのジョブを実行する。日次の後は現在の週次と月次を作り直す。
async fn run_job(state: &AppState, job: &Job) {
    let scopes = jobs::all_category_scopes();
    let today = chrono::Local::now().date_naive();
    let outcome = match job {
        Job::Collect => jobs::run_collect(state).await.map(|_| ()),
        Job::Daily(day) => {
            let r = jobs::run_digests(state, Period::Day, *day, &scopes, false)
                .await
                .map(|_| ());
            if r.is_ok() {
                record(state, "schedule.last_daily_for", &day.to_string());
                let _ = jobs::run_digests(state, Period::Week, today, &scopes, true).await;
                let _ = jobs::run_digests(state, Period::Month, today, &scopes, true).await;
            }
            r
        }
        Job::Weekly(start) => {
            let r = jobs::run_digests(state, Period::Week, *start, &scopes, false)
                .await
                .map(|_| ());
            if r.is_ok() {
                record(state, "schedule.last_weekly_for", &start.to_string());
            }
            r
        }
        Job::Monthly(start) => {
            let r = jobs::run_digests(state, Period::Month, *start, &scopes, false)
                .await
                .map(|_| ());
            if r.is_ok() {
                record(state, "schedule.last_monthly_for", &start.to_string());
            }
            r
        }
    };
    if let Err(e) = outcome {
        state.update_status(|s| s.last_error = Some(e.message));
    }
}

/// 起動直後と60秒ごとに `due_jobs` を評価する。
pub async fn run_loop(app: AppHandle) {
    let mut ticker = tokio::time::interval(StdDuration::from_secs(60));
    loop {
        ticker.tick().await;
        let state = app.state::<AppState>();
        let cfg = ScheduleConfig::from_settings(|k| {
            state.with_store(|s| s.get_setting(k)).ok().flatten()
        });
        let last = read_last_runs(&state);
        let now = chrono::Local::now().with_timezone(&state.tz);
        state.update_status(|s| s.next_collect_at = Some(next_collect_at(now, &cfg, &last)));
        let due = due_jobs(now, &cfg, &last);
        for job in &due {
            run_job(&state, job).await;
        }
        if !due.is_empty() {
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
