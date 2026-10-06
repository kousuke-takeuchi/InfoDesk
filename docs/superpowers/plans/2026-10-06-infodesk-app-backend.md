# InfoDesk App Backend (Plan 2a) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Tauri 2 のアプリ本体 (Rust側) を作る。core を抱えるアプリ状態、画面が呼ぶコマンド、収集と要約のジョブ、スケジューラー、メニューバー常駐、ログイン時起動。画面 (React) は Plan 2b で作る。

**Architecture:** `app/src-tauri` が `infodesk-core` を依存に持つ。`AppState` が `Mutex<Store>` と実行状態を保持し、Tauriコマンドは `AppState` を受け取る薄い関数 (`commands/*.rs`) で、ロジックは `AppState` のメソッドとして単体テストする。LLM呼び出しは「計画 (ロック内、同期)、LLM (ロック外、非同期)、保存 (ロック内、同期)」の3段階で行い、DB接続を `await` をまたいで保持しない。進行状況はイベント (`status-changed`、`items-changed`、`digests-changed`) で画面に通知する。

**Tech Stack:** Tauri 2.11 (`tray-icon` feature)、`tauri-plugin-opener` 2、`tauri-plugin-autostart` 2、tokio、chrono、serde。フロントは create-tauri-app の `react-ts` テンプレート (Plan 2b で実装)。

**Spec:** `docs/superpowers/specs/2026-10-06-infodesk-design.md` (3節 scheduler、7節 メニューバー、8節、9節)。前提: Plan 1 (`2026-10-06-infodesk-core.md`) が完了し、その末尾「Plan 2 への引き継ぎ」の項目をこの計画で扱う。

**この計画の範囲外 (Plan 2b):** React の画面、CSS、フロントの単体テスト。Plan 2b はこの計画のコマンドとイベントをそのまま使う。

## Global Constraints

- 対象OSはmacOSのみ。Tauri は 2.x 系 (`cargo add tauri@2`)。3.0 alpha は使わない。
- コミットメッセージに `Co-Authored-By` などのAI帰属トレーラーを付けない。
- 依存は `cargo add` / `npm install` で追加する。版を手書きしない。
- `cargo test`、`cargo clippy --all-targets -- -D warnings`、`cargo fmt --check` を各タスクのコミット前に通す。
- 時刻はUTCのRFC3339文字列でDBに保存する。日付の境界はローカルタイムゾーン (`Local::now().offset()`)。週は月曜始まり。
- Tauriコマンドの非同期関数は `Send` な Future でなければならない。`MutexGuard<Store>` を `await` をまたいで保持しない。
- Claudeが選択されていて失敗した場合、Ollamaへ自動切替しない。
- LLMが応答しない場合、要約は保留として残し、次回のスケジュールで再試行する。
- DBの場所は Tauri の `app_data_dir()` 直下の `infodesk.db` (macOSでは `~/Library/Application Support/jp.infodesk.app/infodesk.db`)。CLI の既定パスとは異なる。CLI で同じDBを見るときは `--db` で指定する。
- 設定キー (spec 8節): `llm.items_provider`、`llm.digest_provider`、`llm.chat_provider`、`ollama.base_url`、`ollama.model`、`ollama.web_search_api_key`、`claude.bin`、`node.bin`、`summary.top_n`、`schedule.collect_interval_hours`、`schedule.daily_time`、`schedule.weekly_day`、`schedule.weekly_time`、`schedule.monthly_time`、`app.launch_at_login`、`app.stay_resident`。

## スケジュールの決定 (specの補足。Task 6 で spec 6節に追記する)

- 収集のたびに: 記事要約 (当日分の上位N件) を付け、当日の日次要約を `force = true` で作り直す。
- 日次の時刻 (既定 7:00): 前日の日次要約を確定し (未生成なら生成)、現在の週次と月次を `force = true` で作り直す。
- 週次の時刻 (既定 月曜 7:30): 前週の週次要約を生成する。月次の時刻 (既定 1日 8:00): 前月の月次要約を生成する。
- 毎回のスケジュール実行の最後に、保留中の要約を再試行する。
- 起動時: 最後の収集から間隔以上経っていれば収集する。実行されなかった日次、週次、月次があれば補う。

## Review Focus

1. 収集中に「今すぐ収集」を押す。二重に走らず、2回目は無視される (状態 `collecting` で拒否)。Task 5 でテストする。
2. アプリ起動直後に Ollama が落ちている。収集は成功し、要約は保留になり、状態に最後のエラーが残る。ダイジェストとフィードは使える。Task 5 でテストする。
3. PCがスリープしていて 7:00 を過ぎてから復帰した。日次のジョブが1回だけ実行される (スキップもされないし、分単位で何度も走らない)。Task 6 でテストする。
4. 設定の時刻やDBの値が壊れている (`schedule.daily_time = "abc"`)。既定値で動き、パニックしない。Task 6 でテストする。
5. 旧版のDB (Plan 1 で作った `user_version = 0`) を開く。`digests.last_error` 列が追加され、既存データが残る。Task 2 でテストする。

## File Structure

```
Cargo.toml                                members に app/src-tauri を追加
app/                                      create-tauri-app (react-ts) が生成
  package.json, vite.config.ts, src/      Plan 2b で実装 (この計画ではテンプレートのまま)
  src-tauri/
    Cargo.toml                            package infodesk-app, lib infodesk_app_lib
    tauri.conf.json                       productName、ウィンドウ、identifier
    capabilities/default.json             権限
    src/main.rs                           テンプレート (lib の run を呼ぶ)
    src/lib.rs                            Builder: プラグイン、状態、コマンド登録、ウィンドウイベント、トレイ、スケジューラー起動
    src/error.rs                          AppError (Serialize)、AppResult
    src/state.rs                          AppState、AppStatus、Notifier、LlmFactory
    src/commands/mod.rs                   generate_handler 用の再エクスポート
    src/commands/settings.rs              get_settings、set_setting、test_ollama、test_claude、set_launch_at_login
    src/commands/status.rs                get_status
    src/commands/items.rs                 list_items、get_item、set_read、set_starred、mark_all_read、open_item、sidebar_counts
    src/commands/sources.rs               list_sources、add_rss_source、update_source、set_source_enabled、delete_source
    src/commands/topics.rs                list_topics、add_topic、update_topic、delete_topic
    src/commands/digests.rs               get_digests、regenerate_digest、collect_now、retry_pending
    src/jobs.rs                           run_collect、run_item_summaries、run_digests、retry_pending (AppState を受ける非同期関数)
    src/scheduler.rs                      due_jobs (純粋関数)、run_loop
    src/tray.rs                           トレイのメニュー作成と更新
crates/core/src/store/migrate.rs          user_version による移行
crates/core/src/store/{mod,digests,items}.rs   WAL、last_error、count_items
crates/core/src/summarizer/mod.rs         finish_digest が error を渡す
crates/core/src/seed.rs                   設定の既定値を追加、設定は毎回補う
```

---

### Task 1: Tauri アプリの雛形

**Files:**
- Create: `app/` (create-tauri-app が生成)
- Modify: `Cargo.toml` (ワークスペース), `.gitignore`, `app/src-tauri/Cargo.toml`, `app/src-tauri/src/main.rs`, `app/src-tauri/tauri.conf.json`

**Interfaces:**
- Consumes: なし
- Produces: ワークスペースのパッケージ `infodesk-app` (lib 名 `infodesk_app_lib`、`pub fn run()`)、`cargo build -p infodesk-app` と `npm run build` (app/) が通る状態

- [ ] **Step 1: 雛形を生成する**

```bash
cd /Users/ktakeuchi/Documents/InfoDesk
npx -y create-tauri-app@latest app --template react-ts --manager npm --identifier jp.infodesk.app --yes
cd app && npm install && cd ..
```

Expected: `app/package.json`、`app/src-tauri/Cargo.toml`、`app/src-tauri/tauri.conf.json` ができる。`app/src-tauri/Cargo.toml` の `[package] name` は `app`、`[lib] name` は `app_lib`。

- [ ] **Step 2: パッケージ名とワークスペースを整える**

`Cargo.toml` (リポジトリ直下) の members を次にする:

```toml
members = ["crates/core", "crates/cli", "app/src-tauri"]
```

`app/src-tauri/Cargo.toml` を編集: `[package]` の `name = "app"` を `name = "infodesk-app"` に、`[lib]` の `name = "app_lib"` を `name = "infodesk_app_lib"` に変える。`[package]` に `rust-version = "1.89"` を追加する。`app/src-tauri/src/main.rs` の `app_lib::run()` を `infodesk_app_lib::run()` に変える。

`app/src-tauri/` に `Cargo.lock` や `[workspace]` が生成されていれば削除する (ルートのワークスペースに入れるため)。

`.gitignore` (リポジトリ直下) に追加:

```
app/node_modules/
app/dist/
```

- [ ] **Step 3: 依存を追加する**

```bash
cd app/src-tauri
cargo add tauri@2 --features tray-icon
cargo add tauri-plugin-opener@2
cargo add tauri-plugin-autostart@2 --target 'cfg(any(target_os = "macos", windows, target_os = "linux"))'
cargo add infodesk-core --path ../../crates/core
cargo add tokio --features rt-multi-thread,macros,time
cargo add chrono --features serde
cargo add serde --features derive
cargo add serde_json
cargo add reqwest --features json
cd ../..
```

`tauri` と `tauri-plugin-opener` はテンプレートが既に入れている。`cargo add` は既存の行を更新する。結果の `[dependencies]` を見て、`tauri` に `features = ["tray-icon"]` が付いていることを確認する。

- [ ] **Step 4: tauri.conf.json を整える**

`app/src-tauri/tauri.conf.json` を編集する。既存のキーを次の値にする (他のキーは残す):

```json
{
  "productName": "InfoDesk",
  "identifier": "jp.infodesk.app",
  "app": {
    "windows": [
      {
        "label": "main",
        "title": "InfoDesk",
        "width": 1200,
        "height": 800,
        "minWidth": 960,
        "minHeight": 640
      }
    ]
  }
}
```

`app.windows[0]` の既存の `title`/`width`/`height` を上書きし、`label`、`minWidth`、`minHeight` を追加する。`build` セクション (`beforeDevCommand`、`devUrl`、`frontendDist`) はテンプレートのままにする。

- [ ] **Step 5: ビルドを確認する**

Run: `cargo build -p infodesk-app 2>&1 | tail -3 && (cd app && npm run build 2>&1 | tail -3)`
Expected: 両方とも成功。初回の Tauri ビルドは数分かかる。

Run: `cargo test 2>&1 | grep "test result" && cargo clippy --all-targets -- -D warnings 2>&1 | tail -1 && cargo fmt --check && echo FMT_OK`
Expected: core の 75 件が passed、clippy と fmt が clean。テンプレートの `greet` コマンドが clippy に触れた場合は、`lib.rs` から `greet` と `invoke_handler` の行を削除して再実行する (Task 3 で置き換える)。

- [ ] **Step 6: コミット**

```bash
git add Cargo.toml Cargo.lock .gitignore app
git commit -m "feat(app): scaffold Tauri app"
```

---

### Task 2: core の追加 (移行、WAL、last_error、件数、設定の既定値)

**Files:**
- Create: `crates/core/src/store/migrate.rs`
- Modify: `crates/core/src/store/mod.rs`, `crates/core/src/store/schema.rs`, `crates/core/src/store/digests.rs`, `crates/core/src/store/items.rs`, `crates/core/src/model.rs`, `crates/core/src/summarizer/mod.rs`, `crates/core/src/seed.rs`

**Interfaces:**
- Consumes: Plan 1 の `Store`, `NewDigest`, `Digest`, `ItemFilter`, `finish_digest`
- Produces:
  - `Store::open(path)` が `PRAGMA journal_mode=WAL` と `busy_timeout=5000` を設定し、`migrate` を実行する。`user_version` は 2。
  - `model::Digest` に `pub last_error: Option<String>` が増える。
  - `Store::mark_digest_attempt_failed(&self, period, start, scope, error: &str, now) -> Result<DigestStatus>` (引数 `error` が増える。`last_error` に保存。`save_digest_done` は `last_error` を NULL にする)
  - `Store::count_items(&self, filter: &ItemFilter) -> Result<i64>` (`limit`/`offset` を無視した件数)
  - `seed::DEFAULT_SETTINGS` に新しいキーが増え、`seed_defaults` は初回以外でも不足している設定を補う (戻り値は初回のソース件数のまま)。

- [ ] **Step 1: 失敗するテストを書く (移行)**

`crates/core/src/store/migrate.rs`:

```rust
use rusqlite::Connection;

use super::schema::SCHEMA;
use crate::error::Result;

/// 現在のスキーマ版。`migrations` の最後の版と一致させる。
pub const SCHEMA_VERSION: i32 = 2;

#[cfg(test)]
mod tests {
    use super::*;

    fn version(conn: &Connection) -> i32 {
        conn.query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap()
    }

    fn has_column(conn: &Connection, table: &str, col: &str) -> bool {
        let mut stmt = conn
            .prepare(&format!("PRAGMA table_info({table})"))
            .unwrap();
        stmt.query_map([], |r| r.get::<_, String>(1))
            .unwrap()
            .any(|c| c.unwrap() == col)
    }

    #[test]
    fn fresh_database_reaches_current_version() {
        let conn = Connection::open_in_memory().unwrap();
        migrate(&conn).unwrap();
        assert_eq!(version(&conn), SCHEMA_VERSION);
        assert!(has_column(&conn, "digests", "last_error"));
        // 2回目は何もしない。
        migrate(&conn).unwrap();
        assert_eq!(version(&conn), SCHEMA_VERSION);
    }

    #[test]
    fn plan1_database_without_version_gets_last_error_column_and_keeps_rows() {
        // Review Focus 5: Plan 1 のスキーマ (user_version = 0、last_error なし)。
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(SCHEMA).unwrap();
        conn.execute(
            "INSERT INTO digests(period, period_start, scope, body, status, created_at) \
             VALUES ('day', '2026-10-06', 'category:github', 'kept', 'done', '2026-10-06T00:00:00Z')",
            [],
        )
        .unwrap();
        assert!(!has_column(&conn, "digests", "last_error"));

        migrate(&conn).unwrap();
        assert_eq!(version(&conn), SCHEMA_VERSION);
        assert!(has_column(&conn, "digests", "last_error"));
        let body: String = conn
            .query_row("SELECT body FROM digests", [], |r| r.get(0))
            .unwrap();
        assert_eq!(body, "kept");
    }

    #[test]
    fn newer_database_is_rejected() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA user_version = 99").unwrap();
        assert!(migrate(&conn).is_err());
    }
}
```

