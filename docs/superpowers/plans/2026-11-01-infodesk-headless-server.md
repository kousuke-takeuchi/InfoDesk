# InfoDesk Headless Server Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Run InfoDesk on an Ubuntu home server with no GUI as the only writer: `infodesk serve` collects, summarizes (optionally through the Claude API), makes digests and syncs to the NAS on the same schedule as the Mac app.

**Architecture:** The pure schedule rules move from the Tauri crate to `infodesk-core::schedule` so the app and the server share them. A new LLM provider `claude_api` calls `POST /v1/messages` with `reqwest`. NAS rules (setting keys, messages, tailnet host check, URL rewrite) move to `infodesk-core::sync::nas`; a new `sync::headless` runs one NAS sync with the host's `tailscale` CLI and an API-free secrets source (environment). The CLI gains `sync` and `serve` and an OS-aware default database path. systemd runs `serve`.

**Tech Stack:** Rust 2024 workspace (`infodesk-core`, `infodesk` CLI, Tauri app), tokio, reqwest 0.13, rusqlite, wiremock (tests), systemd.

**Spec:** `docs/superpowers/specs/2026-11-01-infodesk-headless-server-design.md`

## Global Constraints

- The Mac app must behave exactly as before. Every existing test in `app/src-tauri`, `crates/core` and `crates/cli` keeps passing.
- Claude API request: `anthropic-version: 2026-10-21`, `anthropic-beta: server-side-fallback-2026-07-01`, body `fallbacks: "default"`, `max_tokens: 16000`, no `temperature` / `top_p` / `top_k`, no `thinking` field.
- Default model `claude-sonnet-5-5`. Default effort: `claude_api.items_effort = low`, `claude_api.digest_effort = ""` (API default). Allowed effort values: `low`, `medium`, `high`, `xhigh`, `max`, empty.
- The API key comes only from `ANTHROPIC_API_KEY`; the NAS password only from `INFODESK_NAS_PASSWORD`. Neither is stored in the database, printed, or included in a `Debug` output or an error message.
- The server sends sync data only to a host that `tailscale status --json` lists as a peer, and connects to that peer's Tailscale IP (IPv4 first).
- User-facing messages reuse the app's existing Japanese strings exactly (e.g. 「NAS が Tailscale の中に見つかりません。NAS の名前を確かめてください」).
- Code comments and CLI help in Japanese, as in the surrounding code. Docs under `docs/` in English with a `.ja.md` twin; `docs/superpowers/` in English only.
- Commits: Conventional Commits, no `Co-Authored-By` or any other AI attribution trailer.
- Run from the worktree root `~/Documents/InfoDesk-wt-server`. Before each commit: `cargo fmt --all` and `cargo clippy --workspace --all-targets -- -D warnings`.

## Review Focus

1. **Copied Mac database on first `serve`.** The copied DB already has `schedule.last_*` keys and `sync.device_id`. Expected: the server continues from those (no backfill storm, no new device id). Pinned in Task 6 (`a_copied_database_continues_its_schedule`).
2. **`ANTHROPIC_API_KEY` missing while `claude_api` is selected.** Expected: one clear log line per attempt (`ANTHROPIC_API_KEY is not set`), collection still runs, digests stay pending and are retried. Pinned in Task 2 (`missing_key_is_a_clear_error`) and Task 6 (`a_missing_provider_does_not_stop_collection`).
3. **`tailscaled` not running or `tailscale` not installed.** Expected: sync fails with 「Tailscale が停止中です」 and nothing is sent. Pinned in Task 4 (`no_tailscale_status_sends_nothing`).
4. **A LAN address or a non-tailnet name as the NAS URL.** Expected: refused with the not-in-tailnet message, nothing sent. Pinned in Task 4 (`a_host_outside_the_tailnet_sends_nothing`).
5. **The API answers 401, or the body mentions the key.** Expected: the error text never contains the key. Pinned in Task 2 (`rejected_key_is_not_echoed`).

---

## File Structure

| File | Responsibility |
|---|---|
| `crates/core/src/schedule.rs` (new) | Pure schedule rules shared by app and server |
| `crates/core/src/llm/claude_api.rs` (new) | Claude API provider |
| `crates/core/src/llm/mod.rs` | Provider selection gains `claude_api` |
| `crates/core/src/seed.rs` | Default `claude_api.*` settings |
| `crates/core/src/sync/nas.rs` (new) | NAS setting keys, messages, tailnet peer match, URL rewrite |
| `crates/core/src/sync/headless.rs` (new) | One NAS sync without the app (host Tailscale, env secrets) |
| `app/src-tauri/src/scheduler.rs` | Uses `infodesk_core::schedule`; keeps `run_loop`, `run_job` |
| `app/src-tauri/src/jobs.rs` | Uses `all_failed_retry_base` |
| `app/src-tauri/src/sync/mod.rs`, `app/src-tauri/src/tailnet.rs` | Re-export moved NAS items |
| `crates/cli/src/lib.rs` | OS-aware default path, `sync`, `serve` commands |
| `crates/cli/src/serve.rs` (new) | The server loop and its job flow |
| `deploy/linux/infodesk.service`, `deploy/linux/infodesk.env.example` (new) | systemd unit and env template |
| `docs/15-home-server.md`, `docs/15-home-server.ja.md` (new); `docs/07-settings*.md`, `docs/08-cli*.md`, `docs/14-mobile-app*.md`, `README*.md` | User docs |

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

### Task 3: NAS rules in core

