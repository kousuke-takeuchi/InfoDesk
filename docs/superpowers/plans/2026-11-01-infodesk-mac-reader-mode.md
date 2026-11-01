# InfoDesk Mac Reader Mode Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a reader mode to the Mac app: it reads the sync folder the k3s cluster writes (like the iPhone app), sends its read and starred marks back as its own ops files, and never collects, summarizes, makes digests or writes any other remote file.

**Architecture:** A new setting `sync.mode` (`writer` default, `reader`) is read by the scheduler, the sync launcher and the commands. The core gains store imports for day and digest files plus a `db_id` reset (`store/reader.rs`) and a reader pass `run_reader_sync` (`sync/reader.rs`), a Rust port of the iPhone `_pass` built on the existing `DriveStore`, format and ops helpers. The app's `run_once` calls `run_reader_sync` instead of `run_sync` in reader mode; the scheduler, job entry points and commands refuse writer work; the frontend shows a toggle and hides or locks writer controls.

**Tech Stack:** Rust 2024 workspace (`infodesk-core`, `infodesk` CLI, Tauri 2 app `infodesk-app`), rusqlite (FTS5), tokio, serde_json; React 19 + TypeScript, vitest + Testing Library.

**Spec:** `docs/superpowers/specs/2026-11-01-infodesk-mac-reader-mode-design.md`

## Rulings

Decisions where the spec is open or the code disagrees with it. Each is binding for the tasks below.

1. **Digest import replaces the period** (spec R6 says "upsert each entry"). Each file's entries are upserted as `done` and local rows of that `(period, period_start)` whose scope is not in the file are deleted. Reason: mirrors the iPhone (`upsertDigests` replaces the period) and removes stale rows the server no longer has.
2. **`switch_to_reader` also deletes digests whose status is not `done`.** Reason: a reader never retries them, so writer-era `pending`/`failed` rows would show "保留中" forever.
3. **Switching is a no-op when the mode already matches.** `switch_to_reader` on a reader keeps the device id; `switch_to_writer` on a writer does not turn sync off. Reason: a double click must not orphan ops files or silently stop sync.
4. **Day import is two-phase inside one transaction:** every changed day file of a pass is upserted first, then each day's local rows missing from its file are deleted. If any day file of the pass failed (hash mismatch, unreadable, date mismatch) the delete phase is skipped and no day file is recorded as imported, so all are retried next pass. Reason: an article moved by `redate` must never be deleted (and lose its marks) because its old day was processed before its new day, or its new day failed.
5. **Bad day records are skipped, not stored:** unknown `category` or unparsable `fetched_at` skips the record with a warning, and its id still counts as present (the local copy is not deleted). Unparsable `published_at` is stored as NULL. Times are normalized to the DB format (`ts`, seconds, `Z`). Reason: `row_to_item` fails on an unknown category, so one bad row would break the whole feed.
6. **Day bounds use the Mac's time zone.** The design assumes the server and the Mac share it (both `Asia/Tokyo`; the CronWorkflow sets `TZ=Asia/Tokyo`).
7. **Meta is ignored entirely** (spec R7): it is never downloaded.
8. **Reader warnings are Japanese and the first one goes to `SyncStatus.error` after a successful reader pass.** Spec section 5 says warnings show "as today", but today no warning reaches the UI. The writer path is unchanged.
9. **No sync after each local mark.** Spec R9 assumes an existing "after a local mark" `request_sync` path; there is none (`set_read`/`set_starred` never request a sync). Marks go out on the start sync, the 10-minute timer, 今すぐ同期 and ⌘R. Reason: one NAS ops file per click is not wanted.
10. **The 10-minute timer counts from the later of the last successful sync and the last request**, so a failing NAS is asked every 10 minutes, not every 15-second tick.
11. **`sync_set_reader_mode` refuses while a sync pass runs** (`同期中は切り替えられません。終わってからもう一度試してください`). It holds the `sync_running` flag during the switch, so no pass starts with a half-switched identity.
12. **Guards sit at the job entry points too** (`jobs::run_collect`, `run_digests`, `retry_pending`, `retry_all`), not only in the three commands. Reason: defence in depth; every path to the LLM goes through them.
13. **`sync.mode` cannot be set with `set_setting`** (only `sync_set_reader_mode`, which also makes the new device id). The CLI ignores `sync.mode` (the server is always a writer), so there is no CLI change and no `docs/08-cli*` change.
14. **`AppStatus` also carries `reader_mode`**, filled by `get_status` from the settings, so the sidebar, feed, digest and settings views see the mode without another call. `SyncStatus.reader_mode` serves the Sync tab (spec 4.4).
15. **The tray item is renamed, not hidden:** in reader mode "今すぐ収集" becomes "今すぐ同期" and starts a sync. Reason: Tauri menu items can be renamed in place; hiding needs a rebuilt menu.
16. **⌘R and the sidebar status button sync instead of collecting in reader mode.**
17. **The LLM tab stays editable in reader mode** with a notice (it also sets the chat provider, and chat still runs on the Mac). Sources, Topics and Schedule tabs are locked (a disabled `fieldset`) with a notice. Spec 4.4 lists LLM among the locked tabs.
18. **Own ops upload is chunked like the writer** (`OPS_PER_FILE` per file, seq from 1), always with `create`, and each uploaded file is recorded as an `in` row so it is not downloaded back. Ops files of this device found in the folder (not recorded) are applied like any other (idempotent), as on the iPhone.
19. **The manifest's `schema` is read from raw JSON before parsing**, so a newer format with other fields still gives the "newer format" message rather than "unreadable".

## Global Constraints

- Commits: Conventional Commits. No `Co-Authored-By` or any other AI attribution trailer.
- Code comments follow the surrounding file's language: Japanese in `crates/core/src/store/*`, `crates/core/src/sync/{nas,mode}.rs` and all `app/` files; English in `crates/core/src/sync/{engine,reader,drive,format}.rs`.
- User-facing strings (UI, errors returned to the UI, reader warnings) are Japanese and copied exactly from this plan.
- Never call `run_sync` in reader mode. The reader never writes any remote file except its own new ops files (`ops-<its device id>-*.json`, via `create`), and never deletes a remote file.
- Run everything from the worktree root `~/Documents/InfoDesk-wt-reader`.
- One-time setup before the first app build or frontend test (the worktree has no `app/node_modules` or `app/dist`; Tauri's `generate_context!` needs `app/dist`): `(cd app && npm ci && npx vite build)`.
- Before each commit, all green:
  - `cargo fmt --all`
  - `cargo test -p infodesk-core -p infodesk`
  - `(cd app/src-tauri && cargo test)`
  - `cargo clippy --workspace --all-targets -- -D warnings`
  - `(cd app && npx tsc && npx vitest run)` (`tsconfig.json` has `noEmit: true`, so `tsc` only type-checks)
- Docs under `docs/` change in both English (`.md`) and Japanese (`.ja.md`); `docs/superpowers/` is English only.

## Review Focus

1. **An article moved by `redate` while the Mac reads.** Expected: it appears once, under its new day, with its read and star marks kept, whatever order the day files are processed in. Pinned in Task 2 (`ids_can_move_between_days_in_one_import_whatever_the_order`) and Task 3 (`redated_articles_move_days_on_the_reader_and_keep_marks`).
2. **A day file from a newer server with an unknown category or a bad time.** Expected: that article is skipped with a warning, the local copy is kept, and the feed still loads. Pinned in Task 2 (`bad_records_are_skipped_and_their_local_copies_kept`).
3. **The reader reads a file the cluster is rewriting (hash differs from the manifest).** Expected: the file is skipped with a warning, nothing is deleted, and it is imported on a later pass. Pinned in Task 3 (`a_file_whose_hash_differs_is_skipped_and_imported_later`).
4. **The user flips 読むだけ while a sync pass runs.** Expected: refused with `同期中は切り替えられません。終わってからもう一度試してください`; the mode and device id are unchanged. Pinned in Task 5 (`set_reader_mode_switches_both_ways_and_refuses_while_syncing`).
5. **The NAS is down in reader mode.** Expected: the scheduler asks for a sync at most every 10 minutes, not every 15-second tick. Pinned in Task 6 (`reader_sync_waits_ten_minutes_after_a_sync_or_a_request`).

---

## File Structure

| File | Responsibility |
|---|---|
| `crates/core/src/sync/mode.rs` (new) | `sync.mode` keys, `is_reader`, the refusal message |
| `crates/core/src/seed.rs` | Default `sync.mode = writer` |
| `crates/core/src/store/reader.rs` (new) | `switch_to_reader`/`switch_to_writer`, `import_days`, `import_digests`, `reset_for_db_id` |
| `crates/core/src/store/items.rs` | `word_index_text` becomes `pub(super)` |
| `crates/core/src/store/mod.rs` | `mod reader;`, re-export `DayImport`, `DigestImport` |
| `crates/core/src/sync/engine.rs` | A few private helpers and constants become `pub(crate)` |
| `crates/core/src/sync/reader.rs` (new) | `run_reader_sync`, `ReaderInput`, `ReaderReport` |
| `app/src-tauri/src/state.rs` | `AppStatus.reader_mode` |
| `app/src-tauri/src/sync/mod.rs` | `SyncStatus.reader_mode`, reader branch in `run_once`, `set_reader_mode` |
| `app/src-tauri/src/commands/{status,sync,digests,settings}.rs`, `lib.rs` | `get_status` fills the mode, `sync_set_reader_mode`, refusals |
| `app/src-tauri/src/{jobs,scheduler,tray}.rs` | Job guards, reader timer, tray label |
| `app/src/{types,api,App}.ts(x)`, `components/Sidebar.tsx`, `views/{FeedView,DigestView,SettingsView}.tsx`, `views/settings/SyncTab.tsx`, `styles/layout.css` | Toggle, hidden buttons, locked tabs |
| `docs/15-home-server*.md`, `docs/12-cloud-sync*.md`, `docs/07-settings*.md` | User docs |

---

### Task 1: Mode setting and the switch

**Files:**
- Create: `crates/core/src/sync/mode.rs`
- Create: `crates/core/src/store/reader.rs`
- Modify: `crates/core/src/sync/mod.rs` (add `pub mod mode;`)
- Modify: `crates/core/src/store/mod.rs` (add `mod reader;` next to the other `mod` lines)
- Modify: `crates/core/src/seed.rs:150` (after `("sync.enabled", "false")`)

**Interfaces:**
- Consumes: `Store::get_setting`, `crate::sync::nas::SETTING_ENABLED`, `uuid::Uuid::new_v4` (already a core dependency).
- Produces:
  - `crate::sync::mode::{SETTING_MODE: &str = "sync.mode", MODE_WRITER = "writer", MODE_READER = "reader", READER_ONLY_MESSAGE = "読むだけのモードでは使えません"}`
  - `pub fn crate::sync::mode::is_reader(store: &Store) -> crate::Result<bool>`
  - `Store::switch_to_reader(&self) -> crate::Result<()>`, `Store::switch_to_writer(&self) -> crate::Result<()>`
  - `pub(super) fn store::reader::put_setting(conn: &rusqlite::Connection, key: &str, value: &str) -> crate::Result<()>` (used again in Task 2)

- [ ] **Step 1: Write the failing tests**

Create `crates/core/src/sync/mode.rs` with only the test module first (so it fails to compile on the missing items):

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_mode_is_writer_and_only_reader_counts_as_reader() {
        assert!(crate::seed::DEFAULT_SETTINGS.contains(&(SETTING_MODE, MODE_WRITER)));
        let s = Store::open_in_memory().unwrap();
        assert!(!is_reader(&s).unwrap(), "no key");
        crate::seed::seed_defaults(&s).unwrap();
        assert!(!is_reader(&s).unwrap(), "seeded");
        s.set_setting(SETTING_MODE, "nonsense").unwrap();
        assert!(!is_reader(&s).unwrap());
        s.set_setting(SETTING_MODE, MODE_READER).unwrap();
        assert!(is_reader(&s).unwrap());
    }
}
```

Create `crates/core/src/store/reader.rs` with the test module:

```rust
#[cfg(test)]
mod tests {
    use crate::model::{Category, DigestScope, ItemFilter, Period};
    use crate::store::{NewDigest, Store};
    use crate::sync::mode::{MODE_READER, MODE_WRITER, SETTING_MODE, is_reader};
    use crate::testutil::{at, new_item, store_with_source};
    use chrono::NaiveDate;

    /// 同期が有効で、端末 id がサーバーと同じ (写した DB) ストア。印を1つ付け、送っていない操作が1行ある。
    fn copied_store() -> Store {
        let s = store_with_source(Category::TechJp).0;
        s.set_setting("sync.enabled", "true").unwrap();
        s.set_setting("sync.device_id", "server-and-old-mac").unwrap();
        let src = s.list_sources().unwrap().remove(0);
        s.upsert_items(&src, &[new_item("https://a/1", "t", "", None)], &at("2026-10-13T00:00:00Z"))
            .unwrap();
        let id = s.query_items(&ItemFilter::default()).unwrap()[0].id;
        s.set_read(id, true).unwrap();
        s
    }

