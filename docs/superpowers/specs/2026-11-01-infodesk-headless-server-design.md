# InfoDesk on the home k3s cluster (headless) design

Date: 2026-11-01
Status: waiting for review (revised the same day: the target is the user's k3s cluster with Argo Workflows, not a systemd host)
Parent designs: [2026-10-06-infodesk-design.md](2026-10-06-infodesk-design.md), [2026-10-13-infodesk-cloud-sync-design.md](2026-10-13-infodesk-cloud-sync-design.md)

## 1. Goal

Run InfoDesk on the user's home k3s cluster, with no GUI, as the only writer. An Argo CronWorkflow runs InfoDesk every few minutes; each run collects articles when due, summarizes them, makes period digests when due and syncs the result to the Synology NAS. Summaries can use the Claude API directly with an API key, instead of the `claude` CLI or Ollama.

### Success criteria

- After the manifests are applied, the CronWorkflow `infodesk-tick` runs every 5 minutes. Over a day it collects at `schedule.collect_interval_minutes`, adds article summaries, finalizes day, week and month digests at the configured times, and syncs to the NAS after each collection and digest, the same rules the Mac app uses today. Each run's result and log are visible in the Argo UI.
- The iPhone app shows articles and digests that the cluster wrote, and read and starred marks still go both ways.
- With `llm.items_provider = claude_api` and `llm.digest_provider = claude_api`, summaries are written by `claude-sonnet-5-5` through `POST /v1/messages`. The digest's `provider` column shows `claude_api:claude-sonnet-5-5`.
- The Mac's existing database (articles, digests, read and starred marks, sources, topics, settings) runs on the cluster after a copy, with no data loss.
- The cluster sends sync data only to a private LAN address (section 4.3).
- The API key and the NAS password are never stored in the database, in the image, in a manifest in git, or printed in logs.

### Out of scope (this design)

- A read-only mode for the Mac app (sub-project 2, its own design). Until it exists, the Mac app must not collect or sync (section 8).
- Chat on the server. The chat agent stays a Mac feature.
- Google Drive as the cluster's storage. The cluster syncs to the NAS only.
- A web UI or remote control API. Sources, topics and settings are changed with the existing CLI, run in a pod (section 4.5).
- Running more than one writer at a time.
- Syncing the Secret from Vault. The Secret is created with `kubectl` for now (D7).

## 2. The cluster (observed 2026-11-01)

- k3s, two nodes: `takeuchi-nuc11atkc2` (192.168.0.177, control plane, always on, runs almost every service) and `gpu-wsl` (WSL inside a Windows PC at 192.168.0.175, tainted `dedicated=gpu:NoSchedule`).
- Argo Workflows v4.0.6. Other projects schedule work with CronWorkflows (`timezone: Asia/Tokyo`, `concurrencyPolicy: Forbid`). No Argo CD; manifests are applied with `kubectl apply`.
- Private image registry `registry.umafolio.site`; the nodes already pull from it.
- Storage class: `local-path` only (a volume is bound to one node).
- The Synology NAS answers WebDAV at `http://192.168.0.151:5005/` and pods can reach it. Pods cannot resolve the name `takeuchi-nas`.
- No Tailscale in the cluster.

## 3. Decisions

| # | Question | Decision | Why |
|---|---|---|---|
| D1 | Role of the Mac after the move | The cluster is the only writer. The Mac becomes a reader later (sub-project 2). | Keeps the one-writer rule of the sync design. |
| D2 | Storage the cluster syncs to | NAS (WebDAV) only | The NAS is on the same LAN. Google Drive would need its OAuth sign-in moved to a headless host. |
| D3 | Existing data | Copy the Mac's `infodesk.db` to the cluster as is | Keeps history, marks and settings. |
| D4 | Claude API model | `claude-sonnet-5-5` for both article summaries and digests | User choice. |
| D5 | How it runs | An Argo CronWorkflow every 5 minutes runs `infodesk tick`: one evaluation of the schedule, then exit. The schedule rules move from the Tauri crate to `infodesk-core` so the app and the cluster share them. | User choice: the same tool as the cluster's other scheduled work, with run history and logs in the Argo UI. Rejected: a Deployment running a loop (no run history in Argo); systemd on a host (the user wants the cluster's management tools). Cost: collection can be at most 5 minutes late. |
| D6 | How the cluster reaches the NAS | Directly over the home LAN, `http://192.168.0.151:5005/infodesk/`. The host of `sync.nas.url` must be a private IPv4 address. | User choice. Simpler than a Tailscale sidecar. Trade-off: WebDAV over plain HTTP inside the home LAN. The private-address rule keeps a typo from sending the password over the internet. |
| D7 | Secrets | A Kubernetes Secret `infodesk-env` in namespace `infodesk`, created by the user with `kubectl create secret`, read with `envFrom` | No secret in git or in the image. The cluster's Vault sync script is written for one other project; reusing it is a later step. |
| D8 | HTTP client for the Claude API | `reqwest` with raw HTTP, as the Ollama provider does | There is no official Anthropic SDK for Rust. |
| D9 | Effort | `low` for article summaries, the API default for digests. Both are settings. | User choice. A short summary does not need deep thinking, and thinking tokens are billed as output. |
| D10 | Node and volume | Every InfoDesk pod runs on `takeuchi-nuc11atkc2` (nodeSelector). The database lives on a 2 GiB `local-path` PVC. | The NUC is always on; the GPU node is a Windows PC that may sleep and is reserved for GPU work. `local-path` binds the volume to the node of the first pod. |
| D11 | Image | Built from a `Dockerfile` in the repository on the WSL node's Docker (amd64), pushed as `registry.umafolio.site/infodesk:<git short sha>`. Base images `rust:1.97-bookworm` (build) and `debian:bookworm-slim` (run). | Both nodes are amd64; the Mac is arm64. reqwest uses rustls, so the runtime image needs only `ca-certificates`. |