**Files:**
- Create: `crates/core/src/sync/nas.rs`
- Modify: `crates/core/src/sync/mod.rs`, `app/src-tauri/src/sync/mod.rs:19-58,364-376`, `app/src-tauri/src/tailnet.rs:159-298`

**Interfaces:**
- Produces (`infodesk_core::sync::nas`): constants `SETTING_ENABLED`, `SETTING_DEVICE_NAME`, `SETTING_LAST_SYNC_AT`, `SETTING_LAST_COMPACTION_AT`, `SETTING_BACKEND`, `SETTING_NAS_URL`, `SETTING_NAS_USER`, `SETTING_RESET_PENDING`, `BACKEND_GDRIVE`, `BACKEND_NAS`, `NAS_URL_MESSAGE`, `NAS_HTTPS_MESSAGE`, `NAS_PREPARE_MESSAGE`, `TAILNET_STOPPED_MESSAGE`, `NAS_INCOMPLETE_MESSAGE`, `NAS_NOT_IN_TAILNET_MESSAGE`, `NETWORK_MESSAGE` (values copied verbatim from `app/src-tauri/src/sync/mod.rs`); `pub fn peer_addr_in_status(json: &str, host: &str) -> Option<IpAddr>`; `pub fn rewrite_nas_url(raw: &str, peer: impl Fn(&str) -> Option<IpAddr>) -> Result<String, &'static str>`.

- [ ] **Step 1: Write the failing tests** (create `nas.rs` with the test module; add `pub mod nas;` to `crates/core/src/sync/mod.rs`)

```rust
#[cfg(test)]
mod tests {
    use super::*;

    const STATUS: &str = r#"{"BackendState":"Running","Peer":{
  "nodekey:aa":{"HostName":"takeuchi-nas","DNSName":"takeuchi-nas.tail1234.ts.net.",
    "TailscaleIPs":["100.100.1.5","fd7a:115c:a1e0::5"]},
  "nodekey:bb":{"HostName":"phone","DNSName":"phone.tail1234.ts.net.",
    "TailscaleIPs":["100.100.1.9"]}}}"#;

    fn peer(host: &str) -> Option<IpAddr> {
        peer_addr_in_status(STATUS, host)
    }

    #[test]
    fn rewrites_the_host_to_the_peer_ip_and_keeps_port_and_path() {
        assert_eq!(
            rewrite_nas_url("http://takeuchi-nas:5005/infodesk/", peer).unwrap(),
            "http://100.100.1.5:5005/infodesk/"
        );
    }

    #[test]
    fn https_bad_urls_and_outsiders_are_refused() {
        assert_eq!(
            rewrite_nas_url("https://takeuchi-nas:5006/infodesk/", peer),
            Err(NAS_HTTPS_MESSAGE)
        );
        assert_eq!(rewrite_nas_url("not a url", peer), Err(NAS_URL_MESSAGE));
        for raw in ["http://192.168.1.10:5005/x/", "http://nas.local:5005/x/"] {
            assert_eq!(rewrite_nas_url(raw, peer), Err(NAS_NOT_IN_TAILNET_MESSAGE), "{raw}");
        }
    }
}
```

Move the three tests `a_peer_with_null_ips_does_not_hide_the_others`, `a_peer_is_found_by_name_or_tailscale_ip`, `hosts_outside_the_tailnet_are_refused` from `app/src-tauri/src/tailnet.rs` into this module (they use `peer_addr_in_status` and the fixture; use `STATUS` in place of `FAKE_STATUS`).

- [ ] **Step 2: Run to see them fail**

Run: `cargo test -p infodesk-core sync::nas`
Expected: compile errors.

- [ ] **Step 3: Implement**

At the top of `nas.rs`:
- Copy the constants listed under Interfaces from `app/src-tauri/src/sync/mod.rs` with their doc comments (keep values byte for byte).
- Move `null_as_default`, `PeersJson`, `PeerJson` (with its `impl`) and `peer_addr_in_status` from `app/src-tauri/src/tailnet.rs`, unchanged. Imports: `use std::net::IpAddr; use serde::Deserialize;`.
- Add:

```rust
/// NAS の URL のホストを、tailnet の端末の Tailscale の IP に書き換える。ポートとパスはそのまま。
/// `peer` はホスト名から端末の IP を探す。tailnet の外のホストには何も送らない。
pub fn rewrite_nas_url(
    raw: &str,
    peer: impl Fn(&str) -> Option<IpAddr>,
) -> Result<String, &'static str> {
    let mut url = reqwest::Url::parse(raw).map_err(|_| NAS_URL_MESSAGE)?;
    if url.scheme() == "https" {
        return Err(NAS_HTTPS_MESSAGE);
    }
    let host = url.host_str().ok_or(NAS_NOT_IN_TAILNET_MESSAGE)?.to_string();
    let ip = peer(&host).ok_or(NAS_NOT_IN_TAILNET_MESSAGE)?;
    url.set_ip_host(ip).map_err(|_| NAS_NOT_IN_TAILNET_MESSAGE)?;
    Ok(url.to_string())
}
```

- [ ] **Step 4: Point the app at core**

In `app/src-tauri/src/sync/mod.rs`, replace the definitions of the moved constants with one re-export (keep the app-only constants such as `TAILNET_LOGIN_MESSAGE`, `TAILNET_APPROVAL_MESSAGE`, `NAS_CHECK_OK`, `FALLBACK_DEVICE_NAME`, `REAUTH_MESSAGE`, `SETTING_NAS_PASSWORD_SET`, `LEGACY_NAS_KEYS`):

