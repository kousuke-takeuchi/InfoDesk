# InfoDesk on the Home k3s Cluster Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Run InfoDesk with no GUI on the user's home k3s cluster as the only writer: an Argo CronWorkflow runs `infodesk tick` every 5 minutes, which collects, summarizes (through the Claude API), makes digests and syncs to the NAS by the same schedule rules as the Mac app.

**Architecture:** The pure schedule rules move from the Tauri crate to `infodesk-core::schedule` so the app and the CLI share them. A new LLM provider `claude_api` calls `POST /v1/messages` with `reqwest`. Shared NAS setting keys and messages move to `infodesk-core::sync::nas`; a new `sync::headless` runs one NAS sync straight over the LAN (private IPv4 URLs only) with the password from the environment. The CLI gains `sync`, `tick` (one evaluation, state kept in the database) and an OS-aware default database path. A Dockerfile and `deploy/k8s/` manifests (WorkflowTemplate, CronWorkflow, PVC, RBAC) run it on the NUC node.

**Tech Stack:** Rust 2024 workspace (`infodesk-core`, `infodesk` CLI, Tauri app), tokio, reqwest 0.13 (rustls), rusqlite, wiremock (tests); Docker (`rust:1.97-bookworm`, `debian:bookworm-slim`), k3s, Argo Workflows v4.0.6.

**Spec:** `docs/superpowers/specs/2026-11-01-infodesk-headless-server-design.md`

## Global Constraints

- The Mac app must behave exactly as before. Every existing test in `app/src-tauri`, `crates/core` and `crates/cli` keeps passing.
- Claude API request: `anthropic-version: 2026-10-21`, `anthropic-beta: server-side-fallback-2026-07-01`, body `fallbacks: "default"`, `max_tokens: 16000`, no `temperature` / `top_p` / `top_k`, no `thinking` field.
- Default model `claude-sonnet-5-5`. Default effort: `claude_api.items_effort = low`, `claude_api.digest_effort = ""` (API default). Allowed effort values: `low`, `medium`, `high`, `xhigh`, `max`, empty.
- The API key comes only from `ANTHROPIC_API_KEY`; the NAS password only from `INFODESK_NAS_PASSWORD`. Neither is stored in the database, printed, or included in a `Debug` output or an error message.
- Sync data goes only to an `http://` URL whose host is a private IPv4 literal (10/8, 172.16/12, 192.168/16). The NAS is `http://192.168.0.151:5005/infodesk/`.
- Every InfoDesk pod: namespace `infodesk`, `nodeSelector kubernetes.io/hostname: takeuchi-nuc11atkc2`, PVC `infodesk-data`, Secret `infodesk-env` via `envFrom`, `TZ=Asia/Tokyo`. Images `registry.umafolio.site/infodesk:<git short sha>`, never `latest`.
- Nothing in git, the image or a manifest holds a real secret. Never print `/etc/rancher/k3s/registries.yaml` or Secret values.
- User-facing messages reuse the app's existing Japanese strings exactly (e.g. 「NAS の URL、ユーザー名、パスワードを入力してください」).
- Code comments and CLI help in Japanese, as in the surrounding code. Docs under `docs/` in English with a `.ja.md` twin; `docs/superpowers/` in English only.
- Commits: Conventional Commits, no `Co-Authored-By` or any other AI attribution trailer.
- Run from the worktree root `~/Documents/InfoDesk-wt-server`. Before each commit: `cargo fmt --all` and `cargo clippy --workspace --all-targets -- -D warnings`.

## Review Focus

1. **Copied Mac database on the first `tick`.** The copied DB already has `schedule.last_*` keys and `sync.device_id`. Expected: the cluster continues from those (no backfill storm, no new device id). Pinned in Task 5 (`a_copied_database_continues_its_schedule`).
2. **`ANTHROPIC_API_KEY` missing while `claude_api` is selected.** Expected: one clear log line per attempt (`ANTHROPIC_API_KEY is not set`), collection still runs, digests stay pending and are retried. Pinned in Task 2 (`missing_key_is_a_clear_error`) and Task 5 (`a_missing_provider_does_not_stop_collection`).
3. **The copied Mac settings still hold `http://takeuchi-nas:5005/infodesk/`.** Pods cannot resolve that name. Expected: refused with the LAN-URL message and nothing sent, until the runbook sets the IP form. Pinned in Task 3 (`a_name_from_the_mac_settings_sends_nothing`).
4. **A public address, a name, `https://` or IPv6 as the NAS URL.** Expected: refused, nothing sent. Pinned in Task 3 (`names_public_addresses_https_and_ipv6_are_refused`).
5. **The API answers 401, or the body mentions the key.** Expected: the error text never contains the key. Pinned in Task 2 (`rejected_key_is_not_echoed`).

---

## File Structure

| File | Responsibility |
|---|---|
| `crates/core/src/schedule.rs` (new) | Pure schedule rules shared by app and CLI |
| `crates/core/src/llm/claude_api.rs` (new) | Claude API provider |
| `crates/core/src/llm/mod.rs` | Provider selection gains `claude_api` |
| `crates/core/src/seed.rs` | Default `claude_api.*` settings |
| `crates/core/src/sync/nas.rs` (new) | Shared NAS setting keys and messages, the LAN URL rule |
| `crates/core/src/sync/headless.rs` (new) | One NAS sync without the app |
| `app/src-tauri/src/scheduler.rs`, `app/src-tauri/src/jobs.rs` | Use `infodesk_core::schedule` |
| `app/src-tauri/src/sync/mod.rs` | Re-exports the moved NAS keys and messages |
| `crates/cli/src/lib.rs` | OS-aware default path, `sync` and `tick` commands |
| `crates/cli/src/tick.rs` (new) | One evaluation of the schedule and its job flow |
| `Dockerfile`, `.dockerignore` (new) | amd64 image of the CLI |
| `deploy/k8s/*` (new) | Namespace, PVC, RBAC, WorkflowTemplate, CronWorkflow, manual run, shell pod, Secret example, build script |
| `docs/15-home-server*.md` (new); `docs/07-settings*.md`, `docs/08-cli*.md`, `docs/14-mobile-app*.md`, `README*.md` | User docs |

---

### Task 1: Shared schedule rules in core

**Files:**
- Create: `crates/core/src/schedule.rs`
- Modify: `crates/core/src/lib.rs`, `app/src-tauri/src/scheduler.rs`, `app/src-tauri/src/jobs.rs:236-250`, `app/src-tauri/src/sync/mod.rs:819-825`

**Interfaces:**
- Produces (`infodesk_core::schedule`): `ScheduleConfig { collect_interval, daily_time, weekly_day, weekly_time, monthly_time }` + `ScheduleConfig::from_settings(get: impl Fn(&str) -> Option<String>) -> Self`; `LastRuns { collect_at, daily_for, weekly_for, monthly_for }` + `LastRuns::from_settings(get: impl Fn(&str) -> Option<String>) -> Self`; `enum Job { Collect, Daily(NaiveDate), Weekly(NaiveDate), Monthly(NaiveDate) }`; `next_collect_at`, `due_jobs`, `filter_recent`, `retry_due`, `job_plan` (same signatures as today in `scheduler.rs`); `pub const RECENT_ATTEMPT: chrono::Duration`; `pub const TICK: std::time::Duration` (15 s); `pub const RETRY_AFTER_ALL_FAILED: chrono::Duration` (10 min); `pub fn all_failed_retry_base(now: DateTime<Utc>, interval: Duration) -> DateTime<Utc>`; `pub fn last_run_record(job: &Job) -> Option<(&'static str, String)>`; `pub fn should_record(job: &Job, generation_failed: bool) -> bool`.

- [ ] **Step 1: Create `crates/core/src/schedule.rs` by moving code**

Move from `app/src-tauri/src/scheduler.rs`, unchanged: the `use` lines it needs, `ScheduleConfig`, `LastRuns`, `Job`, `parse_time`, `parse_weekday`, `impl ScheduleConfig`, `next_collect_at`, `due_jobs`, `RECENT_ATTEMPT`, `filter_recent`, `retry_due`, `previous_month`, `job_plan`. Replace `infodesk_core::` with `crate::` in the moved imports. Make `TICK` public and move it too:

```rust
/// 定期実行の評価間隔。アプリとサーバーで同じ。
pub const TICK: std::time::Duration = std::time::Duration::from_secs(15);
```

Move the pure tests from the `#[cfg(test)] mod tests` of `scheduler.rs` into a `#[cfg(test)] mod tests` at the end of `schedule.rs`, with the helpers they use (`jst`, `d`, `cfg`): `config_defaults_and_tolerates_garbage`, `first_start_collects_and_backfills_previous_periods`, `collect_is_due_after_interval_only`, `daily_runs_once_after_its_time_even_if_late`, `weekly_and_monthly_target_previous_periods`, `job_plan_lists_digest_runs_in_order`, `filter_recent_skips_jobs_attempted_within_15_minutes`, `collect_recent_window_is_capped_by_the_interval`, `pending_retry_runs_at_most_once_per_window`, `minutes_setting_wins_over_hours_and_falls_back`, `monthly_waits_for_its_time_on_the_first_and_rolls_over_the_year`, `weekly_on_sunday_waits_for_weekly_time`, `jobs_come_in_dependency_order`. Leave in the app the tests that use `AppState` or `run_job` (`failed_weekly_run_is_not_recorded`, `rejected_collect_does_not_set_last_error`, the mock provider and `state_with_daily_in_previous_week`).

Add `pub mod schedule;` to `crates/core/src/lib.rs` (keep the list alphabetical: after `pub mod page;`/`pub mod period;`, before `pub mod seed;`).

- [ ] **Step 2: Write failing tests for the new helpers** (append inside `schedule.rs` tests)

