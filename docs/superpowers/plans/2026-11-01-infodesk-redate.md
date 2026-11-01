# Re-dating articles after a collection gap Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add `infodesk redate`, which moves articles collected on one day back to the day they were published, and prints the digest commands that fill the gap.

**Architecture:** A new store module (`store/redate.rs`) selects and moves the articles in one transaction. A new core module (`redate.rs`) decides which day, week and month digests must be made or remade. The CLI command ties them together and prints a report; digests are made with the existing `infodesk digest` command. The sync engine and the mobile app do not change; a sync test proves the moved days are rewritten.

**Tech Stack:** Rust 2024 workspace (`infodesk-core`, `infodesk` CLI), rusqlite, chrono, clap, tokio tests.

**Spec:** `docs/superpowers/specs/2026-11-01-infodesk-redate-design.md`

## Global Constraints

- New `fetched_at` = the article's `published_at`, unchanged (D2). Both are stored with `store::ts` (`to_rfc3339_opts(SecondsFormat::Secs, true)`), so string comparisons keep working.
- An article moves only when its `fetched_at` is on the `--fetched-on` day and its `published_at` is on a day from `--from` to `--to`, all days in the local time zone (D3). Articles with no `published_at` never move.
- Dry run unless `--apply`. `--from <= --to < --fetched-on` is required; otherwise the command fails with a message, exit code 1, and nothing changes (D4).
- `redate` never makes digests; it prints the commands (D5). No change to the sync engine or `mobile/` (D6).
- Summary, read, starred, topics and ops of a moved article do not change.
- Code comments follow the file they are in (Japanese in `crates/cli/src/lib.rs` and most store files; English in `crates/core/src/sync/engine.rs`).
- Commits: Conventional Commits, no `Co-Authored-By` or any other AI attribution trailer.
- Before each commit: `cargo test -p infodesk-core -p infodesk` and `cargo clippy --workspace --all-targets -- -D warnings` pass.

## Review Focus

- Day boundaries in local time: an article published at 15:30Z on 10-26 is 10-27 in JST and must move; one published 14:30Z must not. (Task 1 test.)
- The digest order matters: week digests are built from day digests and month digests from shorter ones, so the printed commands must list every day first, then weeks, then months. (Task 2 test.)
- A period starting today or later must not be listed, else the normal schedule's own digest for it would be skipped later as "already done". (Task 2 test.)
- Running `--apply` twice: the second run finds nothing on `--fetched-on` (the articles moved), prints `nothing to move`, and changes nothing. (Task 1 test.)
- A bad range (`--to` on or after `--fetched-on`, or `--from` after `--to`) must fail before any write. (Tasks 1 and 3 tests.)

**Ruling recorded here:** the spec says the digest plan is "oldest first". Because week and month digests are built from shorter digests, the plan orders by period kind first (day, week, month), and oldest first inside each kind. This matches the spec's own example output.

---

### Task 1: Store: select and move articles

**Files:**
- Create: `crates/core/src/store/redate.rs`
- Modify: `crates/core/src/store/mod.rs` (add `mod redate;` and `pub use redate::{RedateCount, RedateRange};`)
- Modify: `crates/core/src/store/sync.rs:62` (make `day_bounds` `pub(super)`)

**Interfaces:**
- Produces:
  - `pub struct RedateRange { pub fetched_on: NaiveDate, pub from: NaiveDate, pub to: NaiveDate }` with `pub fn validate(&self) -> Result<()>`
  - `pub struct RedateCount { pub day: NaiveDate, pub category: String, pub count: usize }`
  - `Store::redate_preview(&self, r: &RedateRange, tz: FixedOffset) -> Result<Vec<RedateCount>>`
  - `Store::redate_apply(&self, r: &RedateRange, tz: FixedOffset) -> Result<Vec<RedateCount>>`
  - Counts are sorted by `(day, category)`.

- [ ] **Step 1: Write the failing tests** in `crates/core/src/store/redate.rs`:

```rust
#[cfg(test)]
mod tests {
    use chrono::{FixedOffset, NaiveDate};

    use super::{RedateCount, RedateRange};
    use crate::model::{Category, ItemFilter};
    use crate::store::Store;
    use crate::testutil::{at, new_item, store_with_source};

    fn tz() -> FixedOffset {
        FixedOffset::east_opt(9 * 3600).unwrap()
    }

    fn d(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    fn add(s: &Store, url: &str, fetched: &str, published: Option<&str>) -> i64 {
        let src = s.list_sources().unwrap().remove(0);
        let mut it = new_item(url, "t", "", None);
        it.published_at = published.map(at);
        s.upsert_items(&src, &[it], &at(fetched)).unwrap();
        s.query_items(&ItemFilter::default())
            .unwrap()
            .into_iter()
            .find(|i| i.url == url)
            .unwrap()
            .id
    }

    fn range() -> RedateRange {
        RedateRange {
            fetched_on: d("2026-11-01"),
            from: d("2026-10-27"),
            to: d("2026-10-31"),
        }
    }

    const FETCHED: &str = "2026-11-01T04:00:00Z"; // JST 11-01 13:00

    struct Ids {
        moves_30: i64,
        moves_27: i64,
        before_from: i64,
        after_to: i64,
        no_date: i64,
        other_day: i64,
    }

    fn fixture() -> (Store, Ids) {
        let s = store_with_source(Category::TechJp).0;
        let ids = Ids {
            moves_30: add(&s, "https://a/1", FETCHED, Some("2026-10-30T03:00:00Z")),
            // UTC 10-26 15:30 は JST 10-27 00:30。
            moves_27: add(&s, "https://a/2", FETCHED, Some("2026-10-26T15:30:00Z")),
            // UTC 10-26 14:30 は JST 10-26 23:30。
            before_from: add(&s, "https://a/3", FETCHED, Some("2026-10-26T14:30:00Z")),
            // UTC 10-31 15:00 は JST 11-01 00:00。
            after_to: add(&s, "https://a/4", FETCHED, Some("2026-10-31T15:00:00Z")),
            no_date: add(&s, "https://a/5", FETCHED, None),
            other_day: add(&s, "https://a/6", "2026-10-31T04:00:00Z", Some("2026-10-30T00:00:00Z")),
        };
        (s, ids)
    }

    fn expected() -> Vec<RedateCount> {
        vec![
            RedateCount { day: d("2026-10-27"), category: "tech_jp".into(), count: 1 },
            RedateCount { day: d("2026-10-30"), category: "tech_jp".into(), count: 1 },
        ]
    }

    #[test]
    fn preview_counts_by_local_publish_day_and_changes_nothing() {
        let (s, ids) = fixture();
        assert_eq!(s.redate_preview(&range(), tz()).unwrap(), expected());
        let it = s.get_item(ids.moves_30).unwrap().unwrap();
        assert_eq!(it.fetched_at, at(FETCHED));
    }

    #[test]
    fn apply_moves_only_matching_articles_and_keeps_their_state() {
        let (s, ids) = fixture();
        s.set_ai_summary(ids.moves_30, "要約").unwrap();
        s.set_read(ids.moves_30, true).unwrap();
        s.set_starred(ids.moves_30, true).unwrap();

        assert_eq!(s.redate_apply(&range(), tz()).unwrap(), expected());

        let moved = s.get_item(ids.moves_30).unwrap().unwrap();
        assert_eq!(moved.fetched_at, at("2026-10-30T03:00:00Z"));
        assert_eq!(moved.ai_summary.as_deref(), Some("要約"));
        assert!(moved.read && moved.starred);
        assert_eq!(
            s.get_item(ids.moves_27).unwrap().unwrap().fetched_at,
            at("2026-10-26T15:30:00Z")
        );
        for id in [ids.before_from, ids.after_to, ids.no_date] {
            assert_eq!(s.get_item(id).unwrap().unwrap().fetched_at, at(FETCHED), "id {id}");
        }
        assert_eq!(
            s.get_item(ids.other_day).unwrap().unwrap().fetched_at,
            at("2026-10-31T04:00:00Z")
        );

        let on_30: Vec<i64> = s.items_fetched_on(d("2026-10-30"), tz()).unwrap().iter().map(|r| r.id).collect();
        assert_eq!(on_30, vec![ids.moves_30]);
        let on_01: Vec<i64> = s.items_fetched_on(d("2026-11-01"), tz()).unwrap().iter().map(|r| r.id).collect();
        assert!(!on_01.contains(&ids.moves_30));
    }

    #[test]
    fn second_apply_finds_nothing() {
        let (s, _) = fixture();
        s.redate_apply(&range(), tz()).unwrap();
        assert!(s.redate_apply(&range(), tz()).unwrap().is_empty());
    }

    #[test]
    fn bad_ranges_fail_before_any_write() {
        let (s, ids) = fixture();
        let bad = [
            RedateRange { fetched_on: d("2026-11-01"), from: d("2026-10-31"), to: d("2026-10-27") },
            RedateRange { fetched_on: d("2026-11-01"), from: d("2026-10-27"), to: d("2026-11-01") },
            RedateRange { fetched_on: d("2026-11-01"), from: d("2026-10-27"), to: d("2026-11-02") },
        ];
        for r in bad {
            assert!(r.validate().is_err(), "{r:?}");
            assert!(s.redate_preview(&r, tz()).is_err(), "{r:?}");
            assert!(s.redate_apply(&r, tz()).is_err(), "{r:?}");
        }
        assert_eq!(s.get_item(ids.moves_30).unwrap().unwrap().fetched_at, at(FETCHED));
    }
}
```

If `items_fetched_on` returns a type without `id`, use whatever id field it has (`ItemRecord.id`).

- [ ] **Step 2: Run the tests and see them fail**

Run: `cargo test -p infodesk-core store::redate`
Expected: compile error, `RedateRange` and `redate_preview` not found.