```rust
pub use infodesk_core::sync::nas::{
    BACKEND_GDRIVE, BACKEND_NAS, NAS_HTTPS_MESSAGE, NAS_INCOMPLETE_MESSAGE,
    NAS_NOT_IN_TAILNET_MESSAGE, NAS_PREPARE_MESSAGE, NAS_URL_MESSAGE, NETWORK_MESSAGE,
    SETTING_BACKEND, SETTING_DEVICE_NAME, SETTING_ENABLED, SETTING_LAST_COMPACTION_AT,
    SETTING_LAST_SYNC_AT, SETTING_NAS_URL, SETTING_NAS_USER, SETTING_RESET_PENDING,
    TAILNET_STOPPED_MESSAGE,
};
```

Replace the body of `url_in_tailnet` with:

```rust
fn url_in_tailnet(tailnet: &dyn crate::tailnet::Tailnet, raw: &str) -> AppResult<String> {
    infodesk_core::sync::nas::rewrite_nas_url(raw, |host| tailnet.peer_addr(host))
        .map_err(|m| m.to_string().into())
}
```

In `app/src-tauri/src/tailnet.rs`, delete the moved items and add `pub use infodesk_core::sync::nas::peer_addr_in_status;`. Keep `FAKE_STATUS` (used by `FakeTailnet`). If `null_as_default` is still used by other structs in `tailnet.rs` (e.g. the status struct at line ~167), keep a private copy there instead of deleting it. Drop the app's `url` dependency only if nothing else uses it (`grep -rn 'url::' app/src-tauri/src`).

- [ ] **Step 5: Run tests**

Run: `cargo test -p infodesk-core sync:: && cargo test -p infodesk-app`
Expected: PASS, including the app's existing `url_in_tailnet` and NAS tests.

- [ ] **Step 6: Commit**

```bash
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings
git add crates/core/src/sync app/src-tauri/src/sync/mod.rs app/src-tauri/src/tailnet.rs app/src-tauri/Cargo.toml
git commit -m "refactor: move the NAS keys, messages and tailnet host check to core"
```

---

### Task 4: Headless NAS sync

**Files:**
- Create: `crates/core/src/sync/headless.rs`
- Modify: `crates/core/src/sync/mod.rs`

**Interfaces:**
- Consumes: `sync::nas::*` (Task 3), `schedule::ScheduleConfig` (Task 1), `engine::{run_sync, SyncInput, SyncReport, SyncError, compaction_due}`, `webdav::{WebDavConfig, WebDavStore}`, `drive::{DriveStore, DriveError, MemoryDrive}`, `Store::ensure_sync_ids`.
- Produces: `pub const NAS_PASSWORD_ENV: &str = "INFODESK_NAS_PASSWORD"`; `pub const DEFAULT_SERVER_NAME: &str = "infodesk-server"`; `#[async_trait] pub trait PeerStatus: Send + Sync { async fn status_json(&self) -> Option<String>; }`; `pub struct TailscaleCli { pub bin: String }` (`Default` = `"tailscale"`); `pub struct HeadlessEnv { pub nas_password: Option<String>, pub tz: FixedOffset }` + `HeadlessEnv::from_env(tz) -> Self`; `pub type MakeDrive = dyn Fn(WebDavConfig) -> Result<Arc<dyn DriveStore>, DriveError> + Send + Sync`; `pub fn webdav(cfg: WebDavConfig) -> Result<Arc<dyn DriveStore>, DriveError>`; `pub enum SyncOutcome { Disabled, Synced(SyncReport) }`; `pub async fn sync_nas(store: &Mutex<Store>, env: &HeadlessEnv, peers: &dyn PeerStatus, make_drive: &MakeDrive, now: DateTime<Utc>) -> Result<SyncOutcome, String>`.

