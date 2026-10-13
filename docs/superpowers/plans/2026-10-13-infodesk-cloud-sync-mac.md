# クラウド同期 (Mac 側) 実装計画 (Plan 7A)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Mac の InfoDesk が、収集した記事とダイジェストを Google Drive の `appDataFolder` に書き出し、モバイルとの間で既読とスターを双方向に同期できるようにする。

**Architecture:** 同期の中身 (ファイルの形、差分の判断、操作の取り込みとぶつかったときの決め方、まとめ、保持期間) は `crates/core/src/sync/` に置き、Drive とのやりとりは trait `DriveStore` の向こうに隠す。テストはメモリ上の偽の Drive で行う。OAuth、キーチェーン、スケジュールとの接続、設定画面は `app/` 側に置く。

**Tech Stack:** Rust 2024 (rusqlite 0.40、reqwest 0.13、tokio、serde、flate2、sha2、uuid、security-framework)、Tauri 2.12、React 19 + vitest。

**Spec:** `docs/superpowers/specs/2026-10-13-infodesk-cloud-sync-design.md` (3、4、5、7、8節)。親の設計書は `docs/superpowers/specs/2026-10-06-infodesk-design.md`。デザインは `design.pen` の A-13 (書き出し `design/exports/13-settings-sync.png`)。

## Global Constraints

- Drive の権限は `https://www.googleapis.com/auth/drive.appdata` だけ。ファイルはすべて `appDataFolder` の直下 (`parents: ["appDataFolder"]`、一覧は `spaces=appDataFolder`)。
- ファイル名: `manifest.json`、`meta.json.gz`、`day-YYYY-MM-DD.json.gz`、`digest-{day,week,month}-YYYY-MM-DD.json.gz`、`state-snapshot.json.gz`、`ops-<端末ID>-<UTC時刻 YYYYMMDDTHHMMSSmmmZ>-<連番4桁>.json`。
- `manifest.json` の `schema` は `1`。
- 操作の `at` は RFC3339、ミリ秒、UTC、末尾 `Z` (例 `2026-10-13T01:02:03.456Z`)。ぶつかったときは `at` が新しい方、同じなら `device` の文字列が大きい方を採る。
- 保持期間: `day-*` と `digest-day-*` は Mac の日付で直近60日、`digest-week-*` と `digest-month-*` は直近400日。
- 端末 ID は設定 `sync.device_id`、DB 識別子は設定 `sync.db_id` (どちらも UUID v4、初回に作る)。端末名は設定 `sync.device_name` (既定はホスト名)。同期のオン・オフは `sync.enabled` (`"true"`/`"false"`、既定 `"false"`)。
- 更新用トークンは macOS のキーチェーン (サービス名 `jp.infodesk.app.google`、アカウント名 `refresh_token`)。DB に置かない。
- OAuth クライアント ID とシークレットは、git 管理外の `app/src-tauri/google-oauth.json` からビルド時に埋め込む。ファイルがなければ同期は無効 (タブに「このビルドでは同期を使えません」)。ひな形 `app/src-tauri/google-oauth.example.json` をコミットする。
- 同期のエラーは収集やダイジェストのエラー (`last_error`) と混ぜない。表示も別の行。
- UI の文言は日本語。設定タブの見た目は A-13 に合わせる。利用者が入力するのは端末名だけ (ID やシークレットの入力欄は作らない)。
- 単体テストはネットワークにつながない (wiremock と偽の Drive を使う)。
- コミットに AI の署名行 (`Co-Authored-By`、`Claude-Session` など) を付けない。

## Review Focus

1. Drive の一覧がページ分けされる (100件を超える) とき、`nextPageToken` をたどって全件を見ること (Task 3 にテスト)。
2. 同期の途中で失敗したとき、`manifest.json` が書かれず、取り込み済みの印も付かず、次回にやり直せること (Task 4 にテスト)。
3. 同じ記事に対して Mac とモバイルで同時に操作したとき、両端末で同じ結果に落ち着くこと (Task 1 と Task 2 にテスト)。
4. まとめ (compaction) の直後にモバイルが古い `ops-*` を読もうとして消えている場合でも、スナップショットに含まれているので結果が変わらないこと (Task 4 にテスト)。
5. 401 で更新用トークンも使えないとき、同期が止まり「再ログインが必要です」が出て、収集は止まらないこと (Task 5、Task 6 にテスト)。

---

## ファイルの構成

```
crates/core/src/sync/mod.rs        公開する型と関数のまとめ
crates/core/src/sync/format.rs     ファイルの型、名前、gzip、ハッシュ、操作の合成 (純粋関数)
crates/core/src/sync/drive.rs      trait DriveStore、MemoryDrive (偽物)、DriveError
crates/core/src/sync/google.rs     GoogleDrive (reqwest で Drive API v3)
crates/core/src/sync/engine.rs     run_sync: 同期の1回分
crates/core/src/store/sync.rs      同期のための DB 操作 (操作の記録、取り込み、書き出し用の読み出し)
crates/core/src/store/migrate.rs   版5
app/src-tauri/src/sync/mod.rs      アプリ側の同期 (状態、実行、コマンド)
app/src-tauri/src/sync/oauth.rs    OAuth (PKCE、ループバック、トークン交換と更新)
app/src-tauri/src/sync/keychain.rs キーチェーン
app/src-tauri/build.rs             google-oauth.json を読み込み環境変数に渡す
app/src/views/settings/SyncTab.tsx 同期タブ (+ test)
docs/12-cloud-sync.md (+ .ja.md)   使い方と、Google 側の準備の手順書
```