- [ ] **Step 3: Implement** above the test module in `crates/core/src/store/redate.rs`:

```rust
//! 取得の空白を埋める: ある日に取得した記事を、公開日に取得したことにする。

use std::collections::BTreeMap;

use chrono::{DateTime, FixedOffset, NaiveDate, Utc};
use rusqlite::params;

use super::sync::day_bounds;
use super::{Store, ts};
use crate::error::{Error, Result};

/// `fetched_on` の日に取得し、`from`〜`to` の日に公開された記事が対象。日付はすべて現地時間。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RedateRange {
    pub fetched_on: NaiveDate,
    pub from: NaiveDate,
    pub to: NaiveDate,
}

impl RedateRange {
    /// `from <= to < fetched_on` でなければエラー。
    pub fn validate(&self) -> Result<()> {
        if self.from <= self.to && self.to < self.fetched_on {
            Ok(())
        } else {
            Err(Error::Invalid(format!(
                "need --from <= --to < --fetched-on (got --from {} --to {} --fetched-on {})",
                self.from, self.to, self.fetched_on
            )))
        }
    }
}

/// 移す先の日 (公開日) とカテゴリーごとの件数。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RedateCount {
    pub day: NaiveDate,
    pub category: String,
    pub count: usize,
}

impl Store {
    /// 移す記事の件数。何も変えない。
    pub fn redate_preview(&self, r: &RedateRange, tz: FixedOffset) -> Result<Vec<RedateCount>> {
        Ok(counts(&self.redate_targets(r, tz)?, tz))
    }

    /// 対象の記事の `fetched_at` を `published_at` にする。1 つのトランザクションで行う。
    pub fn redate_apply(&self, r: &RedateRange, tz: FixedOffset) -> Result<Vec<RedateCount>> {
        let targets = self.redate_targets(r, tz)?;
        let tx = self.conn.unchecked_transaction()?;
        for (id, _, _) in &targets {
            tx.execute(
                "UPDATE items SET fetched_at = published_at WHERE id = ?1",
                params![id],
            )?;
        }
        tx.commit()?;
        Ok(counts(&targets, tz))
    }

    fn redate_targets(
        &self,
        r: &RedateRange,
        tz: FixedOffset,
    ) -> Result<Vec<(i64, String, DateTime<Utc>)>> {
        r.validate()?;
        let (fetched_from, fetched_until) = day_bounds(r.fetched_on, tz)?;
        let (published_from, _) = day_bounds(r.from, tz)?;
        let (_, published_until) = day_bounds(r.to, tz)?;
        let mut stmt = self.conn.prepare(
            "SELECT id, category, published_at FROM items \
             WHERE fetched_at >= ?1 AND fetched_at < ?2 \
             AND published_at IS NOT NULL AND published_at >= ?3 AND published_at < ?4 \
             ORDER BY published_at, id",
        )?;
        let rows = stmt.query_map(
            params![
                ts(&fetched_from),
                ts(&fetched_until),
                ts(&published_from),
                ts(&published_until)
            ],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            },
        )?;
        let mut out = Vec::new();
        for row in rows {
            let (id, category, published) = row?;
            let published = DateTime::parse_from_rfc3339(&published)
                .map_err(|e| Error::Invalid(format!("item {id}: bad published_at {published}: {e}")))?
                .with_timezone(&Utc);
            out.push((id, category, published));
        }
        Ok(out)
    }
}

fn counts(targets: &[(i64, String, DateTime<Utc>)], tz: FixedOffset) -> Vec<RedateCount> {
    let mut by: BTreeMap<(NaiveDate, String), usize> = BTreeMap::new();
    for (_, category, published) in targets {
        *by.entry((published.with_timezone(&tz).date_naive(), category.clone()))
            .or_default() += 1;
    }
    by.into_iter()
        .map(|((day, category), count)| RedateCount { day, category, count })
        .collect()
}
```

In `crates/core/src/store/sync.rs` change `fn day_bounds(` to `pub(super) fn day_bounds(`. In `crates/core/src/store/mod.rs` add `mod redate;` next to the other `mod` lines and `pub use redate::{RedateCount, RedateRange};` next to the other `pub use` lines. If `Error` from rusqlite does not convert with `?` in this module, follow how `store/items.rs` does it (it already uses `unchecked_transaction()?` and `tx.commit()?`).

- [ ] **Step 4: Run the tests and see them pass**

Run: `cargo test -p infodesk-core store::redate`
Expected: 4 passed.

- [ ] **Step 5: Run the full checks and commit**

```bash
cargo test -p infodesk-core -p infodesk && cargo clippy --workspace --all-targets -- -D warnings
git add crates/core/src/store/redate.rs crates/core/src/store/mod.rs crates/core/src/store/sync.rs
git commit -m "feat(core): move articles from a collection day to their publish day"
```

---

### Task 2: Which digests to make or remake

