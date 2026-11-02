//! `infodesk tick`: 予定を1回だけ評価して終わる。自宅の k3s の Argo CronWorkflow が5分ごとに呼ぶ。
//! 規則はアプリと同じ (`infodesk_core::schedule`)。プロセスをまたぐ状態は DB に置く。

use std::collections::{BTreeMap, HashMap};
use std::sync::{Mutex, MutexGuard};

use chrono::{DateTime, FixedOffset, Local, NaiveDate, Utc};
use infodesk_core::collector::{apply_outcomes, fetch_all, http_client, images};
use infodesk_core::llm::{LlmProvider, LlmRole, provider_from_settings};
use infodesk_core::model::{Category, DigestScope, Period};
use infodesk_core::period::{period_start, utc_range};
use infodesk_core::schedule::{
    Job, LastRuns, RECENT_ATTEMPT, ScheduleConfig, all_failed_retry_base, due_jobs, filter_recent,
    job_plan, last_run_record, retry_due, should_record,
};
use infodesk_core::summarizer::{
    DigestOutcome, finish_digest, finish_item_summary, plan_digest, plan_item_summaries,
};
use infodesk_core::sync::headless::{self, HeadlessEnv, MakeDrive, SyncOutcome};
use infodesk_core::{Result, Store};

pub type LlmFactory = Box<dyn Fn(&Store, LlmRole) -> Result<Box<dyn LlmProvider>> + Send + Sync>;

const SETTING_ATTEMPTS: &str = "schedule.attempts";
const SETTING_LAST_RETRY_AT: &str = "schedule.last_retry_at";

/// ログの1行を受け取る。本番は標準出力にすぐ出す (Pod が途中で止められても Argo に残る)。
pub type Sink = Box<dyn Fn(&str) + Send + Sync>;

const SUMMARY_PROGRESS_EVERY: usize = 20;

pub struct Runner {
    pub(crate) store: Mutex<Store>,
    http: reqwest::Client,
    tz: FixedOffset,
    llm: LlmFactory,
    make_drive: Box<MakeDrive>,
    pub(crate) env: HeadlessEnv,
    sink: Sink,
}

/// 1回の評価のログ。行は出来た順にすぐ `sink` へ渡し、`run` の戻り値用にも貯める。
struct Log<'a> {
    lines: Vec<String>,
    sink: &'a Sink,
}

impl Log<'_> {
    fn push(&mut self, line: String) {
        (self.sink)(&line);
        self.lines.push(line);
    }
}

/// ログに出すエラーの文。Invalid は Display の接頭辞 (`invalid value: `) なしで出す (`run_from` と同じ)。
fn reason(e: &infodesk_core::Error) -> String {
    match e {
        infodesk_core::Error::Invalid(reason) => reason.clone(),
        e => e.to_string(),
    }
}

fn all_scopes() -> Vec<DigestScope> {
    Category::ALL
        .into_iter()
        .map(DigestScope::Category)
        .collect()
}

/// `schedule.attempts` のキー。
pub(crate) fn job_key(job: &Job) -> String {
    match job {
        Job::Collect => "collect".to_string(),
        Job::Daily(d) => format!("daily:{d}"),
        Job::Evening(d) => format!("evening:{d}"),
        Job::Weekly(d) => format!("weekly:{d}"),
        Job::Monthly(d) => format!("monthly:{d}"),
    }
}

pub(crate) fn job_from_key(key: &str) -> Option<Job> {
    if key == "collect" {
        return Some(Job::Collect);
    }
    let (kind, date) = key.split_once(':')?;
    let date = NaiveDate::parse_from_str(date, "%Y-%m-%d").ok()?;
    match kind {
        "daily" => Some(Job::Daily(date)),
        "evening" => Some(Job::Evening(date)),
        "weekly" => Some(Job::Weekly(date)),
        "monthly" => Some(Job::Monthly(date)),
        _ => None,
    }
}

fn label(job: &Job) -> String {
    job_key(job).replace(':', " ")
}