`crates/core/src/store/mod.rs` のモジュール宣言に `mod migrate;` を追加し、`init` を次に変える:

```rust
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let conn = Connection::open(path)?;
        // CLI、アプリ、将来のサイドカーが同じファイルを使うため。
        conn.execute_batch("PRAGMA journal_mode = WAL; PRAGMA busy_timeout = 5000;")?;
        Self::init(conn)
    }

    pub fn open_in_memory() -> Result<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Self> {
        migrate::migrate(&conn)?;
        Ok(Self { conn })
    }
```

`crates/core/src/store/mod.rs` のテストに追加:

```rust
    #[test]
    fn file_database_uses_wal() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("w.db")).unwrap();
        let mode: String = store
            .conn
            .query_row("PRAGMA journal_mode", [], |r| r.get(0))
            .unwrap();
        assert_eq!(mode, "wal");
    }
```

- [ ] **Step 2: テストが失敗することを確認する**

Run: `cargo test -p infodesk-core store::migrate`
Expected: コンパイルエラー。`migrate` が未定義。

- [ ] **Step 3: 移行を実装する**

`crates/core/src/store/migrate.rs` の `SCHEMA_VERSION` の下、テストの上に追加:

```rust
/// (適用後の版, SQL)。版の順に並べる。版1は Plan 1 のスキーマ全体。
const MIGRATIONS: &[(i32, &str)] = &[
    (1, SCHEMA),
    (2, "ALTER TABLE digests ADD COLUMN last_error TEXT;"),
];

fn user_version(conn: &Connection) -> Result<i32> {
    Ok(conn.query_row("PRAGMA user_version", [], |r| r.get(0))?)
}

/// `user_version` を見て、足りない移行を順に適用する。
/// 版0のDBは、空の新規DBか Plan 1 のスキーマのどちらかである。
/// 版1のSQLは `IF NOT EXISTS` だけなので、どちらにも適用できる。
pub fn migrate(conn: &Connection) -> Result<()> {
    let current = user_version(conn)?;
    if current > SCHEMA_VERSION {
        return Err(crate::error::Error::Invalid(format!(
            "database schema version {current} is newer than supported {SCHEMA_VERSION}"
        )));
    }
    for (version, sql) in MIGRATIONS {
        if *version <= current {
            continue;
        }
        conn.execute_batch(&format!(
            "BEGIN; {sql} PRAGMA user_version = {version}; COMMIT;"
        ))?;
    }
    Ok(())
}
```

- [ ] **Step 4: テストが通ることを確認する**

Run: `cargo test -p infodesk-core store::`
Expected: 移行3件と WAL 1件を含めて全件 passed。

- [ ] **Step 5: 失敗するテストを書く (last_error、count_items、設定)**

`crates/core/src/model.rs` の `Digest` に `pub last_error: Option<String>,` を `attempts` の後に追加する。

`crates/core/src/store/digests.rs` のテストを変更する。`mark_digest_attempt_failed` の呼び出し4か所すべてに、`&scope` (または `&gh`) の後に `"boom"` を追加する (例: `store.mark_digest_attempt_failed(Period::Day, day, &scope, "boom", &now)`)。さらに `failed_attempts_go_pending_then_failed_and_success_recovers` の `attempts` の assert の次に追加:

```rust
        assert_eq!(
            store.get_digest(Period::Day, day, &scope).unwrap().unwrap().last_error.as_deref(),
            Some("boom")
        );
```

同じテストの末尾 (`save_digest_done` の後の `status` の assert の次) に追加:

```rust
        assert_eq!(store.get_digest(Period::Day, day, &scope).unwrap().unwrap().last_error, None);
```

`crates/core/src/store/items.rs` のテストに追加:

```rust
    #[test]
    fn count_items_ignores_limit_and_respects_filter() {
        let (store, src) = store_with_source(Category::TechJp);
        let t = at("2026-10-05T00:00:00Z");
        let items: Vec<NewItem> = (0..7)
            .map(|i| new_item(&format!("https://e.com/{i}"), &format!("t{i}"), "", None))
            .collect();
        store.upsert_items(&src, &items, &t).unwrap();
        store.set_read(store.query_items(&ItemFilter::default()).unwrap()[0].id, true).unwrap();
        let f = ItemFilter { category: Some(Category::TechJp), limit: 2, ..Default::default() };
        assert_eq!(store.count_items(&f).unwrap(), 7);
        let f = ItemFilter { unread_only: true, ..Default::default() };
        assert_eq!(store.count_items(&f).unwrap(), 6);
        let f = ItemFilter { category: Some(Category::Arxiv), ..Default::default() };
        assert_eq!(store.count_items(&f).unwrap(), 0);
    }
```

`crates/core/src/seed.rs` のテスト `seeds_once_and_covers_every_category` の `ollama.model` の assert の次に追加:

```rust
        assert_eq!(store.get_setting("schedule.daily_time").unwrap().as_deref(), Some("07:00"));
        assert_eq!(store.get_setting("app.stay_resident").unwrap().as_deref(), Some("true"));
```

同じファイルに新しいテストを追加:

```rust
    #[test]
    fn reseeding_fills_missing_settings_without_touching_existing_ones() {
        let store = Store::open_in_memory().unwrap();
        seed_defaults(&store).unwrap();
        store.set_setting("ollama.model", "other").unwrap();
        store.conn_execute_for_test("DELETE FROM settings WHERE key = 'schedule.daily_time'");
        assert_eq!(seed_defaults(&store).unwrap(), 0);
        assert_eq!(store.get_setting("ollama.model").unwrap().as_deref(), Some("other"));
        assert_eq!(store.get_setting("schedule.daily_time").unwrap().as_deref(), Some("07:00"));
    }
```

`crates/core/src/store/mod.rs` に、テスト専用のヘルパーを追加 (`impl Store` ブロックの末尾):

```rust
    #[cfg(test)]
    pub(crate) fn conn_execute_for_test(&self, sql: &str) {
        self.conn.execute(sql, []).unwrap();
    }
```

`crates/core/src/summarizer/mod.rs` のテスト `llm_failure_and_empty_output_leave_digest_pending_then_failed` で、`assert_eq!(saved.attempts, 2);` の次に追加:

```rust
        assert!(saved.last_error.as_deref().unwrap_or_default().contains("empty response"), "{:?}", saved.last_error);
```

- [ ] **Step 6: テストが失敗することを確認する**

Run: `cargo test -p infodesk-core 2>&1 | grep -E "^error" | head`
Expected: コンパイルエラー。`mark_digest_attempt_failed` の引数数が合わない、`count_items` と `last_error` が未定義。

- [ ] **Step 7: 実装する**

`crates/core/src/store/digests.rs`:

- `DIGEST_COLS` を `"id, period, period_start, scope, body, item_ids, provider, status, attempts, created_at, last_error"` にし、`row_to_digest` に `last_error: r.get(10)?,` を追加する。
- `save_digest_done` の `DO UPDATE SET` に `last_error = NULL,` を追加する (`status = 'done'` の前)。
- `mark_digest_attempt_failed` のシグネチャを `(&self, period: Period, start: NaiveDate, scope: &DigestScope, error: &str, now: &DateTime<Utc>)` にし、SQLを次に変える:

```rust
        self.conn.execute(
            "INSERT INTO digests(period, period_start, scope, status, attempts, created_at, last_error) \
             VALUES (?1, ?2, ?3, 'pending', 1, ?4, ?6) \
             ON CONFLICT(period, period_start, scope) DO UPDATE SET \
               attempts = attempts + 1, \
               last_error = excluded.last_error, \
               status = CASE WHEN attempts + 1 >= ?5 THEN 'failed' ELSE 'pending' END \
             WHERE digests.status != 'done'",
            params![
                period.as_str(),
                date_str(start),
                scope.key(),
                ts(now),
                MAX_ATTEMPTS,
                error
            ],
        )?;
```

`crates/core/src/summarizer/mod.rs` の `finish_digest` の `Err(e)` 分岐を次に変える:

```rust
        Err(e) => {
            let message = e.to_string();
            Ok(DigestOutcome {
                status: store.mark_digest_attempt_failed(
                    job.period,
                    job.period_start,
                    &job.scope,
                    &message,
                    now,
                )?,
                error: Some(message),
            })
        }
```

`crates/core/src/store/items.rs`: `query_items` の WHERE 句の組み立て (`let mut wh` から `let where_sql = ...` まで) を関数に切り出す:

```rust
    /// 絞り込みを WHERE 句と引数に変換する。`limit`/`offset`/`order` は扱わない。
    fn build_where(&self, f: &ItemFilter, args: &mut Vec<Value>) -> Result<String> {
        let mut wh: Vec<String> = Vec::new();
        // ここに query_items にあった category〜text の条件追加をそのまま移す
        Ok(if wh.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", wh.join(" AND "))
        })
    }
```

`query_items` は `let mut args = Vec::new(); let where_sql = self.build_where(f, &mut args)?;` に置き換える。追加:

```rust
    pub fn count_items(&self, f: &ItemFilter) -> Result<i64> {
        let mut args: Vec<Value> = Vec::new();
        let where_sql = self.build_where(f, &mut args)?;
        let sql = format!("SELECT COUNT(*) FROM items {where_sql}");
        Ok(self
            .conn
            .query_row(&sql, params_from_iter(args), |r| r.get(0))?)
    }
```

`crates/core/src/seed.rs`: `DEFAULT_SETTINGS` を次にする:

```rust
pub const DEFAULT_SETTINGS: &[(&str, &str)] = &[
    ("llm.items_provider", "ollama"),
    ("llm.digest_provider", "ollama"),
    ("llm.chat_provider", "ollama"),
    ("ollama.base_url", "http://localhost:11434"),
    ("ollama.model", "gemma4:26b"),
    ("ollama.web_search_api_key", ""),
    ("claude.bin", "claude"),
    ("node.bin", "node"),
    ("summary.top_n", "20"),
    ("schedule.collect_interval_hours", "3"),
    ("schedule.daily_time", "07:00"),
    ("schedule.weekly_day", "mon"),
    ("schedule.weekly_time", "07:30"),
    ("schedule.monthly_time", "08:00"),
    ("app.launch_at_login", "false"),
    ("app.stay_resident", "true"),
];
```

`seed_defaults` を次にする (設定の補完を seeded 判定の前に出す):

```rust
pub fn seed_defaults(store: &Store) -> Result<usize> {
    for (key, value) in DEFAULT_SETTINGS {
        if store.get_setting(key)?.is_none() {
            store.set_setting(key, value)?;
        }
    }
    if store.get_setting(SEEDED_KEY)?.is_some() {
        return Ok(0);
    }
    for (kind, name, category, config) in DEFAULT_SOURCES {
        store.add_source(*kind, name, *category, config)?;
    }
    store.set_setting(SEEDED_KEY, "1")?;
    Ok(DEFAULT_SOURCES.len())
}
```

- [ ] **Step 8: テストが通ることを確認する**

Run: `cargo test 2>&1 | grep "test result" && cargo clippy --all-targets -- -D warnings 2>&1 | tail -1 && cargo fmt --check && echo FMT_OK`
Expected: core が 81 件 passed (75 + 移行3 + WAL1 + count1 + seed1)、CLI と app のビルドも通る。clippy と fmt が clean。

- [ ] **Step 9: コミット**

```bash
git add crates/core
git commit -m "feat(core): add schema migrations, WAL, digest last_error, item counts"
```

---

### Task 3: アプリ状態、エラー、設定と状態のコマンド

**Files:**
- Create: `app/src-tauri/src/error.rs`, `app/src-tauri/src/state.rs`, `app/src-tauri/src/commands/mod.rs`, `app/src-tauri/src/commands/settings.rs`, `app/src-tauri/src/commands/status.rs`
- Modify: `app/src-tauri/src/lib.rs`

**Interfaces:**
- Consumes: `infodesk_core::{Store, seed::seed_defaults, llm::{LlmProvider, LlmRole, provider_from_settings}, collector::http_client}`
- Produces:
  - `error::AppError { message: String }` (Serialize)、`From<infodesk_core::Error>`、`From<String>`、`From<tauri::Error>`、`error::AppResult<T>`
  - `state::AppStatus { collecting, summarizing, digesting: bool, last_collect_at: Option<DateTime<Utc>>, next_collect_at: Option<DateTime<Utc>>, last_error: Option<String> }` (Serialize, Clone, Default)
  - `state::Notifier = Arc<dyn Fn(&str) + Send + Sync>` (イベント名を受ける)
  - `state::LlmFactory = Arc<dyn Fn(&Store, LlmRole) -> infodesk_core::Result<Box<dyn LlmProvider>> + Send + Sync>`
  - `state::AppState { store: Mutex<Store>, http: reqwest::Client, status: Mutex<AppStatus>, tz: FixedOffset, notify: Notifier, llm: LlmFactory }`
  - `AppState::new(store: Store, notify: Notifier) -> AppState` (tz は `Local`、llm は `provider_from_settings`)
  - `AppState::with_store<T>(&self, f: impl FnOnce(&Store) -> infodesk_core::Result<T>) -> AppResult<T>`
  - `AppState::update_status(&self, f: impl FnOnce(&mut AppStatus))` (変更後に `status-changed` を通知)
  - `AppState::status(&self) -> AppStatus`
  - `AppState::llm_provider(&self, role: LlmRole) -> AppResult<Box<dyn LlmProvider>>` (ロック内で作り、ロックを外してから返す)
  - テスト用 (cfg(test)): `state::test_state() -> (AppState, Arc<Mutex<Vec<String>>>)` (メモリDB、seed 済み、通知を記録)
  - コマンド: `get_settings() -> HashMap<String, String>` (DEFAULT_SETTINGS の全キー)、`set_setting(key, value)`、`test_ollama() -> ProbeResult { ok: bool, message: String, millis: u64 }`、`test_claude() -> ProbeResult`、`get_status() -> AppStatus`
  - `lib.rs`: `run()` がDBを開き、seed し、`AppState` を manage し、コマンドを登録する。

- [ ] **Step 1: 失敗するテストを書く**

`app/src-tauri/src/state.rs`:

```rust
use std::sync::{Arc, Mutex};

use chrono::{DateTime, FixedOffset, Local, Utc};
use infodesk_core::llm::{provider_from_settings, LlmProvider, LlmRole};
use infodesk_core::Store;
use serde::Serialize;

use crate::error::{AppError, AppResult};

pub const EVENT_STATUS: &str = "status-changed";
pub const EVENT_ITEMS: &str = "items-changed";
pub const EVENT_DIGESTS: &str = "digests-changed";

#[derive(Debug, Clone, Default, Serialize)]
pub struct AppStatus {
    pub collecting: bool,
    pub summarizing: bool,
    pub digesting: bool,
    pub last_collect_at: Option<DateTime<Utc>>,
    pub next_collect_at: Option<DateTime<Utc>>,
    pub last_error: Option<String>,
}

pub type Notifier = Arc<dyn Fn(&str) + Send + Sync>;
pub type LlmFactory =
    Arc<dyn Fn(&Store, LlmRole) -> infodesk_core::Result<Box<dyn LlmProvider>> + Send + Sync>;

pub struct AppState {
    pub store: Mutex<Store>,
    pub http: reqwest::Client,
    pub status: Mutex<AppStatus>,
    pub tz: FixedOffset,
    pub notify: Notifier,
    pub llm: LlmFactory,
}

#[cfg(test)]
pub(crate) fn test_state() -> (AppState, Arc<Mutex<Vec<String>>>) {
    let events = Arc::new(Mutex::new(Vec::new()));
    let sink = events.clone();
    let store = Store::open_in_memory().unwrap();
    infodesk_core::seed::seed_defaults(&store).unwrap();
    let state = AppState::new(
        store,
        Arc::new(move |name: &str| sink.lock().unwrap().push(name.to_string())),
    );
    (state, events)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn with_store_maps_core_errors() {
        let (state, _) = test_state();
        let n = state.with_store(|s| Ok(s.list_sources()?.len())).unwrap();
        assert_eq!(n, 22);
        let err = state
            .with_store(|_| Err::<(), _>(infodesk_core::Error::Invalid("x".into())))
            .unwrap_err();
        assert_eq!(err.message, "invalid value: x");
    }

    #[test]
    fn update_status_notifies() {
        let (state, events) = test_state();
        state.update_status(|s| s.collecting = true);
        assert!(state.status().collecting);
        assert_eq!(events.lock().unwrap().as_slice(), [EVENT_STATUS]);
    }

    #[test]
    fn llm_provider_reads_settings() {
        let (state, _) = test_state();
        assert_eq!(state.llm_provider(LlmRole::Digest).unwrap().name(), "ollama:gemma4:26b");
        state
            .with_store(|s| s.set_setting("llm.digest_provider", "claude"))
            .unwrap();
        assert_eq!(state.llm_provider(LlmRole::Digest).unwrap().name(), "claude");
    }
}
```

`app/src-tauri/src/error.rs`:

```rust
use serde::Serialize;

/// 画面に返すエラー。Tauri は `Err` を JSON にして `invoke` の reject に渡す。
#[derive(Debug, Clone, Serialize)]
pub struct AppError {
    pub message: String,
}

pub type AppResult<T> = Result<T, AppError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_core_and_string_errors() {
        let e: AppError = infodesk_core::Error::Llm("down".into()).into();
        assert_eq!(e.message, "llm: down");
        let e: AppError = String::from("plain").into();
        assert_eq!(e.message, "plain");
        assert_eq!(serde_json::to_string(&e).unwrap(), r#"{"message":"plain"}"#);
    }
}
```

`app/src-tauri/src/commands/settings.rs`:

```rust
use std::collections::HashMap;
use std::time::Instant;

use infodesk_core::llm::claude::ClaudeProvider;
use infodesk_core::llm::LlmProvider;
use infodesk_core::seed::DEFAULT_SETTINGS;
use serde::Serialize;
use tauri::State;

use crate::error::AppResult;
use crate::state::AppState;

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ProbeResult {
    pub ok: bool,
    pub message: String,
    pub millis: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::test_state;

    #[test]
    fn settings_cover_every_default_key_and_reflect_updates() {
        let (state, _) = test_state();
        let all = settings_map(&state).unwrap();
        for (key, value) in DEFAULT_SETTINGS {
            assert_eq!(all.get(*key).map(String::as_str), Some(*value), "{key}");
        }
        set_setting_value(&state, "ollama.model", "m2").unwrap();
        assert_eq!(settings_map(&state).unwrap()["ollama.model"], "m2");
        assert!(set_setting_value(&state, "unknown.key", "x").is_err());
    }

    #[tokio::test]
    async fn ollama_probe_reports_unreachable() {
        let (state, _) = test_state();
        set_setting_value(&state, "ollama.base_url", "http://127.0.0.1:1").unwrap();
        let r = probe_ollama(&state).await;
        assert!(!r.ok);
        assert!(r.message.contains("127.0.0.1:1"), "{}", r.message);
    }

    #[tokio::test]
    async fn claude_probe_reports_missing_binary() {
        let (state, _) = test_state();
        set_setting_value(&state, "claude.bin", "/nonexistent/claude").unwrap();
        let r = probe_claude(&state).await;
        assert!(!r.ok);
    }
}
```

`app/src-tauri/src/commands/status.rs`:

```rust
use tauri::State;

use crate::state::{AppState, AppStatus};

#[tauri::command]
pub fn get_status(state: State<'_, AppState>) -> AppStatus {
    state.status()
}
```

`app/src-tauri/src/commands/mod.rs`:

```rust
pub mod settings;
pub mod status;
```

`app/src-tauri/src/lib.rs` を次に置き換える:

```rust
mod commands;
mod error;
mod state;

use std::sync::Arc;

use infodesk_core::Store;
use tauri::{Emitter, Manager};

use crate::state::AppState;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let db_path = app.path().app_data_dir()?.join("infodesk.db");
            let store = Store::open(&db_path)?;
            infodesk_core::seed::seed_defaults(&store)?;
            let handle = app.handle().clone();
            let notify: state::Notifier = Arc::new(move |name: &str| {
                let _ = handle.emit(name, ());
            });
            app.manage(AppState::new(store, notify));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::status::get_status,
            commands::settings::get_settings,
            commands::settings::set_setting,
            commands::settings::test_ollama,
            commands::settings::test_claude,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
```

- [ ] **Step 2: テストが失敗することを確認する**

Run: `cargo test -p infodesk-app 2>&1 | grep -E "^error" | head`
Expected: コンパイルエラー。`AppState::new`、`settings_map`、`probe_ollama` などが未定義。

- [ ] **Step 3: 実装する**

`app/src-tauri/src/error.rs` の `AppResult` の下、テストの上に追加:

```rust
impl From<infodesk_core::Error> for AppError {
    fn from(e: infodesk_core::Error) -> Self {
        Self { message: e.to_string() }
    }
}

impl From<String> for AppError {
    fn from(message: String) -> Self {
        Self { message }
    }
}

impl From<tauri::Error> for AppError {
    fn from(e: tauri::Error) -> Self {
        Self { message: e.to_string() }
    }
}

impl std::fmt::Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}
```

`app/src-tauri/src/state.rs` の `AppState` 構造体の下、`test_state` の上に追加:

```rust
impl AppState {
    pub fn new(store: Store, notify: Notifier) -> Self {
        Self {
            store: Mutex::new(store),
            http: infodesk_core::collector::http_client().expect("http client"),
            status: Mutex::new(AppStatus::default()),
            tz: *Local::now().offset(),
            notify,
            llm: Arc::new(|store, role| provider_from_settings(store, role)),
        }
    }

    /// DBをロックして同期処理を行う。ロックは関数の間だけ保持する。
    pub fn with_store<T>(
        &self,
        f: impl FnOnce(&Store) -> infodesk_core::Result<T>,
    ) -> AppResult<T> {
        let store = self
            .store
            .lock()
            .map_err(|_| AppError::from("database lock poisoned".to_string()))?;
        Ok(f(&store)?)
    }

    pub fn status(&self) -> AppStatus {
        self.status.lock().expect("status lock").clone()
    }

    pub fn update_status(&self, f: impl FnOnce(&mut AppStatus)) {
        {
            let mut status = self.status.lock().expect("status lock");
            f(&mut status);
        }
        (self.notify)(EVENT_STATUS);
    }

    pub fn notify(&self, event: &str) {
        (self.notify)(event);
    }

    /// 設定からプロバイダーを作る。戻り値はロックに依存しないので、`await` をまたいで使える。
    pub fn llm_provider(&self, role: LlmRole) -> AppResult<Box<dyn LlmProvider>> {
        let factory = self.llm.clone();
        self.with_store(|store| factory(store, role))
    }
}
```

`app/src-tauri/src/commands/settings.rs` の `ProbeResult` の下、テストの上に追加:

```rust
pub fn settings_map(state: &AppState) -> AppResult<HashMap<String, String>> {
    state.with_store(|store| {
        let mut map = HashMap::new();
        for (key, default) in DEFAULT_SETTINGS {
            map.insert(key.to_string(), store.get_setting_or(key, default)?);
        }
        Ok(map)
    })
}

pub fn set_setting_value(state: &AppState, key: &str, value: &str) -> AppResult<()> {
    if !DEFAULT_SETTINGS.iter().any(|(k, _)| *k == key) {
        return Err(format!("unknown setting '{key}'").into());
    }
    state.with_store(|store| store.set_setting(key, value))
}

fn probe(ok: bool, message: impl Into<String>, started: Instant) -> ProbeResult {
    ProbeResult {
        ok,
        message: message.into(),
        millis: started.elapsed().as_millis() as u64,
    }
}

/// `GET {base}/api/tags` が返れば接続済みとみなす。
pub async fn probe_ollama(state: &AppState) -> ProbeResult {
    let started = Instant::now();
    let base = match state.with_store(|s| s.get_setting_or("ollama.base_url", "http://localhost:11434")) {
        Ok(b) => b,
        Err(e) => return probe(false, e.message, started),
    };
    let url = format!("{}/api/tags", base.trim_end_matches('/'));
    match state.http.get(&url).send().await {
        Ok(resp) if resp.status().is_success() => probe(true, format!("接続済み ({url})"), started),
        Ok(resp) => probe(false, format!("{url}: HTTP {}", resp.status()), started),
        Err(e) => probe(false, format!("{url}: {e}"), started),
    }
}

/// `claude -p` に短い指示を送る。利用者のCLI認証をそのまま使う。
pub async fn probe_claude(state: &AppState) -> ProbeResult {
    let started = Instant::now();
    let bin = match state.with_store(|s| s.get_setting_or("claude.bin", "claude")) {
        Ok(b) => b,
        Err(e) => return probe(false, e.message, started),
    };
    let provider = ClaudeProvider::new(bin);
    match provider.complete("Reply with exactly: OK", "ping").await {
        Ok(_) => probe(true, "接続済み", started),
        Err(e) => probe(false, e.to_string(), started),
    }
}

#[tauri::command]
pub fn get_settings(state: State<'_, AppState>) -> AppResult<HashMap<String, String>> {
    settings_map(&state)
}

#[tauri::command]
pub fn set_setting(state: State<'_, AppState>, key: String, value: String) -> AppResult<()> {
    set_setting_value(&state, &key, &value)
}

#[tauri::command]
pub async fn test_ollama(state: State<'_, AppState>) -> AppResult<ProbeResult> {
    Ok(probe_ollama(&state).await)
}

#[tauri::command]
pub async fn test_claude(state: State<'_, AppState>) -> AppResult<ProbeResult> {
    Ok(probe_claude(&state).await)
}
```

- [ ] **Step 4: テストが通ることを確認する**

Run: `cargo test -p infodesk-app 2>&1 | grep -E "test result|error"`
Expected: 7 passed (state 3、error 1、settings 3)。`tokio::test` に `macros` 機能が必要。Task 1 で追加済み。

`State<'_, AppState>` を取る非同期コマンドは `Result` を返す必要がある (Tauri の制約)。`test_ollama` と `test_claude` はそのとおりにしてある。

Run: `cargo clippy --all-targets -- -D warnings 2>&1 | tail -1 && cargo fmt --check && echo FMT_OK`
Expected: clean。

- [ ] **Step 5: コミット**

```bash
git add app/src-tauri
git commit -m "feat(app): add app state, error type, settings and status commands"
```

---

### Task 4: 記事、ソース、トピックのコマンド

**Files:**
- Create: `app/src-tauri/src/commands/items.rs`, `app/src-tauri/src/commands/sources.rs`, `app/src-tauri/src/commands/topics.rs`
- Modify: `app/src-tauri/src/commands/mod.rs`, `app/src-tauri/src/lib.rs` (コマンド登録)

**Interfaces:**
- Consumes: `AppState::{with_store, notify}`, core の `Store` メソッド、`collector::feed::validate`、`tauri_plugin_opener::OpenerExt`
- Produces (全て `AppResult`):
  - `list_items(filter: ItemFilter) -> Vec<Item>`、`get_item(id) -> Option<Item>`、`set_read(id, read: bool)`、`set_starred(id, starred: bool)`、`mark_all_read(filter) -> usize`、`open_item(id)` (既読にして既定のブラウザで開く)
  - `sidebar_counts() -> SidebarCounts { total_unread: i64, categories: HashMap<String, i64>, topics: HashMap<i64, i64> }` (未読件数)
  - `list_sources() -> Vec<Source>`、`add_rss_source(url, name: Option<String>, category: String) -> Source` (検証してから追加)、`update_source(id, name, category, config)`、`set_source_enabled(id, enabled)`、`delete_source(id)`
  - `list_topics() -> Vec<Topic>`、`add_topic(name, keywords: Vec<String>) -> Topic`、`update_topic(id, name, keywords)`、`delete_topic(id)`
  - 内部関数 (テスト対象): `items::open_item_url(state, id) -> AppResult<String>`、`sources::add_rss(state, url, name, category) -> AppResult<Source>` 他、各コマンドと同名の `*_impl` ではなく、コマンド本体が `state: &AppState` を取る関数に委譲する形

- [ ] **Step 1: 失敗するテストを書く**

`app/src-tauri/src/commands/items.rs`:

```rust
use std::collections::HashMap;

use infodesk_core::model::{Category, Item, ItemFilter};
use serde::Serialize;
use tauri::{AppHandle, State};
use tauri_plugin_opener::OpenerExt;

use crate::error::AppResult;
use crate::state::{AppState, EVENT_ITEMS};

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct SidebarCounts {
    pub total_unread: i64,
    pub categories: HashMap<String, i64>,
    pub topics: HashMap<i64, i64>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::test_state;
    use infodesk_core::model::NewItem;

    fn seed_items(state: &AppState) -> Vec<i64> {
        state
            .with_store(|s| {
                let src = s.list_sources()?.into_iter().find(|x| x.category == Category::Medcare).unwrap();
                let now = chrono::Utc::now();
                let items = ["認知症の新薬", "介護報酬", "Rust 2.0"]
                    .iter()
                    .enumerate()
                    .map(|(i, t)| NewItem {
                        url: format!("https://e.com/{i}"),
                        title: t.to_string(),
                        excerpt: String::new(),
                        author: None,
                        published_at: None,
                        score: None,
                    })
                    .collect::<Vec<_>>();
                s.upsert_items(&src, &items, &now)?;
                Ok(s.query_items(&ItemFilter::default())?.into_iter().map(|i| i.id).collect())
            })
            .unwrap()
    }

    #[test]
    fn read_and_star_notify_items_changed() {
        let (state, events) = test_state();
        let ids = seed_items(&state);
        set_read_impl(&state, ids[0], true).unwrap();
        set_starred_impl(&state, ids[1], true).unwrap();
        let item = get_item_impl(&state, ids[0]).unwrap().unwrap();
        assert!(item.read);
        assert!(get_item_impl(&state, ids[1]).unwrap().unwrap().starred);
        assert_eq!(events.lock().unwrap().iter().filter(|e| *e == EVENT_ITEMS).count(), 2);
    }

    #[test]
    fn sidebar_counts_unread_per_category_and_topic() {
        let (state, _) = test_state();
        let ids = seed_items(&state);
        let topic = state.with_store(|s| s.add_topic("認知症", &["認知症".to_string()])).unwrap();
        set_read_impl(&state, ids[2], true).unwrap();
        let c = sidebar_counts_impl(&state).unwrap();
        assert_eq!(c.total_unread, 2);
        assert_eq!(c.categories["medcare"], 2);
        assert_eq!(c.categories["github"], 0);
        assert_eq!(c.topics[&topic], 1);
    }

    #[test]
    fn mark_all_read_uses_filter() {
        let (state, _) = test_state();
        seed_items(&state);
        let n = mark_all_read_impl(&state, ItemFilter { category: Some(Category::Medcare), ..Default::default() }).unwrap();
        assert_eq!(n, 3);
        assert_eq!(sidebar_counts_impl(&state).unwrap().total_unread, 0);
    }

    #[test]
    fn open_item_marks_read_and_returns_url() {
        let (state, _) = test_state();
        let ids = seed_items(&state);
        let url = open_item_url(&state, ids[0]).unwrap();
        assert_eq!(url, "https://e.com/0");
        assert!(get_item_impl(&state, ids[0]).unwrap().unwrap().read);
        assert!(open_item_url(&state, 9999).is_err());
    }
}
```

`app/src-tauri/src/commands/sources.rs`:

```rust
use infodesk_core::collector::feed;
use infodesk_core::model::{Category, Source, SourceKind};
use tauri::State;

use crate::error::AppResult;
use crate::state::AppState;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::test_state;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const RSS: &str = r#"<rss version="2.0"><channel><title>テスト</title><item><title>t</title><link>https://e.com/1</link></item></channel></rss>"#;

    #[tokio::test]
    async fn add_rss_validates_then_saves_with_feed_title_as_default_name() {
        let server = MockServer::start().await;
        Mock::given(method("GET")).and(path("/feed"))
            .respond_with(ResponseTemplate::new(200).set_body_string(RSS))
            .mount(&server).await;
        Mock::given(method("GET")).and(path("/html"))
            .respond_with(ResponseTemplate::new(200).set_body_string("<html></html>"))
            .mount(&server).await;
        let (state, _) = test_state();
        let before = list_sources_impl(&state).unwrap().len();

        let s = add_rss(&state, &format!("{}/feed", server.uri()), None, "tech_jp").await.unwrap();
        assert_eq!(s.name, "テスト");
        assert_eq!(s.category, Category::TechJp);
        assert_eq!(s.kind, SourceKind::Rss);
        assert_eq!(list_sources_impl(&state).unwrap().len(), before + 1);

        let named = add_rss(&state, &format!("{}/feed?x=1", server.uri()), Some("  名前  ".into()), "medcare").await.unwrap();
        assert_eq!(named.name, "名前");

        assert!(add_rss(&state, &format!("{}/html", server.uri()), None, "tech_jp").await.is_err());
        assert!(add_rss(&state, &format!("{}/feed", server.uri()), None, "nope").await.is_err());
        assert_eq!(list_sources_impl(&state).unwrap().len(), before + 2);
    }

    #[test]
    fn update_toggle_delete() {
        let (state, _) = test_state();
        let first = list_sources_impl(&state).unwrap().remove(0);
        update_source_impl(&state, first.id, "名前2", "tech_intl", &first.config).unwrap();
        set_source_enabled_impl(&state, first.id, false).unwrap();
        let s = list_sources_impl(&state).unwrap().remove(0);
        assert_eq!(s.name, "名前2");
        assert_eq!(s.category, Category::TechIntl);
        assert!(!s.enabled);
        delete_source_impl(&state, first.id).unwrap();
        assert!(list_sources_impl(&state).unwrap().iter().all(|x| x.id != first.id));
        assert!(update_source_impl(&state, first.id, "x", "bad", "c").is_err());
    }
}
```

`app/src-tauri/src/commands/topics.rs`:

```rust
use infodesk_core::model::Topic;
use tauri::State;

use crate::error::AppResult;
use crate::state::{AppState, EVENT_DIGESTS};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::test_state;

    #[test]
    fn topic_crud_notifies_digests_changed() {
        let (state, events) = test_state();
        let t = add_topic_impl(&state, "認知症", vec!["認知症".into(), "dementia".into()]).unwrap();
        assert_eq!(t.keywords.len(), 2);
        update_topic_impl(&state, t.id, "認知症ケア", vec!["介護".into()]).unwrap();
        let all = list_topics_impl(&state).unwrap();
        assert_eq!(all[0].name, "認知症ケア");
        delete_topic_impl(&state, t.id).unwrap();
        assert!(list_topics_impl(&state).unwrap().is_empty());
        assert_eq!(events.lock().unwrap().iter().filter(|e| *e == EVENT_DIGESTS).count(), 3);
        assert!(add_topic_impl(&state, "   ", vec![]).is_err());
    }
}
```

`app/src-tauri/Cargo.toml` に dev-dependency を追加: `cd app/src-tauri && cargo add --dev wiremock && cd ../..`。

`app/src-tauri/src/commands/mod.rs` を次にする:

```rust
pub mod items;
pub mod settings;
pub mod sources;
pub mod status;
pub mod topics;
```

- [ ] **Step 2: テストが失敗することを確認する**

Run: `cargo test -p infodesk-app 2>&1 | grep -E "^error" | sort | uniq -c | head`
Expected: `set_read_impl`、`add_rss`、`add_topic_impl` などが未定義。

- [ ] **Step 3: 実装する**

`app/src-tauri/src/commands/items.rs` の `SidebarCounts` の下、テストの上に追加:

```rust
pub fn list_items_impl(state: &AppState, filter: ItemFilter) -> AppResult<Vec<Item>> {
    state.with_store(|s| s.query_items(&filter))
}

pub fn get_item_impl(state: &AppState, id: i64) -> AppResult<Option<Item>> {
    state.with_store(|s| s.get_item(id))
}

pub fn set_read_impl(state: &AppState, id: i64, read: bool) -> AppResult<()> {
    state.with_store(|s| s.set_read(id, read))?;
    state.notify(EVENT_ITEMS);
    Ok(())
}

pub fn set_starred_impl(state: &AppState, id: i64, starred: bool) -> AppResult<()> {
    state.with_store(|s| s.set_starred(id, starred))?;
    state.notify(EVENT_ITEMS);
    Ok(())
}

pub fn mark_all_read_impl(state: &AppState, filter: ItemFilter) -> AppResult<usize> {
    let n = state.with_store(|s| s.mark_all_read(&filter))?;
    state.notify(EVENT_ITEMS);
    Ok(n)
}

/// 記事を既読にし、開くURLを返す。
pub fn open_item_url(state: &AppState, id: i64) -> AppResult<String> {
    let item = get_item_impl(state, id)?.ok_or_else(|| format!("item {id} not found"))?;
    set_read_impl(state, id, true)?;
    Ok(item.url)
}

pub fn sidebar_counts_impl(state: &AppState) -> AppResult<SidebarCounts> {
    state.with_store(|s| {
        let unread = ItemFilter { unread_only: true, ..Default::default() };
        let total_unread = s.count_items(&unread)?;
        let mut categories = HashMap::new();
        for c in Category::ALL {
            let f = ItemFilter { category: Some(c), ..unread.clone() };
            categories.insert(c.as_str().to_string(), s.count_items(&f)?);
        }
        let mut topics = HashMap::new();
        for t in s.list_topics()? {
            let f = ItemFilter { topic_id: Some(t.id), ..unread.clone() };
            topics.insert(t.id, s.count_items(&f)?);
        }
        Ok(SidebarCounts { total_unread, categories, topics })
    })
}

#[tauri::command]
pub fn list_items(state: State<'_, AppState>, filter: ItemFilter) -> AppResult<Vec<Item>> {
    list_items_impl(&state, filter)
}

#[tauri::command]
pub fn get_item(state: State<'_, AppState>, id: i64) -> AppResult<Option<Item>> {
    get_item_impl(&state, id)
}

#[tauri::command]
pub fn set_read(state: State<'_, AppState>, id: i64, read: bool) -> AppResult<()> {
    set_read_impl(&state, id, read)
}

#[tauri::command]
pub fn set_starred(state: State<'_, AppState>, id: i64, starred: bool) -> AppResult<()> {
    set_starred_impl(&state, id, starred)
}

#[tauri::command]
pub fn mark_all_read(state: State<'_, AppState>, filter: ItemFilter) -> AppResult<usize> {
    mark_all_read_impl(&state, filter)
}

#[tauri::command]
pub fn open_item(app: AppHandle, state: State<'_, AppState>, id: i64) -> AppResult<()> {
    let url = open_item_url(&state, id)?;
    app.opener()
        .open_url(url, None::<&str>)
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn sidebar_counts(state: State<'_, AppState>) -> AppResult<SidebarCounts> {
    sidebar_counts_impl(&state)
}
```

`app/src-tauri/src/commands/sources.rs` の `use` の下、テストの上に追加:

```rust
fn parse_category(s: &str) -> AppResult<Category> {
    Category::parse(s).ok_or_else(|| format!("unknown category '{s}'").into())
}

pub fn list_sources_impl(state: &AppState) -> AppResult<Vec<Source>> {
    state.with_store(|s| s.list_sources())
}

/// フィードを取得して検証してから保存する。解析できなければ保存しない。
pub async fn add_rss(
    state: &AppState,
    url: &str,
    name: Option<String>,
    category: &str,
) -> AppResult<Source> {
    let category = parse_category(category)?;
    let info = feed::validate(&state.http, url).await?;
    let name = name
        .map(|n| n.trim().to_string())
        .filter(|n| !n.is_empty())
        .unwrap_or(info.title);
    state.with_store(|s| {
        let id = s.add_source(SourceKind::Rss, &name, category, url)?;
        Ok(s.get_source(id)?.expect("just inserted"))
    })
}

pub fn update_source_impl(
    state: &AppState,
    id: i64,
    name: &str,
    category: &str,
    config: &str,
) -> AppResult<()> {
    let category = parse_category(category)?;
    state.with_store(|s| s.update_source(id, name, category, config))
}

pub fn set_source_enabled_impl(state: &AppState, id: i64, enabled: bool) -> AppResult<()> {
    state.with_store(|s| s.set_source_enabled(id, enabled))
}

pub fn delete_source_impl(state: &AppState, id: i64) -> AppResult<()> {
    state.with_store(|s| s.delete_source(id))
}

#[tauri::command]
pub fn list_sources(state: State<'_, AppState>) -> AppResult<Vec<Source>> {
    list_sources_impl(&state)
}

#[tauri::command]
pub async fn add_rss_source(
    state: State<'_, AppState>,
    url: String,
    name: Option<String>,
    category: String,
) -> AppResult<Source> {
    add_rss(&state, &url, name, &category).await
}

#[tauri::command]
pub fn update_source(
    state: State<'_, AppState>,
    id: i64,
    name: String,
    category: String,
    config: String,
) -> AppResult<()> {
    update_source_impl(&state, id, &name, &category, &config)
}

#[tauri::command]
pub fn set_source_enabled(state: State<'_, AppState>, id: i64, enabled: bool) -> AppResult<()> {
    set_source_enabled_impl(&state, id, enabled)
}

#[tauri::command]
pub fn delete_source(state: State<'_, AppState>, id: i64) -> AppResult<()> {
    delete_source_impl(&state, id)
}
```

`app/src-tauri/src/commands/topics.rs` の `use` の下、テストの上に追加:

```rust
pub fn list_topics_impl(state: &AppState) -> AppResult<Vec<Topic>> {
    state.with_store(|s| s.list_topics())
}

pub fn add_topic_impl(state: &AppState, name: &str, keywords: Vec<String>) -> AppResult<Topic> {
    if name.trim().is_empty() {
        return Err("topic name is empty".to_string().into());
    }
    let topic = state.with_store(|s| {
        let id = s.add_topic(name, &keywords)?;
        Ok(s.get_topic(id)?.expect("just inserted"))
    })?;
    state.notify(EVENT_DIGESTS);
    Ok(topic)
}

pub fn update_topic_impl(state: &AppState, id: i64, name: &str, keywords: Vec<String>) -> AppResult<()> {
    if name.trim().is_empty() {
        return Err("topic name is empty".to_string().into());
    }
    state.with_store(|s| s.update_topic(id, name, &keywords))?;
    state.notify(EVENT_DIGESTS);
    Ok(())
}

pub fn delete_topic_impl(state: &AppState, id: i64) -> AppResult<()> {
    state.with_store(|s| s.delete_topic(id))?;
    state.notify(EVENT_DIGESTS);
    Ok(())
}

#[tauri::command]
pub fn list_topics(state: State<'_, AppState>) -> AppResult<Vec<Topic>> {
    list_topics_impl(&state)
}

#[tauri::command]
pub fn add_topic(state: State<'_, AppState>, name: String, keywords: Vec<String>) -> AppResult<Topic> {
    add_topic_impl(&state, &name, keywords)
}

#[tauri::command]
pub fn update_topic(state: State<'_, AppState>, id: i64, name: String, keywords: Vec<String>) -> AppResult<()> {
    update_topic_impl(&state, id, &name, keywords)
}

#[tauri::command]
pub fn delete_topic(state: State<'_, AppState>, id: i64) -> AppResult<()> {
    delete_topic_impl(&state, id)
}
```