---

### Task 1: ファイルの形と操作の合成 (core、純粋関数)

**Files:**
- Create: `crates/core/src/sync/mod.rs`, `crates/core/src/sync/format.rs`
- Modify: `crates/core/src/lib.rs` (`pub mod sync;`)、`crates/core/Cargo.toml` (`flate2 = "1.1.10"`、`sha2 = "0.10.9"`。どちらも Cargo.lock にある版)

**Interfaces:**
- Produces (`infodesk_core::sync::format`):
  - `pub const SCHEMA: u32 = 1;`
  - `#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)] pub struct Manifest { pub schema: u32, pub db_id: String, pub written_at: String, pub writer: String, pub files: BTreeMap<String, FileEntry> }`
  - `pub struct FileEntry { pub sha256: String, pub size: u64, pub updated_at: String }`
  - `pub struct MetaFile { pub categories: Vec<CategoryMeta>, pub topics: Vec<TopicMeta>, pub sources: Vec<SourceMeta> }` (`CategoryMeta{id: String, label: String}`、`TopicMeta{id: i64, name: String}`、`SourceMeta{id: i64, name: String, category: String}`)
  - `pub struct DayFile { pub date: String, pub items: Vec<ItemRecord> }`
  - `pub struct ItemRecord { pub id: i64, pub url: String, pub title: String, pub excerpt: String, pub ai_summary: Option<String>, pub image_url: Option<String>, pub source_id: Option<i64>, pub source_name: String, pub category: String, pub score: Option<i64>, pub published_at: Option<String>, pub fetched_at: String, pub topic_ids: Vec<i64> }`
  - `pub struct DigestFile { pub period: String, pub period_start: String, pub stats: Option<serde_json::Value>, pub entries: Vec<DigestRecord> }` (`DigestRecord{ scope: serde_json::Value, body: String, status: String, item_ids: Vec<i64>, provider: String, created_at: String }`。`stats` は `DigestStats` を `serde_json::to_value` したもの)
  - `#[derive(..., Eq)] pub enum Field { Read, Starred }` (serde は `"read"`/`"starred"`)
  - `pub struct Op { pub item_id: i64, pub field: Field, pub value: bool, pub at: String, pub device: String }`
  - `pub struct OpsFile { pub device: String, pub ops: Vec<Op> }`
  - `pub struct StateSnapshot { pub covers: Vec<String>, pub state: BTreeMap<i64, ItemState> }`、`pub struct ItemState { pub read: Option<Stamp>, pub starred: Option<Stamp> }`、`pub struct Stamp { pub value: bool, pub at: String, pub device: String }`
  - `pub fn day_file_name(d: NaiveDate) -> String` (`day-2026-10-13.json.gz`)
  - `pub fn digest_file_name(p: Period, start: NaiveDate) -> String` (`digest-week-2026-10-05.json.gz`)
  - `pub fn ops_file_name(device: &str, at: DateTime<Utc>, seq: u32) -> String`
  - `pub enum FileKind { Manifest, Meta, Day(NaiveDate), Digest(Period, NaiveDate), Snapshot, Ops { device: String } }` と `pub fn parse_file_name(name: &str) -> Option<FileKind>`
  - `pub fn op_time(at: DateTime<Utc>) -> String` (ミリ秒、末尾 `Z`)
  - `pub fn gzip_json<T: Serialize>(v: &T) -> Result<Vec<u8>>`、`pub fn gunzip_json<T: DeserializeOwned>(b: &[u8]) -> Result<T>`
  - `pub fn sha256_hex(b: &[u8]) -> String`
  - `pub fn wins(a: &Stamp, b: &Stamp) -> bool` (a が b に勝つなら true: `at` が新しい、同じなら `device` が大きい)
  - `pub fn apply_op(state: &mut BTreeMap<i64, ItemState>, op: &Op)` (勝つときだけ上書き)
- gzip は同じ入力から同じバイト列になるようにする (ヘッダーの時刻を 0 にする: `flate2::GzBuilder::new().mtime(0)`)。ハッシュの比較が安定するため。

- [ ] **Step 1: 失敗するテストを書く** (`format.rs` の `#[cfg(test)] mod tests`)

