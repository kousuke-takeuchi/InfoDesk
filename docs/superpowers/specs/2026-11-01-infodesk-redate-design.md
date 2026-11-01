# Re-dating articles after a collection gap (design)

Date: 2026-11-01
Status: waiting for review
Parent designs: [2026-11-01-infodesk-headless-server-design.md](2026-11-01-infodesk-headless-server-design.md), [2026-10-13-infodesk-cloud-sync-design.md](2026-10-13-infodesk-cloud-sync-design.md)

## 1. Goal

When InfoDesk does not collect for some days, the first collection afterwards files every article under the day it ran, even articles published during the gap. The days in the gap then have no articles and no daily digest, and the week and month digests miss them.

This design adds a CLI command that moves those articles back to the day they were published, so the gap days can get their articles and digests.

### The case that started this

The Mac app last collected on 2026-10-26. The k3s cluster took over and first collected on 2026-11-01 (641 new articles). Of the first 100 articles per category returned by `infodesk items --days 1` on the cluster, these were published 2026-10-27 to 2026-10-31 (JST):

| Category | Published in the gap / sampled |
|---|---|
| arxiv | 100 / 100 |
| tech_jp | 78 / 100 |
| tech_intl | 44 / 60 |
| medcare | 40 / 100 |
| huggingface | 26 / 48 |
| hackernews | 13 / 30 |
| github | 0 / 18 (no publish date) |

There is no daily digest for 2026-10-27 to 2026-10-31, and the iPhone app shows nothing for those days.

### Success criteria

- `infodesk redate --fetched-on 2026-11-01 --from 2026-10-27 --to 2026-10-31` prints how many articles would move, per target day and category, and changes nothing.
- With `--apply`, those articles get `fetched_at` equal to their `published_at`, in one transaction. Nothing else about them changes (summary, read, starred, topics).
- After the digest commands that `redate` prints and one `infodesk sync`, the NAS has `day-2026-10-27` to `day-2026-10-31` files and their daily digests, and a smaller `day-2026-11-01`. The iPhone app shows the articles under their new days and the new digests, with no app change.

### Out of scope

- Fetching articles again from sources that accept a date (arXiv, HF Papers, Hacker News through Algolia). Considered as options B and C and not chosen (D1).
- Re-dating automatically after a gap. The command is run by hand when needed.
- A Mac app button. The command is CLI only; the Mac app can run it through `--cli` like any other command.

## 2. Decisions

| # | Question | Decision | Why |
|---|---|---|---|
| D1 | How to fill the gap | Re-date articles already collected (option A) | The first collection after the gap already holds most gap articles, RSS included. Re-fetching would only add arXiv, HF Papers and HN, and those URLs mostly exist already (URLs are unique), so they would stay on the later day anyway. |
| D2 | New `fetched_at` value | The article's `published_at`, unchanged | Keeps the order inside the day. Both columns are stored by `store::ts`, so the string comparisons used by every day query keep working. |
| D3 | Which articles move | `fetched_at` on the `--fetched-on` day and `published_at` on a day from `--from` to `--to`, all days in the local time zone (`TZ`, as for every other command) | Narrow and explicit. Articles with no `published_at` stay. Articles collected on other days stay. |
| D4 | Safety | Dry run unless `--apply`; `--from <= --to < --fetched-on` is required, else the command fails with a message and changes nothing | A wrong range moves many articles at once; seeing the counts first is cheap. Moving articles forward in time or onto the day they already sit on is never meant. |
| D5 | Digests | `redate` does not make digests. It prints the digest commands to run next, with `--force` where a finished digest already exists | Digest generation already exists in the CLI (`infodesk digest`) and costs API calls; keeping it a separate, visible step lets the operator stop after the move. |
| D6 | Sync | No change to the sync engine or the mobile app | Day files are fingerprinted from their items, so every changed day is rewritten on the next sync. The mobile app replaces a day's items by id when it imports a day file (`LocalStore.upsertDay`), so an article that moved days is not left behind. |

## 3. Components

### 3.1 Store (`crates/core/src/store/items.rs`)