**Files:**
- Create: `crates/core/src/redate.rs`
- Modify: `crates/core/src/lib.rs` (add `pub mod redate;` in alphabetical order, after `pub mod period;`)

**Interfaces:**
- Consumes: `RedateRange` (Task 1), `Store::list_digests(period, start) -> Result<Vec<Digest>>`, `period_start(period, date)`, `period_end(period, start)` (exclusive end, so it is the next period's start), `DigestStatus::Done`.
- Produces: `pub fn redate_digest_plan(store: &Store, r: &RedateRange, today: NaiveDate) -> Result<Vec<(Period, NaiveDate, bool)>>`. Each entry is `(period, period start, force)`. Order: all days, then all weeks, then all months; oldest first inside each.

Rules (spec 3.2):
- For each kind, the candidates are the starts of every period that overlaps `[r.from, r.fetched_on]`, plus, for each of those, the next period's start.
- An overlapping period is listed. A next period is listed only when it already has a finished digest.
- `force` is true when the period already has a finished digest (any scope).
- A period whose start is `>= today` is never listed.

- [ ] **Step 1: Write the failing tests** in `crates/core/src/redate.rs`:

```rust
#[cfg(test)]
mod tests {
    use chrono::{NaiveDate, Utc};

    use super::redate_digest_plan;
    use crate::model::{Category, DigestScope, Period};
    use crate::store::{NewDigest, RedateRange, Store};

    fn d(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    fn done(s: &Store, period: Period, start: &str) {
        s.save_digest_done(
            &NewDigest {
                period,
                period_start: d(start),
                scope: DigestScope::Category(Category::TechJp),
                body: "b".into(),
                item_ids: vec![],
                provider: "p".into(),
            },
            &Utc::now(),
        )
        .unwrap();
    }

    fn range() -> RedateRange {
        RedateRange { fetched_on: d("2026-11-01"), from: d("2026-10-27"), to: d("2026-10-31") }
    }

    #[test]
    fn the_2026_11_01_gap() {
        let s = Store::open_in_memory().unwrap();
        done(&s, Period::Day, "2026-10-26");
        done(&s, Period::Week, "2026-10-26");
        done(&s, Period::Month, "2026-10-01");
        let plan = redate_digest_plan(&s, &range(), d("2026-11-01")).unwrap();
        assert_eq!(
            plan,
            vec![
                (Period::Day, d("2026-10-27"), false),
                (Period::Day, d("2026-10-28"), false),
                (Period::Day, d("2026-10-29"), false),
                (Period::Day, d("2026-10-30"), false),
                (Period::Day, d("2026-10-31"), false),
                (Period::Week, d("2026-10-26"), true),
                (Period::Month, d("2026-10-01"), true),
            ]
        );
    }

    #[test]
    fn next_period_is_remade_only_when_it_has_a_digest() {
        let s = Store::open_in_memory().unwrap();
        // 11-02 は 11-01 の次の日。比較の数字が変わるので作り直す。
        done(&s, Period::Day, "2026-11-02");
        let plan = redate_digest_plan(&s, &range(), d("2026-11-10")).unwrap();
        let days: Vec<_> = plan.iter().filter(|p| p.0 == Period::Day).cloned().collect();
        assert_eq!(days.first(), Some(&(Period::Day, d("2026-10-27"), false)));
        assert!(days.contains(&(Period::Day, d("2026-11-01"), false)));
        assert!(days.contains(&(Period::Day, d("2026-11-02"), true)));
        assert_eq!(days.len(), 7);
        // 週: 10-26 の週 (重なる) と、次の 11-02 の週は要約がないので出さない。
        let weeks: Vec<_> = plan.iter().filter(|p| p.0 == Period::Week).cloned().collect();
        assert_eq!(weeks, vec![(Period::Week, d("2026-10-26"), false)]);
        // 月: 10 月と 11 月が重なる。
        let months: Vec<_> = plan.iter().filter(|p| p.0 == Period::Month).cloned().collect();
        assert_eq!(
            months,
            vec![(Period::Month, d("2026-10-01"), false), (Period::Month, d("2026-11-01"), false)]
        );
    }

    #[test]
    fn periods_starting_today_or_later_are_left_out() {
        let s = Store::open_in_memory().unwrap();
        let plan = redate_digest_plan(&s, &range(), d("2026-10-30")).unwrap();
        assert!(plan.iter().all(|(_, start, _)| *start < d("2026-10-30")), "{plan:?}");
    }
}
```

Note: 2026-10-26 and 2026-11-02 are Mondays (the week digests in the live data start on 2026-10-26).

- [ ] **Step 2: Run the tests and see them fail**

Run: `cargo test -p infodesk-core redate::tests`
Expected: compile error, `redate_digest_plan` not found.

- [ ] **Step 3: Implement** above the test module:

```rust
//! 記事の日付を付け直したあとに作る (作り直す) 期間要約を決める。

use std::collections::BTreeSet;

use chrono::{Duration, NaiveDate};

use crate::error::Result;
use crate::model::{DigestStatus, Period};
use crate::period::{period_end, period_start};
use crate::store::{RedateRange, Store};

/// 作る期間要約の一覧 `(期間, 開始日, force)`。日次、週次、月次の順で、それぞれ古い順。
/// 週次と月次は短い期間の要約から作るので、この順に実行する。
pub fn redate_digest_plan(
    store: &Store,
    r: &RedateRange,
    today: NaiveDate,
) -> Result<Vec<(Period, NaiveDate, bool)>> {
    let mut out = Vec::new();
    for period in [Period::Day, Period::Week, Period::Month] {
        let mut overlapping = BTreeSet::new();
        let mut d = r.from;
        while d <= r.fetched_on {
            overlapping.insert(period_start(period, d));
            d += Duration::days(1);
        }
        let next: BTreeSet<NaiveDate> = overlapping
            .iter()
            .map(|s| period_end(period, *s))
            .filter(|s| !overlapping.contains(s))
            .collect();
        let mut starts: Vec<(NaiveDate, bool)> = Vec::new();
        for start in overlapping.into_iter().chain(next.iter().copied()) {
            if start >= today {
                continue;
            }
            let has_done = store
                .list_digests(period, start)?
                .iter()
                .any(|d| d.status == DigestStatus::Done);
            if next.contains(&start) && !has_done {
                continue;
            }
            starts.push((start, has_done));
        }
        starts.sort();
        out.extend(starts.into_iter().map(|(s, force)| (period, s, force)));
    }
    Ok(out)
}
```

Add `pub mod redate;` to `crates/core/src/lib.rs` after `pub mod period;`.

- [ ] **Step 4: Run the tests and see them pass**

Run: `cargo test -p infodesk-core redate::tests`
Expected: 3 passed.

- [ ] **Step 5: Run the full checks and commit**

```bash
cargo test -p infodesk-core -p infodesk && cargo clippy --workspace --all-targets -- -D warnings
git add crates/core/src/redate.rs crates/core/src/lib.rs
git commit -m "feat(core): list the digests to make after re-dating articles"
```

---

### Task 3: CLI command `infodesk redate`

**Files:**
- Modify: `crates/cli/src/lib.rs` (the `Command` enum, the `run` match, a new `redate` function, a new `format_redate` function, tests)
- Modify: `docs/08-cli.md`, `docs/08-cli.ja.md` (document the command next to `digest`)

**Interfaces:**
- Consumes: `infodesk_core::store::{RedateRange, RedateCount}`, `Store::redate_preview`, `Store::redate_apply` (Task 1); `infodesk_core::redate::redate_digest_plan` (Task 2); existing `parse_date`, `local_tz`, `today` in `crates/cli/src/lib.rs`.
- Produces: `fn format_redate(counts: &[RedateCount], plan: &[(Period, NaiveDate, bool)], applied: bool, db: &str) -> String`.

Output (exact text; `db` is the `--db` path in use):

```
dry run: 2 articles would move (nothing changed; add --apply)
2026-10-27  tech_jp 1
2026-10-30  medcare 3  tech_jp 1
then run:
  infodesk --db /data/infodesk.db digest day --date 2026-10-27
  infodesk --db /data/infodesk.db digest week --date 2026-10-26 --force
  infodesk --db /data/infodesk.db sync
```

- First line with `--apply`: `moved N articles`.
- One line per day: the date, two spaces, then `category count` pairs joined by two spaces, in `RedateCount` order.
- When `counts` is empty: the whole output is `nothing to move\n` (no command list).
- The `--db` value printed is the path the command used (the explicit `--db`, or the default path).

- [ ] **Step 1: Write the failing tests.** In the existing `mod tests` of `crates/cli/src/lib.rs`:

```rust
    #[test]
    fn redate_requires_all_three_dates() {
        assert!(parse(&["redate", "--from", "2026-10-27", "--to", "2026-10-31"]).is_err());
        assert!(
            parse(&[
                "redate", "--fetched-on", "2026-11-01", "--from", "2026-10-27", "--to", "2026-10-31"
            ])
            .is_ok()
        );
    }

    #[test]
    fn format_redate_lists_counts_then_commands() {
        use infodesk_core::store::RedateCount;
        let d = |s: &str| NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap();
        let counts = vec![
            RedateCount { day: d("2026-10-27"), category: "tech_jp".into(), count: 1 },
            RedateCount { day: d("2026-10-30"), category: "medcare".into(), count: 3 },
            RedateCount { day: d("2026-10-30"), category: "tech_jp".into(), count: 1 },
        ];
        let plan = vec![
            (Period::Day, d("2026-10-27"), false),
            (Period::Week, d("2026-10-26"), true),
        ];
        assert_eq!(
            format_redate(&counts, &plan, false, "/data/infodesk.db"),
            "dry run: 5 articles would move (nothing changed; add --apply)\n\
             2026-10-27  tech_jp 1\n\
             2026-10-30  medcare 3  tech_jp 1\n\
             then run:\n  \
             infodesk --db /data/infodesk.db digest day --date 2026-10-27\n  \
             infodesk --db /data/infodesk.db digest week --date 2026-10-26 --force\n  \
             infodesk --db /data/infodesk.db sync\n"
        );
        assert!(format_redate(&counts, &plan, true, "x").starts_with("moved 5 articles\n"));
        assert_eq!(format_redate(&[], &plan, true, "x"), "nothing to move\n");
    }
```

In `mod entry_tests`:

```rust
    #[test]
    fn redate_checks_the_range_and_runs_on_an_empty_database() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("t.db").display().to_string();
        let base = ["infodesk", "--db", db.as_str(), "redate", "--fetched-on", "2026-11-01"];
        let with = |extra: &[&str]| {
            let mut v: Vec<&str> = base.to_vec();
            v.extend_from_slice(extra);
            args(&v)
        };
        assert_eq!(run_from(with(&["--from", "2026-10-27", "--to", "2026-10-31"])), 0);
        assert_eq!(run_from(with(&["--from", "2026-10-27", "--to", "2026-10-31", "--apply"])), 0);
        assert_eq!(run_from(with(&["--from", "2026-10-31", "--to", "2026-10-27"])), 1);
        assert_eq!(run_from(with(&["--from", "2026-10-27", "--to", "2026-11-01"])), 1);
        assert_eq!(run_from(with(&["--from", "bad", "--to", "2026-10-31"])), 1);
    }
```

- [ ] **Step 2: Run the tests and see them fail**

Run: `cargo test -p infodesk redate`
Expected: compile error, `Command::Redate` / `format_redate` not found.

- [ ] **Step 3: Implement.** Add the variant to `enum Command` (after `Digests`):

```rust
    /// 取得の空白を埋める: 指定日に取得した記事を公開日に移し、作るべき期間要約を表示する
    Redate {
        /// 記事を取得した日 (YYYY-MM-DD)
        #[arg(long)]
        fetched_on: String,
        /// 移す公開日の最初の日 (YYYY-MM-DD)
        #[arg(long)]
        from: String,
        /// 移す公開日の最後の日 (YYYY-MM-DD)。--fetched-on より前
        #[arg(long)]
        to: String,
        /// 実際に移す。付けなければ件数を表示するだけ
        #[arg(long)]
        apply: bool,
    },
```

In `run`, the `path` variable holds the database path before the store is opened. Add to the main `match` (next to `Command::Digests`):

```rust
        Command::Redate {
            fetched_on,
            from,
            to,
            apply,
        } => redate(&store, &fetched_on, &from, &to, apply, &path.display().to_string()),
```

If `path` was moved into `Store::open` before the match, take `let db_label = path.display().to_string();` before opening and pass `&db_label`.

Add the functions (near `digest`):

```rust
fn redate(
    store: &Store,
    fetched_on: &str,
    from: &str,
    to: &str,
    apply: bool,
    db: &str,
) -> Result<()> {
    let r = RedateRange {
        fetched_on: parse_date(fetched_on)?,
        from: parse_date(from)?,
        to: parse_date(to)?,
    };
    r.validate()?;
    let tz = local_tz();
    let counts = if apply {
        store.redate_apply(&r, tz)?
    } else {
        store.redate_preview(&r, tz)?
    };
    let plan = if counts.is_empty() {
        Vec::new()
    } else {
        redate_digest_plan(store, &r, today())?
    };
    print!("{}", format_redate(&counts, &plan, apply, db));
    Ok(())
}

fn format_redate(
    counts: &[RedateCount],
    plan: &[(Period, NaiveDate, bool)],
    applied: bool,
    db: &str,
) -> String {
    if counts.is_empty() {
        return "nothing to move\n".to_string();
    }
    let total: usize = counts.iter().map(|c| c.count).sum();
    let mut out = if applied {
        format!("moved {total} articles\n")
    } else {
        format!("dry run: {total} articles would move (nothing changed; add --apply)\n")
    };
    let mut i = 0;
    while i < counts.len() {
        let day = counts[i].day;
        let mut parts = Vec::new();
        while i < counts.len() && counts[i].day == day {
            parts.push(format!("{} {}", counts[i].category, counts[i].count));
            i += 1;
        }
        out.push_str(&format!("{}  {}\n", day.format("%Y-%m-%d"), parts.join("  ")));
    }
    out.push_str("then run:\n");
    for (period, start, force) in plan {
        out.push_str(&format!(
            "  infodesk --db {db} digest {} --date {}{}\n",
            period.as_str(),
            start.format("%Y-%m-%d"),
            if *force { " --force" } else { "" }
        ));
    }
    out.push_str(&format!("  infodesk --db {db} sync\n"));
    out
}
```

Imports: add `use infodesk_core::redate::redate_digest_plan;` and `use infodesk_core::store::{RedateCount, RedateRange};`. `Period` and `NaiveDate` are already imported in this file; if not, add them.

Docs: in `docs/08-cli.md` add a `redate` entry after the `digest`/`digests` entries, in the same style as its neighbours:

```markdown
### redate

Fills a collection gap. Articles collected on `--fetched-on` whose publish date falls on a day from `--from` to `--to` (local time) are moved to their publish date (`fetched_at` becomes `published_at`). Articles with no publish date stay. Without `--apply` it only prints the counts. It then prints the digest commands to run (day digests first, then week and month, with `--force` where a digest already exists) and `sync`. `--from <= --to < --fetched-on` is required.

    infodesk redate --fetched-on 2026-11-01 --from 2026-10-27 --to 2026-10-31
    infodesk redate --fetched-on 2026-11-01 --from 2026-10-27 --to 2026-10-31 --apply
```

In `docs/08-cli.ja.md` add the Japanese entry in the same place:

```markdown
### redate

取得の空白を埋めます。`--fetched-on` の日に取得した記事のうち、公開日が `--from`〜`--to` の日 (現地時間) のものを、公開日に取得したことにします (`fetched_at` を `published_at` にする)。公開日のない記事は動きません。`--apply` を付けなければ件数を表示するだけです。続けて実行する期間要約のコマンド (日次、週次、月次の順。要約がすでにあれば `--force` 付き) と `sync` を表示します。`--from <= --to < --fetched-on` でなければエラーです。

    infodesk redate --fetched-on 2026-11-01 --from 2026-10-27 --to 2026-10-31
    infodesk redate --fetched-on 2026-11-01 --from 2026-10-27 --to 2026-10-31 --apply
```

Match the heading level and code style the files already use for other commands.

- [ ] **Step 4: Run the tests and see them pass**

Run: `cargo test -p infodesk redate`
Expected: 3 passed.

- [ ] **Step 5: Run the full checks and commit**

```bash
cargo test -p infodesk-core -p infodesk && cargo clippy --workspace --all-targets -- -D warnings
git add crates/cli/src/lib.rs docs/08-cli.md docs/08-cli.ja.md
git commit -m "feat(cli): add redate to fill a collection gap"
```

---

### Task 4: Sync proof and runbook

**Files:**
- Modify: `crates/core/src/sync/engine.rs` (one test in `mod tests`)
- Modify: `docs/15-home-server.md`, `docs/15-home-server.ja.md` (new section before "Troubleshooting")

**Interfaces:**
- Consumes: `RedateRange`, `Store::redate_apply` (Task 1); test helpers already in `engine.rs` tests: `setup()`, `input(now, compact)`, `NOW = "2026-10-13T12:00:00Z"`, `tz()`, `read_file(drive, name)`, `MemoryDrive`, `run_sync`, `gunzip_json`, `DayFile`.

- [ ] **Step 1: Write the test** in `crates/core/src/sync/engine.rs` `mod tests`:

```rust
    #[tokio::test]
    async fn redated_articles_move_between_day_files_on_the_next_sync() {
        let m = setup();
        let id = {
            let s = m.lock().unwrap();
            let src = s.list_sources().unwrap().remove(0);
            let mut it = new_item("https://a/late", "t", "", None);
            it.published_at = Some(at("2026-10-11T03:00:00Z"));
            s.upsert_items(&src, &[it], &at("2026-10-13T03:00:00Z")).unwrap();
            s.query_items(&ItemFilter::default()).unwrap()[0].id
        };
        let drive = MemoryDrive::new();
        let first = run_sync(&m, &drive, &input(NOW, false)).await.unwrap();
        assert!(first.written_files.iter().any(|f| f == "day-2026-10-13.json.gz"));

        m.lock()
            .unwrap()
            .redate_apply(
                &crate::store::RedateRange {
                    fetched_on: NaiveDate::from_ymd_opt(2026, 10, 13).unwrap(),
                    from: NaiveDate::from_ymd_opt(2026, 10, 11).unwrap(),
                    to: NaiveDate::from_ymd_opt(2026, 10, 11).unwrap(),
                },
                tz(),
            )
            .unwrap();
        let second = run_sync(&m, &drive, &input(NOW, false)).await.unwrap();
        assert!(second.written_files.iter().any(|f| f == "day-2026-10-11.json.gz"), "{:?}", second.written_files);

        let day11: DayFile =
            gunzip_json(&read_file(&drive, "day-2026-10-11.json.gz").await.unwrap()).unwrap();
        assert_eq!(day11.items.iter().map(|i| i.id).collect::<Vec<_>>(), vec![id]);
        // 10-13 has no items left: its file is rewritten empty or removed, but it no longer lists the article.
        if let Some(bytes) = read_file(&drive, "day-2026-10-13.json.gz").await {
            let day13: DayFile = gunzip_json(&bytes).unwrap();
            assert!(day13.items.iter().all(|i| i.id != id));
        }
    }
```

If `tz()`, `DayFile` or `gunzip_json` are named differently in this test module, use the names the module already imports (`use super::*;` brings in the engine's imports). If the engine leaves an emptied day's file in place without rewriting it, the last assertion will fail: that is a real bug for the mobile app (the article would show on both days until the mobile app imports the new file, and the old file would still list it). In that case stop and report it instead of weakening the test.

- [ ] **Step 2: Run the test**

Run: `cargo test -p infodesk-core redated_articles_move`
Expected: PASS (no engine change is planned; the test proves D6). If it fails, see the note above.

- [ ] **Step 3: Write the runbook section.** In `docs/15-home-server.md`, insert before `## 8. Troubleshooting` (and renumber "Troubleshooting" to 9, and fix any links to it in the file and in other docs: `grep -rn "15-home-server" docs README*`):

````markdown
## 8. Filling a collection gap

If nothing collected for some days (for example the Mac stopped before the cluster took over), the first collection afterwards files every new article under its own day, and the gap days get no articles and no daily digest. `infodesk redate` moves the articles published during the gap back to their publish day. Articles with no publish date (GitHub Trending) stay where they are. See [the CLI reference](08-cli.md#redate).

1. Stop the scheduled runs and wait until no run is in progress:

   ```bash
   kubectl -n infodesk patch cronworkflow infodesk-tick --type merge -p '{"spec":{"suspend":true}}'
   kubectl -n infodesk get workflows
   ```

   Wait until no workflow shows `Running`.

2. Start the shell pod (section 6) and check what would move:

   ```bash
   cli="kubectl -n infodesk exec infodesk-shell -- infodesk --db /data/infodesk.db"
   $cli redate --fetched-on 2026-11-01 --from 2026-10-27 --to 2026-10-31
   ```

3. If the counts look right, run it again with `--apply`, then run every command it prints under `then run:`, in order, with `kubectl -n infodesk exec infodesk-shell --` in front. The last one is `sync`.

4. Delete the shell pod and start the scheduled runs again:

   ```bash
   kubectl -n infodesk delete pod infodesk-shell
   kubectl -n infodesk patch cronworkflow infodesk-tick --type merge -p '{"spec":{"suspend":false}}'
   ```

The digest commands call the Claude API: one call per category for each day, week and month listed.
````

In `docs/15-home-server.ja.md` insert the Japanese section in the same place, with the same renumbering:

````markdown
## 8. 取得の空白を埋める

何日か収集しなかった場合 (たとえばクラスターに移る前に Mac が止まっていた場合)、そのあと最初の収集で、新しい記事はすべてその日の記事になります。空白の日には記事も日次ダイジェストもできません。`infodesk redate` は、空白の期間に公開された記事を公開日に移します。公開日のない記事 (GitHub Trending) は動きません。[CLI リファレンス](08-cli.ja.md#redate)も参照してください。

1. 定期実行を止め、実行中のものがなくなるまで待ちます。

   ```bash
   kubectl -n infodesk patch cronworkflow infodesk-tick --type merge -p '{"spec":{"suspend":true}}'
   kubectl -n infodesk get workflows
   ```

   `Running` のワークフローがなくなるまで待ちます。

2. シェル用の Pod を起動し (6 章)、移る件数を確かめます。

   ```bash
   cli="kubectl -n infodesk exec infodesk-shell -- infodesk --db /data/infodesk.db"
   $cli redate --fetched-on 2026-11-01 --from 2026-10-27 --to 2026-10-31
   ```

3. 件数がよければ `--apply` を付けてもう一度実行し、`then run:` の下に表示されたコマンドを、上から順に、前に `kubectl -n infodesk exec infodesk-shell --` を付けて実行します。最後は `sync` です。

4. シェル用の Pod を削除し、定期実行を再開します。

   ```bash
   kubectl -n infodesk delete pod infodesk-shell
   kubectl -n infodesk patch cronworkflow infodesk-tick --type merge -p '{"spec":{"suspend":false}}'
   ```

ダイジェストのコマンドは Claude API を使います。表示された日・週・月ごとに、カテゴリーの数だけ呼び出します。
````

Use the section number the file actually gives the shell pod section ("Changing sources, topics and settings" is section 6 today).

- [ ] **Step 4: Run the full checks and commit**

```bash
cargo test -p infodesk-core -p infodesk && cargo clippy --workspace --all-targets -- -D warnings
git add crates/core/src/sync/engine.rs docs/15-home-server.md docs/15-home-server.ja.md
git commit -m "test(sync): moved articles are rewritten into their new day files; docs: runbook for filling a gap"
```

---

## After the tasks (controller, not an implementer)

1. Push `feature/redate` with an explicit refspec and open a PR against `master`.
2. After the user merges: build and push the image from the merge commit (`deploy/k8s/build-and-push.sh`), apply the WorkflowTemplate with the new tag, and run the section 8 runbook for `--fetched-on 2026-11-01 --from 2026-10-27 --to 2026-10-31`, with the user's go-ahead before `--apply`.