```rust
    #[test]
    fn last_runs_from_settings_parses_and_ignores_garbage() {
        let r = LastRuns::from_settings(|k| {
            Some(
                match k {
                    "schedule.last_collect_at" => "2026-10-06T07:00:00+09:00",
                    "schedule.last_daily_for" => "2026-10-05",
                    "schedule.last_weekly_for" => "garbage",
                    _ => return None,
                }
                .to_string(),
            )
        });
        assert_eq!(
            r.collect_at,
            Some(jst("2026-10-06T07:00:00+09:00").with_timezone(&Utc))
        );
        assert_eq!(r.daily_for, Some(d("2026-10-05")));
        assert_eq!(r.weekly_for, None);
        assert_eq!(r.monthly_for, None);
    }

    #[test]
    fn finished_jobs_record_their_key_and_failed_weekly_or_monthly_do_not() {
        assert_eq!(last_run_record(&Job::Collect), None);
        assert_eq!(
            last_run_record(&Job::Daily(d("2026-10-05"))),
            Some(("schedule.last_daily_for", "2026-10-05".to_string()))
        );
        assert_eq!(
            last_run_record(&Job::Weekly(d("2026-09-28"))),
            Some(("schedule.last_weekly_for", "2026-09-28".to_string()))
        );
        assert_eq!(
            last_run_record(&Job::Monthly(d("2026-09-01"))),
            Some(("schedule.last_monthly_for", "2026-09-01".to_string()))
        );
        assert!(should_record(&Job::Daily(d("2026-10-05")), true));
        assert!(!should_record(&Job::Weekly(d("2026-09-28")), true));
        assert!(!should_record(&Job::Monthly(d("2026-09-01")), true));
        assert!(should_record(&Job::Weekly(d("2026-09-28")), false));
    }

    #[test]
    fn all_failed_retry_comes_ten_minutes_later_or_after_a_shorter_interval() {
        let now = jst("2026-10-06T07:00:00+09:00").with_timezone(&Utc);
        // 3時間の間隔: 次の収集は10分後。
        let base = all_failed_retry_base(now, Duration::hours(3));
        assert_eq!(base + Duration::hours(3), now + Duration::minutes(10));
        // 5分の間隔: 間隔どおり。未来にはならない。
        let base = all_failed_retry_base(now, Duration::minutes(5));
        assert_eq!(base, now);
    }
```

- [ ] **Step 3: Run to see them fail**

Run: `cargo test -p infodesk-core schedule::`
Expected: compile error, `LastRuns::from_settings`, `last_run_record`, `should_record`, `all_failed_retry_base` not found.

- [ ] **Step 4: Implement the helpers** (in `schedule.rs`, after `job_plan`)

```rust
impl LastRuns {
    /// `schedule.last_*` の設定から読む。壊れた値は未実行として扱う。
    pub fn from_settings(get: impl Fn(&str) -> Option<String>) -> Self {
        let date = |k: &str| get(k).and_then(|v| NaiveDate::parse_from_str(&v, "%Y-%m-%d").ok());
        Self {
            collect_at: get("schedule.last_collect_at")
                .and_then(|v| DateTime::parse_from_rfc3339(&v).ok())
                .map(|d| d.with_timezone(&Utc)),
            daily_for: date("schedule.last_daily_for"),
            weekly_for: date("schedule.last_weekly_for"),
            monthly_for: date("schedule.last_monthly_for"),
        }
    }
}

/// 要約のジョブが終わったときに書く設定 (キー、値)。収集は `None`。
pub fn last_run_record(job: &Job) -> Option<(&'static str, String)> {
    match job {
        Job::Collect => None,
        Job::Daily(day) => Some(("schedule.last_daily_for", day.to_string())),
        Job::Weekly(start) => Some(("schedule.last_weekly_for", start.to_string())),
        Job::Monthly(start) => Some(("schedule.last_monthly_for", start.to_string())),
    }
}

/// ジョブを実行済みとして記録するか。週次と月次は作り直し (force) なので保留の再試行では
/// 補えない。1件でも生成に失敗したら記録せず、次の評価で再実行する。日次の失敗は保留の再試行で補う。
pub fn should_record(job: &Job, generation_failed: bool) -> bool {
    !(matches!(job, Job::Weekly(_) | Job::Monthly(_)) && generation_failed)
}

/// 全ソースが失敗したとき、次の収集までの待ち時間。
pub const RETRY_AFTER_ALL_FAILED: Duration = Duration::minutes(10);

/// 全ソースが失敗したときに `schedule.last_collect_at` に書く時刻。約10分後 (間隔がそれより
/// 短ければ間隔どおり) に次の収集が来るよう前にずらす。未来にはならない。
pub fn all_failed_retry_base(now: DateTime<Utc>, interval: Duration) -> DateTime<Utc> {
    (now - interval + RETRY_AFTER_ALL_FAILED.min(interval)).min(now)
}
```

- [ ] **Step 5: Point the app at core**

In `app/src-tauri/src/scheduler.rs`, delete the moved items and add at the top:

```rust
pub use infodesk_core::schedule::{
    Job, LastRuns, RECENT_ATTEMPT, ScheduleConfig, TICK, due_jobs, filter_recent, job_plan,
    last_run_record, next_collect_at, retry_due, should_record,
};
```

Replace `read_last_runs` body with:

```rust
fn read_last_runs(state: &AppState) -> LastRuns {
    LastRuns::from_settings(|k| state.with_store(|s| s.get_setting(k)).ok().flatten())
}
```

In `run_job`, replace the block from `let generation_failed = ...` to `record(state, key, &value.to_string());` with:

```rust
    let generation_failed = outcomes
        .iter()
        .any(|(_, o)| o.as_ref().is_some_and(|o| o.error.is_some()));
    if !should_record(job, generation_failed) {
        return;
    }
    if let Some((key, value)) = last_run_record(job) {
        record(state, key, &value);
    }
```

In `app/src-tauri/src/jobs.rs`, delete `const RETRY_AFTER_ALL_FAILED` and replace the `retry_base` line with:

```rust
        let retry_base = infodesk_core::schedule::all_failed_retry_base(now, interval);
```

`crate::scheduler::ScheduleConfig` keeps resolving through the re-export (used in `jobs.rs` and `sync/mod.rs`). Remove now-unused imports that clippy reports.

- [ ] **Step 6: Run all tests**

Run: `cargo test -p infodesk-core schedule:: && cargo test -p infodesk-app`
Expected: PASS, including the moved tests and the app's `failed_weekly_run_is_not_recorded`.

- [ ] **Step 7: Commit**

```bash
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings
git add crates/core/src/schedule.rs crates/core/src/lib.rs app/src-tauri/src/scheduler.rs app/src-tauri/src/jobs.rs
git commit -m "refactor: move the schedule rules to core so a headless server can share them"
```

---

### Task 2: Claude API provider

**Files:**
- Create: `crates/core/src/llm/claude_api.rs`
- Modify: `crates/core/src/llm/mod.rs`, `crates/core/src/seed.rs:130-147`

**Interfaces:**
- Consumes: `LlmProvider`, `LlmRole`, `clean_output` (`crates/core/src/llm/mod.rs`), `claude::is_valid_model_name`.
- Produces: `pub const API_KEY_ENV: &str = "ANTHROPIC_API_KEY"`; `pub struct ClaudeApiProvider`; `ClaudeApiProvider::new(api_key: String, model: &str, effort: &str) -> Result<Self>`; `.with_base_url(&str) -> Self`; `pub fn from_settings(store: &Store, role: LlmRole, api_key: Option<String>) -> Result<ClaudeApiProvider>`; `name()` returns `claude_api:<model>`. `provider_from_settings` accepts `claude_api`.

- [ ] **Step 1: Write the failing tests** (create the file with only the test module and `pub mod claude_api;` in `llm/mod.rs`)

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use wiremock::matchers::{body_partial_json, header, method, path};
    use wiremock::{Mock, MockServer, Request, ResponseTemplate};

    fn provider(server: &MockServer, effort: &str) -> ClaudeApiProvider {
        ClaudeApiProvider::new("sk-test-secret".into(), "claude-sonnet-5-5", effort)
            .unwrap()
            .with_base_url(&server.uri())
    }

    fn ok_body(content: serde_json::Value) -> serde_json::Value {
        json!({"type": "message", "stop_reason": "end_turn", "content": content})
    }

    #[tokio::test]
    async fn sends_a_messages_request_and_joins_text_blocks() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/messages"))
            .and(header("x-api-key", "sk-test-secret"))
            .and(header("anthropic-version", "2026-10-21"))
            .and(header("anthropic-beta", "server-side-fallback-2026-07-01"))
            .and(body_partial_json(json!({
                "model": "claude-sonnet-5-5",
                "max_tokens": 16000,
                "system": "sys",
                "messages": [{"role": "user", "content": "hello"}],
                "output_config": {"effort": "low"},
                "fallbacks": "default"
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(ok_body(json!([
                {"type": "thinking", "thinking": ""},
                {"type": "text", "text": "  要約"},
                {"type": "fallback", "from": {"model": "a"}, "to": {"model": "b"}},
                {"type": "text", "text": "です。\n"}
            ]))))
            .mount(&server)
            .await;
        let p = provider(&server, "low");
        assert_eq!(p.name(), "claude_api:claude-sonnet-5-5");
        assert_eq!(p.complete("sys", "hello").await.unwrap(), "要約です。");
    }

    #[tokio::test]
    async fn sends_no_sampling_thinking_or_empty_effort() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(ok_body(json!([
                {"type": "text", "text": "ok"}
            ]))))
            .mount(&server)
            .await;
        provider(&server, "").complete("s", "p").await.unwrap();
        let reqs: Vec<Request> = server.received_requests().await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&reqs[0].body).unwrap();
        for key in ["temperature", "top_p", "top_k", "thinking", "output_config"] {
            assert!(body.get(key).is_none(), "{key} must not be sent: {body}");
        }
    }

    #[tokio::test]
    async fn refusal_and_truncation_are_errors() {
        for (stop, want) in [("refusal", "refused (cyber)"), ("max_tokens", "truncated")] {
            let server = MockServer::start().await;
            Mock::given(method("POST"))
                .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                    "stop_reason": stop,
                    "stop_details": {"type": "refusal", "category": "cyber"},
                    "content": [{"type": "text", "text": "partial"}]
                })))
                .mount(&server)
                .await;
            let err = provider(&server, "low").complete("s", "p").await.unwrap_err();
            assert!(matches!(&err, Error::Llm(m) if m.contains(want)), "{err}");
        }
    }

    #[tokio::test]
    async fn rejected_key_is_not_echoed() {
        // Review Focus 5
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(401).set_body_json(json!({
                "type": "error",
                "error": {"type": "authentication_error", "message": "invalid x-api-key sk-test-secret"}
            })))
            .mount(&server)
            .await;
        let err = provider(&server, "low").complete("s", "p").await.unwrap_err();
        let text = err.to_string();
        assert!(!text.contains("sk-test-secret"), "{text}");
        assert!(text.contains("401"), "{text}");
    }

    #[tokio::test]
    async fn other_api_errors_carry_the_status_and_message() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(529).set_body_json(json!({
                "type": "error", "error": {"type": "overloaded_error", "message": "Overloaded"}
            })))
            .mount(&server)
            .await;
        let err = provider(&server, "low").complete("s", "p").await.unwrap_err();
        assert!(matches!(&err, Error::Llm(m) if m.contains("529") && m.contains("Overloaded")), "{err}");
    }

    #[test]
    fn invalid_effort_or_model_is_rejected_at_once() {
        assert!(matches!(
            ClaudeApiProvider::new("k".into(), "claude-sonnet-5-5", "lowest"),
            Err(Error::Invalid(_))
        ));
        assert!(matches!(
            ClaudeApiProvider::new("k".into(), "-bad", "low"),
            Err(Error::Invalid(_))
        ));
    }

    #[test]
    fn missing_key_is_a_clear_error() {
        // Review Focus 2
        let store = Store::open_in_memory().unwrap();
        for key in [None, Some(String::new()), Some("  ".into())] {
            let err = from_settings(&store, LlmRole::Items, key).unwrap_err();
            assert!(matches!(&err, Error::Invalid(m) if m == "ANTHROPIC_API_KEY is not set"), "{err}");
        }
    }

    #[test]
    fn effort_follows_the_role_and_debug_hides_the_key() {
        let store = Store::open_in_memory().unwrap();
        crate::seed::seed_defaults(&store).unwrap();
        let items = from_settings(&store, LlmRole::Items, Some("sk-x".into())).unwrap();
        let digest = from_settings(&store, LlmRole::Digest, Some("sk-x".into())).unwrap();
        assert_eq!(items.effort, "low");
        assert_eq!(digest.effort, "");
        assert_eq!(items.name(), "claude_api:claude-sonnet-5-5");
        assert!(!format!("{items:?}").contains("sk-x"));
    }
}
```

- [ ] **Step 2: Run to see them fail**

Run: `cargo test -p infodesk-core llm::claude_api`
Expected: compile errors (`ClaudeApiProvider`, `from_settings` missing).

- [ ] **Step 3: Implement the provider** (top of `crates/core/src/llm/claude_api.rs`)

```rust
use std::time::Duration;