```rust
#[test]
fn file_names_round_trip() {
    let d = NaiveDate::from_ymd_opt(2026, 10, 13).unwrap();
    assert_eq!(day_file_name(d), "day-2026-10-13.json.gz");
    assert_eq!(digest_file_name(Period::Week, NaiveDate::from_ymd_opt(2026, 10, 5).unwrap()), "digest-week-2026-10-05.json.gz");
    let at = DateTime::parse_from_rfc3339("2026-10-13T01:02:03.456Z").unwrap().with_timezone(&Utc);
    let ops = ops_file_name("dev-a", at, 7);
    assert_eq!(ops, "ops-dev-a-20261013T010203456Z-0007.json");
    assert_eq!(parse_file_name(&ops), Some(FileKind::Ops { device: "dev-a".into() }));
    assert_eq!(parse_file_name("day-2026-10-13.json.gz"), Some(FileKind::Day(d)));
    assert_eq!(parse_file_name("manifest.json"), Some(FileKind::Manifest));
    assert_eq!(parse_file_name("state-snapshot.json.gz"), Some(FileKind::Snapshot));
    assert_eq!(parse_file_name("digest-month-2026-10-01.json.gz"), Some(FileKind::Digest(Period::Month, NaiveDate::from_ymd_opt(2026, 10, 1).unwrap())));
    assert_eq!(parse_file_name("random.txt"), None);
}

#[test]
fn device_ids_with_hyphens_parse() {
    // UUID はハイフンを含む。末尾の時刻と連番から切り分ける。
    let at = DateTime::parse_from_rfc3339("2026-10-13T00:00:00Z").unwrap().with_timezone(&Utc);
    let name = ops_file_name("6f1c2a9e-1b2c-4d5e-8f90-123456789abc", at, 1);
    assert_eq!(parse_file_name(&name), Some(FileKind::Ops { device: "6f1c2a9e-1b2c-4d5e-8f90-123456789abc".into() }));
}

#[test]
fn gzip_is_deterministic_and_round_trips() {
    let f = OpsFile { device: "d".into(), ops: vec![] };
    let a = gzip_json(&f).unwrap();
    let b = gzip_json(&f).unwrap();
    assert_eq!(a, b);
    assert_eq!(sha256_hex(&a), sha256_hex(&b));
    let back: OpsFile = gunzip_json(&a).unwrap();
    assert_eq!(back, f);
}

#[test]
fn op_time_has_millis_and_z() {
    let at = DateTime::parse_from_rfc3339("2026-10-13T01:02:03.4567Z").unwrap().with_timezone(&Utc);
    assert_eq!(op_time(at), "2026-10-13T01:02:03.456Z");
}

fn op(item: i64, field: Field, value: bool, at: &str, device: &str) -> Op {
    Op { item_id: item, field, value, at: at.into(), device: device.into() }
}

#[test]
fn newer_op_wins_and_ties_break_on_device() {
    let mut s = BTreeMap::new();
    apply_op(&mut s, &op(1, Field::Read, true, "2026-10-13T00:00:01.000Z", "a"));
    apply_op(&mut s, &op(1, Field::Read, false, "2026-10-13T00:00:00.000Z", "b"));
    assert_eq!(s[&1].read.as_ref().unwrap().value, true);
    apply_op(&mut s, &op(1, Field::Read, false, "2026-10-13T00:00:01.000Z", "b"));
    assert_eq!(s[&1].read.as_ref().unwrap().value, false); // 同時刻は "b" > "a"
    apply_op(&mut s, &op(1, Field::Starred, true, "2026-10-13T00:00:00.000Z", "a"));
    assert_eq!(s[&1].read.as_ref().unwrap().value, false); // 項目は独立
    assert_eq!(s[&1].starred.as_ref().unwrap().value, true);
}

#[test]
fn applying_ops_in_any_order_gives_the_same_state() {
    let ops = vec![
        op(1, Field::Read, true, "2026-10-13T00:00:01.000Z", "mac"),
        op(1, Field::Read, false, "2026-10-13T00:00:02.000Z", "phone"),
        op(1, Field::Starred, true, "2026-10-13T00:00:03.000Z", "mac"),
        op(2, Field::Read, true, "2026-10-13T00:00:01.000Z", "phone"),
    ];
    let mut forward = BTreeMap::new();
    for o in &ops { apply_op(&mut forward, o); }
    let mut backward = BTreeMap::new();
    for o in ops.iter().rev() { apply_op(&mut backward, o); }
    assert_eq!(forward, backward);
}
```

- [ ] **Step 2: 失敗を確かめる**: `cargo test -p infodesk-core sync::format` が「未定義」で失敗する。
- [ ] **Step 3: 実装する**: 上の Interfaces どおり。`parse_file_name` の `ops-` は、末尾から `-NNNN.json` と `-YYYYMMDDTHHMMSSmmmZ` を切り落とした残りを端末 ID とする。`Period` の文字列は既存の serde (`day`/`week`/`month`) に合わせる。
- [ ] **Step 4: 通ることを確かめる**: `cargo test -p infodesk-core sync::format`。
- [ ] **Step 5: コミット**: `git commit -m "feat(core): sync file format, names and last-writer-wins merge"`

---

### Task 2: DB の版5と、同期のための DB 操作 (core)

**Files:**
- Modify: `crates/core/src/store/migrate.rs` (版5)、`crates/core/src/store/items.rs` (`set_read`、`set_starred`、`mark_all_read` が操作を記録する)、`crates/core/src/store/mod.rs` (`mod sync;`)
- Create: `crates/core/src/store/sync.rs`
- Modify: `crates/core/src/seed.rs` (`sync.enabled` = `"false"`)

**版5の SQL:**

```sql
ALTER TABLE items ADD COLUMN read_changed_at TEXT;
ALTER TABLE items ADD COLUMN starred_changed_at TEXT;
CREATE TABLE IF NOT EXISTS sync_ops (
    id INTEGER PRIMARY KEY,
    item_id INTEGER NOT NULL,
    field TEXT NOT NULL,
    value INTEGER NOT NULL,
    at TEXT NOT NULL,
    uploaded INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS sync_files (
    name TEXT PRIMARY KEY,
    kind TEXT NOT NULL,          -- 'out' (書き出した) か 'in' (取り込んだ ops)
    drive_id TEXT,
    sha256 TEXT,
    updated_at TEXT NOT NULL
);
```