`app/src-tauri/src/lib.rs` の `generate_handler!` に追加:

```rust
            commands::items::list_items,
            commands::items::get_item,
            commands::items::set_read,
            commands::items::set_starred,
            commands::items::mark_all_read,
            commands::items::open_item,
            commands::items::sidebar_counts,
            commands::sources::list_sources,
            commands::sources::add_rss_source,
            commands::sources::update_source,
            commands::sources::set_source_enabled,
            commands::sources::delete_source,
            commands::topics::list_topics,
            commands::topics::add_topic,
            commands::topics::update_topic,
            commands::topics::delete_topic,
```

- [ ] **Step 4: テストが通ることを確認する**

Run: `cargo test -p infodesk-app 2>&1 | grep -E "test result|error|panicked"`
Expected: 14 passed。

`ItemFilter` はコマンド引数として JSON から来る。core の `ItemFilter` は `Deserialize` を derive 済みで、`category` は `"medcare"` 形式、`since`/`until` は RFC3339 文字列、省略したフィールドは既定値になる (`#[serde(default)]` がないため、Plan 2b の画面は全フィールドを送る。この点は Plan 2b の `api.ts` に書く)。

Run: `cargo clippy --all-targets -- -D warnings 2>&1 | tail -1 && cargo fmt --check && echo FMT_OK`
Expected: clean。

- [ ] **Step 5: コミット**

```bash
git add app/src-tauri Cargo.lock
git commit -m "feat(app): add item, source, topic commands"
```

---

### Task 5: 収集と要約のジョブ、ダイジェストのコマンド

**Files:**
- Create: `app/src-tauri/src/jobs.rs`, `app/src-tauri/src/commands/digests.rs`
- Modify: `app/src-tauri/src/commands/mod.rs`, `app/src-tauri/src/lib.rs`

**Interfaces:**
- Consumes: `AppState`、core の `collector::{fetch_all, apply_outcomes}`、`summarizer::{plan_item_summaries, finish_item_summary, plan_digest, finish_digest, DigestJob, DigestOutcome}`、`period::{period_start, utc_range}`、`Store::pending_digests`
- Produces:
  - `jobs::run_collect(state: &AppState) -> AppResult<CollectSummary { new_items: usize, failed_sources: Vec<String> }>` (収集中なら `Err("collect already running")`。終了後に記事要約と当日の日次要約を force で行う)
  - `jobs::run_item_summaries(state, since: DateTime<Utc>) -> AppResult<ItemSummaryTotals { ok: usize, failed: usize, error: Option<String> }>`
  - `jobs::run_digests(state, period, date, scopes: &[DigestScope], force) -> AppResult<Vec<(DigestScope, Option<DigestOutcome>)>>`
  - `jobs::retry_pending(state) -> AppResult<usize>` (再試行した件数)
  - `jobs::all_category_scopes() -> Vec<DigestScope>`
  - コマンド: `get_digests(period: String, date: String, topic_id: Option<i64>) -> Vec<DigestView { scope: DigestScope, label: String, digest: Option<Digest> }>`、`regenerate_digest(period, date, scope: DigestScope)` (非同期ジョブを起動して即返す)、`collect_now()` (同上)、`retry_pending_digests()` (同上)
  - 状態: ジョブ中は `AppStatus.collecting / summarizing / digesting` が真になり、失敗は `last_error` に残る。イベント `items-changed`、`digests-changed` を発火する。

- [ ] **Step 1: 失敗するテストを書く**

`app/src-tauri/src/jobs.rs`:

```rust
use chrono::{DateTime, Local, Utc};
use infodesk_core::collector::{apply_outcomes, fetch_all};
use infodesk_core::llm::LlmRole;
use infodesk_core::model::{Category, DigestScope, Period};
use infodesk_core::period::{period_start, utc_range};
use infodesk_core::summarizer::{
    finish_digest, finish_item_summary, plan_digest, plan_item_summaries, DigestOutcome,
};
use serde::Serialize;

use crate::error::AppResult;
use crate::state::{AppState, EVENT_DIGESTS, EVENT_ITEMS};

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct CollectSummary {
    pub new_items: usize,
    pub failed_sources: Vec<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ItemSummaryTotals {
    pub ok: usize,
    pub failed: usize,
    pub error: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::test_state;
    use async_trait::async_trait;
    use infodesk_core::llm::LlmProvider;
    use infodesk_core::model::{ItemFilter, NewItem, SourceKind};
    use std::sync::{Arc, Mutex};
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    struct Mock1(Result<String, String>);

    #[async_trait]
    impl LlmProvider for Mock1 {
        fn name(&self) -> String {
            "mock".into()
        }
        async fn complete(&self, _s: &str, _p: &str) -> infodesk_core::Result<String> {
            self.0.clone().map_err(infodesk_core::Error::Llm)
        }
    }

    fn use_mock(state: &mut AppState, reply: Result<&str, &str>) {
        let reply = reply.map(String::from).map_err(String::from);
        state.llm = Arc::new(move |_, _| Ok(Box::new(Mock1(reply.clone())) as Box<dyn LlmProvider>));
    }

    /// 全ソースを無効にし、wiremock のRSS 1件だけ有効にする。
    async fn one_rss_source(state: &AppState, server: &MockServer, status: u16) -> i64 {
        Mock::given(method("GET")).and(path("/feed"))
            .respond_with(ResponseTemplate::new(status).set_body_string(
                r#"<rss version="2.0"><channel><title>T</title><item><title>認知症の記事</title><link>https://e.com/1</link><description>本文</description></item></channel></rss>"#,
            ))
            .mount(server).await;
        state
            .with_store(|s| {
                for src in s.list_sources()? {
                    s.set_source_enabled(src.id, false)?;
                }
                s.add_source(SourceKind::Rss, "Mock", Category::Medcare, &format!("{}/feed", server.uri()))
            })
            .unwrap()
    }

    #[tokio::test]
    async fn collect_fetches_summarizes_and_builds_today_digest() {
        let server = MockServer::start().await;
        let (mut state, events) = test_state();
        one_rss_source(&state, &server, 200).await;
        use_mock(&mut state, Ok("日本語の要約。"));

        let summary = run_collect(&state).await.unwrap();
        assert_eq!(summary.new_items, 1);
        assert!(summary.failed_sources.is_empty());

        let items = state.with_store(|s| s.query_items(&ItemFilter::default())).unwrap();
        assert_eq!(items[0].ai_summary.as_deref(), Some("日本語の要約。"));
        let today = Local::now().date_naive();
        let digest = state
            .with_store(|s| s.get_digest(Period::Day, today, &DigestScope::Category(Category::Medcare)))
            .unwrap()
            .unwrap();
        assert_eq!(digest.body, "日本語の要約。");
        let st = state.status();
        assert!(!st.collecting && !st.summarizing && !st.digesting);
        assert!(st.last_collect_at.is_some());
        assert_eq!(st.last_error, None);
        let ev = events.lock().unwrap();
        assert!(ev.contains(&EVENT_ITEMS.to_string()));
        assert!(ev.contains(&EVENT_DIGESTS.to_string()));
    }

    #[tokio::test]
    async fn collect_with_llm_down_leaves_pending_and_records_error() {
        // Review Focus 2
        let server = MockServer::start().await;
        let (mut state, _) = test_state();
        one_rss_source(&state, &server, 200).await;
        use_mock(&mut state, Err("connection refused"));

        let summary = run_collect(&state).await.unwrap();
        assert_eq!(summary.new_items, 1);
        let items = state.with_store(|s| s.query_items(&ItemFilter::default())).unwrap();
        assert_eq!(items[0].ai_summary, None);
        let pending = state.with_store(|s| s.pending_digests()).unwrap();
        assert_eq!(pending.len(), 1);
        assert!(pending[0].last_error.as_deref().unwrap().contains("connection refused"));
        assert!(state.status().last_error.unwrap().contains("connection refused"));
    }

    #[tokio::test]
    async fn collect_records_failed_sources_without_stopping() {
        let server = MockServer::start().await;
        let (mut state, _) = test_state();
        one_rss_source(&state, &server, 500).await;
        use_mock(&mut state, Ok("x"));
        let summary = run_collect(&state).await.unwrap();
        assert_eq!(summary.new_items, 0);
        assert_eq!(summary.failed_sources, vec!["Mock".to_string()]);
    }

    #[tokio::test]
    async fn second_collect_while_running_is_rejected() {
        // Review Focus 1
        let (state, _) = test_state();
        state.update_status(|s| s.collecting = true);
        let err = run_collect(&state).await.unwrap_err();
        assert!(err.message.contains("already running"), "{}", err.message);
    }

    #[tokio::test]
    async fn retry_pending_regenerates_when_llm_is_back() {
        let server = MockServer::start().await;
        let (mut state, _) = test_state();
        one_rss_source(&state, &server, 200).await;
        use_mock(&mut state, Err("down"));
        run_collect(&state).await.unwrap();
        assert_eq!(state.with_store(|s| s.pending_digests()).unwrap().len(), 1);

        use_mock(&mut state, Ok("復旧"));
        assert_eq!(retry_pending(&state).await.unwrap(), 1);
        assert!(state.with_store(|s| s.pending_digests()).unwrap().is_empty());
        assert_eq!(state.status().last_error, None);
    }

    #[tokio::test]
    async fn run_digests_returns_outcome_per_scope() {
        let server = MockServer::start().await;
        let (mut state, _) = test_state();
        one_rss_source(&state, &server, 200).await;
        use_mock(&mut state, Ok("要約"));
        run_collect(&state).await.unwrap();
        let today = Local::now().date_naive();
        let results = run_digests(&state, Period::Day, today, &all_category_scopes(), false).await.unwrap();
        assert_eq!(results.len(), 7);
        // medcare は収集時に生成済みなので None (再生成しない)。他は記事なしで None。
        assert!(results.iter().all(|(_, o)| o.is_none()));
        let results = run_digests(&state, Period::Day, today, &[DigestScope::Category(Category::Medcare)], true).await.unwrap();
        assert_eq!(results[0].1.as_ref().unwrap().error, None);
    }
}
```

`app/src-tauri/Cargo.toml` に `async-trait` を dev-dependency で追加: `cd app/src-tauri && cargo add --dev async-trait && cd ../..`。

`app/src-tauri/src/commands/digests.rs`:

```rust
use chrono::NaiveDate;
use infodesk_core::model::{Category, Digest, DigestScope, Period};
use infodesk_core::period::period_start;
use serde::Serialize;
use tauri::{AppHandle, Manager, State};

use crate::error::AppResult;
use crate::jobs;
use crate::state::AppState;

#[derive(Debug, Clone, Serialize)]
pub struct DigestView {
    pub scope: DigestScope,
    pub label: String,
    pub digest: Option<Digest>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::test_state;

    #[test]
    fn get_digests_lists_categories_or_one_topic() {
        let (state, _) = test_state();
        let views = digests_for(&state, "week", "2026-10-06", None).unwrap();
        assert_eq!(views.len(), 7);
        assert_eq!(views[0].label, "GitHub");
        assert!(views.iter().all(|v| v.digest.is_none()));

        let topic = state.with_store(|s| s.add_topic("介護", &["介護".to_string()])).unwrap();
        let views = digests_for(&state, "day", "2026-10-06", Some(topic)).unwrap();
        assert_eq!(views.len(), 1);
        assert_eq!(views[0].label, "介護");
        assert_eq!(views[0].scope, DigestScope::Topic(topic));

        assert!(digests_for(&state, "year", "2026-10-06", None).is_err());
        assert!(digests_for(&state, "day", "not a date", None).is_err());
        assert!(digests_for(&state, "day", "2026-10-06", Some(999)).is_err());
    }
}
```

`app/src-tauri/src/commands/mod.rs` に `pub mod digests;` を追加。`app/src-tauri/src/lib.rs` の先頭に `mod jobs;` を追加。

- [ ] **Step 2: テストが失敗することを確認する**

Run: `cargo test -p infodesk-app 2>&1 | grep -E "^error" | sort | uniq -c | head`
Expected: `run_collect`、`retry_pending`、`run_digests`、`all_category_scopes`、`digests_for` が未定義。

- [ ] **Step 3: 実装する**

`app/src-tauri/src/jobs.rs` の構造体の下、テストの上に追加:

```rust
pub fn all_category_scopes() -> Vec<DigestScope> {
    Category::ALL.into_iter().map(DigestScope::Category).collect()
}

fn now_utc() -> DateTime<Utc> {
    Utc::now()
}

/// ロックを短く2回取り、間で `await` する。
/// 1. 計画 (ロック内) 2. LLM (ロック外) 3. 保存 (ロック内)
pub async fn run_item_summaries(
    state: &AppState,
    since: DateTime<Utc>,
) -> AppResult<ItemSummaryTotals> {
    let limit: u32 = state
        .with_store(|s| s.get_setting_or("summary.top_n", "20"))?
        .parse()
        .unwrap_or(20);
    let llm = state.llm_provider(LlmRole::Items)?;
    let mut totals = ItemSummaryTotals { ok: 0, failed: 0, error: None };
    state.update_status(|s| s.summarizing = true);
    'categories: for category in Category::ALL {
        let jobs = state.with_store(|s| plan_item_summaries(s, category, &since, limit))?;
        for job in jobs {
            let output = llm.complete(&job.system, &job.prompt).await;
            match state.with_store(|s| finish_item_summary(s, job.item_id, output))? {
                Ok(()) => totals.ok += 1,
                Err(message) => {
                    totals.failed += 1;
                    totals.error.get_or_insert(message);
                    break 'categories;
                }
            }
        }
    }
    state.update_status(|s| s.summarizing = false);
    if totals.ok > 0 {
        state.notify(EVENT_ITEMS);
    }
    Ok(totals)
}

/// 期間要約を順に生成する。各 scope の結果 (`None` は生成不要) を返す。
pub async fn run_digests(
    state: &AppState,
    period: Period,
    date: chrono::NaiveDate,
    scopes: &[DigestScope],
    force: bool,
) -> AppResult<Vec<(DigestScope, Option<DigestOutcome>)>> {
    let llm = state.llm_provider(LlmRole::Digest)?;
    let tz = state.tz;
    state.update_status(|s| s.digesting = true);
    let mut results = Vec::new();
    let mut first_error: Option<String> = None;
    for scope in scopes {
        let job = state.with_store(|s| plan_digest(s, period, date, scope, tz, force))?;
        let outcome = match job {
            None => None,
            Some(job) => {
                let output = llm.complete(&job.system, &job.prompt).await;
                let outcome =
                    state.with_store(|s| finish_digest(s, &job, &llm.name(), output, &now_utc()))?;
                if let Some(e) = &outcome.error {
                    first_error.get_or_insert(e.clone());
                }
                Some(outcome)
            }
        };
        results.push((scope.clone(), outcome));
    }
    state.update_status(|s| {
        s.digesting = false;
        if first_error.is_some() {
            s.last_error = first_error.clone();
        }
    });
    state.notify(EVENT_DIGESTS);
    Ok(results)
}

/// 保留中の要約を再試行する。成功した件数を返す。
pub async fn retry_pending(state: &AppState) -> AppResult<usize> {
    let pending = state.with_store(|s| s.pending_digests())?;
    let mut recovered = 0;
    for d in pending {
        let results = run_digests(state, d.period, d.period_start, &[d.scope.clone()], false).await?;
        if let Some((_, Some(outcome))) = results.first()
            && outcome.error.is_none()
        {
            recovered += 1;
        }
    }
    if recovered > 0 {
        state.update_status(|s| s.last_error = None);
    }
    Ok(recovered)
}

/// 収集、記事要約、当日の日次要約。収集中なら拒否する。
pub async fn run_collect(state: &AppState) -> AppResult<CollectSummary> {
    {
        let mut status = state.status.lock().expect("status lock");
        if status.collecting {
            return Err("collect already running".to_string().into());
        }
        status.collecting = true;
    }
    state.notify(crate::state::EVENT_STATUS);

    let result = collect_inner(state).await;
    state.update_status(|s| {
        s.collecting = false;
        if let Err(e) = &result {
            s.last_error = Some(e.message.clone());
        }
    });
    result
}

async fn collect_inner(state: &AppState) -> AppResult<CollectSummary> {
    let sources = state.with_store(|s| s.list_sources())?;
    let outcomes = fetch_all(&state.http, &sources).await;
    let now = now_utc();
    let reports = state.with_store(|s| apply_outcomes(s, &outcomes, &now))?;
    let summary = CollectSummary {
        new_items: reports.iter().map(|r| r.new_items).sum(),
        failed_sources: reports.iter().filter(|r| r.error.is_some()).map(|r| r.name.clone()).collect(),
    };
    state.with_store(|s| s.set_setting("schedule.last_collect_at", &now.to_rfc3339()))?;
    state.update_status(|s| {
        s.last_collect_at = Some(now);
        s.last_error = None;
    });
    state.notify(EVENT_ITEMS);

    let today = Local::now().date_naive();
    let (since, _) = utc_range(Period::Day, period_start(Period::Day, today), state.tz);
    let totals = run_item_summaries(state, since).await?;
    if let Some(e) = &totals.error {
        state.update_status(|s| s.last_error = Some(e.clone()));
    }
    run_digests(state, Period::Day, today, &all_category_scopes(), true).await?;
    Ok(summary)
}
```

`app/src-tauri/src/commands/digests.rs` の `DigestView` の下、テストの上に追加:

```rust
fn parse_period(s: &str) -> AppResult<Period> {
    Period::parse(s).ok_or_else(|| format!("unknown period '{s}'").into())
}

fn parse_date(s: &str) -> AppResult<NaiveDate> {
    NaiveDate::parse_from_str(s, "%Y-%m-%d").map_err(|_| format!("invalid date '{s}'").into())
}

/// カテゴリー別 (topic_id なし) またはトピック別の要約を、生成済みかどうかに関わらず並べる。
pub fn digests_for(
    state: &AppState,
    period: &str,
    date: &str,
    topic_id: Option<i64>,
) -> AppResult<Vec<DigestView>> {
    let period = parse_period(period)?;
    let start = period_start(period, parse_date(date)?);
    let scopes: Vec<(DigestScope, String)> = match topic_id {
        Some(id) => {
            let topic = state
                .with_store(|s| s.get_topic(id))?
                .ok_or_else(|| format!("topic {id} not found"))?;
            vec![(DigestScope::Topic(id), topic.name)]
        }
        None => Category::ALL
            .into_iter()
            .map(|c| (DigestScope::Category(c), c.label_ja().to_string()))
            .collect(),
    };
    state.with_store(|s| {
        scopes
            .into_iter()
            .map(|(scope, label)| {
                Ok(DigestView { digest: s.get_digest(period, start, &scope)?, scope, label })
            })
            .collect()
    })
}

#[tauri::command]
pub fn get_digests(
    state: State<'_, AppState>,
    period: String,
    date: String,
    topic_id: Option<i64>,
) -> AppResult<Vec<DigestView>> {
    digests_for(&state, &period, &date, topic_id)
}

/// ジョブを起動して即座に返す。進行はイベントで通知される。
#[tauri::command]
pub fn regenerate_digest(app: AppHandle, period: String, date: String, scope: DigestScope) -> AppResult<()> {
    let period = parse_period(&period)?;
    let date = parse_date(&date)?;
    tauri::async_runtime::spawn(async move {
        let state = app.state::<AppState>();
        if let Err(e) = jobs::run_digests(&state, period, date, &[scope], true).await {
            state.update_status(|s| s.last_error = Some(e.message));
        }
    });
    Ok(())
}

#[tauri::command]
pub fn collect_now(app: AppHandle) -> AppResult<()> {
    tauri::async_runtime::spawn(async move {
        let state = app.state::<AppState>();
        if let Err(e) = jobs::run_collect(&state).await {
            state.update_status(|s| s.last_error = Some(e.message));
        }
    });
    Ok(())
}

#[tauri::command]
pub fn retry_pending_digests(app: AppHandle) -> AppResult<()> {
    tauri::async_runtime::spawn(async move {
        let state = app.state::<AppState>();
        if let Err(e) = jobs::retry_pending(&state).await {
            state.update_status(|s| s.last_error = Some(e.message));
        }
    });
    Ok(())
}
```

`app/src-tauri/src/lib.rs` の `generate_handler!` に追加:

```rust
            commands::digests::get_digests,
            commands::digests::regenerate_digest,
            commands::digests::collect_now,
            commands::digests::retry_pending_digests,
```

- [ ] **Step 4: テストが通ることを確認する**

Run: `cargo test -p infodesk-app 2>&1 | grep -E "test result|error|panicked"`
Expected: 21 passed。

`tauri::async_runtime::spawn` に渡す Future は `Send` でなければならない。`run_collect` などが `&AppState` を持ちながら `await` するため、`AppState` は `Sync` が必要である。`Mutex<Store>` (`Store: Send`)、`reqwest::Client`、`Arc<dyn Fn + Send + Sync>` で満たされる。`MutexGuard` は `with_store` の中だけで保持される。コンパイルが通れば満たされている。

Run: `cargo clippy --all-targets -- -D warnings 2>&1 | tail -1 && cargo fmt --check && echo FMT_OK`
Expected: clean。

- [ ] **Step 5: コミット**

```bash
git add app/src-tauri Cargo.lock
git commit -m "feat(app): add collection and digest jobs with commands"
```

---

### Task 6: スケジューラー

**Files:**
- Create: `app/src-tauri/src/scheduler.rs`
- Modify: `app/src-tauri/src/lib.rs`, `docs/superpowers/specs/2026-10-06-infodesk-design.md` (6節に実行タイミングを追記)

**Interfaces:**
- Consumes: `jobs::{run_collect, run_digests, retry_pending, all_category_scopes}`、`AppState`、設定キー `schedule.*`、`schedule.last_collect_at`、`schedule.last_daily_for`、`schedule.last_weekly_for`、`schedule.last_monthly_for`
- Produces:
  - `scheduler::ScheduleConfig { collect_interval: Duration, daily_time: NaiveTime, weekly_day: Weekday, weekly_time: NaiveTime, monthly_time: NaiveTime }` と `ScheduleConfig::from_settings(get: impl Fn(&str) -> Option<String>) -> ScheduleConfig` (壊れた値は既定値)
  - `scheduler::LastRuns { collect_at: Option<DateTime<Utc>>, daily_for: Option<NaiveDate>, weekly_for: Option<NaiveDate>, monthly_for: Option<NaiveDate> }`
  - `scheduler::Job { Collect, Daily(NaiveDate), Weekly(NaiveDate), Monthly(NaiveDate) }` (日付は対象期間の開始日)
  - `scheduler::due_jobs(now: DateTime<FixedOffset>, cfg: &ScheduleConfig, last: &LastRuns) -> Vec<Job>` (純粋関数)
  - `scheduler::next_collect_at(now, cfg, last) -> DateTime<Utc>`
  - `scheduler::run_loop(app: AppHandle)` (60秒ごとに `due_jobs` を評価して実行。起動直後にも1回評価する)

- [ ] **Step 1: 失敗するテストを書く**

`app/src-tauri/src/scheduler.rs`:

```rust
use std::time::Duration as StdDuration;

use chrono::{DateTime, Datelike, Duration, FixedOffset, NaiveDate, NaiveTime, Utc, Weekday};
use infodesk_core::model::Period;
use infodesk_core::period::period_start;
use tauri::{AppHandle, Manager};

use crate::jobs;
use crate::state::AppState;

#[derive(Debug, Clone, PartialEq)]
pub struct ScheduleConfig {
    pub collect_interval: Duration,
    pub daily_time: NaiveTime,
    pub weekly_day: Weekday,
    pub weekly_time: NaiveTime,
    pub monthly_time: NaiveTime,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct LastRuns {
    pub collect_at: Option<DateTime<Utc>>,
    pub daily_for: Option<NaiveDate>,
    pub weekly_for: Option<NaiveDate>,
    pub monthly_for: Option<NaiveDate>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Job {
    Collect,
    /// 対象日 (前日)。
    Daily(NaiveDate),
    /// 対象週の月曜 (前週)。
    Weekly(NaiveDate),
    /// 対象月の1日 (前月)。
    Monthly(NaiveDate),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn jst(s: &str) -> DateTime<FixedOffset> {
        DateTime::parse_from_rfc3339(s).unwrap()
    }

    fn d(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    fn cfg() -> ScheduleConfig {
        ScheduleConfig::from_settings(|_| None)
    }

    #[test]
    fn config_defaults_and_tolerates_garbage() {
        // Review Focus 4
        let c = cfg();
        assert_eq!(c.collect_interval, Duration::hours(3));
        assert_eq!(c.daily_time, NaiveTime::from_hms_opt(7, 0, 0).unwrap());
        assert_eq!(c.weekly_day, Weekday::Mon);
        assert_eq!(c.weekly_time, NaiveTime::from_hms_opt(7, 30, 0).unwrap());
        assert_eq!(c.monthly_time, NaiveTime::from_hms_opt(8, 0, 0).unwrap());

        let c = ScheduleConfig::from_settings(|k| {
            Some(match k {
                "schedule.collect_interval_hours" => "abc",
                "schedule.daily_time" => "25:99",
                "schedule.weekly_day" => "funday",
                "schedule.weekly_time" => "",
                "schedule.monthly_time" => "09:15",
                _ => return None,
            }
            .to_string())
        });
        assert_eq!(c.collect_interval, Duration::hours(3));
        assert_eq!(c.daily_time, NaiveTime::from_hms_opt(7, 0, 0).unwrap());
        assert_eq!(c.weekly_day, Weekday::Mon);
        assert_eq!(c.monthly_time, NaiveTime::from_hms_opt(9, 15, 0).unwrap());

        let c = ScheduleConfig::from_settings(|k| match k {
            "schedule.collect_interval_hours" => Some("0".into()),
            "schedule.weekly_day" => Some("sun".into()),
            _ => None,
        });
        assert_eq!(c.collect_interval, Duration::hours(1), "0時間は1時間に切り上げる");
        assert_eq!(c.weekly_day, Weekday::Sun);
    }

    #[test]
    fn first_start_collects_and_backfills_previous_periods() {
        // 2026-10-06 (火) 06:00 JST。日次の7:00より前なので、日次の対象は一昨日 (10-04)。
        // 週次と月次は前期間を補う。記事がなければ生成はされない (plan_digest が None)。
        let now = jst("2026-10-06T06:00:00+09:00");
        let jobs = due_jobs(now, &cfg(), &LastRuns::default());
        assert_eq!(
            jobs,
            vec![Job::Collect, Job::Daily(d("2026-10-04")), Job::Weekly(d("2026-09-28")), Job::Monthly(d("2026-09-01"))]
        );
    }

    #[test]
    fn collect_is_due_after_interval_only() {
        let now = jst("2026-10-06T06:00:00+09:00");
        let last = LastRuns { collect_at: Some(now.with_timezone(&Utc) - Duration::hours(2)), ..Default::default() };
        assert!(!due_jobs(now, &cfg(), &last).contains(&Job::Collect));
        let last = LastRuns { collect_at: Some(now.with_timezone(&Utc) - Duration::hours(3)), ..Default::default() };
        assert!(due_jobs(now, &cfg(), &last).contains(&Job::Collect));
        assert_eq!(
            next_collect_at(now, &cfg(), &last),
            now.with_timezone(&Utc)
        );
    }

    #[test]
    fn daily_runs_once_after_its_time_even_if_late() {
        // Review Focus 3: 7:00 を過ぎてスリープから復帰 (13:42)。前日分を1回だけ。
        let now = jst("2026-10-06T13:42:00+09:00");
        let last = LastRuns {
            collect_at: Some(now.with_timezone(&Utc)),
            weekly_for: Some(d("2026-09-28")),
            monthly_for: Some(d("2026-09-01")),
            ..Default::default()
        };
        let jobs = due_jobs(now, &cfg(), &last);
        assert_eq!(jobs, vec![Job::Daily(d("2026-10-05"))]);

        let done = LastRuns { daily_for: Some(d("2026-10-05")), ..last.clone() };
        assert!(due_jobs(now, &cfg(), &done).is_empty());
        // 翌日の 7:00 前はまだ出ない。
        let tomorrow_early = jst("2026-10-07T06:59:00+09:00");
        assert!(due_jobs(tomorrow_early, &cfg(), &done).is_empty());
        let tomorrow = jst("2026-10-07T07:00:00+09:00");
        assert_eq!(due_jobs(tomorrow, &cfg(), &done), vec![Job::Daily(d("2026-10-06"))]);
    }

    #[test]
    fn weekly_and_monthly_target_previous_periods() {
        // 2026-10-05 (月) 09:00: 前週 (9/28〜) の週次。10/1 以降なので前月 (9月) の月次も。
        let now = jst("2026-10-05T09:00:00+09:00");
        let last = LastRuns {
            collect_at: Some(now.with_timezone(&Utc)),
            daily_for: Some(d("2026-10-04")),
            ..Default::default()
        };
        let jobs = due_jobs(now, &cfg(), &last);
        assert_eq!(jobs, vec![Job::Weekly(d("2026-09-28")), Job::Monthly(d("2026-09-01"))]);

        // 火曜日は週次の対象外だが、未実行なら補う (前週分)。
        let tue = jst("2026-10-06T09:00:00+09:00");
        let last = LastRuns { daily_for: Some(d("2026-10-05")), monthly_for: Some(d("2026-09-01")), collect_at: Some(tue.with_timezone(&Utc)), ..Default::default() };
        assert_eq!(due_jobs(tue, &cfg(), &last), vec![Job::Weekly(d("2026-09-28"))]);

        // 月曜 7:00 (週次の 7:30 より前) は週次を出さない。
        let early = jst("2026-10-05T07:00:00+09:00");
        let last = LastRuns {
            daily_for: Some(d("2026-10-04")),
            weekly_for: Some(d("2026-09-21")),
            monthly_for: Some(d("2026-09-01")),
            collect_at: Some(early.with_timezone(&Utc)),
            ..Default::default()
        };
        assert!(due_jobs(early, &cfg(), &last).is_empty());
    }

    #[test]
    fn jobs_come_in_dependency_order() {
        let now = jst("2026-10-05T09:00:00+09:00");
        let jobs = due_jobs(now, &cfg(), &LastRuns::default());
        assert_eq!(
            jobs,
            vec![Job::Collect, Job::Daily(d("2026-10-04")), Job::Weekly(d("2026-09-28")), Job::Monthly(d("2026-09-01"))]
        );
    }
}
```