use async_trait::async_trait;
use serde_json::{Value, json};

use super::claude::is_valid_model_name;
use super::{LlmProvider, LlmRole, clean_output};
use crate::error::{Error, Result};
use crate::store::Store;

/// API キーを読む環境変数。キーは DB に保存しない。
pub const API_KEY_ENV: &str = "ANTHROPIC_API_KEY";
const BASE_URL: &str = "https://api.anthropic.com";
const API_VERSION: &str = "2026-10-21";
/// 安全機能が断った依頼を、勧められたモデルでやり直す (`fallbacks: "default"`)。
const FALLBACK_BETA: &str = "server-side-fallback-2026-07-01";
const MAX_TOKENS: u32 = 16000;
const TIMEOUT: Duration = Duration::from_secs(300);
/// `output_config.effort` に使える値。空は API の既定。
pub const EFFORTS: [&str; 5] = ["low", "medium", "high", "xhigh", "max"];

pub struct ClaudeApiProvider {
    api_key: String,
    model: String,
    pub(crate) effort: String,
    base_url: String,
    http: reqwest::Client,
}

impl std::fmt::Debug for ClaudeApiProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ClaudeApiProvider")
            .field("api_key", &"<redacted>")
            .field("model", &self.model)
            .field("effort", &self.effort)
            .field("base_url", &self.base_url)
            .finish()
    }
}

impl ClaudeApiProvider {
    pub fn new(api_key: String, model: &str, effort: &str) -> Result<Self> {
        let model = model.trim();
        if !is_valid_model_name(model) {
            return Err(Error::Invalid(format!(
                "claude_api.model: invalid model name '{model}'"
            )));
        }
        let effort = effort.trim();
        if !effort.is_empty() && !EFFORTS.contains(&effort) {
            return Err(Error::Invalid(format!(
                "claude_api effort: '{effort}' is not one of {}",
                EFFORTS.join(", ")
            )));
        }
        Ok(Self {
            api_key,
            model: model.to_string(),
            effort: effort.to_string(),
            base_url: BASE_URL.to_string(),
            http: reqwest::Client::builder().timeout(TIMEOUT).build()?,
        })
    }

    /// テスト用。接続先を差し替える。
    pub fn with_base_url(mut self, base_url: &str) -> Self {
        self.base_url = base_url.trim_end_matches('/').to_string();
        self
    }
}

/// 設定と API キーから作る。用途ごとの effort は `claude_api.items_effort` / `claude_api.digest_effort`。
pub fn from_settings(
    store: &Store,
    role: LlmRole,
    api_key: Option<String>,
) -> Result<ClaudeApiProvider> {
    let api_key = api_key
        .map(|k| k.trim().to_string())
        .filter(|k| !k.is_empty())
        .ok_or_else(|| Error::Invalid(format!("{API_KEY_ENV} is not set")))?;
    let effort_key = match role {
        LlmRole::Items => "claude_api.items_effort",
        LlmRole::Digest => "claude_api.digest_effort",
    };
    ClaudeApiProvider::new(
        api_key,
        &store.get_setting_or("claude_api.model", "claude-sonnet-5-5")?,
        &store.get_setting_or(effort_key, "")?,
    )
}

/// API のエラー応答を文にする。認証の失敗では本文を使わない (キーが含まれうる)。
fn api_error(status: reqwest::StatusCode, body: &Value) -> String {
    if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
        return format!("claude_api {status}: the API key was rejected");
    }
    let message = body["error"]["message"].as_str().unwrap_or("unknown error");
    format!("claude_api {status}: {message}")
}

#[async_trait]
impl LlmProvider for ClaudeApiProvider {
    fn name(&self) -> String {
        format!("claude_api:{}", self.model)
    }

    async fn complete(&self, system: &str, prompt: &str) -> Result<String> {
        let mut body = json!({
            "model": self.model,
            "max_tokens": MAX_TOKENS,
            "system": system,
            "messages": [{"role": "user", "content": prompt}],
            "fallbacks": "default"
        });
        if !self.effort.is_empty() {
            body["output_config"] = json!({"effort": self.effort});
        }
        let resp = self
            .http
            .post(format!("{}/v1/messages", self.base_url))
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", API_VERSION)
            .header("anthropic-beta", FALLBACK_BETA)
            .json(&body)
            .send()
            .await?;
        let status = resp.status();
        let value: Value = resp
            .json()
            .await
            .map_err(|_| Error::Llm(format!("claude_api {status}: unreadable response")))?;
        if !status.is_success() {
            return Err(Error::Llm(api_error(status, &value)));
        }
        match value["stop_reason"].as_str() {
            Some("refusal") => {
                let category = value["stop_details"]["category"].as_str().unwrap_or("unknown");
                return Err(Error::Llm(format!("claude_api: refused ({category})")));
            }
            Some("max_tokens") => {
                return Err(Error::Llm("claude_api: truncated (max_tokens)".to_string()));
            }
            _ => {}
        }
        // thinking や fallback の区切りなど、text 以外のブロックは読まない。
        let text: String = value["content"]
            .as_array()
            .map(|blocks| {
                blocks
                    .iter()
                    .filter(|b| b["type"] == "text")
                    .filter_map(|b| b["text"].as_str())
                    .collect()
            })
            .unwrap_or_default();
        clean_output(&text)
    }
}
```

- [ ] **Step 4: Wire selection and defaults**

In `crates/core/src/llm/mod.rs`: add `pub mod claude_api;` (after `pub mod claude;`), update the doc comment of `provider_from_settings` to list `claude_api` and the `claude_api.*` keys, and add the arm:

```rust
        "claude_api" => Ok(Box::new(claude_api::from_settings(
            store,
            role,
            std::env::var(claude_api::API_KEY_ENV).ok(),
        )?)),
```

Add to the existing `provider_from_settings_selects_by_role` test, before the `"gpt"` case:

```rust
        store.set_setting("llm.items_provider", "claude_api").unwrap();
        // 環境変数 ANTHROPIC_API_KEY の有無で結果が変わる。どちらでも正しい形か確かめる。
        match provider_from_settings(&store, LlmRole::Items) {
            Ok(p) => assert_eq!(p.name(), "claude_api:claude-sonnet-5-5"),
            Err(e) => assert!(matches!(e, Error::Invalid(m) if m.contains("ANTHROPIC_API_KEY"))),
        }
```

In `crates/core/src/seed.rs` `DEFAULT_SETTINGS`, after `("claude.model", "sonnet"),` add:

```rust
    ("claude_api.model", "claude-sonnet-5-5"),
    ("claude_api.items_effort", "low"),
    ("claude_api.digest_effort", ""),
```

- [ ] **Step 5: Run tests**

Run: `cargo test -p infodesk-core llm:: seed::`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings
git add crates/core/src/llm crates/core/src/seed.rs
git commit -m "feat(core): a Claude API provider that calls /v1/messages with an API key from the environment"
```

---

### Task 3: NAS keys and headless NAS sync over the LAN

**Files:**
- Create: `crates/core/src/sync/nas.rs`, `crates/core/src/sync/headless.rs`
- Modify: `crates/core/src/sync/mod.rs`, `app/src-tauri/src/sync/mod.rs:19-58`

**Interfaces:**
- Consumes: `schedule::ScheduleConfig` (Task 1), `engine::{run_sync, SyncInput, SyncReport, SyncError, compaction_due}`, `webdav::{WebDavConfig, WebDavStore}`, `drive::{DriveStore, DriveError, MemoryDrive}`, `Store::ensure_sync_ids`.
- Produces (`infodesk_core::sync::nas`): constants `SETTING_ENABLED`, `SETTING_DEVICE_NAME`, `SETTING_LAST_SYNC_AT`, `SETTING_LAST_COMPACTION_AT`, `SETTING_BACKEND`, `SETTING_NAS_URL`, `SETTING_NAS_USER`, `SETTING_RESET_PENDING`, `BACKEND_GDRIVE`, `BACKEND_NAS`, `NAS_INCOMPLETE_MESSAGE`, `NAS_PREPARE_MESSAGE`, `NETWORK_MESSAGE` (values copied verbatim from `app/src-tauri/src/sync/mod.rs`), new `NAS_LAN_URL_MESSAGE`, `pub fn lan_nas_url(raw: &str) -> Result<String, &'static str>`.
- Produces (`infodesk_core::sync::headless`): `pub const NAS_PASSWORD_ENV: &str = "INFODESK_NAS_PASSWORD"`; `pub const DEFAULT_SERVER_NAME: &str = "infodesk-server"`; `pub struct HeadlessEnv { pub nas_password: Option<String>, pub tz: FixedOffset }` + `HeadlessEnv::from_env(tz) -> Self`; `pub type MakeDrive = dyn Fn(WebDavConfig) -> Result<Arc<dyn DriveStore>, DriveError> + Send + Sync`; `pub fn webdav(cfg: WebDavConfig) -> Result<Arc<dyn DriveStore>, DriveError>`; `#[derive(Debug)] pub enum SyncOutcome { Disabled, Synced(SyncReport) }`; `pub async fn sync_nas(store: &Mutex<Store>, env: &HeadlessEnv, make_drive: &MakeDrive, now: DateTime<Utc>) -> Result<SyncOutcome, String>`.