`SCHEMA_VERSION` を 5 にする。

**Interfaces (Store のメソッド、`store/sync.rs`):**
- `set_read(id, read)` と `set_starred(id, starred)` は、値を変えたとき `*_changed_at` を今の時刻 (`op_time`) にし、`sync_ops` に1行足す。値が変わらないときは何もしない。`mark_all_read` は既読にした記事ごとに同じことをする。
- `pub fn ensure_sync_ids(&self) -> Result<(String, String)>`。`sync.device_id` と `sync.db_id` がなければ UUID v4 を作って保存し、`(device_id, db_id)` を返す (`uuid = { version = "1.27.0", features = ["v4"] }` を core に足す。Cargo.lock にある版)。
- `pub fn pending_local_ops(&self, device: &str) -> Result<Vec<(i64, Op)>>`。`uploaded = 0` の行を古い順に。`Op.device` には引数を入れる。
- `pub fn mark_ops_uploaded(&self, ids: &[i64]) -> Result<()>`
- `pub fn apply_remote_op(&self, op: &Op, local_device: &str) -> Result<bool>`。対象の記事がなければ false。現在値と `*_changed_at` (NULL は最も古いとみなす。その場合の比較用の端末は `local_device`) を `Stamp` にして `format::wins(op, current)` なら値と `*_changed_at` を書き換える。`sync_ops` には記録しない。変えたら true。
- `pub fn local_state_stamps(&self, item_ids: Option<&[i64]>) -> Result<BTreeMap<i64, ItemState>>`。まとめ用。`*_changed_at` が NULL の項目は含めない。
- `pub fn items_fetched_on(&self, date: NaiveDate, tz: FixedOffset) -> Result<Vec<ItemRecord>>`。その日 (tz の日付) に取得した記事。`topic_ids` は全トピックについて既存のキーワード照合 (`ItemFilter` の topic 絞り込みと同じ照合) で求める。`fetched_at` 昇順。
- `pub fn days_with_items(&self, since: NaiveDate, tz: FixedOffset) -> Result<Vec<NaiveDate>>`
- `pub fn sync_file(&self, name: &str) -> Result<Option<SyncFileRow>>`、`pub fn put_sync_file(&self, row: &SyncFileRow) -> Result<()>`、`pub fn delete_sync_file(&self, name: &str) -> Result<()>`、`pub fn sync_files(&self, kind: &str) -> Result<Vec<SyncFileRow>>`。`pub struct SyncFileRow { pub name: String, pub kind: String, pub drive_id: Option<String>, pub sha256: Option<String>, pub updated_at: String }`

- [ ] **Step 1: 失敗するテストを書く** (`store/sync.rs` の tests。既存のテスト補助 `testutil` で記事を作る)

```rust
#[test]
fn local_changes_are_journaled_once() {
    let s = Store::open_in_memory().unwrap();
    let id = insert_item(&s, "https://a/1");
    s.set_read(id, true).unwrap();
    s.set_read(id, true).unwrap(); // 変化なし
    s.set_starred(id, true).unwrap();
    let ops = s.pending_local_ops("mac").unwrap();
    assert_eq!(ops.len(), 2);
    assert_eq!(ops[0].1.field, Field::Read);
    assert_eq!(ops[1].1.field, Field::Starred);
    s.mark_ops_uploaded(&ops.iter().map(|(i, _)| *i).collect::<Vec<_>>()).unwrap();
    assert!(s.pending_local_ops("mac").unwrap().is_empty());
}

#[test]
fn mark_all_read_journals_each_item() {
    let s = Store::open_in_memory().unwrap();
    insert_item(&s, "https://a/1");
    insert_item(&s, "https://a/2");
    assert_eq!(s.mark_all_read(&ItemFilter::default()).unwrap(), 2);
    assert_eq!(s.pending_local_ops("mac").unwrap().len(), 2);
}

#[test]
fn remote_op_applies_only_when_newer_and_is_not_journaled() {
    let s = Store::open_in_memory().unwrap();
    let id = insert_item(&s, "https://a/1");
    s.set_read(id, true).unwrap(); // 今の時刻で記録
    let old = Op { item_id: id, field: Field::Read, value: false, at: "2000-01-01T00:00:00.000Z".into(), device: "phone".into() };
    assert!(!s.apply_remote_op(&old, "mac").unwrap());
    assert!(s.get_item(id).unwrap().unwrap().read);
    let new = Op { at: "2999-01-01T00:00:00.000Z".into(), ..old.clone() };
    assert!(s.apply_remote_op(&new, "mac").unwrap());
    assert!(!s.get_item(id).unwrap().unwrap().read);
    assert_eq!(s.pending_local_ops("mac").unwrap().len(), 1); // 取り込みは記録しない
}

#[test]
fn remote_op_for_unknown_item_is_ignored() {
    let s = Store::open_in_memory().unwrap();
    let op = Op { item_id: 999, field: Field::Starred, value: true, at: "2026-10-13T00:00:00.000Z".into(), device: "phone".into() };
    assert!(!s.apply_remote_op(&op, "mac").unwrap());
}

#[test]
fn sync_ids_are_created_once() {
    let s = Store::open_in_memory().unwrap();
    let a = s.ensure_sync_ids().unwrap();
    let b = s.ensure_sync_ids().unwrap();
    assert_eq!(a, b);
    assert_ne!(a.0, a.1);
}

#[test]
fn items_fetched_on_uses_the_local_day_and_topic_matches() {
    // tz = +09:00。UTC 2026-10-12T15:30Z は JST 10-13 00:30。
    // 記事を2件 (10-12 JST と 10-13 JST) 作り、トピック「認知症」(keywords: 認知症) を1つ作る。
    // 10-13 の記事のタイトルに「認知症」を含める。
    // items_fetched_on(10-13) は1件で、topic_ids == [そのトピックの id]。
}
```