fn parse_utc(v: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(v)
        .ok()
        .map(|d| d.with_timezone(&Utc))
}

impl Runner {
    pub fn new(
        store: Store,
        tz: FixedOffset,
        llm: LlmFactory,
        make_drive: Box<MakeDrive>,
        env: HeadlessEnv,
    ) -> Result<Self> {
        Ok(Self {
            store: Mutex::new(store),
            http: http_client()?,
            tz,
            llm,
            make_drive,
            env,
            sink: Box::new(|_| {}),
        })
    }

    /// ログの各行を出来た順に受け取る先を決める。既定は何もしない。
    pub fn with_sink(mut self, sink: Sink) -> Self {
        self.sink = sink;
        self
    }

    fn lock(&self) -> MutexGuard<'_, Store> {
        self.store.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn get(&self, key: &str) -> Option<String> {
        self.lock().get_setting(key).ok().flatten()
    }

    fn load_attempts(&self) -> Result<HashMap<Job, DateTime<Utc>>> {
        let raw = self
            .lock()
            .get_setting(SETTING_ATTEMPTS)?
            .unwrap_or_default();
        let map: BTreeMap<String, String> = serde_json::from_str(&raw).unwrap_or_default();
        Ok(map
            .iter()
            .filter_map(|(k, v)| Some((job_from_key(k)?, parse_utc(v)?)))
            .collect())
    }

    fn save_attempts(&self, attempts: &HashMap<Job, DateTime<Utc>>) {
        let map: BTreeMap<String, String> = attempts
            .iter()
            .map(|(j, at)| (job_key(j), at.to_rfc3339()))
            .collect();
        let json = serde_json::to_string(&map).unwrap_or_else(|_| "{}".to_string());
        let _ = self.lock().set_setting(SETTING_ATTEMPTS, &json);
    }

    /// 1回の評価。実行したことをログの行として順に返す (各行は出来たときに `sink` にも渡す)。
    /// DB を読めないときだけ `Err`。
    pub async fn run(&self, now: DateTime<FixedOffset>) -> Result<Vec<String>> {
        let mut log = Log {
            lines: Vec::new(),
            sink: &self.sink,
        };
        let now_utc = now.with_timezone(&Utc);
        let mut attempts = self.load_attempts()?;
        attempts.retain(|_, at| now_utc - *at < RECENT_ATTEMPT);
        let cfg = ScheduleConfig::from_settings(|k| self.get(k));
        let last = LastRuns::from_settings(|k| self.get(k));
        let due = filter_recent(due_jobs(now, &cfg, &last), &attempts, now_utc, &cfg);
        let today = now.date_naive();
        for job in &due {
            attempts.insert(job.clone(), now_utc);
            // 途中で時間切れになっても試行を覚えているよう、実行の前に書く。
            self.save_attempts(&attempts);
            let changed = match job {
                Job::Collect => self.collect(now_utc, &cfg, &mut log).await,
                _ => self.digest_job(job, today, &mut log).await,
            };
            if changed {
                self.sync(now_utc, &mut log).await;
            }
        }
        self.save_attempts(&attempts);
        let last_retry_at = self.get(SETTING_LAST_RETRY_AT).and_then(|v| parse_utc(&v));
        if retry_due(last_retry_at, now_utc) {
            let _ = self
                .lock()
                .set_setting(SETTING_LAST_RETRY_AT, &now_utc.to_rfc3339());
            if self.retry_pending(&mut log).await {
                self.sync(now_utc, &mut log).await;
            }
        }
        Ok(log.lines)
    }