- [ ] **Step 1: Write the failing tests for the LAN rule** (create `nas.rs` with the test module; add `pub mod nas;` and `pub mod headless;` to `crates/core/src/sync/mod.rs`)

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn private_ipv4_urls_are_accepted_as_they_are() {
        for raw in [
            "http://192.168.0.151:5005/infodesk/",
            "http://10.0.0.5:5005/x/",
            "http://172.16.3.4:5005/x/",
        ] {
            assert_eq!(lan_nas_url(raw).unwrap(), raw, "{raw}");
        }
    }

    #[test]
    fn names_public_addresses_https_and_ipv6_are_refused() {
        // Review Focus 4
        for raw in [
            "http://takeuchi-nas:5005/infodesk/",
            "http://nas.example.com:5005/x/",
            "http://8.8.8.8:5005/x/",
            "http://172.32.0.1:5005/x/",
            "https://192.168.0.151:5006/infodesk/",
            "http://[fd00::5]:5005/x/",
            "not a url",
            "",
        ] {
            assert_eq!(lan_nas_url(raw), Err(NAS_LAN_URL_MESSAGE), "{raw:?}");
        }
    }
}
```

- [ ] **Step 2: Run to see them fail**

Run: `cargo test -p infodesk-core sync::nas`
Expected: compile errors.

- [ ] **Step 3: Implement `nas.rs`** (above the tests)

Copy the constants listed under Interfaces from `app/src-tauri/src/sync/mod.rs`, with their doc comments, byte for byte. Then add:

```rust
use std::net::Ipv4Addr;

/// LAN の NAS の URL が使えないとき。名前は Pod から引けず、LAN の外に出るおそれもあるので IP に限る。
pub const NAS_LAN_URL_MESSAGE: &str =
    "NAS の URL は LAN の IP アドレスで書いてください (例: http://192.168.0.151:5005/infodesk/)";

/// クラスターから LAN の NAS へ直接つなぐときの URL を確かめる。http で、ホストが
/// プライベートな IPv4 (10/8、172.16/12、192.168/16) のときだけ通す。それ以外には何も送らない。
pub fn lan_nas_url(raw: &str) -> Result<String, &'static str> {
    let url = reqwest::Url::parse(raw.trim()).map_err(|_| NAS_LAN_URL_MESSAGE)?;
    if url.scheme() != "http" {
        return Err(NAS_LAN_URL_MESSAGE);
    }
    let ip: Ipv4Addr = url
        .host_str()
        .and_then(|h| h.parse().ok())
        .ok_or(NAS_LAN_URL_MESSAGE)?;
    if !ip.is_private() {
        return Err(NAS_LAN_URL_MESSAGE);
    }
    Ok(url.to_string())
}
```

In `app/src-tauri/src/sync/mod.rs`, replace the definitions of the moved constants with one re-export and leave every app-only constant in place:

```rust
pub use infodesk_core::sync::nas::{
    BACKEND_GDRIVE, BACKEND_NAS, NAS_INCOMPLETE_MESSAGE, NAS_PREPARE_MESSAGE, NETWORK_MESSAGE,
    SETTING_BACKEND, SETTING_DEVICE_NAME, SETTING_ENABLED, SETTING_LAST_COMPACTION_AT,
    SETTING_LAST_SYNC_AT, SETTING_NAS_URL, SETTING_NAS_USER, SETTING_RESET_PENDING,
};
```

- [ ] **Step 4: Run** `cargo test -p infodesk-core sync::nas && cargo test -p infodesk-app` — Expected: PASS.

- [ ] **Step 5: Write the failing tests for `sync_nas`** (test module in `headless.rs`)

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::sync::drive::MemoryDrive;
    use chrono::TimeZone;

    const NAS: &str = "http://192.168.0.151:5005/infodesk/";

    fn jst() -> FixedOffset {
        FixedOffset::east_opt(9 * 3600).unwrap()
    }

    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 11, 1, 3, 0, 0).unwrap()
    }

    fn nas_store(url: &str) -> Mutex<Store> {
        let s = Store::open_in_memory().unwrap();
        crate::seed::seed_defaults(&s).unwrap();
        s.set_setting(SETTING_ENABLED, "true").unwrap();
        s.set_setting(SETTING_BACKEND, BACKEND_NAS).unwrap();
        s.set_setting(SETTING_NAS_URL, url).unwrap();
        s.set_setting(SETTING_NAS_USER, "infodesk").unwrap();
        Mutex::new(s)
    }

    fn env(pw: Option<&str>) -> HeadlessEnv {
        HeadlessEnv { nas_password: pw.map(str::to_string), tz: jst() }
    }

    /// 作ろうとした WebDAV の設定を記録し、メモリ上の保存先を返す。
    fn recording() -> (Arc<Mutex<Vec<WebDavConfig>>>, Box<MakeDrive>) {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let s = seen.clone();
        let make: Box<MakeDrive> = Box::new(move |cfg| {
            s.lock().unwrap().push(cfg);
            Ok(Arc::new(MemoryDrive::new()) as Arc<dyn DriveStore>)
        });
        (seen, make)
    }

    #[tokio::test]
    async fn off_or_google_does_nothing() {
        let store = nas_store(NAS);
        let (seen, make) = recording();
        store.lock().unwrap().set_setting(SETTING_ENABLED, "false").unwrap();
        let got = sync_nas(&store, &env(Some("pw")), &*make, now()).await;
        assert!(matches!(got, Ok(SyncOutcome::Disabled)));
        store.lock().unwrap().set_setting(SETTING_ENABLED, "true").unwrap();
        store.lock().unwrap().set_setting(SETTING_BACKEND, BACKEND_GDRIVE).unwrap();
        let got = sync_nas(&store, &env(Some("pw")), &*make, now()).await;
        assert!(matches!(got, Ok(SyncOutcome::Disabled)));
        assert!(seen.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn missing_password_is_incomplete() {
        let store = nas_store(NAS);
        let (seen, make) = recording();
        for pw in [None, Some("")] {
            let got = sync_nas(&store, &env(pw), &*make, now()).await;
            assert_eq!(got.err().as_deref(), Some(NAS_INCOMPLETE_MESSAGE));
        }
        assert!(seen.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_name_from_the_mac_settings_sends_nothing() {
        // Review Focus 3: the copied Mac settings hold http://takeuchi-nas:5005/infodesk/
        let store = nas_store("http://takeuchi-nas:5005/infodesk/");
        let (seen, make) = recording();
        let got = sync_nas(&store, &env(Some("pw")), &*make, now()).await;
        assert_eq!(got.err().as_deref(), Some(NAS_LAN_URL_MESSAGE));
        assert!(seen.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_normal_pass_connects_directly_and_records_the_time() {
        let store = nas_store(NAS);
        let (seen, make) = recording();
        let got = sync_nas(&store, &env(Some("pw")), &*make, now()).await;
        assert!(matches!(got, Ok(SyncOutcome::Synced(_))), "{got:?}");
        let cfg = seen.lock().unwrap()[0].clone();
        assert_eq!(cfg.url, NAS);
        assert_eq!(cfg.user, "infodesk");
        assert_eq!(cfg.password, "pw");
        assert!(cfg.proxy.is_none());
        let s = store.lock().unwrap();
        assert_eq!(s.get_setting(SETTING_LAST_SYNC_AT).unwrap(), Some(now().to_rfc3339()));
        // 初回はまとめる (保存先にスナップショットがない)。
        assert_eq!(s.get_setting(SETTING_LAST_COMPACTION_AT).unwrap(), Some(now().to_rfc3339()));
    }

    #[test]
    fn debug_hides_the_password() {
        assert!(!format!("{:?}", env(Some("secret-pw"))).contains("secret-pw"));
    }
}
```

- [ ] **Step 6: Run to see them fail**

Run: `cargo test -p infodesk-core sync::headless`
Expected: compile errors.

- [ ] **Step 7: Implement `headless.rs`** (above the tests)