最後のテストは、コメントの条件どおりに既存の挿入補助を使って具体的に書く (時刻は `fetched_at` を直接指定できる補助を使う。なければ `insert_items` に渡す `NewItem` の `fetched_at` を指定する)。

- [ ] **Step 2: 失敗を確かめる**: `cargo test -p infodesk-core store::sync`
- [ ] **Step 3: 実装する**: 版5、メソッド、`set_read`/`set_starred`/`mark_all_read` の変更。既存の migrate のテスト (版を数えるもの) を版5に合わせる。
- [ ] **Step 4: 通ることを確かめる**: `cargo test -p infodesk-core`
- [ ] **Step 5: コミット**: `git commit -m "feat(core): schema v5 with read/star change times and a sync journal"`

---

### Task 3: Drive とのやりとり (core)

**Files:**
- Create: `crates/core/src/sync/drive.rs`, `crates/core/src/sync/google.rs`

**Interfaces:**
- `#[derive(Clone, Debug, PartialEq)] pub struct RemoteFile { pub id: String, pub name: String, pub modified_time: String, pub size: Option<u64> }`
- `#[derive(Debug, thiserror::Error)] pub enum DriveError { #[error("Google ドライブに接続できません: {0}")] Retryable(String), #[error("Google の再ログインが必要です")] AuthExpired, #[error("Google ドライブのエラー: {0}")] Other(String) }`
- `#[async_trait] pub trait DriveStore: Send + Sync { async fn list(&self) -> Result<Vec<RemoteFile>, DriveError>; async fn download(&self, id: &str) -> Result<Vec<u8>, DriveError>; async fn create(&self, name: &str, bytes: Vec<u8>, mime: &str) -> Result<RemoteFile, DriveError>; async fn update(&self, id: &str, bytes: Vec<u8>, mime: &str) -> Result<RemoteFile, DriveError>; async fn delete(&self, id: &str) -> Result<(), DriveError>; }`
- `pub struct MemoryDrive` (`Mutex<BTreeMap<id, (RemoteFile, Vec<u8>)>>`、`fail_next: Mutex<Option<DriveError>>` で次の1回を失敗させられる、`pub fn names(&self) -> Vec<String>`、`pub fn put(&self, name, bytes)` で他端末のファイルを置ける)。`modified_time` は呼び出しごとに増える疑似時刻。
- `pub struct GoogleDrive { http: reqwest::Client, base: String, token: Arc<dyn AccessToken> }`、`#[async_trait] pub trait AccessToken: Send + Sync { async fn get(&self) -> Result<String, DriveError>; async fn invalidate(&self); }`。`base` の既定は `https://www.googleapis.com` (テストでは wiremock の URL)。
  - list: `GET {base}/drive/v3/files?spaces=appDataFolder&fields=nextPageToken,files(id,name,modifiedTime,size)&pageSize=1000[&pageToken=...]`、`nextPageToken` をたどる。
  - download: `GET {base}/drive/v3/files/{id}?alt=media`
  - create: `POST {base}/upload/drive/v3/files?uploadType=multipart&fields=id,name,modifiedTime,size`、本文は `multipart/related` (1つ目 JSON `{"name":..,"parents":["appDataFolder"]}`、2つ目は本体)。
  - update: `PATCH {base}/upload/drive/v3/files/{id}?uploadType=media&fields=id,name,modifiedTime,size`
  - delete: `DELETE {base}/drive/v3/files/{id}` (404 は成功とみなす)
  - 状態コードの分類: 401 は `token.invalidate()` してから1回だけやり直し、それでも 401 なら `AuthExpired`。429、5xx、403 で本文の `reason` が `userRateLimitExceeded` か `rateLimitExceeded` は `Retryable`。接続エラーは `Retryable`。その他は `Other`。

- [ ] **Step 1: 失敗するテストを書く**: wiremock で:
  - `list_follows_page_tokens`: 1ページ目が `nextPageToken: "p2"` と2件、2ページ目が1件 → 3件。
  - `create_sends_multipart_with_app_data_parent`: リクエスト本文に `"parents":["appDataFolder"]` と本体のバイト列が入る。
  - `unauthorized_retries_once_with_a_new_token_then_reports_auth_expired`: 401 を2回返すと `AuthExpired`、`invalidate` が1回呼ばれる。1回目だけ 401 なら成功。
  - `rate_limit_is_retryable`: 403 `{"error":{"errors":[{"reason":"userRateLimitExceeded"}]}}` と 429 と 503 が `Retryable`。
  - `delete_treats_404_as_done`。
  - `MemoryDrive` の `fail_next` が次の1回だけ効くこと。