    fn d(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    #[test]
    fn switch_to_reader_gives_a_new_device_and_drops_journal_and_unfinished_digests() {
        let s = copied_store();
        assert_eq!(s.pending_local_ops("x").unwrap().len(), 1);
        let now = at("2026-10-13T00:00:00Z");
        s.save_digest_done(
            &NewDigest {
                period: Period::Day,
                period_start: d("2026-10-12"),
                scope: DigestScope::Category(Category::Github),
                body: "done".into(),
                item_ids: vec![],
                provider: "p".into(),
            },
            &now,
        )
        .unwrap();
        s.mark_digest_attempt_failed(
            Period::Day,
            d("2026-10-12"),
            &DigestScope::Category(Category::Arxiv),
            "down",
            &now,
        )
        .unwrap();

        s.switch_to_reader().unwrap();

        assert!(is_reader(&s).unwrap());
        let device = s.get_setting("sync.device_id").unwrap().unwrap();
        assert_ne!(device, "server-and-old-mac");
        assert_eq!(device.len(), 36, "uuid v4: {device}");
        assert!(s.pending_local_ops("x").unwrap().is_empty());
        let digests = s.list_digests(Period::Day, d("2026-10-12")).unwrap();
        assert_eq!(digests.len(), 1, "only the done digest stays");
        assert_eq!(digests[0].body, "done");
        // 印そのもの (記事の列) は残る。
        assert!(s.query_items(&ItemFilter::default()).unwrap()[0].read);
    }

    #[test]
    fn a_second_switch_to_reader_keeps_the_device_id() {
        let s = copied_store();
        s.switch_to_reader().unwrap();
        let first = s.get_setting("sync.device_id").unwrap();
        s.switch_to_reader().unwrap();
        assert_eq!(s.get_setting("sync.device_id").unwrap(), first);
    }

    #[test]
    fn switch_to_writer_turns_sync_off_only_when_leaving_reader() {
        let s = copied_store();
        s.switch_to_writer().unwrap();
        assert_eq!(s.get_setting("sync.enabled").unwrap().as_deref(), Some("true"), "already a writer");
        s.switch_to_reader().unwrap();
        s.switch_to_writer().unwrap();
        assert_eq!(s.get_setting(SETTING_MODE).unwrap().as_deref(), Some(MODE_WRITER));
        assert_eq!(s.get_setting("sync.enabled").unwrap().as_deref(), Some("false"));
        assert_ne!(s.get_setting(SETTING_MODE).unwrap().as_deref(), Some(MODE_READER));
    }
}
```

Add `pub mod mode;` to `crates/core/src/sync/mod.rs` (keep alphabetical order: after `pub mod headless;`) and `mod reader;` to `crates/core/src/store/mod.rs` (after `mod redate;`).

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p infodesk-core sync::mode store::reader`
Expected: FAIL to compile with "cannot find value `SETTING_MODE`" and "no method named `switch_to_reader`".

- [ ] **Step 3: Write the implementation**

At the top of `crates/core/src/sync/mode.rs`, above the test module:

```rust
//! 同期の役割。`writer` は記事を集めて保存先を書き、`reader` は保存先を読むだけ
//! (書くのは自分の既読とスターの ops ファイルだけ)。アプリが使う。サーバー (CLI) はいつも writer。

use crate::store::Store;

pub const SETTING_MODE: &str = "sync.mode";
pub const MODE_WRITER: &str = "writer";
pub const MODE_READER: &str = "reader";
/// 読むだけのモードで、収集や要約を頼まれたときの文。
pub const READER_ONLY_MESSAGE: &str = "読むだけのモードでは使えません";

/// `sync.mode` が `reader` か。キーがない、ほかの値は writer として読む。
pub fn is_reader(store: &Store) -> crate::Result<bool> {
    Ok(store.get_setting(SETTING_MODE)?.as_deref() == Some(MODE_READER))
}
```

In `crates/core/src/seed.rs`, add after `("sync.enabled", "false"),`:

```rust
    ("sync.mode", "writer"),
```

At the top of `crates/core/src/store/reader.rs`, above the test module:

```rust
//! 読むだけのモード (サーバーが書き、この Mac は読む) のための DB 操作。

use rusqlite::{Connection, OptionalExtension, params};

use super::Store;
use crate::error::Result;
use crate::sync::mode::{MODE_READER, MODE_WRITER, SETTING_MODE};
use crate::sync::nas::SETTING_ENABLED;

/// 呼び出し側のトランザクションの中で設定を書く。
pub(super) fn put_setting(conn: &Connection, key: &str, value: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO settings(key, value) VALUES (?1, ?2) \
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![key, value],
    )?;
    Ok(())
}

fn mode_in(conn: &Connection) -> Result<Option<String>> {
    Ok(conn
        .query_row(
            "SELECT value FROM settings WHERE key = ?1",
            params![SETTING_MODE],
            |r| r.get(0),
        )
        .optional()?)
}

impl Store {
    /// 読むだけのモードにする。1 つのトランザクションで、新しい端末 id を作り (写した DB の id は
    /// サーバーのものなので、そのままだとこの Mac の ops ファイルをサーバーが自分のものとして飛ばす)、
    /// 送っていない操作をすべて捨て、生成済みでないダイジェストを消す (読むだけでは再試行しない)。
    /// もう読むだけなら何もしない。
    pub fn switch_to_reader(&self) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        if mode_in(&tx)?.as_deref() == Some(MODE_READER) {
            return Ok(());
        }
        put_setting(&tx, SETTING_MODE, MODE_READER)?;
        put_setting(&tx, "sync.device_id", &uuid::Uuid::new_v4().to_string())?;
        tx.execute("DELETE FROM sync_ops", [])?;
        tx.execute("DELETE FROM digests WHERE status != 'done'", [])?;
        tx.commit()?;
        Ok(())
    }

    /// 書き手に戻す。同期も同時にオフにする (サーバーと 2 台で書かないよう、オンは手で戻す)。
    /// 読むだけでなければ何もしない。
    pub fn switch_to_writer(&self) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        if mode_in(&tx)?.as_deref() != Some(MODE_READER) {
            return Ok(());
        }
        put_setting(&tx, SETTING_MODE, MODE_WRITER)?;
        put_setting(&tx, SETTING_ENABLED, "false")?;
        tx.commit()?;
        Ok(())
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p infodesk-core sync::mode store::reader`
Expected: PASS (4 tests).

- [ ] **Step 5: Full checks and commit**

Run the Global Constraints checks (Rust only is enough here, but `(cd app/src-tauri && cargo test)` must stay green: `settings_cover_every_default_key_and_reflect_updates` now also covers `sync.mode`).

```bash
git add crates/core/src/sync/mode.rs crates/core/src/sync/mod.rs crates/core/src/store/reader.rs crates/core/src/store/mod.rs crates/core/src/seed.rs
git commit -m "feat(core): sync.mode setting and switching the Mac to reader"
```

---

### Task 2: Store imports for day and digest files, and the db_id reset

**Files:**
- Modify: `crates/core/src/store/reader.rs`
- Modify: `crates/core/src/store/items.rs:40` (`fn word_index_text` becomes `pub(super) fn word_index_text`)
- Modify: `crates/core/src/store/mod.rs` (add `pub use reader::{DayImport, DigestImport};` next to the other `pub use` lines)

**Interfaces:**
- Consumes: `put_setting` (Task 1), `super::sync::day_bounds(d: NaiveDate, tz: FixedOffset) -> Result<(DateTime<Utc>, DateTime<Utc>)>`, `super::ts`, `super::items::word_index_text`, `crate::sync::format::{DayFile, ItemRecord, DigestRecord}`, `crate::model::{Category, DigestScope, Period}`.
- Produces:
  - `pub struct DayImport { pub upserted: usize, pub removed: usize, pub skipped: Vec<String> }` (`Debug, Clone, Default, PartialEq, Eq`)
  - `pub struct DigestImport { pub saved: usize, pub removed: usize, pub skipped: Vec<String> }` (same derives)
  - `Store::import_days(&self, days: &[DayFile], tz: FixedOffset, remove_missing: bool) -> Result<DayImport>`
  - `Store::import_digests(&self, period: Period, start: NaiveDate, entries: &[DigestRecord]) -> Result<DigestImport>`
  - `Store::reset_for_db_id(&self, db_id: &str) -> Result<()>`

- [ ] **Step 1: Write the failing tests**

Append to the test module in `crates/core/src/store/reader.rs` (add these `use` lines at the top of the module: `use crate::sync::format::{DayFile, DigestRecord, ItemRecord};`, `use chrono::FixedOffset;`):

```rust
    fn utc() -> FixedOffset {
        FixedOffset::east_opt(0).unwrap()
    }

    fn rec(id: i64, url: &str, title: &str, fetched: &str) -> ItemRecord {
        ItemRecord {
            id,
            url: url.into(),
            title: title.into(),
            excerpt: String::new(),
            ai_summary: None,
            image_url: None,
            source_id: Some(1),
            source_name: "Test Feed".into(),
            category: "tech_jp".into(),
            score: None,
            published_at: None,
            fetched_at: fetched.into(),
            topic_ids: vec![],
        }
    }

    fn day(date: &str, items: Vec<ItemRecord>) -> DayFile {
        DayFile { date: date.into(), items }
    }

    fn search(s: &Store, text: &str) -> Vec<i64> {
        s.query_items(&ItemFilter { text: Some(text.into()), ..Default::default() })
            .unwrap()
            .into_iter()
            .map(|i| i.id)
            .collect()
    }

    #[test]
    fn import_days_inserts_and_updates_articles_without_touching_marks() {
        let s = Store::open_in_memory().unwrap();
        s.set_setting("sync.enabled", "true").unwrap();
        let r = s
            .import_days(&[day("2026-10-13", vec![rec(10, "https://a/10", "oldtitle", "2026-10-13T01:00:00Z")])], utc(), true)
            .unwrap();
        assert_eq!(r, DayImport { upserted: 1, removed: 0, skipped: vec![] });
        s.set_starred(10, true).unwrap();
        let mut newer = rec(10, "https://a/10", "Rust release notes", "2026-10-13T01:00:00.000Z");
        newer.ai_summary = Some("要約".into());
        s.import_days(&[day("2026-10-13", vec![newer])], utc(), true).unwrap();
        let item = s.get_item(10).unwrap().unwrap();
        assert_eq!(item.title, "Rust release notes");
        assert_eq!(item.ai_summary.as_deref(), Some("要約"));
        assert!(item.starred, "marks come from ops, never from day files");
        assert_eq!(item.fetched_at, at("2026-10-13T01:00:00Z"));
        assert_eq!(search(&s, "release"), vec![10], "search index follows the new title");
        assert!(search(&s, "oldtitle").is_empty(), "the old title is gone from the index");
    }

    #[test]
    fn ids_can_move_between_days_in_one_import_whatever_the_order() {
        // Review Focus 1
        let s = Store::open_in_memory().unwrap();
        s.set_setting("sync.enabled", "true").unwrap();
        s.import_days(
            &[
                day("2026-10-12", vec![rec(1, "https://a/1", "one", "2026-10-12T01:00:00Z")]),
                day("2026-10-13", vec![
                    rec(2, "https://a/2", "two", "2026-10-13T01:00:00Z"),
                    rec(3, "https://a/3", "three", "2026-10-13T02:00:00Z"),
                ]),
            ],
            utc(),
            true,
        )
        .unwrap();
        s.set_read(3, true).unwrap();
        // 3 は 12 日へ移った。新しい 13 日を先に渡しても、3 は消えない。
        let r = s
            .import_days(
                &[
                    day("2026-10-13", vec![rec(2, "https://a/2", "two", "2026-10-13T01:00:00Z")]),
                    day("2026-10-12", vec![
                        rec(1, "https://a/1", "one", "2026-10-12T01:00:00Z"),
                        rec(3, "https://a/3", "three", "2026-10-12T05:00:00Z"),
                    ]),
                ],
                utc(),
                true,
            )
            .unwrap();
        assert_eq!(r.removed, 0);
        let moved = s.get_item(3).unwrap().unwrap();
        assert_eq!(moved.fetched_at, at("2026-10-12T05:00:00Z"));
        assert!(moved.read);
        // 13 日のファイルが空になれば、13 日の記事は消える (索引からも)。
        let r = s.import_days(&[day("2026-10-13", vec![])], utc(), true).unwrap();
        assert_eq!(r.removed, 1);
        assert!(s.get_item(2).unwrap().is_none());
        assert!(search(&s, "two").is_empty());
    }

    #[test]
    fn without_remove_missing_nothing_is_deleted() {
        let s = Store::open_in_memory().unwrap();
        s.import_days(&[day("2026-10-13", vec![rec(2, "https://a/2", "two", "2026-10-13T01:00:00Z")])], utc(), true)
            .unwrap();
        let r = s.import_days(&[day("2026-10-13", vec![])], utc(), false).unwrap();
        assert_eq!(r.removed, 0);
        assert!(s.get_item(2).unwrap().is_some());
    }

    #[test]
    fn a_url_held_by_another_id_is_replaced() {
        let (s, src) = store_with_source(Category::TechJp);
        s.upsert_items(&src, &[new_item("https://a/x", "local", "", None)], &at("2026-10-13T00:00:00Z"))
            .unwrap();
        let local = s.query_items(&ItemFilter::default()).unwrap()[0].id;
        s.import_days(&[day("2026-10-13", vec![rec(50, "https://a/x", "server", "2026-10-13T03:00:00Z")])], utc(), true)
            .unwrap();
        assert!(s.get_item(local).unwrap().is_none());
        assert_eq!(s.get_item(50).unwrap().unwrap().title, "server");
        assert!(search(&s, "local").is_empty());
    }

    #[test]
    fn bad_records_are_skipped_and_their_local_copies_kept() {
        // Review Focus 2
        let s = Store::open_in_memory().unwrap();
        s.import_days(&[day("2026-10-13", vec![rec(7, "https://a/7", "seven", "2026-10-13T01:00:00Z")])], utc(), true)
            .unwrap();
        let mut unknown = rec(7, "https://a/7", "seven v2", "2026-10-13T01:00:00Z");
        unknown.category = "podcasts".into();
        let bad_time = rec(8, "https://a/8", "eight", "yesterday");
        let mut bad_published = rec(9, "https://a/9", "nine", "2026-10-13T02:00:00Z");
        bad_published.published_at = Some("soon".into());
        let r = s
            .import_days(&[day("2026-10-13", vec![unknown, bad_time, bad_published])], utc(), true)
            .unwrap();
        assert_eq!(r.skipped.len(), 2, "{:?}", r.skipped);
        assert_eq!(r.removed, 0, "a skipped id still counts as present");
        assert_eq!(s.get_item(7).unwrap().unwrap().title, "seven", "local copy kept");
        assert!(s.get_item(8).unwrap().is_none());
        assert_eq!(s.get_item(9).unwrap().unwrap().published_at, None);
        assert_eq!(s.query_items(&ItemFilter::default()).unwrap().len(), 2, "the feed still loads");
    }

    fn entry(scope: serde_json::Value, body: &str, created_at: &str) -> DigestRecord {
        DigestRecord {
            scope,
            body: body.into(),
            status: "done".into(),
            item_ids: vec![1, 2],
            provider: "claude_api:claude-sonnet-5-5".into(),
            created_at: created_at.into(),
        }
    }

    #[test]
    fn import_digests_replaces_the_period_and_keeps_file_times() {
        let s = Store::open_in_memory().unwrap();
        let start = d("2026-10-13");
        let now = at("2026-10-13T00:00:00Z");
        let github = DigestScope::Category(Category::Github);
        s.save_digest_done(
            &NewDigest { period: Period::Day, period_start: start, scope: github.clone(), body: "old".into(), item_ids: vec![], provider: "ollama".into() },
            &now,
        )
        .unwrap();
        s.save_digest_done(
            &NewDigest { period: Period::Day, period_start: start, scope: DigestScope::Category(Category::Arxiv), body: "gone".into(), item_ids: vec![], provider: "ollama".into() },
            &now,
        )
        .unwrap();
        let r = s
            .import_digests(
                Period::Day,
                start,
                &[
                    entry(serde_json::json!({"category": "github"}), "new", "2026-10-13T22:00:00.000Z"),
                    entry(serde_json::json!({"topic": 4}), "topic", "2026-10-13T22:00:00.000Z"),
                    entry(serde_json::json!({"podcast": 1}), "?", "2026-10-13T22:00:00.000Z"),
                ],
            )
            .unwrap();
        assert_eq!((r.saved, r.removed, r.skipped.len()), (2, 1, 1), "{r:?}");
        let g = s.get_digest(Period::Day, start, &github).unwrap().unwrap();
        assert_eq!(g.body, "new");
        assert_eq!(g.provider, "claude_api:claude-sonnet-5-5");
        assert_eq!(g.created_at, at("2026-10-13T22:00:00Z"));
        assert_eq!(g.item_ids, vec![1, 2]);
        assert!(s.get_digest(Period::Day, start, &DigestScope::Category(Category::Arxiv)).unwrap().is_none());
        assert!(s.get_digest(Period::Day, start, &DigestScope::Topic(4)).unwrap().is_some());
    }

    #[test]
    fn reset_for_db_id_wipes_synced_data_and_keeps_the_rest() {
        let (s, _) = store_with_source(Category::TechJp);
        s.import_days(&[day("2026-10-13", vec![rec(1, "https://a/1", "stale", "2026-10-13T01:00:00Z")])], utc(), true)
            .unwrap();
        s.import_digests(Period::Day, d("2026-10-13"), &[entry(serde_json::json!({"category": "github"}), "b", "2026-10-13T22:00:00.000Z")])
            .unwrap();
        s.add_topic("AI", &["AI".to_string()]).unwrap();
        s.create_chat_thread("話", &at("2026-10-13T00:00:00Z")).unwrap();
        s.set_setting("sync.device_name", "Mac").unwrap();
        s.put_sync_file(&crate::store::SyncFileRow {
            name: "day-2026-10-13.json.gz".into(),
            kind: "in".into(),
            drive_id: Some("id1".into()),
            sha256: Some("x".into()),
            updated_at: "t".into(),
            fingerprint: None,
        })
        .unwrap();

        s.reset_for_db_id("db2").unwrap();

        assert!(s.query_items(&ItemFilter::default()).unwrap().is_empty());
        assert!(s.list_digests(Period::Day, d("2026-10-13")).unwrap().is_empty());
        assert!(s.sync_files("in").unwrap().is_empty());
        assert_eq!(s.get_setting("sync.db_id").unwrap().as_deref(), Some("db2"));
        assert_eq!(s.get_setting("sync.device_name").unwrap().as_deref(), Some("Mac"));
        assert_eq!(s.list_topics().unwrap().len(), 1);
        assert_eq!(s.list_sources().unwrap().len(), 1);
        assert_eq!(s.list_chat_threads().unwrap().len(), 1);
        // 同じ id の記事を入れ直しても、古いタイトルが索引に残っていない。
        s.import_days(&[day("2026-10-13", vec![rec(1, "https://b/1", "fresh", "2026-10-13T01:00:00Z")])], utc(), true)
            .unwrap();
        assert!(search(&s, "stale").is_empty());
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p infodesk-core store::reader`
Expected: FAIL to compile with "no method named `import_days`" (and `import_digests`, `reset_for_db_id`, `DayImport`).

- [ ] **Step 3: Write the implementation**

In `crates/core/src/store/items.rs` change `fn word_index_text(s: &str) -> String` to `pub(super) fn word_index_text(s: &str) -> String`.

In `crates/core/src/store/reader.rs` extend the imports:

```rust
use std::collections::HashSet;

use chrono::{DateTime, FixedOffset, NaiveDate, Utc};
use rusqlite::{Connection, OptionalExtension, params};

use super::items::word_index_text;
use super::sync::day_bounds;
use super::{Store, ts};
use crate::error::{Error, Result};
use crate::model::{Category, DigestScope, Period};
use crate::sync::format::{DayFile, DigestRecord, ItemRecord};
use crate::sync::mode::{MODE_READER, MODE_WRITER, SETTING_MODE};
use crate::sync::nas::SETTING_ENABLED;
```

Add above `impl Store`:

```rust
/// 日ファイルの取り込みの結果。`skipped` は取り込めなかった記事の説明 (画面の警告になる)。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DayImport {
    pub upserted: usize,
    pub removed: usize,
    pub skipped: Vec<String>,
}

/// ダイジェストファイルの取り込みの結果。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DigestImport {
    pub saved: usize,
    pub removed: usize,
    pub skipped: Vec<String>,
}

/// RFC 3339 の時刻を DB の形 (`ts`: 秒まで、UTC、末尾 Z) にする。読めなければ None。
fn norm_ts(s: &str) -> Option<String> {
    DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|d| ts(&d.with_timezone(&Utc)))
}

fn unindex_item(conn: &Connection, id: i64) -> Result<()> {
    conn.execute("DELETE FROM items_fts WHERE rowid = ?1", params![id])?;
    conn.execute("DELETE FROM items_words WHERE rowid = ?1", params![id])?;
    Ok(())
}

/// 記事を消す。検索の索引 (`items_fts`、`items_words`) からも消す。
fn delete_item(conn: &Connection, id: i64) -> Result<()> {
    unindex_item(conn, id)?;
    conn.execute("DELETE FROM items WHERE id = ?1", params![id])?;
    Ok(())
}

/// 1 件を id で入れるか更新する。既読、スター、その時刻と端末、`author` は変えない。
/// 同じ URL を別の id で持っていたら、先に消す (url は UNIQUE)。索引は作り直す。
fn upsert_record(conn: &Connection, r: &ItemRecord, fetched_at: &str) -> Result<()> {
    let others: Vec<i64> = {
        let mut stmt = conn.prepare("SELECT id FROM items WHERE url = ?1 AND id != ?2")?;
        stmt.query_map(params![r.url, r.id], |row| row.get(0))?
            .collect::<rusqlite::Result<_>>()?
    };
    for id in others {
        delete_item(conn, id)?;
    }
    let published_at = r.published_at.as_deref().and_then(norm_ts);
    conn.execute(
        "INSERT INTO items(id, source_id, source_name, category, url, title, excerpt, \
         published_at, fetched_at, score, ai_summary, image_url) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12) \
         ON CONFLICT(id) DO UPDATE SET source_id = excluded.source_id, \
         source_name = excluded.source_name, category = excluded.category, url = excluded.url, \
         title = excluded.title, excerpt = excluded.excerpt, published_at = excluded.published_at, \
         fetched_at = excluded.fetched_at, score = excluded.score, \
         ai_summary = excluded.ai_summary, image_url = excluded.image_url",
        params![
            r.id,
            r.source_id,
            r.source_name,
            r.category,
            r.url,
            r.title,
            r.excerpt,
            published_at,
            fetched_at,
            r.score,
            r.ai_summary,
            r.image_url,
        ],
    )?;
    unindex_item(conn, r.id)?;
    conn.execute(
        "INSERT INTO items_fts(rowid, title, excerpt) VALUES (?1, ?2, ?3)",
        params![r.id, r.title, r.excerpt],
    )?;
    conn.execute(
        "INSERT INTO items_words(rowid, title, excerpt) VALUES (?1, ?2, ?3)",
        params![r.id, word_index_text(&r.title), word_index_text(&r.excerpt)],
    )?;
    Ok(())
}
```

Add inside `impl Store`:

```rust
    /// 日ファイルを取り込む。1 つのトランザクションで、まず全ファイルの記事を id で入れるか更新し、
    /// `remove_missing` ならそのあと、各日 (`tz` の日付) に取得した記事のうちその日のファイルにない
    /// id を消す。全部入れてから消すので、日をまたいで移った記事 (redate) は消えず、印も残る。
    /// カテゴリーを知らない、取得時刻が読めない記事は入れずに `skipped` に書く (その id の手元の
    /// 記事は消さない)。公開時刻が読めなければ NULL にする。
    pub fn import_days(
        &self,
        days: &[DayFile],
        tz: FixedOffset,
        remove_missing: bool,
    ) -> Result<DayImport> {
        let tx = self.conn.unchecked_transaction()?;
        let mut out = DayImport::default();
        let mut keep: Vec<(NaiveDate, HashSet<i64>)> = Vec::new();
        for file in days {
            let date = NaiveDate::parse_from_str(&file.date, "%Y-%m-%d")
                .map_err(|e| Error::Invalid(format!("bad day file date {}: {e}", file.date)))?;
            let mut ids = HashSet::new();
            for r in &file.items {
                ids.insert(r.id);
                if Category::parse(&r.category).is_none() {
                    out.skipped.push(format!(
                        "{} の記事 {} を取り込めませんでした (カテゴリー {} を知りません)",
                        file.date, r.id, r.category
                    ));
                    continue;
                }
                let Some(fetched_at) = norm_ts(&r.fetched_at) else {
                    out.skipped.push(format!(
                        "{} の記事 {} を取り込めませんでした (取得時刻が読めません)",
                        file.date, r.id
                    ));
                    continue;
                };
                upsert_record(&tx, r, &fetched_at)?;
                out.upserted += 1;
            }
            keep.push((date, ids));
        }
        if remove_missing {
            for (date, ids) in &keep {
                let (start, end) = day_bounds(*date, tz)?;
                let local: Vec<i64> = {
                    let mut stmt = tx.prepare(
                        "SELECT id FROM items WHERE fetched_at >= ?1 AND fetched_at < ?2",
                    )?;
                    stmt.query_map(params![ts(&start), ts(&end)], |r| r.get(0))?
                        .collect::<rusqlite::Result<_>>()?
                };
                for id in local.into_iter().filter(|id| !ids.contains(id)) {
                    delete_item(&tx, id)?;
                    out.removed += 1;
                }
            }
        }
        tx.commit()?;
        Ok(out)
    }

    /// 1 期間のダイジェストファイルを取り込む。期間ごと置き換える: 各要約を `done` で入れるか更新し
    /// (`created_at` と `provider` はファイルのまま、再試行の回数とエラーは消す)、ファイルにない scope の
    /// 行は消す。scope が読めない、`done` でない、時刻が読めない要約は `skipped` に書く。
    pub fn import_digests(
        &self,
        period: Period,
        start: NaiveDate,
        entries: &[DigestRecord],
    ) -> Result<DigestImport> {
        let start_s = start.format("%Y-%m-%d").to_string();
        let tx = self.conn.unchecked_transaction()?;
        let mut out = DigestImport::default();
        let mut keys = HashSet::new();
        for e in entries {
            let scope = serde_json::from_value::<DigestScope>(e.scope.clone()).ok();
            let created_at = norm_ts(&e.created_at);
            let (Some(scope), Some(created_at), "done") = (scope, created_at, e.status.as_str())
            else {
                out.skipped.push(format!(
                    "{} {} の要約を取り込めませんでした (scope {})",
                    period.as_str(),
                    start_s,
                    e.scope
                ));
                continue;
            };
            tx.execute(
                "INSERT INTO digests(period, period_start, scope, body, item_ids, provider, status, \
                 attempts, created_at, last_error) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'done', 0, ?7, NULL) \
                 ON CONFLICT(period, period_start, scope) DO UPDATE SET body = excluded.body, \
                 item_ids = excluded.item_ids, provider = excluded.provider, status = 'done', \
                 attempts = 0, created_at = excluded.created_at, last_error = NULL",
                params![
                    period.as_str(),
                    start_s,
                    scope.key(),
                    e.body,
                    serde_json::to_string(&e.item_ids)?,
                    e.provider,
                    created_at,
                ],
            )?;
            keys.insert(scope.key());
            out.saved += 1;
        }
        let local: Vec<(i64, String)> = {
            let mut stmt =
                tx.prepare("SELECT id, scope FROM digests WHERE period = ?1 AND period_start = ?2")?;
            stmt.query_map(params![period.as_str(), start_s], |r| Ok((r.get(0)?, r.get(1)?)))?
                .collect::<rusqlite::Result<_>>()?
        };
        for (id, scope) in local {
            if !keys.contains(&scope) {
                tx.execute("DELETE FROM digests WHERE id = ?1", params![id])?;
                out.removed += 1;
            }
        }
        tx.commit()?;
        Ok(out)
    }

    /// 保存先の `db_id` が変わった (サーバーの DB を作り直した)。記事、索引、ダイジェスト、送っていない
    /// 操作、ファイルの記録を消し、新しい `db_id` を覚える。ソース、トピック、設定、チャットは残す。
    pub fn reset_for_db_id(&self, db_id: &str) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        tx.execute_batch(
            "DELETE FROM items; DELETE FROM items_fts; DELETE FROM items_words; \
             DELETE FROM digests; DELETE FROM sync_ops; DELETE FROM sync_files;",
        )?;
        put_setting(&tx, "sync.db_id", db_id)?;
        tx.commit()?;
        Ok(())
    }
```

In `crates/core/src/store/mod.rs` add `pub use reader::{DayImport, DigestImport};`.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p infodesk-core store::`
Expected: PASS (the new 8 tests plus every existing store test).

- [ ] **Step 5: Full checks and commit**

Run the Global Constraints checks.

```bash
git add crates/core/src/store/reader.rs crates/core/src/store/items.rs crates/core/src/store/mod.rs
git commit -m "feat(core): import day and digest files and reset for a new db_id"
```

---

### Task 3: Reader pass: read the folder

**Files:**
- Create: `crates/core/src/sync/reader.rs`
- Modify: `crates/core/src/sync/mod.rs` (add `pub mod reader;` after `pub mod nas;`)
- Modify: `crates/core/src/sync/engine.rs` (visibility only: `const MAX_SEQ`, `const OPS_PER_FILE`, `const MIME_JSON`, `const MANIFEST`, `fn st`, `fn locked`, `fn ops_sort_key`, `fn local_ops` become `pub(crate)`)

**Interfaces:**
- Consumes: Task 2's `import_days`, `import_digests`, `reset_for_db_id`; `Store::{sync_files, put_sync_file, apply_remote_ops, set_setting, get_setting}`; `engine::{SyncError, locked, st, ops_sort_key, local_ops, MANIFEST}`; `format::*`; `DriveStore`.
- Produces (used by Task 4 and Task 5):
  - `pub struct ReaderInput { pub device_id: String, pub now: DateTime<Utc>, pub tz: FixedOffset }` (`Debug, Clone`)
  - `pub struct ReaderReport { pub imported_files: usize, pub applied_ops: usize, pub uploaded_ops: usize, pub warnings: Vec<String> }` (`Debug, Clone, Default, PartialEq, Eq`)
  - `pub async fn run_reader_sync(store: &Mutex<Store>, drive: &dyn DriveStore, input: &ReaderInput) -> Result<ReaderReport, SyncError>`
  - `pub const NO_DATA_MESSAGE: &str = "同期フォルダにまだデータがありません"`, `pub const NEWER_SCHEMA_MESSAGE: &str = "同期データの形式が新しくなっています。アプリを更新してください"`, `pub const MANIFEST_UNREADABLE_MESSAGE: &str = "manifest.json を読めませんでした"`, `pub const SETTING_READER_WRITER: &str = "sync.reader.writer"`, `pub const SETTING_READER_WRITTEN_AT: &str = "sync.reader.written_at"`
  - Test helpers in `reader::tests` reused by Task 4: `writer()`, `writer_input(now)`, `reader()`, `reader_input(m, now)`, `add_item(m, url, fetched)`, `urls(m)`, `files(drive)`, `Spy`.

- [ ] **Step 1: Write the failing tests**

Create `crates/core/src/sync/reader.rs` with the test module only:

```rust
#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use async_trait::async_trait;
    use chrono::{FixedOffset, NaiveDate};