```rust
//! アプリなしで NAS へ1回同期する。自宅の k3s で動く `infodesk tick` と `infodesk sync` が使う。
//! NAS へは LAN で直接つなぐ。パスワードは環境変数から読み、DB には置かない。

use std::sync::{Arc, Mutex};

use chrono::{DateTime, FixedOffset, Utc};

use super::drive::{DriveError, DriveStore};
use super::engine::{SyncError, SyncInput, SyncReport, compaction_due, run_sync};
use super::nas::*;
use super::webdav::{WebDavConfig, WebDavStore};
use crate::schedule::ScheduleConfig;
use crate::store::Store;

pub const NAS_PASSWORD_ENV: &str = "INFODESK_NAS_PASSWORD";
/// `sync.device_name` が空のときの書き手の名前。
pub const DEFAULT_SERVER_NAME: &str = "infodesk-server";

pub struct HeadlessEnv {
    pub nas_password: Option<String>,
    pub tz: FixedOffset,
}

impl std::fmt::Debug for HeadlessEnv {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HeadlessEnv")
            .field("nas_password", &self.nas_password.as_ref().map(|_| "<redacted>"))
            .field("tz", &self.tz)
            .finish()
    }
}

impl HeadlessEnv {
    pub fn from_env(tz: FixedOffset) -> Self {
        Self { nas_password: std::env::var(NAS_PASSWORD_ENV).ok(), tz }
    }
}

pub type MakeDrive =
    dyn Fn(WebDavConfig) -> Result<Arc<dyn DriveStore>, DriveError> + Send + Sync;

/// 本物の WebDAV クライアント。
pub fn webdav(cfg: WebDavConfig) -> Result<Arc<dyn DriveStore>, DriveError> {
    Ok(Arc::new(WebDavStore::new(cfg)?))
}

#[derive(Debug)]
pub enum SyncOutcome {
    /// 同期が無効か、保存先が NAS ではない。
    Disabled,
    Synced(SyncReport),
}

struct Prepared {
    url: String,
    user: String,
    device_id: String,
    db_id: String,
    writer: String,
    last_compaction: Option<DateTime<Utc>>,
    schedule: ScheduleConfig,
}

fn lock(store: &Mutex<Store>) -> std::sync::MutexGuard<'_, Store> {
    store.lock().unwrap_or_else(|e| e.into_inner())
}

/// 設定を読む。無効なら `None`。
fn prepare(store: &Mutex<Store>) -> crate::Result<Option<Prepared>> {
    let s = lock(store);
    let text = |k: &str| -> crate::Result<String> {
        Ok(s.get_setting(k)?.unwrap_or_default().trim().to_string())
    };
    if text(SETTING_ENABLED)? != "true" || text(SETTING_BACKEND)? != BACKEND_NAS {
        return Ok(None);
    }
    // 保存先の切り替えの予約。ここでは保存先の控えを持たないので、消すだけでよい。
    if text(SETTING_RESET_PENDING)? == "true" {
        s.set_setting(SETTING_RESET_PENDING, "false")?;
    }
    let (device_id, db_id) = s.ensure_sync_ids()?;
    let writer = Some(text(SETTING_DEVICE_NAME)?)
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| DEFAULT_SERVER_NAME.to_string());
    let last_compaction = s
        .get_setting(SETTING_LAST_COMPACTION_AT)?
        .and_then(|v| DateTime::parse_from_rfc3339(&v).ok())
        .map(|d| d.with_timezone(&Utc));
    let schedule = ScheduleConfig::from_settings(|k| s.get_setting(k).ok().flatten());
    Ok(Some(Prepared {
        url: text(SETTING_NAS_URL)?,
        user: text(SETTING_NAS_USER)?,
        device_id,
        db_id,
        writer,
        last_compaction,
        schedule,
    }))
}

/// NAS へ1回同期する。エラーは利用者に見せる文 (秘密を含まない)。
pub async fn sync_nas(
    store: &Mutex<Store>,
    env: &HeadlessEnv,
    make_drive: &MakeDrive,
    now: DateTime<Utc>,
) -> Result<SyncOutcome, String> {
    let Some(p) = prepare(store).map_err(|e| e.to_string())? else {
        return Ok(SyncOutcome::Disabled);
    };
    let password = env.nas_password.clone().filter(|pw| !pw.is_empty());
    let (Some(password), false, false) = (password, p.url.is_empty(), p.user.is_empty()) else {
        return Err(NAS_INCOMPLETE_MESSAGE.to_string());
    };
    let url = lan_nas_url(&p.url).map_err(str::to_string)?;
    let drive = make_drive(WebDavConfig { url, user: p.user, password, proxy: None })
        .map_err(|_| NAS_PREPARE_MESSAGE.to_string())?;
    let compact = p.last_compaction.is_none()
        || compaction_due(p.last_compaction, now, env.tz, p.schedule.daily_time);
    let input = SyncInput {
        device_id: p.device_id,
        db_id: p.db_id,
        writer: p.writer,
        now,
        tz: env.tz,
        compact,
    };
    let report = run_sync(store, drive.as_ref(), &input)
        .await
        .map_err(|e| match e {
            SyncError::Drive(DriveError::Retryable(_)) => NETWORK_MESSAGE.to_string(),
            other => other.to_string(),
        })?;
    let stamp = now.to_rfc3339();
    let s = lock(store);
    s.set_setting(SETTING_LAST_SYNC_AT, &stamp).map_err(|e| e.to_string())?;
    if report.compacted {
        s.set_setting(SETTING_LAST_COMPACTION_AT, &stamp).map_err(|e| e.to_string())?;
    }
    Ok(SyncOutcome::Synced(report))
}
```

- [ ] **Step 8: Run** `cargo test -p infodesk-core sync:: && cargo test -p infodesk-app` — Expected: PASS.

- [ ] **Step 9: Commit**

```bash
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings
git add crates/core/src/sync app/src-tauri/src/sync/mod.rs
git commit -m "feat(core): one NAS sync without the app, straight over the home LAN"
```

---

### Task 4: CLI default path by OS and `infodesk sync`

**Files:**
- Modify: `crates/cli/src/lib.rs:15-23,168-171,493-551`

**Interfaces:**
- Consumes: `sync::headless::{sync_nas, HeadlessEnv, SyncOutcome, webdav}` (Task 3).
- Produces: `fn db_path_for(os: &str, home: Option<String>, xdg_data_home: Option<String>) -> Result<PathBuf>`; `Command::Sync`; `local_tz()` becomes `pub(crate)` for Task 5.

- [ ] **Step 1: Write failing tests** (append to `mod tests` in `lib.rs`)

```rust
    #[test]
    fn default_db_path_follows_the_os() {
        let home = Some("/home/kou".to_string());
        assert_eq!(
            db_path_for("macos", home.clone(), None).unwrap(),
            PathBuf::from("/home/kou/Library/Application Support/InfoDesk/infodesk.db")
        );
        assert_eq!(
            db_path_for("linux", home.clone(), None).unwrap(),
            PathBuf::from("/home/kou/.local/share/InfoDesk/infodesk.db")
        );
        assert_eq!(
            db_path_for("linux", home.clone(), Some("/data".into())).unwrap(),
            PathBuf::from("/data/InfoDesk/infodesk.db")
        );
        // 空の XDG_DATA_HOME は未設定と同じ。
        assert_eq!(
            db_path_for("linux", home, Some(String::new())).unwrap(),
            PathBuf::from("/home/kou/.local/share/InfoDesk/infodesk.db")
        );
        assert!(db_path_for("linux", None, None).is_err());
    }

    #[test]
    fn sync_and_tick_are_commands() {
        assert!(matches!(parse(&["sync"]).unwrap().command, Command::Sync));
    }
```

- [ ] **Step 2: Run to see them fail**

Run: `cargo test -p infodesk default_db_path_follows_the_os`
Expected: compile errors.

- [ ] **Step 3: Implement**

Replace `default_db_path`:

```rust
fn default_db_path() -> Result<PathBuf> {
    db_path_for(
        std::env::consts::OS,
        std::env::var("HOME").ok(),
        std::env::var("XDG_DATA_HOME").ok(),
    )
}

/// OS ごとの既定の DB の場所。macOS は Application Support、ほかは XDG のデータの場所。
fn db_path_for(os: &str, home: Option<String>, xdg_data_home: Option<String>) -> Result<PathBuf> {
    let home = || home.clone().ok_or_else(|| Error::Invalid("HOME is not set".into()));
    let base = match os {
        "macos" => PathBuf::from(home()?).join("Library/Application Support"),
        _ => match xdg_data_home.filter(|d| !d.is_empty()) {
            Some(dir) => PathBuf::from(dir),
            None => PathBuf::from(home()?).join(".local/share"),
        },
    };
    Ok(base.join("InfoDesk/infodesk.db"))
}
```

Change the `--db` doc comment to: `/// DBファイルのパス。既定は macOS では ~/Library/Application Support/InfoDesk/infodesk.db、Linux では ~/.local/share/InfoDesk/infodesk.db`. Make `fn local_tz` `pub(crate)`.

Add to `enum Command` after `Digests`:

```rust
    /// NAS へ今すぐ1回同期する (保存先が NAS のときだけ)
    Sync,
```

Add:

```rust
async fn sync(store: Store) -> Result<()> {
    use infodesk_core::sync::headless::{self, HeadlessEnv, SyncOutcome};
    let store = std::sync::Mutex::new(store);
    let env = HeadlessEnv::from_env(local_tz());
    match headless::sync_nas(&store, &env, &headless::webdav, Utc::now()).await {
        Ok(SyncOutcome::Disabled) => {
            println!("同期は無効です (sync.enabled = true と sync.backend = nas を確かめてください)");
            Ok(())
        }
        Ok(SyncOutcome::Synced(r)) => {
            println!(
                "ok  取り込み {}  反映 {}  送信 {}  書き込み {}  削除 {}",
                r.imported_ops,
                r.applied_ops,
                r.uploaded_ops,
                r.written_files.len(),
                r.deleted_files.len()
            );
            for w in r.warnings {
                println!("warn {w}");
            }
            Ok(())
        }
        Err(message) => Err(Error::Invalid(message)),
    }
}
```

In `run`, after `seed::seed_defaults(&store)?;` and before the `match`:

```rust
    if matches!(cli.command, Command::Sync) {
        return sync(store).await;
    }
```

and add `Command::Sync => unreachable!("handled before the match"),` to the match.

- [ ] **Step 4: Run** `cargo test -p infodesk` — Expected: PASS.

- [ ] **Step 5: Commit**

```bash
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings
git add crates/cli/src/lib.rs
git commit -m "feat(cli): a Linux default database path and a sync command for the NAS"
```

---

### Task 5: `infodesk tick`

**Files:**
- Create: `crates/cli/src/tick.rs`
- Modify: `crates/cli/src/lib.rs`, `crates/cli/Cargo.toml`

**Interfaces:**
- Consumes: `schedule::*` (Task 1), `llm::{provider_from_settings, LlmProvider, LlmRole}`, `sync::headless::*` (Task 3), `collector::{http_client, fetch_all, apply_outcomes, images}`, `summarizer::{plan_item_summaries, finish_item_summary, plan_digest, finish_digest, DigestOutcome}`, `model::{Category, DigestScope, Period}`, `period::{period_start, utc_range}`.
- Produces: `pub type LlmFactory = Box<dyn Fn(&Store, LlmRole) -> infodesk_core::Result<Box<dyn LlmProvider>> + Send + Sync>`; `pub struct Runner`; `Runner::new(store, tz, llm, make_drive, env) -> Result<Runner>`; `pub async fn run(&self, now: DateTime<FixedOffset>) -> Result<Vec<String>>` (log lines in order; `Err` only when the database cannot be read); `pub async fn tick(store: Store, tz: FixedOffset) -> Result<()>`; `Command::Tick`. Settings written: `schedule.attempts` (JSON object `{"<job key>": "<RFC 3339>"}`), `schedule.last_retry_at` (RFC 3339). Job keys: `collect`, `daily:YYYY-MM-DD`, `weekly:YYYY-MM-DD`, `monthly:YYYY-MM-DD`.