    /// 収集と記事要約。記録まで進んだら真 (同期する)。
    async fn collect(&self, now: DateTime<Utc>, cfg: &ScheduleConfig, log: &mut Log<'_>) -> bool {
        let sources = match self.lock().list_sources() {
            Ok(s) => s,
            Err(e) => {
                log.push(format!("collect: error {}", reason(&e)));
                return false;
            }
        };
        let mut outcomes = fetch_all(&self.http, &sources).await;
        let urls: Vec<String> = outcomes
            .iter()
            .filter_map(|o| o.result.as_ref().ok())
            .flatten()
            .map(|i| i.url.clone())
            .collect();
        let known = self.lock().existing_urls(&urls).unwrap_or_default();
        images::fill_page_images(&mut outcomes, &known).await;
        let reports = match apply_outcomes(&self.lock(), &outcomes, &now) {
            Ok(r) => r,
            Err(e) => {
                log.push(format!("collect: error {}", reason(&e)));
                return false;
            }
        };
        let new_items: usize = reports.iter().map(|r| r.new_items).sum();
        let failed: Vec<&str> = reports
            .iter()
            .filter(|r| r.error.is_some())
            .map(|r| r.name.as_str())
            .collect();
        if !reports.is_empty() && failed.len() == reports.len() {
            let base = all_failed_retry_base(now, cfg.collect_interval);
            let _ = self
                .lock()
                .set_setting("schedule.last_collect_at", &base.to_rfc3339());
            log.push("collect: all sources failed, retrying in about 10 minutes".to_string());
            return false;
        }
        if let Err(e) = self
            .lock()
            .set_setting("schedule.last_collect_at", &now.to_rfc3339())
        {
            log.push(format!("collect: error {}", reason(&e)));
            return false;
        }
        log.push(format!(
            "collect: {new_items} new, failed sources: {failed:?}"
        ));
        self.item_summaries(now, log).await;
        true
    }