- [ ] **Step 2: テストが失敗することを確認する**

Run: `cargo test -p infodesk-app scheduler 2>&1 | grep -E "^error" | head -3`
Expected: `from_settings`、`due_jobs`、`next_collect_at` が未定義。`lib.rs` に `mod scheduler;` を追加してから実行する。

- [ ] **Step 3: 実装する**

`app/src-tauri/src/scheduler.rs` の `Job` の下、テストの上に追加:

```rust
fn parse_time(s: Option<String>, default: (u32, u32)) -> NaiveTime {
    s.and_then(|v| NaiveTime::parse_from_str(v.trim(), "%H:%M").ok())
        .unwrap_or_else(|| NaiveTime::from_hms_opt(default.0, default.1, 0).expect("valid"))
}

fn parse_weekday(s: Option<String>) -> Weekday {
    s.and_then(|v| v.trim().to_lowercase().parse::<Weekday>().ok())
        .unwrap_or(Weekday::Mon)
}

impl ScheduleConfig {
    /// 設定から読む。壊れた値や未設定は既定値 (3時間、7:00、月曜 7:30、1日 8:00)。
    pub fn from_settings(get: impl Fn(&str) -> Option<String>) -> Self {
        let hours = get("schedule.collect_interval_hours")
            .and_then(|v| v.trim().parse::<i64>().ok())
            .unwrap_or(3)
            .clamp(1, 24 * 7);
        Self {
            collect_interval: Duration::hours(hours),
            daily_time: parse_time(get("schedule.daily_time"), (7, 0)),
            weekly_day: parse_weekday(get("schedule.weekly_day")),
            weekly_time: parse_time(get("schedule.weekly_time"), (7, 30)),
            monthly_time: parse_time(get("schedule.monthly_time"), (8, 0)),
        }
    }
}

/// 次に収集する時刻。最後の収集がなければ今。
pub fn next_collect_at(now: DateTime<FixedOffset>, cfg: &ScheduleConfig, last: &LastRuns) -> DateTime<Utc> {
    let now = now.with_timezone(&Utc);
    match last.collect_at {
        Some(at) => (at + cfg.collect_interval).max(now),
        None => now,
    }
}

/// 今の時刻で実行すべきジョブを、依存順 (収集、日次、週次、月次) で返す。
///
/// 日次は「今日の daily_time 以降なら前日分」、まだなら「一昨日分まで」を対象にする。
/// 対象の開始日が最後に生成した日付より後なら実行する。実行は1回で、遅れても
/// 分単位に繰り返さない。週次と月次は、曜日や日にかかわらず、未実行の前期間を補う。
/// ただし当日が実行日の場合は、その時刻までは出さない。
pub fn due_jobs(now: DateTime<FixedOffset>, cfg: &ScheduleConfig, last: &LastRuns) -> Vec<Job> {
    let mut jobs = Vec::new();
    if next_collect_at(now, cfg, last) <= now.with_timezone(&Utc) {
        jobs.push(Job::Collect);
    }
    let today = now.date_naive();
    let time = now.time();

    // 日次: 対象は「最後に daily_time を迎えた日」の前日。
    let daily_anchor = if time >= cfg.daily_time { today } else { today - Duration::days(1) };
    let daily_target = daily_anchor - Duration::days(1);
    if last.daily_for.is_none_or(|d| d < daily_target) {
        jobs.push(Job::Daily(daily_target));
    }

    // 週次: 対象は現在の週の前の週。今週の実行日時をまだ迎えていなければ、前週の実行分
    // (つまり2週前) まで。
    let this_week = period_start(Period::Week, today);
    let weekly_run_day = this_week + Duration::days(cfg.weekly_day.num_days_from_monday() as i64);
    let weekly_reached = today > weekly_run_day || (today == weekly_run_day && time >= cfg.weekly_time);
    let weekly_target = if weekly_reached { this_week } else { this_week - Duration::days(7) } - Duration::days(7);
    if last.weekly_for.is_none_or(|d| d < weekly_target) {
        jobs.push(Job::Weekly(weekly_target));
    }

    // 月次: 対象は前月。今月1日の monthly_time をまだ迎えていなければ、前々月まで。
    let this_month = period_start(Period::Month, today);
    let monthly_reached = today > this_month || time >= cfg.monthly_time;
    let anchor = if monthly_reached { this_month } else { previous_month(this_month) };
    let monthly_target = previous_month(anchor);
    if last.monthly_for.is_none_or(|d| d < monthly_target) {
        jobs.push(Job::Monthly(monthly_target));
    }
    jobs
}

fn previous_month(first_of_month: NaiveDate) -> NaiveDate {
    period_start(Period::Month, first_of_month - Duration::days(1))
}

fn read_last_runs(state: &AppState) -> LastRuns {
    let get = |k: &str| state.with_store(|s| s.get_setting(k)).ok().flatten();
    let date = |k: &str| get(k).and_then(|v| NaiveDate::parse_from_str(&v, "%Y-%m-%d").ok());
    LastRuns {
        collect_at: get("schedule.last_collect_at")
            .and_then(|v| DateTime::parse_from_rfc3339(&v).ok())
            .map(|d| d.with_timezone(&Utc)),
        daily_for: date("schedule.last_daily_for"),
        weekly_for: date("schedule.last_weekly_for"),
        monthly_for: date("schedule.last_monthly_for"),
    }
}

fn record(state: &AppState, key: &str, value: &str) {
    let _ = state.with_store(|s| s.set_setting(key, value));
}

/// 1つのジョブを実行する。日次の後は現在の週次と月次を作り直す。
async fn run_job(state: &AppState, job: &Job) {
    let scopes = jobs::all_category_scopes();
    let today = chrono::Local::now().date_naive();
    let outcome = match job {
        Job::Collect => jobs::run_collect(state).await.map(|_| ()),
        Job::Daily(day) => {
            let r = jobs::run_digests(state, Period::Day, *day, &scopes, false).await.map(|_| ());
            if r.is_ok() {
                record(state, "schedule.last_daily_for", &day.to_string());
                let _ = jobs::run_digests(state, Period::Week, today, &scopes, true).await;
                let _ = jobs::run_digests(state, Period::Month, today, &scopes, true).await;
            }
            r
        }
        Job::Weekly(start) => {
            let r = jobs::run_digests(state, Period::Week, *start, &scopes, false).await.map(|_| ());
            if r.is_ok() {
                record(state, "schedule.last_weekly_for", &start.to_string());
            }
            r
        }
        Job::Monthly(start) => {
            let r = jobs::run_digests(state, Period::Month, *start, &scopes, false).await.map(|_| ());
            if r.is_ok() {
                record(state, "schedule.last_monthly_for", &start.to_string());
            }
            r
        }
    };
    if let Err(e) = outcome {
        state.update_status(|s| s.last_error = Some(e.message));
    }
}

/// 起動直後と60秒ごとに `due_jobs` を評価する。
pub async fn run_loop(app: AppHandle) {
    let mut ticker = tokio::time::interval(StdDuration::from_secs(60));
    loop {
        ticker.tick().await;
        let state = app.state::<AppState>();
        let cfg = ScheduleConfig::from_settings(|k| state.with_store(|s| s.get_setting(k)).ok().flatten());
        let last = read_last_runs(&state);
        let now = chrono::Local::now().with_timezone(&state.tz);
        state.update_status(|s| s.next_collect_at = Some(next_collect_at(now, &cfg, &last)));
        let due = due_jobs(now, &cfg, &last);
        for job in &due {
            run_job(&state, job).await;
        }
        if !due.is_empty() {
            let _ = jobs::retry_pending(&state).await;
        }
    }
}
```

`app/src-tauri/src/lib.rs`: 先頭に `mod scheduler;` を追加し、`setup` の `app.manage(...)` の後に追加:

```rust
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(scheduler::run_loop(handle));
```

- [ ] **Step 4: テストが通ることを確認する**

Run: `cargo test -p infodesk-app 2>&1 | grep -E "test result|error|panicked"`
Expected: 27 passed。

`Option::is_none_or` は Rust 1.82 以降で使える。`rust-version = "1.89"` なので問題ない。

Run: `cargo clippy --all-targets -- -D warnings 2>&1 | tail -1 && cargo fmt --check && echo FMT_OK`
Expected: clean。

- [ ] **Step 5: spec に実行タイミングを追記する**

`docs/superpowers/specs/2026-10-06-infodesk-design.md` の6節「### プロバイダー」の直前に次を挿入する:

```markdown
### 実行タイミング (Plan 2a で確定)

- 収集のたびに、当日分の記事要約を付け、当日の日次要約を作り直す。
- 日次の時刻 (既定 7:00) に前日の日次要約を確定し、現在の週次と月次を作り直す。
- 週次の時刻 (既定 月曜 7:30) に前週の週次要約を、月次の時刻 (既定 1日 8:00) に前月の月次要約を生成する。
- 実行されなかったジョブは、次に起動または復帰したときに1回だけ補う。各スケジュール実行の最後に保留中の要約を再試行する。
```

- [ ] **Step 6: コミット**

```bash
git add app/src-tauri docs/superpowers/specs/2026-10-06-infodesk-design.md
git commit -m "feat(app): add scheduler with catch-up and pending retry"
```

---

### Task 7: メニューバー、ウィンドウの常駐、ログイン時起動

**Files:**
- Create: `app/src-tauri/src/tray.rs`
- Modify: `app/src-tauri/src/lib.rs`, `app/src-tauri/src/commands/settings.rs`, `app/src-tauri/src/state.rs`

**Interfaces:**
- Consumes: `AppState`、`AppStatus`、`tauri::tray::TrayIconBuilder`、`tauri::menu::{Menu, MenuItem, PredefinedMenuItem}`、`tauri_plugin_autostart::ManagerExt`
- Produces:
  - `tray::TrayMenu { status_line: MenuItem<Wry>, unread_line: MenuItem<Wry> }` を `AppState` ではなく Tauri の managed state として保持 (`app.manage(TrayMenu)`)
  - `tray::install(app: &tauri::App) -> tauri::Result<()>` (メニュー: 状態行2つ (無効)、区切り、今すぐ収集、ウィンドウを開く、区切り、設定…、区切り、InfoDesk を終了)
  - `tray::refresh(app: &AppHandle)` (状態行を `AppStatus` と未読数から更新する。`status-changed` と `items-changed` のたびに `lib.rs` の通知クロージャから呼ぶ)
  - `tray::status_lines(status: &AppStatus, unread: i64, daily_done: bool, tz: FixedOffset) -> (String, String)` (純粋関数。例: `最終収集 9:00 · 記事128件` は記事数を持たないので `最終収集 9:00`、`未読 61件 · 日次要約 生成済み`)
  - コマンド: `set_launch_at_login(enabled: bool)` (autostart を有効/無効にし、設定 `app.launch_at_login` を保存)、`show_main_window()`
  - ウィンドウを閉じると、設定 `app.stay_resident` が `true` なら隠す、`false` なら終了する。
  - 起動引数に `--hidden` があればウィンドウを表示しない (ログイン時起動用)。

- [ ] **Step 1: 失敗するテストを書く**

`app/src-tauri/src/tray.rs`:

```rust
use chrono::{FixedOffset, Local};
use infodesk_core::model::{Category, DigestScope, ItemFilter, Period};
use infodesk_core::period::period_start;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{App, AppHandle, Manager, Wry};

use crate::state::{AppState, AppStatus};

pub struct TrayMenu {
    pub status_line: MenuItem<Wry>,
    pub unread_line: MenuItem<Wry>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn jst() -> FixedOffset {
        FixedOffset::east_opt(9 * 3600).unwrap()
    }

    #[test]
    fn status_lines_describe_collect_and_digest_state() {
        let mut st = AppStatus::default();
        assert_eq!(
            status_lines(&st, 0, false, jst()),
            ("未収集".to_string(), "未読 0件 · 日次要約 未生成".to_string())
        );
        st.last_collect_at = Some("2026-10-06T00:00:00Z".parse().unwrap());
        assert_eq!(status_lines(&st, 61, true, jst()).0, "最終収集 09:00");
        assert_eq!(status_lines(&st, 61, true, jst()).1, "未読 61件 · 日次要約 生成済み");
        st.collecting = true;
        assert_eq!(status_lines(&st, 61, true, jst()).0, "収集中…");
        st.collecting = false;
        st.digesting = true;
        assert_eq!(status_lines(&st, 61, false, jst()).1, "未読 61件 · 要約を生成中…");
        st.digesting = false;
        st.last_error = Some("ollama 404: model not found".into());
        assert_eq!(status_lines(&st, 61, false, jst()).1, "未読 61件 · エラー: ollama 404: model not found");
    }
}
```