- [ ] **Step 2: 失敗を確かめる**: `cargo test -p infodesk-core sync::google sync::drive`
- [ ] **Step 3: 実装する**
- [ ] **Step 4: 通ることを確かめる**
- [ ] **Step 5: コミット**: `git commit -m "feat(core): Google Drive appDataFolder client behind a DriveStore trait"`

---

### Task 4: 同期の1回分 (core)

**Files:**
- Create: `crates/core/src/sync/engine.rs`

**Interfaces:**
- `pub struct SyncInput { pub device_id: String, pub db_id: String, pub writer: String /* 端末名 */, pub now: DateTime<Utc>, pub tz: FixedOffset, pub compact: bool }`
- `pub struct SyncReport { pub imported_ops: usize, pub applied_ops: usize, pub uploaded_ops: usize, pub written_files: Vec<String>, pub deleted_files: Vec<String> }`
- `pub async fn run_sync(store: &Mutex<Store>, drive: &dyn DriveStore, input: &SyncInput) -> Result<SyncReport, SyncError>`
  - `SyncError` は `Drive(DriveError)` と `Store(String)`。
  - store は `std::sync::Mutex<Store>` (アプリの `AppState` の持ち方に合わせる。実装前に `app/src-tauri/src/state.rs` の `with_store` を読み、ロックを await をまたいで持たないこと)。
- `pub fn compaction_due(last: Option<DateTime<Utc>>, now: DateTime<Utc>, tz: FixedOffset, daily_time: NaiveTime) -> bool`。今日の `daily_time` を過ぎていて、最後のまとめがそれより前なら true。

**run_sync の順序 (仕様 5.2):**
1. `drive.list()`。`manifest.json` の `db_id` が自分と違っても Mac は書き手なので上書きしてよい (Mac 側では気にしない)。
2. 他端末の `ops-*` (端末 ID が自分でない) のうち、`sync_files` に `kind='in'` で記録のないものを古い順に読み、各 `Op` を `apply_remote_op`。読み終えたファイルを `kind='in'` で記録する。
3. `pending_local_ops` があれば `OpsFile` を作って `ops_file_name(device, now, seq)` で `create`。成功したら `mark_ops_uploaded`。`seq` は同じミリ秒内の重複を避けるための連番 (1から)。
4. 書き出し:
   - `meta.json.gz` (カテゴリー、トピック、ソース)
   - 直近60日 (`now` の tz の日付から59日前まで) の `days_with_items` について `day-*.json.gz`
   - 直近60日の日次、直近400日の週次と月次のダイジェストで、少なくとも1つの scope が保存済みのもの。`stats` は既存の `digest_stats(period, date, tz, None)` の値。
   - 各ファイルは `gzip_json` してハッシュを取り、`sync_files` の `kind='out'` のハッシュと同じなら書かない。違えば `drive_id` があれば `update`、なければ `create`。記録を更新する。
5. `compact` が true なら: 全端末の `ops-*` (自分の分も) と既存の `state-snapshot.json.gz` を読み、`apply_op` で合成し、保持期間内の記事だけ残した `StateSnapshot` を作って書く。`covers` に読んだ `ops-*` の名前を入れ、書けたらそれらを `delete`、`sync_files` からも消す。
6. 保持期間を過ぎた `day-*` と `digest-*` を Drive から消し、`sync_files` からも消す。
7. 最後に `manifest.json` (Drive 上の全ファイルの名前、ハッシュ、サイズ、`updated_at`) を書く。`manifest.json` 自身は `files` に入れない。

途中で `DriveError` になったら、その場でやめて返す。それまでに成功した書き出しの記録は残してよいが、`manifest.json` は書かない。

- [ ] **Step 1: 失敗するテストを書く**: `MemoryDrive` と `Store::open_in_memory()` で:
  - `first_sync_writes_meta_days_digests_and_manifest_last`: 記事2日分とダイジェスト1件。`written_files` の最後が `manifest.json`。manifest の `files` に他のファイルがすべて入り、ハッシュが一致する。
  - `second_sync_without_changes_writes_only_the_manifest`
  - `phone_ops_are_applied_once`: `MemoryDrive::put("ops-phone-...json", ...)` で既読の操作を置く → 同期で Mac の記事が既読になる。2回目の同期では `imported_ops == 0`。
  - `local_ops_are_uploaded_once`
  - `failure_midway_skips_the_manifest_and_retries_next_time`: `fail_next` を書き出しの途中で効かせる → `Err(Retryable)`、manifest がない。次の同期で全部そろう。
  - `compaction_merges_ops_into_the_snapshot_and_deletes_them`: Mac とモバイルの操作 → `compact: true` の同期後、`ops-*` が消え、`state-snapshot.json.gz` に両方の結果が入っている。モバイル側で「スナップショット + 残りの ops」を合成した結果が、まとめる前の全 ops の合成と同じ。
  - `retention_deletes_old_days`: 61日前の `day-*` が消え、60日目は残る。
  - `compaction_due` の境界 (daily_time の直前と直後、同じ日に2回目は false)。
- [ ] **Step 2: 失敗を確かめる**
- [ ] **Step 3: 実装する**
- [ ] **Step 4: 通ることを確かめる**: `cargo test -p infodesk-core`
- [ ] **Step 5: コミット**: `git commit -m "feat(core): one sync pass with import, upload, compaction and retention"`

---

### Task 5: OAuth とキーチェーン (app)