    /// 今日取得した記事の上位に要約を付ける。最初の失敗で止める (アプリと同じ)。
    async fn item_summaries(&self, now: DateTime<Utc>, log: &mut Log<'_>) {
        let today = now.with_timezone(&self.tz).date_naive();
        let (since, _) = utc_range(Period::Day, period_start(Period::Day, today), self.tz);
        let limit: u32 = self
            .get("summary.top_n")
            .and_then(|v| v.parse().ok())
            .unwrap_or(20);
        let llm = match (self.llm)(&self.lock(), LlmRole::Items) {
            Ok(l) => l,
            Err(e) => {
                log.push(format!("summaries: error {}", reason(&e)));
                return;
            }
        };
        let (mut ok, mut error) = (0, None);
        'categories: for category in Category::ALL {
            let jobs = match plan_item_summaries(&self.lock(), category, &since, limit) {
                Ok(j) => j,
                Err(e) => {
                    error = Some(reason(&e));
                    break;
                }
            };
            for job in jobs {
                let output = llm.complete(&job.system, &job.prompt).await;
                match finish_item_summary(&self.lock(), job.item_id, output) {
                    Ok(Ok(())) => {
                        ok += 1;
                        // 要約は1件ずつ時間がかかるので、途中経過も出す。
                        if ok % SUMMARY_PROGRESS_EVERY == 0 {
                            log.push(format!("summaries: {ok} so far"));
                        }
                    }
                    Ok(Err(message)) => {
                        error = Some(message);
                        break 'categories;
                    }
                    Err(e) => {
                        error = Some(reason(&e));
                        break 'categories;
                    }
                }
            }
        }
        let detail = error.map(|e| format!(", error {e}")).unwrap_or_default();
        log.push(format!("summaries: {ok} ok{detail}"));
    }

    /// 期間要約を順に作る。ロックは計画と保存のときだけ取る。
    async fn digests(
        &self,
        period: Period,
        date: NaiveDate,
        scopes: &[DigestScope],
        force: bool,
    ) -> Result<Vec<Option<DigestOutcome>>> {
        let llm = (self.llm)(&self.lock(), LlmRole::Digest)?;
        let mut out = Vec::new();
        for scope in scopes {
            let job = plan_digest(&self.lock(), period, date, scope, self.tz, force)?;
            out.push(match job {
                None => None,
                Some(job) => {
                    let output = llm.complete(&job.system, &job.prompt).await;
                    Some(finish_digest(
                        &self.lock(),
                        &job,
                        &llm.name(),
                        output,
                        &Utc::now(),
                    )?)
                }
            });
        }
        Ok(out)
    }

    /// 日次・夕刊・週次・月次のジョブ。アプリの `run_job` と同じ規則。何か生成したら真。
    async fn digest_job(&self, job: &Job, today: NaiveDate, log: &mut Log<'_>) -> bool {
        let label = label(job);
        let mut plan = job_plan(job, today).into_iter();
        let Some((period, start, force)) = plan.next() else {
            return false;
        };
        let outcomes = match self.digests(period, start, &all_scopes(), force).await {
            Ok(o) => o,
            Err(e) => {
                log.push(format!("digest {label}: error {}", reason(&e)));
                return false;
            }
        };
        let errors: Vec<String> = outcomes
            .iter()
            .flatten()
            .filter_map(|o| o.error.clone())
            .collect();
        if !should_record(job, !errors.is_empty()) {
            log.push(format!("digest {label}: failed, will retry: {errors:?}"));
            return true;
        }
        if let Some((key, value)) = last_run_record(job) {
            let _ = self.lock().set_setting(key, &value);
        }
        for (period, start, force) in plan {
            let _ = self.digests(period, start, &all_scopes(), force).await;
        }
        let detail = if errors.is_empty() {
            String::new()
        } else {
            format!(", pending: {errors:?}")
        };
        log.push(format!("digest {label}: done{detail}"));
        true
    }

    /// 保留中の要約を再試行する。保留があれば真。
    async fn retry_pending(&self, log: &mut Log<'_>) -> bool {
        let pending = match self.lock().pending_digests() {
            Ok(p) => p,
            Err(e) => {
                log.push(format!("retry: error {}", reason(&e)));
                return false;
            }
        };
        if pending.is_empty() {
            return false;
        }
        let mut recovered = 0;
        for d in &pending {
            if let Ok(o) = self
                .digests(
                    d.period,
                    d.period_start,
                    std::slice::from_ref(&d.scope),
                    false,
                )
                .await
                && o.first()
                    .is_some_and(|o| o.as_ref().is_some_and(|o| o.error.is_none()))
            {
                recovered += 1;
            }
        }
        log.push(format!(
            "retry: {recovered} of {} pending recovered",
            pending.len()
        ));
        true
    }

    async fn sync(&self, now: DateTime<Utc>, log: &mut Log<'_>) {
        let got = headless::sync_nas(&self.store, &self.env, &*self.make_drive, now).await;
        log.push(match got {
            Ok(SyncOutcome::Disabled) => "sync: off".to_string(),
            Ok(SyncOutcome::Synced(r)) => format!(
                "sync: ok, imported {}, applied {}, uploaded {}, written {}",
                r.imported_ops,
                r.applied_ops,
                r.uploaded_ops,
                r.written_files.len()
            ),
            Err(message) => format!("sync: error {message}"),
        });
    }
}