## 4. Components

### 4.1 Shared schedule rules in `infodesk-core`

Move the schedule rules, without changing behavior, from `app/src-tauri/src/scheduler.rs` to a new module `crates/core/src/schedule.rs`: `ScheduleConfig`, `LastRuns`, `Job`, `next_collect_at`, `due_jobs`, `filter_recent`, `retry_due`, `job_plan`, `RECENT_ATTEMPT`, the tick interval, and their tests. They are pure functions of settings and time. Add small pure helpers that the app's `run_job` and `run_collect` use today inline:

- `LastRuns::from_settings`: reads the `schedule.last_*` keys (today `read_last_runs` in the app).
- `last_run_record(job)` and `should_record(job, generation_failed)`: which key a finished digest job writes, and the rule that a weekly or monthly job with a failed generation is not recorded.
- `all_failed_retry_base(now, interval)`: the `schedule.last_collect_at` value written when every source failed, so the next collection comes about 10 minutes later.

The job flow itself (`app/src-tauri/src/jobs.rs`) stays in the app. It is tied to the app's window status, events, sync requests and locking. The CLI has its own short job flow (section 4.4) that calls the same core building blocks (`collector::fetch_all`, `apply_outcomes`, `summarizer::plan_item_summaries` / `finish_item_summary`, `plan_digest` / `finish_digest`) and the same schedule rules. Decision (2026-11-01, while planning): moving the job flow was dropped because it would mean rewriting the app's 955-line job module around a new interface for little gain; the rules, which are the part that must not drift, are shared.

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
  "output_config": {"effort": "<effort>"},
  "fallbacks": "default"
}
```

- `output_config` is sent only when the effort setting for the role is not empty. For article summaries the default is `low`; for digests the default is empty (the API default).
- No `temperature`, `top_p`, `top_k` (rejected on `2026-10-21`) and no `thinking` field (Sonnet 5.5 runs adaptive thinking by default; `{"type": "disabled"}` is rejected).
- `fallbacks: "default"` re-runs a request that a safety classifier declines on Anthropic's recommended model, server side. For `claude-sonnet-5-5` only the `"default"` form is accepted, and only on the Claude API.
- Response: check `stop_reason` first. `refusal` is an `Error::Llm("refused (<category>)")`. `max_tokens` is an `Error::Llm("truncated")`. Otherwise join the `text` blocks of `content` in order and ignore every other block type (`thinking`, `fallback`). Pass the result through `clean_output`.
- HTTP errors: 401 and 403 give `Error::Llm` with a fixed message that does not include the key. 429, 500, 529 and network errors give `Error::Llm` too; the existing pending-digest retry and the next collection handle a later attempt. No retry loop inside the provider.
- Timeout: 300 seconds per request.
- `name()` returns `claude_api:<model>`.

Settings (new keys, seeded with defaults):

| Key | Default | Meaning |
|---|---|---|
| `claude_api.model` | `claude-sonnet-5-5` | Model id. Same character rule as `claude.model`. |
| `claude_api.items_effort` | `low` | Effort for article summaries: `low`, `medium`, `high`, `xhigh`, `max`, or empty for the API default. |
| `claude_api.digest_effort` | empty | Effort for day, week and month digests. Same values. |

An effort value outside the list makes creating the provider fail with `Error::Invalid`, so a typo is reported at once instead of as an API 400 on every call.

`provider_from_settings` accepts a third value, `claude_api`, for `llm.items_provider` and `llm.digest_provider`, and passes the effort key for the role. The key comes only from the environment variable `ANTHROPIC_API_KEY`. If it is missing or empty, creating the provider fails with `Error::Invalid("ANTHROPIC_API_KEY is not set")`.

The Mac app's LLM tab does not offer `claude_api` in this design. The core code works there too if the key is set in the app's environment, so adding it to the app later is a UI change only.

### 4.3 Headless NAS sync over the LAN

New module `crates/core/src/sync/headless.rs`:

```rust
pub async fn sync_nas(
    store: &Mutex<Store>,
    env: &HeadlessEnv,          // NAS password from INFODESK_NAS_PASSWORD, time zone
    make_drive: &MakeDrive,     // WebDAV in production, MemoryDrive in tests
    now: DateTime<Utc>,
) -> Result<SyncOutcome, String> // Disabled | Synced(SyncReport); Err is a user-facing message
```

- Reads `sync.enabled`, `sync.backend`, `sync.nas.url`, `sync.nas.user`, `sync.device_name`, `sync.last_compaction_at`, `schedule.daily_time`. Returns `Disabled` when sync is off or the backend is not `nas`. Clears `sync.reset_pending` if set (the cluster keeps no cached storage client).
- The password comes from `INFODESK_NAS_PASSWORD`. Missing or empty: the app's message 「NAS の URL、ユーザー名、パスワードを入力してください」.
- LAN rule: the URL must be `http://`, and its host must be an IPv4 literal in `10.0.0.0/8`, `172.16.0.0/12` or `192.168.0.0/16`. A name, a public address or `https://` is refused with 「NAS の URL は LAN の IP アドレスで書いてください (例: http://192.168.0.151:5005/infodesk/)」 and nothing is sent. Names are refused because pods cannot resolve the NAS name and a name could resolve outside the LAN.
- Builds `WebDavConfig { proxy: None, .. }` and calls the existing `run_sync` with `SyncInput` from `ensure_sync_ids`, the device name (default `infodesk-server`), the compaction rule (`compaction_due`) and the time zone.
- Writes `sync.last_sync_at` and `sync.last_compaction_at` the same way the app does.
- Messages and setting keys the app and this module share move to `crates/core/src/sync/nas.rs`; the app re-exports them. The app's Tailscale code is not touched.