- [ ] **Step 1: Dependencies**

In `crates/cli/Cargo.toml` `[dependencies]` add `reqwest = "0.13.5"`. In `[dev-dependencies]` add `async-trait = "0.1.92"`. (No tokio feature change: `tick` runs once inside the existing runtime.)

- [ ] **Step 2: Write the failing tests** (create `tick.rs` with the test module; add `mod tick;` to `lib.rs`)

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use chrono::TimeZone;
    use infodesk_core::sync::drive::MemoryDrive;
    use infodesk_core::sync::nas::*;

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
        store.set_setting(SETTING_NAS_URL, "http://192.168.0.151:5005/infodesk/").unwrap();
        store.set_setting(SETTING_NAS_USER, "infodesk").unwrap();
        let env = HeadlessEnv { nas_password: Some("pw".into()), tz: jst() };
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

    #[tokio::test]
    async fn first_run_collects_digests_and_syncs_in_order() {
        let r = runner(ok_llm());
        let log = r.run(at(8, 0)).await.unwrap();
        let pos = |p: &str| log.iter().position(|l| l.starts_with(p)).unwrap_or_else(|| panic!("{p} in {log:?}"));
        assert!(pos("collect:") < pos("sync:"), "{log:?}");
        assert!(has(&log, "digest daily 2026-11-01"), "{log:?}");
        assert!(setting(&r, "schedule.last_collect_at").is_some());
        assert_eq!(setting(&r, "schedule.last_daily_for").as_deref(), Some("2026-11-01"));
        assert!(setting(&r, SETTING_LAST_SYNC_AT).is_some());
        assert!(setting(&r, "schedule.attempts").unwrap().contains("\"collect\""));
        assert!(setting(&r, "schedule.last_retry_at").is_some());
    }

    #[tokio::test]
    async fn a_second_run_right_after_does_nothing() {
        let r = runner(ok_llm());
        r.run(at(8, 0)).await.unwrap();
        let log = r.run(at(8, 5)).await.unwrap();
        assert!(log.iter().all(|l| !l.starts_with("collect:") && !l.starts_with("digest")), "{log:?}");
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
            st.set_setting("schedule.last_collect_at", &at(7, 30).to_rfc3339()).unwrap();
            st.set_setting("schedule.last_daily_for", "2026-11-01").unwrap();
            st.set_setting("schedule.last_weekly_for", "2026-10-26").unwrap();
            st.set_setting("schedule.last_monthly_for", "2026-10-01").unwrap();
            st.set_setting("sync.device_id", "mac-device").unwrap();
        }
        let log = r.run(at(8, 0)).await.unwrap();
        assert!(log.iter().all(|l| !l.starts_with("collect:") && !l.starts_with("digest")), "{log:?}");
        assert_eq!(setting(&r, "sync.device_id").as_deref(), Some("mac-device"));
    }

    #[tokio::test]
    async fn a_missing_provider_does_not_stop_collection() {
        // Review Focus 2
        let llm: LlmFactory = Box::new(|_, _| {
            Err(infodesk_core::Error::Invalid("ANTHROPIC_API_KEY is not set".into()))
        });
        let r = runner(llm);
        let log = r.run(at(8, 0)).await.unwrap();
        assert!(has(&log, "collect:"), "{log:?}");
        assert!(log.iter().any(|l| l.contains("ANTHROPIC_API_KEY is not set")), "{log:?}");
        assert!(setting(&r, "schedule.last_collect_at").is_some());
    }

    #[tokio::test]
    async fn a_sync_error_is_logged_and_the_run_still_succeeds() {
        let mut r = runner(ok_llm());
        r.env.nas_password = None;
        let log = r.run(at(8, 0)).await.unwrap();
        assert!(log.iter().any(|l| l.starts_with("sync: error") && l.contains(NAS_INCOMPLETE_MESSAGE)), "{log:?}");
    }

    #[test]
    fn job_keys_round_trip() {
        let d = |s: &str| NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap();
        for job in [
            Job::Collect,
            Job::Daily(d("2026-11-01")),
            Job::Weekly(d("2026-10-26")),
            Job::Monthly(d("2026-10-01")),
        ] {
            assert_eq!(job_from_key(&job_key(&job)), Some(job.clone()), "{job:?}");
        }
        assert_eq!(job_from_key("weekly:garbage"), None);
        assert_eq!(job_from_key("other"), None);
    }
}
```

Add this helper to the same test module (it saves one article through the public collector path; the collection time is the article time, so the article falls in that period whichever time the digest window uses):

```rust
    fn add_item(s: &Store, source: infodesk_core::model::Source, at: &str) {
        let time = DateTime::parse_from_rfc3339(at).unwrap().with_timezone(&Utc);
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
```

- [ ] **Step 3: Run to see them fail**

Run: `cargo test -p infodesk tick::`
Expected: compile errors.

- [ ] **Step 4: Implement** (top of `tick.rs`)

```rust
//! `infodesk tick`: 予定を1回だけ評価して終わる。自宅の k3s の Argo CronWorkflow が5分ごとに呼ぶ。
//! 規則はアプリと同じ (`infodesk_core::schedule`)。プロセスをまたぐ状態は DB に置く。

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex, MutexGuard};

use chrono::{DateTime, FixedOffset, Local, NaiveDate, Utc};
use infodesk_core::collector::{apply_outcomes, fetch_all, http_client, images};
use infodesk_core::llm::{LlmProvider, LlmRole, provider_from_settings};
use infodesk_core::model::{Category, DigestScope, Period};
use infodesk_core::period::{period_start, utc_range};
use infodesk_core::schedule::{
    Job, LastRuns, RECENT_ATTEMPT, ScheduleConfig, all_failed_retry_base, due_jobs,
    filter_recent, job_plan, last_run_record, retry_due, should_record,
};
use infodesk_core::summarizer::{
    DigestOutcome, finish_digest, finish_item_summary, plan_digest, plan_item_summaries,
};
use infodesk_core::sync::drive::DriveStore;
use infodesk_core::sync::headless::{self, HeadlessEnv, MakeDrive, SyncOutcome};
use infodesk_core::{Result, Store};

pub type LlmFactory =
    Box<dyn Fn(&Store, LlmRole) -> Result<Box<dyn LlmProvider>> + Send + Sync>;

const SETTING_ATTEMPTS: &str = "schedule.attempts";
const SETTING_LAST_RETRY_AT: &str = "schedule.last_retry_at";

pub struct Runner {
    pub(crate) store: Mutex<Store>,
    http: reqwest::Client,
    tz: FixedOffset,
    llm: LlmFactory,
    make_drive: Box<MakeDrive>,
    pub(crate) env: HeadlessEnv,
}

fn all_scopes() -> Vec<DigestScope> {
    Category::ALL.into_iter().map(DigestScope::Category).collect()
}

/// `schedule.attempts` のキー。
pub(crate) fn job_key(job: &Job) -> String {
    match job {
        Job::Collect => "collect".to_string(),
        Job::Daily(d) => format!("daily:{d}"),
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
        "weekly" => Some(Job::Weekly(date)),
        "monthly" => Some(Job::Monthly(date)),
        _ => None,
    }
}

fn label(job: &Job) -> String {
    job_key(job).replace(':', " ")
}

fn parse_utc(v: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(v).ok().map(|d| d.with_timezone(&Utc))
}

impl Runner {
    pub fn new(
        store: Store,
        tz: FixedOffset,
        llm: LlmFactory,
        make_drive: Box<MakeDrive>,
        env: HeadlessEnv,
    ) -> Result<Self> {
        Ok(Self { store: Mutex::new(store), http: http_client()?, tz, llm, make_drive, env })
    }

    fn lock(&self) -> MutexGuard<'_, Store> {
        self.store.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn get(&self, key: &str) -> Option<String> {
        self.lock().get_setting(key).ok().flatten()
    }

    fn load_attempts(&self) -> Result<HashMap<Job, DateTime<Utc>>> {
        let raw = self.lock().get_setting(SETTING_ATTEMPTS)?.unwrap_or_default();
        let map: BTreeMap<String, String> = serde_json::from_str(&raw).unwrap_or_default();
        Ok(map
            .iter()
            .filter_map(|(k, v)| Some((job_from_key(k)?, parse_utc(v)?)))
            .collect())
    }

    fn save_attempts(&self, attempts: &HashMap<Job, DateTime<Utc>>) {
        let map: BTreeMap<String, String> =
            attempts.iter().map(|(j, at)| (job_key(j), at.to_rfc3339())).collect();
        let json = serde_json::to_string(&map).unwrap_or_else(|_| "{}".to_string());
        let _ = self.lock().set_setting(SETTING_ATTEMPTS, &json);
    }

    /// 1回の評価。実行したことをログの行として順に返す。DB を読めないときだけ `Err`。
    pub async fn run(&self, now: DateTime<FixedOffset>) -> Result<Vec<String>> {
        let mut log = Vec::new();
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
            let _ = self.lock().set_setting(SETTING_LAST_RETRY_AT, &now_utc.to_rfc3339());
            if self.retry_pending(&mut log).await {
                self.sync(now_utc, &mut log).await;
            }
        }
        Ok(log)
    }