    use super::*;
    use crate::model::{Category, DigestScope, ItemFilter, NewItem, Period};
    use crate::period::period_start;
    use crate::store::{NewDigest, RedateRange};
    use crate::sync::drive::{DriveError, MemoryDrive, RemoteFile};
    use crate::sync::engine::{SyncInput, run_sync};
    use crate::sync::format::{DayFile, Field, Op, OpsFile, gzip_json, ops_file_name};
    use crate::testutil::{at, new_item, store_with_source};

    pub(super) const NOW: &str = "2026-10-13T12:00:00Z";

    pub(super) fn tz() -> FixedOffset {
        FixedOffset::east_opt(0).unwrap()
    }

    /// The server: the only writer.
    pub(super) fn writer() -> Mutex<Store> {
        let s = store_with_source(Category::TechJp).0;
        s.set_setting("sync.enabled", "true").unwrap();
        s.set_setting("sync.device_id", "server").unwrap();
        Mutex::new(s)
    }

    pub(super) fn writer_input(now: &str) -> SyncInput {
        SyncInput {
            device_id: "server".into(),
            db_id: "db1".into(),
            writer: "infodesk-server".into(),
            now: at(now),
            tz: tz(),
            compact: true,
        }
    }

    /// The Mac: same database id as the server, switched to reader.
    pub(super) fn reader() -> Mutex<Store> {
        let s = store_with_source(Category::TechJp).0;
        s.set_setting("sync.enabled", "true").unwrap();
        s.set_setting("sync.db_id", "db1").unwrap();
        s.switch_to_reader().unwrap();
        Mutex::new(s)
    }

    pub(super) fn reader_input(m: &Mutex<Store>, now: &str) -> ReaderInput {
        let device_id = m.lock().unwrap().get_setting("sync.device_id").unwrap().unwrap();
        ReaderInput { device_id, now: at(now), tz: tz() }
    }

    pub(super) fn add(m: &Mutex<Store>, item: NewItem, fetched: &str) -> i64 {
        let s = m.lock().unwrap();
        let src = s.list_sources().unwrap().remove(0);
        let url = item.url.clone();
        s.upsert_items(&src, &[item], &at(fetched)).unwrap();
        s.query_items(&ItemFilter { limit: 1000, ..Default::default() })
            .unwrap()
            .into_iter()
            .find(|i| i.url == url)
            .unwrap()
            .id
    }

    pub(super) fn add_item(m: &Mutex<Store>, url: &str, fetched: &str) -> i64 {
        add(m, new_item(url, "t", "", None), fetched)
    }

