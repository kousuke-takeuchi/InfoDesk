# InfoDesk headless server design

Date: 2026-11-01
Status: waiting for review
Parent designs: [2026-10-06-infodesk-design.md](2026-10-06-infodesk-design.md), [2026-10-13-infodesk-cloud-sync-design.md](2026-10-13-infodesk-cloud-sync-design.md)

## 1. Goal

Run InfoDesk on a home server (Ubuntu, no GUI) as the only writer. The server collects articles, summarizes them, makes period digests and syncs the result to the Synology NAS on a schedule, with no one logged in. Summaries can use the Claude API directly with an API key, instead of the `claude` CLI or Ollama.

### Success criteria

- After `systemctl enable --now infodesk`, the server collects at `schedule.collect_interval_minutes`, adds article summaries, finalizes day, week and month digests at the configured times, and syncs to the NAS after each collection and digest, the same as the Mac app does today.
- The iPhone app shows articles and digests that the server wrote, and read and starred marks still go both ways.
- With `llm.items_provider = claude_api` and `llm.digest_provider = claude_api`, summaries are written by `claude-sonnet-5-5` through `POST /v1/messages`. The digest's `provider` column shows `claude_api:claude-sonnet-5-5`.
- The Mac's existing database (articles, digests, read and starred marks, sources, topics, settings) runs on the server after a copy, with no data loss.
- The server sends sync data only to a device in the user's tailnet, the same rule the Mac app has.
- The API key and the NAS password are never stored in the database or printed in logs.

### Out of scope (this design)

- A read-only mode for the Mac app (sub-project 2, its own design). Until it exists, the Mac app must not collect or sync (section 8).
- Chat on the server. The chat agent stays a Mac feature.
- Google Drive as the server's storage. The server syncs to the NAS only.
- A web UI or remote control API on the server. Sources, topics and settings are changed with the existing CLI over ssh.
- Running more than one writer at a time.

## 2. Decisions

| # | Question | Decision | Why |
|---|---|---|---|
| D1 | Role of the Mac after the server runs | The server is the only writer. The Mac becomes a reader later (sub-project 2). | Keeps the one-writer rule of the sync design. |
| D2 | Storage the server syncs to | NAS (WebDAV) only | The NAS and the server are both at home. Google Drive would need its OAuth sign-in moved to a headless host. |
| D3 | Existing data | Copy the Mac's `infodesk.db` to the server as is | Keeps history, marks and settings. |
| D4 | Claude API model | `claude-sonnet-5-5` for both article summaries and digests | User choice. |
| D5 | How the server runs | A new `infodesk serve` command in the CLI, run by systemd. The schedule rules and the job flow move from the Tauri crate to `infodesk-core` so the app and the server share them. | One copy of the schedule rules (daily, weekly, monthly, pending retry). Rejected: systemd timers calling one-shot commands (the rules would be written twice and drift); the Tauri app under Xvfb (Linux build of the app, the Tailscale sidecar and a keyring are all extra work). |
| D6 | How the server reaches the NAS | The host's own Tailscale (`tailscaled`), not the in-app node | A server has a normal system service for this. No sidecar on Linux. |
| D7 | Secrets on the server | An environment file read by systemd (`EnvironmentFile=`), mode 600 | No keyring on a headless host. The database is copied between machines and must not hold secrets. |
| D8 | HTTP client for the Claude API | `reqwest` with raw HTTP, as the Ollama provider does | There is no official Anthropic SDK for Rust. |

## 3. Overview

```
Ubuntu server (systemd: infodesk.service)
 infodesk serve
  ├─ schedule loop (infodesk-core::schedule)  ── same rules as the Mac app
  ├─ jobs: collect, item summaries, digests, pending retry (infodesk-core::jobs)
  │    └─ LLM provider: claude_api ── HTTPS ──> api.anthropic.com /v1/messages
  └─ sync (infodesk-core::sync::run_sync)
       └─ WebDAV ── host tailscaled (WireGuard) ──> Synology NAS ──> iPhone app
```