```

The rest of `impl Runner` is the job flow. Write these methods exactly as follows (they mirror the app's `jobs.rs` and `scheduler.rs::run_job`, with short locks around store calls and none across an `await`):

```rust
    /// 収集と記事要約。記録まで進んだら真 (同期する)。
    async fn collect(&self, now: DateTime<Utc>, cfg: &ScheduleConfig, log: &mut Vec<String>) -> bool {
        let sources = match self.lock().list_sources() {
            Ok(s) => s,
            Err(e) => {
                log.push(format!("collect: error {e}"));
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
                log.push(format!("collect: error {e}"));
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
            let _ = self.lock().set_setting("schedule.last_collect_at", &base.to_rfc3339());
            log.push("collect: all sources failed, retrying in about 10 minutes".to_string());
            return false;
        }
        if let Err(e) = self.lock().set_setting("schedule.last_collect_at", &now.to_rfc3339()) {
            log.push(format!("collect: error {e}"));
            return false;
        }
        log.push(format!("collect: {new_items} new, failed sources: {failed:?}"));
        self.item_summaries(now, log).await;
        true
    }

    /// 今日取得した記事の上位に要約を付ける。最初の失敗で止める (アプリと同じ)。
    async fn item_summaries(&self, now: DateTime<Utc>, log: &mut Vec<String>) {
        let today = now.with_timezone(&self.tz).date_naive();
        let (since, _) = utc_range(Period::Day, period_start(Period::Day, today), self.tz);
        let limit: u32 = self.get("summary.top_n").and_then(|v| v.parse().ok()).unwrap_or(20);
        let llm = match (self.llm)(&self.lock(), LlmRole::Items) {
            Ok(l) => l,
            Err(e) => {
                log.push(format!("summaries: error {e}"));
                return;
            }
        };
        let (mut ok, mut error) = (0, None);
        'categories: for category in Category::ALL {
            let jobs = match plan_item_summaries(&self.lock(), category, &since, limit) {
                Ok(j) => j,
                Err(e) => {
                    error = Some(e.to_string());
                    break;
                }
            };
            for job in jobs {
                let output = llm.complete(&job.system, &job.prompt).await;
                match finish_item_summary(&self.lock(), job.item_id, output) {
                    Ok(Ok(())) => ok += 1,
                    Ok(Err(message)) => {
                        error = Some(message);
                        break 'categories;
                    }
                    Err(e) => {
                        error = Some(e.to_string());
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
                    Some(finish_digest(&self.lock(), &job, &llm.name(), output, &Utc::now())?)
                }
            });
        }
        Ok(out)
    }

    /// 日次・週次・月次のジョブ。アプリの `run_job` と同じ規則。何か生成したら真。
    async fn digest_job(&self, job: &Job, today: NaiveDate, log: &mut Vec<String>) -> bool {
        let label = label(job);
        let mut plan = job_plan(job, today).into_iter();
        let Some((period, start, force)) = plan.next() else {
            return false;
        };
        let outcomes = match self.digests(period, start, &all_scopes(), force).await {
            Ok(o) => o,
            Err(e) => {
                log.push(format!("digest {label}: error {e}"));
                return false;
            }
        };
        let errors: Vec<String> = outcomes.iter().flatten().filter_map(|o| o.error.clone()).collect();
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
        let detail = if errors.is_empty() { String::new() } else { format!(", pending: {errors:?}") };
        log.push(format!("digest {label}: done{detail}"));
        true
    }

    /// 保留中の要約を再試行する。保留があれば真。
    async fn retry_pending(&self, log: &mut Vec<String>) -> bool {
        let pending = match self.lock().pending_digests() {
            Ok(p) => p,
            Err(e) => {
                log.push(format!("retry: error {e}"));
                return false;
            }
        };
        if pending.is_empty() {
            return false;
        }
        let mut recovered = 0;
        for d in &pending {
            if let Ok(o) = self
                .digests(d.period, d.period_start, std::slice::from_ref(&d.scope), false)
                .await
                && o.first().is_some_and(|o| o.as_ref().is_some_and(|o| o.error.is_none()))
            {
                recovered += 1;
            }
        }
        log.push(format!("retry: {recovered} of {} pending recovered", pending.len()));
        true
    }

    async fn sync(&self, now: DateTime<Utc>, log: &mut Vec<String>) {
        let got = headless::sync_nas(&self.store, &self.env, &*self.make_drive, now).await;
        log.push(match got {
            Ok(SyncOutcome::Disabled) => "sync: off".to_string(),
            Ok(SyncOutcome::Synced(r)) => format!(
                "sync: ok, imported {}, applied {}, uploaded {}, written {}",
                r.imported_ops, r.applied_ops, r.uploaded_ops, r.written_files.len()
            ),
            Err(message) => format!("sync: error {message}"),
        });
    }
}

/// 1回評価して、ログを標準出力に出す。
pub async fn tick(store: Store, tz: FixedOffset) -> Result<()> {
    let make_drive: Box<MakeDrive> = Box::new(headless::webdav);
    let runner = Runner::new(
        store,
        tz,
        Box::new(provider_from_settings),
        make_drive,
        HeadlessEnv::from_env(tz),
    )?;
    let lines = runner.run(Local::now().with_timezone(&tz)).await?;
    if lines.is_empty() {
        println!("tick: nothing due");
    }
    for line in lines {
        println!("{line}");
    }
    Ok(())
}
```

If clippy reports `Arc` or `DriveStore` unused outside tests, move those imports into the test module.

- [ ] **Step 5: Wire the command** in `lib.rs`

Add to `enum Command` after `Sync`:

```rust
    /// 予定を1回だけ評価して、収集・要約・同期のうち時刻が来たものを実行する (k3s の CronWorkflow 用)
    Tick,
```

In `run`, next to the `Sync` early return:

```rust
    if matches!(cli.command, Command::Tick) {
        return tick::tick(store, local_tz()).await;
    }
```

and `Command::Tick => unreachable!("handled before the match"),` in the match. Extend `sync_and_tick_are_commands` with `assert!(matches!(parse(&["tick"]).unwrap().command, Command::Tick));`.

- [ ] **Step 6: Run** `cargo test -p infodesk` — Expected: PASS.

- [ ] **Step 7: Manual smoke run on the Mac** (sync off, sources on, LLM default Ollama may be absent)

```bash
cargo run -q -p infodesk -- --db /tmp/infodesk-tick-smoke.db tick
cargo run -q -p infodesk -- --db /tmp/infodesk-tick-smoke.db tick
```

Expected: the first prints `collect: ...`, a `summaries:` line (an error line is fine if Ollama is not running) and digest lines; the second prints `tick: nothing due` or only a `retry:` line. Exit code 0 both times. Then `rm /tmp/infodesk-tick-smoke.db*`.

- [ ] **Step 8: Commit**

```bash
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings
git add crates/cli
git commit -m "feat(cli): infodesk tick evaluates the schedule once for a CronWorkflow"
```

---

### Task 6: Container image, k3s manifests and docs

**Files:**
- Create: `Dockerfile`, `.dockerignore`, `deploy/k8s/namespace.yaml`, `deploy/k8s/pvc.yaml`, `deploy/k8s/rbac.yaml`, `deploy/k8s/workflowtemplate.yaml`, `deploy/k8s/cronworkflow.yaml`, `deploy/k8s/run-once.yaml`, `deploy/k8s/shell.yaml`, `deploy/k8s/secret.example.yaml`, `deploy/k8s/build-and-push.sh`, `docs/15-home-server.md`, `docs/15-home-server.ja.md`
- Modify: `docs/07-settings.md`, `docs/07-settings.ja.md`, `docs/08-cli.md`, `docs/08-cli.ja.md`, `docs/14-mobile-app.md`, `docs/14-mobile-app.ja.md`, `README.md`, `README.ja.md`

- [ ] **Step 1: `Dockerfile`**

```dockerfile
# infodesk CLI for the home k3s cluster (amd64). Build on the WSL node; see docs/15-home-server.md.
FROM rust:1.97-bookworm AS build
WORKDIR /src
COPY . .
RUN cargo build --release -p infodesk --locked

FROM build AS test
RUN cargo test -p infodesk-core -p infodesk --locked

FROM debian:bookworm-slim
RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates tzdata \
 && rm -rf /var/lib/apt/lists/* \
 && useradd --system --uid 10001 --home-dir /data --no-create-home infodesk
COPY --from=build /src/target/release/infodesk /usr/local/bin/infodesk
USER 10001
ENTRYPOINT ["infodesk"]
```

- [ ] **Step 2: `.dockerignore`**

```
.git
target
**/node_modules
app/dist
app/src-tauri/target
app/src-tauri/google-oauth.json
mobile
design
design.pen
docs
**/*.env
```

The workspace lists `app/src-tauri` as a member, so its `Cargo.toml` and sources stay in the context (only `-p infodesk` is built).

- [ ] **Step 3: `deploy/k8s/build-and-push.sh`** (run on the Mac from the repository root)

```bash
#!/usr/bin/env bash
# Builds the image on the WSL node of 192.168.0.175 and pushes it to registry.umafolio.site.
# Usage: deploy/k8s/build-and-push.sh   (prints the image tag)
set -euo pipefail
host="Kousuke Takeuchi@192.168.0.175"
tag="$(git rev-parse --short HEAD)"
image="registry.umafolio.site/infodesk:${tag}"
dir="/root/build/infodesk-${tag}"
# base64 keeps the tar intact through the Windows PowerShell session.
git archive --format=tar.gz HEAD | base64 | ssh -o BatchMode=yes "$host" \
  "wsl -d Ubuntu-24.04 -- bash -c 'rm -rf ${dir} && mkdir -p ${dir} && base64 -d | tar -xz -C ${dir}'"
ssh -o BatchMode=yes "$host" "wsl -d Ubuntu-24.04 -- bash -c 'cd ${dir} && docker build --target test -t infodesk-test:${tag} . && docker build -t ${image} . && docker push ${image}'"
echo "$image"
```

`chmod +x deploy/k8s/build-and-push.sh`.

- [ ] **Step 4: Manifests**

`deploy/k8s/namespace.yaml`:

```yaml
apiVersion: v1
kind: Namespace
metadata:
  name: infodesk
```

`deploy/k8s/pvc.yaml`:

```yaml
apiVersion: v1
kind: PersistentVolumeClaim
metadata:
  name: infodesk-data
  namespace: infodesk
spec:
  accessModes: [ReadWriteOnce]
  storageClassName: local-path
  resources:
    requests:
      storage: 2Gi
```

`deploy/k8s/rbac.yaml` (same rules as `umafolio/argo-workflow-executor`):

```yaml
apiVersion: v1
kind: ServiceAccount
metadata:
  name: infodesk
  namespace: infodesk
---
apiVersion: rbac.authorization.k8s.io/v1
kind: Role
metadata:
  name: argo-workflow-executor
  namespace: infodesk
rules:
  - apiGroups: [""]
    resources: [pods, pods/exec]
    verbs: [create, get, list, watch, patch, delete]
  - apiGroups: [""]
    resources: [pods/log]
    verbs: [get, watch]
  - apiGroups: [argoproj.io]
    resources: [workflowtaskresults]
    verbs: [create, get, list, watch, patch]
---
apiVersion: rbac.authorization.k8s.io/v1
kind: RoleBinding
metadata:
  name: argo-workflow-executor
  namespace: infodesk
roleRef:
  apiGroup: rbac.authorization.k8s.io
  kind: Role
  name: argo-workflow-executor
subjects:
  - kind: ServiceAccount
    name: infodesk
    namespace: infodesk
```

`deploy/k8s/workflowtemplate.yaml` (replace `TAG` with the tag printed by `build-and-push.sh`):

```yaml
apiVersion: argoproj.io/v1alpha1
kind: WorkflowTemplate
metadata:
  name: infodesk-tick
  namespace: infodesk
spec:
  entrypoint: tick
  serviceAccountName: infodesk
  activeDeadlineSeconds: 3000
  nodeSelector:
    kubernetes.io/hostname: takeuchi-nuc11atkc2
  securityContext:
    runAsUser: 10001
    runAsGroup: 10001
    fsGroup: 10001
  volumes:
    - name: data
      persistentVolumeClaim:
        claimName: infodesk-data
  templates:
    - name: tick
      container:
        image: registry.umafolio.site/infodesk:TAG
        args: ["--db", "/data/infodesk.db", "tick"]
        env:
          - name: TZ
            value: Asia/Tokyo
        envFrom:
          - secretRef:
              name: infodesk-env
        volumeMounts:
          - name: data
            mountPath: /data
        resources:
          requests:
            cpu: 50m
            memory: 64Mi
          limits:
            memory: 512Mi
```

`deploy/k8s/cronworkflow.yaml`:

```yaml
apiVersion: argoproj.io/v1alpha1
kind: CronWorkflow
metadata:
  name: infodesk-tick
  namespace: infodesk
spec:
  schedules: ["*/5 * * * *"]
  timezone: Asia/Tokyo
  concurrencyPolicy: Forbid
  startingDeadlineSeconds: 120
  successfulJobsHistoryLimit: 3
  failedJobsHistoryLimit: 5
  workflowSpec:
    workflowTemplateRef:
      name: infodesk-tick