    /// (id, url, title, fetched_at, ai_summary, read, starred), by id.
    #[allow(clippy::type_complexity)]
    pub(super) fn rows(m: &Mutex<Store>) -> Vec<(i64, String, String, String, Option<String>, bool, bool)> {
        let mut v: Vec<_> = m
            .lock()
            .unwrap()
            .query_items(&ItemFilter { limit: 1000, ..Default::default() })
            .unwrap()
            .into_iter()
            .map(|i| (i.id, i.url, i.title, i.fetched_at.to_rfc3339(), i.ai_summary, i.read, i.starred))
            .collect();
        v.sort();
        v
    }

    pub(super) fn urls(m: &Mutex<Store>) -> Vec<String> {
        rows(m).into_iter().map(|r| r.1).collect()
    }

    fn digests_of(m: &Mutex<Store>, p: Period, start: NaiveDate) -> Vec<(String, String, Vec<i64>, String, String)> {
        m.lock()
            .unwrap()
            .list_digests(p, start)
            .unwrap()
            .into_iter()
            .map(|d| (d.scope.key(), d.body, d.item_ids, d.provider, d.created_at.to_rfc3339()))
            .collect()
    }

    fn save(m: &Mutex<Store>, p: Period, start: NaiveDate, body: &str, ids: Vec<i64>) {
        m.lock()
            .unwrap()
            .save_digest_done(
                &NewDigest {
                    period: p,
                    period_start: start,
                    scope: DigestScope::Category(Category::TechJp),
                    body: body.into(),
                    item_ids: ids,
                    provider: "claude_api:x".into(),
                },
                &at(NOW),
            )
            .unwrap();
    }

    pub(super) async fn files(drive: &MemoryDrive) -> BTreeMap<String, Vec<u8>> {
        let mut out = BTreeMap::new();
        for f in drive.list().await.unwrap() {
            out.insert(f.name.clone(), drive.download(&f.id).await.unwrap());
        }
        out
    }