/// 1回評価して、ログを1行ずつ出来たときに標準出力に出す。
pub async fn tick(store: Store, tz: FixedOffset) -> Result<()> {
    let make_drive: Box<MakeDrive> = Box::new(headless::webdav);
    let runner = Runner::new(
        store,
        tz,
        Box::new(provider_from_settings),
        make_drive,
        HeadlessEnv::from_env(tz),
    )?
    .with_sink(Box::new(|line: &str| println!("{line}")));
    let lines = runner.run(Local::now().with_timezone(&tz)).await?;
    if lines.is_empty() {
        println!("tick: nothing due");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use chrono::TimeZone;
    use infodesk_core::sync::drive::{DriveStore, MemoryDrive};
    use infodesk_core::sync::nas::*;
    use std::sync::Arc;

    struct Reply(std::result::Result<String, String>);
    #[async_trait]
    impl LlmProvider for Reply {
        fn name(&self) -> String {
            "mock".into()
        }
        async fn complete(&self, _s: &str, _p: &str) -> infodesk_core::Result<String> {
            self.0.clone().map_err(infodesk_core::Error::Llm)
        }
    }

    fn jst() -> FixedOffset {
        FixedOffset::east_opt(9 * 3600).unwrap()
    }

    /// 2026-11-02 は月曜。
    fn at(h: u32, m: u32) -> DateTime<FixedOffset> {
        jst().with_ymd_and_hms(2026, 11, 2, h, m, 0).unwrap()
    }

    /// ソースはすべて無効 (通信しない)。同期は LAN の NAS、パスワードあり。
    fn runner(llm: LlmFactory) -> Runner {
        let store = Store::open_in_memory().unwrap();
        infodesk_core::seed::seed_defaults(&store).unwrap();
        for src in store.list_sources().unwrap() {
            store.set_source_enabled(src.id, false).unwrap();
        }
        store.set_setting(SETTING_ENABLED, "true").unwrap();
        store.set_setting(SETTING_BACKEND, BACKEND_NAS).unwrap();
        store
            .set_setting(SETTING_NAS_URL, "http://192.168.0.151:5005/infodesk/")
            .unwrap();
        store.set_setting(SETTING_NAS_USER, "infodesk").unwrap();
        let env = HeadlessEnv {
            nas_password: Some("pw".into()),
            tz: jst(),
        };
        let drive: Box<MakeDrive> =
            Box::new(|_| Ok(Arc::new(MemoryDrive::new()) as Arc<dyn DriveStore>));
        Runner::new(store, jst(), llm, drive, env).unwrap()
    }

    fn ok_llm() -> LlmFactory {
        Box::new(|_, _| Ok(Box::new(Reply(Ok("要約".into()))) as Box<dyn LlmProvider>))
    }

    fn setting(r: &Runner, key: &str) -> Option<String> {
        r.store.lock().unwrap().get_setting(key).unwrap()
    }

    fn has(log: &[String], prefix: &str) -> bool {
        log.iter().any(|l| l.starts_with(prefix))
    }

    fn add_item(s: &Store, source: infodesk_core::model::Source, at: &str) {
        let time = DateTime::parse_from_rfc3339(at)
            .unwrap()
            .with_timezone(&Utc);
        let outcome = infodesk_core::collector::FetchOutcome {
            source,
            result: Ok(vec![infodesk_core::model::NewItem {
                url: "https://example.com/weekly-1".into(),
                title: "認知症の記事".into(),
                excerpt: "本文".into(),
                author: None,
                published_at: Some(time),
                score: None,
                image_url: None,
            }]),
        };
        infodesk_core::collector::apply_outcomes(s, &[outcome], &time).unwrap();
    }

    #[tokio::test]
    async fn first_run_collects_digests_and_syncs_in_order() {
        let r = runner(ok_llm());
        let log = r.run(at(8, 0)).await.unwrap();
        let pos = |p: &str| {
            log.iter()
                .position(|l| l.starts_with(p))
                .unwrap_or_else(|| panic!("{p} in {log:?}"))
        };
        assert!(pos("collect:") < pos("sync:"), "{log:?}");
        assert!(has(&log, "digest daily 2026-11-01"), "{log:?}");
        assert!(setting(&r, "schedule.last_collect_at").is_some());
        assert_eq!(
            setting(&r, "schedule.last_daily_for").as_deref(),
            Some("2026-11-01")
        );
        assert!(setting(&r, SETTING_LAST_SYNC_AT).is_some());
        assert!(
            setting(&r, "schedule.attempts")
                .unwrap()
                .contains("\"collect\"")
        );
        assert!(setting(&r, "schedule.last_retry_at").is_some());
    }

    #[tokio::test]
    async fn a_second_run_right_after_does_nothing() {
        let r = runner(ok_llm());
        r.run(at(8, 0)).await.unwrap();
        let log = r.run(at(8, 5)).await.unwrap();
        assert!(
            log.iter()
                .all(|l| !l.starts_with("collect:") && !l.starts_with("digest")),
            "{log:?}"
        );
    }

    #[tokio::test]
    async fn a_failed_weekly_waits_15_minutes_between_processes() {
        // 各 tick は別のプロセスなので、試行時刻は DB の schedule.attempts だけが覚えている。
        let llm: LlmFactory = Box::new(|_, role| {
            let reply = match role {
                LlmRole::Items => Ok("要約".to_string()),
                LlmRole::Digest => Err("down".to_string()),
            };
            Ok(Box::new(Reply(reply)) as Box<dyn LlmProvider>)
        });
        let r = runner(llm);
        // 前週 (2026-10-26 の週) に記事を1件置き、週次の生成を試させて失敗させる。
        {
            let s = r.store.lock().unwrap();
            let source = s.list_sources().unwrap().remove(0);
            add_item(&s, source, "2026-10-28T09:00:00+09:00");
            // 週次は日次要約から作る (記事からは作らない) ので、前週の日次要約も置く。
            s.save_digest_done(
                &infodesk_core::store::NewDigest {
                    period: Period::Day,
                    period_start: NaiveDate::from_ymd_opt(2026, 10, 28).unwrap(),
                    scope: DigestScope::Category(Category::ALL[0]),
                    body: "日次".into(),
                    item_ids: vec![],
                    provider: "mock".into(),
                },
                &Utc::now(),
            )
            .unwrap();
        }
        r.run(at(8, 0)).await.unwrap();
        assert_eq!(setting(&r, "schedule.last_weekly_for"), None);
        let log = r.run(at(8, 5)).await.unwrap();
        assert!(!has(&log, "digest weekly"), "{log:?}");
        let log = r.run(at(8, 16)).await.unwrap();
        assert!(has(&log, "digest weekly 2026-10-26"), "{log:?}");
    }

    #[tokio::test]
    async fn a_copied_database_continues_its_schedule() {
        // Review Focus 1
        let r = runner(ok_llm());
        {
            let st = r.store.lock().unwrap();
            st.set_setting("schedule.last_collect_at", &at(7, 30).to_rfc3339())
                .unwrap();
            st.set_setting("schedule.last_daily_for", "2026-11-01")
                .unwrap();
            st.set_setting("schedule.last_weekly_for", "2026-10-26")
                .unwrap();
            st.set_setting("schedule.last_monthly_for", "2026-10-01")
                .unwrap();
            st.set_setting("sync.device_id", "mac-device").unwrap();
        }
        let log = r.run(at(8, 0)).await.unwrap();
        assert!(
            log.iter()
                .all(|l| !l.starts_with("collect:") && !l.starts_with("digest")),
            "{log:?}"
        );
        assert_eq!(setting(&r, "sync.device_id").as_deref(), Some("mac-device"));
    }

    #[tokio::test]
    async fn a_missing_provider_does_not_stop_collection() {
        // Review Focus 2
        let llm: LlmFactory = Box::new(|_, _| {
            Err(infodesk_core::Error::Invalid(
                "ANTHROPIC_API_KEY is not set".into(),
            ))
        });
        let r = runner(llm);
        let log = r.run(at(8, 0)).await.unwrap();
        assert!(has(&log, "collect:"), "{log:?}");
        // Invalid の接頭辞 (`invalid value: `) は出さない。
        assert!(
            log.iter()
                .any(|l| l == "summaries: error ANTHROPIC_API_KEY is not set"),
            "{log:?}"
        );
        assert!(
            log.iter()
                .any(|l| l == "digest daily 2026-11-01: error ANTHROPIC_API_KEY is not set"),
            "{log:?}"
        );
        assert!(log.iter().all(|l| !l.contains("invalid value:")), "{log:?}");
        assert!(setting(&r, "schedule.last_collect_at").is_some());
    }

    #[tokio::test]
    async fn each_line_reaches_the_sink_as_it_happens() {
        // Pod が途中で止められても Argo にそこまでのログが残るよう、行は出来た順にすぐ出す。
        let seen: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let collect_seen_first = Arc::new(Mutex::new(None));
        let (seen_in_llm, flag) = (seen.clone(), collect_seen_first.clone());
        let llm: LlmFactory = Box::new(move |_, role| {
            if role == LlmRole::Items {
                let got = seen_in_llm
                    .lock()
                    .unwrap()
                    .iter()
                    .any(|l| l.starts_with("collect:"));
                flag.lock().unwrap().get_or_insert(got);
            }
            Ok(Box::new(Reply(Ok("要約".into()))) as Box<dyn LlmProvider>)
        });
        let sink_lines = seen.clone();
        let r = runner(llm).with_sink(Box::new(move |line: &str| {
            sink_lines.lock().unwrap().push(line.to_string());
        }));
        let log = r.run(at(8, 0)).await.unwrap();
        assert_eq!(*collect_seen_first.lock().unwrap(), Some(true));
        assert_eq!(*seen.lock().unwrap(), log);
    }

    #[tokio::test]
    async fn item_summaries_report_progress_every_20() {
        let r = runner(ok_llm());
        {
            let s = r.store.lock().unwrap();
            s.set_setting("summary.top_n", "30").unwrap();
            let source = s.list_sources().unwrap().remove(0);
            let time = at(7, 0).with_timezone(&Utc);
            let items = (0..25)
                .map(|i| infodesk_core::model::NewItem {
                    url: format!("https://example.com/today-{i}"),
                    title: format!("記事 {i}"),
                    excerpt: "本文".into(),
                    author: None,
                    published_at: Some(time),
                    score: None,
                    image_url: None,
                })
                .collect();
            let outcome = infodesk_core::collector::FetchOutcome {
                source,
                result: Ok(items),
            };
            infodesk_core::collector::apply_outcomes(&s, &[outcome], &time).unwrap();
        }
        let log = r.run(at(8, 0)).await.unwrap();
        let progress: Vec<&String> = log.iter().filter(|l| l.ends_with("so far")).collect();
        assert_eq!(progress, ["summaries: 20 so far"], "{log:?}");
        let pos = |want: &str| log.iter().position(|l| l == want);
        assert!(
            pos("summaries: 20 so far") < pos("summaries: 25 ok"),
            "{log:?}"
        );
    }

    #[tokio::test]
    async fn a_sync_error_is_logged_and_the_run_still_succeeds() {
        let mut r = runner(ok_llm());
        r.env.nas_password = None;
        let log = r.run(at(8, 0)).await.unwrap();
        assert!(
            log.iter()
                .any(|l| l.starts_with("sync: error") && l.contains(NAS_INCOMPLETE_MESSAGE)),
            "{log:?}"
        );
    }

    /// 応答を途中で変えられる LLM。
    fn switchable_llm() -> (Arc<Mutex<std::result::Result<String, String>>>, LlmFactory) {
        let reply = Arc::new(Mutex::new(Ok("夕刊の要約".to_string())));
        let shared = reply.clone();
        let llm: LlmFactory = Box::new(move |_, _| {
            Ok(Box::new(Reply(shared.lock().unwrap().clone())) as Box<dyn LlmProvider>)
        });
        (reply, llm)
    }

    /// 今日 (2026-11-02) の記事を1件置き、その scope を返す。
    fn today_item(r: &Runner) -> DigestScope {
        let s = r.store.lock().unwrap();
        let source = s.list_sources().unwrap().remove(0);
        let scope = DigestScope::Category(source.category);
        add_item(&s, source, "2026-11-02T10:00:00+09:00");
        scope
    }

    fn day_body(r: &Runner, scope: &DigestScope) -> Option<String> {
        r.store
            .lock()
            .unwrap()
            .get_digest(
                Period::Day,
                NaiveDate::from_ymd_opt(2026, 11, 2).unwrap(),
                scope,
            )
            .unwrap()
            .map(|d| d.body)
    }

    fn next_morning(h: u32, m: u32) -> DateTime<FixedOffset> {
        jst().with_ymd_and_hms(2026, 11, 3, h, m, 0).unwrap()
    }

    #[tokio::test]
    async fn the_evening_run_makes_todays_digest_and_the_next_morning_rebuilds_it() {
        let (reply, llm) = switchable_llm();
        let r = runner(llm);
        let scope = today_item(&r);
        let log = r.run(at(16, 59)).await.unwrap();
        assert!(!has(&log, "digest evening"), "{log:?}");
        assert_eq!(day_body(&r, &scope), None);

        let log = r.run(at(17, 2)).await.unwrap();
        let evening = log
            .iter()
            .position(|l| l == "digest evening 2026-11-02: done")
            .unwrap_or_else(|| panic!("{log:?}"));
        let last_sync = log.iter().rposition(|l| l.starts_with("sync:")).unwrap();
        assert!(
            evening < last_sync,
            "the evening edition is synced: {log:?}"
        );
        assert_eq!(day_body(&r, &scope).as_deref(), Some("夕刊の要約"));
        assert_eq!(
            setting(&r, "schedule.last_evening_for").as_deref(),
            Some("2026-11-02")
        );
        let log = r.run(at(17, 20)).await.unwrap();
        assert!(!has(&log, "digest evening"), "once a day: {log:?}");

        *reply.lock().unwrap() = Ok("朝刊の要約".to_string());
        let log = r.run(next_morning(7, 0)).await.unwrap();
        assert!(has(&log, "digest daily 2026-11-02: done"), "{log:?}");
        assert_eq!(day_body(&r, &scope).as_deref(), Some("朝刊の要約"));
        assert_eq!(
            setting(&r, "schedule.last_daily_for").as_deref(),
            Some("2026-11-02")
        );
    }

    #[tokio::test]
    async fn a_failed_morning_rebuild_keeps_the_evening_edition_and_retries() {
        // Review Focus 1
        let (reply, llm) = switchable_llm();
        let r = runner(llm);
        let scope = today_item(&r);
        r.run(at(17, 0)).await.unwrap();
        assert_eq!(day_body(&r, &scope).as_deref(), Some("夕刊の要約"));

        *reply.lock().unwrap() = Err("down".to_string());
        let log = r.run(next_morning(7, 0)).await.unwrap();
        assert!(
            has(&log, "digest daily 2026-11-02: failed, will retry"),
            "{log:?}"
        );
        assert_eq!(day_body(&r, &scope).as_deref(), Some("夕刊の要約"));
        assert_eq!(
            setting(&r, "schedule.last_daily_for").as_deref(),
            Some("2026-11-01")
        );
        let log = r.run(next_morning(7, 5)).await.unwrap();
        assert!(!has(&log, "digest daily"), "{log:?}");

        *reply.lock().unwrap() = Ok("朝刊の要約".to_string());
        let log = r.run(next_morning(7, 16)).await.unwrap();
        assert!(has(&log, "digest daily 2026-11-02: done"), "{log:?}");
        assert_eq!(day_body(&r, &scope).as_deref(), Some("朝刊の要約"));
    }

    #[test]
    fn job_keys_round_trip() {
        let d = |s: &str| NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap();
        for job in [
            Job::Collect,
            Job::Daily(d("2026-11-01")),
            Job::Evening(d("2026-11-02")),
            Job::Weekly(d("2026-10-26")),
            Job::Monthly(d("2026-10-01")),
        ] {
            assert_eq!(job_from_key(&job_key(&job)), Some(job.clone()), "{job:?}");
        }
        assert_eq!(
            job_key(&Job::Evening(d("2026-11-02"))),
            "evening:2026-11-02"
        );
        assert_eq!(job_from_key("weekly:garbage"), None);
        assert_eq!(job_from_key("other"), None);
    }
}