```

`deploy/k8s/run-once.yaml`:

```yaml
apiVersion: argoproj.io/v1alpha1
kind: Workflow
metadata:
  generateName: infodesk-tick-manual-
  namespace: infodesk
spec:
  workflowTemplateRef:
    name: infodesk-tick
```

`deploy/k8s/shell.yaml` (apply only to run CLI commands; delete afterwards; replace `TAG`):

```yaml
apiVersion: v1
kind: Pod
metadata:
  name: infodesk-shell
  namespace: infodesk
spec:
  nodeSelector:
    kubernetes.io/hostname: takeuchi-nuc11atkc2
  securityContext:
    runAsUser: 10001
    runAsGroup: 10001
    fsGroup: 10001
  containers:
    - name: shell
      image: registry.umafolio.site/infodesk:TAG
      command: ["sleep", "infinity"]
      env:
        - name: TZ
          value: Asia/Tokyo
      envFrom:
        - secretRef:
            name: infodesk-env
      volumeMounts:
        - name: data
          mountPath: /data
  volumes:
    - name: data
      persistentVolumeClaim:
        claimName: infodesk-data
```

`deploy/k8s/secret.example.yaml`:

```yaml
# Documentation only. Do not fill in and do not commit real values.
# Create the real Secret by hand:
#   kubectl -n infodesk create secret generic infodesk-env \
#     --from-literal=ANTHROPIC_API_KEY='<key from the Claude Console>' \
#     --from-literal=INFODESK_NAS_PASSWORD='<DSM password of the WebDAV user>'
apiVersion: v1
kind: Secret
metadata:
  name: infodesk-env
  namespace: infodesk
type: Opaque
stringData:
  ANTHROPIC_API_KEY: ""
  INFODESK_NAS_PASSWORD: ""
```

Validate: on the WSL node, `kubectl apply --dry-run=server -f deploy/k8s/namespace.yaml` first, then (after the namespace exists in Step 7) `kubectl apply --dry-run=server -f` for the others. Expected: no errors.

- [ ] **Step 5: `docs/15-home-server.md` and `.ja.md`**

Header: `**日本語: [15-home-server.ja.md](15-home-server.ja.md)** | ← [Mobile App](14-mobile-app.md)` and `# Home Server (k3s and Argo Workflows)`. The Japanese page has `**English: [15-home-server.md](15-home-server.md)** | ← [前: モバイルアプリ](14-mobile-app.ja.md)` and follows the wording style of `docs/13-nas-setup.ja.md`. Sections:

1. **What it does**: the CronWorkflow `infodesk-tick` runs every 5 minutes on the NUC node; each run collects, summarizes, makes digests and syncs to the NAS when due, by the same rules as the Mac app; the iPhone reads the NAS as before; run history and logs are in the Argo UI (namespace `infodesk`).
2. **Why the Mac must stop**: the copied database carries the Mac's sync device id; two writers with one id overwrite each other's files. Until the Mac reader mode exists, keep the Mac app's sync off and the app closed.
3. **Set up**: spec section 8 as commands, in order: `deploy/k8s/build-and-push.sh`; put the tag into `workflowtemplate.yaml` and `shell.yaml`; `kubectl apply -f deploy/k8s/namespace.yaml -f deploy/k8s/pvc.yaml -f deploy/k8s/rbac.yaml`; the user types the `kubectl create secret generic infodesk-env ...` command; on the Mac turn off 同期する and quit InfoDesk; `sqlite3 "$HOME/Library/Application Support/jp.infodesk.app/infodesk.db" ".backup /tmp/infodesk.db"`; `kubectl apply -f deploy/k8s/shell.yaml`; `kubectl -n infodesk cp /tmp/infodesk.db infodesk-shell:/data/infodesk.db`; the `config set` commands through `kubectl -n infodesk exec infodesk-shell -- infodesk --db /data/infodesk.db config set ...` for `llm.items_provider claude_api`, `llm.digest_provider claude_api`, `sync.nas.url http://192.168.0.151:5005/infodesk/`, `sync.device_name infodesk-k3s`; `... sync`; `kubectl -n infodesk delete pod infodesk-shell`; `kubectl apply -f deploy/k8s/workflowtemplate.yaml`; `kubectl create -f deploy/k8s/run-once.yaml` and read the log (`kubectl -n infodesk logs -l workflows.argoproj.io/workflow --tail=100`); `kubectl apply -f deploy/k8s/cronworkflow.yaml`. Note that kubectl runs on the WSL node (`ssh "Kousuke Takeuchi@192.168.0.175"`, then `wsl -d Ubuntu-24.04`), and that `/tmp/infodesk.db` must be copied there first (`scp` to Windows, then it is visible in WSL under `/mnt/c/...`).
4. **Updating**: `build-and-push.sh`, change the tag in `workflowtemplate.yaml`, `kubectl apply -f deploy/k8s/workflowtemplate.yaml`; the next run uses it.
5. **Claude API settings**: `claude_api.model` (default `claude-sonnet-5-5`), `claude_api.items_effort` (default `low`), `claude_api.digest_effort` (default empty = API default); the cost estimate from spec section 9; check spend in the Claude Console after a week; the key lives only in the Secret `infodesk-env`.
6. **Changing sources, topics and settings**: apply `shell.yaml`, run `kubectl -n infodesk exec infodesk-shell -- infodesk --db /data/infodesk.db source|topic|config ...` (link to [CLI](08-cli.md)), delete the pod.
7. **NAS address**: the URL must be a LAN IP (`http://192.168.0.151:5005/infodesk/`); give the NAS a fixed address (DHCP reservation) so it does not change.
8. **Troubleshooting** table: `sync: error NAS の URL は LAN の IP アドレスで書いてください…` (set `sync.nas.url` to the IP form); `sync: error NAS の URL、ユーザー名、パスワードを入力してください` (check the Secret and `sync.nas.user`); `sync: error ネットワークに接続できません…` (NAS asleep or address changed); `summaries: error ANTHROPIC_API_KEY is not set` (recreate the Secret; the next run reads it); `claude_api 401 ... the API key was rejected` (create a new key in the Console and recreate the Secret); `claude_api 429` / `529` (temporary; digests stay pending and are retried every 15 minutes); a run stuck `Pending` (the PVC is bound to the NUC; check that node is Ready).

- [ ] **Step 6: Update existing docs**

- `docs/14-mobile-app.md` header: append ` | Next: [Home Server →](15-home-server.md)`; `.ja.md`: append ` | [次: 自宅サーバー →](15-home-server.ja.md)`.
- `README.md` and `README.ja.md` docs table: add row 15 after row 14 (`| 15 | [Home Server](docs/15-home-server.md) | Run InfoDesk headless on the home k3s cluster with an Argo CronWorkflow, syncing to the NAS |` and the Japanese equivalent).
- `docs/07-settings.md` (and `.ja.md`): `llm.items_provider` / `llm.digest_provider` accept `claude_api`; add rows for `claude_api.model`, `claude_api.items_effort`, `claude_api.digest_effort`, `schedule.attempts`, `schedule.last_retry_at` (the last two: written by `infodesk tick`, not shown in Settings); note that `claude_api` needs `ANTHROPIC_API_KEY` and is not offered in the Mac app's LLM tab.
- `docs/08-cli.md` (and `.ja.md`): the database path section names both defaults (macOS, and Linux with `XDG_DATA_HOME`); add `sync` and `tick` to the command list with a short section each (what they print; `tick` exits 0 unless the database cannot be read; link to [Home Server](15-home-server.md)).

- [ ] **Step 7: Build and dry-run on the cluster**

Run: `deploy/k8s/build-and-push.sh` (the `test` stage runs `cargo test` on Linux). Then on the WSL node: `kubectl apply -f deploy/k8s/namespace.yaml && kubectl apply --dry-run=server -f deploy/k8s/pvc.yaml -f deploy/k8s/rbac.yaml -f deploy/k8s/workflowtemplate.yaml -f deploy/k8s/cronworkflow.yaml`.
Expected: build, tests and push succeed and print `registry.umafolio.site/infodesk:<sha>`; dry-run reports each object as created (server dry run). Do not create the Secret, copy the database or apply the CronWorkflow in this step: those are the user's go-ahead (spec section 8).

- [ ] **Step 8: Check for secrets and commit**

Run: `git grep -n -E 'sk-ant|umafolio-reg-' -- . ':!docs/superpowers'` — Expected: no output.

```bash
git add Dockerfile .dockerignore deploy docs README.md README.ja.md
git commit -m "feat: run InfoDesk on the home k3s cluster with an Argo CronWorkflow"
```

---

## After the tasks

- Open one PR from `feature/headless-server` to `master`.
- The migration (Secret, Mac shutdown, database copy, first manual run, CronWorkflow) is done with the user, step by step, following `docs/15-home-server.md`. The user types the API key and the NAS password into `kubectl create secret` themselves.