**Files:**
- Create: `app/src-tauri/src/sync/mod.rs`, `app/src-tauri/src/sync/oauth.rs`, `app/src-tauri/src/sync/keychain.rs`, `app/src-tauri/build.rs` に追記, `app/src-tauri/google-oauth.example.json`
- Modify: `.gitignore` (`app/src-tauri/google-oauth.json`)、`app/src-tauri/Cargo.toml` (`sha2 = "0.10.9"`、`base64 = "0.22.1"`、`rand = "0.10.3"` は使わずに `uuid` の v4 か `getrandom` で足りるならそれでよい。キーチェーンは `security-framework` を Cargo.lock の版で使い、API は `~/.cargo/registry/src/*/security-framework-*/src/passwords.rs` を読んで確かめる)

**ビルド時の埋め込み:** `build.rs` で `google-oauth.json` (`{"client_id": "...", "client_secret": "..."}`) があれば `cargo:rustc-env=INFODESK_GOOGLE_CLIENT_ID=...` と `..._SECRET=...` を出し、`cargo:rerun-if-changed=google-oauth.json`。コードでは `option_env!` で読み、どちらかが無ければ `oauth_config() -> Option<OAuthConfig>` が `None`。

**Interfaces:**
- `pub struct OAuthConfig { pub client_id: String, pub client_secret: String, pub auth_url: String, pub token_url: String, pub revoke_url: String }` (既定 `https://accounts.google.com/o/oauth2/v2/auth`、`https://oauth2.googleapis.com/token`、`https://oauth2.googleapis.com/revoke`)
- `pub fn pkce_pair() -> (String /*verifier*/, String /*challenge S256*/)`
- `pub fn auth_url(cfg: &OAuthConfig, redirect: &str, challenge: &str, state: &str) -> String`。`scope=https://www.googleapis.com/auth/drive.appdata openid email`、`access_type=offline`、`prompt=consent`、`code_challenge_method=S256`、`response_type=code`。
- `pub async fn login(app: &AppHandle) -> AppResult<String /*email*/>`。`127.0.0.1:0` で待ち受け、既定のブラウザで `auth_url` を開き (既存の opener を使う)、`/?code=..&state=..` を1回だけ受け、`state` を照合し、簡単な日本語の HTML (「ログインしました。InfoDesk に戻ってください。」) を返し、コードをトークンに交換する。更新用トークンをキーチェーンへ、メールアドレス (`id_token` の `email`。署名の検証はしない。表示用) を設定 `sync.account_email` へ。待ち受けは5分で打ち切る。
- `pub struct TokenSource` (`AccessToken` を実装): キーチェーンの更新用トークンでアクセストークンを取り、期限 (`expires_in` - 60秒) まで覚える。`invalid_grant` なら `AuthExpired`。
- `pub async fn logout(app)`。revoke を呼び (失敗しても続ける)、キーチェーンと `sync.account_email` を消す。

- [ ] **Step 1: 失敗するテストを書く**
  - `pkce_challenge_matches_rfc7636_example`: RFC 7636 付録 B の verifier `dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk` から challenge `E9Melhoa2OwvFrEMTJguCHoeV2N9H4NqmzSDqAHUcPM`。
  - `auth_url_has_offline_access_and_only_app_data_scope`
  - `token_source_refreshes_and_caches`: wiremock の token エンドポイント。2回 `get` しても1回だけ呼ぶ。期限切れ後はもう1回。
  - `invalid_grant_means_auth_expired`
  - `callback_parser_rejects_wrong_state`: 受け取った URL の解析を関数に分けてテストする。
  - キーチェーンは実機のキーチェーンを触らないよう、trait `SecretStore` にし、テストではメモリ版を使う。
- [ ] **Step 2〜4:** 失敗、実装、成功 (`cargo test -p infodesk-app sync`)
- [ ] **Step 5: コミット**: `git commit -m "feat(app): Google sign-in with PKCE and a keychain-held refresh token"`

---

### Task 6: 同期をアプリに組み込む (app)

**Files:**
- Modify: `app/src-tauri/src/sync/mod.rs`, `app/src-tauri/src/state.rs` (同期の状態)、`app/src-tauri/src/jobs.rs` (収集とダイジェストの後)、`app/src-tauri/src/commands/` (新しいコマンド)、`app/src-tauri/src/lib.rs` (`generate_handler!`)

**Interfaces:**
- `AppState` に `sync: Mutex<SyncStatus>` と、同期中かどうかのフラグ。`#[derive(Serialize)] pub struct SyncStatus { pub available: bool /* oauth_config があるか */, pub signed_in: bool, pub account_email: Option<String>, pub enabled: bool, pub syncing: bool, pub last_sync_at: Option<DateTime<Utc>>, pub error: Option<String>, pub device_name: String }`
- イベント `sync-changed`。
- `pub async fn sync_once(state: &AppState) -> AppResult<()>`。無効、未ログイン、同期中ならすぐ戻る。`run_sync` を呼び、`compact` は `compaction_due(設定 sync.last_compaction_at, ...)`。成功で `last_sync_at` と `sync.last_sync_at`、まとめたら `sync.last_compaction_at`。失敗は `SyncStatus.error` だけに入れる (`last_error` は触らない)。`AuthExpired` なら `signed_in=false` にして「Google の再ログインが必要です」。取り込みで記事が変わったら `items-changed` を出す。
- 呼ぶ場所: `run_collect` の成功の後、`run_digests` の後 (スケジュールと手動の両方)、`collect_now` (Ctrl+R) の後。どれも `tauri::async_runtime::spawn` で投げ、呼び元を待たせない。
- コマンド: `sync_status() -> SyncStatus`、`sync_login() -> SyncStatus`、`sync_logout() -> SyncStatus`、`sync_set_enabled(enabled: bool)`、`sync_set_device_name(name: String)`、`sync_now()`。

