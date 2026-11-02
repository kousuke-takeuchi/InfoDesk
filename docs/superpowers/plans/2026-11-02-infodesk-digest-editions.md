# Digest Editions (朝刊 / 夕刊) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make today's day digest automatically in the evening (夕刊), always rebuild it the next morning (朝刊), and show the newest edition with edition headings and prev/next browsing on the iPhone and the Mac.

**Architecture:** The schedule rules live in `crates/core/src/schedule.rs`; a new `Job::Evening(today)` and a forced daily rebuild flow unchanged through the CLI `tick` and the Mac writer scheduler. Readers derive the edition of a stored day digest from its entries' `created_at` (no sync format change). The Mac gets a store query and a command listing stored period starts so it can browse like mobile.

**Tech Stack:** Rust (chrono, rusqlite, tokio), Tauri 2 + React/TypeScript (vitest, Testing Library), Flutter/Dart (flutter_test).

**Spec:** `docs/superpowers/specs/2026-11-02-infodesk-digest-editions-design.md`

## Global Constraints

- Three PRs, each on a fresh branch from `master`, none stacked. PR 1 = Tasks 1-4 (branch `feature/digest-editions`, which also carries the spec and this plan). PR 2 = Tasks 5-6 (branch `feature/mobile-digest-editions`, from master after PR 1 merges). PR 3 = Tasks 7-9 (branch `feature/mac-digest-editions`, from master after PR 1 merges). PR 2 and PR 3 must not depend on each other's code.
- Commits follow Conventional Commits (`feat:`, `fix:`, `test:`, `docs:`, `chore:`).
- NO `Co-Authored-By` trailer and no other AI attribution line in any commit message or PR body.
- Comments follow each file's existing language: Rust under `crates/` and `app/src-tauri/` in Japanese, TypeScript under `app/src` in Japanese, Dart under `mobile/` in English.
- UI strings are Japanese. Exact strings: 「夕刊」「朝刊」「M月D日 夕刊」「M月D日 朝刊」「M月D日の記事」「M月D日 H時までの記事」「M月D日 H時M分までの記事」「夕刊の時刻」「前へ」「次へ」. A year prefix 「YYYY年」 is added only when the date's year differs from now's year.
- New setting: `schedule.evening_time`, default `17:00`. New bookkeeping key: `schedule.last_evening_for`. CLI attempt key: `evening:<YYYY-MM-DD>`. Log line: `digest evening <date>: ...`.
- Edition rule: newest `created_at` among `status == done` entries of the day digest for D, in local time: `< D+1 00:00` → 夕刊 of D (cutoff = that time, or none if it is before D 00:00); `>= D+1 00:00` → 朝刊 of D+1. No finished entry → no edition (plain period heading).
- Sync format unchanged. No change to `crates/core/src/sync/`.
- Test times are built from local dates (`DateTime(y, m, d, h)` in Dart, `new Date(y, m - 1, d, h)` in TS) so tests pass in any machine time zone.
- User docs `docs/*.md` and `docs/*.ja.md` are updated in the task that changes the behaviour they describe, both languages.
- Do not push, do not switch branches in a shared checkout; use a worktree per branch.

Test commands (run from the repository root unless a directory is given):

- `cargo test -p infodesk-core -p infodesk`
- `cargo test -p infodesk-app` (same as `cargo test` in `app/src-tauri`)
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cd app && npx tsc --noEmit && npx vitest run`
- `cd mobile && /Users/ktakeuchi/develop/flutter/bin/flutter analyze && /Users/ktakeuchi/develop/flutter/bin/flutter test`

## Review Focus

1. A failed morning rebuild must not leave the evening edition as the final version forever: the daily job is not recorded and runs again after 15 minutes, while the evening body stays visible (Task 2 test `a_failed_morning_rebuild_keeps_the_evening_edition_and_retries`).
2. An evening that passed while the server was down must not be made up the next morning (that would label a whole-day digest as a 夕刊 of the wrong day): `due_jobs` only ever targets today (Task 1 test `evening_is_for_today_only_and_not_made_up`).
3. Minute formatting and the midnight boundary: 17:00 → 「17時」, 17:03 → 「17時3分」, exactly 00:00 of D+1 → 朝刊 (Task 5 and Task 8 unit tests).
4. A reader who stepped back must not be yanked to the newest edition by a sync, and one who did not step must see the new 夕刊 after a sync (Task 6 and Task 9 tests).
5. Mac requests must use the shown date: stats, regenerate, chat attachment and chat period after stepping to an older edition (Task 9 test `shows today's evening edition and steps back to a morning edition`).

---

## PR 1: schedule (core, CLI, Mac writer)

### Task 1: Evening job and forced morning rebuild in the core schedule

**PR:** 1

**Files:**
- Modify: `crates/core/src/schedule.rs`
- Modify: `crates/core/src/seed.rs` (`DEFAULT_SETTINGS`)
- Test: `crates/core/src/schedule.rs` (`mod tests`)

**Interfaces:**
- Consumes: nothing new.
- Produces:
  - `ScheduleConfig { ..., pub evening_time: NaiveTime }`
  - `LastRuns { ..., pub evening_for: Option<NaiveDate> }`
  - `Job::Evening(NaiveDate)`
  - `due_jobs` order: `Collect, Daily, Evening, Weekly, Monthly`
  - `job_plan(&Job::Daily(day), today) == [(Period::Day, day, true), (Period::Week, today, true), (Period::Month, today, true)]`
  - `job_plan(&Job::Evening(day), today) == [(Period::Day, day, true)]`
  - `last_run_record(&Job::Evening(day)) == Some(("schedule.last_evening_for", day.to_string()))`
  - `should_record(job, failed) == !(failed && matches!(job, Daily | Weekly | Monthly))`
  - `DEFAULT_SETTINGS` contains `("schedule.evening_time", "17:00")`

- [ ] **Step 1: Write the failing tests**

Add to `mod tests` in `crates/core/src/schedule.rs`:

```rust
    #[test]
    fn evening_time_defaults_to_17_and_tolerates_garbage() {
        assert_eq!(cfg().evening_time, NaiveTime::from_hms_opt(17, 0, 0).unwrap());
        let with = |v: &'static str| {
            ScheduleConfig::from_settings(|k| (k == "schedule.evening_time").then(|| v.to_string()))
                .evening_time
        };
        assert_eq!(with("18:30"), NaiveTime::from_hms_opt(18, 30, 0).unwrap());
        assert_eq!(with("25:99"), NaiveTime::from_hms_opt(17, 0, 0).unwrap());
        assert_eq!(with(""), NaiveTime::from_hms_opt(17, 0, 0).unwrap());
    }

    /// 2026-11-02 (月) の朝の分は済んでいる状態。収集も今済んだことにする。
    fn morning_done(now: DateTime<FixedOffset>) -> LastRuns {
        LastRuns {
            collect_at: Some(now.with_timezone(&Utc)),
            daily_for: Some(d("2026-11-01")),
            weekly_for: Some(d("2026-10-26")),
            monthly_for: Some(d("2026-10-01")),
            evening_for: None,
        }
    }

    #[test]
    fn evening_runs_once_after_its_time() {
        let before = jst("2026-11-02T16:59:00+09:00");
        assert!(due_jobs(before, &cfg(), &morning_done(before)).is_empty());
        let at = jst("2026-11-02T17:00:00+09:00");
        assert_eq!(
            due_jobs(at, &cfg(), &morning_done(at)),
            vec![Job::Evening(d("2026-11-02"))]
        );
        let late = jst("2026-11-02T23:59:00+09:00");
        let done = LastRuns {
            evening_for: Some(d("2026-11-02")),
            ..morning_done(late)
        };
        assert!(due_jobs(late, &cfg(), &done).is_empty());
    }

    #[test]
    fn evening_is_for_today_only_and_not_made_up() {
        // Review Focus 2: 前日の夕方に止まっていても、翌朝は日次だけ。夕刊は補わない。
        let next = jst("2026-11-03T08:00:00+09:00");
        let last = LastRuns {
            evening_for: Some(d("2026-10-31")),
            ..morning_done(next)
        };
        assert_eq!(
            due_jobs(next, &cfg(), &last),
            vec![Job::Daily(d("2026-11-02"))]
        );
        let evening = jst("2026-11-03T17:00:00+09:00");
        let last = LastRuns {
            daily_for: Some(d("2026-11-02")),
            evening_for: Some(d("2026-11-02")),
            ..morning_done(evening)
        };
        assert_eq!(
            due_jobs(evening, &cfg(), &last),
            vec![Job::Evening(d("2026-11-03"))]
        );
    }

    #[test]
    fn evening_comes_after_collect_and_daily() {
        let now = jst("2026-11-02T17:00:00+09:00");
        assert_eq!(
            due_jobs(now, &cfg(), &LastRuns::default()),
            vec![
                Job::Collect,
                Job::Daily(d("2026-11-01")),
                Job::Evening(d("2026-11-02")),
                Job::Weekly(d("2026-10-26")),
                Job::Monthly(d("2026-10-01")),
            ]
        );
    }

    #[test]
    fn evening_plan_record_and_retry_rule() {
        let today = d("2026-11-02");
        assert_eq!(
            job_plan(&Job::Evening(today), today),
            vec![(Period::Day, today, true)]
        );
        assert_eq!(
            last_run_record(&Job::Evening(today)),
            Some(("schedule.last_evening_for", "2026-11-02".to_string()))
        );
        // 夕刊の失敗は保留の再試行で補うので記録する。朝の作り直しの失敗は記録しない。
        assert!(should_record(&Job::Evening(today), true));
        assert!(!should_record(&Job::Daily(d("2026-11-01")), true));
        assert!(should_record(&Job::Daily(d("2026-11-01")), false));
    }

    #[test]
    fn evening_time_is_a_default_setting() {
        assert!(
            crate::seed::DEFAULT_SETTINGS.contains(&("schedule.evening_time", "17:00"))
        );
    }
```