- [ ] **Step 1: Write the failing tests** (create the file with the test module; add `pub mod headless;` to `sync/mod.rs`)

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::sync::drive::MemoryDrive;
    use chrono::TimeZone;

    const STATUS: &str = r#"{"BackendState":"Running","Peer":{
  "nodekey:aa":{"HostName":"takeuchi-nas","DNSName":"takeuchi-nas.tail1234.ts.net.",
    "TailscaleIPs":["100.100.1.5"]}}}"#;

    struct Fixed(Option<&'static str>);
    #[async_trait]
    impl PeerStatus for Fixed {
        async fn status_json(&self) -> Option<String> {
            self.0.map(str::to_string)
        }
    }

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
        s.set_setting(SETTING_NAS_USER, "kou").unwrap();
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
        let store = nas_store("http://takeuchi-nas:5005/infodesk/");
        let (seen, make) = recording();
        store.lock().unwrap().set_setting(SETTING_ENABLED, "false").unwrap();
        let got = sync_nas(&store, &env(Some("pw")), &Fixed(Some(STATUS)), &*make, now()).await;
        assert!(matches!(got, Ok(SyncOutcome::Disabled)));
        store.lock().unwrap().set_setting(SETTING_ENABLED, "true").unwrap();
        store.lock().unwrap().set_setting(SETTING_BACKEND, BACKEND_GDRIVE).unwrap();
        let got = sync_nas(&store, &env(Some("pw")), &Fixed(Some(STATUS)), &*make, now()).await;
        assert!(matches!(got, Ok(SyncOutcome::Disabled)));
        assert!(seen.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn missing_password_is_incomplete() {
        let store = nas_store("http://takeuchi-nas:5005/infodesk/");
        let (seen, make) = recording();
        for pw in [None, Some("")] {
            let got = sync_nas(&store, &env(pw), &Fixed(Some(STATUS)), &*make, now()).await;
            assert_eq!(got.err().as_deref(), Some(NAS_INCOMPLETE_MESSAGE));
        }
        assert!(seen.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn no_tailscale_status_sends_nothing() {
        // Review Focus 3
        let store = nas_store("http://takeuchi-nas:5005/infodesk/");
        let (seen, make) = recording();
        let got = sync_nas(&store, &env(Some("pw")), &Fixed(None), &*make, now()).await;
        assert_eq!(got.err().as_deref(), Some(TAILNET_STOPPED_MESSAGE));
        assert!(seen.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_host_outside_the_tailnet_sends_nothing() {
        // Review Focus 4
        let store = nas_store("http://192.168.1.10:5005/infodesk/");
        let (seen, make) = recording();
        let got = sync_nas(&store, &env(Some("pw")), &Fixed(Some(STATUS)), &*make, now()).await;
        assert_eq!(got.err().as_deref(), Some(NAS_NOT_IN_TAILNET_MESSAGE));
        assert!(seen.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_normal_pass_connects_to_the_peer_ip_and_records_the_time() {
        let store = nas_store("http://takeuchi-nas:5005/infodesk/");
        let (seen, make) = recording();
        let got = sync_nas(&store, &env(Some("pw")), &Fixed(Some(STATUS)), &*make, now()).await;
        assert!(matches!(got, Ok(SyncOutcome::Synced(_))), "{got:?}");
        let cfg = seen.lock().unwrap()[0].clone();
        assert_eq!(cfg.url, "http://100.100.1.5:5005/infodesk/");
        assert_eq!(cfg.user, "kou");
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

`SyncOutcome` derives `Debug` for the `{got:?}` messages; `WebDavConfig` already derives `Clone`.

- [ ] **Step 2: Run to see them fail**

Run: `cargo test -p infodesk-core sync::headless`
Expected: compile errors.

- [ ] **Step 3: Implement** (top of `headless.rs`)

```rust
//! アプリなしで NAS へ1回同期する。自宅サーバーの `infodesk serve` と `infodesk sync` が使う。
//! NAS へは OS の Tailscale を通す。パスワードは環境変数から読み、DB には置かない。

use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
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
const STATUS_TIMEOUT: Duration = Duration::from_secs(10);

/// tailnet の端末一覧 (`tailscale status --json` と同じ形の JSON)。取れなければ `None`。
#[async_trait]
pub trait PeerStatus: Send + Sync {
    async fn status_json(&self) -> Option<String>;
}

/// OS の `tailscale` コマンドで端末一覧を取る。
pub struct TailscaleCli {
    pub bin: String,
}

impl Default for TailscaleCli {
    fn default() -> Self {
        Self { bin: "tailscale".to_string() }
    }
}

#[async_trait]
impl PeerStatus for TailscaleCli {
    async fn status_json(&self) -> Option<String> {
        let run = tokio::process::Command::new(&self.bin)
            .args(["status", "--json"])
            .kill_on_drop(true)
            .output();
        let out = tokio::time::timeout(STATUS_TIMEOUT, run).await.ok()?.ok()?;
        out.status
            .success()
            .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
    }
}

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
    // 保存先の切り替えの予約。サーバーは保存先の控えを持たないので、消すだけでよい。
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
    peers: &dyn PeerStatus,
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
    let status = peers
        .status_json()
        .await
        .ok_or_else(|| TAILNET_STOPPED_MESSAGE.to_string())?;
    let url = rewrite_nas_url(&p.url, |host| peer_addr_in_status(&status, host))
        .map_err(str::to_string)?;
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

- [ ] **Step 4: Run tests**

Run: `cargo test -p infodesk-core sync::headless`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings
git add crates/core/src/sync
git commit -m "feat(core): one NAS sync without the app, through the host's Tailscale"
```

---

### Task 5: CLI default path by OS and `infodesk sync`

**Files:**
- Modify: `crates/cli/src/lib.rs:15-23,168-171,493-551`

**Interfaces:**
- Consumes: `sync::headless::{sync_nas, HeadlessEnv, TailscaleCli, SyncOutcome, webdav}` (Task 4).
- Produces: `fn db_path_for(os: &str, home: Option<String>, xdg_data_home: Option<String>) -> Result<PathBuf>`; `Command::Sync`; `pub(crate) fn local_tz()` (already exists, make it `pub(crate)` for Task 6).

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
    fn sync_and_serve_are_commands() {
        assert!(matches!(parse(&["sync"]).unwrap().command, Command::Sync));
    }
```

- [ ] **Step 2: Run to see them fail**

Run: `cargo test -p infodesk default_db_path_follows_the_os sync_and_serve_are_commands`
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

Change the `--db` doc comment to: `/// DBファイルのパス。既定は macOS では ~/Library/Application Support/InfoDesk/infodesk.db、Linux では ~/.local/share/InfoDesk/infodesk.db`.

Add to `enum Command` after `Digests`:

```rust
    /// NAS へ今すぐ1回同期する (保存先が NAS のときだけ)
    Sync,
```

Add the function:

```rust
async fn sync(store: Store) -> Result<()> {
    use infodesk_core::sync::headless::{self, HeadlessEnv, SyncOutcome, TailscaleCli};
    let store = std::sync::Mutex::new(store);
    let env = HeadlessEnv::from_env(local_tz());
    match headless::sync_nas(&store, &env, &TailscaleCli::default(), &headless::webdav, Utc::now())
        .await
    {
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

- [ ] **Step 4: Run tests**

Run: `cargo test -p infodesk`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings
git add crates/cli/src/lib.rs
git commit -m "feat(cli): a Linux default database path and a sync command for the NAS"
```

---

### Task 6: `infodesk serve`

**Files:**
- Create: `crates/cli/src/serve.rs`
- Modify: `crates/cli/src/lib.rs`, `crates/cli/Cargo.toml`

**Interfaces:**
- Consumes: `schedule::*` (Task 1), `llm::{provider_from_settings, LlmProvider, LlmRole}`, `sync::headless::*` (Task 4), `collector::{http_client, fetch_all, apply_outcomes, images}`, `summarizer::{plan_item_summaries, finish_item_summary, plan_digest, finish_digest, DigestOutcome}`, `model::{Category, DigestScope}`, `period::{period_start, utc_range}`.
- Produces: `pub type LlmFactory = Box<dyn Fn(&Store, LlmRole) -> infodesk_core::Result<Box<dyn LlmProvider>> + Send + Sync>`; `pub struct Server`; `Server::new(store, tz, llm, peers, make_drive, env) -> Result<Server>`; `pub async fn tick(&mut self, now: DateTime<FixedOffset>) -> Vec<String>` (log lines, in order); `pub async fn serve(store: Store, tz: FixedOffset) -> Result<()>`; `Command::Serve`.

- [ ] **Step 1: Dependencies**

In `crates/cli/Cargo.toml` `[dependencies]`: change tokio to `tokio = { version = "1.53.2", features = ["rt-multi-thread", "macros", "signal", "time"] }` and add `reqwest = "0.13.5"`. In `[dev-dependencies]` add `async-trait = "0.1.92"`.

- [ ] **Step 2: Write the failing tests** (create `serve.rs` with the test module; add `mod serve;` to `lib.rs`)

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use chrono::TimeZone;
    use infodesk_core::sync::drive::MemoryDrive;
    use infodesk_core::sync::headless::PeerStatus;
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

    struct Peers;
    #[async_trait]
    impl PeerStatus for Peers {
        async fn status_json(&self) -> Option<String> {
            Some(r#"{"Peer":{"k":{"HostName":"nas","DNSName":"nas.t.ts.net.","TailscaleIPs":["100.1.1.1"]}}}"#.into())
        }
    }

    fn jst() -> FixedOffset {
        FixedOffset::east_opt(9 * 3600).unwrap()
    }

    fn at(h: u32, m: u32) -> DateTime<FixedOffset> {
        jst().with_ymd_and_hms(2026, 11, 2, h, m, 0).unwrap()
    }

    /// ソースはすべて無効 (通信しない)。同期は NAS、パスワードあり。
    fn server(llm: LlmFactory) -> Server {
        let store = Store::open_in_memory().unwrap();
        infodesk_core::seed::seed_defaults(&store).unwrap();
        for src in store.list_sources().unwrap() {
            store.set_source_enabled(src.id, false).unwrap();
        }
        store.set_setting(SETTING_ENABLED, "true").unwrap();
        store.set_setting(SETTING_BACKEND, BACKEND_NAS).unwrap();
        store.set_setting(SETTING_NAS_URL, "http://nas:5005/infodesk/").unwrap();
        store.set_setting(SETTING_NAS_USER, "kou").unwrap();
        let env = HeadlessEnv { nas_password: Some("pw".into()), tz: jst() };
        let drive: Box<MakeDrive> =
            Box::new(|_| Ok(Arc::new(MemoryDrive::new()) as Arc<dyn DriveStore>));
        Server::new(store, jst(), llm, Box::new(Peers), drive, env).unwrap()
    }

    fn mock_llm() -> LlmFactory {
        Box::new(|_, _| Ok(Box::new(Reply(Ok("要約".into()))) as Box<dyn LlmProvider>))
    }

    fn setting(s: &Server, key: &str) -> Option<String> {
        s.store.lock().unwrap().get_setting(key).unwrap()
    }

    #[tokio::test]
    async fn first_tick_collects_digests_and_syncs_in_order() {
        let mut s = server(mock_llm());
        let log = s.tick(at(8, 0)).await;
        let pos = |p: &str| log.iter().position(|l| l.starts_with(p)).unwrap_or_else(|| panic!("{p} in {log:?}"));
        assert!(pos("collect:") < pos("sync:"), "{log:?}");
        assert!(log.iter().any(|l| l.starts_with("digest daily 2026-11-01")), "{log:?}");
        assert!(setting(&s, "schedule.last_collect_at").is_some());
        assert_eq!(setting(&s, "schedule.last_daily_for").as_deref(), Some("2026-11-01"));
        assert!(setting(&s, SETTING_LAST_SYNC_AT).is_some());
    }

    #[tokio::test]
    async fn a_second_tick_right_after_does_nothing() {
        let mut s = server(mock_llm());
        s.tick(at(8, 0)).await;
        let log = s.tick(at(8, 1)).await;
        assert!(log.iter().all(|l| !l.starts_with("collect:")), "{log:?}");
    }

    #[tokio::test]
    async fn a_copied_database_continues_its_schedule() {
        // Review Focus 1
        let mut s = server(mock_llm());
        {
            let st = s.store.lock().unwrap();
            st.set_setting("schedule.last_collect_at", &at(7, 30).to_rfc3339()).unwrap();
            st.set_setting("schedule.last_daily_for", "2026-11-01").unwrap();
            st.set_setting("schedule.last_weekly_for", "2026-10-26").unwrap();
            st.set_setting("schedule.last_monthly_for", "2026-10-01").unwrap();
            st.set_setting("sync.device_id", "mac-device").unwrap();
        }
        let log = s.tick(at(8, 0)).await;
        assert!(log.iter().all(|l| !l.starts_with("collect:") && !l.starts_with("digest")), "{log:?}");
        assert_eq!(setting(&s, "sync.device_id").as_deref(), Some("mac-device"));
    }

    #[tokio::test]
    async fn a_missing_provider_does_not_stop_collection() {
        // Review Focus 2
        let llm: LlmFactory = Box::new(|_, _| {
            Err(infodesk_core::Error::Invalid("ANTHROPIC_API_KEY is not set".into()))
        });
        let mut s = server(llm);
        let log = s.tick(at(8, 0)).await;
        assert!(log.iter().any(|l| l.starts_with("collect:")), "{log:?}");
        assert!(log.iter().any(|l| l.contains("ANTHROPIC_API_KEY is not set")), "{log:?}");
        assert!(setting(&s, "schedule.last_collect_at").is_some());
    }

    #[tokio::test]
    async fn a_sync_error_is_logged_and_the_loop_goes_on() {
        let mut s = server(mock_llm());
        s.env.nas_password = None;
        let log = s.tick(at(8, 0)).await;
        assert!(log.iter().any(|l| l.starts_with("sync: error") && l.contains(NAS_INCOMPLETE_MESSAGE)), "{log:?}");
        assert!(setting(&s, "schedule.last_collect_at").is_some());
    }
}
```

- [ ] **Step 3: Run to see them fail**

Run: `cargo test -p infodesk serve::`
Expected: compile errors.

- [ ] **Step 4: Implement** (top of `serve.rs`)

```rust
//! `infodesk serve`: 画面なしで、アプリと同じ規則で収集・要約・同期を続ける (自宅サーバー用)。

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};

use chrono::{DateTime, FixedOffset, Local, NaiveDate, Utc};
use infodesk_core::collector::{apply_outcomes, fetch_all, http_client, images};
use infodesk_core::llm::{LlmProvider, LlmRole, provider_from_settings};
use infodesk_core::model::{Category, DigestScope, Period};
use infodesk_core::period::{period_start, utc_range};
use infodesk_core::schedule::{
    Job, LastRuns, RECENT_ATTEMPT, ScheduleConfig, TICK, all_failed_retry_base, due_jobs,
    filter_recent, job_plan, last_run_record, retry_due, should_record,
};
use infodesk_core::summarizer::{
    DigestOutcome, finish_digest, finish_item_summary, plan_digest, plan_item_summaries,
};
use infodesk_core::sync::drive::DriveStore;
use infodesk_core::sync::headless::{
    self, HeadlessEnv, MakeDrive, PeerStatus, SyncOutcome, TailscaleCli,
};
use infodesk_core::{Result, Store};

pub type LlmFactory =
    Box<dyn Fn(&Store, LlmRole) -> Result<Box<dyn LlmProvider>> + Send + Sync>;

pub struct Server {
    pub(crate) store: Mutex<Store>,
    http: reqwest::Client,
    tz: FixedOffset,
    llm: LlmFactory,
    peers: Box<dyn PeerStatus>,
    make_drive: Box<MakeDrive>,
    pub(crate) env: HeadlessEnv,
    attempts: HashMap<Job, DateTime<Utc>>,
    last_retry_at: Option<DateTime<Utc>>,
}

fn all_scopes() -> Vec<DigestScope> {
    Category::ALL.into_iter().map(DigestScope::Category).collect()
}

fn job_label(job: &Job) -> String {
    match job {
        Job::Collect => "collect".to_string(),
        Job::Daily(d) => format!("daily {d}"),
        Job::Weekly(d) => format!("weekly {d}"),
        Job::Monthly(d) => format!("monthly {d}"),
    }
}

impl Server {
    pub fn new(
        store: Store,
        tz: FixedOffset,
        llm: LlmFactory,
        peers: Box<dyn PeerStatus>,
        make_drive: Box<MakeDrive>,
        env: HeadlessEnv,
    ) -> Result<Self> {
        Ok(Self {
            store: Mutex::new(store),
            http: http_client()?,
            tz,
            llm,
            peers,
            make_drive,
            env,
            attempts: HashMap::new(),
            last_retry_at: None,
        })
    }

    fn lock(&self) -> MutexGuard<'_, Store> {
        self.store.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn get(&self, key: &str) -> Option<String> {
        self.lock().get_setting(key).ok().flatten()
    }

    /// 1回の評価。実行したことをログの行として順に返す。
    pub async fn tick(&mut self, now: DateTime<FixedOffset>) -> Vec<String> {
        let mut log = Vec::new();
        let cfg = ScheduleConfig::from_settings(|k| self.get(k));
        let last = LastRuns::from_settings(|k| self.get(k));
        let now_utc = now.with_timezone(&Utc);
        self.attempts.retain(|_, at| now_utc - *at < RECENT_ATTEMPT);
        let due = filter_recent(due_jobs(now, &cfg, &last), &self.attempts, now_utc, &cfg);
        let today = now.date_naive();
        for job in &due {
            self.attempts.insert(job.clone(), now_utc);
            let changed = match job {
                Job::Collect => self.collect(now_utc, &cfg, &mut log).await,
                _ => self.digest_job(job, today, &mut log).await,
            };
            if changed {
                self.sync(now_utc, &mut log).await;
            }
        }
        if retry_due(self.last_retry_at, now_utc) {
            self.last_retry_at = Some(now_utc);
            if self.retry_pending(&mut log).await {
                self.sync(now_utc, &mut log).await;
            }
        }
        log
    }

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
        let label = job_label(job);
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
        let got = headless::sync_nas(&self.store, &self.env, self.peers.as_ref(), &*self.make_drive, now).await;
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

/// SIGTERM か Ctrl-C まで評価を続ける。評価の途中で止めず、終わってから抜ける。
pub async fn serve(store: Store, tz: FixedOffset) -> Result<()> {
    let make_drive: Box<MakeDrive> = Box::new(headless::webdav);
    let mut server = Server::new(
        store,
        tz,
        Box::new(provider_from_settings),
        Box::new(TailscaleCli::default()),
        make_drive,
        HeadlessEnv::from_env(tz),
    )?;
    println!("serve: started");
    let mut ticker = tokio::time::interval(TICK);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    loop {
        tokio::select! {
            _ = ticker.tick() => {
                for line in server.tick(Local::now().with_timezone(&tz)).await {
                    println!("{line}");
                }
            }
            _ = term.recv() => break,
            _ = tokio::signal::ctrl_c() => break,
        }
    }
    println!("serve: stopped");
    Ok(())
}
```

Notes for the implementer:
- `self.digests` is called while `self` is borrowed immutably; `tick` takes `&mut self` only to update `attempts` and `last_retry_at`, which it does outside those calls.
- `tokio::signal::unix::signal(...)?` returns `std::io::Error`, which converts through `infodesk_core::Error::Io`.
- The `select!` body runs after the branch is chosen, so a SIGTERM during a tick is handled after the tick finishes (the spec's "finish the current step").
- `Arc` and `DriveStore` imports are used by the tests; if clippy reports them unused outside tests, move them into the test module.

- [ ] **Step 5: Wire the command** in `lib.rs`

Add to `enum Command` after `Sync`:

```rust
    /// 定期実行を続ける (自宅サーバーの systemd 用)。SIGTERM で終わる
    Serve,
```

In `run`, next to the `Sync` early return:

```rust
    if matches!(cli.command, Command::Serve) {
        return serve::serve(store, local_tz()).await;
    }
```

and `Command::Serve => unreachable!("handled before the match"),` in the match. Extend `sync_and_serve_are_commands` with `assert!(matches!(parse(&["serve"]).unwrap().command, Command::Serve));`.

- [ ] **Step 6: Run tests**

Run: `cargo test -p infodesk`
Expected: PASS.

- [ ] **Step 7: Manual smoke run on the Mac** (no network sources, sync off)

```bash
cargo run -q -p infodesk -- --db /tmp/infodesk-serve-smoke.db serve
```

Expected: prints `serve: started`, then `collect: ...`, `summaries: ...` and digest lines within 15 seconds; Ctrl-C prints `serve: stopped`. Delete `/tmp/infodesk-serve-smoke.db*` afterwards.

- [ ] **Step 8: Commit**

```bash
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings
git add crates/cli
git commit -m "feat(cli): infodesk serve runs the schedule headless for a home server"
```

---

### Task 7: systemd files and docs

**Files:**
- Create: `deploy/linux/infodesk.service`, `deploy/linux/infodesk.env.example`, `docs/15-home-server.md`, `docs/15-home-server.ja.md`
- Modify: `docs/07-settings.md`, `docs/07-settings.ja.md`, `docs/08-cli.md`, `docs/08-cli.ja.md`, `docs/14-mobile-app.md`, `docs/14-mobile-app.ja.md`, `README.md`, `README.ja.md`, `.gitignore` (only if `deploy/` is ignored)

- [ ] **Step 1: `deploy/linux/infodesk.service`**

```ini
[Unit]
Description=InfoDesk collector (headless)
After=network-online.target tailscaled.service
Wants=network-online.target

[Service]
Type=simple
User=infodesk
Group=infodesk
EnvironmentFile=/etc/infodesk/infodesk.env
ExecStart=/usr/local/bin/infodesk --db /var/lib/infodesk/infodesk.db serve
Restart=on-failure
RestartSec=60
NoNewPrivileges=yes
ProtectSystem=strict
ProtectHome=yes
ReadWritePaths=/var/lib/infodesk
PrivateTmp=yes

[Install]
WantedBy=multi-user.target
```

- [ ] **Step 2: `deploy/linux/infodesk.env.example`**

```sh
# Copy to /etc/infodesk/infodesk.env, then: sudo chown root:root and sudo chmod 600.
# Never commit the real file.

# Claude API key from the Claude Console (API Keys). Needed when
# llm.items_provider or llm.digest_provider is claude_api.
ANTHROPIC_API_KEY=

# Password of the Synology DSM user that owns the WebDAV folder.
INFODESK_NAS_PASSWORD=
```

- [ ] **Step 3: `docs/15-home-server.md`**

Header line, in the style of the other pages:

```markdown
**日本語: [15-home-server.ja.md](15-home-server.ja.md)** | ← [Mobile App](14-mobile-app.md)

# Home Server (Ubuntu, headless)
```

Sections, with these contents:
1. **What it does**: the server is the only writer; it collects, summarizes, makes digests and syncs to the NAS on the Mac app's schedule; the iPhone reads the NAS as before; the Mac app must not collect or sync while the server runs (a Mac reader mode comes later).
2. **Why the Mac must stop**: the copied database carries the Mac's sync device id; two writers with one id overwrite each other's files.
3. **Set up the server**: the runbook from spec section 8, steps 1 to 8, as commands:

```sh
sudo apt install -y build-essential pkg-config git curl
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
curl -fsSL https://tailscale.com/install.sh | sh
sudo tailscale up            # same tailnet as the NAS
sudo timedatectl set-timezone Asia/Tokyo
sudo useradd --system --home /var/lib/infodesk --create-home --shell /usr/sbin/nologin infodesk
git clone <your InfoDesk repository> && cd InfoDesk
cargo build --release -p infodesk
sudo install -m 755 target/release/infodesk /usr/local/bin/infodesk
sudo install -d -m 700 /etc/infodesk
sudo install -m 600 deploy/linux/infodesk.env.example /etc/infodesk/infodesk.env
sudoedit /etc/infodesk/infodesk.env   # fill in the two values
```

   Then: on the Mac turn off 同期する, quit InfoDesk and turn off launch at login; copy `~/Library/Application Support/jp.infodesk.app/infodesk.db` (and `infodesk.db-wal`, `infodesk.db-shm` if present) to the server as `/var/lib/infodesk/infodesk.db` owned by `infodesk`; set the providers and the device name:

```sh
db=/var/lib/infodesk/infodesk.db
sudo -u infodesk infodesk --db $db config set llm.items_provider claude_api
sudo -u infodesk infodesk --db $db config set llm.digest_provider claude_api
sudo -u infodesk infodesk --db $db config set sync.device_name "$(hostname)"
sudo -u infodesk infodesk --db $db config set sync.enabled true
sudo -u infodesk env $(sudo cat /etc/infodesk/infodesk.env | xargs) infodesk --db $db sync
sudo install -m 644 deploy/linux/infodesk.service /etc/systemd/system/
sudo systemctl daemon-reload && sudo systemctl enable --now infodesk
journalctl -u infodesk -f
```

4. **Claude API settings**: `claude_api.model` (default `claude-sonnet-5-5`), `claude_api.items_effort` (default `low`), `claude_api.digest_effort` (default empty = API default); the cost estimate from spec section 9 and the advice to check spend in the Claude Console after a week; the key lives only in `/etc/infodesk/infodesk.env`.
5. **Changing sources and topics**: `ssh` to the server and use `infodesk --db /var/lib/infodesk/infodesk.db source|topic|config ...` as `sudo -u infodesk` (link to [CLI](08-cli.md)).
6. **Troubleshooting** table: `sync: error Tailscale が停止中です` (run `tailscale status`; start `tailscaled`); `sync: error NAS が Tailscale の中に見つかりません…` (use the NAS name from the admin console); `sync: error NAS の URL、ユーザー名、パスワードを入力してください` (check `INFODESK_NAS_PASSWORD` and `sync.nas.url` / `sync.nas.user`); `summaries: error ANTHROPIC_API_KEY is not set` (fill the env file, `systemctl restart infodesk`); `claude_api 401 ... the API key was rejected` (create a new key in the Console); `claude_api 429` / `529` (temporary; digests stay pending and are retried every 15 minutes).

`docs/15-home-server.ja.md`: the same page in Japanese, header `**English: [15-home-server.md](15-home-server.md)** | ← [前: モバイルアプリ](14-mobile-app.ja.md)`, following the wording style of `docs/13-nas-setup.ja.md`.

- [ ] **Step 4: Update existing docs**

- `docs/14-mobile-app.md` header: append ` | Next: [Home Server →](15-home-server.md)`; `.ja.md`: append ` | [次: 自宅サーバー →](15-home-server.ja.md)`.
- `README.md` and `README.ja.md` docs table: add row 15 after row 14 (`| 15 | [Home Server](docs/15-home-server.md) | Run InfoDesk headless on an Ubuntu server with systemd, syncing to the NAS |` and the Japanese equivalent).
- `docs/07-settings.md` (and `.ja.md`): in the settings table, `llm.items_provider` / `llm.digest_provider` accept `claude_api` too; add rows for `claude_api.model`, `claude_api.items_effort`, `claude_api.digest_effort` with the defaults and meanings from Task 2; note that `claude_api` is not offered in the Mac app's LLM tab and needs `ANTHROPIC_API_KEY`.
- `docs/08-cli.md` (and `.ja.md`): the database path section names both defaults (macOS and Linux with `XDG_DATA_HOME`); add `sync` and `serve` to the command list and a short section for each (what they print, that `serve` stops on SIGTERM, link to [Home Server](15-home-server.md)).

- [ ] **Step 5: Check**

Run: `cargo test --workspace` and `git status` (the env example is committed; no real secrets anywhere: `git grep -n 'sk-ant' -- . ':!docs/superpowers'` must print nothing).
Expected: tests PASS; grep prints nothing.

- [ ] **Step 6: Commit**

```bash
git add deploy docs README.md README.ja.md
git commit -m "docs: run InfoDesk headless on a home server"
```

---

## After the tasks

- On the Ubuntu server: `cargo test -p infodesk-core -p infodesk` once (spec section 7), then the runbook.
- The user puts the API key into `/etc/infodesk/infodesk.env` themselves.
- One PR from `feature/headless-server` to `master`.