- [ ] **Step 1: 失敗するテストを書く**: `MemoryDrive` を差し込めるようにして (例: `AppState` に `drive_factory` を持たせる、またはテスト用の関数に drive を渡す):
  - `sync_errors_do_not_touch_last_error`
  - `auth_expired_signs_out_and_says_so`
  - `sync_is_skipped_when_disabled_or_signed_out`
  - `a_running_sync_is_not_started_twice`
  - `imported_changes_emit_items_changed`
  - 収集の後に同期が投げられること (既存の jobs のテストのやり方に合わせ、通知の記録で確かめる)。
- [ ] **Step 2〜4:** 失敗、実装、成功 (`cargo test --workspace`、`cargo clippy --all-targets -- -D warnings`)
- [ ] **Step 5: コミット**: `git commit -m "feat(app): run a sync after collections and digests, with its own status and errors"`

---

### Task 7: 設定の「同期」タブ (UI)

**Files:**
- Create: `app/src/views/settings/SyncTab.tsx`, `app/src/views/settings/SyncTab.test.tsx`
- Modify: `app/src/views/SettingsView.tsx` (タブ「同期」を最後に)、`app/src/api.ts`、`app/src/types.ts` (`SyncStatus`)、`EVENTS.sync = "sync-changed"`

**画面 (A-13 に合わせる):**
- 「Google ドライブ」節: 説明文「収集した記事とダイジェストを、Google ドライブのアプリ専用の領域に置きます。スマホの InfoDesk から読めます。」
  - 未ログイン: 行「Google アカウント」、補足「ログインすると、ブラウザで Google の画面が開きます。ID やパスワードはアプリに入力しません。」、主ボタン「Google でログイン」(押している間は「ブラウザでログインしてください…」で無効)。
  - ログイン済み: 補足「{email} でログイン中」、ボタン「ログアウト」。
  - `available=false`: 説明の下に「このビルドでは同期を使えません」だけを出し、他の行は出さない。
- 「同期」節: 「同期する」(トグル、未ログインなら無効)、「この Mac の名前」(入力欄、Enter か入力欄から離れたときに保存。IME 変換中の Enter は無視。既存の `shared.tsx` の部品を使う)、「最終同期」(時刻、「今すぐ同期」ボタン、同期中は無効)、エラーがあれば赤い1行。
- 「Drive に置く期間」節: 説明「記事と日次ダイジェストは直近60日、週次と月次のダイジェストは直近400日。スマホの控えは消えません。」
- `sync-changed` で取り直す。

- [ ] **Step 1: 失敗するテストを書く**: 未ログイン、ログイン済み、`available=false`、ログインボタンが `sync_login` を呼ぶ、トグルが `sync_set_enabled`、名前の保存、今すぐ同期、エラー表示、イベントで再取得。
- [ ] **Step 2〜4:** 失敗、実装、成功 (`cd app && npm test -- --run && npm run build`)
- [ ] **Step 5: コミット**: `git commit -m "feat(ui): settings sync tab with Google sign-in"`

---

### Task 8: 手順書、ドキュメント、確認

**Files:**
- Create: `docs/12-cloud-sync.md`, `docs/12-cloud-sync.ja.md`
- Modify: `docs/11-keyboard-shortcuts*` (次へのリンク)、`docs/07-settings*` (同期タブと設定キー)、`docs/09-privacy-and-security*` (Drive に送るもの、権限、キーチェーン)、`docs/10-roadmap*`、`README.md`/`README.ja.md` (表に12、ハイライト1行)、`docs/internal/architecture.md`、`data-model.md` (版5)、`feature-inventory.md`

**12-cloud-sync の中身:**
1. 何をするか (Mac が書き出し、スマホが読む。既読とスターは双方向)。
2. 使い方 (設定 → 同期 → Google でログイン → 同期する)。
3. 開発者向け: Google 側の準備 (仕様 7節の手順を具体的に。Cloud Console の画面名、公開状態を「本番」にする理由 (テストだと7日で切れる)、`google-oauth.json` の置き場所と形、ビルド)。
4. Drive に置くファイルと保持期間。
5. 困ったとき (再ログイン、Drive の容量、レート制限)。

- [ ] **Step 1:** 文書を書く (英語が正本、日本語の対。絵文字とダッシュ記号は使わない)。
- [ ] **Step 2:** 全体の確認

```bash
cargo test --workspace && cargo clippy --all-targets -- -D warnings && cargo fmt --check
cd app && npm test -- --run && npm run build && cd ..
cd agent && npm test -- --run && cd ..
```

- [ ] **Step 3:** 手動の確認一覧を報告に書く (利用者が行う): Google でログインできる、同期で Drive にファイルができる (Drive の「設定 → アプリの管理」で InfoDesk が隠しデータを持つことが見える)、ログアウト、再ログイン、ネットワークを切ったときのエラー表示と回復。
- [ ] **Step 4: コミット**: `git commit -m "docs: cloud sync guide and Google setup"`