## 4. Components

### 4.1 Shared schedule and jobs in `infodesk-core`

Move, without changing behavior:

- From `app/src-tauri/src/scheduler.rs`: `ScheduleConfig`, `LastRuns`, `Job`, `next_collect_at`, `due_jobs`, `filter_recent`, `retry_due`, `job_plan` and their tests. They are pure functions of settings and time. New module: `crates/core/src/schedule.rs`.
- From `app/src-tauri/src/jobs.rs`: the parts of `run_collect`, `run_item_summaries`, `run_digests`, `retry_pending` that only use the store, the collector and the LLM provider. New module: `crates/core/src/jobs.rs`. App-only parts (status events to the window, the tray, the `is_collecting` flag held in `AppState`) stay in the app and wrap the core functions.
- A core `run_loop` is not shared. The app keeps its loop (it updates window status). The server has its own small loop in the CLI crate (section 4.4). Both call the same `due_jobs`, `filter_recent`, `retry_due` and the same core jobs.

The app must behave exactly as before. Its existing tests move with the code, and the app's tests keep passing.

### 4.2 Claude API provider (`claude_api`)

New file `crates/core/src/llm/claude_api.rs`, implementing `LlmProvider`.

Request (non-streaming, raw HTTP):

```
POST https://api.anthropic.com/v1/messages
x-api-key: <ANTHROPIC_API_KEY>
anthropic-version: 2026-10-21
anthropic-beta: server-side-fallback-2026-07-01
content-type: application/json

{
  "model": "<claude_api.model>",
  "max_tokens": 16000,
  "system": "<system>",
  "messages": [{"role": "user", "content": "<prompt>"}],
  "fallbacks": "default"
}
```

- No `temperature`, `top_p`, `top_k` (rejected on `2026-10-21`) and no `thinking` field (Sonnet 5.5 runs adaptive thinking by default; `{"type": "disabled"}` is rejected).
- `fallbacks: "default"` re-runs a request that a safety classifier declines on Anthropic's recommended model, server side. For `claude-sonnet-5-5` only the `"default"` form is accepted, and only on the Claude API.
- Response: check `stop_reason` first. `refusal` is an `Error::Llm("refused: <category>")`. `max_tokens` is an `Error::Llm("truncated")`. Otherwise join the `text` blocks of `content` in order and ignore every other block type (`thinking`, `fallback`). Pass the result through `clean_output`.
- HTTP errors: 401 and 403 give `Error::Llm` with a fixed message that does not include the key. 429, 500, 529 and network errors give `Error::Llm` too; the existing pending-digest retry and the next collection handle a later attempt. No retry loop inside the provider.
- Timeout: 300 seconds per request.
- `name()` returns `claude_api:<model>`.

Settings (new keys, seeded with defaults):

| Key | Default | Meaning |
|---|---|---|
| `claude_api.model` | `claude-sonnet-5-5` | Model id. Same character rule as `claude.model`. |

`provider_from_settings` accepts a third value, `claude_api`, for `llm.items_provider` and `llm.digest_provider`. The key comes only from the environment variable `ANTHROPIC_API_KEY`. If it is missing or empty, creating the provider fails with `Error::Invalid("ANTHROPIC_API_KEY is not set")`.

The Mac app's LLM tab does not offer `claude_api` in this design (the Mac will not summarize after the move). The core code works there too if the key is set in the app's environment, so adding it to the app later is a UI change only.

### 4.3 Headless sync

New module `crates/core/src/sync/headless.rs` with one function the server calls after each collection and each digest run:

```rust
pub async fn sync_nas(store: &Mutex<Store>, env: &HeadlessEnv, now: DateTime<Utc>) -> Result<SyncReport, SyncError>
```