Change the existing tests so they compile and match the new rules:

- In `weekly_and_monthly_target_previous_periods` (the `early` literal), `monthly_waits_for_its_time_on_the_first_and_rolls_over_the_year` (both full `LastRuns { ... }` literals) and `weekly_on_sunday_waits_for_weekly_time`, add the field `evening_for: None,` after `monthly_for`.
- In `job_plan_lists_digest_runs_in_order`, the first entry becomes `(Period::Day, d("2026-10-05"), true),` and the comment above the `Weekly` assertion stays.
- In `last_runs_from_settings_parses_and_ignores_garbage`, add `"schedule.last_evening_for" => "2026-10-05",` to the match and `assert_eq!(r.evening_for, Some(d("2026-10-05")));`.
- In `finished_jobs_record_their_key_and_failed_weekly_or_monthly_do_not`, rename it to `finished_jobs_record_their_key_and_failed_daily_weekly_or_monthly_do_not` and replace `assert!(should_record(&Job::Daily(d("2026-10-05")), true));` with `assert!(!should_record(&Job::Daily(d("2026-10-05")), true));`.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p infodesk-core schedule`
Expected: compile errors (`no field evening_time`, `no variant Evening`, `missing field evening_for`).

- [ ] **Step 3: Implement**

In `crates/core/src/schedule.rs`:

```rust
#[derive(Debug, Clone, PartialEq)]
pub struct ScheduleConfig {
    pub collect_interval: Duration,
    pub daily_time: NaiveTime,
    /// 今日の日次要約 (夕刊) を作る時刻。
    pub evening_time: NaiveTime,
    pub weekly_day: Weekday,
    pub weekly_time: NaiveTime,
    pub monthly_time: NaiveTime,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct LastRuns {
    pub collect_at: Option<DateTime<Utc>>,
    pub daily_for: Option<NaiveDate>,
    pub evening_for: Option<NaiveDate>,
    pub weekly_for: Option<NaiveDate>,
    pub monthly_for: Option<NaiveDate>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Job {
    Collect,
    /// 対象日 (前日)。夕刊があっても作り直す (朝刊)。
    Daily(NaiveDate),
    /// 対象日 (今日)。夕刊。
    Evening(NaiveDate),
    /// 対象週の月曜 (前週)。
    Weekly(NaiveDate),
    /// 対象月の1日 (前月)。
    Monthly(NaiveDate),
}
```

`ScheduleConfig::from_settings`: update the doc comment default list to 「(3時間、7:00、17:00、月曜 7:30、1日 8:00)」 and add `evening_time: parse_time(get("schedule.evening_time"), (17, 0)),` after `daily_time`.

`due_jobs`: extend the doc comment with 「夕刊は evening_time 以降の最初の評価で今日を対象に1回だけ出す。過ぎた日の夕刊は補わない (翌朝の日次が作る)。」 and insert after the daily block:

```rust
    // 夕刊: 対象は今日だけ。前日分は補わない。
    if time >= cfg.evening_time && last.evening_for.is_none_or(|d| d < today) {
        jobs.push(Job::Evening(today));
    }
```

`job_plan`: update the doc comment with 「日次は夕刊が作った前日の要約を作り直す (force)。夕刊は今日の日次要約だけを作る。」 and change the arms:

```rust
        Job::Daily(day) => vec![
            (Period::Day, *day, true),
            (Period::Week, today, true),
            (Period::Month, today, true),
        ],
        Job::Evening(day) => vec![(Period::Day, *day, true)],
```

`LastRuns::from_settings`: add `evening_for: date("schedule.last_evening_for"),`.

`last_run_record`: add `Job::Evening(day) => Some(("schedule.last_evening_for", day.to_string())),`.

`should_record`:

```rust
/// ジョブを実行済みとして記録するか。日次・週次・月次は作り直し (force) なので保留の再試行では
/// 補えない (生成済みの行は失敗で変わらない)。1件でも生成に失敗したら記録せず、次の評価で再実行する。
/// 夕刊の失敗は保留の再試行で補うので記録する。
pub fn should_record(job: &Job, generation_failed: bool) -> bool {
    !(matches!(job, Job::Daily(_) | Job::Weekly(_) | Job::Monthly(_)) && generation_failed)
}
```

In `crates/core/src/seed.rs`, add after `("schedule.daily_time", "07:00"),`:

```rust
    ("schedule.evening_time", "17:00"),
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p infodesk-core`
Expected: PASS (all schedule and seed tests).

Run: `cargo build -p infodesk`
Expected: FAIL in `crates/cli/src/tick.rs` (`non-exhaustive patterns: Job::Evening(_) not covered` in `job_key`). Task 2 fixes it; do not commit a broken workspace. Continue to Task 2 before committing, or commit Tasks 1 and 2 together (Step 5 of Task 2).

### Task 2: CLI tick runs the evening job and retries a failed morning rebuild

**PR:** 1

**Files:**
- Modify: `crates/cli/src/tick.rs` (`job_key`, `job_from_key`, tests)
- Modify: `docs/08-cli.md`, `docs/08-cli.ja.md` (`tick` section)
- Test: `crates/cli/src/tick.rs` (`mod tests`)

**Interfaces:**
- Consumes: `Job::Evening`, `job_plan`, `should_record`, `last_run_record` from Task 1.
- Produces: attempt key `evening:<date>`; log lines `digest evening <date>: done` and `digest daily <date>: failed, will retry: [...]`.

- [ ] **Step 1: Write the failing tests**

In `mod tests` of `crates/cli/src/tick.rs`, extend `job_keys_round_trip` with `Job::Evening(d("2026-11-02")),` in the array and add `assert_eq!(job_key(&Job::Evening(d("2026-11-02"))), "evening:2026-11-02");`. Then add:

```rust
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
            .get_digest(Period::Day, NaiveDate::from_ymd_opt(2026, 11, 2).unwrap(), scope)
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
        assert!(evening < last_sync, "the evening edition is synced: {log:?}");
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
```

Note: `add_item` uses one fixed URL; each test calls it once.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p infodesk tick`
Expected: compile error `non-exhaustive patterns: &Job::Evening(_) not covered` in `job_key`.

- [ ] **Step 3: Implement**

In `crates/cli/src/tick.rs`:

```rust
pub(crate) fn job_key(job: &Job) -> String {
    match job {
        Job::Collect => "collect".to_string(),
        Job::Daily(d) => format!("daily:{d}"),
        Job::Evening(d) => format!("evening:{d}"),
        Job::Weekly(d) => format!("weekly:{d}"),
        Job::Monthly(d) => format!("monthly:{d}"),
    }
}
```

and in `job_from_key` add `"evening" => Some(Job::Evening(date)),`. Update the doc comment of `digest_job` to 「日次・夕刊・週次・月次のジョブ。アプリの `run_job` と同じ規則。何か生成したら真。」.

Docs, `docs/08-cli.md` `### tick` paragraph: replace "collection, article summaries, day, week and month digests, and sync to the NAS" with "collection, article summaries, day digests (the evening edition of today at `schedule.evening_time` and the rebuilt previous day at `schedule.daily_time`), week and month digests, and sync to the NAS", and add `digest evening <date>: ...` to the list of lines after `digest daily <date>: ...`. `docs/08-cli.ja.md`: replace 「収集、記事要約、日次・週次・月次のダイジェスト、NAS への同期」 with 「収集、記事要約、日次のダイジェスト (`schedule.evening_time` に今日の夕刊、`schedule.daily_time` に前日の作り直し)、週次・月次のダイジェスト、NAS への同期」 and add 「`digest evening <日付>: ...`」 after 「`digest daily <日付>: ...`」.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p infodesk-core -p infodesk`
Expected: PASS.

Run: `cargo clippy --workspace --all-targets -- -D warnings`
Expected: no warnings (the Mac crate compiles unchanged because `scheduler.rs` matches only `Job::Collect`).

- [ ] **Step 5: Commit (Tasks 1 and 2)**

```bash
git add crates/core/src/schedule.rs crates/core/src/seed.rs crates/cli/src/tick.rs docs/08-cli.md docs/08-cli.ja.md
git commit -m "feat: make an evening edition of today's digest and rebuild it every morning"
```

### Task 3: Mac writer: scheduler test, Schedule tab field, schedule docs

**PR:** 1

**Files:**
- Test: `app/src-tauri/src/scheduler.rs` (`mod tests`; no production change)
- Modify: `app/src/views/settings/ScheduleTab.tsx`
- Test: `app/src/views/settings/ScheduleTab.test.tsx`
- Modify: `docs/03-sources-and-topics.md`, `docs/03-sources-and-topics.ja.md` (§5), `docs/07-settings.md`, `docs/07-settings.ja.md` (Schedule tab bullet, settings table, bookkeeping keys), `docs/02-getting-started.md`, `docs/02-getting-started.ja.md` (step 5), `docs/04-digests.md`, `docs/04-digests.ja.md` (§2 last paragraph)

**Interfaces:**
- Consumes: `Job::Evening`, forced `Job::Daily` plan, `schedule.evening_time` in `DEFAULT_SETTINGS` (Task 1).
- Produces: nothing used by later tasks.

- [ ] **Step 1: Write the failing tests**

In `app/src-tauri/src/scheduler.rs` `mod tests`, add:

```rust
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
```

In `app/src/views/settings/ScheduleTab.test.tsx`, add `"schedule.evening_time": "17:00",` to the `settings` fixture, add `["夕刊の時刻"]` to the `it.each` list of time fields, and add:

```tsx
  it("saves the evening edition time", async () => {
    const ipc = setupIpc({ get_settings: () => settings, set_setting: () => undefined });
    render(<ScheduleTab status={status} />);
    const evening = await screen.findByLabelText("夕刊の時刻");
    expect(evening).toHaveValue("17:00");
    await userEvent.clear(evening);
    await userEvent.type(evening, "18:30{enter}");
    expect(ipc.last("set_setting")).toEqual({ key: "schedule.evening_time", value: "18:30" });
    expect(screen.getByText(/夕刊の時刻に今日の日次要約を作ります/)).toBeInTheDocument();
  });
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p infodesk-app the_evening_edition`
Expected: PASS already (no production change is needed; this test pins the shared core behaviour on the Mac). If it fails, the core change in Task 1 is wrong; fix it there.

Run: `cd app && npx vitest run src/views/settings/ScheduleTab.test.tsx`
Expected: FAIL (`Unable to find a label with the text of: 夕刊の時刻`).

- [ ] **Step 3: Implement**

In `app/src/views/settings/ScheduleTab.tsx`, replace the 「要約の生成」 description and add the field after the daily field:

```tsx
        <p className="desc">夕刊の時刻に今日の日次要約を作ります (夕刊)。日次の時刻に前日の日次要約を一日分で作り直して確定し (朝刊)、現在の週次と月次を作り直します。収集では要約を作り直しません。</p>
        {timeField("daily-time", "日次要約の時刻", "schedule.daily_time", "HH:MM")}
        {timeField("evening-time", "夕刊の時刻", "schedule.evening_time", "HH:MM。1日1回、過ぎた分は補いません")}
```

Docs (both languages, same meaning):

- `docs/03-sources-and-topics.md` §5, replace the **Daily digest** bullet with:
  - "- **Daily digest (morning edition, 朝刊)**: at 07:00 the previous day's digest is rebuilt from the whole day and finalized (`schedule.daily_time`), even when an evening edition exists. If the rebuild fails, the evening edition stays and the job is tried again every 15 minutes."
  - "- **Evening edition (夕刊)**: at 17:00 today's digest is made from the articles collected so far (`schedule.evening_time`). It runs once a day; an evening that was missed is not made up. You can still make today's digest at any time with **Generate** or **Regenerate**."
- `docs/03-sources-and-topics.ja.md` §5, replace the **日次** bullet with:
  - 「- **日次 (朝刊)**: 07:00 に前日の要約を一日分で作り直して確定します (`schedule.daily_time`)。夕刊があっても作り直します。作り直しに失敗したときは夕刊が残り、15分ごとにやり直します。」
  - 「- **夕刊**: 17:00 に、それまでに集めた記事で今日の要約を作ります (`schedule.evening_time`)。1日1回で、過ぎた分は補いません。今日の要約は「生成」や「再生成」でいつでも作れます。」
- `docs/07-settings.md`: the Schedule tab bullet "要約の生成" becomes "the evening edition time, the daily time, the weekly day and time, and the monthly time (on the 1st). At the evening time today's digest is made (the evening edition). At the daily time the previous day's digest is rebuilt and finalized (the morning edition) and the current week and month are rebuilt. Collection does not rebuild digests; **Generate** or **Regenerate** makes today's digest at any time." Add the table row after `schedule.daily_time`: "| `schedule.evening_time` | `17:00` | Time (HH:MM) to make today's digest (the evening edition). Once a day; a missed evening is not made up. |" and change the `schedule.daily_time` description to "Time (HH:MM) to rebuild and finalize the previous day's digest (the morning edition)." Add `schedule.last_evening_for` to the bookkeeping keys sentence.
- `docs/07-settings.ja.md`: the same three changes: 「要約の生成」 bullet 「夕刊の時刻、日次の時刻、週次の曜日と時刻、月次の時刻 (毎月1日)。夕刊の時刻に今日の日次要約を作ります (夕刊)。日次の時刻に前日の日次要約を作り直して確定し (朝刊)、現在の週次と月次を作り直します。収集では要約を作り直しません。「生成」または「再生成」で今日の要約をいつでも作れます。」; row 「| `schedule.evening_time` | `17:00` | 今日の日次要約 (夕刊) を作る時刻 (HH:MM)。1日1回で、過ぎた分は補いません。 |」; `schedule.daily_time` 「前日の日次要約を作り直して確定する時刻 (HH:MM)。朝刊。」; add `schedule.last_evening_for` to the bookkeeping sentence.
- `docs/02-getting-started.md` step 5: replace "and the previous day's digest is finalized at 07:00. Today's digest is made when you press **Generate**." with "today's digest is made at 17:00 (the evening edition) and rebuilt and finalized at 07:00 the next morning (the morning edition). You can also make today's digest with **Generate**." `docs/02-getting-started.ja.md`: replace 「前日の日次要約は 07:00 に確定します。今日の要約は「生成」ボタンで作ります。」 with 「今日の要約は 17:00 に作られ (夕刊)、翌朝 07:00 に作り直して確定します (朝刊)。「生成」ボタンでも作れます。」
- `docs/04-digests.md` §2 last paragraph: "Collection does not build digests. Today's digest is made automatically at the evening time (the evening edition, 夕刊), and the day is rebuilt and finalized at the daily time the next morning (the morning edition, 朝刊). Press **Generate** (or **Regenerate**) to make it at any other time. When digests are generated is described in [Sources and Topics](03-sources-and-topics.md#5-collection-schedule)." `docs/04-digests.ja.md`: 「今日の要約は収集では作られません。夕刊の時刻に自動で作られ (夕刊)、翌朝の日次の時刻に一日分で作り直して確定します (朝刊)。ほかの時刻には「生成」(または「再生成」)で作ります。ダイジェストを作る時刻は[ソースとトピック](03-sources-and-topics.ja.md#5-収集のスケジュール)にあります。」

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p infodesk-app`
Expected: PASS.

Run: `cd app && npx tsc --noEmit && npx vitest run`
Expected: PASS.

Run: `cargo clippy --workspace --all-targets -- -D warnings`
Expected: no warnings.

- [ ] **Step 5: Commit**

```bash
git add app/src-tauri/src/scheduler.rs app/src/views/settings/ScheduleTab.tsx app/src/views/settings/ScheduleTab.test.tsx docs/02-getting-started.md docs/02-getting-started.ja.md docs/03-sources-and-topics.md docs/03-sources-and-topics.ja.md docs/04-digests.md docs/04-digests.ja.md docs/07-settings.md docs/07-settings.ja.md
git commit -m "feat: add the evening edition time to the Schedule tab"
```

### Task 4: Ship PR 1 to the home server (after the PR merges)

**PR:** 1 (operations; no code)

**Files:** none committed. `deploy/k8s/workflowtemplate.yaml` and `deploy/k8s/shell.yaml` keep the `TAG` placeholder in git; the tag goes into a local copy only.

- [ ] **Step 1: Build and push the image from merged master**

Run (repository root, on the merge commit of master): `deploy/k8s/build-and-push.sh`
Expected: the test stage and the image build pass, and it prints `registry.umafolio.site/infodesk:<short sha>`.

- [ ] **Step 2: Apply the WorkflowTemplate with the new tag**

Put the printed tag in place of `TAG` in a local copy of `deploy/k8s/workflowtemplate.yaml` (and `deploy/k8s/shell.yaml`), then run, as in `docs/15-home-server.md` "Updating":
`kubectl apply -f deploy/k8s/workflowtemplate.yaml`
Expected: `workflowtemplate.argoproj.io/... configured`.

- [ ] **Step 3: Check the first evening and morning runs**

In the Argo log of the first run after 17:00: `digest evening <today>: done` followed by `sync: ok, ...`. In the first run after 07:00 the next day: `digest daily <yesterday>: done`. On the iPhone (still the old build) the 今日 tab shows today's digest after 17:05.

---

## PR 2: mobile edition headings and selection

Start from a fresh branch `feature/mobile-digest-editions` from master (PR 1 merged). Nothing here depends on PR 1 code; it reads `created_at` that every writer already writes.

### Task 5: Edition rule for the phone

**PR:** 2

**Files:**
- Create: `mobile/lib/app/editions.dart`
- Test: `mobile/test/app/editions_test.dart`

**Interfaces:**
- Consumes: `DigestRecord` (`mobile/lib/sync/format.dart`: `status`, `createdAt`).
- Produces:
  - `enum EditionKind { morning, evening }`
  - `class Edition { EditionKind kind; DateTime issue; DateTime articles; DateTime? cutoff; }`
  - `Edition? editionOf(String start, List<DigestRecord> entries)`
  - `({String title, String range}) editionHeading(Edition e, {required DateTime now})`

- [ ] **Step 1: Write the failing test**

`mobile/test/app/editions_test.dart`:

```dart
import 'package:flutter_test/flutter_test.dart';
import 'package:infodesk_mobile/app/editions.dart';
import 'package:infodesk_mobile/sync/format.dart';

DigestRecord entry(Object made, {String status = 'done'}) => DigestRecord(
  scope: {'category': 'medcare'},
  body: 'b',
  status: status,
  itemIds: const [],
  provider: 'p',
  createdAt: made is DateTime ? made.toUtc().toIso8601String() : made as String,
);

final now = DateTime(2026, 11, 2, 18);

({String title, String range}) heading(String start, List<DigestRecord> e) =>
    editionHeading(editionOf(start, e)!, now: now);

void main() {
  test('made during the day is that day\'s evening edition', () {
    final e = editionOf('2026-11-02', [entry(DateTime(2026, 11, 2, 17, 3))])!;
    expect(e.kind, EditionKind.evening);
    expect(e.issue, DateTime(2026, 11, 2));
    expect(e.cutoff, DateTime(2026, 11, 2, 17, 3));
    expect(heading('2026-11-02', [entry(DateTime(2026, 11, 2, 17, 3))]), (
      title: '11月2日 夕刊',
      range: '11月2日 17時3分までの記事',
    ));
    expect(
      heading('2026-11-02', [entry(DateTime(2026, 11, 2, 17))]).range,
      '11月2日 17時までの記事',
    );
    expect(
      heading('2026-11-02', [entry(DateTime(2026, 11, 2, 23, 59))]).range,
      '11月2日 23時59分までの記事',
    );
  });

  test('made after the day ends is the next day\'s morning edition', () {
    expect(heading('2026-11-01', [entry(DateTime(2026, 11, 2, 7, 5))]), (
      title: '11月2日 朝刊',
      range: '11月1日の記事',
    ));
    // Exactly midnight already belongs to the next day.
    final e = editionOf('2026-11-01', [entry(DateTime(2026, 11, 2))])!;
    expect(e.kind, EditionKind.morning);
    expect(e.cutoff, isNull);
  });

  test('the newest finished entry decides; others are ignored', () {
    final mixed = [
      entry(DateTime(2026, 11, 2, 17)),
      entry(DateTime(2026, 11, 3, 7)),
    ];
    expect(editionOf('2026-11-02', mixed)!.kind, EditionKind.morning);
    final pendingLater = [
      entry(DateTime(2026, 11, 2, 17)),
      entry(DateTime(2026, 11, 3, 7), status: 'pending'),
      entry('garbage'),
    ];
    expect(editionOf('2026-11-02', pendingLater)!.kind, EditionKind.evening);
  });

  test('no finished or readable entry means no edition', () {
    expect(editionOf('2026-11-02', const []), isNull);
    expect(
      editionOf('2026-11-02', [entry(DateTime(2026, 11, 2, 17), status: 'failed')]),
      isNull,
    );
    expect(editionOf('2026-11-02', [entry('garbage')]), isNull);
  });

  test('a time before the day gives no cutoff', () {
    expect(heading('2026-11-02', [entry(DateTime(2026, 11, 1, 23))]), (
      title: '11月2日 夕刊',
      range: '11月2日の記事',
    ));
  });

  test('month and year boundaries', () {
    expect(heading('2026-10-31', [entry(DateTime(2026, 11, 1, 7))]), (
      title: '11月1日 朝刊',
      range: '10月31日の記事',
    ));
    final e = editionOf('2026-12-31', [entry(DateTime(2027, 1, 1, 7))])!;
    expect(editionHeading(e, now: DateTime(2027, 1, 5)), (
      title: '1月1日 朝刊',
      range: '2026年12月31日の記事',
    ));
  });
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cd mobile && /Users/ktakeuchi/develop/flutter/bin/flutter test test/app/editions_test.dart`
Expected: FAIL (`Target of URI doesn't exist: 'package:infodesk_mobile/app/editions.dart'`).

- [ ] **Step 3: Implement**

`mobile/lib/app/editions.dart`:

```dart
import '../sync/format.dart';

enum EditionKind { morning, evening }

/// A stored day digest seen as a newspaper edition.
class Edition {
  const Edition({
    required this.kind,
    required this.issue,
    required this.articles,
    this.cutoff,
  });

  final EditionKind kind;

  /// The day the edition is named after: D+1 for a morning edition, D for an
  /// evening one (local midnight).
  final DateTime issue;

  /// The day whose articles it covers (D, local midnight).
  final DateTime articles;

  /// When an evening edition was made (local). Null for a morning edition,
  /// or when the time is before the day itself.
  final DateTime? cutoff;
}

/// The edition of the day digest for [start] (YYYY-MM-DD). The newest
/// `created_at` among finished entries decides: during D it is D's evening
/// edition, from the end of D on it is D+1's morning edition (D's final
/// version). Null when no entry is finished or no time can be read.
Edition? editionOf(String start, List<DigestRecord> entries) {
  final day = DateTime.parse(start);
  DateTime? newest;
  for (final e in entries) {
    if (e.status != 'done') continue;
    final t = DateTime.tryParse(e.createdAt)?.toLocal();
    if (t != null && (newest == null || t.isAfter(newest))) newest = t;
  }
  if (newest == null) return null;
  final next = DateTime(day.year, day.month, day.day + 1);
  if (!newest.isBefore(next)) {
    return Edition(kind: EditionKind.morning, issue: next, articles: day);
  }
  return Edition(
    kind: EditionKind.evening,
    issue: day,
    articles: day,
    cutoff: newest.isBefore(day) ? null : newest,
  );
}

/// 「11月2日 朝刊」/「11月1日の記事」, 「11月2日 夕刊」/「11月2日 17時までの記事」.
/// The year is shown only when it differs from [now]'s.
({String title, String range}) editionHeading(
  Edition e, {
  required DateTime now,
}) {
  String md(DateTime d) =>
      '${d.year == now.year ? '' : '${d.year}年'}${d.month}月${d.day}日';
  if (e.kind == EditionKind.morning) {
    return (title: '${md(e.issue)} 朝刊', range: '${md(e.articles)}の記事');
  }
  final c = e.cutoff;
  final until = c == null
      ? 'の記事'
      : ' ${c.hour}時${c.minute == 0 ? '' : '${c.minute}分'}までの記事';
  return (title: '${md(e.issue)} 夕刊', range: '${md(e.articles)}$until');
}
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `cd mobile && /Users/ktakeuchi/develop/flutter/bin/flutter test test/app/editions_test.dart && /Users/ktakeuchi/develop/flutter/bin/flutter analyze`
Expected: PASS, `No issues found!`.

- [ ] **Step 5: Commit**

```bash
git add mobile/lib/app/editions.dart mobile/test/app/editions_test.dart
git commit -m "feat(mobile): derive the morning or evening edition of a day digest"
```

### Task 6: Edition headings on the iPhone 今日 tab

**PR:** 2

**Files:**
- Modify: `mobile/lib/ui/digest_screen.dart`
- Test: `mobile/test/ui/screens_test.dart`
- Modify: `docs/14-mobile-app.md`, `docs/14-mobile-app.ja.md` (Digest section)

**Interfaces:**
- Consumes: `editionOf`, `editionHeading` (Task 5); `LocalStore.digestStarts`, `LocalStore.digests` (existing).
- Produces: nothing used later.

- [ ] **Step 1: Write the failing tests**

In `mobile/test/ui/screens_test.dart`:

1. Change the default `createdAt` in `digestFile` to `DateTime(2026, 10, 15, 9).toUtc().toIso8601String(),` and in 'renders overview, topics, headline and numbered points' replace `expect(find.text('今日のダイジェスト'), findsOneWidget);` with `expect(find.text('10月15日 夕刊'), findsOneWidget);` and add `expect(find.text('10月15日 9時までの記事'), findsOneWidget);`.

2. In group 'browsing past digests', replace `dayAt` with:

```dart
      DigestFile dayAt(DateTime d) {
        final f = digestFile(body: '見出し: ${dateString(d)}の要点');
        // Past days carry their morning edition (07:00 the next day); today
        // carries an evening edition made at 09:00.
        final made = d.isBefore(dayOnly(fakeNow))
            ? DateTime(d.year, d.month, d.day + 1, 7)
            : DateTime(d.year, d.month, d.day, 9);
        return DigestFile(
          period: 'day',
          periodStart: dateString(d),
          stats: f.stats,
          entries: [
            for (final e in f.entries)
              DigestRecord(
                scope: e.scope,
                body: e.body,
                status: e.status,
                itemIds: e.itemIds,
                provider: e.provider,
                createdAt: made.toUtc().toIso8601String(),
              ),
          ],
        );
      }
```

3. Replace these tests in that group (keep 'previous and next skip gaps and disable at the ends', 'switching period resets the selection' and 'the title names past weeks and months' as they are):

```dart
      testWidgets('falls back to the newest earlier day as its morning edition', (
        tester,
      ) async {
        await pumpApp(tester, setup: (h) => addDays(h, [1, 3]));
        expect(find.text(dayText(1)), findsOneWidget);
        expect(find.text('10月15日 朝刊'), findsOneWidget);
        expect(find.text('10月14日の記事'), findsOneWidget);
        expect(find.text('今日の分は明日の朝 7 時ごろにできます'), findsNothing);
        expect(find.text('この期間のダイジェストはまだありません'), findsNothing);
      });

      testWidgets('today\'s evening edition comes first', (tester) async {
        await pumpApp(tester, setup: (h) => addDays(h, [0, 1]));
        expect(find.text(dayText(0)), findsOneWidget);
        expect(find.text('10月15日 夕刊'), findsOneWidget);
        expect(find.text('10月15日 9時までの記事'), findsOneWidget);
      });

      testWidgets('each edition is named after its issue day', (tester) async {
        await pumpApp(tester, setup: (h) => addDays(h, [1, 4]));
        expect(find.text('10月15日 朝刊'), findsOneWidget);
        await tester.tap(find.byTooltip('前へ'));
        await tester.pumpAndSettle();
        expect(find.text('10月12日 朝刊'), findsOneWidget);
        expect(find.text('10月11日の記事'), findsOneWidget);
      });

      testWidgets('an empty day tab keeps the plain heading and has no note', (
        tester,
      ) async {
        await pumpApp(tester);
        expect(find.text('今日のダイジェスト'), findsOneWidget);
        expect(find.text('今日の分は明日の朝 7 時ごろにできます'), findsNothing);
      });

      testWidgets('yesterday after today\'s evening edition is its morning edition', (
        tester,
      ) async {
        await pumpApp(tester, setup: (h) => addDays(h, [0, 1]));
        await tester.tap(find.byTooltip('前へ'));
        await tester.pumpAndSettle();
        expect(find.text(dayText(1)), findsOneWidget);
        expect(find.text('10月15日 朝刊'), findsOneWidget);
        expect(find.text('10月14日の記事'), findsOneWidget);
      });

      testWidgets('the title stays on one line on a phone', (tester) async {
        await pumpApp(tester, setup: (h) => addDays(h, [0]));
        final title = find.text('10月15日 夕刊');
        expect(title, findsOneWidget);
        // One line is 40 logical pixels; two would be 80.
        expect(tester.getSize(title).height, lessThan(60));
      });

      testWidgets('a sync that brings today\'s evening edition shows it', (
        tester,
      ) async {
        // Review Focus 4
        final h = await pumpApp(tester, setup: (h) => addDays(h, [1]));
        expect(find.text('10月15日 朝刊'), findsOneWidget);
        addDays(h, [0]);
        await h.controller.sync();
        await tester.pumpAndSettle();
        expect(find.text('10月15日 夕刊'), findsOneWidget);
        expect(find.text(dayText(0)), findsOneWidget);
      });

      testWidgets('a stepped selection stays after a sync', (tester) async {
        // Review Focus 4
        final h = await pumpApp(tester, setup: (h) => addDays(h, [1, 3]));
        await tester.tap(find.byTooltip('前へ'));
        await tester.pumpAndSettle();
        expect(find.text(dayText(3)), findsOneWidget);
        addDays(h, [0]);
        await h.controller.sync();
        await tester.pumpAndSettle();
        expect(find.text(dayText(3)), findsOneWidget);
      });
```

The replaced tests are: 'falls back to the newest earlier day with a note', 'no note when today exists', 'the title names the shown day, not today', 'the note shows whenever today has no digest yet', 'no note on a past day when today exists', 'the title stays on one line on a phone'.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cd mobile && /Users/ktakeuchi/develop/flutter/bin/flutter test test/ui/screens_test.dart`
Expected: FAIL (headings still read 「今日のダイジェスト」/「昨日のダイジェスト」, the note is shown, and the sync test still shows the morning edition).

- [ ] **Step 3: Implement**

In `mobile/lib/ui/digest_screen.dart`:

- Add `import '../app/editions.dart';`.
- Add a field to `_DigestScreenState`:

```dart
  /// True once the reader stepped with the arrows. Until then the tab
  /// follows the newest edition, so a sync that brings one shows it.
  bool _pinned = false;
```

- In `_reload`, replace `final start = _pick(resetSelection ? null : _selected, starts);` with:

```dart
    final keep = resetSelection || !_pinned ? null : _selected;
    final start = _pick(keep, starts);
```

- In `_step`, set `_pinned = true;` inside the `setState`.
- In the `Segmented` `onChanged`, set `_pinned = false;` inside the `setState`.
- In `build`, replace the `heading` computation and delete `showTodayNote` and the `if (showTodayNote) Padding(...)` block:

```dart
    final shownStart = _selected ?? _todayStart;
    final digest = _digest;
    // On the day tab a stored digest reads as an edition: 「11月2日 夕刊」.
    final edition =
        _period == DigestPeriod.day &&
            digest != null &&
            digest.periodStart == shownStart
        ? editionOf(shownStart, digest.entries)
        : null;
    final heading = edition != null
        ? editionHeading(edition, now: c.now())
        : periodHeading(_period, DateTime.parse(shownStart), now: c.now());
```

Docs: in `docs/14-mobile-app.md` "### Digest", after the first sentence add: "On 今日 the newest edition is shown. Today's digest made in the evening is the evening edition (「11月2日 夕刊」, with the time it covers, for example 「11月2日 17時までの記事」). After the day ends the server rebuilds it from the whole day; that version is the next day's morning edition (「11月3日 朝刊」, 「11月2日の記事」). Use the arrows beside the date to step through earlier editions, weeks and months. The tab moves to a new edition after a sync until you step with the arrows." In `docs/14-mobile-app.ja.md` 「### ダイジェスト」 (the paragraph starting 「今日、週、月を切り替えます」), add: 「今日のタブには一番新しい版が出ます。夕方に作られた今日の要約は夕刊です (「11月2日 夕刊」。対象の時刻も出ます。例: 「11月2日 17時までの記事」)。日が終わるとサーバーが一日分で作り直し、翌日の朝刊になります (「11月3日 朝刊」、「11月2日の記事」)。日付の横の矢印で、前の版や週、月を順に見られます。矢印で動かすまでは、同期で新しい版が届くとそちらに切り替わります。」

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cd mobile && /Users/ktakeuchi/develop/flutter/bin/flutter analyze && /Users/ktakeuchi/develop/flutter/bin/flutter test`
Expected: `No issues found!` and all tests pass. The other 「今日のダイジェスト」 assertions in `screens_test.dart` (tab switching, onboarding, empty state) have no stored digest and stay unchanged.

- [ ] **Step 5: Commit**

```bash
git add mobile/lib/ui/digest_screen.dart mobile/test/ui/screens_test.dart docs/14-mobile-app.md docs/14-mobile-app.ja.md
git commit -m "feat(mobile): show morning and evening edition headings on the digest tab"
```

---

## PR 3: Mac edition headings, selection and browsing

Start from a fresh branch `feature/mac-digest-editions` from master (PR 1 merged).

### Task 7: List stored period starts (core store and Tauri command)

**PR:** 3

**Files:**
- Modify: `crates/core/src/store/digests.rs` (new method + test)
- Modify: `app/src-tauri/src/commands/digests.rs` (command + test)
- Modify: `app/src-tauri/src/lib.rs` (register the command after `commands::digests::digest_stats,`)
- Modify: `app/src/api.ts`

**Interfaces:**
- Consumes: nothing new.
- Produces:
  - `Store::done_digest_starts(&self, period: Period, topic_id: Option<i64>) -> Result<Vec<NaiveDate>>` (oldest first)
  - `pub fn digest_starts_for(state: &AppState, period: &str, topic_id: Option<i64>) -> AppResult<Vec<String>>`
  - Tauri command `list_digest_starts(period: String, topic_id: Option<i64>) -> Vec<String>`
  - `api.listDigestStarts(period: Period, topicId: number | null): Promise<string[]>` invoking `"list_digest_starts", { period, topicId }`

- [ ] **Step 1: Write the failing tests**

In `crates/core/src/store/digests.rs` `mod tests`:

```rust
    #[test]
    fn done_digest_starts_lists_finished_starts_per_scope_kind() {
        let store = Store::open_in_memory().unwrap();
        let now = at("2026-10-06T00:00:00Z");
        let github = DigestScope::Category(Category::Github);
        for (period, start, scope) in [
            (Period::Day, "2026-10-05", github.clone()),
            (Period::Day, "2026-10-03", github.clone()),
            (Period::Day, "2026-10-05", DigestScope::Category(Category::Arxiv)),
            (Period::Day, "2026-10-04", DigestScope::Topic(7)),
            (Period::Week, "2026-09-28", github.clone()),
        ] {
            store
                .save_digest_done(&nd(period, start, scope, "b", &[]), &now)
                .unwrap();
        }
        // 保留だけの日は入れない。
        store
            .mark_digest_attempt_failed(Period::Day, d("2026-10-06"), &github, "down", &now)
            .unwrap();
        assert_eq!(
            store.done_digest_starts(Period::Day, None).unwrap(),
            vec![d("2026-10-03"), d("2026-10-05")]
        );
        assert_eq!(
            store.done_digest_starts(Period::Day, Some(7)).unwrap(),
            vec![d("2026-10-04")]
        );
        assert!(store.done_digest_starts(Period::Day, Some(70)).unwrap().is_empty());
        assert_eq!(
            store.done_digest_starts(Period::Week, None).unwrap(),
            vec![d("2026-09-28")]
        );
    }
```

In `app/src-tauri/src/commands/digests.rs` `mod tests`:

```rust
    #[test]
    fn list_digest_starts_returns_finished_starts_as_strings() {
        use infodesk_core::store::NewDigest;
        let (state, _) = test_state();
        let now: chrono::DateTime<chrono::Utc> = "2026-10-06T00:00:00Z".parse().unwrap();
        state
            .with_store(|s| {
                for day in ["2026-10-05", "2026-10-04"] {
                    s.save_digest_done(
                        &NewDigest {
                            period: Period::Day,
                            period_start: NaiveDate::parse_from_str(day, "%Y-%m-%d").unwrap(),
                            scope: DigestScope::Category(Category::Github),
                            body: "b".into(),
                            item_ids: vec![],
                            provider: "mock".into(),
                        },
                        &now,
                    )?;
                }
                Ok(())
            })
            .unwrap();
        assert_eq!(
            digest_starts_for(&state, "day", None).unwrap(),
            vec!["2026-10-04", "2026-10-05"]
        );
        assert!(digest_starts_for(&state, "week", None).unwrap().is_empty());
        assert!(digest_starts_for(&state, "year", None).is_err());
    }
```

In `app/src/api.test.ts` ('sends the full filter and camelCase arguments'), add `list_digest_starts: () => ["2026-10-05"],` to the `setupIpc({ ... })` handlers after `get_digests: () => [],`, and after the `digestStats` assertion add:

```ts
    expect(await api.listDigestStarts("week", 3)).toEqual(["2026-10-05"]);
    expect(ipc.last("list_digest_starts")).toEqual({ period: "week", topicId: 3 });
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p infodesk-core done_digest_starts`
Expected: FAIL (`no method named done_digest_starts`).

- [ ] **Step 3: Implement**

`crates/core/src/store/digests.rs`, in `impl Store`:

```rust
    /// 完了した要約がある期間の開始 (古い順)。`topic_id` がなければカテゴリー別、あればそのトピック。
    pub fn done_digest_starts(&self, period: Period, topic_id: Option<i64>) -> Result<Vec<NaiveDate>> {
        let scope = match topic_id {
            Some(id) => DigestScope::Topic(id).key(),
            None => "category:%".to_string(),
        };
        let mut stmt = self.conn.prepare(
            "SELECT DISTINCT period_start FROM digests \
             WHERE period = ?1 AND status = 'done' AND scope LIKE ?2 \
             ORDER BY period_start",
        )?;
        let rows = stmt
            .query_map(params![period.as_str(), scope], |r| col_parse(r, 0, parse_date))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }
```

`app/src-tauri/src/commands/digests.rs`:

```rust
/// 完了した要約がある期間の開始 (YYYY-MM-DD、古い順)。画面の前後の移動に使う。
pub fn digest_starts_for(
    state: &AppState,
    period: &str,
    topic_id: Option<i64>,
) -> AppResult<Vec<String>> {
    let period = parse_period(period)?;
    let starts = state.with_store(|s| s.done_digest_starts(period, topic_id))?;
    Ok(starts
        .into_iter()
        .map(|d| d.format("%Y-%m-%d").to_string())
        .collect())
}

#[tauri::command]
pub fn list_digest_starts(
    state: State<'_, AppState>,
    period: String,
    topic_id: Option<i64>,
) -> AppResult<Vec<String>> {
    digest_starts_for(&state, &period, topic_id)
}
```

`app/src-tauri/src/lib.rs`: add `commands::digests::list_digest_starts,` after `commands::digests::digest_stats,`.

`app/src/api.ts`, after `digestStats`:

```ts
  /** 完了した要約がある期間の開始 (YYYY-MM-DD、古い順)。 */
  listDigestStarts: (period: Period, topicId: number | null) =>
    invoke<string[]>("list_digest_starts", { period, topicId }),
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p infodesk-core -p infodesk-app && cargo clippy --workspace --all-targets -- -D warnings`
Expected: PASS, no warnings.

Run: `cd app && npx tsc --noEmit && npx vitest run src/api.test.ts`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/core/src/store/digests.rs app/src-tauri/src/commands/digests.rs app/src-tauri/src/lib.rs app/src/api.ts app/src/api.test.ts
git commit -m "feat: list stored digest period starts for browsing"
```

### Task 8: Edition, browsing and heading helpers for the Mac

**PR:** 3

**Files:**
- Create: `app/src/lib/editions.ts`, `app/src/lib/editions.test.ts`
- Create: `app/src/lib/browse.ts`, `app/src/lib/browse.test.ts`
- Modify: `app/src/lib/dates.ts` (`periodHeading`, new `parseLocalDate`), `app/src/lib/dates.test.ts`

**Interfaces:**
- Consumes: `Digest` from `app/src/types.ts`.
- Produces:
  - `type Edition = { kind: "morning"; issue: Date; articles: Date } | { kind: "evening"; issue: Date; articles: Date; cutoff: Date | null }`
  - `editionOf(start: string, digests: (Digest | null)[]): Edition | null`
  - `editionHeading(e: Edition, now: Date): { title: string; range: string }`
  - `browseStarts(stored: string[], current: string): string[]`
  - `pickStart(stored: string[], current: string, keep: string | null): string`
  - `periodHeading(period: Period, shown: Date, now: Date = shown): { title: string; range: string }`
  - `parseLocalDate(s: string): Date`

- [ ] **Step 1: Write the failing tests**

`app/src/lib/editions.test.ts`:

```ts
import { describe, expect, it } from "vitest";
import { editionHeading, editionOf } from "./editions";
import type { Digest } from "../types";

const at = (y: number, m: number, d: number, h = 0, min = 0) => new Date(y, m - 1, d, h, min);
const dg = (made: Date | string, status: Digest["status"] = "done"): Digest => ({
  id: 1, period: "day", period_start: "2026-11-02", scope: { category: "github" }, body: "b", item_ids: [], provider: "p",
  status, attempts: 0, created_at: typeof made === "string" ? made : made.toISOString(), last_error: null,
});
const now = at(2026, 11, 2, 18);
const heading = (start: string, ds: (Digest | null)[]) => editionHeading(editionOf(start, ds)!, now);

describe("editionOf", () => {
  it("names a digest made during the day its evening edition", () => {
    // Review Focus 3
    expect(heading("2026-11-02", [dg(at(2026, 11, 2, 17, 3))])).toEqual({ title: "11月2日 夕刊", range: "11月2日 17時3分までの記事" });
    expect(heading("2026-11-02", [dg(at(2026, 11, 2, 17))]).range).toBe("11月2日 17時までの記事");
    expect(heading("2026-11-02", [dg(at(2026, 11, 2, 23, 59))]).range).toBe("11月2日 23時59分までの記事");
  });

  it("names a digest made after the day its next morning edition", () => {
    expect(heading("2026-11-01", [dg(at(2026, 11, 2, 7, 5))])).toEqual({ title: "11月2日 朝刊", range: "11月1日の記事" });
    expect(editionOf("2026-11-01", [dg(at(2026, 11, 2))])?.kind).toBe("morning");
  });

  it("uses the newest finished digest and ignores the rest", () => {
    expect(editionOf("2026-11-02", [dg(at(2026, 11, 2, 17)), dg(at(2026, 11, 3, 7))])?.kind).toBe("morning");
    expect(editionOf("2026-11-02", [dg(at(2026, 11, 2, 17)), dg(at(2026, 11, 3, 7), "pending"), dg("garbage"), null])?.kind).toBe("evening");
  });

  it("has no edition without a finished readable digest", () => {
    expect(editionOf("2026-11-02", [])).toBeNull();
    expect(editionOf("2026-11-02", [null, dg(at(2026, 11, 2, 17), "failed")])).toBeNull();
    expect(editionOf("2026-11-02", [dg("garbage")])).toBeNull();
  });

  it("gives no cutoff for a time before the day", () => {
    expect(heading("2026-11-02", [dg(at(2026, 11, 1, 23))])).toEqual({ title: "11月2日 夕刊", range: "11月2日の記事" });
  });

  it("crosses month and year boundaries", () => {
    expect(heading("2026-10-31", [dg(at(2026, 11, 1, 7))])).toEqual({ title: "11月1日 朝刊", range: "10月31日の記事" });
    expect(editionHeading(editionOf("2026-12-31", [dg(at(2027, 1, 1, 7))])!, at(2027, 1, 5))).toEqual({ title: "1月1日 朝刊", range: "2026年12月31日の記事" });
  });
});
```

`app/src/lib/browse.test.ts`:

```ts
import { describe, expect, it } from "vitest";
import { browseStarts, pickStart } from "./browse";

describe("browseStarts", () => {
  it("keeps stored starts up to the current one and always adds the current one", () => {
    expect(browseStarts(["2026-10-04", "2026-10-07", "2026-10-05"], "2026-10-06")).toEqual(["2026-10-04", "2026-10-05", "2026-10-06"]);
    expect(browseStarts(["2026-10-06"], "2026-10-06")).toEqual(["2026-10-06"]);
    expect(browseStarts([], "2026-10-06")).toEqual(["2026-10-06"]);
  });
});

describe("pickStart", () => {
  const stored = ["2026-10-03", "2026-10-05"];
  it("keeps a reachable kept start", () => {
    expect(pickStart(stored, "2026-10-06", "2026-10-03")).toBe("2026-10-03");
    expect(pickStart(stored, "2026-10-06", "2026-10-06")).toBe("2026-10-06");
  });
  it("otherwise picks the current start when it has a digest, else the newest earlier one, else the current one", () => {
    expect(pickStart([...stored, "2026-10-06"], "2026-10-06", null)).toBe("2026-10-06");
    expect(pickStart(stored, "2026-10-06", null)).toBe("2026-10-05");
    expect(pickStart(stored, "2026-10-06", "2026-10-04")).toBe("2026-10-05");
    expect(pickStart(stored, "2026-10-06", "2026-10-09")).toBe("2026-10-05");
    expect(pickStart([], "2026-10-06", null)).toBe("2026-10-06");
  });
});
```

In `app/src/lib/dates.test.ts`, add `parseLocalDate` to the import and:

```ts
  it("words a shown period relative to now", () => {
    const now = new Date(2026, 9, 6, 9, 0);
    expect(periodHeading("day", new Date(2026, 9, 5), now).title).toBe("昨日のダイジェスト");
    expect(periodHeading("day", new Date(2026, 9, 1), now)).toEqual({ title: "10月1日のダイジェスト", range: "2026年10月1日(木)" });
    expect(periodHeading("week", new Date(2026, 8, 28), now).title).toBe("先週のダイジェスト");
    expect(periodHeading("week", new Date(2026, 8, 21), now).title).toBe("9月21日の週");
    expect(periodHeading("month", new Date(2026, 8, 1), now).title).toBe("先月のダイジェスト");
    expect(periodHeading("month", new Date(2025, 11, 1), now).title).toBe("2025年12月のダイジェスト");
    expect(periodHeading("day", new Date(2025, 11, 31), now).title).toBe("2025年12月31日のダイジェスト");
  });

  it("parses YYYY-MM-DD as local midnight", () => {
    expect(parseLocalDate("2026-10-05")).toEqual(new Date(2026, 9, 5));
  });
```

The existing `periodHeading` assertions (two arguments) stay unchanged.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cd app && npx vitest run src/lib`
Expected: FAIL (`Failed to resolve import "./editions"`, `"./browse"`, and `parseLocalDate is not a function`).

- [ ] **Step 3: Implement**

`app/src/lib/editions.ts`:

```ts
import type { Digest } from "../types";

/** 日次要約を新聞の版として見たもの。issue は版の名前の日 (朝刊は D+1、夕刊は D)、articles は記事の日 (D)。 */
export type Edition =
  | { kind: "morning"; issue: Date; articles: Date }
  | { kind: "evening"; issue: Date; articles: Date; cutoff: Date | null };

/**
 * `start` (YYYY-MM-DD) の日次要約の版。完了した要約の created_at の最新で決める。
 * D のうちなら D の夕刊、D が終わった後なら D+1 の朝刊 (D の確定版)。完了した要約がない、
 * または時刻を読めないときは null。
 */
export function editionOf(start: string, digests: (Digest | null)[]): Edition | null {
  const [y, m, d] = start.split("-").map(Number);
  const day = new Date(y, m - 1, d);
  const next = new Date(y, m - 1, d + 1);
  let newest: Date | null = null;
  for (const g of digests) {
    if (!g || g.status !== "done") continue;
    const t = new Date(g.created_at);
    if (Number.isNaN(t.getTime())) continue;
    if (!newest || t > newest) newest = t;
  }
  if (!newest) return null;
  if (newest >= next) return { kind: "morning", issue: next, articles: day };
  return { kind: "evening", issue: day, articles: day, cutoff: newest < day ? null : newest };
}

/** 「11月2日 朝刊」/「11月1日の記事」、「11月2日 夕刊」/「11月2日 17時までの記事」。年は今と違うときだけ付ける。 */
export function editionHeading(e: Edition, now: Date): { title: string; range: string } {
  const md = (d: Date) => `${d.getFullYear() === now.getFullYear() ? "" : `${d.getFullYear()}年`}${d.getMonth() + 1}月${d.getDate()}日`;
  if (e.kind === "morning") return { title: `${md(e.issue)} 朝刊`, range: `${md(e.articles)}の記事` };
  const c = e.cutoff;
  const until = c ? ` ${c.getHours()}時${c.getMinutes() === 0 ? "" : `${c.getMinutes()}分`}までの記事` : "の記事";
  return { title: `${md(e.issue)} 夕刊`, range: `${md(e.articles)}${until}` };
}
```

`app/src/lib/browse.ts`:

```ts
/** 前後に移動できる期間の開始 (古い順)。保存済みのうち current 以前のものに、current を必ず足す (手で生成できるように)。 */
export function browseStarts(stored: string[], current: string): string[] {
  return [...new Set([...stored.filter((s) => s <= current), current])].sort();
}

/**
 * 表示する期間の開始。keep が移動できる開始ならそのまま。なければ current に要約があれば current、
 * なければ current より前の最新、それもなければ current。
 */
export function pickStart(stored: string[], current: string, keep: string | null): string {
  if (keep !== null && keep <= current && (keep === current || stored.includes(keep))) return keep;
  if (stored.includes(current)) return current;
  const earlier = stored.filter((s) => s < current).sort();
  return earlier.length > 0 ? earlier[earlier.length - 1] : current;
}
```

`app/src/lib/dates.ts`: replace `periodHeading` and add `parseLocalDate`:

```ts
/** YYYY-MM-DD をローカルの 0:00 として読む。 */
export function parseLocalDate(s: string): Date {
  const [y, m, d] = s.split("-").map(Number);
  return new Date(y, m - 1, d);
}

/** `shown` を含む期間の見出し。`now` から見た言い方 (今日 / 昨日 / 10月1日、今週 / 先週 / 9月21日の週、今月 / 先月 / 2025年12月)。 */
export function periodHeading(period: Period, shown: Date, now: Date = shown): { title: string; range: string } {
  const start = periodStart(period, shown);
  const current = periodStart(period, now);
  const y = start.getFullYear() === current.getFullYear() ? "" : `${start.getFullYear()}年`;
  const md = `${y}${start.getMonth() + 1}月${start.getDate()}日`;
  if (period === "day") {
    const yesterday = addDays(current, -1);
    const name = start >= current ? "今日" : start.getTime() === yesterday.getTime() ? "昨日" : md;
    return { title: `${name}のダイジェスト`, range: dateJa(start, true) };
  }
  if (period === "week") {
    const last = addDays(periodEnd("week", start), -1);
    const prev = addDays(current, -7);
    const title = start >= current ? "今週のダイジェスト" : start.getTime() === prev.getTime() ? "先週のダイジェスト" : `${md}の週`;
    return { title, range: `${dateJa(start, true)} 〜 ${dateJa(last, false)}` };
  }
  const prev = new Date(current.getFullYear(), current.getMonth() - 1, 1);
  const name = start >= current ? "今月" : start.getTime() === prev.getTime() ? "先月" : `${y}${start.getMonth() + 1}月`;
  return { title: `${name}のダイジェスト`, range: `${start.getFullYear()}年${start.getMonth() + 1}月` };
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cd app && npx tsc --noEmit && npx vitest run src/lib`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add app/src/lib/editions.ts app/src/lib/editions.test.ts app/src/lib/browse.ts app/src/lib/browse.test.ts app/src/lib/dates.ts app/src/lib/dates.test.ts
git commit -m "feat(app): add digest edition, browsing and relative heading helpers"
```

### Task 9: Mac DigestView: edition headings, prev/next, shown date everywhere

**PR:** 3

**Files:**
- Modify: `app/src/views/DigestView.tsx`
- Test: `app/src/views/DigestView.test.tsx`, `app/src/App.test.tsx` (default handler only)
- Modify: `docs/04-digests.md`, `docs/04-digests.ja.md` (§1 Periods)

**Interfaces:**
- Consumes: `api.listDigestStarts` (Task 7); `editionOf`, `editionHeading`, `browseStarts`, `pickStart`, `periodHeading(period, shown, now)`, `parseLocalDate` (Task 8).
- Produces: nothing used later.

- [ ] **Step 1: Write the failing tests**

In `app/src/views/DigestView.test.tsx`:

1. The test wrapper becomes `const setupIpc = (handlers: IpcHandlers) => baseSetupIpc({ digest_stats: () => EMPTY_STATS, list_digest_starts: () => [], ...handlers });`.
2. In `digest()`, the default `created_at` becomes `new Date(2026, 9, 6, 8, 0).toISOString()` (a 夕刊 of today at 08:00 under the fake 09:00 clock, in any time zone).
3. Update expectations that change because the heading is now an edition and requests use the shown period start:
   - 'renders sections per state and the heading for today': `"今日のダイジェスト"` → `"10月6日 夕刊"`, and `"2026年10月6日(火)"` → `"10月6日 8時までの記事"`.
   - 'filters to one category when a category is selected and reports errors' and 'refetches digests when digests-changed fires': `"今日のダイジェスト"` → `"10月6日 夕刊"`.
   - 'switches period and scope and shows generating', 'refetches the overview for a new period and on items-changed', 'reports the period on mount and when it changes': the week `date` `"2026-10-06"` → `"2026-10-05"` (the week's start).
4. Add:

```tsx
const editionViews = (date: string): DV[] => {
  const [y, m, d] = date.split("-").map(Number);
  // 今日 (10/6) は 8 時の夕刊、前の日は翌朝 7 時の朝刊。
  const made = date === "2026-10-06" ? new Date(y, m - 1, d, 8) : new Date(y, m - 1, d + 1, 7);
  return [{ scope: { category: "github" }, label: "GitHub", digest: digest({ period_start: date, created_at: made.toISOString(), body: `- ${date}の要点。` }), item_count: 2 }];
};

describe("DigestView editions", () => {
  it("shows today's evening edition and steps back to a morning edition", async () => {
    // Review Focus 5
    const onAskChat = vi.fn();
    const onPeriodChange = vi.fn();
    const ipc = setupIpc({
      list_digest_starts: () => ["2026-10-04", "2026-10-05", "2026-10-06"],
      get_digests: (a) => editionViews(a.date), get_item: () => null, regenerate_digest: () => undefined,
    });
    renderAll(status, { onAskChat, onPeriodChange });
    expect(await screen.findByRole("heading", { name: "10月6日 夕刊" })).toBeInTheDocument();
    expect(screen.getByText("10月6日 8時までの記事")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "次へ" })).toBeDisabled();
    expect(ipc.last("list_digest_starts")).toEqual({ period: "day", topicId: null });

    await userEvent.click(screen.getByRole("button", { name: "前へ" }));
    expect(await screen.findByRole("heading", { name: "10月6日 朝刊" })).toBeInTheDocument();
    expect(screen.getByText("10月5日の記事")).toBeInTheDocument();
    expect(await screen.findByText("2026-10-05の要点。")).toBeInTheDocument();
    expect(ipc.last("get_digests")).toEqual({ period: "day", date: "2026-10-05", topicId: null });
    await waitFor(() => expect(ipc.last("digest_stats")).toEqual({ period: "day", date: "2026-10-05", topicId: null }));
    expect(onPeriodChange).toHaveBeenLastCalledWith({ view: "digest", period: "day", date: "2026-10-05" });
    await userEvent.click(screen.getByRole("button", { name: "再生成" }));
    expect(ipc.last("regenerate_digest")).toEqual({ period: "day", date: "2026-10-05", scopes: [{ category: "github" }] });
    await userEvent.click(screen.getByRole("button", { name: "GitHubの要約についてチャットで質問" }));
    expect(onAskChat).toHaveBeenCalledWith({ type: "digest", period: "day", date: "2026-10-05", scope: "category:github", label: "GitHub" });

    await userEvent.click(screen.getByRole("button", { name: "前へ" }));
    expect(await screen.findByRole("heading", { name: "10月5日 朝刊" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "前へ" })).toBeDisabled();
  });

  it("opens on the newest earlier edition when today has none, and today stays reachable", async () => {
    const ipc = setupIpc({
      list_digest_starts: () => ["2026-10-05"],
      get_digests: (a) => a.date === "2026-10-05" ? editionViews(a.date) : [{ scope: { category: "github" }, label: "GitHub", digest: null, item_count: 3 }],
      get_item: () => null,
    });
    renderAll();
    expect(await screen.findByRole("heading", { name: "10月6日 朝刊" })).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "次へ" }));
    expect(await screen.findByRole("heading", { name: "今日のダイジェスト" })).toBeInTheDocument();
    expect(screen.getByText("2026年10月6日(火)")).toBeInTheDocument();
    expect(await screen.findByRole("button", { name: "生成" })).toBeInTheDocument();
    expect(ipc.last("get_digests")).toEqual({ period: "day", date: "2026-10-06", topicId: null });
  });

  it("browses stored weeks and asks for the shown week", async () => {
    const ipc = setupIpc({
      list_digest_starts: (a) => a.period === "week" ? ["2026-09-21", "2026-09-28", "2026-10-05"] : [],
      get_digests: () => fresh(views), get_item: () => null,
    });
    renderAll();
    await screen.findByText("一つ目。");
    await userEvent.click(screen.getByRole("button", { name: "週" }));
    expect(await screen.findByRole("heading", { name: "今週のダイジェスト" })).toBeInTheDocument();
    await waitFor(() => expect(screen.getByRole("button", { name: "前へ" })).toBeEnabled());
    await userEvent.click(screen.getByRole("button", { name: "前へ" }));
    expect(await screen.findByRole("heading", { name: "先週のダイジェスト" })).toBeInTheDocument();
    expect(screen.getByText("2026年9月28日(月) 〜 10月4日(日)")).toBeInTheDocument();
    await waitFor(() => expect(ipc.last("get_digests")).toEqual({ period: "week", date: "2026-09-28", topicId: null }));
    await userEvent.click(screen.getByRole("button", { name: "前へ" }));
    expect(await screen.findByRole("heading", { name: "9月21日の週" })).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "今日" }));
    expect(await screen.findByRole("heading", { name: "10月6日 夕刊" })).toBeInTheDocument();
  });

  it("follows a new edition until the reader steps", async () => {
    // Review Focus 4
    let stored = ["2026-10-05"];
    setupIpc({ list_digest_starts: () => stored, get_digests: (a) => editionViews(a.date), get_item: () => null });
    renderAll();
    expect(await screen.findByRole("heading", { name: "10月6日 朝刊" })).toBeInTheDocument();
    stored = ["2026-10-05", "2026-10-06"];
    await emitEvent(EVENTS.digests);
    expect(await screen.findByRole("heading", { name: "10月6日 夕刊" })).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "前へ" }));
    expect(await screen.findByRole("heading", { name: "10月6日 朝刊" })).toBeInTheDocument();
    await emitEvent(EVENTS.digests);
    await settle();
    expect(screen.getByRole("heading", { name: "10月6日 朝刊" })).toBeInTheDocument();
  });

  it("reports a failure to list starts and still shows today", async () => {
    setupIpc({ list_digest_starts: () => { throw { message: "db locked" }; }, get_digests: () => fresh(views), get_item: () => null });
    renderAll();
    expect(await screen.findByText("db locked")).toBeInTheDocument();
    expect(await screen.findByText("一つ目。")).toBeInTheDocument();
  });
});
```

In `app/src/App.test.tsx` `handlers()`, add `list_digest_starts: () => [],` after `get_digests: () => [],`.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cd app && npx vitest run src/views/DigestView.test.tsx`
Expected: FAIL (no 「前へ」 button, heading still 「今日のダイジェスト」, week requests still send `2026-10-06`).

- [ ] **Step 3: Implement**

In `app/src/views/DigestView.tsx`:

- Imports: add `ChevronLeft, ChevronRight` to the `lucide-react` import; replace the dates import with `import { localDateString, parseLocalDate, periodHeading, periodStart } from "../lib/dates";`; add `import { editionHeading, editionOf } from "../lib/editions";` and `import { browseStarts, pickStart } from "../lib/browse";`.
- State and derived values, replacing the block from `const today = new Date();` to `const requested = ...`:

```tsx
  // 保存済みの期間の開始 (古い順)。どの条件のものかを一緒に持つ。
  const [starts, setStarts] = useState<{ key: string; list: string[] } | null>(null);
  const [startsError, setStartsError] = useState<string | null>(null);
  // 矢印で選んだ開始。null か別の期間のものなら、最新の版に追従する。
  const [pinned, setPinned] = useState<{ period: Period; start: string } | null>(null);
  const latestStarts = useRef(0);
  const today = new Date();
  const current = localDateString(periodStart(period, today));
  const topicId = selection.kind === "topic" ? selection.topicId : null;
  const startsKey = `${period}|${topicId ?? ""}|${current}`;
  const stored = starts && starts.key === startsKey ? starts.list : null;
  const keep = pinned && pinned.period === period ? pinned.start : null;
  // 保存済みの開始が届くまでは取らない (今日を取ってから前の日に切り替えないため)。
  const date = stored === null ? null : pickStart(stored, current, keep);
  const walk = stored === null ? [] : browseStarts(stored, current);
  const index = date === null ? -1 : walk.indexOf(date);
  // 親が毎回新しい関数を渡しても、期間か日付が変わったときだけ通知する。
  const periodChange = useRef(onPeriodChange);
  periodChange.current = onPeriodChange;
  useEffect(() => { if (date !== null) periodChange.current({ view: "digest", period, date }); }, [period, date]);
  const loadKey = `${period}|${date ?? ""}|${topicId ?? ""}`;
  const context = `${loadKey}|${selectionKey(selection)}`;
  const views = loaded && loaded.key === loadKey ? loaded.views : null;
  const requested = requests.context === context ? requests.keys : NO_KEYS;
```

- `load` and `loadStats`: start each with `if (date === null) return;` (inside the callback, before `++latestLoad.current` / `++latestStats.current`). Their dependency arrays already contain `date`.
- Add the starts loader next to them:

```tsx
  /** 保存済みの期間の開始。失敗したら今の期間だけで続ける。 */
  const loadStarts = useCallback(() => {
    const mine = ++latestStarts.current;
    api.listDigestStarts(period, topicId)
      .then((list) => {
        if (latestStarts.current !== mine) return;
        setStarts({ key: startsKey, list });
        setStartsError(null);
      })
      .catch((e) => {
        if (latestStarts.current !== mine) return;
        setStarts({ key: startsKey, list: [] });
        setStartsError(errorMessage(e));
      });
  }, [period, topicId, startsKey]);

  useEffect(() => {
    loadStarts();
    return () => { latestStarts.current++; };
  }, [loadStarts, refreshTick]);
```

- The digests event also refreshes the starts: `useTauriEvent(EVENTS.digests, () => { loadStarts(); load(true); });`.
- `request`: `if (scopes.length === 0 || date === null) return;` and keep `api.regenerateDigest(period, date, scopes)`.
- Step and heading, replacing `const heading = periodHeading(period, today);`:

```tsx
  const step = (delta: number) => {
    const next = walk[index + delta];
    if (index >= 0 && next !== undefined) setPinned({ period, start: next });
  };
  // 今日のタブでは、保存済みの日次要約を版として見せる (「11月2日 夕刊」)。
  const edition = period === "day" && date !== null && views !== null ? editionOf(date, views.map((v) => v.digest)) : null;
  const heading = edition ? editionHeading(edition, today) : periodHeading(period, date === null ? today : parseLocalDate(date), today);
```

- The period segmented control resets the pin: `onChange={(p) => { setPeriod(p); setPinned(null); }}`, and the `periodDay`/`periodWeek`/`periodMonth` actions call `setPinned(null)` as well.
- Header: `<ErrorLine message={startsError} />` after `<ErrorLine message={error} />`, and

```tsx
        <div className="view-header">
          <button className="btn btn-ghost" aria-label="前へ" disabled={index <= 0} onClick={() => step(-1)}><ChevronLeft size={14} /></button>
          <h1>{heading.title}</h1>
          <span className="text-secondary">{heading.range}</span>
          <button className="btn btn-ghost" aria-label="次へ" disabled={index < 0 || index >= walk.length - 1} onClick={() => step(1)}><ChevronRight size={14} /></button>
          {scopeNote && <span className="text-secondary">· {scopeNote}</span>}
```

- The chat button's attachment uses `date: date ?? current`.

Docs: `docs/04-digests.md` §1, replace "The screen always shows the current period, and the heading shows its dates." with "Today opens on the newest edition: today's digest made in the evening is the evening edition (「11月2日 夕刊」, with the time it covers, for example 「11月2日 17時までの記事」); after the day ends it is rebuilt from the whole day and becomes the next day's morning edition (「11月3日 朝刊」, 「11月2日の記事」). If today has no digest yet, the newest earlier edition is shown. The arrows beside the heading step through earlier days, weeks or months that have a digest; the current period is always reachable, so you can generate it. Until you step, the screen moves to a new edition when one is made or synced. The overview, Regenerate and chat use the period shown." `docs/04-digests.ja.md` §1, replace 「画面にはいつも現在の期間が出て、見出しにその日付が出ます。」 with 「今日のタブは一番新しい版から開きます。夕方に作られた今日の要約は夕刊です (「11月2日 夕刊」。対象の時刻も出ます。例: 「11月2日 17時までの記事」)。日が終わると一日分で作り直され、翌日の朝刊になります (「11月3日 朝刊」、「11月2日の記事」)。今日の要約がまだなければ、その前の一番新しい版が出ます。見出しの横の矢印で、要約のある前の日、週、月に移れます。今の期間にはいつでも移れるので、そこで生成できます。矢印で動かすまでは、新しい版ができたり同期で届いたりするとそちらに切り替わります。概観、再生成、チャットは表示中の期間を使います。」

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cd app && npx tsc --noEmit && npx vitest run`
Expected: PASS. If another existing DigestView test fails, the only acceptable causes are the heading text (today with a finished digest is now 「10月6日 夕刊」) or the week date (`2026-10-05`); change only that expectation, not the behaviour.

Run: `cargo test -p infodesk-app && cargo clippy --workspace --all-targets -- -D warnings`
Expected: PASS, no warnings.

- [ ] **Step 5: Commit**

```bash
git add app/src/views/DigestView.tsx app/src/views/DigestView.test.tsx app/src/App.test.tsx docs/04-digests.md docs/04-digests.ja.md
git commit -m "feat(app): show digest editions and browse earlier days, weeks and months"
```