    fn day(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    /// Counts downloads and records every write.
    #[derive(Default)]
    pub(super) struct Spy {
        pub(super) inner: MemoryDrive,
        pub(super) downloads: AtomicUsize,
        pub(super) writes: Mutex<Vec<String>>,
    }

    #[async_trait]
    impl DriveStore for Spy {
        async fn list(&self) -> Result<Vec<RemoteFile>, DriveError> {
            self.inner.list().await
        }
        async fn download(&self, id: &str) -> Result<Vec<u8>, DriveError> {
            self.downloads.fetch_add(1, Ordering::SeqCst);
            self.inner.download(id).await
        }
        async fn create(&self, name: &str, bytes: Vec<u8>, mime: &str) -> Result<RemoteFile, DriveError> {
            self.writes.lock().unwrap().push(format!("create {name}"));
            self.inner.create(name, bytes, mime).await
        }
        async fn update(&self, id: &str, bytes: Vec<u8>, mime: &str) -> Result<RemoteFile, DriveError> {
            self.writes.lock().unwrap().push(format!("update {id}"));
            self.inner.update(id, bytes, mime).await
        }
        async fn delete(&self, id: &str) -> Result<(), DriveError> {
            self.writes.lock().unwrap().push(format!("delete {id}"));
            self.inner.delete(id).await
        }
    }

    #[tokio::test]
    async fn round_trip_matches_the_writer_items_digests_and_marks() {
        let w = writer();
        let a1 = add_item(&w, "https://a/1", "2026-10-12T03:00:00Z");
        let a2 = add_item(&w, "https://a/2", "2026-10-13T03:00:00Z");
        w.lock().unwrap().set_read(a1, true).unwrap();
        w.lock().unwrap().set_starred(a2, true).unwrap();
        w.lock().unwrap().set_ai_summary(a2, "要約").unwrap();
        let week = period_start(Period::Week, day("2026-10-13"));
        save(&w, Period::Day, day("2026-10-13"), "日次", vec![a2]);
        save(&w, Period::Week, week, "週次", vec![a1, a2]);
        let drive = MemoryDrive::new();
        run_sync(&w, &drive, &writer_input(NOW)).await.unwrap();

        let r = reader();
        let report = run_reader_sync(&r, &drive, &reader_input(&r, NOW)).await.unwrap();

        assert!(report.warnings.is_empty(), "{:?}", report.warnings);
        assert!(report.imported_files >= 4, "{report:?}");
        assert_eq!(rows(&r), rows(&w));
        assert_eq!(digests_of(&r, Period::Day, day("2026-10-13")), digests_of(&w, Period::Day, day("2026-10-13")));
        assert_eq!(digests_of(&r, Period::Week, week), digests_of(&w, Period::Week, week));
        let s = r.lock().unwrap();
        assert_eq!(s.get_setting(SETTING_READER_WRITER).unwrap().as_deref(), Some("infodesk-server"));
        assert!(s.get_setting(SETTING_READER_WRITTEN_AT).unwrap().is_some());
    }

    #[tokio::test]
    async fn a_copied_database_is_updated_in_place() {
        let w = writer();
        let id = add_item(&w, "https://a/1", "2026-10-13T01:00:00Z");
        w.lock().unwrap().set_ai_summary(id, "サーバーの要約").unwrap();
        let drive = MemoryDrive::new();
        run_sync(&w, &drive, &writer_input(NOW)).await.unwrap();
        // The Mac had the same article (same id) and read it before the switch.
        let r = reader();
        assert_eq!(add_item(&r, "https://a/1", "2026-10-13T01:00:00Z"), id);
        r.lock().unwrap().set_read(id, true).unwrap();

        run_reader_sync(&r, &drive, &reader_input(&r, NOW)).await.unwrap();

        let item = r.lock().unwrap().get_item(id).unwrap().unwrap();
        assert_eq!(item.ai_summary.as_deref(), Some("サーバーの要約"));
        assert!(item.read, "the Mac's own mark stays");
        assert_eq!(urls(&r), vec!["https://a/1"]);
    }

    #[tokio::test]
    async fn redated_articles_move_days_on_the_reader_and_keep_marks() {
        // Review Focus 1
        let w = writer();
        let moved = add(
            &w,
            NewItem { published_at: Some(at("2026-10-10T08:00:00Z")), ..new_item("https://a/moved", "t", "", None) },
            "2026-10-13T03:00:00Z",
        );
        let stays = add_item(&w, "https://a/stays", "2026-10-13T04:00:00Z");
        let drive = MemoryDrive::new();
        run_sync(&w, &drive, &writer_input(NOW)).await.unwrap();
        let r = reader();
        run_reader_sync(&r, &drive, &reader_input(&r, NOW)).await.unwrap();
        r.lock().unwrap().set_starred(moved, true).unwrap();

        let range = RedateRange { fetched_on: day("2026-10-13"), from: day("2026-10-10"), to: day("2026-10-12") };
        w.lock().unwrap().redate_apply(&range, tz()).unwrap();
        run_sync(&w, &drive, &writer_input("2026-10-13T12:10:00Z")).await.unwrap();
        let report = run_reader_sync(&r, &drive, &reader_input(&r, "2026-10-13T12:11:00Z")).await.unwrap();

        assert!(report.warnings.is_empty(), "{:?}", report.warnings);
        let s = r.lock().unwrap();
        let m = s.get_item(moved).unwrap().unwrap();
        assert_eq!(m.fetched_at, at("2026-10-10T08:00:00Z"));
        assert!(m.starred, "the mark survives the move");
        assert!(s.get_item(stays).unwrap().is_some());
        drop(s);
        assert_eq!(urls(&r).len(), 2);
    }

    #[tokio::test]
    async fn a_new_database_id_wipes_items_and_digests_but_keeps_sources_topics_and_settings() {
        let w = writer();
        add_item(&w, "https://server/1", "2026-10-13T01:00:00Z");
        let drive = MemoryDrive::new();
        run_sync(&w, &drive, &SyncInput { db_id: "db2".into(), ..writer_input(NOW) }).await.unwrap();
        let r = reader();
        add_item(&r, "https://mac-only/1", "2026-10-13T02:00:00Z");
        save(&r, Period::Day, day("2026-10-12"), "手元だけ", vec![]);
        r.lock().unwrap().add_topic("AI", &["AI".to_string()]).unwrap();
        r.lock().unwrap().set_setting("sync.device_name", "Mac").unwrap();

        run_reader_sync(&r, &drive, &reader_input(&r, NOW)).await.unwrap();

        assert_eq!(urls(&r), vec!["https://server/1"]);
        assert!(digests_of(&r, Period::Day, day("2026-10-12")).is_empty());
        let s = r.lock().unwrap();
        assert_eq!(s.get_setting("sync.db_id").unwrap().as_deref(), Some("db2"));
        assert_eq!(s.get_setting("sync.device_name").unwrap().as_deref(), Some("Mac"));
        assert_eq!(s.list_topics().unwrap().len(), 1);
        assert_eq!(s.list_sources().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn no_manifest_or_a_newer_schema_changes_nothing() {
        let r = reader();
        let drive = MemoryDrive::new();
        let report = run_reader_sync(&r, &drive, &reader_input(&r, NOW)).await.unwrap();
        assert_eq!(report.warnings, vec![NO_DATA_MESSAGE.to_string()]);
        assert!(drive.names().is_empty());

        drive.put("manifest.json", serde_json::to_vec(&serde_json::json!({"schema": 2, "layout": "new"})).unwrap());
        let report = run_reader_sync(&r, &drive, &reader_input(&r, NOW)).await.unwrap();
        assert_eq!(report.warnings, vec![NEWER_SCHEMA_MESSAGE.to_string()]);
        assert_eq!(r.lock().unwrap().get_setting("sync.db_id").unwrap().as_deref(), Some("db1"));
        assert_eq!(drive.names(), vec!["manifest.json"]);

        drive.put("manifest.json", b"{not json".to_vec());
        let report = run_reader_sync(&r, &drive, &reader_input(&r, NOW)).await.unwrap();
        assert_eq!(report.warnings, vec![MANIFEST_UNREADABLE_MESSAGE.to_string()]);
    }

    #[tokio::test]
    async fn a_file_whose_hash_differs_is_skipped_and_imported_later() {
        // Review Focus 3
        let w = writer();
        add_item(&w, "https://a/1", "2026-10-12T03:00:00Z");
        add_item(&w, "https://a/2", "2026-10-13T03:00:00Z");
        let drive = MemoryDrive::new();
        run_sync(&w, &drive, &writer_input(NOW)).await.unwrap();
        let good = files(&drive).await["day-2026-10-13.json.gz"].clone();
        drive.put(
            "day-2026-10-13.json.gz",
            gzip_json(&DayFile { date: "2026-10-13".into(), items: vec![] }).unwrap(),
        );
        let r = reader();

        let report = run_reader_sync(&r, &drive, &reader_input(&r, NOW)).await.unwrap();
        assert!(
            report.warnings.iter().any(|w| w.contains("day-2026-10-13.json.gz") && w.contains("ハッシュ")),
            "{:?}",
            report.warnings
        );
        assert_eq!(urls(&r), vec!["https://a/1"]);

        drive.put("day-2026-10-13.json.gz", good);
        let report = run_reader_sync(&r, &drive, &reader_input(&r, NOW)).await.unwrap();
        assert!(report.warnings.is_empty(), "{:?}", report.warnings);
        assert_eq!(urls(&r), vec!["https://a/1", "https://a/2"]);
    }

    #[tokio::test]
    async fn an_unchanged_folder_downloads_only_the_manifest() {
        let w = writer();
        add_item(&w, "https://a/1", "2026-10-13T03:00:00Z");
        let spy = Spy::default();
        run_sync(&w, &spy.inner, &writer_input(NOW)).await.unwrap();
        let r = reader();
        run_reader_sync(&r, &spy, &reader_input(&r, NOW)).await.unwrap();
        spy.downloads.store(0, Ordering::SeqCst);
        let report = run_reader_sync(&r, &spy, &reader_input(&r, NOW)).await.unwrap();
        assert_eq!(spy.downloads.load(Ordering::SeqCst), 1);
        assert_eq!((report.imported_files, report.applied_ops), (0, 0));
    }

    #[tokio::test]
    async fn phone_ops_are_applied_once_and_ops_of_another_database_never() {
        let w = writer();
        let id = add_item(&w, "https://a/1", "2026-10-13T03:00:00Z");
        let drive = MemoryDrive::new();
        run_sync(&w, &drive, &writer_input(NOW)).await.unwrap();
        let op = |field, device: &str| Op {
            item_id: id,
            field,
            value: true,
            at: "2026-10-13T11:00:00.000Z".into(),
            device: device.into(),
        };
        for (device, db, field) in [("phone", "db1", Field::Starred), ("oldphone", "db0", Field::Read)] {
            let file = OpsFile { device: device.into(), db_id: Some(db.into()), ops: vec![op(field, device)] };
            drive.put(
                &ops_file_name(device, at("2026-10-13T11:00:00Z"), 1),
                serde_json::to_vec(&file).unwrap(),
            );
        }
        let r = reader();
        let report = run_reader_sync(&r, &drive, &reader_input(&r, NOW)).await.unwrap();
        assert_eq!(report.applied_ops, 1);
        let item = r.lock().unwrap().get_item(id).unwrap().unwrap();
        assert!(item.starred);
        assert!(!item.read, "an op of another database is never applied");
        let again = run_reader_sync(&r, &drive, &reader_input(&r, NOW)).await.unwrap();
        assert_eq!(again.applied_ops, 0);
    }
}
```

Add `pub mod reader;` to `crates/core/src/sync/mod.rs`.

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p infodesk-core sync::reader`
Expected: FAIL to compile with "cannot find function `run_reader_sync`" (and the `ReaderInput`, constants).

- [ ] **Step 3: Write the implementation**

In `crates/core/src/sync/engine.rs` change only visibility: `pub(crate) const MAX_SEQ`, `pub(crate) const OPS_PER_FILE`, `pub(crate) const MIME_JSON`, `pub(crate) const MANIFEST`, `pub(crate) fn st`, `pub(crate) fn locked`, `pub(crate) fn ops_sort_key`, `pub(crate) fn local_ops`.

At the top of `crates/core/src/sync/reader.rs`, above the test module:

```rust
//! Reader pass, a port of the phone's `_pass` (mobile/lib/sync/engine.dart): reads the
//! folder another device writes. It never writes the manifest, meta, day, digest or
//! snapshot files and never deletes anything; its only remote writes are its own new
//! ops files. The store lock is never held across an await.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Mutex;

use chrono::{DateTime, FixedOffset, Utc};

use super::drive::{DriveStore, RemoteFile};
use super::engine::{MANIFEST, SyncError, local_ops, locked, ops_sort_key};
use super::format::{
    DayFile, DigestFile, FileKind, Manifest, Op, OpsFile, SCHEMA, StateSnapshot, gunzip_json,
    normalize_op, op_time, parse_file_name, sha256_hex,
};
use crate::store::{Store, SyncFileRow};

pub const NO_DATA_MESSAGE: &str = "同期フォルダにまだデータがありません";
pub const NEWER_SCHEMA_MESSAGE: &str =
    "同期データの形式が新しくなっています。アプリを更新してください";
pub const MANIFEST_UNREADABLE_MESSAGE: &str = "manifest.json を読めませんでした";
/// The manifest's `writer` and `written_at`, kept for display.
pub const SETTING_READER_WRITER: &str = "sync.reader.writer";
pub const SETTING_READER_WRITTEN_AT: &str = "sync.reader.written_at";

#[derive(Debug, Clone)]
pub struct ReaderInput {
    pub device_id: String,
    pub now: DateTime<Utc>,
    pub tz: FixedOffset,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReaderReport {
    /// Day, digest and snapshot files imported in this pass.
    pub imported_files: usize,
    pub applied_ops: usize,
    pub uploaded_ops: usize,
    /// Japanese messages for the Sync tab. Nothing in them stopped the pass.
    pub warnings: Vec<String>,
}

/// A data file listed in the manifest whose hash differs from the recorded import.
struct Todo {
    name: String,
    kind: FileKind,
    sha256: String,
    id: String,
}

fn hash_mismatch(name: &str) -> String {
    format!("{name} のハッシュが一致しないため取り込みませんでした")
}

fn unreadable(name: &str) -> String {
    format!("{name} を読めませんでした")
}

fn in_row(name: &str, id: &str, sha: &str, now: DateTime<Utc>) -> SyncFileRow {
    SyncFileRow {
        name: name.to_string(),
        kind: "in".to_string(),
        drive_id: Some(id.to_string()),
        sha256: Some(sha.to_string()),
        updated_at: op_time(now),
        fingerprint: None,
    }
}

struct Pass<'a> {
    store: &'a Mutex<Store>,
    drive: &'a dyn DriveStore,
    input: &'a ReaderInput,
    remote: BTreeMap<String, RemoteFile>,
    report: ReaderReport,
}

impl Pass<'_> {
    /// Reads the manifest. `None` (with a warning) when there is none, it is
    /// unreadable, or its schema is not ours; the pass then touches nothing.
    async fn manifest(&mut self) -> Result<Option<Manifest>, SyncError> {
        let Some(f) = self.remote.get(MANIFEST).cloned() else {
            self.report.warnings.push(NO_DATA_MESSAGE.to_string());
            return Ok(None);
        };
        let bytes = self.drive.download(&f.id).await?;
        // The schema is read first, so a newer layout is reported as newer.
        let Ok(value) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
            self.report.warnings.push(MANIFEST_UNREADABLE_MESSAGE.to_string());
            return Ok(None);
        };
        match value.get("schema").and_then(serde_json::Value::as_u64) {
            Some(s) if s == u64::from(SCHEMA) => {}
            Some(_) => {
                self.report.warnings.push(NEWER_SCHEMA_MESSAGE.to_string());
                return Ok(None);
            }
            None => {
                self.report.warnings.push(MANIFEST_UNREADABLE_MESSAGE.to_string());
                return Ok(None);
            }
        }
        match serde_json::from_value::<Manifest>(value) {
            Ok(m) => Ok(Some(m)),
            Err(_) => {
                self.report.warnings.push(MANIFEST_UNREADABLE_MESSAGE.to_string());
                Ok(None)
            }
        }
    }

    /// Files of the kind `want` accepts that the manifest lists, the folder
    /// holds, and whose hash differs from the recorded import.
    fn changed(&self, manifest: &Manifest, want: fn(&FileKind) -> bool) -> Result<Vec<Todo>, SyncError> {
        let done: HashMap<String, String> = locked(self.store, |s| s.sync_files("in"))?
            .into_iter()
            .filter_map(|r| r.sha256.map(|h| (r.name, h)))
            .collect();
        let mut out = Vec::new();
        for (name, entry) in &manifest.files {
            let (Some(kind), Some(file)) = (parse_file_name(name), self.remote.get(name)) else {
                continue;
            };
            if !want(&kind) || done.get(name) == Some(&entry.sha256) {
                continue;
            }
            out.push(Todo {
                name: name.clone(),
                kind,
                sha256: entry.sha256.clone(),
                id: file.id.clone(),
            });
        }
        Ok(out)
    }

    /// Downloads a data file and checks it against the manifest's hash.
    /// `None` (with a warning) on a mismatch.
    async fn fetch(&mut self, t: &Todo) -> Result<Option<Vec<u8>>, SyncError> {
        let bytes = self.drive.download(&t.id).await?;
        if sha256_hex(&bytes) != t.sha256 {
            self.report.warnings.push(hash_mismatch(&t.name));
            return Ok(None);
        }
        Ok(Some(bytes))
    }

    fn record(&self, t: &Todo) -> Result<(), SyncError> {
        let row = in_row(&t.name, &t.id, &t.sha256, self.input.now);
        locked(self.store, |s| s.put_sync_file(&row))
    }

    /// Imports every changed day file in one store call. When any of them
    /// failed, nothing is deleted and none is recorded, so all are retried.
    async fn import_days(&mut self, manifest: &Manifest) -> Result<(), SyncError> {
        let todo = self.changed(manifest, |k| matches!(k, FileKind::Day(_)))?;
        let mut parsed: Vec<DayFile> = Vec::new();
        let mut all_ok = true;
        for t in &todo {
            let FileKind::Day(date) = t.kind else { continue };
            let Some(bytes) = self.fetch(t).await? else {
                all_ok = false;
                continue;
            };
            match gunzip_json::<DayFile>(&bytes) {
                Ok(f) if f.date == date.format("%Y-%m-%d").to_string() => parsed.push(f),
                _ => {
                    self.report.warnings.push(unreadable(&t.name));
                    all_ok = false;
                }
            }
        }
        if parsed.is_empty() {
            return Ok(());
        }
        let tz = self.input.tz;
        let result = locked(self.store, |s| s.import_days(&parsed, tz, all_ok))?;
        self.report.warnings.extend(result.skipped);
        self.report.imported_files += parsed.len();
        if all_ok {
            for t in &todo {
                self.record(t)?;
            }
        }
        Ok(())
    }

    async fn import_digests(&mut self, manifest: &Manifest) -> Result<(), SyncError> {
        for t in self.changed(manifest, |k| matches!(k, FileKind::Digest(..)))? {
            let FileKind::Digest(period, start) = t.kind else { continue };
            let Some(bytes) = self.fetch(&t).await? else { continue };
            let file = match gunzip_json::<DigestFile>(&bytes) {
                Ok(f)
                    if f.period == period.as_str()
                        && f.period_start == start.format("%Y-%m-%d").to_string() =>
                {
                    f
                }
                _ => {
                    self.report.warnings.push(unreadable(&t.name));
                    continue;
                }
            };
            let result = locked(self.store, |s| s.import_digests(period, start, &file.entries))?;
            self.report.warnings.extend(result.skipped);
            self.record(&t)?;
            self.report.imported_files += 1;
        }
        Ok(())
    }

    /// Applies the snapshot as ops: each stamp competes with the stored one.
    async fn import_snapshot(&mut self, manifest: &Manifest) -> Result<(), SyncError> {
        for t in self.changed(manifest, |k| matches!(k, FileKind::Snapshot))? {
            let Some(bytes) = self.fetch(&t).await? else { continue };
            let Ok(snap) = gunzip_json::<StateSnapshot>(&bytes) else {
                self.report.warnings.push(unreadable(&t.name));
                continue;
            };
            let ops: Vec<Op> = local_ops(snap.state).filter_map(|op| normalize_op(&op)).collect();
            let device = self.input.device_id.clone();
            self.report.applied_ops += locked(self.store, |s| s.apply_remote_ops(&ops, &device))?;
            self.record(&t)?;
            self.report.imported_files += 1;
        }
        Ok(())
    }

    /// Applies every ops file not yet imported, oldest first, this device's
    /// own included. A file of another database is recorded, never applied.
    async fn apply_ops_files(&mut self, manifest: &Manifest) -> Result<(), SyncError> {
        let done: HashSet<String> = locked(self.store, |s| s.sync_files("in"))?
            .into_iter()
            .map(|r| r.name)
            .collect();
        let mut names: Vec<String> = self
            .remote
            .keys()
            .filter(|n| matches!(parse_file_name(n), Some(FileKind::Ops { .. })) && !done.contains(*n))
            .cloned()
            .collect();
        names.sort_by_key(|n| ops_sort_key(n));
        for name in names {
            let id = self.remote[&name].id.clone();
            let bytes = self.drive.download(&id).await?;
            match serde_json::from_slice::<OpsFile>(&bytes) {
                Ok(file) if file.db_id.as_ref().is_none_or(|db| *db == manifest.db_id) => {
                    let ops: Vec<Op> = file.ops.iter().filter_map(normalize_op).collect();
                    let device = self.input.device_id.clone();
                    self.report.applied_ops +=
                        locked(self.store, |s| s.apply_remote_ops(&ops, &device))?;
                }
                Ok(_) => {}
                Err(_) => self.report.warnings.push(unreadable(&name)),
            }
            let row = in_row(&name, &id, &sha256_hex(&bytes), self.input.now);
            locked(self.store, |s| s.put_sync_file(&row))?;
        }
        Ok(())
    }
}

pub async fn run_reader_sync(
    store: &Mutex<Store>,
    drive: &dyn DriveStore,
    input: &ReaderInput,
) -> Result<ReaderReport, SyncError> {
    // Only names of the sync format are ever read (a NAS folder may hold other files).
    let remote = drive
        .list()
        .await?
        .into_iter()
        .filter(|f| parse_file_name(&f.name).is_some())
        .map(|f| (f.name.clone(), f))
        .collect();
    let mut pass = Pass { store, drive, input, remote, report: ReaderReport::default() };
    let Some(manifest) = pass.manifest().await? else {
        return Ok(pass.report);
    };
    locked(store, |s| {
        s.set_setting(SETTING_READER_WRITER, &manifest.writer)?;
        s.set_setting(SETTING_READER_WRITTEN_AT, &manifest.written_at)?;
        if s.get_setting("sync.db_id")?.as_deref() != Some(manifest.db_id.as_str()) {
            s.reset_for_db_id(&manifest.db_id)?;
        }
        Ok(())
    })?;
    // Meta is never imported: the Mac keeps its own sources and topics.
    pass.import_days(&manifest).await?;
    pass.import_digests(&manifest).await?;
    pass.import_snapshot(&manifest).await?;
    pass.apply_ops_files(&manifest).await?;
    Ok(pass.report)
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p infodesk-core sync::`
Expected: PASS (the 8 new reader tests and every engine test).

- [ ] **Step 5: Full checks and commit**

Run the Global Constraints checks.

```bash
git add crates/core/src/sync/reader.rs crates/core/src/sync/mod.rs crates/core/src/sync/engine.rs
git commit -m "feat(core): reader sync pass that imports the writer's folder"
```

---

### Task 4: Reader pass: upload the Mac's own marks

**Files:**
- Modify: `crates/core/src/sync/reader.rs`

**Interfaces:**
- Consumes: Task 3's `Pass`, `in_row`, test helpers; `Store::{pending_local_ops, mark_ops_uploaded, put_sync_file}`; `engine::{OPS_PER_FILE, MAX_SEQ, MIME_JSON, st}`; `format::ops_file_name`.
- Produces: `run_reader_sync` now also fills `ReaderReport.uploaded_ops`. Each upload is one `drive.create` of `ops-<device_id>-<now>-<seq>.json` with `db_id` = the manifest's.

- [ ] **Step 1: Write the failing tests**

Append to `reader::tests`:

```rust
    #[tokio::test]
    async fn reader_marks_reach_the_writer() {
        let w = writer();
        let id = add_item(&w, "https://a/1", "2026-10-13T03:00:00Z");
        let drive = MemoryDrive::new();
        run_sync(&w, &drive, &writer_input(NOW)).await.unwrap();
        let r = reader();
        run_reader_sync(&r, &drive, &reader_input(&r, NOW)).await.unwrap();
        r.lock().unwrap().set_starred(id, true).unwrap();

        let input = reader_input(&r, "2026-10-13T12:01:00Z");
        assert_ne!(input.device_id, "server", "the writer must not skip it as its own");
        let report = run_reader_sync(&r, &drive, &input).await.unwrap();
        assert_eq!(report.uploaded_ops, 1);
        assert!(r.lock().unwrap().pending_local_ops(&input.device_id).unwrap().is_empty());
        let mine = format!("ops-{}-", input.device_id);
        assert!(drive.names().iter().any(|n| n.starts_with(&mine)), "{:?}", drive.names());

        let wr = run_sync(&w, &drive, &writer_input("2026-10-13T12:02:00Z")).await.unwrap();
        assert_eq!(wr.applied_ops, 1);
        assert!(w.lock().unwrap().get_item(id).unwrap().unwrap().starred);
        // The writer folded the batch into the snapshot and deleted it; the star stays.
        run_reader_sync(&r, &drive, &reader_input(&r, "2026-10-13T12:03:00Z")).await.unwrap();
        assert!(r.lock().unwrap().get_item(id).unwrap().unwrap().starred);
    }

    #[tokio::test]
    async fn the_reader_writes_nothing_but_its_own_new_ops_files() {
        let w = writer();
        let id = add_item(&w, "https://a/1", "2026-10-13T03:00:00Z");
        save(&w, Period::Day, day("2026-10-13"), "日次", vec![id]);
        let spy = Spy::default();
        run_sync(&w, &spy.inner, &writer_input(NOW)).await.unwrap();
        let before = files(&spy.inner).await;
        let r = reader();

        run_reader_sync(&r, &spy, &reader_input(&r, NOW)).await.unwrap();
        r.lock().unwrap().set_read(id, true).unwrap();
        let input = reader_input(&r, "2026-10-13T12:01:00Z");
        run_reader_sync(&r, &spy, &input).await.unwrap();
        run_reader_sync(&r, &spy, &reader_input(&r, "2026-10-13T12:02:00Z")).await.unwrap();

        let writes = spy.writes.lock().unwrap().clone();
        assert_eq!(writes.len(), 1, "{writes:?}");
        assert!(writes[0].starts_with(&format!("create ops-{}-", input.device_id)), "{writes:?}");
        let after = files(&spy.inner).await;
        for (name, bytes) in &before {
            assert_eq!(after.get(name), Some(bytes), "{name} unchanged");
        }
        assert_eq!(after.len(), before.len() + 1);
    }

    #[tokio::test]
    async fn a_failed_upload_keeps_the_marks_for_the_next_pass() {
        let w = writer();
        let id = add_item(&w, "https://a/1", "2026-10-13T03:00:00Z");
        let drive = MemoryDrive::new();
        run_sync(&w, &drive, &writer_input(NOW)).await.unwrap();
        let r = reader();
        run_reader_sync(&r, &drive, &reader_input(&r, NOW)).await.unwrap();
        r.lock().unwrap().set_read(id, true).unwrap();
        *drive.fail_on_name.lock().unwrap() = Some("ops-".into());

        let input = reader_input(&r, "2026-10-13T12:01:00Z");
        let err = run_reader_sync(&r, &drive, &input).await.unwrap_err();
        assert!(matches!(err, SyncError::Drive(DriveError::Retryable(_))), "{err:?}");
        assert_eq!(r.lock().unwrap().pending_local_ops(&input.device_id).unwrap().len(), 1);

        let report = run_reader_sync(&r, &drive, &reader_input(&r, "2026-10-13T12:02:00Z")).await.unwrap();
        assert_eq!(report.uploaded_ops, 1);
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p infodesk-core sync::reader`
Expected: FAIL: `reader_marks_reach_the_writer` (`left: 0, right: 1` on `uploaded_ops`), `the_reader_writes_nothing_but_its_own_new_ops_files` (`writes.len()` 0), `a_failed_upload_keeps_the_marks_for_the_next_pass` (`unwrap_err` on `Ok`).

- [ ] **Step 3: Write the implementation**

Extend the `engine` import in `reader.rs` to `use super::engine::{MANIFEST, MAX_SEQ, MIME_JSON, OPS_PER_FILE, SyncError, local_ops, locked, ops_sort_key, st};` and the `format` import with `ops_file_name`. Add to `impl Pass`:

```rust
    /// Uploads this device's unsent marks as new ops files tagged with the
    /// manifest's `db_id`, then marks them sent. Each file is recorded as
    /// imported so it is not downloaded back.
    async fn upload_own_ops(&mut self, manifest: &Manifest) -> Result<(), SyncError> {
        let device = self.input.device_id.clone();
        let pending = locked(self.store, |s| s.pending_local_ops(&device))?;
        if pending.is_empty() {
            return Ok(());
        }
        let chunk = OPS_PER_FILE.max(pending.len().div_ceil(MAX_SEQ));
        for (i, part) in pending.chunks(chunk).enumerate() {
            let name = ops_file_name(&device, self.input.now, (i + 1) as u32);
            let file = OpsFile {
                device: device.clone(),
                db_id: Some(manifest.db_id.clone()),
                ops: part.iter().map(|(_, op)| op.clone()).collect(),
            };
            let bytes = st(serde_json::to_vec(&file).map_err(Into::into))?;
            let sha = sha256_hex(&bytes);
            let created = self.drive.create(&name, bytes, MIME_JSON).await?;
            let ids: Vec<i64> = part.iter().map(|(id, _)| *id).collect();
            let row = in_row(&name, &created.id, &sha, self.input.now);
            locked(self.store, |s| {
                s.put_sync_file(&row)?;
                s.mark_ops_uploaded(&ids)
            })?;
            self.report.uploaded_ops += ids.len();
        }
        Ok(())
    }
```

In `run_reader_sync`, after `pass.apply_ops_files(&manifest).await?;` add:

```rust
    pass.upload_own_ops(&manifest).await?;
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p infodesk-core sync::`
Expected: PASS (11 reader tests and every engine test).

- [ ] **Step 5: Full checks and commit**

Run the Global Constraints checks.

```bash
git add crates/core/src/sync/reader.rs
git commit -m "feat(core): reader pass uploads the Mac's own marks as ops files"
```

---

### Task 5: App: reader branch in the sync launcher and the mode switch

**Files:**
- Modify: `app/src-tauri/src/state.rs` (`AppStatus` gains `pub reader_mode: bool`)
- Modify: `app/src-tauri/src/commands/status.rs`
- Modify: `app/src-tauri/src/sync/mod.rs` (`SyncStatus`, `refresh_status`, `run_once`, new `run_reader_once`, `record_failure`, `set_reader_mode`)
- Modify: `app/src-tauri/src/commands/sync.rs` (new command)
- Modify: `app/src-tauri/src/lib.rs` (register `commands::sync::sync_set_reader_mode` after `commands::sync::sync_set_enabled`)

**Interfaces:**
- Consumes: `infodesk_core::sync::mode::is_reader`, `infodesk_core::sync::reader::{ReaderInput, run_reader_sync, NO_DATA_MESSAGE}`, `Store::{switch_to_reader, switch_to_writer}`.
- Produces:
  - `AppStatus.reader_mode: bool` (serialized; filled only by `get_status`), `pub fn commands::status::status_with_mode(state: &AppState) -> AppStatus`
  - `SyncStatus.reader_mode: bool`
  - `pub const sync::SYNC_RUNNING_MESSAGE: &str = "同期中は切り替えられません。終わってからもう一度試してください"`
  - `pub fn sync::set_reader_mode(state: &AppState, on: bool) -> AppResult<bool>` (true when it changed the mode)
  - Tauri command `sync_set_reader_mode(on: bool) -> SyncStatus`

- [ ] **Step 1: Write the failing tests**

In `app/src-tauri/src/commands/status.rs` append:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_carries_the_reader_mode_from_the_settings() {
        let (state, _) = crate::state::test_state();
        assert!(!status_with_mode(&state).reader_mode);
        state.with_store(|s| s.switch_to_reader()).unwrap();
        assert!(status_with_mode(&state).reader_mode);
    }
}
```

In `app/src-tauri/src/sync/mod.rs` tests module, add:

```rust
    /// サーバー (書き手) が 1 回同期した保存先。記事は 1 件。
    async fn server_folder(drive: &MemoryDrive) {
        use infodesk_core::model::{Category, NewItem, SourceKind};
        use infodesk_core::sync::engine::SyncInput;
        let s = infodesk_core::Store::open_in_memory().unwrap();
        let id = s
            .add_source(SourceKind::Rss, "Feed", Category::TechJp, "https://e.com/feed")
            .unwrap();
        let src = s.get_source(id).unwrap().unwrap();
        let item = NewItem {
            url: "https://server.example/1".into(),
            title: "サーバーの記事".into(),
            excerpt: String::new(),
            author: None,
            published_at: None,
            score: None,
            image_url: None,
        };
        s.upsert_items(&src, &[item], &Utc::now()).unwrap();
        let input = SyncInput {
            device_id: "server".into(),
            db_id: "db1".into(),
            writer: "infodesk-server".into(),
            now: Utc::now(),
            tz: chrono::FixedOffset::east_opt(9 * 3600).unwrap(),
            compact: true,
        };
        run_sync(&std::sync::Mutex::new(s), drive, &input).await.unwrap();
    }

    async fn folder_bytes(drive: &MemoryDrive) -> Vec<(String, Vec<u8>)> {
        let mut out = Vec::new();
        for f in drive.list().await.unwrap() {
            out.push((f.name.clone(), drive.download(&f.id).await.unwrap()));
        }
        out
    }

    fn to_reader(state: &AppState) {
        state
            .with_store(|s| {
                s.set_setting("sync.db_id", "db1")?;
                s.switch_to_reader()
            })
            .unwrap();
    }

    #[tokio::test]
    async fn reader_mode_imports_the_folder_without_writing_it() {
        let (mut state, events) = test_state();
        let drive = Arc::new(MemoryDrive::new());
        server_folder(&drive).await;
        use_drive(&mut state, drive.clone());
        to_reader(&state);
        let before = folder_bytes(&drive).await;

        sync_once(&state).await.unwrap();

        assert_eq!(folder_bytes(&drive).await, before, "nothing written");
        let urls: Vec<String> = state
            .with_store(|s| s.query_items(&infodesk_core::model::ItemFilter::default()))
            .unwrap()
            .into_iter()
            .map(|i| i.url)
            .collect();
        assert!(urls.contains(&"https://server.example/1".to_string()), "{urls:?}");
        let st = state.sync.lock().unwrap().clone();
        assert!(st.last_sync_at.is_some());
        assert_eq!(st.error, None);
        assert!(events.lock().unwrap().contains(&EVENT_ITEMS.to_string()));
    }

    #[tokio::test]
    async fn reader_mode_shows_an_empty_folder_and_writes_nothing() {
        let (mut state, _) = test_state();
        let drive = ready(&mut state);
        to_reader(&state);
        sync_once(&state).await.unwrap();
        assert!(drive.names().is_empty(), "{:?}", drive.names());
        assert_eq!(
            state.sync.lock().unwrap().error.as_deref(),
            Some(infodesk_core::sync::reader::NO_DATA_MESSAGE)
        );
    }

    #[test]
    fn set_reader_mode_switches_both_ways_and_refuses_while_syncing() {
        // Review Focus 4
        let (state, events) = test_state();
        state.with_store(|s| s.set_setting(SETTING_ENABLED, "true")).unwrap();
        let old = state.with_store(|s| s.ensure_sync_ids()).unwrap().0;

        state.sync_running.store(true, Ordering::SeqCst);
        let err = set_reader_mode(&state, true).unwrap_err();
        assert_eq!(err.message, SYNC_RUNNING_MESSAGE);
        assert!(!status_now(&state).reader_mode);
        state.sync_running.store(false, Ordering::SeqCst);

        assert!(set_reader_mode(&state, true).unwrap());
        assert!(!state.sync_running.load(Ordering::SeqCst), "flag released");
        let st = status_now(&state);
        assert!(st.reader_mode);
        assert!(st.enabled, "turning reader on keeps sync on");
        let new = state.with_store(|s| s.get_setting("sync.device_id")).unwrap().unwrap();
        assert_ne!(new, old);
        assert!(!set_reader_mode(&state, true).unwrap(), "already a reader");
        assert!(events.lock().unwrap().contains(&EVENT_STATUS.to_string()));

        assert!(set_reader_mode(&state, false).unwrap());
        let st = status_now(&state);
        assert!(!st.reader_mode);
        assert!(!st.enabled, "turning reader off turns sync off");
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `(cd app/src-tauri && cargo test reader_mode)`
Expected: FAIL to compile with "no field `reader_mode`" and "cannot find function `set_reader_mode`".

- [ ] **Step 3: Write the implementation**

`app/src-tauri/src/state.rs`, in `AppStatus` after `last_error`:

```rust
    /// 読むだけのモード (サーバーが収集)。`get_status` が設定から埋める。ここには保存しない。
    pub reader_mode: bool,
```

`app/src-tauri/src/commands/status.rs` becomes:

```rust
use infodesk_core::sync::mode::is_reader;
use tauri::State;

use crate::state::{AppState, AppStatus};

#[tauri::command]
pub fn get_status(state: State<'_, AppState>) -> AppStatus {
    status_with_mode(&state)
}

/// 状態に、設定から読んだ読むだけのモードを添える。
pub fn status_with_mode(state: &AppState) -> AppStatus {
    let mut status = state.status();
    status.reader_mode = state.with_store(is_reader).unwrap_or(false);
    status
}
```

`app/src-tauri/src/sync/mod.rs`:

1. Imports: `use infodesk_core::sync::mode::is_reader;`, `use infodesk_core::sync::reader::{ReaderInput, run_reader_sync};`, and change `use crate::state::{AppState, EVENT_ITEMS, EVENT_SYNC};` to `use crate::state::{AppState, EVENT_DIGESTS, EVENT_ITEMS, EVENT_STATUS, EVENT_SYNC};`.
2. Constant next to `REAUTH_MESSAGE`:

```rust
/// 同期の実行中に読むだけのモードを切り替えようとした。
pub const SYNC_RUNNING_MESSAGE: &str =
    "同期中は切り替えられません。終わってからもう一度試してください";
```

3. `SyncStatus`, after `enabled`:

```rust
    /// 読むだけのモード (サーバーが収集し、この Mac は保存先を読む)。
    pub reader_mode: bool,
```

4. `refresh_status`: read the mode in the same lock and copy it:

```rust
    let read = state.with_store(|s| {
        Ok((
            s.get_setting(SETTING_ENABLED)?,
            s.get_setting(oauth::SETTING_ACCOUNT_EMAIL)?,
            s.get_setting(SETTING_DEVICE_NAME)?,
            s.get_setting(SETTING_LAST_SYNC_AT)?,
            read_nas(s)?,
            is_reader(s)?,
        ))
    });
    let (enabled, email, name, last, nas, reader) = match read {
```

and after `st.enabled = enabled.as_deref() == Some("true");` add `st.reader_mode = reader;`.

5. Extract the whole body of the existing `Err(err) => { ... }` arm of `run_once` after `state.notify(EVENT_ITEMS);` into:

```rust
/// 同期の失敗を `SyncStatus` に写す。実行中に認証が変わっていたら (`stale`)、古い結果は捨てる。
fn record_failure(state: &AppState, err: SyncError, stale: &dyn Fn() -> bool) {
    {
        // 古さの確認と書き換えを同じロックの中で行い、間に入ったサインインを消さない。
        let mut st = state.sync.lock().unwrap_or_else(|e| e.into_inner());
        if stale() {
            // 実行中にサインインし直された。古い認証の結果は捨てる。
            return;
        }
        match err {
            SyncError::Drive(DriveError::AuthExpired) => {
                reset_auth(state);
                let _ = state.with_store(|s| s.set_setting(oauth::SETTING_ACCOUNT_EMAIL, ""));
                st.signed_in = false;
                st.account_email = None;
                st.error = Some(REAUTH_MESSAGE.to_string());
            }
            SyncError::Drive(DriveError::BadCredentials) => {
                // 設定が変わるまで再試行しない。signed_in は変えない。
                state.nas_rejected.store(true, Ordering::SeqCst);
                st.error = Some(DriveError::BadCredentials.to_string());
            }
            SyncError::Drive(DriveError::Retryable(_)) => {
                st.error = Some(NETWORK_MESSAGE.to_string());
            }
            other => st.error = Some(other.to_string()),
        }
    }
    state.notify(EVENT_SYNC);
}
```

and make the writer arm:

```rust
        Err(err) => {
            // 取り込みのあとで失敗しても、取り込み済みの変更は画面に出す必要がある。
            state.notify(EVENT_ITEMS);
            record_failure(state, err, &stale);
        }
```

6. In `run_once`, replace `let now = Utc::now();` (just before `let prepared = ...`) with:

```rust
    let now = Utc::now();
    let reader = match state.with_store(is_reader) {
        Ok(r) => r,
        Err(e) => {
            record_error(state, &e.message);
            return Err(e);
        }
    };
    if reader {
        // 読むだけのモード: 書き手の同期 (`run_sync`) は決して呼ばない。
        return run_reader_once(state, drive.as_ref(), now, &stale).await;
    }
```

7. Add after `run_once`:

```rust
/// 読むだけのモードの 1 回。保存先を読み、書くのはこの Mac の ops ファイルだけ。
/// 警告 (データがない、読めないファイルなど) は、最初の 1 つを同期の行に出す。
async fn run_reader_once(
    state: &AppState,
    drive: &dyn DriveStore,
    now: DateTime<Utc>,
    stale: &dyn Fn() -> bool,
) -> AppResult<()> {
    let device_id = match state.with_store(|s| Ok(s.ensure_sync_ids()?.0)) {
        Ok(d) => d,
        Err(e) => {
            record_error(state, &e.message);
            return Err(e);
        }
    };
    let input = ReaderInput {
        device_id,
        now,
        tz: state.tz,
    };
    match run_reader_sync(&state.store, drive, &input).await {
        Ok(report) => {
            let stamp = now.to_rfc3339();
            let recorded = state.with_store(|s| {
                if stale() || s.get_setting(SETTING_RESET_PENDING)?.as_deref() == Some("true") {
                    return Ok(false);
                }
                s.set_setting(SETTING_LAST_SYNC_AT, &stamp)?;
                Ok(true)
            });
            if recorded.unwrap_or(false) {
                update(state, |s| {
                    s.last_sync_at = Some(now);
                    s.error = report.warnings.first().cloned();
                });
            }
            if report.imported_files > 0 || report.applied_ops > 0 {
                state.notify(EVENT_ITEMS);
                state.notify(EVENT_DIGESTS);
            }
        }
        Err(err) => {
            state.notify(EVENT_ITEMS);
            state.notify(EVENT_DIGESTS);
            record_failure(state, err, stale);
        }
    }
    Ok(())
}

/// 読むだけのモードを切り替える。変えたら `true`。同期の実行中は断る。切り替えの間は実行中の印を
/// 持ち、古い役割や端末 id のままの同期が始まらないようにする。入れると新しい端末 id を作り、
/// 送っていない印を捨てる。切ると同期もオフにする (サーバーと 2 台で書かないよう、オンは手で戻す)。
pub fn set_reader_mode(state: &AppState, on: bool) -> AppResult<bool> {
    if state
        .sync_running
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return Err(SYNC_RUNNING_MESSAGE.to_string().into());
    }
    let result = state.with_store(|s| {
        if is_reader(s)? == on {
            return Ok(false);
        }
        if on {
            s.switch_to_reader()?;
        } else {
            s.switch_to_writer()?;
        }
        Ok(true)
    });
    state.sync_running.store(false, Ordering::SeqCst);
    let changed = result?;
    if changed {
        update(state, |s| {
            s.reader_mode = on;
            if !on {
                s.enabled = false;
                s.error = None;
            }
        });
        // サイドバーや画面の表示 (AppStatus.reader_mode) も変わる。
        state.notify(EVENT_STATUS);
    }
    Ok(changed)
}
```

`app/src-tauri/src/commands/sync.rs`, after `sync_set_enabled`:

```rust
/// 「読むだけ (サーバーが収集)」。入れたら同期を 1 回投げる。切ると同期もオフになるので、
/// NAS なら Tailscale も止める (別スレッドで)。
#[tauri::command]
pub fn sync_set_reader_mode(app: AppHandle, on: bool) -> AppResult<SyncStatus> {
    let state = app.state::<AppState>();
    if sync::set_reader_mode(&state, on)? {
        if on {
            sync::start_tailnet_if_needed(&state);
            sync::spawn_sync(&app);
        } else if sync::backend_is_nas(&state) {
            drop(sync::stop_tailnet(&state));
        }
    }
    Ok(sync::status_now(&state))
}
```

Register `commands::sync::sync_set_reader_mode,` in `app/src-tauri/src/lib.rs` after `commands::sync::sync_set_enabled,`.

- [ ] **Step 4: Run tests to verify they pass**

Run: `(cd app/src-tauri && cargo test)`
Expected: PASS (the 4 new tests and every existing app test, including the writer sync tests).

- [ ] **Step 5: Full checks and commit**

Run the Global Constraints checks.

```bash
git add app/src-tauri/src/state.rs app/src-tauri/src/commands/status.rs app/src-tauri/src/sync/mod.rs app/src-tauri/src/commands/sync.rs app/src-tauri/src/lib.rs
git commit -m "feat(app): reader sync in reader mode and the sync_set_reader_mode command"
```

---

### Task 6: App: refuse writer work, reader timer, tray

**Files:**
- Modify: `app/src-tauri/src/jobs.rs` (`ensure_writer`, guards in `run_collect`, `run_digests`, `retry_from`)
- Modify: `app/src-tauri/src/commands/digests.rs` (`collect_now`, `regenerate_digest`, `retry_pending_digests`)
- Modify: `app/src-tauri/src/commands/settings.rs` (`set_setting_value` refuses `sync.mode`)
- Modify: `app/src-tauri/src/scheduler.rs` (`reader_sync_due`, reader branch in `run_loop`)
- Modify: `app/src-tauri/src/tray.rs` (`action_label`, `TrayMenu.action_line`, menu handler)

**Interfaces:**
- Consumes: `infodesk_core::sync::mode::{is_reader, READER_ONLY_MESSAGE}`, `crate::sync::{spawn_sync, SETTING_LAST_SYNC_AT}`.
- Produces:
  - `pub fn jobs::ensure_writer(state: &AppState) -> AppResult<()>` (Err message `読むだけのモードでは使えません`)
  - `pub const scheduler::READER_SYNC_MINUTES: i64 = 10`, `pub fn scheduler::reader_sync_due(last_sync: Option<DateTime<Utc>>, last_request: Option<DateTime<Utc>>, now: DateTime<Utc>) -> bool`
  - `pub fn tray::action_label(reader: bool) -> &'static str`

- [ ] **Step 1: Write the failing tests**

`app/src-tauri/src/jobs.rs` tests module:

```rust
    #[tokio::test]
    async fn reader_mode_refuses_collect_digests_and_retries() {
        let (state, _) = crate::state::test_state();
        state.with_store(|s| s.switch_to_reader()).unwrap();
        let err = run_collect(&state).await.unwrap_err();
        assert_eq!(err.message, READER_ONLY_MESSAGE);
        let st = state.status();
        assert!(!st.collecting);
        assert_eq!(st.last_error, None);
        let day = chrono::NaiveDate::from_ymd_opt(2026, 10, 6).unwrap();
        let err = run_digests(&state, Period::Day, day, &all_category_scopes(), true)
            .await
            .unwrap_err();
        assert_eq!(err.message, READER_ONLY_MESSAGE);
        assert!(!state.status().digesting);
        assert_eq!(retry_all(&state).await.unwrap_err().message, READER_ONLY_MESSAGE);
        assert_eq!(retry_pending(&state).await.unwrap_err().message, READER_ONLY_MESSAGE);
        state.with_store(|s| s.switch_to_writer()).unwrap();
        assert!(ensure_writer(&state).is_ok());
    }
```

`app/src-tauri/src/commands/settings.rs` tests module:

```rust
    #[test]
    fn reader_mode_is_not_a_plain_setting() {
        let (state, _) = test_state();
        let err = set_setting_value(&state, "sync.mode", "reader").unwrap_err();
        assert!(err.message.contains("sync_set_reader_mode"), "{}", err.message);
        assert_eq!(settings_map(&state).unwrap()["sync.mode"], "writer");
    }
```

`app/src-tauri/src/scheduler.rs` tests module:

```rust
    #[test]
    fn reader_sync_waits_ten_minutes_after_a_sync_or_a_request() {
        // Review Focus 5
        let t = |s: &str| s.parse::<DateTime<Utc>>().unwrap();
        let now = t("2026-11-01T10:00:00Z");
        assert!(reader_sync_due(None, None, now), "first tick after start");
        assert!(!reader_sync_due(Some(t("2026-11-01T09:51:00Z")), None, now));
        assert!(reader_sync_due(Some(t("2026-11-01T09:50:00Z")), None, now));
        // NAS down: the last success is old, but a request went out a minute ago.
        assert!(!reader_sync_due(Some(t("2026-11-01T08:00:00Z")), Some(t("2026-11-01T09:59:00Z")), now));
        assert!(reader_sync_due(Some(t("2026-11-01T08:00:00Z")), Some(t("2026-11-01T09:50:00Z")), now));
    }

    #[tokio::test]
    async fn a_job_in_reader_mode_makes_no_digest_and_records_no_run() {
        use infodesk_core::model::Period;
        let state = state_with_daily_in_previous_week(Ok("週次"));
        state.with_store(|s| s.switch_to_reader()).unwrap();
        run_job(&state, &Job::Weekly(d("2026-09-28")), d("2026-10-06")).await;
        assert_eq!(
            state.with_store(|s| s.get_setting("schedule.last_weekly_for")).unwrap(),
            None
        );
        assert!(
            state
                .with_store(|s| s.list_digests(Period::Week, d("2026-09-28")))
                .unwrap()
                .is_empty()
        );
    }
```

`app/src-tauri/src/tray.rs` tests module:

```rust
    #[test]
    fn the_action_item_syncs_in_reader_mode() {
        assert_eq!(action_label(false), "今すぐ収集");
        assert_eq!(action_label(true), "今すぐ同期");
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `(cd app/src-tauri && cargo test reader)` then `(cd app/src-tauri && cargo test action_item)`
Expected: FAIL to compile with "cannot find function `ensure_writer`", "`reader_sync_due`", "`action_label`".

- [ ] **Step 3: Write the implementation**

`app/src-tauri/src/jobs.rs`: add `use infodesk_core::sync::mode::{READER_ONLY_MESSAGE, is_reader};` and

```rust
/// 読むだけのモードなら断る。収集、要約、再試行の入り口で呼ぶ (画面とスケジューラーでも止めるが、念のため)。
pub fn ensure_writer(state: &AppState) -> AppResult<()> {
    if state.with_store(is_reader)? {
        return Err(READER_ONLY_MESSAGE.to_string().into());
    }
    Ok(())
}
```

Call `ensure_writer(state)?;` as the first statement of `run_collect` (before the `status` lock block), of `run_digests` (before `let tz = state.tz;`) and of `retry_from` (before `let pending = ...`).

`app/src-tauri/src/commands/digests.rs`: as the first statement of `regenerate_digest`, `collect_now` and `retry_pending_digests` add:

```rust
    jobs::ensure_writer(&app.state::<AppState>())?;
```

`app/src-tauri/src/commands/settings.rs`, in `set_setting_value` after the `app.launch_at_login` check:

```rust
    if key == "sync.mode" {
        return Err("'sync.mode' cannot be set directly; use sync_set_reader_mode"
            .to_string()
            .into());
    }
```

`app/src-tauri/src/scheduler.rs`: add `use infodesk_core::sync::mode::is_reader;` and `use crate::sync::SETTING_LAST_SYNC_AT;`, then:

```rust
/// 読むだけのモードで同期を頼む間隔 (分)。サーバーは最短 5 分ごとに書く。
pub const READER_SYNC_MINUTES: i64 = 10;

/// 読むだけのモードで同期を頼むか。最後の成功と最後の依頼の遅いほうから 10 分たったら頼む
/// (NAS が落ちていても、15 秒ごとには頼まない)。どちらもなければ (起動直後) すぐ頼む。
pub fn reader_sync_due(
    last_sync: Option<DateTime<Utc>>,
    last_request: Option<DateTime<Utc>>,
    now: DateTime<Utc>,
) -> bool {
    last_sync
        .max(last_request)
        .is_none_or(|t| now - t >= chrono::Duration::minutes(READER_SYNC_MINUTES))
}
```

In `run_loop`, declare `let mut last_reader_request: Option<DateTime<Utc>> = None;` next to `last_retry_at`, and right after `let state = app.state::<AppState>();` insert:

```rust
        if state.with_store(is_reader).unwrap_or(false) {
            // 読むだけのモード: 収集、要約、再試行はしない。10 分ごとに同期だけ頼む。
            state.update_status(|s| s.next_collect_at = None);
            let now_utc = Utc::now();
            let last_sync = state
                .with_store(|s| s.get_setting(SETTING_LAST_SYNC_AT))
                .ok()
                .flatten()
                .and_then(|v| DateTime::parse_from_rfc3339(&v).ok())
                .map(|d| d.with_timezone(&Utc));
            if reader_sync_due(last_sync, last_reader_request, now_utc) {
                last_reader_request = Some(now_utc);
                state.request_sync();
            }
            continue;
        }
```

Update the doc comment of `run_loop` with one line: `/// 読むだけのモードでは、ジョブを評価せず、`reader_sync_due` のときに同期だけ頼む。`

`app/src-tauri/src/tray.rs`: add `use infodesk_core::sync::mode::is_reader;`; add `pub action_line: MenuItem<Wry>,` to `TrayMenu`;

```rust
/// トレイの操作の項目の文。読むだけのモードでは同期にする。
pub fn action_label(reader: bool) -> &'static str {
    if reader { "今すぐ同期" } else { "今すぐ収集" }
}

fn reader_mode(state: &AppState) -> bool {
    state.with_store(is_reader).unwrap_or(false)
}
```

In `refresh`, after setting `unread_line`: `let _ = menu.action_line.set_text(action_label(reader_mode(&state)));`. In `install`, keep the item id `collect` and pass `action_line: collect.clone(),` into `app.manage(TrayMenu { .. })` (`MenuItem` is `Clone`). Replace the `"collect"` arm of `on_menu_event` with:

```rust
            "collect" => {
                if reader_mode(&app.state::<AppState>()) {
                    crate::sync::spawn_sync(app);
                } else {
                    let _ = crate::commands::digests::collect_now(app.clone());
                }
            }
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `(cd app/src-tauri && cargo test)`
Expected: PASS.

- [ ] **Step 5: Full checks and commit**

Run the Global Constraints checks.

```bash
git add app/src-tauri/src/jobs.rs app/src-tauri/src/commands/digests.rs app/src-tauri/src/commands/settings.rs app/src-tauri/src/scheduler.rs app/src-tauri/src/tray.rs
git commit -m "feat(app): refuse writer jobs in reader mode and sync every 10 minutes"
```

---

### Task 7: Frontend: reader toggle, hidden writer buttons, locked tabs

**Files:**
- Modify: `app/src/types.ts` (`AppStatus.reader_mode`, `SyncStatus.reader_mode`)
- Modify: `app/src/api.ts` (`syncSetReaderMode`)
- Modify: `app/src/views/settings/SyncTab.tsx`
- Modify: `app/src/App.tsx`, `app/src/components/Sidebar.tsx`
- Modify: `app/src/views/FeedView.tsx`, `app/src/views/DigestView.tsx`, `app/src/views/SettingsView.tsx`, `app/src/styles/layout.css`
- Modify (fixtures, add `reader_mode: false`): `app/src/App.test.tsx`, `app/src/components/Sidebar.test.tsx`, `app/src/lib/digest.test.ts`, `app/src/views/SettingsView.test.tsx` (both the `status` and the inline `sync_status` object), `app/src/views/DigestView.test.tsx`, `app/src/views/FeedView.test.tsx`, `app/src/views/settings/ScheduleTab.test.tsx`, `app/src/views/settings/SyncTab.test.tsx` (`out`)
- Test: the same test files

**Interfaces:**
- Consumes: Task 5's `get_status` → `AppStatus.reader_mode`, `sync_status` → `SyncStatus.reader_mode`, command `sync_set_reader_mode({ on })`; existing `sync_now`.
- Produces: `api.syncSetReaderMode(on: boolean): Promise<SyncStatus>`.

- [ ] **Step 1: Write the failing tests**

`app/src/views/settings/SyncTab.test.tsx`, inside `describe("SyncTab")`:

```tsx
  it("turns reader mode on and off and explains that sync went off", async () => {
    let current: SyncStatus = signedIn;
    const ipc = setupIpc({
      sync_status: () => current,
      sync_set_reader_mode: (a) => { current = { ...current, reader_mode: a.on, enabled: a.on ? current.enabled : false }; return current; },
    });
    render(<SyncTab />);
    const toggle = await screen.findByRole("switch", { name: "読むだけ (サーバーが収集)" });
    expect(toggle).toHaveAttribute("aria-checked", "false");
    await userEvent.click(toggle);
    expect(ipc.last("sync_set_reader_mode")).toEqual({ on: true });
    await waitFor(() => expect(toggle).toHaveAttribute("aria-checked", "true"));
    expect(screen.queryByText(/同期をオフにしました/)).not.toBeInTheDocument();
    await waitFor(() => expect(toggle).toBeEnabled());
    await userEvent.click(toggle);
    expect(ipc.last("sync_set_reader_mode")).toEqual({ on: false });
    expect(await screen.findByText("同期をオフにしました。この Mac で収集して書き出すなら、「同期する」をもう一度オンにしてください。")).toBeInTheDocument();
    expect(screen.getByRole("switch", { name: "同期する" })).toHaveAttribute("aria-checked", "false");
  });

  it("shows a refused switch", async () => {
    setupIpc({
      sync_status: () => signedIn,
      sync_set_reader_mode: () => { throw { message: "同期中は切り替えられません。終わってからもう一度試してください" }; },
    });
    render(<SyncTab />);
    await userEvent.click(await screen.findByRole("switch", { name: "読むだけ (サーバーが収集)" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("同期中は切り替えられません");
  });
```

`app/src/views/SettingsView.test.tsx`, inside `describe("SettingsView")`:

```tsx
  it("locks the sources tab in reader mode with a notice", async () => {
    setupIpc({ list_sources: () => [], source_item_counts: () => ({}) });
    render(<SettingsView status={{ ...status, reader_mode: true }} />);
    expect(await screen.findByRole("button", { name: "RSSを追加" })).toBeDisabled();
    expect(screen.getByText("読むだけのモードです。ソース、トピック、スケジュールはサーバーの設定が使われます。サーバーの設定は CLI で変えます。")).toBeInTheDocument();
  });

  it("keeps the LLM tab editable in reader mode for the chat", async () => {
    setupIpc({ get_settings: () => settings, ollama_models: () => [] });
    render(<SettingsView status={{ ...status, reader_mode: true }} initialTab="llm" />);
    expect(await screen.findByLabelText("Node のパス")).toBeEnabled();
    expect(screen.getByText("読むだけのモードです。記事の要約とダイジェストはサーバーが作ります。ここの設定は AI チャットにだけ使います。")).toBeInTheDocument();
  });
```

`app/src/views/DigestView.test.tsx`, inside `describe("DigestView")`:

```tsx
  it("hides every generate and retry button in reader mode", async () => {
    setupIpc({ get_digests: () => fresh(views), get_item: item });
    renderAll({ ...status, reader_mode: true });
    expect(await screen.findByText("一つ目。")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "再生成" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "今すぐ再試行" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "再試行" })).not.toBeInTheDocument();
  });
```

`app/src/views/FeedView.test.tsx`, next to the first-launch empty state test:

```tsx
  it("has no collect button in the empty state in reader mode", async () => {
    setupIpc({ list_items: () => [], count_items: () => 0, list_sources: () => [] });
    render(<FeedView selection={{ kind: "all" }} topics={[]} status={{ ...status, last_collect_at: null, reader_mode: true }}
      view="feed" onSwitchView={() => {}} onOpenSources={() => {}} {...CHAT} />);
    expect(await screen.findByText("まだ記事がありません")).toBeInTheDocument();
    expect(screen.getByText("読むだけのモードです。記事はサーバーが集め、同期で届きます。")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "今すぐ収集" })).not.toBeInTheDocument();
  });
```

`app/src/App.test.tsx`, inside `describe("keyboard shortcuts")`:

```tsx
    it("Cmd+R and the sidebar button sync instead of collecting in reader mode", async () => {
      const ipc = setupIpc(handlers({ get_status: () => ({ ...STATUS, reader_mode: true }), sync_now: () => undefined }));
      await ready();
      const button = await screen.findByTitle("今すぐ同期 (⌘R)");
      await press("r", { metaKey: true });
      await waitFor(() => expect(ipc.count("sync_now")).toBe(1));
      await userEvent.click(button);
      await waitFor(() => expect(ipc.count("sync_now")).toBe(2));
      expect(ipc.count("collect_now")).toBe(0);
    });
```

Add `reader_mode: false` to every `AppStatus` fixture listed under **Files**, to the `out` fixture in `SyncTab.test.tsx` and the inline `sync_status` object in `SettingsView.test.tsx`, and to any other `AppStatus` or `SyncStatus` literal that `npx tsc` names in Step 4.

- [ ] **Step 2: Run tests to verify they fail**

Run: `(cd app && npx tsc)`
Expected: FAIL with "Object literal may only specify known properties, and 'reader_mode' does not exist in type 'AppStatus'".

- [ ] **Step 3: Write the implementation**

`app/src/types.ts`, in `AppStatus` after `last_error`:

```ts
  /** 読むだけのモード (サーバーが収集)。get_status が設定から埋める。 */
  reader_mode: boolean;
```

and in `SyncStatus` after `enabled`:

```ts
  /** 読むだけのモード (サーバーが収集し、この Mac は保存先を読む)。 */
  reader_mode: boolean;
```

`app/src/api.ts`, after `syncSetEnabled`:

```ts
  /** 読むだけのモード。切ると同期もオフになる。同期の実行中は断られる。 */
  syncSetReaderMode: (on: boolean) => invoke<SyncStatus>("sync_set_reader_mode", { on }),
```

`app/src/App.tsx`: in `IDLE_STATUS` add `reader_mode: false`; replace `collectNow` with:

```tsx
  // 読むだけのモードでは、収集の代わりに同期する (⌘R とサイドバーのボタン)。
  const collectNow = useCallback(() => {
    (status.reader_mode ? api.syncNow() : api.collectNow())
      .then(() => setActionError(null))
      .catch((e) => setActionError(errorMessage(e)));
  }, [status.reader_mode]);
```

`app/src/components/Sidebar.tsx`, replace the status button:

```tsx
      <button className="sidebar-status" onClick={onCollectNow} disabled={status.collecting}
        title={status.reader_mode ? "今すぐ同期 (⌘R)" : "今すぐ収集 (⌘R)"}>
        <RefreshCw size={14} className={status.collecting ? "spin" : undefined} />
        <span>
          {status.reader_mode ? "読むだけ · サーバーが収集"
            : status.collecting ? "収集中…" : status.last_collect_at ? `最終収集 ${formatClock(status.last_collect_at)}` : "未収集"}
        </span>
      </button>
```

`app/src/views/FeedView.tsx`, in the first-launch empty state replace the `<p className="text-secondary">…</p>`, the button row and the trailing `<p className="tiny text-tertiary">…</p>` with:

```tsx
            {status.reader_mode ? (
              <p className="text-secondary">読むだけのモードです。記事はサーバーが集め、同期で届きます。</p>
            ) : (
              <>
                <p className="text-secondary">収集を始めると、GitHub、arXiv、Hugging Face、Hacker News、国内外のtechニュース、医療介護ニュースが集まり、朝7時に日次要約が生成されます。</p>
                <div style={{ display: "flex", gap: 8 }}>
                  <button className="btn btn-primary" disabled={status.collecting} onClick={() => run(api.collectNow())}>今すぐ収集</button>
                  <button className="btn" onClick={onOpenSources}>ソースを確認</button>
                </div>
                <p className="tiny text-tertiary">収集には1〜2分かかります。進行状況はサイドバー下部に表示されます。</p>
              </>
            )}
```

`app/src/views/DigestView.tsx`: add `const canGenerate = !status.reader_mode;` next to `const heading = ...`; wrap the header `再生成` button as `{canGenerate && (<button …>…再生成</button>)}`; and render the three per-scope buttons (`今すぐ再試行`, `再試行`, `生成`) only when `canGenerate`, e.g. `{canGenerate && <button className="btn" onClick={() => request([v.scope])}>今すぐ再試行</button>}`.

`app/src/styles/layout.css`, after the `.settings` rule:

```css
/* 読むだけのモードで、タブの中身をまとめて無効にする fieldset。見た目は .settings と同じ並び。 */
.settings-lock { display: flex; flex-direction: column; gap: 32px; border: 0; padding: 0; margin: 0; min-width: 0; }
```

`app/src/views/SettingsView.tsx`:

```tsx
/** 読むだけのモードで、編集を止めるタブ。LLM はチャットに使うので止めない。 */
const LOCKED: ReadonlySet<SettingsTab> = new Set<SettingsTab>(["sources", "topics", "schedule"]);
const READER_LOCKED_NOTE = "読むだけのモードです。ソース、トピック、スケジュールはサーバーの設定が使われます。サーバーの設定は CLI で変えます。";
const READER_LLM_NOTE = "読むだけのモードです。記事の要約とダイジェストはサーバーが作ります。ここの設定は AI チャットにだけ使います。";
```

and render the body as:

```tsx
      <div className="view settings">
        {status.reader_mode && LOCKED.has(tab) && <p className="desc" role="note">{READER_LOCKED_NOTE}</p>}
        {status.reader_mode && tab === "llm" && <p className="desc" role="note">{READER_LLM_NOTE}</p>}
        <fieldset className="settings-lock" disabled={status.reader_mode && LOCKED.has(tab)}>
          {tab === "sources" && <SourcesTab />}
          {tab === "topics" && <TopicsTab />}
          {tab === "llm" && <LlmTab />}
          {tab === "schedule" && <ScheduleTab status={status} />}
          {tab === "sync" && <SyncTab />}
        </fieldset>
      </div>
```

`app/src/views/settings/SyncTab.tsx`: add `const [writerNotice, setWriterNotice] = useState(false);` with the other state; change the hint of 同期する to `{status.reader_mode ? "起動時、10 分ごと、⌘R のときに保存先を読みます" : "収集のあと、ダイジェストを作ったあと、⌘R のときに同期します"}`; and add right after the 同期する row:

```tsx
            <div className="setting-row">
              <div className="label"><span>読むだけ (サーバーが収集)</span><span className="hint">収集と要約はサーバーに任せ、この Mac は保存先を読みます。既読とスターは送ります</span></div>
              <Toggle checked={status.reader_mode} ariaLabel="読むだけ (サーバーが収集)" disabled={status.syncing || busy.has("reader")}
                onChange={(v) => { void run("reader", () => api.syncSetReaderMode(v)).then((r) => setWriterNotice(r.ok && !v)); }} />
            </div>
            {writerNotice && <p className="desc">同期をオフにしました。この Mac で収集して書き出すなら、「同期する」をもう一度オンにしてください。</p>}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `(cd app && npx tsc && npx vitest run)`
Expected: PASS (no type errors; the 7 new tests and every existing test).

- [ ] **Step 5: Full checks and commit**

Run the Global Constraints checks.

```bash
git add app/src
git commit -m "feat(app): reader mode toggle, hidden writer buttons and locked tabs"
```

---

### Task 8: Docs

**Files:**
- Modify: `docs/15-home-server.md`, `docs/15-home-server.ja.md` (section 1 bullet, section 2 replaced, end of section 3)
- Modify: `docs/12-cloud-sync.md`, `docs/12-cloud-sync.ja.md` (section 1)
- Modify: `docs/07-settings.md`, `docs/07-settings.ja.md` (Sync tab row and the sync keys paragraph)

**Interfaces:**
- Consumes: the UI strings of Task 7 and the behaviour of Tasks 1-6, quoted exactly.
- Produces: nothing for code.

- [ ] **Step 1: Check the current text**

Run: `grep -n "Why the Mac must stop\|Mac を止める理由\|reads the NAS as before\|これまでどおり NAS を読みます\|one writer\|書き手は1台\|Only \`sync.enabled\` is a plain\|普通の設定は" docs/15-home-server.md docs/15-home-server.ja.md docs/12-cloud-sync.md docs/12-cloud-sync.ja.md docs/07-settings.md docs/07-settings.ja.md`
Expected: one match per file for each phrase being replaced below.

- [ ] **Step 2: `docs/15-home-server.md`**

In section 1, replace `The iPhone app reads the NAS as before.` with `The iPhone app reads the NAS as before, and so does the Mac app in reader mode (section 2).`

Replace the whole section `## 2. Why the Mac must stop` (heading and paragraph) with:

```markdown
## 2. Reading on the Mac (reader mode)

The cluster is the only writer of the NAS folder. The Mac app must never write it as well: the database you copy to the cluster carries the Mac's sync device id, and two writers overwrite each other's files. The Mac app reads the folder in reader mode instead, like the iPhone app.

To switch the Mac to reader mode, after the cluster's first successful sync (section 3):

1. Open InfoDesk, then Settings, 同期. Check that 保存先 is NAS and the NAS address and user are set.
2. Turn on 読むだけ (サーバーが収集). The Mac gets a new sync device id and drops its unsent marks (the cluster already has them in the copied database).
3. Turn on 同期する if it is off. ログイン時に起動 can be on again.

In reader mode the Mac:

- syncs when it starts, every 10 minutes, on 今すぐ同期, and on ⌘R (⌘R and the sidebar button sync instead of collecting);
- never collects, summarizes or makes digests; the collect and regenerate buttons are hidden, and the menu bar item becomes 今すぐ同期;
- writes only its own change batches (`ops-<device id>-…json`) with the read and starred marks made on the Mac; the cluster folds them in at its next sync and the iPhone sees them;
- keeps articles older than the 60 days on the NAS: it never deletes an article because its day left the folder.

The Sources, Topics and Schedule tabs are locked in reader mode; change the cluster's settings with the CLI (section 6). The LLM tab stays editable because AI chat still runs on the Mac.

Turning 読むだけ off also turns 同期する off. Do not turn 同期する on again while the cluster runs: the Mac would write the folder as a second writer.
```

At the end of section 3 (after its last step), add:

```markdown
When the first `infodesk-tick` run has synced to the NAS, switch the Mac to reader mode (section 2).
```

- [ ] **Step 3: `docs/15-home-server.ja.md`**

In section 1, replace `iPhone のアプリは、これまでどおり NAS を読みます。` with `iPhone のアプリは、これまでどおり NAS を読みます。読むだけのモードの Mac のアプリも同じです (2 章)。`

Replace the whole section `## 2. Mac を止める理由` with:

```markdown
## 2. Mac で読む (読むだけのモード)

NAS のフォルダーを書くのはクラスターだけです。Mac のアプリが同時に書いてはいけません。クラスターへ写す DB には Mac の同期の端末 id が入っていて、書き手が 2 台あると、互いのファイルを上書きしてしまいます。Mac のアプリは、iPhone のアプリと同じように、読むだけのモードでフォルダーを読みます。

クラスターの最初の同期が終わったら (3 章)、次の手順で Mac を読むだけのモードにします。

1. InfoDesk を開き、設定の「同期」を開きます。保存先が NAS で、NAS のアドレスとユーザー名が入っていることを確かめます。
2. 「読むだけ (サーバーが収集)」をオンにします。Mac は新しい同期の端末 id を作り、送っていない印を捨てます (写した DB に入っているので、クラスターはもう持っています)。
3. 「同期する」がオフならオンにします。「ログイン時に起動」もオンに戻せます。

読むだけのモードの Mac は、次のように動きます。

- 起動したとき、10 分ごと、「今すぐ同期」と ⌘R のときに同期します (⌘R とサイドバーのボタンは、収集の代わりに同期します)。
- 収集、要約、ダイジェストの生成はしません。収集と再生成のボタンは出ず、メニューバーの項目は「今すぐ同期」になります。
- 書くのは、Mac で付けた既読とスターの変更の束 (`ops-<端末ID>-…json`) だけです。クラスターが次の同期で取り込み、iPhone にも届きます。
- NAS にある 60 日より古い記事も残します。日のファイルがフォルダーから消えても、記事は消しません。

読むだけのモードでは、ソース、トピック、スケジュールのタブは編集できません。クラスターの設定は CLI で変えます (6 章)。AI チャットは Mac で動くので、LLM のタブは編集できます。

「読むだけ」をオフにすると、「同期する」もオフになります。クラスターが動いている間は、「同期する」をオンに戻さないでください。Mac が 2 台目の書き手としてフォルダーを書いてしまいます。
```

At the end of section 3, add:

```markdown
最初の `infodesk-tick` の実行が NAS へ同期したら、Mac を読むだけのモードにします (2 章)。
```

- [ ] **Step 4: `docs/12-cloud-sync.md` and `.ja.md`**

In section 1 of `docs/12-cloud-sync.md`, after the bullet `- Each file on Drive has one writer, so devices never overwrite each other.` add:

```markdown
- Only one device writes the articles and digests: the Mac, or the home server when you use one ([Home Server](15-home-server.md)). A Mac in reader mode (読むだけ) reads the folder like the phone and writes only its own change batches.
```

In section 1 of `docs/12-cloud-sync.ja.md`, after `- Drive 上の各ファイルの書き手は1台だけなので、端末どうしが上書きし合うことはありません。` add:

```markdown
- 記事とダイジェストを書くのは 1 台だけです。Mac か、自宅サーバーを使うならそのサーバーです ([自宅サーバー](15-home-server.ja.md))。読むだけのモードの Mac は、スマホと同じようにフォルダーを読み、自分の変更の束だけを書きます。
```

- [ ] **Step 5: `docs/07-settings.md` and `.ja.md`**

In `docs/07-settings.md`, in the Tabs table row `| Sync | … |`, after `turn sync on or off,` insert ` reader mode (読むだけ, see [Home Server](15-home-server.md)),`. In the sync keys paragraph, after `` `sync.last_compaction_at`. `` insert: `` `sync.mode` (`writer`, the default, or `reader`; set by 読むだけ in the Sync tab, which also makes a new `sync.device_id`), `sync.reader.writer` and `sync.reader.written_at` (the writer name and write time of the last manifest a reader read). ``

In `docs/07-settings.ja.md`, in the 同期 row of the tabs table, replace `同期のオンとオフ、` with `同期のオンとオフ、読むだけのモード (「読むだけ」。[自宅サーバー](15-home-server.ja.md)を参照)、`. In the sync keys paragraph, after `` `sync.last_compaction_at` です。 `` insert: `` `sync.mode` (`writer` が既定、または `reader`。同期タブの「読むだけ」が切り替え、そのとき `sync.device_id` も作り直す)、`sync.reader.writer` と `sync.reader.written_at` (読むだけのモードで最後に読んだ manifest の書き手と書いた時刻) もあります。 ``

- [ ] **Step 6: Check and commit**

Run: `grep -n "Mac must stop\|Mac を止める理由\|Until a Mac reader mode exists\|読み取り専用にする機能ができるまで" docs/*.md`
Expected: no output.

```bash
git add docs/15-home-server.md docs/15-home-server.ja.md docs/12-cloud-sync.md docs/12-cloud-sync.ja.md docs/07-settings.md docs/07-settings.ja.md
git commit -m "docs: switch the Mac to reader mode instead of stopping it"
```