### 4.4 CLI changes

- `infodesk tick`: one evaluation, then exit. Reads the schedule config and last runs, computes due jobs with the shared functions, runs them in order (collect with item summaries; day, week, month digests), syncs after a collection or digest job, and retries pending digests when `retry_due` says so. Prints one line per step. Exit code 0 even when a source, an LLM call or a sync failed (those are retried by the rules, and a red run every 5 minutes would hide real breakage); exit code 1 only when the database cannot be opened or read.
- State that the app keeps in memory between ticks is kept in the database, because each `tick` is a new process: `schedule.attempts` (JSON object, job label to RFC 3339 time of the last attempt, used by `filter_recent`) and `schedule.last_retry_at` (used by `retry_due`).
- `infodesk sync`: one NAS sync now, for checks and for the migration runbook. Prints the `SyncReport` counts or the error.
- Default database path by OS: macOS keeps `~/Library/Application Support/InfoDesk/infodesk.db`. Linux uses `$XDG_DATA_HOME/InfoDesk/infodesk.db`, or `~/.local/share/InfoDesk/infodesk.db` when `XDG_DATA_HOME` is unset. `--db` still overrides it.
- The time zone is the process's local time zone. The pods set `TZ=Asia/Tokyo` and the image includes `tzdata`.

### 4.5 Container image and manifests