```rust
pub struct RedateRange {
    pub fetched_on: NaiveDate,
    pub from: NaiveDate,
    pub to: NaiveDate,
}

pub struct RedateCount {
    pub day: NaiveDate,      // the published day (local), the new fetched day
    pub category: String,
    pub count: usize,
}

impl Store {
    /// Articles the range would move, counted per new day and category.
    pub fn redate_preview(&self, r: &RedateRange, tz: FixedOffset) -> Result<Vec<RedateCount>>;
    /// Moves them in one transaction and returns the same counts.
    pub fn redate_apply(&self, r: &RedateRange, tz: FixedOffset) -> Result<Vec<RedateCount>>;
}
```

Both select with `fetched_at` inside the `fetched_on` day bounds and `published_at` inside `[from 00:00, to + 1 day 00:00)` local, using the same `day_bounds` helper as `items_fetched_on`. `redate_apply` runs `UPDATE items SET fetched_at = published_at WHERE id IN (...)` inside one transaction. Ops, item state and summaries are untouched.

### 3.2 Digest periods to redo (`crates/core/src/schedule.rs` or a new small module)

```rust
pub fn redate_digest_plan(store: &Store, r: &RedateRange) -> Result<Vec<(Period, NaiveDate, bool)>>;
```

Returns, oldest first, each day, week and month whose range overlaps `[from, fetched_on]`, plus the period right after each of those when a finished digest already exists for it (a digest's stats compare its period with the previous one, so the next period's numbers change too). The `bool` is `force`: true when a finished digest exists for that period. Periods whose start is today or later are left out; the normal schedule makes them.

### 3.3 CLI (`crates/cli/src/lib.rs`)

```
infodesk redate --fetched-on YYYY-MM-DD --from YYYY-MM-DD --to YYYY-MM-DD [--apply]
```

Output, dry run (numbers are an example):

```
dry run: 263 articles would move (nothing changed; add --apply)
2026-10-27  huggingface 2  tech_intl 2  tech_jp 2
...
then run:
  infodesk digest day --date 2026-10-27
  ...
  infodesk digest week --date 2026-10-26 --force
  infodesk digest month --date 2026-10-01 --force
  infodesk sync
```

With `--apply` the first line reads `moved 263 articles` and the rest is the same. Exit code 0 on success, 1 on a bad range or a store error (same as other commands).

### 3.4 Runbook (`docs/15-home-server.md` and `.ja.md`)

A new section "Filling a collection gap":

1. Suspend the CronWorkflow: `kubectl -n infodesk patch cronworkflow infodesk-tick --type merge -p '{"spec":{"suspend":true}}'`, and wait until no `infodesk` workflow is Running.
2. Start the shell pod.
3. Run `redate` without `--apply`, check the counts, then with `--apply`.
4. Run the printed digest commands and `sync`.
5. Delete the shell pod and resume the CronWorkflow (`"suspend":false`).

## 4. Error handling

- Bad dates or a range that breaks D4: message, exit 1, nothing changed.
- Nothing to move: prints `nothing to move` and no digest commands.
- Store error during `--apply`: the transaction rolls back; the message names the error.

## 5. Testing

Store tests (in-memory store, JST):

- Moves only articles collected on `fetched_on` whose publish day is in range; an article published the day before `from`, one published on `fetched_on`, one with no `published_at`, and one collected on another day all stay.
- The day boundary uses local time (an article published 2026-10-26T15:30Z is 2026-10-27 JST and moves).
- `redate_preview` changes nothing; `redate_apply` returns the same counts as the preview and keeps summary, read and starred.
- After `redate_apply`, `items_fetched_on` returns the article on its new day and not on the old one, and `day_fingerprints` changes for both days.

Plan test: for the 2026-11-01 case, the plan lists day digests 10-27 to 10-31 (no force), week 10-26 (force when it exists) and month 10-01 (force), and leaves out 11-01 and November while they start today.

CLI test: dry run output and exit codes for a bad range.

Sync test: after `redate_apply`, one sync pass writes the new day files and rewrites the old day's file.

## 6. Applying it to the 2026-11-01 gap

After the PR is merged and a new image is built and pushed, the operator (Claude, with the user's go-ahead) runs the runbook in section 3.4 with `--fetched-on 2026-11-01 --from 2026-10-27 --to 2026-10-31`. Expected API use: 5 daily digests, 1 weekly and 1 monthly digest, about the same as one day of normal digests times 7.
