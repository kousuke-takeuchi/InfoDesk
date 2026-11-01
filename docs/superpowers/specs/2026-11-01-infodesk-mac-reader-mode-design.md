# Mac app reader mode (sub-project 2) design

Date: 2026-11-01
Status: written overnight without the user; every decision below was made by Claude and is marked for the user's review (section 3). Nothing here is merged until the user approves the PR.
Parent designs: [2026-11-01-infodesk-headless-server-design.md](2026-11-01-infodesk-headless-server-design.md) (sub-project 1, the k3s writer), [2026-10-13-infodesk-cloud-sync-design.md](2026-10-13-infodesk-cloud-sync-design.md) (sync format, one-writer rule)

## 1. Goal

Since 2026-11-01 the k3s cluster is the only writer: it collects, summarizes, makes digests and writes the sync folder on the NAS. The Mac app must not do any of that any more, but the user still wants to read on the Mac. This design adds a reader mode to the Mac app: it reads the sync folder like the iPhone app does, and sends its read and starred marks back.

### Success criteria

- With reader mode on, the Mac app never collects, never summarizes, never makes digests, and never writes `manifest.json`, `meta.json.gz`, day, digest or snapshot files. Its only remote writes are new ops files named with its own device id.
- After a reader sync, the feed, article view, stars and digests on the Mac show what the cluster wrote, including the articles moved by `redate`.
- A read or star mark made on the Mac reaches the iPhone (through the cluster's next compaction or directly as an ops file), and a mark made on the iPhone reaches the Mac.
- Turning reader mode on from the existing copied database does not need a fresh download: the Mac's database is the cluster's starting point, so existing rows are updated in place.
- Turning reader mode off does not let the Mac overwrite the cluster's folder by accident: sync is switched off at the same time and must be switched on again by hand.

### Out of scope

- Reader mode for Google Drive is included only as far as the drive abstraction gives it for free; the user's setup is NAS. No separate testing on Google Drive.
- Editing sources, topics or LLM settings for the cluster from the Mac. In reader mode those Mac tabs only show the local copy, with a notice that the cluster's settings are changed with the CLI (docs/15-home-server section 6).
- Period statistics from the files. Digest views keep computing statistics from local items, which only cover the 60 days kept in day files (section 4.4).
- Chat. It keeps working on the Mac's local copy, as today.

## 2. What exists

- The writer pass `run_sync` (crates/core/src/sync/engine.rs) builds every file from the local database. With a different `db_id` it deletes every remote file. It must never run in reader mode.
- The iPhone reader (mobile/lib/sync/engine.dart `_pass`): reads the manifest, resets local data if `db_id` changed, imports meta, day, digest and snapshot files whose sha256 changed, applies every ops file, uploads its own pending ops as a new ops file. It never deletes and never writes the manifest.
- The Rust core already has the format types and helpers (format.rs), the `DriveStore` trait with WebDAV, Google and memory implementations, and the store's ops helpers (`pending_local_ops`, `mark_ops_uploaded`, `apply_remote_ops`, `sync_files`). It has no import for day, digest or meta files.
- The Mac app runs collect and digest jobs from `app/src-tauri/src/scheduler.rs`, and sync from `app/src-tauri/src/sync/mod.rs` (`run_once` calls `run_sync`).
- The copied database: the cluster's database is a copy of the Mac's, so the cluster uses the Mac's `sync.device_id` and `sync.db_id`. The Mac must get a new device id before it writes any ops file, else its ops files would look like the cluster's own and be skipped by the cluster (`import_remote_ops` skips its own device).

## 3. Decisions (made by Claude overnight; for the user's review)

| # | Question | Decision | Why |
|---|---|---|---|
| R1 | Where the mode lives | New setting `sync.mode`, `writer` (default) or `reader`, in `DEFAULT_SETTINGS` | One switch read by the scheduler, the sync launcher and the commands. Default keeps today's behaviour for anyone not using a server. |
| R2 | Device identity on switching to reader | Generate a new `sync.device_id` and discard local pending ops | The cluster inherited the Mac's device id. A new id makes the Mac's ops files visible to the cluster and the iPhone. Pending ops from the writer era were already in the copy. |
| R3 | Reader sync engine | New `run_reader_sync` in `crates/core/src/sync/reader.rs`, a port of the iPhone `_pass`, using the same `DriveStore` | Proven algorithm, same file rules, testable in Rust with `MemoryDrive` against the real writer `run_sync`. |
| R4 | Different `db_id` on the server | Wipe local items, digests, item state, `sync_ops` and `sync_files`, keep sources, topics, settings and chat, then adopt the manifest's `db_id` | Same as the iPhone. Not triggered for the user today (same `db_id`), but needed if the cluster is ever rebuilt from scratch. |
| R5 | Importing a day file | Replace the day: upsert every record by id (insert or update the article fields, never `read`/`starred`), delete local articles whose `fetched_at` is on that day and whose id is not in the file, keep FTS tables in step; an existing row with the same URL but another id is deleted first | Mirrors the iPhone's "replace the day" rule in a schema without a date column, and handles articles moved by `redate`. |
| R6 | Importing a digest file | Upsert each entry into `digests` as `done` (scope key from the JSON scope), keep `created_at` and `provider` from the file; the file's `stats` are not stored | The views already read `digests`. Stats keep coming from local items (out of scope above). |
| R7 | Meta file | Not imported. Local sources and topics stay as they are | Meta topics have no keywords, and the Mac's topic filter is keyword-based. The Mac's copies came from the same database, so names and ids already match. |
| R8 | Snapshot and ops | Snapshot state and every ops file whose `db_id` matches are applied with `apply_remote_ops` (stamp rule `wins`) | Same rule as the writer and the iPhone. |
| R9 | When the reader syncs | On start, every 10 minutes, on "今すぐ同期" and Cmd+R, and after a local mark (the existing `request_sync` path) | The cluster writes at most every 5 minutes; 10 minutes keeps NAS traffic small. |
| R10 | Writer jobs in reader mode | The scheduler skips collect, daily, weekly, monthly and pending-digest retries; the commands `collect_now`, `regenerate_digest` and `retry_pending_digests` return an error `読むだけのモードでは使えません`; the UI hides "今すぐ収集", the digest regenerate buttons and the tray's collect item | Defence in depth: the UI hides, the backend refuses, the scheduler skips. |
| R11 | Turning reader mode off | Sets `sync.enabled = false` at the same time, with a notice in the Sync tab | The Mac would otherwise write as a second writer at the next sync. |
| R12 | Where the switch is | Sync tab, section "同期", a toggle "読むだけ (サーバーが収集)" under "同期する", with one line of explanation | Next to the settings it changes. |
| R13 | Retention | A reader never deletes local articles because a day file disappeared from the folder (only R5's per-day replace deletes) | Old days vanish from the folder after 60 days; the Mac keeps what it has, like an archive. |

## 4. Components

### 4.1 Core: setting and mode helper (`crates/core/src/sync/nas.rs` or a new `sync/mode.rs`)

- `SETTING_MODE = "sync.mode"`, values `MODE_WRITER = "writer"`, `MODE_READER = "reader"`, default `writer` in `seed.rs` `DEFAULT_SETTINGS`.
- `pub fn is_reader(store: &Store) -> Result<bool>`.
- `Store::switch_to_reader(&self) -> Result<()>`: in one transaction sets `sync.mode = reader`, writes a new UUID v4 to `sync.device_id`, deletes all rows of `sync_ops`. `Store::switch_to_writer(&self) -> Result<()>`: sets `sync.mode = writer` and `sync.enabled = false`.

### 4.2 Core: imports (`crates/core/src/store/reader.rs`)

- `Store::import_day(&self, day: &DayFile, tz: FixedOffset) -> Result<()>` (R5), one transaction.
- `Store::import_digests(&self, file: &DigestFile) -> Result<()>` (R6), one transaction.
- `Store::reset_for_db_id(&self, db_id: &str) -> Result<()>` (R4), one transaction.

### 4.3 Core: reader pass (`crates/core/src/sync/reader.rs`)

```rust
pub struct ReaderInput { pub device_id: String, pub now: DateTime<Utc>, pub tz: FixedOffset }
pub struct ReaderReport { pub imported_files: usize, pub applied_ops: usize, pub uploaded_ops: usize, pub warnings: Vec<String> }
pub async fn run_reader_sync(store: &Mutex<Store>, drive: &dyn DriveStore, input: &ReaderInput) -> Result<ReaderReport, SyncError>;
```

Steps, the same as the iPhone `_pass`: list; read the manifest (none: warning `同期フォルダにまだデータがありません`, return); schema not 1: warning, return; store `manifest.writer`/`written_at` in settings `sync.reader.writer` / `sync.reader.written_at`; `db_id` differs: `reset_for_db_id`; import day, digest and snapshot files listed in the manifest whose sha256 differs from the `sync_files` `in` row (verify the download's sha256 against the manifest, skip with a warning on mismatch or parse error); apply every ops file not yet imported whose `db_id` is absent or equal to the manifest's, oldest first; upload local pending ops as one new ops file `ops_file_name(device_id, now, 1)` with `db_id` = the manifest's, then mark them uploaded. The store lock is never held across an await.

### 4.4 App

- `sync/mod.rs` `run_once`: if `is_reader`, build the drive as today and call `run_reader_sync`; record `sync.last_sync_at`; emit the items event when anything was imported or applied. Never call `run_sync` in reader mode.
- `scheduler.rs`: if `is_reader`, skip all jobs; request a sync when 10 minutes passed since `sync.last_sync_at` (R9).
- Commands: `collect_now`, `regenerate_digest`, `retry_pending_digests` refuse in reader mode (R10). New commands `sync_set_reader_mode(on: bool)` calling `switch_to_reader` / `switch_to_writer` and then `request_sync` when turned on. `SyncStatus` gains `reader_mode: bool`.
- Frontend: Sync tab toggle (R12); hide collect and regenerate buttons and disable the Sources, Topics and LLM tabs' edit controls with a notice when `reader_mode`; tray collect item hidden.

## 5. Error handling

- No manifest, newer schema, unreadable file: warnings shown in the Sync tab's last-sync line, as today; nothing imported from that file.
- NAS errors: same messages and retry rules as the writer path.
- Refused commands: the error text above, shown by the existing error toast.

## 6. Testing

- Core, round trip: a writer store with items, marks and digests runs `run_sync` into a `MemoryDrive`; a reader store (`switch_to_reader`) runs `run_reader_sync`; the reader's `query_items` and `list_digests` match, with read and starred marks. A mark set on the reader is uploaded and then applied by the writer's next `run_sync`.
- Core: re-dated articles move between days on the reader (writer runs `redate_apply` + `run_sync`, reader syncs again).
- Core: the reader never writes anything but `ops-<its device>-*.json` (check the `MemoryDrive` file names before and after).
- Core: `db_id` change wipes items and digests but keeps sources, topics and settings.
- Core: `switch_to_reader` changes the device id and clears `sync_ops`; `switch_to_writer` turns sync off.
- App: commands refuse in reader mode (unit tests on the guard function); scheduler skips jobs in reader mode (unit test on the decision function).
- Frontend: typecheck and existing tests pass; the toggle and hidden buttons are checked by the user in the morning (not verifiable without the GUI here).

## 7. Rollout

1. PR with this spec, the plan and the code; CI-equivalent local checks green. Not merged without the user.
2. The user installs the build, turns on "読むだけ" in the Sync tab, and checks the feed and digests against the iPhone.