- `Dockerfile` (repository root) and `.dockerignore`: build `infodesk` with `cargo build --release -p infodesk --locked` in `rust:1.97-bookworm`; copy it into `debian:bookworm-slim` with `ca-certificates` and `tzdata`; run as a non-root user (uid 10001); `ENTRYPOINT ["infodesk"]`.
- `deploy/k8s/` (applied with `kubectl apply -f deploy/k8s/`):
  - `namespace.yaml`: namespace `infodesk`.
  - `pvc.yaml`: PVC `infodesk-data`, `local-path`, 2 GiB, `ReadWriteOnce`.
  - `workflowtemplate.yaml`: WorkflowTemplate `infodesk-tick` with the one container: image `registry.umafolio.site/infodesk:<tag>`, args `--db /data/infodesk.db tick`, `envFrom: secretRef infodesk-env`, `env TZ=Asia/Tokyo`, volume `infodesk-data` at `/data`, `nodeSelector kubernetes.io/hostname: takeuchi-nuc11atkc2`, `securityContext` runAsUser/fsGroup 10001, small requests (50m CPU, 64 Mi), `activeDeadlineSeconds: 3000` (a collection with 140 summaries can take many minutes).
  - `cronworkflow.yaml`: CronWorkflow `infodesk-tick` with `workflowTemplateRef: infodesk-tick`, schedule `*/5 * * * *`, `timezone: Asia/Tokyo`, `concurrencyPolicy: Forbid` (the next run waits while one is running), `startingDeadlineSeconds: 120`, history 3 succeeded / 5 failed.
  - `run-once.yaml`: a Workflow with `generateName: infodesk-tick-manual-` and the same `workflowTemplateRef`, for a run by hand with `kubectl create -f` (the `argo` CLI is not installed on the cluster hosts; the Argo UI "Submit" works too).
  - `rbac.yaml`: ServiceAccount `infodesk` and Role/RoleBinding `argo-workflow-executor`, the same rules as in namespace `umafolio` (pods and pods/exec, pods/log, `argoproj.io` workflowtaskresults).
  - `shell.yaml`: a Pod `infodesk-shell` (same image, same volume, node and Secret, command `sleep infinity`), applied only when the user wants to run CLI commands; deleted afterwards.
  - `secret.example.yaml`: documentation only, never applied with real values; the runbook creates the Secret with `kubectl create secret generic infodesk-env --from-literal=...`.

## 5. Data flow per run

1. Argo starts a pod from the template; `infodesk tick` opens `/data/infodesk.db` and evaluates the schedule.
2. If collection is due: fetch sources, save, record `schedule.last_collect_at`, summarize today's top articles with `llm.items_provider`, then `sync_nas`.
3. If a day, week or month job is due: make digests with `llm.digest_provider`, record the `schedule.last_*_for` key (except a failed weekly or monthly), then `sync_nas`.
4. At most every 15 minutes, retry pending digests; sync if any were pending.
5. Write `schedule.attempts` and `schedule.last_retry_at`, exit.

This is the order the app uses; the shared rules keep it.

## 6. Error handling

