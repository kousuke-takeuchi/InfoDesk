# InfoDesk Digest Editions (朝刊 / 夕刊) Design

Date: 2026-11-02. Status: approved by the user (decisions below are fixed).

## Goal

Today's day digest is not made until the next morning (07:00), so the 今日 tab on the
iPhone shows yesterday's digest all day with the note 「今日の分は明日の朝 7 時ごろにできます」.
This change adds a second automatic run of the day digest in the evening and names the
two versions like a newspaper:

- **夕刊 (evening edition)**: at `schedule.evening_time` (default 17:00) the scheduler makes
  today's day digest from the articles collected so far.
- **朝刊 (morning edition)**: at `schedule.daily_time` (default 07:00) the scheduler always
  rebuilds the previous day's day digest, which now covers the whole day. This version is
  the final one; week and month digests are built from it as before.

Both apps (iPhone and Mac) show the newest edition on the 今日 tab with an edition heading,
and both let the reader step through older editions, weeks and months.

## Decisions

| # | Topic | Decision |
|---|---|---|
| 1 | Evening run | New setting `schedule.evening_time`, default `17:00`, in `DEFAULT_SETTINGS`. The first scheduler evaluation at or after that time makes today's day digest (`Job::Evening(today)`), then syncs. |
| 2 | Missed evening | Not made up. An evening that passed while the server or the Mac was down gets no evening edition; the next morning's daily job makes the final version. |
| 3 | Morning run | The daily job (`Job::Daily(yesterday)`, `schedule.daily_time`) always regenerates the day digest (`force = true`), because the evening edition already created it. Then it rebuilds the current week and month as today. |
| 4 | Week and month | Unchanged. Built from day digests and rebuilt by the daily job. The evening job does not rebuild them. |
| 5 | Sync format | Unchanged. Digest files already carry `created_at` per entry. |
| 6 | Shared rules | `crates/core/src/schedule.rs` owns the new job, its record key and its retry rule. The CLI `tick` and the Mac writer scheduler call the same functions (`due_jobs`, `job_plan`, `last_run_record`, `should_record`). |
| 7 | Record key | `schedule.last_evening_for` (YYYY-MM-DD of the day the evening edition was made for). `schedule.attempts` key `evening:<date>` in the CLI. |
| 8 | Evening retries | The evening job is always recorded. Scopes that failed become `pending` digests and the existing pending retry (at most once per 15 minutes) finishes them. A scope that already had a finished digest keeps it (the store never overwrites a `done` row with a failure). |
| 9 | Morning retries | The daily job is no longer recorded when any scope fails. Because the evening edition is a `done` row, a failed rebuild would never be retried by the pending retry. The whole daily job runs again after `RECENT_ATTEMPT` (15 minutes), like the weekly and monthly jobs. The week and month rebuild waits until the day succeeds. |
| 10 | Edition rule | A day digest for date D whose newest finished `created_at` (local time) is during D is 「D の夕刊」. If it is at or after the end of D (local midnight of D+1), it is 「D+1 の朝刊」, D's final whole-day version. One date = one edition. |
| 11 | Edition per date, not per scope | The newest `created_at` among the date's finished entries decides. Pending and failed entries are ignored. A date with no finished entry has no edition and uses the plain period heading. |
| 12 | Evening subtitle | Uses the actual cutoff (the newest `created_at`): 「11月2日 17時までの記事」 when the minute is 0, 「11月2日 17時3分までの記事」 otherwise. |
| 13 | Morning heading | Title 「11月2日 朝刊」, subtitle 「11月1日の記事」. Evening title 「11月2日 夕刊」. A year prefix (「2025年」) is added when the date is not in the current year, as the existing headings do. |
| 14 | 今日 tab default | The newest edition: today's day digest if it exists (夕刊), otherwise the newest earlier one. The iPhone note 「今日の分は明日の朝 7 時ごろにできます」 is removed. |
| 15 | Following new editions | Until the reader steps with the arrows, the tab follows the newest edition (a sync or `digests-changed` that brings today's evening edition switches to it). After a step, the selection is kept while it still exists. Switching the period resets it. |
| 16 | Prev/next | One stored day digest = one edition. Week and month tabs browse stored period starts (mobile already does, merged PR #5). |
| 17 | Mac browsing | The Mac gets the same browsing. A new command `list_digest_starts(period, topic_id)` returns the starts that have a finished digest. The current period start is always reachable on the Mac (so a writer can still generate today's digest by hand); the iPhone walks stored starts only, as today. |
| 18 | Mac data follows the shown date | `get_digests`, `digest_stats`, `regenerate_digest`, the chat attachment and the chat period all use the shown period start, not today. |
| 19 | Mac week/month titles | The Mac's `periodHeading` gains the same relative wording as mobile (今日/昨日/M月D日, 今週/先週/M月D日の週, 今月/先月/M月). |
| 20 | Mac schedule UI | The Schedule tab gets a 「夕刊の時刻」 field for `schedule.evening_time`. |
| 21 | Delivery | Three PRs, each from master, none stacked: (1) core schedule, CLI tick, Mac writer scheduler and Schedule tab, with this spec and the plan; (2) mobile edition headings and selection; (3) Mac edition headings, selection and browsing. PR 2 and PR 3 depend only on PR 1 being merged (and PR 3 not even on that). |

## Timeline example (server, defaults)

| Time (JST) | What runs | Day digest for 11-02 |
|---|---|---|
| 11-02 07:00 | daily job for 11-01 (force), week and month rebuild | none |
| 11-02 17:00 to 17:05 (first tick) | collect if due, then `evening 2026-11-02`, then sync | made, `created_at` 11-02 17:0x → 「11月2日 夕刊」 |
| 11-03 07:00 | daily job for 11-02 (force), week and month rebuild, sync | rebuilt, `created_at` 11-03 07:0x → 「11月3日 朝刊」 (11月2日の記事) |

Between 00:00 and 07:00 on 11-03 the 今日 tab shows 「11月2日 夕刊」 (the newest edition).

## Components

### PR 1: schedule (core, CLI, Mac writer)

`crates/core/src/schedule.rs`

- `ScheduleConfig.evening_time: NaiveTime`, parsed from `schedule.evening_time`, default 17:00, garbage falls back to the default (same `parse_time` as the other times).
- `LastRuns.evening_for: Option<NaiveDate>`, read from `schedule.last_evening_for`.
- `Job::Evening(NaiveDate)`: target is today.
- `due_jobs`: after the daily job, push `Job::Evening(today)` when `now.time() >= evening_time` and `evening_for` is before today (or unset). Order becomes Collect, Daily, Evening, Weekly, Monthly, so a collection in the same evaluation feeds the evening edition.
- `job_plan`: `Daily(day)` → `[(Day, day, true), (Week, today, true), (Month, today, true)]` (the first entry's `force` changes from false to true). `Evening(day)` → `[(Day, day, true)]`. `force` on the evening run also replaces a digest made by hand earlier in the day; it runs once per day.
- `last_run_record`: `Evening(day)` → `("schedule.last_evening_for", day)`.
- `should_record`: false when generation failed for Daily, Weekly and Monthly; Evening is always recorded.

`crates/core/src/seed.rs`: `("schedule.evening_time", "17:00")` in `DEFAULT_SETTINGS` (after `schedule.daily_time`). This also makes the key settable from the Mac (`set_setting` accepts only keys in `DEFAULT_SETTINGS`).

`crates/cli/src/tick.rs`: `job_key`/`job_from_key` gain `evening:<date>`. Nothing else changes: `run` already syncs after any digest job that returns true, and `digest_job` already follows `job_plan`, `should_record` and `last_run_record`. The log line is `digest evening <date>: done` (or `: failed, will retry: [...]`, `: error ...`).

`app/src-tauri/src/scheduler.rs`: no production change. `run_job` already follows `job_plan`, `should_record` and `last_run_record`, and `jobs::run_digests` already requests a sync. A test pins the evening-then-morning behaviour on the Mac.

`app/src/views/settings/ScheduleTab.tsx`: a 「夕刊の時刻」 time field (`schedule.evening_time`) and the new description of the two runs.

### PR 2: mobile (`mobile/`)

- `lib/app/editions.dart` (new): `EditionKind`, `Edition`, `editionOf(start, entries)`, `editionHeading(edition, now:)`.
- `lib/ui/digest_screen.dart`: on the 今日 tab the heading comes from `editionOf` when the shown file has a finished entry, otherwise from `periodHeading`. The note is removed. A `_pinned` flag makes the selection follow the newest edition until the reader steps.

### PR 3: Mac (`app/`, plus one store query in core)

- `crates/core/src/store/digests.rs`: `Store::done_digest_starts(period, topic_id) -> Vec<NaiveDate>` (distinct `period_start` with a `done` row; category scopes when `topic_id` is None, that topic's scope otherwise; oldest first).
- `app/src-tauri/src/commands/digests.rs`: `list_digest_starts(period, topic_id) -> Vec<String>`, registered in `lib.rs`.
- `app/src/api.ts`: `listDigestStarts(period, topicId)`.
- `app/src/lib/editions.ts` (new): `editionOf`, `editionHeading` (same rule and strings as mobile).
- `app/src/lib/browse.ts` (new): `browseStarts(stored, current)`, `pickStart(stored, current, keep)`.
- `app/src/lib/dates.ts`: `periodHeading(period, shown, now = shown)` with relative wording; `parseLocalDate(s)`.
- `app/src/views/DigestView.tsx`: loads the starts, picks the shown start, prev/next buttons (「前へ」/「次へ」) beside the heading, edition heading on the 今日 tab, and every request uses the shown start.

## Error handling

- Evening run with no LLM provider (`provider_from_settings` fails): logged as `digest evening <date>: error ...`, not recorded, retried after 15 minutes, as the other digest jobs.
- Evening run where some scopes fail: recorded; failed scopes are `pending` and retried by the pending retry; finished scopes stay.
- Morning rebuild where some scopes fail: not recorded; the evening edition stays visible (`done` rows are never overwritten by a failure); the whole daily job retries every 15 minutes until it succeeds. Week and month are rebuilt after that success.
- No articles today: `plan_digest` returns None for each scope; the evening job is recorded and makes nothing; the 今日 tab shows the newest earlier edition.
- Unreadable `created_at` in a synced file: that entry is ignored for the edition; if no entry is readable the plain heading is used.
- `created_at` before D (only with a wrong clock on the writer): treated as an evening edition with no cutoff time, subtitle 「M月D日の記事」.
- Mac `list_digest_starts` fails: the error line shows it and the view falls back to the current period start.

## Testing

- Core (`cargo test -p infodesk-core`): evening due/once-per-day/not-made-up, job order, config default and garbage, `job_plan` force for daily and evening, record key, `should_record` for daily failures; `done_digest_starts` (PR 3).
- CLI (`cargo test -p infodesk`): evening run at 17:02 makes today's digest and syncs after it; it does not run again that day; next morning's run rebuilds it with the new body; a failed morning rebuild keeps the evening body and retries after 15 minutes; attempt keys round-trip `evening:<date>`.
- Mac backend (`cargo test` in `app/src-tauri`): `run_job` evening then daily replaces the body and records `schedule.last_evening_for`; `list_digest_starts` (PR 3).
- Mac UI (`npx vitest run`, `npx tsc --noEmit`): Schedule tab saves the evening time; `editionOf`/`editionHeading`/`browseStarts`/`pickStart`/`periodHeading`; DigestView shows the evening edition, steps to a morning edition, falls back to the newest earlier edition while today stays reachable, browses weeks, follows a new edition until the user steps, and sends the shown date to every command.
- Mobile (`flutter test`, `flutter analyze`): edition unit tests (boundaries at midnight, minute formatting, pending ignored, mixed scopes, year prefix, month boundary); widget tests for headings, the removed note, following a new edition after a sync, and keeping a stepped selection.
- Lint: `cargo clippy --workspace --all-targets -- -D warnings` (and in `app/src-tauri`).

All test times are built from local dates (`DateTime(y, m, d, h)` in Dart, `new Date(y, m, d, h)` in TS) so the results do not depend on the machine's time zone.

## Rollout

PR 1 (server and Mac writer):

1. Merge PR 1.
2. On the Mac, from a checkout of master at the merge commit: `deploy/k8s/build-and-push.sh` (prints `registry.umafolio.site/infodesk:<tag>`).
3. Put the tag in `deploy/k8s/workflowtemplate.yaml` and `deploy/k8s/shell.yaml` (the `TAG` placeholder or the previous tag), then `kubectl apply -f deploy/k8s/workflowtemplate.yaml` (kubectl runs via ssh to the WSL node 192.168.0.175, as in `docs/15-home-server.md`). The next CronWorkflow run uses the new image.
4. Check the first run after 17:00 in the Argo log: `digest evening <today>: done`, then `sync: ok, ...`. Check the next morning's run: `digest daily <yesterday>: done`.
5. Mac writer users get the change with the next app release; the setting appears in the Schedule tab with 17:00.

PR 2 and PR 3 change only readers; they ship with the next iPhone build and the next Mac release. Older readers keep working: they see the evening edition as today's digest under the old heading.

User docs updated in the matching PR: schedule (`03-sources-and-topics`, `07-settings`, `02-getting-started`, `04-digests` §2, `08-cli`) in PR 1; `14-mobile-app` in PR 2; `04-digests` §1 in PR 3. Both languages each time.