- Reads `sync.enabled`, `sync.backend`, `sync.nas.url`, `sync.nas.user`, `sync.last_compaction_at`, `schedule.daily_time` from the store. Returns without syncing when sync is off or the backend is not `nas`.
- The password comes from `INFODESK_NAS_PASSWORD`. Missing means a sync error, logged once per run.
- Tailnet check, the same rule as the app: run `tailscale status --json`, collect each peer's short name, full DNS name (with and without the final dot), host name and Tailscale IPs, and accept the URL host only when it matches one. Then connect to that peer's Tailscale IPv4 address with the same port and path. Any other host is refused with the app's message 「NAS が Tailscale の中に見つかりません。NAS の名前を確かめてください」 and nothing is sent. The matching function is pure and moves to core so the app uses the same one (`url_in_tailnet` today takes the app's `Tailnet` trait; it is split into a pure matcher over a peer list plus the app's adapter).
- Builds `WebDavConfig` with `proxy: None` (host Tailscale routes the traffic) and calls the existing `run_sync` with `SyncInput` from `ensure_sync_ids`, `sync.device_name`, the compaction rule (`compaction_due`) and the server's time zone.
- Writes `sync.last_sync_at` and `sync.last_compaction_at` the same way the app does.
- One sync at a time: the server loop runs jobs in sequence, so no lock beyond that is needed.

### 4.4 CLI changes

- `infodesk serve`: runs until SIGTERM or SIGINT. Every 60 seconds (the app's `TICK`): read the schedule config and last runs from the store, compute due jobs with the shared functions, run them in order, sync after a collection or a digest job, and run `retry_pending` when `retry_due` says so. Logs one line per job and per sync to stdout (systemd sends it to the journal). On SIGTERM it finishes the current step and exits; a sync in progress is allowed to finish.
- `infodesk sync`: one NAS sync now, for checks and for the migration runbook. Prints the `SyncReport` counts or the error.
- Default database path by OS: macOS keeps `~/Library/Application Support/InfoDesk/infodesk.db`. Linux uses `$XDG_DATA_HOME/InfoDesk/infodesk.db`, or `~/.local/share/InfoDesk/infodesk.db` when `XDG_DATA_HOME` is unset. `--db` still overrides it. The help text for `--db` names both.
- The time zone is the host's local time zone (the app uses the Mac's). The runbook sets the server to `Asia/Tokyo`.

### 4.5 Deployment files

New folder `deploy/linux/`:

- `infodesk.service`: a systemd system unit. `User=infodesk`, `ExecStart=/usr/local/bin/infodesk --db /var/lib/infodesk/infodesk.db serve`, `EnvironmentFile=/etc/infodesk/infodesk.env`, `Restart=on-failure`, `RestartSec=60`, `After=network-online.target tailscaled.service`, `Wants=network-online.target`, plus basic hardening (`NoNewPrivileges=yes`, `ProtectSystem=strict`, `ReadWritePaths=/var/lib/infodesk`, `PrivateTmp=yes`).
- `infodesk.env.example`: `ANTHROPIC_API_KEY=` and `INFODESK_NAS_PASSWORD=` with comments. The real file is `/etc/infodesk/infodesk.env`, owner root, mode 600.

The binary is built on the server with `cargo build --release -p infodesk` (no cross build in this design).

## 5. Data flow per collection

1. `serve` sees `Job::Collect` due, runs core collect (sources, images), records `schedule.last_collect_at`.
2. Runs item summaries for today's top articles per category with `llm.items_provider`.
3. Runs `sync_nas`.
4. When a day, week or month job is due, runs core digests with `llm.digest_provider`, records the `schedule.last_*_for` key, then runs `sync_nas`.
5. Every 15 minutes at most, retries pending digests.

This is the order the app uses today; the move to core keeps it.

## 6. Error handling