- A failing source, LLM call or sync never fails the run. Each failure is one log line and is retried by the existing rules (next collection, pending retry, next sync).
- A database that cannot be opened or read fails the run (exit 1), so Argo shows it red.
- A run cut off by `activeDeadlineSeconds` leaves the database consistent (SQLite transactions); the next run continues from the recorded keys.
- Secrets: the API key, the NAS password and request headers are never logged. `WebDavConfig` already redacts its secrets in `Debug`; the new provider and `HeadlessEnv` do the same, with tests.

## 7. Testing

- Moved schedule tests run in `infodesk-core`; the app's remaining tests still pass.
- `claude_api` provider against a local mock HTTP server: request headers and body shape, effort per role, invalid effort, text-block joining with `thinking` and `fallback` blocks present, `refusal`, `max_tokens`, 401 without the key in the message, missing `ANTHROPIC_API_KEY`, `Debug` redaction.
- LAN rule: private IPv4 literals accepted; names, public addresses, `https://`, IPv6 refused.
- `sync_nas` with `MemoryDrive`: sync off, wrong backend, missing password, a refused URL (nothing sent), a normal pass.
- `tick`: one run on a fresh database collects, digests and syncs in order; a second run right after does nothing; a copied database continues its schedule; attempts survive between runs; a missing API key does not stop collection.
- Linux default path: unit test with `XDG_DATA_HOME` set and unset.
- The repository has no CI. The image build runs the release build on Linux; `cargo test` also runs once in the build container (`docker build --target test`).
- Manual check: `kubectl create -f deploy/k8s/run-once.yaml`, read the pod log, confirm the iPhone app shows new articles.

## 8. Migration runbook (goes into `docs/15-home-server.md`, English and Japanese)

1. Build and push the image from the WSL node (`docker build`, `docker push registry.umafolio.site/infodesk:<sha>`).
2. `kubectl apply -f deploy/k8s/namespace.yaml -f deploy/k8s/pvc.yaml -f deploy/k8s/rbac.yaml`.
3. The user creates the Secret: `kubectl -n infodesk create secret generic infodesk-env --from-literal=ANTHROPIC_API_KEY=... --from-literal=INFODESK_NAS_PASSWORD=...` (typed by the user, not stored in a file).
4. On the Mac: turn off "同期する", quit InfoDesk, turn off launch at login. From now on the Mac app must not run its collector or sync until sub-project 2, because the copied database carries the Mac's sync device id and two writers with one id would overwrite each other's files.
5. Make one consistent file: `sqlite3 ~/Library/Application\ Support/jp.infodesk.app/infodesk.db ".backup /tmp/infodesk.db"`.
6. Apply `shell.yaml`, `kubectl cp /tmp/infodesk.db infodesk/infodesk-shell:/data/infodesk.db`, fix ownership if needed.
7. In the shell pod: `infodesk --db /data/infodesk.db config set` for `llm.items_provider claude_api`, `llm.digest_provider claude_api`, `sync.nas.url http://192.168.0.151:5005/infodesk/`, `sync.device_name infodesk-k3s`; then `infodesk --db /data/infodesk.db sync` once and check the counts.
8. Delete the shell pod, apply `workflowtemplate.yaml`, run it once with `run-once.yaml` and read the log, then apply `cronworkflow.yaml`.
9. Ask the user to fix the NAS's address (DHCP reservation for 192.168.0.151) if it is not fixed already.

## 9. Cost estimate (Claude API, for the user's decision)

Assumption: about 140 article summaries a day (7 categories × `summary.top_n` 20), about 3,000 input tokens and 500 output tokens each including thinking, plus a few digests. At Sonnet 5.5 prices ($2 / $10 per 1M), that is about $0.84 input + $0.70 output, so about $1.5 a day, or roughly $45 a month. Thinking tokens are billed as output, so the real figure depends on how much the model thinks; `low` effort for article summaries (D9) keeps that part small. Lowering `summary.top_n` lowers the cost in proportion.

After the first week, check the real spend in the Claude Console and the quality of the summaries. If summaries are too thin, raise `claude_api.items_effort` to `medium`.