`app/src-tauri/src/commands/settings.rs` のテストに追加:

```rust
    #[test]
    fn stay_resident_defaults_true_and_reads_setting() {
        let (state, _) = test_state();
        assert!(stay_resident(&state));
        set_setting_value(&state, "app.stay_resident", "false").unwrap();
        assert!(!stay_resident(&state));
        set_setting_value(&state, "app.stay_resident", "garbage").unwrap();
        assert!(stay_resident(&state), "不正な値は既定の true");
    }
```

- [ ] **Step 2: テストが失敗することを確認する**

Run: `cargo test -p infodesk-app 2>&1 | grep -E "^error" | head -3`
Expected: `status_lines`、`stay_resident` が未定義。`lib.rs` に `mod tray;` を追加してから実行する。

- [ ] **Step 3: 実装する**

`app/src-tauri/src/tray.rs` の `TrayMenu` の下、テストの上に追加:

```rust
/// メニューの状態行。1行目は収集、2行目は未読と日次要約。
pub fn status_lines(status: &AppStatus, unread: i64, daily_done: bool, tz: FixedOffset) -> (String, String) {
    let first = if status.collecting {
        "収集中…".to_string()
    } else {
        match status.last_collect_at {
            Some(at) => format!("最終収集 {}", at.with_timezone(&tz).format("%H:%M")),
            None => "未収集".to_string(),
        }
    };
    let digest = if status.digesting || status.summarizing {
        "要約を生成中…".to_string()
    } else if let Some(e) = &status.last_error {
        format!("エラー: {e}")
    } else if daily_done {
        "日次要約 生成済み".to_string()
    } else {
        "日次要約 未生成".to_string()
    };
    (first, format!("未読 {unread}件 · {digest}"))
}

fn daily_done_today(state: &AppState) -> bool {
    let today = period_start(Period::Day, Local::now().date_naive());
    state
        .with_store(|s| {
            for c in Category::ALL {
                if s.get_digest(Period::Day, today, &DigestScope::Category(c))?.is_some() {
                    return Ok(true);
                }
            }
            Ok(false)
        })
        .unwrap_or(false)
}

fn unread_total(state: &AppState) -> i64 {
    state
        .with_store(|s| s.count_items(&ItemFilter { unread_only: true, ..Default::default() }))
        .unwrap_or(0)
}

/// 状態行を更新する。`status-changed` と `items-changed` のたびに呼ぶ。
pub fn refresh(app: &AppHandle) {
    let Some(menu) = app.try_state::<TrayMenu>() else {
        return;
    };
    let state = app.state::<AppState>();
    let (first, second) = status_lines(&state.status(), unread_total(&state), daily_done_today(&state), state.tz);
    let _ = menu.status_line.set_text(first);
    let _ = menu.unread_line.set_text(second);
}

pub fn show_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

pub fn install(app: &App) -> tauri::Result<()> {
    let status_line = MenuItem::with_id(app, "status", "未収集", false, None::<&str>)?;
    let unread_line = MenuItem::with_id(app, "unread", "未読 0件", false, None::<&str>)?;
    let collect = MenuItem::with_id(app, "collect", "今すぐ収集", true, None::<&str>)?;
    let open = MenuItem::with_id(app, "open", "ウィンドウを開く", true, Some("CmdOrCtrl+O"))?;
    let settings = MenuItem::with_id(app, "settings", "設定…", true, Some("CmdOrCtrl+,"))?;
    let quit = MenuItem::with_id(app, "quit", "InfoDesk を終了", true, Some("CmdOrCtrl+Q"))?;
    let menu = Menu::with_items(
        app,
        &[
            &status_line,
            &unread_line,
            &PredefinedMenuItem::separator(app)?,
            &collect,
            &open,
            &PredefinedMenuItem::separator(app)?,
            &settings,
            &PredefinedMenuItem::separator(app)?,
            &quit,
        ],
    )?;
    app.manage(TrayMenu { status_line, unread_line });
    TrayIconBuilder::with_id("main")
        .icon(app.default_window_icon().expect("app icon").clone())
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "collect" => {
                let _ = crate::commands::digests::collect_now(app.clone());
            }
            "open" => show_main_window(app),
            "settings" => {
                show_main_window(app);
                let _ = tauri::Emitter::emit(app, "open-settings", ());
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .build(app)?;
    refresh(app.handle());
    Ok(())
}
```

`app/src-tauri/src/commands/settings.rs` に追加 (`probe` 関数の上):

```rust
/// `app.stay_resident` 設定。不正な値は true。
pub fn stay_resident(state: &AppState) -> bool {
    state
        .with_store(|s| s.get_setting_or("app.stay_resident", "true"))
        .map(|v| v != "false")
        .unwrap_or(true)
}

#[tauri::command]
pub fn set_launch_at_login(app: tauri::AppHandle, state: State<'_, AppState>, enabled: bool) -> AppResult<()> {
    use tauri_plugin_autostart::ManagerExt;
    let launcher = app.autolaunch();
    let result = if enabled { launcher.enable() } else { launcher.disable() };
    result.map_err(|e| e.to_string())?;
    set_setting_value(&state, "app.launch_at_login", if enabled { "true" } else { "false" })
}

#[tauri::command]
pub fn show_main_window(app: tauri::AppHandle) {
    crate::tray::show_main_window(&app);
}
```

`app/src-tauri/src/lib.rs` を次に置き換える (コマンド登録は Task 4〜5 の分を全て含める):

```rust
mod commands;
mod error;
mod jobs;
mod scheduler;
mod state;
mod tray;

use std::sync::Arc;

use infodesk_core::Store;
use tauri::{Emitter, Manager, WindowEvent};

use crate::state::{AppState, EVENT_ITEMS, EVENT_STATUS};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec!["--hidden"]),
        ))
        .setup(|app| {
            let db_path = app.path().app_data_dir()?.join("infodesk.db");
            let store = Store::open(&db_path)?;
            infodesk_core::seed::seed_defaults(&store)?;
            let handle = app.handle().clone();
            let notify: state::Notifier = Arc::new(move |name: &str| {
                let _ = handle.emit(name, ());
                if name == EVENT_STATUS || name == EVENT_ITEMS {
                    tray::refresh(&handle);
                }
            });
            app.manage(AppState::new(store, notify));
            tray::install(app)?;
            if std::env::args().any(|a| a == "--hidden")
                && let Some(window) = app.get_webview_window("main")
            {
                let _ = window.hide();
            }
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(scheduler::run_loop(handle));
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                let state = window.app_handle().state::<AppState>();
                if commands::settings::stay_resident(&state) {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::status::get_status,
            commands::settings::get_settings,
            commands::settings::set_setting,
            commands::settings::test_ollama,
            commands::settings::test_claude,
            commands::settings::set_launch_at_login,
            commands::settings::show_main_window,
            commands::items::list_items,
            commands::items::get_item,
            commands::items::set_read,
            commands::items::set_starred,
            commands::items::mark_all_read,
            commands::items::open_item,
            commands::items::sidebar_counts,
            commands::sources::list_sources,
            commands::sources::add_rss_source,
            commands::sources::update_source,
            commands::sources::set_source_enabled,
            commands::sources::delete_source,
            commands::topics::list_topics,
            commands::topics::add_topic,
            commands::topics::update_topic,
            commands::topics::delete_topic,
            commands::digests::get_digests,
            commands::digests::regenerate_digest,
            commands::digests::collect_now,
            commands::digests::retry_pending_digests,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
```

`tray::refresh` は通知クロージャから呼ばれるが、`refresh` 自体は `with_store` でDBをロックする。通知は `update_status` と `notify` の中で、ロックを解放した後に呼ばれるので、デッドロックしない (`update_status` は `status` のロックを閉じてから通知し、`with_store` は閉じたクロージャの外で通知する)。これを守るため、`with_store` のクロージャ内から `notify` を呼んではいけない。

- [ ] **Step 4: テストとビルドを確認する**

Run: `cargo test -p infodesk-app 2>&1 | grep -E "test result|error|panicked" && cargo clippy --all-targets -- -D warnings 2>&1 | tail -1 && cargo fmt --check && echo FMT_OK`
Expected: 29 passed、clean。

コンパイルエラーが出た場合の当たり: `MenuItem::with_id` の第5引数は `Option<impl AsRef<str>>` (アクセラレーター)、`Menu::with_items` は `&[&dyn IsMenuItem<R>]` を取る。`PredefinedMenuItem::separator(app)?` を配列の中で `&` 付きで書くために、一時変数に受けてから並べる必要があればそうする。`api.prevent_close()` は `WindowEvent::CloseRequested { api, .. }` の `api` に対して呼ぶ。

- [ ] **Step 5: コミット**

```bash
git add app/src-tauri
git commit -m "feat(app): add tray menu, resident window, launch at login"
```

---

### Task 8: 権限設定と実機での動作確認

**Files:**
- Modify: `app/src-tauri/capabilities/default.json`, `README.md`

**Interfaces:**
- Consumes: Task 1〜7 の全部
- Produces: `npm run tauri dev` で起動し、トレイメニューから収集でき、DBに記事が入る状態。Plan 2b の前提。

- [ ] **Step 1: capabilities を整える**

`app/src-tauri/capabilities/default.json` を次にする (テンプレートの内容を置き換える):

```json
{
  "$schema": "../gen/schemas/desktop-schema.json",
  "identifier": "default",
  "description": "InfoDesk main window",
  "windows": ["main"],
  "permissions": [
    "core:default",
    "core:event:default",
    "core:window:allow-show",
    "core:window:allow-hide",
    "core:window:allow-set-focus",
    "opener:default",
    "autostart:default"
  ]
}
```

`opener` と `autostart` はRust側から使うため、JS側の権限は最小でよい。`npm run tauri dev` の起動時に権限名が不明だとエラーになるので、エラーに従って名前を直す (`gen/schemas/desktop-schema.json` に一覧がある)。

- [ ] **Step 2: 起動して確認する**

```bash
cd app && npm run tauri dev
```

確認 (手動):

1. ウィンドウ (テンプレートの画面) が開き、メニューバーにアイコンが出る。メニューの1行目が「未収集」、2行目が「未読 0件 · 日次要約 未生成」。
2. 起動から1分以内に収集が始まり (起動直後の `due_jobs` に `Collect`)、1行目が「収集中…」になり、終わると「最終収集 HH:MM」になる。
3. `ls ~/Library/Application\ Support/jp.infodesk.app/` に `infodesk.db` と `infodesk.db-wal` がある。
4. 別の端末で `cargo run -q -p infodesk -- --db ~/Library/Application\ Support/jp.infodesk.app/infodesk.db items --limit 3` が記事を表示する。
5. Ollama が動いていれば、数分後に2行目が「未読 N件 · 日次要約 生成済み」になる。Ollama を止めている場合は「エラー: …」になり、`infodesk --db … digest day` が保留を表示する。
6. ウィンドウを閉じてもアプリが終了せず、メニューの「ウィンドウを開く」で再表示される。「InfoDesk を終了」で終了する。
7. もう一度起動し、「今すぐ収集」を2回続けて押す。2回目は無視され、クラッシュしない。

いずれかが期待と違う場合、原因を直してからこのステップをやり直す。確認した結果 (各項目の実際の挙動) をコミットメッセージの本文に書く。

- [ ] **Step 3: README を更新する**

`README.md` の「## 構成」に行を追加:

```markdown
- `app/`: Tauri アプリ (`app/src-tauri` がRust側、`app/src` が画面)
```

「## 使い方」の末尾に追加:

````markdown
## アプリ

```bash
cd app && npm install && npm run tauri dev
```

アプリのDBは `~/Library/Application Support/jp.infodesk.app/infodesk.db`。CLIで同じDBを見るには `--db` で指定する。
````

- [ ] **Step 4: コミット**

```bash
git add app/src-tauri/capabilities README.md
git commit -m "feat(app): configure capabilities, verify tray and scheduler on macOS"
```

---

## Plan 2b (画面) への引き継ぎ

- コマンド一覧と引数の形はこの計画の各 Produces ブロックのとおり。`ItemFilter` は全フィールドを送る (serde の既定値がない)。
- イベント: `status-changed` (再取得: `get_status`)、`items-changed` (再取得: `list_items`、`sidebar_counts`)、`digests-changed` (再取得: `get_digests`)、`open-settings` (設定画面を開く)。ペイロードは空。
- `Digest.last_error` と `DigestStatus` でダイジェストの状態 (生成中は `AppStatus.digesting`、保留と失敗は `status`、理由は `last_error`) を表示する。
- デザインは `design.pen` と `design/exports/` (A-1〜A-12)。

最終レビューで決まった、Plan 2b が前提にする事実と最初に行うこと:

- `invoke` の引数は camelCase (`topicId`)。`ItemFilter` は全フィールドを送る (`#[serde(default)]` を core に付けるのが先)。`SidebarCounts.topics` のキーは文字列 (`{"3": 5}`)。`DigestScope` は `{"category":"github"}` または `{"topic":3}`。
- `set_setting` は `claude.bin` / `node.bin` を検証し (コマンド名、または存在するファイルの絶対パス)、`app.launch_at_login` を拒否する (`set_launch_at_login` を使う)。
- CSP は `default-src 'self'; connect-src ipc: http://ipc.localhost; img-src 'self' data:; style-src 'self' 'unsafe-inline'; font-src 'self' data:`。外部画像や外部フォントは読めない。
- 未提供のコマンド: 件数取得 (`count_items`)、`last_error` のクリア、`CollectSummary` の取得。必要になった時点で追加する。
- 残課題 (2b または 3 で扱う): 記事要約は最初の失敗で当日分を全て止める。日次要約は新着0件でも収集ごとに作り直す。全ソース失敗時の再試行は約15分。強制再生成の失敗は15分ごとに全カテゴリーを再実行する。DBが開けない時のダイアログとログがない。ロックが poison すると再起動まで全コマンドが失敗する。トレイ1行目に記事数がない。capabilities に JS 側の autostart/opener 権限が残っている。
- 利用者による手動確認が未了: トレイの表示と「今すぐ収集」の二重押し、ウィンドウを閉じて常駐、再表示、Dock クリックでの再表示 (`target/debug/bundle/macos/InfoDesk.app`)。