- A failing source, LLM call or sync never stops the loop. Each failure is one log line and is retried by the existing rules (next collection, pending retry, next sync).
- A store error at the start of a tick is logged and the tick is skipped.
- Secrets: the API key, the NAS password and the `Authorization` header are never logged. `WebDavConfig` already redacts its secrets in `Debug`; the new provider and `HeadlessEnv` do the same, with tests.
- If `tailscale` is not installed or `tailscaled` is not running, the sync fails with 「Tailscale が停止中です」 (the app's message) and nothing is sent.

## 7. Testing

- Moved schedule and job tests run in `infodesk-core`. The app's remaining tests still pass.
- `claude_api` provider against a local mock HTTP server (the Ollama provider's test pattern): request headers and body shape (no sampling fields, `fallbacks`, version header), text-block joining with `thinking` and `fallback` blocks present, `refusal`, `max_tokens`, 401 without the key in the message, missing `ANTHROPIC_API_KEY`, `Debug` redaction.
- Tailnet matcher: the app's existing cases, run against the pure function, plus parsing a recorded `tailscale status --json` fixture.
- `sync_nas` against the existing WebDAV test server: sync off, wrong backend, missing password, host not in tailnet (nothing sent), a normal pass.
- `serve` loop: one tick with a fake clock and a mock provider runs collect, summaries and sync in order; SIGTERM ends the loop.
- Linux default path: unit test with `XDG_DATA_HOME` set and unset.
- The repository has no CI. Run `cargo test -p infodesk-core -p infodesk` on the Ubuntu server once, in addition to the Mac, to catch Linux-only failures (paths, `tailscale` parsing).
- Manual check on the server: `infodesk sync` once, then `systemctl start infodesk` and watch `journalctl -u infodesk -f` through one collection; confirm the iPhone app shows the new articles.

## 8. Migration runbook (goes into `docs/15-home-server.md`, English and Japanese)

1. On the server: install build tools and Rust, Tailscale (`tailscale up`, same tailnet as the NAS), set the time zone to `Asia/Tokyo`, create the `infodesk` user and `/var/lib/infodesk`.
2. Build and install `infodesk` to `/usr/local/bin`.
3. Write `/etc/infodesk/infodesk.env` with the API key and the NAS password (mode 600).
4. On the Mac: turn off "同期する", quit InfoDesk, and turn off launch at login. From now on the Mac app must not run its collector or sync until sub-project 2, because the copied database carries the Mac's sync device id and two writers with one id would overwrite each other's files.
5. Copy `~/Library/Application Support/jp.infodesk.app/infodesk.db` (and `-wal`/`-shm` if present, with the app quit) to `/var/lib/infodesk/infodesk.db`, owner `infodesk`.
6. On the server: `infodesk config` to set `llm.items_provider` and `llm.digest_provider` to `claude_api`, `sync.device_name` to the server's name, and `sync.enabled` to `true` (backend, NAS URL and user are already in the copied settings).
7. `sudo -u infodesk infodesk --db /var/lib/infodesk/infodesk.db sync` once and check the counts.
8. `systemctl enable --now infodesk`, then watch the journal through one collection.

## 9. Cost estimate (Claude API, for the user's decision)

Assumption: about 140 article summaries a day (7 categories × `summary.top_n` 20), about 3,000 input tokens and 500 output tokens each including thinking, plus a few digests. At Sonnet 5.5 prices ($2 / $10 per 1M), that is about $0.84 input + $0.70 output, so about $1.5 a day, or roughly $45 a month. Thinking tokens are billed as output, so the real figure depends on how much the model thinks. Lowering `summary.top_n` lowers it in proportion.

## 10. Open question for review

- Effort for article summaries. Sonnet 5.5 defaults to `high` effort, which can spend many thinking tokens on a short summary. Option: add `claude_api.effort` (empty = API default) and set `output_config.effort` to `low` for article summaries, keeping the default for digests. This is a quality-for-cost trade; the user decides. The design above leaves it out until decided.
