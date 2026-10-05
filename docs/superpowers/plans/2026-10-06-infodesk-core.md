# InfoDesk Core (Plan 1 of 2) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 収集、保存、絞り込み、要約を行うRustライブラリ `infodesk-core` と、それを端末から動かすCLI `infodesk` を作る。

**Architecture:** Cargoワークスペースに2クレートを置く。`crates/core` は store (SQLite + FTS5)、collector (HTTP取得とパース)、llm (Ollama / Claude)、summarizer (記事要約と階層式の期間要約) を持つ。`crates/cli` は core を呼ぶ薄いコマンドである。DB操作は同期、HTTPとLLMは非同期とし、非同期処理の途中でDB接続を保持しない構成 (計画、実行、保存の3段階) にする。Plan 2 のTauriアプリが同じ core をそのまま使う。

**Tech Stack:** Rust 1.97、rusqlite 0.40 (bundled、FTS5 trigram)、reqwest 0.13、feed-rs 3.0、scraper 0.27、tokio 1、chrono 0.4、async-trait、thiserror 2、clap 4、wiremock 0.6 (テスト)。

**Spec:** `docs/superpowers/specs/2026-10-06-infodesk-design.md`

**この計画の範囲:** specの3節 (collector、store、summarizer)、4節、5節、6節、8節、9節、10節。scheduler、メニューバー常駐、画面 (spec 7節) は Plan 2 (`Tauri app`) で扱う。Plan 2 は Plan 1 の実装完了後、実際のインターフェースを見て書く。

## Global Constraints

- 対象OSはmacOSのみ。
- コミットメッセージに `Co-Authored-By` などのAI帰属トレーラーを付けない。
- 依存クレートは `cargo add` で追加する。版を手書きしない。
- 単体テストはネットワークに接続しない。HTTPは wiremock、実レスポンスは `crates/core/tests/fixtures/` のファイルを使う。
- 時刻はUTCのRFC3339文字列 (`2026-10-05T01:00:00Z` 形式、秒精度、末尾 `Z`) でDBに保存する。日付の境界はローカルタイムゾーンで計算する。
- 期間 (今日、週、月) の判定は記事の `fetched_at` (初回取得時刻) を使う。週は月曜始まり。
- 記事はURLを一意キーとする。再取得時は `score` だけ更新し、既読、スター、要約を保持する。
- Claudeが選択されていて失敗した場合、Ollamaへ自動切替しない。
- 要約と期間要約は日本語で生成する。
- 記事本文の取得とスクレイピングはしない。フィードとAPIが返すタイトルと概要だけを扱う。

## 検証済みの外部仕様 (2026-10-06 に実物で確認)

| 対象 | 確認した内容 |
|---|---|
| GitHub trending | `https://github.com/trending?since=daily\|weekly\|monthly` がHTMLを返す。リポジトリは `article.Box-row`、リンクは `h2 a` の `href` (`/owner/repo`)、説明は `p`、期間内スター数は本文中の `1,430 stars today` (`this week`、`this month`)。 |
| arXiv | `https://export.arxiv.org/api/query?search_query=cat:cs.AI&sortBy=submittedDate&sortOrder=descending&max_results=N` がAtomを返す。feed-rsで解析できる。 |
| Hugging Face | `https://huggingface.co/api/models?sort=trendingScore&limit=N` はJSON配列。キーは `id, likes, trendingScore, downloads, pipeline_tag, createdAt`。`https://huggingface.co/api/daily_papers?limit=N` はJSON配列。キーは `paper{id, upvotes}, title, summary, publishedAt`。 |
| Hacker News | `https://hacker-news.firebaseio.com/v0/topstories.json` はid配列 (500件)。`/v0/item/{id}.json` のキーは `by, id, score, time, title, type, url`。 |
| RSS (200で解析可) | はてなブックマークIT、Publickey、ITmedia、Zenn、TechCrunch、The Verge、Ars Technica、MIT Technology Review、joint-kaigo.com、care-news.jp、Google News検索 (認知症、介護、dementia)。URLはTask 8のseedに記載。 |
| RSSなし | CBnews (`cbnews.jp`) とケアマネタイムス (`caremanagement.jp`) はフィードのリンクがない。PubMedは検索RSSのURLをサイト上で生成する方式で、固定URLがない。3つとも初期ソースから外し、Google News検索のRSSで代替する。 |
| feed-rs 3.0 | `feed_rs::parser::parse(&[u8])`。`feed.title: Option<Text>`、`entry.title / summary: Option<Text>` (`.content`)、`entry.links[].href`、`entry.content.body: Option<String>`、`entry.published / updated: Option<DateTime<Utc>>`、`entry.authors[].name: Option<String>`。 |
| FTS5 trigram | bundledのSQLiteは3.53.2。`tokenize='trigram'` が使える。3文字以上の語句はMATCHで一致する。2文字の「介護」はMATCHで一致しない (0件)。2文字以下はLIKEで照合する必要がある。 |
| Ollama | `POST http://localhost:11434/api/chat`、本文 `{"model","stream":false,"think":false,"options":{"num_ctx":16384},"messages":[{role,content}]}`。応答は `message.content`。 |
| claude CLI 2.1.285 | `claude -p --tools "" --output-format json --no-session-persistence --setting-sources project --system-prompt <text>`、プロンプトは標準入力。応答JSONの `result` が本文、`is_error` が真偽値。`--bare` は `Not logged in` になるため使わない。 |

specからの変更点は次の2つ。Task 8とTask 5で反映する。

- spec 3節の trait `Source` は、`SourceKind` による `match` の関数 `fetch_source` に置き換える。実装が6種類で固定のため、traitは不要である。
- spec 4節の医療介護の初期ソースは、上表の理由で joint-kaigo.com、care-news.jp、Google News検索に置き換える。

## Review Focus

1. 2文字以下のキーワード (例: 介護、AI)。trigramのMATCHでは一致しない。利用者は一致を期待する。Task 3でテストする。
2. FTS5の特殊文字を含むキーワード (`C++`、`"quoted"`、`AND`、`*`)。SQLエラーにならず、文字どおりに照合する。Task 3でテストする。
3. RSSの概要にHTMLタグと実体参照が入る、日付がない、リンクがないエントリー。タグは除去し、日付なしは受け入れ、リンクなしは捨てる。Task 5でテストする。
4. GitHub trendingのHTML構造が変わり、リポジトリが0件になる。空の成功ではなくエラーにして、設定画面のソース状態に出す。Task 6でテストする。
5. LLMが空文字または空白だけを返す。生成済みとして保存せず、失敗として扱い再試行の対象にする。Task 9とTask 10でテストする。

## File Structure

```
Cargo.toml                         ワークスペース定義
.gitignore
crates/core/
  Cargo.toml
  src/lib.rs                       モジュール宣言
  src/error.rs                     Error, Result
  src/model.rs                     Category, SourceKind, Period, Source, Item, Topic, Digest など
  src/period.rs                    期間の開始日、終了日、UTC範囲
  src/testutil.rs                  テスト用ヘルパー (cfg(test))
  src/seed.rs                      初期ソースと既定設定
  src/store/mod.rs                 Store::open、時刻と列の変換ヘルパー
  src/store/schema.rs              CREATE TABLE
  src/store/settings.rs            設定のキーと値
  src/store/sources.rs             ソースのCRUD
  src/store/topics.rs              トピックのCRUD
  src/store/items.rs               upsert、絞り込み、キーワード照合
  src/store/digests.rs             期間要約の保存と取得
  src/collector/mod.rs             http_client、fetch_source、fetch_all、apply_outcomes
  src/collector/text.rs            strip_html、collapse_ws、truncate_chars
  src/collector/feed.rs            RSS / Atom の解析、取得、検証
  src/collector/github.rs          GitHub trending
  src/collector/hackernews.rs      Hacker News
  src/collector/huggingface.rs     Hugging Face
  src/llm/mod.rs                   LlmProvider、clean_output、provider_from_settings
  src/llm/ollama.rs                OllamaProvider
  src/llm/claude.rs                ClaudeProvider
  src/summarizer/mod.rs            plan_digest、finish_digest、run_digest、summarize_top_items
  src/summarizer/prompts.rs        プロンプト文
  examples/fetch_fixtures.rs       実レスポンスをfixturesに保存する
  tests/fixtures/                  保存した実レスポンス
crates/cli/
  Cargo.toml
  src/main.rs                      infodesk コマンド
```

---

### Task 1: ワークスペース、モデル、Store の初期化、設定

**Files:**
- Create: `Cargo.toml`, `.gitignore`, `crates/core/Cargo.toml` (cargo new), `crates/core/src/lib.rs`, `crates/core/src/error.rs`, `crates/core/src/model.rs`, `crates/core/src/store/mod.rs`, `crates/core/src/store/schema.rs`, `crates/core/src/store/settings.rs`

**Interfaces:**
- Consumes: なし
- Produces:
  - `infodesk_core::error::{Error, Result}`。`Error` の変種は `Db, Http, InvalidFeed(String), Parse(String), Llm(String), Io, Json, Invalid(String)`。
  - `infodesk_core::model::*` (下のコードの全型)。
  - `Store::open(path: &Path) -> Result<Store>`, `Store::open_in_memory() -> Result<Store>`
  - `Store::get_setting(&self, key: &str) -> Result<Option<String>>`, `Store::get_setting_or(&self, key: &str, default: &str) -> Result<String>`, `Store::set_setting(&self, key: &str, value: &str) -> Result<()>`
  - crate内ヘルパー: `store::ts(&DateTime<Utc>) -> String`, `store::col_ts(&Row, usize)`, `store::col_opt_ts(&Row, usize)`, `store::col_parse(&Row, usize, impl Fn(&str) -> Option<T>)`, `store::conv_err(usize, String) -> rusqlite::Error`

- [ ] **Step 1: ワークスペースとクレートを作る**

`Cargo.toml` (リポジトリ直下):

```toml
[workspace]
resolver = "2"
members = ["crates/core"]
```

`.gitignore`:

```
target/
.DS_Store
```

実行:

```bash
cargo new --lib crates/core --name infodesk-core --vcs none
cd crates/core
cargo add rusqlite --features bundled
cargo add chrono --features serde
cargo add serde --features derive
cargo add serde_json thiserror async-trait feed-rs scraper
cargo add reqwest --features json
cargo add tokio --features process,io-util
cargo add --dev tokio --features macros,rt-multi-thread
cargo add --dev wiremock tempfile
cd ../..
```

- [ ] **Step 2: error.rs と model.rs を書く**

`crates/core/src/error.rs`:

```rust
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("database: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("http: {0}")]
    Http(#[from] reqwest::Error),
    #[error("invalid feed: {0}")]
    InvalidFeed(String),
    #[error("parse: {0}")]
    Parse(String),
    #[error("llm: {0}")]
    Llm(String),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("invalid value: {0}")]
    Invalid(String),
}

pub type Result<T> = std::result::Result<T, Error>;
```

`crates/core/src/model.rs`:

```rust
use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    Github,
    Arxiv,
    Huggingface,
    Hackernews,
    TechJp,
    TechIntl,
    Medcare,
}

impl Category {
    pub const ALL: [Category; 7] = [
        Category::Github,
        Category::Arxiv,
        Category::Huggingface,
        Category::Hackernews,
        Category::TechJp,
        Category::TechIntl,
        Category::Medcare,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Category::Github => "github",
            Category::Arxiv => "arxiv",
            Category::Huggingface => "huggingface",
            Category::Hackernews => "hackernews",
            Category::TechJp => "tech_jp",
            Category::TechIntl => "tech_intl",
            Category::Medcare => "medcare",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|c| c.as_str() == s)
    }

    pub fn label_ja(self) -> &'static str {
        match self {
            Category::Github => "GitHub",
            Category::Arxiv => "arXiv",
            Category::Huggingface => "Hugging Face",
            Category::Hackernews => "Hacker News",
            Category::TechJp => "国内tech",
            Category::TechIntl => "海外tech",
            Category::Medcare => "医療介護",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceKind {
    GithubTrending,
    Arxiv,
    HfModels,
    HfPapers,
    Hackernews,
    Rss,
}

impl SourceKind {
    pub const ALL: [SourceKind; 6] = [
        SourceKind::GithubTrending,
        SourceKind::Arxiv,
        SourceKind::HfModels,
        SourceKind::HfPapers,
        SourceKind::Hackernews,
        SourceKind::Rss,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            SourceKind::GithubTrending => "github_trending",
            SourceKind::Arxiv => "arxiv",
            SourceKind::HfModels => "hf_models",
            SourceKind::HfPapers => "hf_papers",
            SourceKind::Hackernews => "hackernews",
            SourceKind::Rss => "rss",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.as_str() == s)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Period {
    Day,
    Week,
    Month,
}

impl Period {
    pub fn as_str(self) -> &'static str {
        match self {
            Period::Day => "day",
            Period::Week => "week",
            Period::Month => "month",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        [Period::Day, Period::Week, Period::Month]
            .into_iter()
            .find(|p| p.as_str() == s)
    }

    pub fn label_ja(self) -> &'static str {
        match self {
            Period::Day => "今日",
            Period::Week => "今週",
            Period::Month => "今月",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DigestStatus {
    Pending,
    Done,
    Failed,
}

impl DigestStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            DigestStatus::Pending => "pending",
            DigestStatus::Done => "done",
            DigestStatus::Failed => "failed",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        [DigestStatus::Pending, DigestStatus::Done, DigestStatus::Failed]
            .into_iter()
            .find(|p| p.as_str() == s)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DigestScope {
    Category(Category),
    Topic(i64),
}

impl DigestScope {
    /// DBの `digests.scope` 列に入れる文字列。例: `category:github`、`topic:3`。
    pub fn key(&self) -> String {
        match self {
            DigestScope::Category(c) => format!("category:{}", c.as_str()),
            DigestScope::Topic(id) => format!("topic:{id}"),
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        let (kind, rest) = s.split_once(':')?;
        match kind {
            "category" => Category::parse(rest).map(DigestScope::Category),
            "topic" => rest.parse().ok().map(DigestScope::Topic),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Source {
    pub id: i64,
    pub kind: SourceKind,
    pub name: String,
    pub category: Category,
    /// RSSはフィードURL。arxivは分類 (`cs.AI`)。github_trendingは `daily|weekly|monthly`。他は識別用の固定文字列。
    pub config: String,
    pub enabled: bool,
    pub last_fetched_at: Option<DateTime<Utc>>,
    pub last_error: Option<String>,
}

/// collectorが返す、保存前の記事。
#[derive(Debug, Clone, PartialEq)]
pub struct NewItem {
    pub url: String,
    pub title: String,
    pub excerpt: String,
    pub author: Option<String>,
    pub published_at: Option<DateTime<Utc>>,
    pub score: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Item {
    pub id: i64,
    /// ソースが削除された記事は None。
    pub source_id: Option<i64>,
    pub source_name: String,
    pub category: Category,
    pub url: String,
    pub title: String,
    pub excerpt: String,
    pub author: Option<String>,
    pub published_at: Option<DateTime<Utc>>,
    pub fetched_at: DateTime<Utc>,
    pub score: Option<i64>,
    pub ai_summary: Option<String>,
    pub read: bool,
    pub starred: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Topic {
    pub id: i64,
    pub name: String,
    pub keywords: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Digest {
    pub id: i64,
    pub period: Period,
    pub period_start: NaiveDate,
    pub scope: DigestScope,
    pub body: String,
    pub item_ids: Vec<i64>,
    pub provider: String,
    pub status: DigestStatus,
    pub attempts: i64,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ItemOrder {
    /// 公開日時 (なければ取得日時) の新しい順。
    #[default]
    Recent,
    /// 指標の大きい順。指標なしは後ろ。
    Score,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ItemFilter {
    pub category: Option<Category>,
    pub topic_id: Option<i64>,
    pub source_id: Option<i64>,
    /// `fetched_at >= since`
    pub since: Option<DateTime<Utc>>,
    /// `fetched_at < until`
    pub until: Option<DateTime<Utc>>,
    pub text: Option<String>,
    pub unread_only: bool,
    pub starred_only: bool,
    pub order: ItemOrder,
    /// 0 のときは 100 件。
    pub limit: u32,
    pub offset: u32,
}
```

- [ ] **Step 3: 失敗するテストを書く**

`crates/core/src/store/mod.rs`:

```rust
mod schema;
mod settings;

use std::path::Path;

use chrono::{DateTime, SecondsFormat, Utc};
use rusqlite::{Connection, Row};

use crate::error::Result;

pub struct Store {
    pub(crate) conn: Connection,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Category, DigestScope, Period, SourceKind};

    #[test]
    fn settings_roundtrip_and_default() {
        let store = Store::open_in_memory().unwrap();
        assert_eq!(store.get_setting("k").unwrap(), None);
        assert_eq!(store.get_setting_or("k", "d").unwrap(), "d");
        store.set_setting("k", "v1").unwrap();
        store.set_setting("k", "v2").unwrap();
        assert_eq!(store.get_setting("k").unwrap().as_deref(), Some("v2"));
    }

    #[test]
    fn open_twice_keeps_data() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.db");
        Store::open(&path).unwrap().set_setting("k", "v").unwrap();
        let again = Store::open(&path).unwrap();
        assert_eq!(again.get_setting("k").unwrap().as_deref(), Some("v"));
    }

    #[test]
    fn enum_strings_roundtrip() {
        for c in Category::ALL {
            assert_eq!(Category::parse(c.as_str()), Some(c));
        }
        for k in SourceKind::ALL {
            assert_eq!(SourceKind::parse(k.as_str()), Some(k));
        }
        assert_eq!(Period::parse("week"), Some(Period::Week));
        assert_eq!(Category::parse("nope"), None);
        let s = DigestScope::Topic(3);
        assert_eq!(DigestScope::parse(&s.key()), Some(s));
        let s = DigestScope::Category(Category::Medcare);
        assert_eq!(s.key(), "category:medcare");
        assert_eq!(DigestScope::parse(&s.key()), Some(s));
        assert_eq!(DigestScope::parse("garbage"), None);
    }

    #[test]
    fn ts_is_second_precision_utc_z() {
        let t: DateTime<Utc> = "2026-10-05T10:00:00.123+09:00".parse().unwrap();
        assert_eq!(ts(&t), "2026-10-05T01:00:00Z");
    }
}
```

`crates/core/src/lib.rs`:

```rust
pub mod error;
pub mod model;
pub mod store;

pub use error::{Error, Result};
pub use store::Store;
```

- [ ] **Step 4: テストが失敗することを確認する**

Run: `cargo test -p infodesk-core`
Expected: コンパイルエラー。`schema`、`settings` のファイルがない、`Store::open_in_memory` と `ts` が未定義。

- [ ] **Step 5: 実装する**

`crates/core/src/store/schema.rs`:

```rust
pub const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS sources (
    id INTEGER PRIMARY KEY,
    kind TEXT NOT NULL,
    name TEXT NOT NULL,
    category TEXT NOT NULL,
    config TEXT NOT NULL,
    enabled INTEGER NOT NULL DEFAULT 1,
    last_fetched_at TEXT,
    last_error TEXT,
    UNIQUE(kind, config)
);
CREATE TABLE IF NOT EXISTS items (
    id INTEGER PRIMARY KEY,
    source_id INTEGER,
    source_name TEXT NOT NULL,
    category TEXT NOT NULL,
    url TEXT NOT NULL UNIQUE,
    title TEXT NOT NULL,
    excerpt TEXT NOT NULL DEFAULT '',
    author TEXT,
    published_at TEXT,
    fetched_at TEXT NOT NULL,
    score INTEGER,
    ai_summary TEXT,
    read INTEGER NOT NULL DEFAULT 0,
    starred INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS idx_items_category_fetched ON items(category, fetched_at);
CREATE VIRTUAL TABLE IF NOT EXISTS items_fts USING fts5(title, excerpt, tokenize='trigram');
CREATE TABLE IF NOT EXISTS topics (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    keywords TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS digests (
    id INTEGER PRIMARY KEY,
    period TEXT NOT NULL,
    period_start TEXT NOT NULL,
    scope TEXT NOT NULL,
    body TEXT NOT NULL DEFAULT '',
    item_ids TEXT NOT NULL DEFAULT '[]',
    provider TEXT NOT NULL DEFAULT '',
    status TEXT NOT NULL,
    attempts INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL,
    UNIQUE(period, period_start, scope)
);
CREATE TABLE IF NOT EXISTS settings (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
"#;
```

`crates/core/src/store/mod.rs` の `pub struct Store` の下、`#[cfg(test)]` の上に追加:

```rust
impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        Self::init(Connection::open(path)?)
    }

    pub fn open_in_memory() -> Result<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Self> {
        conn.execute_batch(schema::SCHEMA)?;
        Ok(Self { conn })
    }
}

/// DBに保存する時刻文字列。秒精度、UTC、末尾Z。文字列比較が時刻順と一致する。
pub(crate) fn ts(dt: &DateTime<Utc>) -> String {
    dt.to_rfc3339_opts(SecondsFormat::Secs, true)
}

pub(crate) fn conv_err(col: usize, msg: String) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(col, rusqlite::types::Type::Text, msg.into())
}

pub(crate) fn col_parse<T>(
    row: &Row,
    col: usize,
    parse: impl Fn(&str) -> Option<T>,
) -> rusqlite::Result<T> {
    let s: String = row.get(col)?;
    parse(&s).ok_or_else(|| conv_err(col, format!("unexpected value: {s}")))
}

fn parse_ts(s: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|d| d.with_timezone(&Utc))
}

pub(crate) fn col_ts(row: &Row, col: usize) -> rusqlite::Result<DateTime<Utc>> {
    col_parse(row, col, parse_ts)
}

pub(crate) fn col_opt_ts(row: &Row, col: usize) -> rusqlite::Result<Option<DateTime<Utc>>> {
    let s: Option<String> = row.get(col)?;
    match s {
        None => Ok(None),
        Some(s) => parse_ts(&s)
            .map(Some)
            .ok_or_else(|| conv_err(col, format!("unexpected value: {s}"))),
    }
}
```

`crates/core/src/store/settings.rs`:

```rust
use rusqlite::{params, OptionalExtension};

use super::Store;
use crate::error::Result;

impl Store {
    pub fn get_setting(&self, key: &str) -> Result<Option<String>> {
        Ok(self
            .conn
            .query_row("SELECT value FROM settings WHERE key = ?1", params![key], |r| {
                r.get(0)
            })
            .optional()?)
    }

    pub fn get_setting_or(&self, key: &str, default: &str) -> Result<String> {
        Ok(self.get_setting(key)?.unwrap_or_else(|| default.to_string()))
    }

    pub fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO settings(key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }
}
```

- [ ] **Step 6: テストが通ることを確認する**

Run: `cargo test -p infodesk-core`
Expected: 4 passed。未使用警告 (`col_ts` など) は出てよい。後のタスクで使う。

- [ ] **Step 7: コミット**

```bash
git add Cargo.toml Cargo.lock .gitignore crates/core
git commit -m "feat(core): add workspace, models, store schema, settings"
```

---

### Task 2: ソースとトピックのCRUD

**Files:**
- Create: `crates/core/src/store/sources.rs`, `crates/core/src/store/topics.rs`
- Modify: `crates/core/src/store/mod.rs` (モジュール宣言を追加)

**Interfaces:**
- Consumes: `Store { conn }`, `col_parse`, `col_opt_ts`, `ts`, `model::{Source, SourceKind, Category, Topic}`
- Produces:
  - `Store::add_source(&self, kind: SourceKind, name: &str, category: Category, config: &str) -> Result<i64>`
  - `Store::list_sources(&self) -> Result<Vec<Source>>` (id順)
  - `Store::get_source(&self, id: i64) -> Result<Option<Source>>`
  - `Store::update_source(&self, id: i64, name: &str, category: Category, config: &str) -> Result<()>`
  - `Store::set_source_enabled(&self, id: i64, enabled: bool) -> Result<()>`
  - `Store::delete_source(&self, id: i64) -> Result<()>` (記事は残し、`items.source_id` をNULLにする)
  - `Store::record_fetch(&self, id: i64, at: &DateTime<Utc>, error: Option<&str>) -> Result<()>`
  - `Store::add_topic(&self, name: &str, keywords: &[String]) -> Result<i64>`
  - `Store::list_topics(&self) -> Result<Vec<Topic>>`, `Store::get_topic(&self, id: i64) -> Result<Option<Topic>>`, `Store::find_topic_by_name(&self, name: &str) -> Result<Option<Topic>>`
  - `Store::update_topic(&self, id: i64, name: &str, keywords: &[String]) -> Result<()>`, `Store::delete_topic(&self, id: i64) -> Result<()>`

- [ ] **Step 1: 失敗するテストを書く**

`crates/core/src/store/mod.rs` の先頭のモジュール宣言を次に変更:

```rust
mod schema;
mod settings;
mod sources;
mod topics;
```

`crates/core/src/store/sources.rs`:

```rust
use chrono::{DateTime, Utc};
use rusqlite::{params, OptionalExtension, Row};

use super::{col_opt_ts, col_parse, ts, Store};
use crate::error::Result;
use crate::model::{Category, Source, SourceKind};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_list_update_toggle() {
        let store = Store::open_in_memory().unwrap();
        let id = store
            .add_source(SourceKind::Rss, "Zenn", Category::TechJp, "https://zenn.dev/feed")
            .unwrap();
        let s = store.get_source(id).unwrap().unwrap();
        assert_eq!(s.name, "Zenn");
        assert_eq!(s.kind, SourceKind::Rss);
        assert!(s.enabled);
        assert!(s.last_fetched_at.is_none());

        store
            .update_source(id, "Zenn トレンド", Category::TechIntl, "https://zenn.dev/feed2")
            .unwrap();
        store.set_source_enabled(id, false).unwrap();
        let s = store.list_sources().unwrap().remove(0);
        assert_eq!(s.name, "Zenn トレンド");
        assert_eq!(s.category, Category::TechIntl);
        assert_eq!(s.config, "https://zenn.dev/feed2");
        assert!(!s.enabled);
    }

    #[test]
    fn duplicate_kind_and_config_is_rejected() {
        let store = Store::open_in_memory().unwrap();
        store
            .add_source(SourceKind::Rss, "A", Category::TechJp, "https://a/feed")
            .unwrap();
        let dup = store.add_source(SourceKind::Rss, "B", Category::Medcare, "https://a/feed");
        assert!(dup.is_err());
    }

    #[test]
    fn record_fetch_sets_time_and_error_then_clears_error() {
        let store = Store::open_in_memory().unwrap();
        let id = store
            .add_source(SourceKind::Hackernews, "HN", Category::Hackernews, "top")
            .unwrap();
        let t1: DateTime<Utc> = "2026-10-05T01:00:00Z".parse().unwrap();
        store.record_fetch(id, &t1, Some("timeout")).unwrap();
        let s = store.get_source(id).unwrap().unwrap();
        assert_eq!(s.last_fetched_at, Some(t1));
        assert_eq!(s.last_error.as_deref(), Some("timeout"));

        let t2: DateTime<Utc> = "2026-10-05T04:00:00Z".parse().unwrap();
        store.record_fetch(id, &t2, None).unwrap();
        let s = store.get_source(id).unwrap().unwrap();
        assert_eq!(s.last_fetched_at, Some(t2));
        assert_eq!(s.last_error, None);
    }

    #[test]
    fn delete_removes_source() {
        let store = Store::open_in_memory().unwrap();
        let id = store
            .add_source(SourceKind::Rss, "A", Category::TechJp, "https://a/feed")
            .unwrap();
        store.delete_source(id).unwrap();
        assert!(store.get_source(id).unwrap().is_none());
        assert!(store.list_sources().unwrap().is_empty());
    }
}
```

`crates/core/src/store/topics.rs`:

```rust
use rusqlite::{params, OptionalExtension, Row};

use super::{conv_err, Store};
use crate::error::Result;
use crate::model::Topic;

#[cfg(test)]
mod tests {
    use super::*;

    fn kws(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn topic_crud() {
        let store = Store::open_in_memory().unwrap();
        let id = store
            .add_topic("認知症", &kws(&["認知症", "dementia", "Alzheimer"]))
            .unwrap();
        let t = store.get_topic(id).unwrap().unwrap();
        assert_eq!(t.name, "認知症");
        assert_eq!(t.keywords, kws(&["認知症", "dementia", "Alzheimer"]));
        assert_eq!(store.find_topic_by_name("認知症").unwrap().unwrap().id, id);
        assert!(store.find_topic_by_name("なし").unwrap().is_none());

        store.update_topic(id, "認知症ケア", &kws(&["介護"])).unwrap();
        let all = store.list_topics().unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].name, "認知症ケア");
        assert_eq!(all[0].keywords, kws(&["介護"]));

        store.delete_topic(id).unwrap();
        assert!(store.get_topic(id).unwrap().is_none());
    }

    #[test]
    fn keywords_are_trimmed_and_blank_ones_dropped() {
        let store = Store::open_in_memory().unwrap();
        let id = store.add_topic("t", &kws(&["  AI ", "", "   "])).unwrap();
        assert_eq!(store.get_topic(id).unwrap().unwrap().keywords, kws(&["AI"]));
    }
}
```

- [ ] **Step 2: テストが失敗することを確認する**

Run: `cargo test -p infodesk-core store::`
Expected: コンパイルエラー。`add_source`、`add_topic` などが未定義。

- [ ] **Step 3: 実装する**

`crates/core/src/store/sources.rs` の `use` の下、テストの上に追加:

```rust
const SOURCE_COLS: &str =
    "id, kind, name, category, config, enabled, last_fetched_at, last_error";

fn row_to_source(r: &Row) -> rusqlite::Result<Source> {
    Ok(Source {
        id: r.get(0)?,
        kind: col_parse(r, 1, SourceKind::parse)?,
        name: r.get(2)?,
        category: col_parse(r, 3, Category::parse)?,
        config: r.get(4)?,
        enabled: r.get(5)?,
        last_fetched_at: col_opt_ts(r, 6)?,
        last_error: r.get(7)?,
    })
}

impl Store {
    pub fn add_source(
        &self,
        kind: SourceKind,
        name: &str,
        category: Category,
        config: &str,
    ) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO sources(kind, name, category, config) VALUES (?1, ?2, ?3, ?4)",
            params![kind.as_str(), name, category.as_str(), config],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn list_sources(&self) -> Result<Vec<Source>> {
        let mut stmt = self
            .conn
            .prepare(&format!("SELECT {SOURCE_COLS} FROM sources ORDER BY id"))?;
        let rows = stmt
            .query_map([], row_to_source)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn get_source(&self, id: i64) -> Result<Option<Source>> {
        Ok(self
            .conn
            .query_row(
                &format!("SELECT {SOURCE_COLS} FROM sources WHERE id = ?1"),
                params![id],
                row_to_source,
            )
            .optional()?)
    }

    pub fn update_source(
        &self,
        id: i64,
        name: &str,
        category: Category,
        config: &str,
    ) -> Result<()> {
        self.conn.execute(
            "UPDATE sources SET name = ?2, category = ?3, config = ?4 WHERE id = ?1",
            params![id, name, category.as_str(), config],
        )?;
        Ok(())
    }

    pub fn set_source_enabled(&self, id: i64, enabled: bool) -> Result<()> {
        self.conn.execute(
            "UPDATE sources SET enabled = ?2 WHERE id = ?1",
            params![id, enabled],
        )?;
        Ok(())
    }

    /// ソースを削除する。取得済みの記事は残す。
    pub fn delete_source(&self, id: i64) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        tx.execute(
            "UPDATE items SET source_id = NULL WHERE source_id = ?1",
            params![id],
        )?;
        tx.execute("DELETE FROM sources WHERE id = ?1", params![id])?;
        tx.commit()?;
        Ok(())
    }

    pub fn record_fetch(&self, id: i64, at: &DateTime<Utc>, error: Option<&str>) -> Result<()> {
        self.conn.execute(
            "UPDATE sources SET last_fetched_at = ?2, last_error = ?3 WHERE id = ?1",
            params![id, ts(at), error],
        )?;
        Ok(())
    }
}
```

`crates/core/src/store/topics.rs` の `use` の下、テストの上に追加:

```rust
fn row_to_topic(r: &Row) -> rusqlite::Result<Topic> {
    let raw: String = r.get(2)?;
    Ok(Topic {
        id: r.get(0)?,
        name: r.get(1)?,
        keywords: serde_json::from_str(&raw).map_err(|e| conv_err(2, e.to_string()))?,
    })
}

fn clean_keywords(keywords: &[String]) -> Result<String> {
    let cleaned: Vec<&str> = keywords
        .iter()
        .map(|k| k.trim())
        .filter(|k| !k.is_empty())
        .collect();
    Ok(serde_json::to_string(&cleaned)?)
}

impl Store {
    pub fn add_topic(&self, name: &str, keywords: &[String]) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO topics(name, keywords) VALUES (?1, ?2)",
            params![name.trim(), clean_keywords(keywords)?],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn list_topics(&self) -> Result<Vec<Topic>> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, name, keywords FROM topics ORDER BY id")?;
        let rows = stmt
            .query_map([], row_to_topic)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn get_topic(&self, id: i64) -> Result<Option<Topic>> {
        Ok(self
            .conn
            .query_row(
                "SELECT id, name, keywords FROM topics WHERE id = ?1",
                params![id],
                row_to_topic,
            )
            .optional()?)
    }

    pub fn find_topic_by_name(&self, name: &str) -> Result<Option<Topic>> {
        Ok(self
            .conn
            .query_row(
                "SELECT id, name, keywords FROM topics WHERE name = ?1",
                params![name.trim()],
                row_to_topic,
            )
            .optional()?)
    }

    pub fn update_topic(&self, id: i64, name: &str, keywords: &[String]) -> Result<()> {
        self.conn.execute(
            "UPDATE topics SET name = ?2, keywords = ?3 WHERE id = ?1",
            params![id, name.trim(), clean_keywords(keywords)?],
        )?;
        Ok(())
    }

    pub fn delete_topic(&self, id: i64) -> Result<()> {
        self.conn
            .execute("DELETE FROM topics WHERE id = ?1", params![id])?;
        Ok(())
    }
}
```

- [ ] **Step 4: テストが通ることを確認する**

Run: `cargo test -p infodesk-core store::`
Expected: Task 1の4件と合わせて 10 passed。

- [ ] **Step 5: コミット**

```bash
git add crates/core
git commit -m "feat(core): add source and topic storage"
```

---

### Task 3: 記事の保存、重複排除、絞り込み、キーワード照合

**Files:**
- Create: `crates/core/src/store/items.rs`, `crates/core/src/testutil.rs`
- Modify: `crates/core/src/store/mod.rs` (`mod items;` を追加), `crates/core/src/lib.rs` (testutilを追加)

**Interfaces:**
- Consumes: Task 2の `add_source`, `get_source`, `get_topic`, `add_topic`, `delete_source`。`model::{Item, NewItem, ItemFilter, ItemOrder, Source, Category}`
- Produces:
  - `Store::upsert_items(&self, source: &Source, items: &[NewItem], now: &DateTime<Utc>) -> Result<usize>` (戻り値は新規件数)
  - `Store::query_items(&self, filter: &ItemFilter) -> Result<Vec<Item>>`
  - `Store::get_item(&self, id: i64) -> Result<Option<Item>>`
  - `Store::set_read(&self, id: i64, read: bool) -> Result<()>`, `Store::set_starred(&self, id: i64, starred: bool) -> Result<()>`, `Store::set_ai_summary(&self, id: i64, summary: &str) -> Result<()>`
  - `Store::top_unsummarized(&self, category: Category, since: &DateTime<Utc>, limit: u32) -> Result<Vec<Item>>` (`since` 以降に取得した記事を指標順に `limit` 件取り、そのうち要約がないもの)
  - テスト用 (cfg(test)): `testutil::at(&str) -> DateTime<Utc>`, `testutil::new_item(url, title, excerpt, score: Option<i64>) -> NewItem`, `testutil::store_with_source(category: Category) -> (Store, Source)`

- [ ] **Step 1: テスト用ヘルパーを書く**

`crates/core/src/testutil.rs`:

```rust
use chrono::{DateTime, Utc};

use crate::model::{Category, NewItem, Source, SourceKind};
use crate::store::Store;

pub fn at(s: &str) -> DateTime<Utc> {
    s.parse().unwrap()
}

pub fn new_item(url: &str, title: &str, excerpt: &str, score: Option<i64>) -> NewItem {
    NewItem {
        url: url.to_string(),
        title: title.to_string(),
        excerpt: excerpt.to_string(),
        author: None,
        published_at: None,
        score,
    }
}

/// メモリ上のStoreと、指定カテゴリーのRSSソース1件を返す。
pub fn store_with_source(category: Category) -> (Store, Source) {
    let store = Store::open_in_memory().unwrap();
    let id = store
        .add_source(SourceKind::Rss, "Test Feed", category, "https://example.com/feed")
        .unwrap();
    let source = store.get_source(id).unwrap().unwrap();
    (store, source)
}
```

`crates/core/src/lib.rs` に追加:

```rust
#[cfg(test)]
pub(crate) mod testutil;
```

`crates/core/src/store/mod.rs` のモジュール宣言に `mod items;` を追加する (schema, settings, sources, topics の並びに入れる)。

- [ ] **Step 2: 失敗するテストを書く**

`crates/core/src/store/items.rs`:

```rust
use chrono::{DateTime, Utc};
use rusqlite::types::Value;
use rusqlite::{params, params_from_iter, OptionalExtension, Row};

use super::{col_opt_ts, col_parse, col_ts, ts, Store};
use crate::error::{Error, Result};
use crate::model::{Category, Item, ItemFilter, ItemOrder, NewItem, Source};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{at, new_item, store_with_source};

    fn titles(items: &[Item]) -> Vec<&str> {
        items.iter().map(|i| i.title.as_str()).collect()
    }

    fn kws(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn upsert_dedupes_by_url_and_keeps_user_state() {
        let (store, src) = store_with_source(Category::TechJp);
        let t1 = at("2026-10-05T01:00:00Z");
        let n = store
            .upsert_items(&src, &[new_item("https://e.com/a", "A", "x", Some(10))], &t1)
            .unwrap();
        assert_eq!(n, 1);
        let id = store.query_items(&ItemFilter::default()).unwrap()[0].id;
        store.set_read(id, true).unwrap();
        store.set_starred(id, true).unwrap();
        store.set_ai_summary(id, "要約").unwrap();

        let t2 = at("2026-10-06T01:00:00Z");
        let n = store
            .upsert_items(
                &src,
                &[
                    new_item("https://e.com/a", "A changed", "y", Some(99)),
                    new_item("https://e.com/b", "B", "", None),
                ],
                &t2,
            )
            .unwrap();
        assert_eq!(n, 1);

        let a = store.get_item(id).unwrap().unwrap();
        assert_eq!(a.title, "A");
        assert_eq!(a.score, Some(99));
        assert_eq!(a.fetched_at, t1);
        assert!(a.read && a.starred);
        assert_eq!(a.ai_summary.as_deref(), Some("要約"));
        assert_eq!(a.source_name, "Test Feed");
        assert_eq!(a.category, Category::TechJp);
        assert_eq!(store.query_items(&ItemFilter::default()).unwrap().len(), 2);
    }

    #[test]
    fn refetch_without_score_keeps_old_score() {
        let (store, src) = store_with_source(Category::Github);
        let t = at("2026-10-05T01:00:00Z");
        store
            .upsert_items(&src, &[new_item("https://e.com/a", "A", "", Some(10))], &t)
            .unwrap();
        store
            .upsert_items(&src, &[new_item("https://e.com/a", "A", "", None)], &t)
            .unwrap();
        assert_eq!(store.query_items(&ItemFilter::default()).unwrap()[0].score, Some(10));
    }

    #[test]
    fn filters_by_category_time_read_star_source() {
        let (store, src) = store_with_source(Category::TechJp);
        let other_id = store
            .add_source(crate::model::SourceKind::Hackernews, "HN", Category::Hackernews, "top")
            .unwrap();
        let other = store.get_source(other_id).unwrap().unwrap();
        store
            .upsert_items(&src, &[new_item("https://e.com/old", "old", "", None)], &at("2026-10-01T00:00:00Z"))
            .unwrap();
        store
            .upsert_items(&src, &[new_item("https://e.com/new", "new", "", None)], &at("2026-10-05T00:00:00Z"))
            .unwrap();
        store
            .upsert_items(&other, &[new_item("https://e.com/hn", "hn", "", Some(5))], &at("2026-10-05T00:00:00Z"))
            .unwrap();

        let f = ItemFilter { category: Some(Category::TechJp), ..Default::default() };
        assert_eq!(store.query_items(&f).unwrap().len(), 2);

        let f = ItemFilter { source_id: Some(other.id), ..Default::default() };
        assert_eq!(titles(&store.query_items(&f).unwrap()), vec!["hn"]);

        let f = ItemFilter {
            since: Some(at("2026-10-05T00:00:00Z")),
            until: Some(at("2026-10-06T00:00:00Z")),
            category: Some(Category::TechJp),
            ..Default::default()
        };
        assert_eq!(titles(&store.query_items(&f).unwrap()), vec!["new"]);

        let f = ItemFilter { until: Some(at("2026-10-05T00:00:00Z")), ..Default::default() };
        assert_eq!(titles(&store.query_items(&f).unwrap()), vec!["old"]);

        let new_id = store.query_items(&ItemFilter { text: Some("new".into()), ..Default::default() }).unwrap()[0].id;
        store.set_read(new_id, true).unwrap();
        store.set_starred(new_id, true).unwrap();
        let f = ItemFilter { unread_only: true, ..Default::default() };
        assert_eq!(store.query_items(&f).unwrap().len(), 2);
        let f = ItemFilter { starred_only: true, ..Default::default() };
        assert_eq!(titles(&store.query_items(&f).unwrap()), vec!["new"]);
    }

    #[test]
    fn orders_and_paginates() {
        let (store, src) = store_with_source(Category::Github);
        let t = at("2026-10-05T00:00:00Z");
        let mut low = new_item("https://e.com/1", "low", "", Some(1));
        low.published_at = Some(at("2026-10-04T00:00:00Z"));
        let mut high = new_item("https://e.com/2", "high", "", Some(100));
        high.published_at = Some(at("2026-10-02T00:00:00Z"));
        let none = new_item("https://e.com/3", "none", "", None);
        store.upsert_items(&src, &[low, high, none], &t).unwrap();

        let recent = store.query_items(&ItemFilter::default()).unwrap();
        assert_eq!(titles(&recent), vec!["none", "low", "high"]);

        let f = ItemFilter { order: ItemOrder::Score, ..Default::default() };
        assert_eq!(titles(&store.query_items(&f).unwrap()), vec!["high", "low", "none"]);

        let f = ItemFilter { order: ItemOrder::Score, limit: 1, offset: 1, ..Default::default() };
        assert_eq!(titles(&store.query_items(&f).unwrap()), vec!["low"]);
    }

    #[test]
    fn topic_matches_japanese_english_and_short_keywords() {
        let (store, src) = store_with_source(Category::Medcare);
        let t = at("2026-10-05T00:00:00Z");
        store
            .upsert_items(
                &src,
                &[
                    new_item("https://e.com/1", "認知症ケアの最前線", "", None),
                    new_item("https://e.com/2", "New Dementia drug approved", "", None),
                    new_item("https://e.com/3", "介護報酬の改定", "", None),
                    new_item("https://e.com/4", "無関係", "本文に Alzheimer が出る", None),
                    new_item("https://e.com/5", "Rust 2.0", "", None),
                ],
                &t,
            )
            .unwrap();

        let long = store.add_topic("認知症", &kws(&["認知症", "dementia", "alzheimer"])).unwrap();
        let f = ItemFilter { topic_id: Some(long), ..Default::default() };
        let mut got = titles(&store.query_items(&f).unwrap()).into_iter().map(String::from).collect::<Vec<_>>();
        got.sort();
        assert_eq!(got, vec!["New Dementia drug approved", "無関係", "認知症ケアの最前線"]);

        // Review Focus 1: 2文字のキーワードも一致する。
        let short = store.add_topic("介護", &kws(&["介護"])).unwrap();
        let f = ItemFilter { topic_id: Some(short), ..Default::default() };
        assert_eq!(titles(&store.query_items(&f).unwrap()), vec!["介護報酬の改定"]);

        let empty = store.add_topic("空", &[]).unwrap();
        let f = ItemFilter { topic_id: Some(empty), ..Default::default() };
        assert!(store.query_items(&f).unwrap().is_empty());

        let f = ItemFilter { topic_id: Some(9999), ..Default::default() };
        assert!(matches!(store.query_items(&f), Err(Error::Invalid(_))));
    }

    #[test]
    fn keywords_with_special_characters_match_literally() {
        // Review Focus 2
        let (store, src) = store_with_source(Category::TechIntl);
        let t = at("2026-10-05T00:00:00Z");
        store
            .upsert_items(
                &src,
                &[
                    new_item("https://e.com/1", "Modern C++ tips", "", None),
                    new_item("https://e.com/2", "He said \"quoted\" words", "", None),
                    new_item("https://e.com/3", "Cats AND dogs", "", None),
                    new_item("https://e.com/4", "100% done", "", None),
                    new_item("https://e.com/5", "plain", "", None),
                ],
                &t,
            )
            .unwrap();
        for (text, want) in [
            ("C++", "Modern C++ tips"),
            ("\"quoted\"", "He said \"quoted\" words"),
            ("AND", "Cats AND dogs"),
            ("0%", "100% done"),
        ] {
            let f = ItemFilter { text: Some(text.to_string()), ..Default::default() };
            assert_eq!(titles(&store.query_items(&f).unwrap()), vec![want], "text={text}");
        }
        for text in ["*", "a*", "NEAR(", "%", "_"] {
            let f = ItemFilter { text: Some(text.to_string()), ..Default::default() };
            assert!(store.query_items(&f).is_ok(), "text={text}");
        }
        let f = ItemFilter { text: Some("   ".into()), ..Default::default() };
        assert_eq!(store.query_items(&f).unwrap().len(), 5);
    }

    #[test]
    fn top_unsummarized_takes_top_n_then_drops_summarized() {
        let (store, src) = store_with_source(Category::Github);
        let t = at("2026-10-05T00:00:00Z");
        store
            .upsert_items(
                &src,
                &[
                    new_item("https://e.com/1", "s30", "", Some(30)),
                    new_item("https://e.com/2", "s20", "", Some(20)),
                    new_item("https://e.com/3", "s10", "", Some(10)),
                ],
                &t,
            )
            .unwrap();
        store
            .upsert_items(&src, &[new_item("https://e.com/0", "old", "", Some(999))], &at("2026-10-01T00:00:00Z"))
            .unwrap();

        let top = store.top_unsummarized(Category::Github, &t, 2).unwrap();
        assert_eq!(titles(&top), vec!["s30", "s20"]);

        store.set_ai_summary(top[0].id, "済").unwrap();
        let top = store.top_unsummarized(Category::Github, &t, 2).unwrap();
        assert_eq!(titles(&top), vec!["s20"]);
        assert!(store.top_unsummarized(Category::Arxiv, &t, 2).unwrap().is_empty());
    }

    #[test]
    fn deleting_source_keeps_items() {
        let (store, src) = store_with_source(Category::TechJp);
        store
            .upsert_items(&src, &[new_item("https://e.com/a", "A", "", None)], &at("2026-10-05T00:00:00Z"))
            .unwrap();
        store.delete_source(src.id).unwrap();
        let items = store.query_items(&ItemFilter::default()).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].source_id, None);
        assert_eq!(items[0].source_name, "Test Feed");
    }
}
```

- [ ] **Step 3: テストが失敗することを確認する**

Run: `cargo test -p infodesk-core store::items`
Expected: コンパイルエラー。`upsert_items`、`query_items` などが未定義。

- [ ] **Step 4: 実装する**

`crates/core/src/store/items.rs` の `use` の下、テストの上に追加:

```rust
const ITEM_COLS: &str = "items.id, items.source_id, items.source_name, items.category, \
     items.url, items.title, items.excerpt, items.author, items.published_at, \
     items.fetched_at, items.score, items.ai_summary, items.read, items.starred";

fn row_to_item(r: &Row) -> rusqlite::Result<Item> {
    Ok(Item {
        id: r.get(0)?,
        source_id: r.get(1)?,
        source_name: r.get(2)?,
        category: col_parse(r, 3, Category::parse)?,
        url: r.get(4)?,
        title: r.get(5)?,
        excerpt: r.get(6)?,
        author: r.get(7)?,
        published_at: col_opt_ts(r, 8)?,
        fetched_at: col_ts(r, 9)?,
        score: r.get(10)?,
        ai_summary: r.get(11)?,
        read: r.get(12)?,
        starred: r.get(13)?,
    })
}

/// キーワードのいずれかがタイトルか概要に含まれる、というSQL条件を作る。
/// trigramのFTS5は3文字未満の語句に一致しないため、3文字未満はLIKEで照合する。
/// 3文字以上はフレーズとして引用し、FTS5の演算子 (AND, *, NEAR など) を無効にする。
fn keyword_clause(keywords: &[String], args: &mut Vec<Value>) -> String {
    let mut parts = Vec::new();
    for kw in keywords.iter().map(|k| k.trim()).filter(|k| !k.is_empty()) {
        if kw.chars().count() >= 3 {
            args.push(Value::Text(format!("\"{}\"", kw.replace('"', "\"\""))));
            parts.push(
                "items.id IN (SELECT rowid FROM items_fts WHERE items_fts MATCH ?)".to_string(),
            );
        } else {
            let pattern = format!(
                "%{}%",
                kw.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_")
            );
            args.push(Value::Text(pattern.clone()));
            args.push(Value::Text(pattern));
            parts.push(
                "(items.title LIKE ? ESCAPE '\\' OR items.excerpt LIKE ? ESCAPE '\\')".to_string(),
            );
        }
    }
    if parts.is_empty() {
        "0".to_string()
    } else {
        format!("({})", parts.join(" OR "))
    }
}

impl Store {
    pub fn upsert_items(
        &self,
        source: &Source,
        items: &[NewItem],
        now: &DateTime<Utc>,
    ) -> Result<usize> {
        let tx = self.conn.unchecked_transaction()?;
        let mut created = 0;
        for it in items {
            let inserted = tx.execute(
                "INSERT INTO items(source_id, source_name, category, url, title, excerpt, \
                 author, published_at, fetched_at, score) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10) \
                 ON CONFLICT(url) DO NOTHING",
                params![
                    source.id,
                    source.name,
                    source.category.as_str(),
                    it.url,
                    it.title,
                    it.excerpt,
                    it.author,
                    it.published_at.as_ref().map(ts),
                    ts(now),
                    it.score,
                ],
            )?;
            if inserted == 1 {
                let id = tx.last_insert_rowid();
                tx.execute(
                    "INSERT INTO items_fts(rowid, title, excerpt) VALUES (?1, ?2, ?3)",
                    params![id, it.title, it.excerpt],
                )?;
                created += 1;
            } else if let Some(score) = it.score {
                tx.execute(
                    "UPDATE items SET score = ?1 WHERE url = ?2",
                    params![score, it.url],
                )?;
            }
        }
        tx.commit()?;
        Ok(created)
    }

    pub fn query_items(&self, f: &ItemFilter) -> Result<Vec<Item>> {
        let mut wh: Vec<String> = Vec::new();
        let mut args: Vec<Value> = Vec::new();
        if let Some(c) = f.category {
            wh.push("items.category = ?".into());
            args.push(Value::Text(c.as_str().into()));
        }
        if let Some(id) = f.source_id {
            wh.push("items.source_id = ?".into());
            args.push(Value::Integer(id));
        }
        if let Some(since) = &f.since {
            wh.push("items.fetched_at >= ?".into());
            args.push(Value::Text(ts(since)));
        }
        if let Some(until) = &f.until {
            wh.push("items.fetched_at < ?".into());
            args.push(Value::Text(ts(until)));
        }
        if f.unread_only {
            wh.push("items.read = 0".into());
        }
        if f.starred_only {
            wh.push("items.starred = 1".into());
        }
        if let Some(topic_id) = f.topic_id {
            let topic = self
                .get_topic(topic_id)?
                .ok_or_else(|| Error::Invalid(format!("topic {topic_id} not found")))?;
            wh.push(keyword_clause(&topic.keywords, &mut args));
        }
        if let Some(text) = f.text.as_deref().map(str::trim).filter(|t| !t.is_empty()) {
            wh.push(keyword_clause(&[text.to_string()], &mut args));
        }
        let where_sql = if wh.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", wh.join(" AND "))
        };
        let order = match f.order {
            ItemOrder::Recent => {
                "COALESCE(items.published_at, items.fetched_at) DESC, items.id DESC"
            }
            ItemOrder::Score => {
                "items.score IS NULL, items.score DESC, items.fetched_at DESC, items.id DESC"
            }
        };
        let limit = if f.limit == 0 { 100 } else { f.limit };
        args.push(Value::Integer(limit as i64));
        args.push(Value::Integer(f.offset as i64));
        let sql =
            format!("SELECT {ITEM_COLS} FROM items {where_sql} ORDER BY {order} LIMIT ? OFFSET ?");
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt
            .query_map(params_from_iter(args), row_to_item)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn get_item(&self, id: i64) -> Result<Option<Item>> {
        Ok(self
            .conn
            .query_row(
                &format!("SELECT {ITEM_COLS} FROM items WHERE items.id = ?1"),
                params![id],
                row_to_item,
            )
            .optional()?)
    }

    pub fn set_read(&self, id: i64, read: bool) -> Result<()> {
        self.conn
            .execute("UPDATE items SET read = ?2 WHERE id = ?1", params![id, read])?;
        Ok(())
    }

    pub fn set_starred(&self, id: i64, starred: bool) -> Result<()> {
        self.conn.execute(
            "UPDATE items SET starred = ?2 WHERE id = ?1",
            params![id, starred],
        )?;
        Ok(())
    }

    pub fn set_ai_summary(&self, id: i64, summary: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE items SET ai_summary = ?2 WHERE id = ?1",
            params![id, summary],
        )?;
        Ok(())
    }

    /// `since` 以降に取得した記事を指標順に `limit` 件取り、そのうち要約がないものを返す。
    pub fn top_unsummarized(
        &self,
        category: Category,
        since: &DateTime<Utc>,
        limit: u32,
    ) -> Result<Vec<Item>> {
        let sql = format!(
            "SELECT * FROM (SELECT {ITEM_COLS} FROM items \
             WHERE items.category = ?1 AND items.fetched_at >= ?2 \
             ORDER BY items.score IS NULL, items.score DESC, items.fetched_at DESC, items.id DESC \
             LIMIT ?3) WHERE ai_summary IS NULL"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt
            .query_map(params![category.as_str(), ts(since), limit], row_to_item)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }
}
```

- [ ] **Step 5: テストが通ることを確認する**

Run: `cargo test -p infodesk-core store::items`
Expected: 8 passed。

`top_unsummarized` の外側SELECTで並び順が崩れてテストが失敗した場合は、外側に `ORDER BY score IS NULL, score DESC, fetched_at DESC, id DESC` を追加する。

- [ ] **Step 6: コミット**

```bash
git add crates/core
git commit -m "feat(core): add item storage with dedupe, filters, keyword matching"
```

---

### Task 4: 期間の計算と期間要約の保存

**Files:**
- Create: `crates/core/src/period.rs`, `crates/core/src/store/digests.rs`
- Modify: `crates/core/src/lib.rs` (`pub mod period;`), `crates/core/src/store/mod.rs` (`mod digests;` と `pub use digests::NewDigest;`)

**Interfaces:**
- Consumes: `model::{Period, Digest, DigestScope, DigestStatus}`, `col_parse`, `col_ts`, `conv_err`, `ts`
- Produces:
  - `period::period_start(period: Period, date: NaiveDate) -> NaiveDate`
  - `period::period_end(period: Period, start: NaiveDate) -> NaiveDate` (排他的な終了日)
  - `period::utc_range(period: Period, start: NaiveDate, tz: FixedOffset) -> (DateTime<Utc>, DateTime<Utc>)`
  - `store::NewDigest { period, period_start: NaiveDate, scope: DigestScope, body: String, item_ids: Vec<i64>, provider: String }`
  - `Store::save_digest_done(&self, d: &NewDigest, now: &DateTime<Utc>) -> Result<i64>`
  - `Store::mark_digest_attempt_failed(&self, period: Period, start: NaiveDate, scope: &DigestScope, now: &DateTime<Utc>) -> Result<DigestStatus>` (1回目と2回目は `Pending`、3回目で `Failed`。生成済みの行は変更せず `Done` を返す)
  - `Store::get_digest(&self, period: Period, start: NaiveDate, scope: &DigestScope) -> Result<Option<Digest>>`
  - `Store::list_digests(&self, period: Period, start: NaiveDate) -> Result<Vec<Digest>>`
  - `Store::digests_in_range(&self, period: Period, scope: &DigestScope, from: NaiveDate, to_exclusive: NaiveDate) -> Result<Vec<Digest>>` (生成済みのみ、開始日順)
  - `Store::pending_digests(&self) -> Result<Vec<Digest>>`

- [ ] **Step 1: 失敗するテストを書く**

`crates/core/src/period.rs`:

```rust
use chrono::{DateTime, Datelike, Duration, FixedOffset, NaiveDate, TimeZone, Utc};

use crate::model::Period;

#[cfg(test)]
mod tests {
    use super::*;

    fn d(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    #[test]
    fn starts() {
        // 2026-10-06 は火曜日。
        assert_eq!(period_start(Period::Day, d("2026-10-06")), d("2026-10-06"));
        assert_eq!(period_start(Period::Week, d("2026-10-06")), d("2026-10-05"));
        assert_eq!(period_start(Period::Week, d("2026-10-05")), d("2026-10-05"));
        assert_eq!(period_start(Period::Week, d("2026-10-11")), d("2026-10-05"));
        assert_eq!(period_start(Period::Month, d("2026-10-06")), d("2026-10-01"));
    }

    #[test]
    fn ends_are_exclusive() {
        assert_eq!(period_end(Period::Day, d("2026-10-06")), d("2026-10-07"));
        assert_eq!(period_end(Period::Week, d("2026-10-05")), d("2026-10-12"));
        assert_eq!(period_end(Period::Month, d("2026-10-01")), d("2026-11-01"));
        assert_eq!(period_end(Period::Month, d("2026-12-01")), d("2027-01-01"));
    }

    #[test]
    fn utc_range_uses_local_midnight() {
        let jst = FixedOffset::east_opt(9 * 3600).unwrap();
        let (from, to) = utc_range(Period::Day, d("2026-10-06"), jst);
        assert_eq!(from.to_rfc3339(), "2026-10-05T15:00:00+00:00");
        assert_eq!(to.to_rfc3339(), "2026-10-06T15:00:00+00:00");
        let (from, to) = utc_range(Period::Month, d("2026-10-01"), jst);
        assert_eq!(from.to_rfc3339(), "2026-09-30T15:00:00+00:00");
        assert_eq!(to.to_rfc3339(), "2026-10-31T15:00:00+00:00");
    }
}
```

`crates/core/src/store/digests.rs`:

```rust
use chrono::{DateTime, NaiveDate, Utc};
use rusqlite::{params, OptionalExtension, Row};

use super::{col_parse, col_ts, conv_err, ts, Store};
use crate::error::Result;
use crate::model::{Digest, DigestScope, DigestStatus, Period};

#[derive(Debug, Clone)]
pub struct NewDigest {
    pub period: Period,
    pub period_start: NaiveDate,
    pub scope: DigestScope,
    pub body: String,
    pub item_ids: Vec<i64>,
    pub provider: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Category;
    use crate::testutil::at;

    fn d(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    fn nd(period: Period, start: &str, scope: DigestScope, body: &str, ids: &[i64]) -> NewDigest {
        NewDigest {
            period,
            period_start: d(start),
            scope,
            body: body.to_string(),
            item_ids: ids.to_vec(),
            provider: "mock".to_string(),
        }
    }

    #[test]
    fn save_get_and_overwrite() {
        let store = Store::open_in_memory().unwrap();
        let scope = DigestScope::Category(Category::Github);
        let now = at("2026-10-06T00:00:00Z");
        store
            .save_digest_done(&nd(Period::Day, "2026-10-06", scope.clone(), "v1", &[1, 2]), &now)
            .unwrap();
        store
            .save_digest_done(&nd(Period::Day, "2026-10-06", scope.clone(), "v2", &[3]), &now)
            .unwrap();
        let got = store.get_digest(Period::Day, d("2026-10-06"), &scope).unwrap().unwrap();
        assert_eq!(got.body, "v2");
        assert_eq!(got.item_ids, vec![3]);
        assert_eq!(got.status, DigestStatus::Done);
        assert_eq!(got.provider, "mock");
        assert!(store.get_digest(Period::Week, d("2026-10-05"), &scope).unwrap().is_none());
        assert_eq!(store.list_digests(Period::Day, d("2026-10-06")).unwrap().len(), 1);
    }

    #[test]
    fn failed_attempts_go_pending_then_failed_and_success_recovers() {
        let store = Store::open_in_memory().unwrap();
        let scope = DigestScope::Topic(1);
        let now = at("2026-10-06T00:00:00Z");
        let day = d("2026-10-06");
        assert_eq!(
            store.mark_digest_attempt_failed(Period::Day, day, &scope, &now).unwrap(),
            DigestStatus::Pending
        );
        assert_eq!(store.pending_digests().unwrap().len(), 1);
        assert_eq!(
            store.mark_digest_attempt_failed(Period::Day, day, &scope, &now).unwrap(),
            DigestStatus::Pending
        );
        assert_eq!(
            store.mark_digest_attempt_failed(Period::Day, day, &scope, &now).unwrap(),
            DigestStatus::Failed
        );
        assert!(store.pending_digests().unwrap().is_empty());
        assert_eq!(store.get_digest(Period::Day, day, &scope).unwrap().unwrap().attempts, 3);

        store
            .save_digest_done(&nd(Period::Day, "2026-10-06", scope.clone(), "ok", &[]), &now)
            .unwrap();
        assert_eq!(
            store.get_digest(Period::Day, day, &scope).unwrap().unwrap().status,
            DigestStatus::Done
        );
    }

    #[test]
    fn failure_does_not_downgrade_done_digest() {
        let store = Store::open_in_memory().unwrap();
        let scope = DigestScope::Category(Category::Arxiv);
        let now = at("2026-10-06T00:00:00Z");
        store
            .save_digest_done(&nd(Period::Day, "2026-10-06", scope.clone(), "keep", &[]), &now)
            .unwrap();
        let st = store
            .mark_digest_attempt_failed(Period::Day, d("2026-10-06"), &scope, &now)
            .unwrap();
        assert_eq!(st, DigestStatus::Done);
        let got = store.get_digest(Period::Day, d("2026-10-06"), &scope).unwrap().unwrap();
        assert_eq!(got.body, "keep");
    }

    #[test]
    fn range_returns_done_only_in_order_for_scope() {
        let store = Store::open_in_memory().unwrap();
        let gh = DigestScope::Category(Category::Github);
        let ax = DigestScope::Category(Category::Arxiv);
        let now = at("2026-10-06T00:00:00Z");
        for (day, scope, body) in [
            ("2026-10-07", gh.clone(), "d7"),
            ("2026-10-05", gh.clone(), "d5"),
            ("2026-10-12", gh.clone(), "next week"),
            ("2026-10-06", ax.clone(), "other scope"),
        ] {
            store.save_digest_done(&nd(Period::Day, day, scope, body, &[]), &now).unwrap();
        }
        store
            .mark_digest_attempt_failed(Period::Day, d("2026-10-06"), &gh, &now)
            .unwrap();
        let got = store
            .digests_in_range(Period::Day, &gh, d("2026-10-05"), d("2026-10-12"))
            .unwrap();
        let bodies: Vec<&str> = got.iter().map(|g| g.body.as_str()).collect();
        assert_eq!(bodies, vec!["d5", "d7"]);
    }
}
```

`crates/core/src/lib.rs` に `pub mod period;` を追加。`crates/core/src/store/mod.rs` のモジュール宣言に `mod digests;` を追加し、その下に `pub use digests::NewDigest;` を追加。

- [ ] **Step 2: テストが失敗することを確認する**

Run: `cargo test -p infodesk-core`
Expected: コンパイルエラー。`period_start`、`save_digest_done` などが未定義。

- [ ] **Step 3: 実装する**

`crates/core/src/period.rs` の `use` の下、テストの上に追加:

```rust
/// 日付を含む期間の開始日。週は月曜始まり。
pub fn period_start(period: Period, date: NaiveDate) -> NaiveDate {
    match period {
        Period::Day => date,
        Period::Week => date - Duration::days(date.weekday().num_days_from_monday() as i64),
        Period::Month => date.with_day(1).expect("day 1 always exists"),
    }
}

/// 期間の終了日 (その日を含まない)。`start` は `period_start` の戻り値を渡す。
pub fn period_end(period: Period, start: NaiveDate) -> NaiveDate {
    match period {
        Period::Day => start + Duration::days(1),
        Period::Week => start + Duration::days(7),
        Period::Month => {
            let (y, m) = if start.month() == 12 {
                (start.year() + 1, 1)
            } else {
                (start.year(), start.month() + 1)
            };
            NaiveDate::from_ymd_opt(y, m, 1).expect("first of month always exists")
        }
    }
}

fn local_midnight_utc(date: NaiveDate, tz: FixedOffset) -> DateTime<Utc> {
    let naive = date.and_hms_opt(0, 0, 0).expect("midnight always exists");
    tz.from_local_datetime(&naive)
        .single()
        .expect("fixed offset has no gaps")
        .with_timezone(&Utc)
}

/// 期間をUTCの半開区間 `[from, to)` に変換する。日付の境界は `tz` の0時。
pub fn utc_range(
    period: Period,
    start: NaiveDate,
    tz: FixedOffset,
) -> (DateTime<Utc>, DateTime<Utc>) {
    (
        local_midnight_utc(start, tz),
        local_midnight_utc(period_end(period, start), tz),
    )
}
```

`crates/core/src/store/digests.rs` の `NewDigest` の下、テストの上に追加:

```rust
const DIGEST_COLS: &str =
    "id, period, period_start, scope, body, item_ids, provider, status, attempts, created_at";

const MAX_ATTEMPTS: i64 = 3;

fn parse_date(s: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(s, "%Y-%m-%d").ok()
}

fn date_str(d: NaiveDate) -> String {
    d.format("%Y-%m-%d").to_string()
}

fn row_to_digest(r: &Row) -> rusqlite::Result<Digest> {
    let ids: String = r.get(5)?;
    Ok(Digest {
        id: r.get(0)?,
        period: col_parse(r, 1, Period::parse)?,
        period_start: col_parse(r, 2, parse_date)?,
        scope: col_parse(r, 3, DigestScope::parse)?,
        body: r.get(4)?,
        item_ids: serde_json::from_str(&ids).map_err(|e| conv_err(5, e.to_string()))?,
        provider: r.get(6)?,
        status: col_parse(r, 7, DigestStatus::parse)?,
        attempts: r.get(8)?,
        created_at: col_ts(r, 9)?,
    })
}

impl Store {
    pub fn save_digest_done(&self, d: &NewDigest, now: &DateTime<Utc>) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO digests(period, period_start, scope, body, item_ids, provider, status, created_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'done', ?7) \
             ON CONFLICT(period, period_start, scope) DO UPDATE SET \
               body = excluded.body, item_ids = excluded.item_ids, provider = excluded.provider, \
               status = 'done', created_at = excluded.created_at",
            params![
                d.period.as_str(),
                date_str(d.period_start),
                d.scope.key(),
                d.body,
                serde_json::to_string(&d.item_ids)?,
                d.provider,
                ts(now),
            ],
        )?;
        let id = self.conn.query_row(
            "SELECT id FROM digests WHERE period = ?1 AND period_start = ?2 AND scope = ?3",
            params![d.period.as_str(), date_str(d.period_start), d.scope.key()],
            |r| r.get(0),
        )?;
        Ok(id)
    }

    /// 生成の失敗を記録する。生成済みの行は変更しない。
    pub fn mark_digest_attempt_failed(
        &self,
        period: Period,
        start: NaiveDate,
        scope: &DigestScope,
        now: &DateTime<Utc>,
    ) -> Result<DigestStatus> {
        self.conn.execute(
            "INSERT INTO digests(period, period_start, scope, status, attempts, created_at) \
             VALUES (?1, ?2, ?3, 'pending', 1, ?4) \
             ON CONFLICT(period, period_start, scope) DO UPDATE SET \
               attempts = attempts + 1, \
               status = CASE WHEN attempts + 1 >= ?5 THEN 'failed' ELSE 'pending' END \
             WHERE digests.status != 'done'",
            params![period.as_str(), date_str(start), scope.key(), ts(now), MAX_ATTEMPTS],
        )?;
        let status = self.conn.query_row(
            "SELECT status FROM digests WHERE period = ?1 AND period_start = ?2 AND scope = ?3",
            params![period.as_str(), date_str(start), scope.key()],
            |r| col_parse(r, 0, DigestStatus::parse),
        )?;
        Ok(status)
    }

    pub fn get_digest(
        &self,
        period: Period,
        start: NaiveDate,
        scope: &DigestScope,
    ) -> Result<Option<Digest>> {
        Ok(self
            .conn
            .query_row(
                &format!(
                    "SELECT {DIGEST_COLS} FROM digests \
                     WHERE period = ?1 AND period_start = ?2 AND scope = ?3"
                ),
                params![period.as_str(), date_str(start), scope.key()],
                row_to_digest,
            )
            .optional()?)
    }

    pub fn list_digests(&self, period: Period, start: NaiveDate) -> Result<Vec<Digest>> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {DIGEST_COLS} FROM digests WHERE period = ?1 AND period_start = ?2 ORDER BY scope"
        ))?;
        let rows = stmt
            .query_map(params![period.as_str(), date_str(start)], row_to_digest)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn digests_in_range(
        &self,
        period: Period,
        scope: &DigestScope,
        from: NaiveDate,
        to_exclusive: NaiveDate,
    ) -> Result<Vec<Digest>> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {DIGEST_COLS} FROM digests \
             WHERE period = ?1 AND scope = ?2 AND status = 'done' \
               AND period_start >= ?3 AND period_start < ?4 \
             ORDER BY period_start"
        ))?;
        let rows = stmt
            .query_map(
                params![period.as_str(), scope.key(), date_str(from), date_str(to_exclusive)],
                row_to_digest,
            )?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn pending_digests(&self) -> Result<Vec<Digest>> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {DIGEST_COLS} FROM digests WHERE status = 'pending' ORDER BY period_start, scope"
        ))?;
        let rows = stmt
            .query_map([], row_to_digest)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }
}
```

- [ ] **Step 4: テストが通ることを確認する**

Run: `cargo test -p infodesk-core`
Expected: 全件 passed (period 3件、digests 4件を含む)。

- [ ] **Step 5: コミット**

```bash
git add crates/core
git commit -m "feat(core): add period math and digest storage"
```

---

### Task 5: RSS / Atom の解析、取得、検証

**Files:**
- Create: `crates/core/src/collector/mod.rs`, `crates/core/src/collector/text.rs`, `crates/core/src/collector/feed.rs`
- Modify: `crates/core/src/lib.rs` (`pub mod collector;`)

**Interfaces:**
- Consumes: `model::NewItem`, `error::{Error, Result}`
- Produces:
  - `collector::http_client() -> Result<reqwest::Client>` (User-Agent `InfoDesk/0.1`、タイムアウト30秒)
  - `collector::text::collapse_ws(&str) -> String`, `collector::text::strip_html(&str) -> String`, `collector::text::truncate_chars(&str, max: usize) -> String`
  - `collector::feed::parse_feed(bytes: &[u8]) -> Result<(Option<String>, Vec<NewItem>)>` (フィードのタイトルと記事。解析できなければ `Error::InvalidFeed`)
  - `collector::feed::fetch(http: &reqwest::Client, url: &str) -> Result<Vec<NewItem>>`
  - `collector::feed::FeedInfo { title: String, item_count: usize }`
  - `collector::feed::validate(http: &reqwest::Client, url: &str) -> Result<FeedInfo>` (RSS追加時の検証。spec 4節)

- [ ] **Step 1: 失敗するテストを書く**

`crates/core/src/lib.rs` に `pub mod collector;` を追加。

`crates/core/src/collector/mod.rs`:

```rust
pub mod feed;
pub mod text;

use std::time::Duration;

use crate::error::Result;

pub fn http_client() -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .user_agent("InfoDesk/0.1")
        .timeout(Duration::from_secs(30))
        .build()?)
}
```

`crates/core/src/collector/text.rs`:

```rust
use scraper::Html;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_tags_and_entities() {
        assert_eq!(
            strip_html("<p>本文 <b>太字</b>&amp;記号</p>\n<br> 次の行"),
            "本文 太字 &記号 次の行"
        );
        assert_eq!(strip_html("plain  text\n here"), "plain text here");
        assert_eq!(strip_html(""), "");
    }

    #[test]
    fn truncates_on_char_boundary() {
        assert_eq!(truncate_chars("あいうえお", 3), "あいう…");
        assert_eq!(truncate_chars("あいう", 3), "あいう");
        assert_eq!(truncate_chars("abc", 10), "abc");
    }
}
```

`crates/core/src/collector/feed.rs`:

```rust
use super::text::{strip_html, truncate_chars};
use crate::error::{Error, Result};
use crate::model::NewItem;

const EXCERPT_MAX_CHARS: usize = 500;

#[derive(Debug, Clone, PartialEq)]
pub struct FeedInfo {
    pub title: String,
    pub item_count: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collector::http_client;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const RSS: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<rss version="2.0"><channel><title>テストフィード</title>
<item><title>認知症の新薬 &amp; 治療</title><link>https://e.com/a</link>
  <description>&lt;p&gt;本文 &lt;b&gt;太字&lt;/b&gt;&lt;/p&gt;</description>
  <pubDate>Mon, 05 Oct 2026 10:00:00 +0900</pubDate><author>x@y.z (山田)</author></item>
<item><title>日付なし</title><link>https://e.com/b</link></item>
<item><title>リンクなし</title><description>捨てる</description></item>
<item><link>https://e.com/notitle</link></item>
</channel></rss>"#;

    const ATOM: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<feed xmlns="http://www.w3.org/2005/Atom"><title>Atom Feed</title>
<entry><id>http://arxiv.org/abs/2610.00001v1</id>
  <title>A Paper
   With Newline</title>
  <link href="http://arxiv.org/abs/2610.00001v1" rel="alternate" type="text/html"/>
  <link href="http://arxiv.org/pdf/2610.00001v1" rel="related" type="application/pdf"/>
  <summary>Abstract text.</summary><published>2026-10-05T01:00:00Z</published>
  <updated>2026-10-05T02:00:00Z</updated><author><name>Alice</name></author></entry>
</feed>"#;

    #[test]
    fn parses_rss_and_cleans_entries() {
        // Review Focus 3
        let (title, items) = parse_feed(RSS.as_bytes()).unwrap();
        assert_eq!(title.as_deref(), Some("テストフィード"));
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].url, "https://e.com/a");
        assert_eq!(items[0].title, "認知症の新薬 & 治療");
        assert_eq!(items[0].excerpt, "本文 太字");
        assert_eq!(
            items[0].published_at.unwrap().to_rfc3339(),
            "2026-10-05T01:00:00+00:00"
        );
        assert_eq!(items[0].score, None);
        assert_eq!(items[1].title, "日付なし");
        assert_eq!(items[1].published_at, None);
        assert_eq!(items[1].excerpt, "");
    }

    #[test]
    fn parses_atom_first_link_and_author() {
        let (title, items) = parse_feed(ATOM.as_bytes()).unwrap();
        assert_eq!(title.as_deref(), Some("Atom Feed"));
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].url, "http://arxiv.org/abs/2610.00001v1");
        assert_eq!(items[0].title, "A Paper With Newline");
        assert_eq!(items[0].excerpt, "Abstract text.");
        assert_eq!(items[0].author.as_deref(), Some("Alice"));
        assert_eq!(
            items[0].published_at.unwrap().to_rfc3339(),
            "2026-10-05T01:00:00+00:00"
        );
    }

    #[test]
    fn long_excerpt_is_truncated() {
        let body = "あ".repeat(600);
        let xml = format!(
            r#"<rss version="2.0"><channel><title>T</title><item><title>t</title><link>https://e.com/x</link><description>{body}</description></item></channel></rss>"#
        );
        let (_, items) = parse_feed(xml.as_bytes()).unwrap();
        assert_eq!(items[0].excerpt.chars().count(), 501);
    }

    #[test]
    fn non_feed_content_is_invalid_feed() {
        let err = parse_feed(b"<html><body>not a feed</body></html>").unwrap_err();
        assert!(matches!(err, Error::InvalidFeed(_)));
        assert!(matches!(parse_feed(b"").unwrap_err(), Error::InvalidFeed(_)));
    }

    #[tokio::test]
    async fn validate_accepts_rss_and_atom() {
        let server = MockServer::start().await;
        Mock::given(method("GET")).and(path("/rss"))
            .respond_with(ResponseTemplate::new(200).set_body_string(RSS))
            .mount(&server).await;
        Mock::given(method("GET")).and(path("/atom"))
            .respond_with(ResponseTemplate::new(200).set_body_string(ATOM))
            .mount(&server).await;
        let http = http_client().unwrap();
        let info = validate(&http, &format!("{}/rss", server.uri())).await.unwrap();
        assert_eq!(info, FeedInfo { title: "テストフィード".into(), item_count: 2 });
        let info = validate(&http, &format!("{}/atom", server.uri())).await.unwrap();
        assert_eq!(info.title, "Atom Feed");
        assert_eq!(fetch(&http, &format!("{}/rss", server.uri())).await.unwrap().len(), 2);
    }

    #[tokio::test]
    async fn validate_rejects_html_404_and_unreachable() {
        let server = MockServer::start().await;
        Mock::given(method("GET")).and(path("/html"))
            .respond_with(ResponseTemplate::new(200).set_body_string("<html></html>"))
            .mount(&server).await;
        Mock::given(method("GET")).and(path("/missing"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&server).await;
        let http = http_client().unwrap();
        let err = validate(&http, &format!("{}/html", server.uri())).await.unwrap_err();
        assert!(matches!(err, Error::InvalidFeed(_)));
        let err = validate(&http, &format!("{}/missing", server.uri())).await.unwrap_err();
        assert!(matches!(err, Error::Http(_)));
        let err = validate(&http, "http://127.0.0.1:1/feed").await.unwrap_err();
        assert!(matches!(err, Error::Http(_)));
        let err = validate(&http, "not a url").await.unwrap_err();
        assert!(matches!(err, Error::Http(_)));
    }

    #[tokio::test]
    async fn validate_uses_url_when_feed_has_no_title() {
        let server = MockServer::start().await;
        let xml = r#"<rss version="2.0"><channel><item><title>t</title><link>https://e.com/x</link></item></channel></rss>"#;
        Mock::given(method("GET")).and(path("/untitled"))
            .respond_with(ResponseTemplate::new(200).set_body_string(xml))
            .mount(&server).await;
        let url = format!("{}/untitled", server.uri());
        let info = validate(&http_client().unwrap(), &url).await.unwrap();
        assert_eq!(info.title, url);
    }
}
```

- [ ] **Step 2: テストが失敗することを確認する**

Run: `cargo test -p infodesk-core collector::`
Expected: コンパイルエラー。`strip_html`、`parse_feed`、`validate` などが未定義。

- [ ] **Step 3: 実装する**

`crates/core/src/collector/text.rs` の `use` の下、テストの上に追加:

```rust
/// 連続する空白と改行を1つの空白にまとめ、前後の空白を除く。
pub fn collapse_ws(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// HTMLタグを除き、実体参照を文字に戻す。
pub fn strip_html(s: &str) -> String {
    let fragment = Html::parse_fragment(s);
    let text = fragment
        .root_element()
        .text()
        .collect::<Vec<_>>()
        .join(" ");
    collapse_ws(&text)
}

/// `max` 文字を超える場合、`max` 文字で切って `…` を付ける。
pub fn truncate_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max).collect();
    out.push('…');
    out
}
```

`crates/core/src/collector/feed.rs` の `FeedInfo` の下、テストの上に追加:

```rust
/// RSS / Atom を解析する。リンクまたはタイトルのないエントリーは捨てる。
pub fn parse_feed(bytes: &[u8]) -> Result<(Option<String>, Vec<NewItem>)> {
    let feed = feed_rs::parser::parse(bytes).map_err(|e| Error::InvalidFeed(e.to_string()))?;
    let feed_title = feed
        .title
        .as_ref()
        .map(|t| strip_html(&t.content))
        .filter(|t| !t.is_empty());
    let mut items = Vec::new();
    for e in &feed.entries {
        let url = match e.links.first() {
            Some(link) => link.href.trim().to_string(),
            None if e.id.starts_with("http") => e.id.clone(),
            None => continue,
        };
        let title = e
            .title
            .as_ref()
            .map(|t| strip_html(&t.content))
            .unwrap_or_default();
        if url.is_empty() || title.is_empty() {
            continue;
        }
        let raw_excerpt = e
            .summary
            .as_ref()
            .map(|t| t.content.clone())
            .or_else(|| e.content.as_ref().and_then(|c| c.body.clone()))
            .unwrap_or_default();
        items.push(NewItem {
            url,
            title,
            excerpt: truncate_chars(&strip_html(&raw_excerpt), EXCERPT_MAX_CHARS),
            author: e.authors.first().and_then(|p| p.name.clone()),
            published_at: e.published.or(e.updated),
            score: None,
        });
    }
    Ok((feed_title, items))
}

async fn get_bytes(http: &reqwest::Client, url: &str) -> Result<Vec<u8>> {
    let resp = http.get(url).send().await?.error_for_status()?;
    Ok(resp.bytes().await?.to_vec())
}

pub async fn fetch(http: &reqwest::Client, url: &str) -> Result<Vec<NewItem>> {
    let bytes = get_bytes(http, url).await?;
    Ok(parse_feed(&bytes)?.1)
}

/// フィードを1回取得して検証する。タイトルがなければURLを表示名に使う。
pub async fn validate(http: &reqwest::Client, url: &str) -> Result<FeedInfo> {
    let bytes = get_bytes(http, url).await?;
    let (title, items) = parse_feed(&bytes)?;
    Ok(FeedInfo {
        title: title.unwrap_or_else(|| url.to_string()),
        item_count: items.len(),
    })
}
```

- [ ] **Step 4: テストが通ることを確認する**

Run: `cargo test -p infodesk-core collector::`
Expected: 9 passed。

`validate_rejects_html_404_and_unreachable` の `"not a url"` が `Error::Http` 以外になった場合は、実際のエラー変種に合わせてその1行のアサートを `assert!(validate(...).await.is_err())` に変える。他のアサートは変えない。

- [ ] **Step 5: コミット**

```bash
git add crates/core
git commit -m "feat(core): add RSS/Atom parsing, fetch, validation"
```

---

### Task 6: GitHub trending

**Files:**
- Create: `crates/core/src/collector/github.rs`, `crates/core/examples/fetch_fixtures.rs`, `crates/core/tests/fixtures/github_trending_daily.html` (exampleが生成)
- Modify: `crates/core/src/collector/mod.rs` (`pub mod github;`)

**Interfaces:**
- Consumes: `collector::text::collapse_ws`, `collector::http_client`, `model::NewItem`
- Produces:
  - `collector::github::parse(html: &str) -> Result<Vec<NewItem>>` (0件なら `Error::Parse`)
  - `collector::github::fetch(http: &reqwest::Client, base: &str, since: &str) -> Result<Vec<NewItem>>` (`since` は `daily|weekly|monthly`)
  - `collector::github::BASE: &str = "https://github.com"`

- [ ] **Step 1: 実レスポンスを保存するexampleを書いて実行する**

`crates/core/examples/fetch_fixtures.rs`:

```rust
//! 実レスポンスを tests/fixtures に保存する。パーサーのテストが読む。
//! 実行: cargo run -p infodesk-core --example fetch_fixtures

use std::path::Path;

const TARGETS: &[(&str, &str)] = &[
    ("github_trending_daily.html", "https://github.com/trending?since=daily"),
    (
        "arxiv_cs_ai.xml",
        "https://export.arxiv.org/api/query?search_query=cat:cs.AI&sortBy=submittedDate&sortOrder=descending&max_results=5",
    ),
    ("hf_models.json", "https://huggingface.co/api/models?sort=trendingScore&limit=5"),
    ("hf_papers.json", "https://huggingface.co/api/daily_papers?limit=5"),
    ("hn_topstories.json", "https://hacker-news.firebaseio.com/v0/topstories.json"),
];

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    std::fs::create_dir_all(&dir)?;
    let http = infodesk_core::collector::http_client()?;
    for (name, url) in TARGETS {
        let bytes = http.get(*url).send().await?.error_for_status()?.bytes().await?;
        std::fs::write(dir.join(name), &bytes)?;
        println!("{name}: {} bytes", bytes.len());
    }
    Ok(())
}
```

Run: `cargo run -p infodesk-core --example fetch_fixtures`
Expected: 5行出力される。各ファイルが0バイトより大きい。

- [ ] **Step 2: 失敗するテストを書く**

`crates/core/src/collector/mod.rs` の先頭に `pub mod github;` を追加。

`crates/core/src/collector/github.rs`:

```rust
use scraper::{Html, Selector};

use super::text::collapse_ws;
use crate::error::{Error, Result};
use crate::model::NewItem;

pub const BASE: &str = "https://github.com";

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"<html><body>
<article class="Box-row">
  <h2 class="h3 lh-condensed"><a href="/tester-army/e2e"> tester-army / e2e </a></h2>
  <p class="col-9 color-fg-muted my-1">End to end
     testing agent</p>
  <a href="/tester-army/e2e/stargazers">12,345</a>
  <span class="d-inline-block float-sm-right"> 1,430 stars today </span>
</article>
<article class="Box-row">
  <h2 class="h3 lh-condensed"><a href="/solo/repo">solo / repo</a></h2>
  <span class="d-inline-block float-sm-right"> 87 stars this week </span>
</article>
<article class="Box-row"><h2><a href="/not-a-repo">x</a></h2></article>
</body></html>"#;

    #[test]
    fn parses_repositories() {
        let items = parse(SAMPLE).unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].url, "https://github.com/tester-army/e2e");
        assert_eq!(items[0].title, "tester-army/e2e");
        assert_eq!(items[0].excerpt, "End to end testing agent");
        assert_eq!(items[0].score, Some(1430));
        assert_eq!(items[0].author.as_deref(), Some("tester-army"));
        assert_eq!(items[1].title, "solo/repo");
        assert_eq!(items[1].excerpt, "");
        assert_eq!(items[1].score, Some(87));
    }

    #[test]
    fn zero_repositories_is_an_error() {
        // Review Focus 4
        let err = parse("<html><body><div>redesigned page</div></body></html>").unwrap_err();
        assert!(matches!(err, Error::Parse(_)));
        assert!(matches!(parse("").unwrap_err(), Error::Parse(_)));
    }

    #[test]
    fn parses_saved_real_page() {
        let html = include_str!("../../tests/fixtures/github_trending_daily.html");
        let items = parse(html).unwrap();
        assert!(items.len() >= 5, "got {}", items.len());
        for it in &items {
            assert!(it.url.starts_with("https://github.com/"), "{}", it.url);
            assert_eq!(it.title.split('/').count(), 2, "{}", it.title);
        }
        assert!(items.iter().any(|i| i.score.is_some()));
    }
}
```

- [ ] **Step 3: テストが失敗することを確認する**

Run: `cargo test -p infodesk-core collector::github`
Expected: コンパイルエラー。`parse` が未定義。

- [ ] **Step 4: 実装する**

`crates/core/src/collector/github.rs` の `BASE` の下、テストの上に追加:

```rust
const STAR_MARKERS: [&str; 6] = [
    " stars today",
    " stars this week",
    " stars this month",
    " star today",
    " star this week",
    " star this month",
];

/// 本文中の `1,430 stars today` から数値を取り出す。
fn stars_in_period(text: &str) -> Option<i64> {
    for marker in STAR_MARKERS {
        if let Some(idx) = text.find(marker) {
            let number = text[..idx].rsplit(' ').next()?;
            return number.replace(',', "").parse().ok();
        }
    }
    None
}

pub fn parse(html: &str) -> Result<Vec<NewItem>> {
    let doc = Html::parse_document(html);
    let article = Selector::parse("article.Box-row").expect("valid selector");
    let link = Selector::parse("h2 a").expect("valid selector");
    let desc = Selector::parse("p").expect("valid selector");
    let mut out = Vec::new();
    for el in doc.select(&article) {
        let Some(href) = el.select(&link).next().and_then(|a| a.value().attr("href")) else {
            continue;
        };
        let repo = href.trim_matches('/');
        if repo.split('/').count() != 2 {
            continue;
        }
        let excerpt = el
            .select(&desc)
            .next()
            .map(|p| collapse_ws(&p.text().collect::<String>()))
            .unwrap_or_default();
        let text = collapse_ws(&el.text().collect::<Vec<_>>().join(" "));
        out.push(NewItem {
            url: format!("{BASE}/{repo}"),
            title: repo.to_string(),
            excerpt,
            author: repo.split('/').next().map(String::from),
            published_at: None,
            score: stars_in_period(&text),
        });
    }
    if out.is_empty() {
        return Err(Error::Parse(
            "github trending: no repositories found (page structure may have changed)".into(),
        ));
    }
    Ok(out)
}

pub async fn fetch(http: &reqwest::Client, base: &str, since: &str) -> Result<Vec<NewItem>> {
    let url = format!("{base}/trending?since={since}");
    let html = http.get(&url).send().await?.error_for_status()?.text().await?;
    parse(&html)
}
```

- [ ] **Step 5: テストが通ることを確認する**

Run: `cargo test -p infodesk-core collector::github`
Expected: 3 passed。

- [ ] **Step 6: コミット**

```bash
git add crates/core
git commit -m "feat(core): add GitHub trending collector"
```

---

### Task 7: Hacker News と Hugging Face

**Files:**
- Create: `crates/core/src/collector/hackernews.rs`, `crates/core/src/collector/huggingface.rs`
- Modify: `crates/core/src/collector/mod.rs` (`pub mod hackernews; pub mod huggingface;`)
- Uses: `crates/core/tests/fixtures/hf_models.json`, `hf_papers.json` (Task 6で保存済み)

**Interfaces:**
- Consumes: `collector::text::{collapse_ws, truncate_chars}`, `collector::http_client`, `model::NewItem`
- Produces:
  - `collector::hackernews::BASE: &str = "https://hacker-news.firebaseio.com/v0"`
  - `collector::hackernews::parse_item(bytes: &[u8]) -> Option<NewItem>`
  - `collector::hackernews::fetch(http: &reqwest::Client, base: &str, limit: usize) -> Result<Vec<NewItem>>`
  - `collector::huggingface::BASE: &str = "https://huggingface.co"`
  - `collector::huggingface::parse_models(bytes: &[u8]) -> Result<Vec<NewItem>>`, `parse_papers(bytes: &[u8]) -> Result<Vec<NewItem>>`
  - `collector::huggingface::fetch_models(http, base: &str, limit: usize) -> Result<Vec<NewItem>>`, `fetch_papers(http, base: &str, limit: usize) -> Result<Vec<NewItem>>`

- [ ] **Step 1: 失敗するテストを書く**

`crates/core/src/collector/mod.rs` の先頭のモジュール宣言を次にする:

```rust
pub mod feed;
pub mod github;
pub mod hackernews;
pub mod huggingface;
pub mod text;
```

`crates/core/src/collector/hackernews.rs`:

```rust
use chrono::DateTime;
use serde::Deserialize;

use crate::error::Result;
use crate::model::NewItem;

pub const BASE: &str = "https://hacker-news.firebaseio.com/v0";

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collector::http_client;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[test]
    fn parses_story_with_url() {
        let json = br#"{"by":"pg","descendants":3,"id":1,"kids":[15],"score":57,"time":1160418111,"title":"Y Combinator","type":"story","url":"http://ycombinator.com"}"#;
        let it = parse_item(json).unwrap();
        assert_eq!(it.url, "http://ycombinator.com");
        assert_eq!(it.title, "Y Combinator");
        assert_eq!(it.score, Some(57));
        assert_eq!(it.author.as_deref(), Some("pg"));
        assert_eq!(it.published_at.unwrap().timestamp(), 1160418111);
    }

    #[test]
    fn story_without_url_links_to_discussion() {
        let json = br#"{"by":"a","id":42,"score":5,"time":1,"title":"Ask HN: x","type":"story"}"#;
        assert_eq!(parse_item(json).unwrap().url, "https://news.ycombinator.com/item?id=42");
    }

    #[test]
    fn null_deleted_and_untitled_are_skipped() {
        assert!(parse_item(b"null").is_none());
        assert!(parse_item(br#"{"id":1,"deleted":true,"type":"story"}"#).is_none());
        assert!(parse_item(b"not json").is_none());
    }

    #[tokio::test]
    async fn fetch_takes_first_n_and_skips_broken_items() {
        let server = MockServer::start().await;
        Mock::given(method("GET")).and(path("/topstories.json"))
            .respond_with(ResponseTemplate::new(200).set_body_string("[1,2,3,4]"))
            .mount(&server).await;
        Mock::given(method("GET")).and(path("/item/1.json"))
            .respond_with(ResponseTemplate::new(200)
                .set_body_string(r#"{"id":1,"title":"one","score":10,"time":1,"type":"story"}"#))
            .mount(&server).await;
        Mock::given(method("GET")).and(path("/item/2.json"))
            .respond_with(ResponseTemplate::new(200).set_body_string("null"))
            .mount(&server).await;
        Mock::given(method("GET")).and(path("/item/3.json"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server).await;
        let items = fetch(&http_client().unwrap(), &server.uri(), 3).await.unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].title, "one");
    }

    #[tokio::test]
    async fn fetch_fails_when_list_fails() {
        let server = MockServer::start().await;
        Mock::given(method("GET")).and(path("/topstories.json"))
            .respond_with(ResponseTemplate::new(503))
            .mount(&server).await;
        assert!(fetch(&http_client().unwrap(), &server.uri(), 3).await.is_err());
    }
}
```

`crates/core/src/collector/huggingface.rs`:

```rust
use chrono::{DateTime, Utc};
use serde::Deserialize;

use super::text::{collapse_ws, truncate_chars};
use crate::error::Result;
use crate::model::NewItem;

pub const BASE: &str = "https://huggingface.co";

const EXCERPT_MAX_CHARS: usize = 500;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_models() {
        let json = br#"[{"_id":"x","id":"Cloudflare/clef","likes":1452,"trendingScore":1353,
            "private":false,"downloads":5416,"tags":["transformers"],"pipeline_tag":"text-generation",
            "createdAt":"2026-09-30T21:15:35.000Z","modelId":"Cloudflare/clef"},
            {"id":"bare/model"}]"#;
        let items = parse_models(json).unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].url, "https://huggingface.co/Cloudflare/clef");
        assert_eq!(items[0].title, "Cloudflare/clef");
        assert_eq!(items[0].score, Some(1353));
        assert_eq!(items[0].excerpt, "text-generation · likes 1452 · downloads 5416");
        assert_eq!(items[0].author.as_deref(), Some("Cloudflare"));
        assert_eq!(
            items[0].published_at.unwrap().to_rfc3339(),
            "2026-09-30T21:15:35+00:00"
        );
        assert_eq!(items[1].score, None);
        assert_eq!(items[1].excerpt, "");
    }

    #[test]
    fn parses_papers() {
        let json = br#"[{"paper":{"id":"2609.34826","upvotes":3,"title":"ignored"},
            "publishedAt":"2026-09-27T20:00:00.000Z","title":"A  Paper\n Title",
            "summary":"Line one.\nLine two.","numComments":0}]"#;
        let items = parse_papers(json).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].url, "https://huggingface.co/papers/2609.34826");
        assert_eq!(items[0].title, "A Paper Title");
        assert_eq!(items[0].excerpt, "Line one. Line two.");
        assert_eq!(items[0].score, Some(3));
    }

    #[test]
    fn invalid_json_is_an_error() {
        assert!(parse_models(b"<html>").is_err());
        assert!(parse_papers(b"{}").is_err());
    }

    #[test]
    fn parses_saved_real_responses() {
        let models = parse_models(include_bytes!("../../tests/fixtures/hf_models.json")).unwrap();
        assert!(!models.is_empty());
        assert!(models.iter().all(|m| m.url.starts_with("https://huggingface.co/")));
        let papers = parse_papers(include_bytes!("../../tests/fixtures/hf_papers.json")).unwrap();
        assert!(!papers.is_empty());
        assert!(papers.iter().all(|p| p.url.starts_with("https://huggingface.co/papers/")));
        assert!(papers.iter().all(|p| !p.title.is_empty()));
    }
}
```

- [ ] **Step 2: テストが失敗することを確認する**

Run: `cargo test -p infodesk-core collector::h`
Expected: コンパイルエラー。`parse_item`、`parse_models` などが未定義。

- [ ] **Step 3: 実装する**

`crates/core/src/collector/hackernews.rs` の `BASE` の下、テストの上に追加:

```rust
#[derive(Deserialize)]
struct HnItem {
    id: u64,
    title: Option<String>,
    url: Option<String>,
    score: Option<i64>,
    by: Option<String>,
    time: Option<i64>,
}

/// 記事1件のJSONを変換する。null、削除済み、タイトルなしは None。
pub fn parse_item(bytes: &[u8]) -> Option<NewItem> {
    let item: HnItem = serde_json::from_slice::<Option<HnItem>>(bytes).ok()??;
    let title = item.title.filter(|t| !t.trim().is_empty())?;
    Some(NewItem {
        url: item
            .url
            .filter(|u| !u.is_empty())
            .unwrap_or_else(|| format!("https://news.ycombinator.com/item?id={}", item.id)),
        title,
        excerpt: String::new(),
        author: item.by,
        published_at: item.time.and_then(|t| DateTime::from_timestamp(t, 0)),
        score: item.score,
    })
}

/// 上位 `limit` 件を取得する。個別の記事の取得失敗は飛ばす。
pub async fn fetch(http: &reqwest::Client, base: &str, limit: usize) -> Result<Vec<NewItem>> {
    let ids: Vec<u64> = http
        .get(format!("{base}/topstories.json"))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let mut out = Vec::new();
    for id in ids.into_iter().take(limit) {
        let resp = match http.get(format!("{base}/item/{id}.json")).send().await {
            Ok(r) if r.status().is_success() => r,
            _ => continue,
        };
        if let Ok(bytes) = resp.bytes().await {
            if let Some(item) = parse_item(&bytes) {
                out.push(item);
            }
        }
    }
    Ok(out)
}
```

`crates/core/src/collector/huggingface.rs` の定数の下、テストの上に追加:

```rust
#[derive(Deserialize)]
struct Model {
    id: String,
    likes: Option<i64>,
    #[serde(rename = "trendingScore")]
    trending_score: Option<f64>,
    downloads: Option<i64>,
    pipeline_tag: Option<String>,
    #[serde(rename = "createdAt")]
    created_at: Option<DateTime<Utc>>,
}

#[derive(Deserialize)]
struct PaperEntry {
    paper: PaperRef,
    title: String,
    summary: Option<String>,
    #[serde(rename = "publishedAt")]
    published_at: Option<DateTime<Utc>>,
}

#[derive(Deserialize)]
struct PaperRef {
    id: String,
    upvotes: Option<i64>,
}

pub fn parse_models(bytes: &[u8]) -> Result<Vec<NewItem>> {
    let models: Vec<Model> = serde_json::from_slice(bytes)?;
    Ok(models
        .into_iter()
        .map(|m| {
            let mut parts = Vec::new();
            if let Some(tag) = &m.pipeline_tag {
                parts.push(tag.clone());
            }
            if let Some(likes) = m.likes {
                parts.push(format!("likes {likes}"));
            }
            if let Some(downloads) = m.downloads {
                parts.push(format!("downloads {downloads}"));
            }
            NewItem {
                url: format!("{BASE}/{}", m.id),
                author: m.id.split_once('/').map(|(owner, _)| owner.to_string()),
                title: m.id,
                excerpt: parts.join(" · "),
                published_at: m.created_at,
                score: m.trending_score.map(|s| s.round() as i64),
            }
        })
        .collect())
}

pub fn parse_papers(bytes: &[u8]) -> Result<Vec<NewItem>> {
    let entries: Vec<PaperEntry> = serde_json::from_slice(bytes)?;
    Ok(entries
        .into_iter()
        .map(|e| NewItem {
            url: format!("{BASE}/papers/{}", e.paper.id),
            title: collapse_ws(&e.title),
            excerpt: truncate_chars(
                &collapse_ws(e.summary.as_deref().unwrap_or_default()),
                EXCERPT_MAX_CHARS,
            ),
            author: None,
            published_at: e.published_at,
            score: e.paper.upvotes,
        })
        .collect())
}

async fn get_bytes(http: &reqwest::Client, url: &str) -> Result<Vec<u8>> {
    Ok(http
        .get(url)
        .send()
        .await?
        .error_for_status()?
        .bytes()
        .await?
        .to_vec())
}

pub async fn fetch_models(http: &reqwest::Client, base: &str, limit: usize) -> Result<Vec<NewItem>> {
    let url = format!("{base}/api/models?sort=trendingScore&limit={limit}");
    parse_models(&get_bytes(http, &url).await?)
}

pub async fn fetch_papers(http: &reqwest::Client, base: &str, limit: usize) -> Result<Vec<NewItem>> {
    let url = format!("{base}/api/daily_papers?limit={limit}");
    parse_papers(&get_bytes(http, &url).await?)
}
```

- [ ] **Step 4: テストが通ることを確認する**

Run: `cargo test -p infodesk-core collector::h`
Expected: 9 passed (hackernews 5件、huggingface 4件)。

- [ ] **Step 5: コミット**

```bash
git add crates/core
git commit -m "feat(core): add Hacker News and Hugging Face collectors"
```

---

### Task 8: 収集の振り分け、失敗の隔離、初期ソース

**Files:**
- Create: `crates/core/src/seed.rs`
- Modify: `crates/core/src/collector/mod.rs` (`fetch_source`, `fetch_all`, `apply_outcomes` を追加), `crates/core/src/lib.rs` (`pub mod seed;`)
- Uses: `crates/core/tests/fixtures/arxiv_cs_ai.xml`

**Interfaces:**
- Consumes: Task 5〜7の各 `fetch` / `parse`、`Store::{list_sources, upsert_items, record_fetch, add_source, get_setting, set_setting}`
- Produces:
  - `collector::arxiv_url(category: &str) -> String`
  - `collector::fetch_source(http: &reqwest::Client, source: &Source) -> Result<Vec<NewItem>>`
  - `collector::FetchOutcome { pub source: Source, pub result: Result<Vec<NewItem>> }`
  - `collector::fetch_all(http: &reqwest::Client, sources: &[Source]) -> Vec<FetchOutcome>` (有効なソースだけ。失敗しても続行)
  - `collector::CollectReport { pub source_id: i64, pub name: String, pub new_items: usize, pub error: Option<String> }`
  - `collector::apply_outcomes(store: &Store, outcomes: &[FetchOutcome], now: &DateTime<Utc>) -> Result<Vec<CollectReport>>`
  - `seed::seed_defaults(store: &Store) -> Result<usize>` (追加した件数。2回目以降は0)
  - `seed::DEFAULT_SETTINGS: &[(&str, &str)]`

- [ ] **Step 1: 失敗するテストを書く**

`crates/core/src/lib.rs` に `pub mod seed;` を追加。

`crates/core/src/collector/mod.rs` の末尾に追加:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Category, ItemFilter, SourceKind};
    use crate::store::Store;
    use crate::testutil::at;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const RSS: &str = r#"<rss version="2.0"><channel><title>T</title>
<item><title>記事1</title><link>https://e.com/1</link></item>
<item><title>記事2</title><link>https://e.com/2</link></item></channel></rss>"#;

    #[test]
    fn arxiv_url_and_saved_response_parse() {
        assert_eq!(
            arxiv_url("cs.AI"),
            "https://export.arxiv.org/api/query?search_query=cat:cs.AI&sortBy=submittedDate&sortOrder=descending&max_results=50"
        );
        let (_, items) =
            feed::parse_feed(include_bytes!("../../tests/fixtures/arxiv_cs_ai.xml")).unwrap();
        assert!(!items.is_empty());
        assert!(items.iter().all(|i| i.url.contains("arxiv.org/abs/")), "{:?}", items[0].url);
        assert!(items.iter().all(|i| !i.title.contains('\n')));
        assert!(items.iter().all(|i| i.published_at.is_some()));
    }

    #[tokio::test]
    async fn one_failing_source_does_not_stop_the_others() {
        let server = MockServer::start().await;
        Mock::given(method("GET")).and(path("/ok"))
            .respond_with(ResponseTemplate::new(200).set_body_string(RSS))
            .mount(&server).await;
        Mock::given(method("GET")).and(path("/broken"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server).await;

        let store = Store::open_in_memory().unwrap();
        let broken = store
            .add_source(SourceKind::Rss, "Broken", Category::TechJp, &format!("{}/broken", server.uri()))
            .unwrap();
        let ok = store
            .add_source(SourceKind::Rss, "Ok", Category::TechJp, &format!("{}/ok", server.uri()))
            .unwrap();
        let disabled = store
            .add_source(SourceKind::Rss, "Off", Category::TechJp, &format!("{}/off", server.uri()))
            .unwrap();
        store.set_source_enabled(disabled, false).unwrap();

        let http = http_client().unwrap();
        let outcomes = fetch_all(&http, &store.list_sources().unwrap()).await;
        assert_eq!(outcomes.len(), 2);

        let now = at("2026-10-05T01:00:00Z");
        let reports = apply_outcomes(&store, &outcomes, &now).unwrap();
        assert_eq!(reports.len(), 2);
        assert_eq!(reports[0].source_id, broken);
        assert!(reports[0].error.is_some());
        assert_eq!(reports[0].new_items, 0);
        assert_eq!(reports[1].source_id, ok);
        assert_eq!(reports[1].error, None);
        assert_eq!(reports[1].new_items, 2);

        let s = store.get_source(broken).unwrap().unwrap();
        assert!(s.last_error.is_some());
        assert_eq!(s.last_fetched_at, Some(now));
        let s = store.get_source(ok).unwrap().unwrap();
        assert_eq!(s.last_error, None);
        assert_eq!(s.last_fetched_at, Some(now));
        assert!(store.get_source(disabled).unwrap().unwrap().last_fetched_at.is_none());
        assert_eq!(store.query_items(&ItemFilter::default()).unwrap().len(), 2);

        // 2回目は新規0件。
        let outcomes = fetch_all(&http, &store.list_sources().unwrap()).await;
        let reports = apply_outcomes(&store, &outcomes, &now).unwrap();
        assert_eq!(reports[1].new_items, 0);
    }
}
```

`crates/core/src/seed.rs`:

```rust
use crate::error::Result;
use crate::model::{Category, SourceKind};
use crate::store::Store;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seeds_once_and_covers_every_category() {
        let store = Store::open_in_memory().unwrap();
        let added = seed_defaults(&store).unwrap();
        assert_eq!(added, DEFAULT_SOURCES.len());
        let sources = store.list_sources().unwrap();
        for c in Category::ALL {
            assert!(sources.iter().any(|s| s.category == c), "no source for {c:?}");
        }
        assert!(sources
            .iter()
            .filter(|s| s.kind == SourceKind::Rss)
            .all(|s| s.config.starts_with("http")));
        assert_eq!(store.get_setting("ollama.model").unwrap().as_deref(), Some("gemma4:26b"));

        assert_eq!(seed_defaults(&store).unwrap(), 0);
        assert_eq!(store.list_sources().unwrap().len(), DEFAULT_SOURCES.len());
    }

    #[test]
    fn reseeding_does_not_restore_deleted_sources_or_reset_settings() {
        let store = Store::open_in_memory().unwrap();
        seed_defaults(&store).unwrap();
        let first = store.list_sources().unwrap().remove(0);
        store.delete_source(first.id).unwrap();
        store.set_setting("ollama.model", "other").unwrap();
        seed_defaults(&store).unwrap();
        assert_eq!(store.list_sources().unwrap().len(), DEFAULT_SOURCES.len() - 1);
        assert_eq!(store.get_setting("ollama.model").unwrap().as_deref(), Some("other"));
    }
}
```

- [ ] **Step 2: テストが失敗することを確認する**

Run: `cargo test -p infodesk-core`
Expected: コンパイルエラー。`arxiv_url`、`fetch_all`、`apply_outcomes`、`seed_defaults`、`DEFAULT_SOURCES` が未定義。

- [ ] **Step 3: 実装する**

`crates/core/src/collector/mod.rs` の `http_client` の下、テストの上に追加。先頭の `use` も次に置き換える:

```rust
use std::time::Duration;

use chrono::{DateTime, Utc};

use crate::error::Result;
use crate::model::{NewItem, Source, SourceKind};
use crate::store::Store;
```

```rust
const HN_LIMIT: usize = 30;
const HF_LIMIT: usize = 30;

pub fn arxiv_url(category: &str) -> String {
    format!(
        "https://export.arxiv.org/api/query?search_query=cat:{category}&sortBy=submittedDate&sortOrder=descending&max_results=50"
    )
}

/// ソース1件を取得する。種類ごとの取得器に振り分ける。
pub async fn fetch_source(http: &reqwest::Client, source: &Source) -> Result<Vec<NewItem>> {
    match source.kind {
        SourceKind::Rss => feed::fetch(http, &source.config).await,
        SourceKind::Arxiv => feed::fetch(http, &arxiv_url(&source.config)).await,
        SourceKind::GithubTrending => github::fetch(http, github::BASE, &source.config).await,
        SourceKind::Hackernews => hackernews::fetch(http, hackernews::BASE, HN_LIMIT).await,
        SourceKind::HfModels => huggingface::fetch_models(http, huggingface::BASE, HF_LIMIT).await,
        SourceKind::HfPapers => huggingface::fetch_papers(http, huggingface::BASE, HF_LIMIT).await,
    }
}

pub struct FetchOutcome {
    pub source: Source,
    pub result: Result<Vec<NewItem>>,
}

/// 有効なソースを順に取得する。1件の失敗で他を止めない。DBには触れない。
pub async fn fetch_all(http: &reqwest::Client, sources: &[Source]) -> Vec<FetchOutcome> {
    let mut out = Vec::new();
    for source in sources.iter().filter(|s| s.enabled) {
        let result = fetch_source(http, source).await;
        out.push(FetchOutcome {
            source: source.clone(),
            result,
        });
    }
    out
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct CollectReport {
    pub source_id: i64,
    pub name: String,
    pub new_items: usize,
    pub error: Option<String>,
}

/// 取得結果をDBに保存し、ソースごとの成否を記録する。
pub fn apply_outcomes(
    store: &Store,
    outcomes: &[FetchOutcome],
    now: &DateTime<Utc>,
) -> Result<Vec<CollectReport>> {
    let mut reports = Vec::new();
    for outcome in outcomes {
        let (new_items, error) = match &outcome.result {
            Ok(items) => (store.upsert_items(&outcome.source, items, now)?, None),
            Err(e) => (0, Some(e.to_string())),
        };
        store.record_fetch(outcome.source.id, now, error.as_deref())?;
        reports.push(CollectReport {
            source_id: outcome.source.id,
            name: outcome.source.name.clone(),
            new_items,
            error,
        });
    }
    Ok(reports)
}
```

`crates/core/src/seed.rs` の `use` の下、テストの上に追加:

```rust
/// (種類, 表示名, カテゴリー, config)。URLは2026-10-06に取得と解析を確認した。
pub const DEFAULT_SOURCES: &[(SourceKind, &str, Category, &str)] = &[
    (SourceKind::GithubTrending, "GitHub Trending (日)", Category::Github, "daily"),
    (SourceKind::GithubTrending, "GitHub Trending (週)", Category::Github, "weekly"),
    (SourceKind::GithubTrending, "GitHub Trending (月)", Category::Github, "monthly"),
    (SourceKind::Arxiv, "arXiv cs.AI", Category::Arxiv, "cs.AI"),
    (SourceKind::Arxiv, "arXiv cs.CL", Category::Arxiv, "cs.CL"),
    (SourceKind::Arxiv, "arXiv cs.LG", Category::Arxiv, "cs.LG"),
    (SourceKind::HfModels, "Hugging Face Trending Models", Category::Huggingface, "trending"),
    (SourceKind::HfPapers, "Hugging Face Daily Papers", Category::Huggingface, "daily"),
    (SourceKind::Hackernews, "Hacker News", Category::Hackernews, "top"),
    (SourceKind::Rss, "はてなブックマーク IT", Category::TechJp, "https://b.hatena.ne.jp/hotentry/it.rss"),
    (SourceKind::Rss, "Publickey", Category::TechJp, "https://www.publickey1.jp/atom.xml"),
    (SourceKind::Rss, "ITmedia", Category::TechJp, "https://rss.itmedia.co.jp/rss/2.0/itmedia_all.xml"),
    (SourceKind::Rss, "Zenn", Category::TechJp, "https://zenn.dev/feed"),
    (SourceKind::Rss, "TechCrunch", Category::TechIntl, "https://techcrunch.com/feed/"),
    (SourceKind::Rss, "The Verge", Category::TechIntl, "https://www.theverge.com/rss/index.xml"),
    (SourceKind::Rss, "Ars Technica", Category::TechIntl, "https://feeds.arstechnica.com/arstechnica/index"),
    (SourceKind::Rss, "MIT Technology Review", Category::TechIntl, "https://www.technologyreview.com/feed/"),
    (SourceKind::Rss, "joint-kaigo.com", Category::Medcare, "https://www.joint-kaigo.com/feed/"),
    (SourceKind::Rss, "care-news.jp", Category::Medcare, "http://www.care-news.jp/media.rss"),
    (
        SourceKind::Rss,
        "Google News: 認知症",
        Category::Medcare,
        "https://news.google.com/rss/search?q=%E8%AA%8D%E7%9F%A5%E7%97%87&hl=ja&gl=JP&ceid=JP:ja",
    ),
    (
        SourceKind::Rss,
        "Google News: 介護",
        Category::Medcare,
        "https://news.google.com/rss/search?q=%E4%BB%8B%E8%AD%B7&hl=ja&gl=JP&ceid=JP:ja",
    ),
    (
        SourceKind::Rss,
        "Google News: dementia",
        Category::Medcare,
        "https://news.google.com/rss/search?q=dementia&hl=en-US&gl=US&ceid=US:en",
    ),
];

/// 設定のキーと既定値。
pub const DEFAULT_SETTINGS: &[(&str, &str)] = &[
    ("llm.items_provider", "ollama"),
    ("llm.digest_provider", "ollama"),
    ("ollama.base_url", "http://localhost:11434"),
    ("ollama.model", "gemma4:26b"),
    ("claude.bin", "claude"),
    ("summary.top_n", "20"),
];

const SEEDED_KEY: &str = "seeded";

/// 初回だけ初期ソースと既定設定を入れる。戻り値は追加したソースの件数。
pub fn seed_defaults(store: &Store) -> Result<usize> {
    if store.get_setting(SEEDED_KEY)?.is_some() {
        return Ok(0);
    }
    for (kind, name, category, config) in DEFAULT_SOURCES {
        store.add_source(*kind, name, *category, config)?;
    }
    for (key, value) in DEFAULT_SETTINGS {
        if store.get_setting(key)?.is_none() {
            store.set_setting(key, value)?;
        }
    }
    store.set_setting(SEEDED_KEY, "1")?;
    Ok(DEFAULT_SOURCES.len())
}
```

- [ ] **Step 4: テストが通ることを確認する**

Run: `cargo test -p infodesk-core`
Expected: 全件 passed。

- [ ] **Step 5: specの初期ソース表を実態に合わせる**

`docs/superpowers/specs/2026-10-06-infodesk-design.md` の4節の表で、医療介護の行を次に置き換える:

```markdown
| 医療介護 | joint-kaigo.com、care-news.jp、Google News検索 (認知症、介護、dementia) | RSS |
```

表の直後の段落を次に置き換える:

```markdown
この表のソースは2026-10-06に取得と解析を確認した。CBnews、ケアマネタイムス、PubMedは固定のRSS URLがないため初期ソースから外し、Google News検索のRSSで代替した。
```

3節の collector の「インターフェース」と「実装」の2行を次に置き換える:

```markdown
- インターフェース: 関数 `fetch_source(http, source)` が `Vec<NewItem>` を返す。ソースの種類 (`SourceKind`) で取得器に振り分ける。
- 実装: `github`、`hackernews`、`huggingface`、`feed` (RSS、Atom、arXiv) の各モジュール。
```

- [ ] **Step 6: コミット**

```bash
git add crates/core docs/superpowers/specs/2026-10-06-infodesk-design.md
git commit -m "feat(core): add collection dispatch, failure isolation, default sources"
```

---

### Task 9: LLMプロバイダー (Ollama、Claude)

**Files:**
- Create: `crates/core/src/llm/mod.rs`, `crates/core/src/llm/ollama.rs`, `crates/core/src/llm/claude.rs`
- Modify: `crates/core/src/lib.rs` (`pub mod llm;`)

**Interfaces:**
- Consumes: `Store::get_setting_or`, `error::{Error, Result}`
- Produces:
  - `llm::LlmProvider` trait: `fn name(&self) -> String`, `async fn complete(&self, system: &str, prompt: &str) -> Result<String>` (`Send + Sync`、`#[async_trait]`)
  - `llm::clean_output(raw: &str) -> Result<String>` (前後の空白を除く。空なら `Error::Llm`)
  - `llm::ollama::OllamaProvider::new(base_url: &str, model: &str) -> Result<OllamaProvider>`、`name()` は `ollama:<model>`
  - `llm::claude::ClaudeProvider::new(bin: impl Into<PathBuf>) -> ClaudeProvider`、`name()` は `claude`
  - `llm::LlmRole { Items, Digest }`
  - `llm::provider_from_settings(store: &Store, role: LlmRole) -> Result<Box<dyn LlmProvider>>`
  - テスト用 (cfg(test)): `llm::mock::MockProvider::ok(reply: &str)`, `MockProvider::failing(msg: &str)`, `MockProvider::calls(&self) -> Vec<(String, String)>` (system, prompt の履歴)

- [ ] **Step 1: 失敗するテストを書く**

`crates/core/src/lib.rs` に `pub mod llm;` を追加。

`crates/core/src/llm/mod.rs`:

```rust
pub mod claude;
pub mod ollama;

use async_trait::async_trait;

use crate::error::{Error, Result};
use crate::store::Store;

#[async_trait]
pub trait LlmProvider: Send + Sync {
    /// 期間要約の `provider` 列に記録する名前。
    fn name(&self) -> String;
    async fn complete(&self, system: &str, prompt: &str) -> Result<String>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LlmRole {
    Items,
    Digest,
}

#[cfg(test)]
pub(crate) mod mock {
    use std::sync::Mutex;

    use super::*;

    pub struct MockProvider {
        reply: std::result::Result<String, String>,
        calls: Mutex<Vec<(String, String)>>,
    }

    impl MockProvider {
        pub fn ok(reply: &str) -> Self {
            Self { reply: Ok(reply.to_string()), calls: Mutex::new(Vec::new()) }
        }

        pub fn failing(msg: &str) -> Self {
            Self { reply: Err(msg.to_string()), calls: Mutex::new(Vec::new()) }
        }

        pub fn calls(&self) -> Vec<(String, String)> {
            self.calls.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl LlmProvider for MockProvider {
        fn name(&self) -> String {
            "mock".to_string()
        }

        async fn complete(&self, system: &str, prompt: &str) -> Result<String> {
            self.calls
                .lock()
                .unwrap()
                .push((system.to_string(), prompt.to_string()));
            self.reply.clone().map_err(Error::Llm)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_output_trims_and_rejects_empty() {
        // Review Focus 5
        assert_eq!(clean_output("  要約です。\n").unwrap(), "要約です。");
        assert!(matches!(clean_output(""), Err(Error::Llm(_))));
        assert!(matches!(clean_output(" \n\t "), Err(Error::Llm(_))));
    }

    #[test]
    fn provider_from_settings_selects_by_role() {
        let store = Store::open_in_memory().unwrap();
        // 設定がなければ既定のOllama。
        assert_eq!(
            provider_from_settings(&store, LlmRole::Items).unwrap().name(),
            "ollama:gemma4:26b"
        );
        store.set_setting("ollama.model", "other:1b").unwrap();
        store.set_setting("llm.digest_provider", "claude").unwrap();
        assert_eq!(
            provider_from_settings(&store, LlmRole::Items).unwrap().name(),
            "ollama:other:1b"
        );
        assert_eq!(provider_from_settings(&store, LlmRole::Digest).unwrap().name(), "claude");

        store.set_setting("llm.items_provider", "gpt").unwrap();
        assert!(matches!(
            provider_from_settings(&store, LlmRole::Items),
            Err(Error::Invalid(_))
        ));
    }
}
```

`crates/core/src/llm/ollama.rs`:

```rust
use std::time::Duration;

use async_trait::async_trait;
use serde_json::json;

use super::{clean_output, LlmProvider};
use crate::error::{Error, Result};

/// 既定の4096では長い入力が切り捨てられるため、明示する。
const NUM_CTX: u32 = 16384;

pub struct OllamaProvider {
    base_url: String,
    model: String,
    http: reqwest::Client,
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{body_partial_json, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn sends_chat_request_and_returns_content() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/chat"))
            .and(body_partial_json(json!({
                "model": "m1",
                "stream": false,
                "think": false,
                "messages": [
                    {"role": "system", "content": "sys"},
                    {"role": "user", "content": "hello"}
                ]
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "message": {"role": "assistant", "content": "  返答です。\n"},
                "done": true
            })))
            .mount(&server)
            .await;
        let p = OllamaProvider::new(&format!("{}/", server.uri()), "m1").unwrap();
        assert_eq!(p.name(), "ollama:m1");
        assert_eq!(p.complete("sys", "hello").await.unwrap(), "返答です。");
    }

    #[tokio::test]
    async fn error_status_empty_content_and_unreachable_are_errors() {
        let server = MockServer::start().await;
        Mock::given(method("POST")).and(path("/api/chat"))
            .respond_with(ResponseTemplate::new(404).set_body_json(json!({"error": "model not found"})))
            .mount(&server).await;
        let p = OllamaProvider::new(&server.uri(), "missing").unwrap();
        let err = p.complete("s", "p").await.unwrap_err().to_string();
        assert!(err.contains("model not found"), "{err}");

        let server = MockServer::start().await;
        Mock::given(method("POST")).and(path("/api/chat"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"message": {"content": "  "}})))
            .mount(&server).await;
        let p = OllamaProvider::new(&server.uri(), "m").unwrap();
        assert!(matches!(p.complete("s", "p").await, Err(Error::Llm(_))));

        let p = OllamaProvider::new("http://127.0.0.1:1", "m").unwrap();
        assert!(p.complete("s", "p").await.is_err());
    }
}
```

`crates/core/src/llm/claude.rs`:

```rust
use std::path::PathBuf;
use std::process::Stdio;

use async_trait::async_trait;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

use super::{clean_output, LlmProvider};
use crate::error::{Error, Result};

pub struct ClaudeProvider {
    bin: PathBuf,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    /// 引数と標準入力を記録し、指定のJSONを返す偽の claude コマンドを作る。
    fn fake_claude(dir: &std::path::Path, stdout_json: &str, exit_code: i32) -> PathBuf {
        let bin = dir.join("claude");
        let script = format!(
            "#!/bin/sh\nprintf '%s\\n' \"$@\" > \"{d}/args.txt\"\ncat > \"{d}/stdin.txt\"\nprintf '%s' '{out}'\nexit {code}\n",
            d = dir.display(),
            out = stdout_json,
            code = exit_code
        );
        std::fs::write(&bin, script).unwrap();
        std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
        bin
    }

    #[tokio::test]
    async fn passes_flags_and_prompt_and_returns_result() {
        let dir = tempfile::tempdir().unwrap();
        let bin = fake_claude(dir.path(), r#"{"is_error":false,"result":" 要約です。 "}"#, 0);
        let p = ClaudeProvider::new(bin);
        assert_eq!(p.name(), "claude");
        assert_eq!(p.complete("システム指示", "本文プロンプト").await.unwrap(), "要約です。");

        let args = std::fs::read_to_string(dir.path().join("args.txt")).unwrap();
        let args: Vec<&str> = args.lines().collect();
        assert_eq!(
            args,
            vec![
                "-p",
                "--tools",
                "",
                "--output-format",
                "json",
                "--no-session-persistence",
                "--setting-sources",
                "project",
                "--system-prompt",
                "システム指示",
            ]
        );
        let stdin = std::fs::read_to_string(dir.path().join("stdin.txt")).unwrap();
        assert_eq!(stdin, "本文プロンプト");
    }

    #[tokio::test]
    async fn error_flag_nonzero_exit_garbage_and_missing_binary_are_errors() {
        let dir = tempfile::tempdir().unwrap();
        let bin = fake_claude(dir.path(), r#"{"is_error":true,"result":"Not logged in"}"#, 0);
        let err = ClaudeProvider::new(bin).complete("s", "p").await.unwrap_err().to_string();
        assert!(err.contains("Not logged in"), "{err}");

        let dir = tempfile::tempdir().unwrap();
        let bin = fake_claude(dir.path(), r#"{"is_error":false,"result":"x"}"#, 1);
        assert!(ClaudeProvider::new(bin).complete("s", "p").await.is_err());

        let dir = tempfile::tempdir().unwrap();
        let bin = fake_claude(dir.path(), "not json", 0);
        assert!(matches!(
            ClaudeProvider::new(bin).complete("s", "p").await,
            Err(Error::Llm(_))
        ));

        let dir = tempfile::tempdir().unwrap();
        let bin = fake_claude(dir.path(), r#"{"is_error":false,"result":"  "}"#, 0);
        assert!(matches!(
            ClaudeProvider::new(bin).complete("s", "p").await,
            Err(Error::Llm(_))
        ));

        let p = ClaudeProvider::new("/nonexistent/claude-binary");
        assert!(p.complete("s", "p").await.is_err());
    }
}
```

- [ ] **Step 2: テストが失敗することを確認する**

Run: `cargo test -p infodesk-core llm::`
Expected: コンパイルエラー。`clean_output`、`provider_from_settings`、`OllamaProvider::new`、`ClaudeProvider::new` が未定義。

- [ ] **Step 3: 実装する**

`crates/core/src/llm/mod.rs` の `LlmRole` の下、`mock` モジュールの上に追加:

```rust
/// LLMの出力を整える。空の出力は失敗として扱う。
pub fn clean_output(raw: &str) -> Result<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(Error::Llm("empty response".into()));
    }
    Ok(trimmed.to_string())
}

/// 設定から、用途に対応するプロバイダーを作る。
/// 設定キー: `llm.items_provider` / `llm.digest_provider` (`ollama` または `claude`)、
/// `ollama.base_url`、`ollama.model`、`claude.bin`。
pub fn provider_from_settings(store: &Store, role: LlmRole) -> Result<Box<dyn LlmProvider>> {
    let key = match role {
        LlmRole::Items => "llm.items_provider",
        LlmRole::Digest => "llm.digest_provider",
    };
    match store.get_setting_or(key, "ollama")?.as_str() {
        "ollama" => Ok(Box::new(ollama::OllamaProvider::new(
            &store.get_setting_or("ollama.base_url", "http://localhost:11434")?,
            &store.get_setting_or("ollama.model", "gemma4:26b")?,
        )?)),
        "claude" => Ok(Box::new(claude::ClaudeProvider::new(
            store.get_setting_or("claude.bin", "claude")?,
        ))),
        other => Err(Error::Invalid(format!("{key}: unknown provider '{other}'"))),
    }
}
```

`crates/core/src/llm/ollama.rs` の `OllamaProvider` の下、テストの上に追加:

```rust
impl OllamaProvider {
    pub fn new(base_url: &str, model: &str) -> Result<Self> {
        Ok(Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            model: model.to_string(),
            // ローカルLLMの生成は長い。モデルの読み込みも含めて待つ。
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(600))
                .build()?,
        })
    }
}

#[async_trait]
impl LlmProvider for OllamaProvider {
    fn name(&self) -> String {
        format!("ollama:{}", self.model)
    }

    async fn complete(&self, system: &str, prompt: &str) -> Result<String> {
        let body = json!({
            "model": self.model,
            "stream": false,
            "think": false,
            "options": {"num_ctx": NUM_CTX},
            "messages": [
                {"role": "system", "content": system},
                {"role": "user", "content": prompt}
            ]
        });
        let resp = self
            .http
            .post(format!("{}/api/chat", self.base_url))
            .json(&body)
            .send()
            .await?;
        let status = resp.status();
        let value: serde_json::Value = resp.json().await?;
        if !status.is_success() {
            let detail = value["error"].as_str().unwrap_or("unknown error");
            return Err(Error::Llm(format!("ollama {status}: {detail}")));
        }
        clean_output(value["message"]["content"].as_str().unwrap_or_default())
    }
}
```

`crates/core/src/llm/claude.rs` の `ClaudeProvider` の下、テストの上に追加:

```rust
impl ClaudeProvider {
    pub fn new(bin: impl Into<PathBuf>) -> Self {
        Self { bin: bin.into() }
    }
}

#[async_trait]
impl LlmProvider for ClaudeProvider {
    fn name(&self) -> String {
        "claude".to_string()
    }

    /// `claude -p` をツールなしで1回実行する。
    /// 作業ディレクトリを一時ディレクトリにし、`--setting-sources project` で
    /// 利用者のグローバル設定 (フックなど) が出力に混ざることを防ぐ。
    async fn complete(&self, system: &str, prompt: &str) -> Result<String> {
        let mut child = Command::new(&self.bin)
            .args([
                "-p",
                "--tools",
                "",
                "--output-format",
                "json",
                "--no-session-persistence",
                "--setting-sources",
                "project",
                "--system-prompt",
                system,
            ])
            .current_dir(std::env::temp_dir())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        if let Some(mut stdin) = child.stdin.take() {
            stdin.write_all(prompt.as_bytes()).await?;
            stdin.shutdown().await?;
        }
        let out = child.wait_with_output().await?;
        let value: serde_json::Value = serde_json::from_slice(&out.stdout).map_err(|e| {
            Error::Llm(format!(
                "claude: unparsable output ({e}): {}",
                String::from_utf8_lossy(&out.stderr).trim()
            ))
        })?;
        let result = value["result"].as_str().unwrap_or_default();
        if value["is_error"].as_bool().unwrap_or(false) || !out.status.success() {
            return Err(Error::Llm(format!("claude: {result}")));
        }
        clean_output(result)
    }
}
```

- [ ] **Step 4: テストが通ることを確認する**

Run: `cargo test -p infodesk-core llm::`
Expected: 6 passed。

- [ ] **Step 5: コミット**

```bash
git add crates/core
git commit -m "feat(core): add Ollama and Claude LLM providers"
```

---

### Task 10: 記事要約と階層式の期間要約

**Files:**
- Create: `crates/core/src/summarizer/mod.rs`, `crates/core/src/summarizer/prompts.rs`
- Modify: `crates/core/src/lib.rs` (`pub mod summarizer;`)

**Interfaces:**
- Consumes: `Store::{query_items, top_unsummarized, set_ai_summary, get_topic, get_digest, digests_in_range, save_digest_done, mark_digest_attempt_failed}`, `store::NewDigest`, `period::{period_start, period_end, utc_range}`, `llm::{LlmProvider, clean_output}`, `collector::text::truncate_chars`
- Produces:
  - `summarizer::DigestJob { pub period: Period, pub period_start: NaiveDate, pub scope: DigestScope, pub system: String, pub prompt: String, pub item_ids: Vec<i64> }`
  - `summarizer::plan_digest(store: &Store, period: Period, date: NaiveDate, scope: &DigestScope, tz: FixedOffset, force: bool) -> Result<Option<DigestJob>>` (同期。入力がない、または生成済みで `force` が偽なら `None`)
  - `summarizer::finish_digest(store: &Store, job: &DigestJob, provider: &str, output: Result<String>, now: &DateTime<Utc>) -> Result<DigestStatus>` (同期)
  - `summarizer::run_digest(store: &Store, llm: &dyn LlmProvider, period: Period, date: NaiveDate, scope: &DigestScope, tz: FixedOffset, force: bool, now: &DateTime<Utc>) -> Result<Option<DigestStatus>>`
  - `summarizer::summarize_top_items(store: &Store, llm: &dyn LlmProvider, category: Category, since: &DateTime<Utc>, limit: u32) -> Result<(usize, usize)>` (成功件数、失敗件数)
  - `summarizer::prompts::{ITEM_SYSTEM, DIGEST_SYSTEM, item_prompt(&Item) -> String, digest_prompt(label: &str, period: Period, lines: &[String]) -> String}`

Plan 2のTauriコマンドは `Send` なFutureを要求する。`Store` (rusqliteの接続) は `Sync` でないため、`run_digest` と `summarize_top_items` はCLI用の便利関数である。Plan 2は `plan_digest` (同期)、`llm.complete` (非同期)、`finish_digest` (同期) を別々に呼ぶ。

- [ ] **Step 1: 失敗するテストを書く**

`crates/core/src/lib.rs` に `pub mod summarizer;` を追加。

`crates/core/src/summarizer/prompts.rs`:

```rust
use crate::collector::text::truncate_chars;
use crate::model::{Item, Period};

pub const ITEM_SYSTEM: &str = "あなたは技術と医療介護のニュースを要約する編集者です。\
入力された記事を日本語で1〜2文に要約します。英語の記事は日本語に訳して要約します。\
要約文だけを出力し、前置き、見出し、記号を付けません。入力にない事実を加えません。";

pub const DIGEST_SYSTEM: &str = "あなたは情報収集アシスタントです。\
与えられた記事一覧、または下位期間の要約から、指定された期間の動向を日本語でまとめます。\
重要な話題を3〜6個の箇条書き (各1〜2文) にし、最後に全体の傾向を1文で述べます。\
入力にない事実を加えません。前置きを付けません。";

const ITEM_EXCERPT_MAX_CHARS: usize = 400;

pub fn item_prompt(item: &Item) -> String {
    format!(
        "タイトル: {}\nソース: {}\n概要: {}",
        item.title,
        item.source_name,
        truncate_chars(&item.excerpt, ITEM_EXCERPT_MAX_CHARS)
    )
}

pub fn digest_prompt(label: &str, period: Period, lines: &[String]) -> String {
    format!(
        "対象: {label}\n期間: {}\n\n入力:\n{}",
        period.label_ja(),
        lines.join("\n")
    )
}
```

`crates/core/src/summarizer/mod.rs`:

```rust
pub mod prompts;

use std::collections::BTreeSet;

use chrono::{DateTime, FixedOffset, NaiveDate, Utc};

use crate::collector::text::truncate_chars;
use crate::error::{Error, Result};
use crate::llm::{clean_output, LlmProvider};
use crate::model::{
    Category, DigestScope, DigestStatus, Item, ItemFilter, ItemOrder, Period,
};
use crate::period::{period_end, period_start, utc_range};
use crate::store::{NewDigest, Store};

/// 日次のカテゴリー要約に入れる記事の上限。
const CATEGORY_ITEM_LIMIT: u32 = 60;
/// トピック別要約に入れる記事の上限。
const TOPIC_ITEM_LIMIT: u32 = 80;
const LINE_DETAIL_MAX_CHARS: usize = 200;

#[derive(Debug, Clone)]
pub struct DigestJob {
    pub period: Period,
    pub period_start: NaiveDate,
    pub scope: DigestScope,
    pub system: String,
    pub prompt: String,
    pub item_ids: Vec<i64>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::mock::MockProvider;
    use crate::model::Source;
    use crate::testutil::{at, new_item, store_with_source};

    fn jst() -> FixedOffset {
        FixedOffset::east_opt(9 * 3600).unwrap()
    }

    fn d(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    const NOW: &str = "2026-10-06T22:00:00Z";

    /// JSTの10-06に2件 (指標 5 と 50)、10-05に1件を入れる。
    fn seeded() -> (Store, Source) {
        let (store, src) = store_with_source(Category::Github);
        store
            .upsert_items(&src, &[new_item("https://e.com/prev", "前日の記事", "", Some(999))], &at("2026-10-05T14:00:00Z"))
            .unwrap();
        store
            .upsert_items(
                &src,
                &[
                    new_item("https://e.com/low", "低スコア記事", "概要A", Some(5)),
                    new_item("https://e.com/high", "高スコア記事", "概要B", Some(50)),
                ],
                &at("2026-10-05T16:00:00Z"),
            )
            .unwrap();
        (store, src)
    }

    #[tokio::test]
    async fn item_summaries_are_saved_for_top_items() {
        let (store, _) = seeded();
        let llm = MockProvider::ok(" 一文要約。 ");
        let (ok, failed) =
            summarize_top_items(&store, &llm, Category::Github, &at("2026-10-05T15:00:00Z"), 1)
                .await
                .unwrap();
        assert_eq!((ok, failed), (1, 0));
        let calls = llm.calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, prompts::ITEM_SYSTEM);
        assert!(calls[0].1.contains("高スコア記事"));
        let items = store
            .query_items(&ItemFilter { text: Some("高スコア記事".into()), ..Default::default() })
            .unwrap();
        assert_eq!(items[0].ai_summary.as_deref(), Some("一文要約。"));
    }

    #[tokio::test]
    async fn item_summary_failure_stops_early_and_leaves_items_for_retry() {
        let (store, _) = seeded();
        let llm = MockProvider::failing("connection refused");
        let (ok, failed) =
            summarize_top_items(&store, &llm, Category::Github, &at("2026-10-05T15:00:00Z"), 10)
                .await
                .unwrap();
        assert_eq!((ok, failed), (0, 2));
        assert_eq!(llm.calls().len(), 1);
        assert_eq!(
            store.top_unsummarized(Category::Github, &at("2026-10-05T15:00:00Z"), 10).unwrap().len(),
            2
        );

        // Review Focus 5: 空の出力は保存しない。
        let llm = MockProvider::ok("   ");
        let (ok, failed) =
            summarize_top_items(&store, &llm, Category::Github, &at("2026-10-05T15:00:00Z"), 10)
                .await
                .unwrap();
        assert_eq!((ok, failed), (0, 2));
    }

    #[tokio::test]
    async fn day_digest_uses_items_of_that_local_day_in_score_order() {
        let (store, _) = seeded();
        let high = store
            .query_items(&ItemFilter { text: Some("高スコア記事".into()), ..Default::default() })
            .unwrap()[0]
            .id;
        store.set_ai_summary(high, "AI要約B").unwrap();

        let llm = MockProvider::ok("日次まとめ");
        let scope = DigestScope::Category(Category::Github);
        let st = run_digest(&store, &llm, Period::Day, d("2026-10-06"), &scope, jst(), false, &at(NOW))
            .await
            .unwrap();
        assert_eq!(st, Some(DigestStatus::Done));

        let (system, prompt) = llm.calls().remove(0);
        assert_eq!(system, prompts::DIGEST_SYSTEM);
        assert!(prompt.contains("対象: GitHub"));
        assert!(prompt.contains("期間: 今日"));
        assert!(!prompt.contains("前日の記事"));
        assert!(prompt.contains("AI要約B"));
        assert!(!prompt.contains("概要B"));
        assert!(prompt.contains("概要A"));
        assert!(prompt.find("高スコア記事").unwrap() < prompt.find("低スコア記事").unwrap());

        let saved = store.get_digest(Period::Day, d("2026-10-06"), &scope).unwrap().unwrap();
        assert_eq!(saved.body, "日次まとめ");
        assert_eq!(saved.provider, "mock");
        assert_eq!(saved.item_ids.len(), 2);
        assert_eq!(saved.item_ids[0], high);
    }

    #[tokio::test]
    async fn no_input_means_no_llm_call_and_no_digest() {
        let (store, _) = seeded();
        let llm = MockProvider::ok("x");
        let scope = DigestScope::Category(Category::Arxiv);
        let st = run_digest(&store, &llm, Period::Day, d("2026-10-06"), &scope, jst(), false, &at(NOW))
            .await
            .unwrap();
        assert_eq!(st, None);
        // 日次要約がまだない週次も同じ。
        let scope = DigestScope::Category(Category::Github);
        let st = run_digest(&store, &llm, Period::Week, d("2026-10-06"), &scope, jst(), false, &at(NOW))
            .await
            .unwrap();
        assert_eq!(st, None);
        assert!(llm.calls().is_empty());
        assert!(store.get_digest(Period::Week, d("2026-10-05"), &scope).unwrap().is_none());
    }

    #[tokio::test]
    async fn week_uses_day_digests_and_month_uses_week_digests() {
        let (store, _) = seeded();
        let scope = DigestScope::Category(Category::Github);
        for (day, body) in [("2026-10-05", "月曜のまとめ"), ("2026-10-06", "火曜のまとめ")] {
            let llm = MockProvider::ok(body);
            run_digest(&store, &llm, Period::Day, d(day), &scope, jst(), false, &at(NOW))
                .await
                .unwrap();
        }

        let llm = MockProvider::ok("週次まとめ");
        let st = run_digest(&store, &llm, Period::Week, d("2026-10-08"), &scope, jst(), false, &at(NOW))
            .await
            .unwrap();
        assert_eq!(st, Some(DigestStatus::Done));
        let prompt = llm.calls().remove(0).1;
        assert!(prompt.contains("期間: 今週"));
        assert!(prompt.contains("月曜のまとめ") && prompt.contains("火曜のまとめ"));
        assert!(prompt.find("月曜のまとめ").unwrap() < prompt.find("火曜のまとめ").unwrap());
        assert!(!prompt.contains("高スコア記事"));
        let week = store.get_digest(Period::Week, d("2026-10-05"), &scope).unwrap().unwrap();
        assert_eq!(week.item_ids.len(), 3);

        let llm = MockProvider::ok("月次まとめ");
        let st = run_digest(&store, &llm, Period::Month, d("2026-10-20"), &scope, jst(), false, &at(NOW))
            .await
            .unwrap();
        assert_eq!(st, Some(DigestStatus::Done));
        let prompt = llm.calls().remove(0).1;
        assert!(prompt.contains("期間: 今月"));
        assert!(prompt.contains("週次まとめ"));
        assert!(!prompt.contains("月曜のまとめ"));
        let month = store.get_digest(Period::Month, d("2026-10-01"), &scope).unwrap().unwrap();
        assert_eq!(month.body, "月次まとめ");
        assert_eq!(month.item_ids.len(), 3);
    }

    #[tokio::test]
    async fn done_digest_is_reused_unless_forced() {
        let (store, _) = seeded();
        let scope = DigestScope::Category(Category::Github);
        let day = d("2026-10-06");
        run_digest(&store, &MockProvider::ok("v1"), Period::Day, day, &scope, jst(), false, &at(NOW))
            .await
            .unwrap();

        let llm = MockProvider::ok("v2");
        let st = run_digest(&store, &llm, Period::Day, day, &scope, jst(), false, &at(NOW)).await.unwrap();
        assert_eq!(st, None);
        assert!(llm.calls().is_empty());
        assert_eq!(store.get_digest(Period::Day, day, &scope).unwrap().unwrap().body, "v1");

        let st = run_digest(&store, &llm, Period::Day, day, &scope, jst(), true, &at(NOW)).await.unwrap();
        assert_eq!(st, Some(DigestStatus::Done));
        assert_eq!(store.get_digest(Period::Day, day, &scope).unwrap().unwrap().body, "v2");

        // 強制再生成が失敗しても、生成済みの本文は残る。
        let st = run_digest(&store, &MockProvider::failing("down"), Period::Day, day, &scope, jst(), true, &at(NOW))
            .await
            .unwrap();
        assert_eq!(st, Some(DigestStatus::Done));
        assert_eq!(store.get_digest(Period::Day, day, &scope).unwrap().unwrap().body, "v2");
    }

    #[tokio::test]
    async fn llm_failure_and_empty_output_leave_digest_pending_then_failed() {
        // Review Focus 5
        let (store, _) = seeded();
        let scope = DigestScope::Category(Category::Github);
        let day = d("2026-10-06");
        let st = run_digest(&store, &MockProvider::failing("down"), Period::Day, day, &scope, jst(), false, &at(NOW))
            .await
            .unwrap();
        assert_eq!(st, Some(DigestStatus::Pending));
        let st = run_digest(&store, &MockProvider::ok("  \n "), Period::Day, day, &scope, jst(), false, &at(NOW))
            .await
            .unwrap();
        assert_eq!(st, Some(DigestStatus::Pending));
        let saved = store.get_digest(Period::Day, day, &scope).unwrap().unwrap();
        assert_eq!(saved.body, "");
        assert_eq!(saved.attempts, 2);
        let st = run_digest(&store, &MockProvider::failing("down"), Period::Day, day, &scope, jst(), false, &at(NOW))
            .await
            .unwrap();
        assert_eq!(st, Some(DigestStatus::Failed));

        // 保留または失敗の後でも、成功すれば生成済みになる。
        let st = run_digest(&store, &MockProvider::ok("復旧"), Period::Day, day, &scope, jst(), false, &at(NOW))
            .await
            .unwrap();
        assert_eq!(st, Some(DigestStatus::Done));
    }

    #[tokio::test]
    async fn topic_digest_reads_matching_items_directly_for_any_period() {
        let (store, src) = store_with_source(Category::Medcare);
        store
            .upsert_items(
                &src,
                &[
                    new_item("https://e.com/1", "介護報酬の改定", "", None),
                    new_item("https://e.com/2", "無関係な記事", "", None),
                ],
                &at("2026-10-05T16:00:00Z"),
            )
            .unwrap();
        let topic = store.add_topic("介護", &["介護".to_string()]).unwrap();
        let scope = DigestScope::Topic(topic);

        let llm = MockProvider::ok("トピック週次");
        let st = run_digest(&store, &llm, Period::Week, d("2026-10-06"), &scope, jst(), false, &at(NOW))
            .await
            .unwrap();
        assert_eq!(st, Some(DigestStatus::Done));
        let prompt = llm.calls().remove(0).1;
        assert!(prompt.contains("対象: 介護"));
        assert!(prompt.contains("介護報酬の改定"));
        assert!(!prompt.contains("無関係な記事"));
        assert_eq!(
            store.get_digest(Period::Week, d("2026-10-05"), &scope).unwrap().unwrap().item_ids.len(),
            1
        );

        let missing = DigestScope::Topic(9999);
        let err = run_digest(&store, &llm, Period::Day, d("2026-10-06"), &missing, jst(), false, &at(NOW)).await;
        assert!(matches!(err, Err(Error::Invalid(_))));
    }
}
```

- [ ] **Step 2: テストが失敗することを確認する**

Run: `cargo test -p infodesk-core summarizer::`
Expected: コンパイルエラー。`summarize_top_items`、`run_digest` などが未定義。

- [ ] **Step 3: 実装する**

`crates/core/src/summarizer/mod.rs` の `DigestJob` の下、テストの上に追加:

```rust
fn item_line(item: &Item) -> String {
    let detail = item
        .ai_summary
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or(&item.excerpt);
    if detail.trim().is_empty() {
        format!("- {}", item.title)
    } else {
        format!(
            "- {} | {}",
            item.title,
            truncate_chars(detail, LINE_DETAIL_MAX_CHARS)
        )
    }
}

fn items_input(items: &[Item]) -> (Vec<String>, Vec<i64>) {
    (
        items.iter().map(item_line).collect(),
        items.iter().map(|i| i.id).collect(),
    )
}

/// 期間要約のジョブを組み立てる。LLMは呼ばない。
/// 入力がない場合と、生成済みで `force` が偽の場合は `None`。
pub fn plan_digest(
    store: &Store,
    period: Period,
    date: NaiveDate,
    scope: &DigestScope,
    tz: FixedOffset,
    force: bool,
) -> Result<Option<DigestJob>> {
    let start = period_start(period, date);
    if !force {
        if let Some(existing) = store.get_digest(period, start, scope)? {
            if existing.status == DigestStatus::Done {
                return Ok(None);
            }
        }
    }
    let (from, to) = utc_range(period, start, tz);

    let (label, lines, item_ids) = match scope {
        // トピック別は、期間によらず該当記事から直接作る。
        DigestScope::Topic(topic_id) => {
            let topic = store
                .get_topic(*topic_id)?
                .ok_or_else(|| Error::Invalid(format!("topic {topic_id} not found")))?;
            let items = store.query_items(&ItemFilter {
                topic_id: Some(*topic_id),
                since: Some(from),
                until: Some(to),
                order: ItemOrder::Score,
                limit: TOPIC_ITEM_LIMIT,
                ..Default::default()
            })?;
            let (lines, ids) = items_input(&items);
            (topic.name, lines, ids)
        }
        DigestScope::Category(category) => match period {
            Period::Day => {
                let items = store.query_items(&ItemFilter {
                    category: Some(*category),
                    since: Some(from),
                    until: Some(to),
                    order: ItemOrder::Score,
                    limit: CATEGORY_ITEM_LIMIT,
                    ..Default::default()
                })?;
                let (lines, ids) = items_input(&items);
                (category.label_ja().to_string(), lines, ids)
            }
            // 週次は日次要約から、月次は週次要約から作る。
            Period::Week | Period::Month => {
                let child = if period == Period::Week {
                    Period::Day
                } else {
                    Period::Week
                };
                let children =
                    store.digests_in_range(child, scope, start, period_end(period, start))?;
                let lines = children
                    .iter()
                    .map(|c| format!("## {}\n{}", c.period_start, c.body))
                    .collect();
                let ids: BTreeSet<i64> = children
                    .iter()
                    .flat_map(|c| c.item_ids.iter().copied())
                    .collect();
                (
                    category.label_ja().to_string(),
                    lines,
                    ids.into_iter().collect(),
                )
            }
        },
    };

    if lines.is_empty() {
        return Ok(None);
    }
    Ok(Some(DigestJob {
        period,
        period_start: start,
        scope: scope.clone(),
        system: prompts::DIGEST_SYSTEM.to_string(),
        prompt: prompts::digest_prompt(&label, period, &lines),
        item_ids,
    }))
}

/// LLMの結果を保存する。失敗と空の出力は、保留 (3回目で失敗) として記録する。
pub fn finish_digest(
    store: &Store,
    job: &DigestJob,
    provider: &str,
    output: Result<String>,
    now: &DateTime<Utc>,
) -> Result<DigestStatus> {
    match output.and_then(|raw| clean_output(&raw)) {
        Ok(body) => {
            store.save_digest_done(
                &NewDigest {
                    period: job.period,
                    period_start: job.period_start,
                    scope: job.scope.clone(),
                    body,
                    item_ids: job.item_ids.clone(),
                    provider: provider.to_string(),
                },
                now,
            )?;
            Ok(DigestStatus::Done)
        }
        Err(_) => store.mark_digest_attempt_failed(job.period, job.period_start, &job.scope, now),
    }
}

/// 計画、LLM呼び出し、保存をまとめて行う。`None` は生成不要だったことを表す。
#[allow(clippy::too_many_arguments)]
pub async fn run_digest(
    store: &Store,
    llm: &dyn LlmProvider,
    period: Period,
    date: NaiveDate,
    scope: &DigestScope,
    tz: FixedOffset,
    force: bool,
    now: &DateTime<Utc>,
) -> Result<Option<DigestStatus>> {
    let Some(job) = plan_digest(store, period, date, scope, tz, force)? else {
        return Ok(None);
    };
    let output = llm.complete(&job.system, &job.prompt).await;
    Ok(Some(finish_digest(store, &job, &llm.name(), output, now)?))
}

/// カテゴリーの上位 `limit` 件のうち、要約がない記事に要約を付ける。
/// 1件失敗したら残りを中止する (LLMが落ちている場合に全件で待たないため)。
/// 失敗した記事は要約なしのまま残り、次回の実行で再び対象になる。
pub async fn summarize_top_items(
    store: &Store,
    llm: &dyn LlmProvider,
    category: Category,
    since: &DateTime<Utc>,
    limit: u32,
) -> Result<(usize, usize)> {
    let items = store.top_unsummarized(category, since, limit)?;
    let mut ok = 0;
    for item in &items {
        let result = llm
            .complete(prompts::ITEM_SYSTEM, &prompts::item_prompt(item))
            .await
            .and_then(|raw| clean_output(&raw));
        match result {
            Ok(summary) => {
                store.set_ai_summary(item.id, &summary)?;
                ok += 1;
            }
            Err(_) => break,
        }
    }
    Ok((ok, items.len() - ok))
}
```

- [ ] **Step 4: テストが通ることを確認する**

Run: `cargo test -p infodesk-core summarizer::`
Expected: 8 passed。

Run: `cargo test -p infodesk-core && cargo clippy -p infodesk-core --all-targets -- -D warnings`
Expected: 全テスト passed、clippyの警告なし。clippyが指摘した箇所は挙動を変えずに直す。

- [ ] **Step 5: コミット**

```bash
git add crates/core
git commit -m "feat(core): add item summaries and hierarchical digests"
```

---

### Task 11: CLI と実環境での動作確認

**Files:**
- Create: `crates/cli/Cargo.toml` (cargo new), `crates/cli/src/main.rs`, `README.md`
- Modify: `Cargo.toml` (members に `crates/cli` を追加)

**Interfaces:**
- Consumes: core の公開API全部 (Task 1〜10の Produces)
- Produces: 実行ファイル `infodesk`。サブコマンドは `collect`, `summarize`, `digest`, `items`, `source`, `topic`, `config`。Plan 2 は CLI に依存しない。

- [ ] **Step 1: クレートを作る**

```bash
cargo new --bin crates/cli --name infodesk --vcs none
cd crates/cli
cargo add infodesk-core --path ../core
cargo add clap --features derive
cargo add tokio --features macros,rt-multi-thread
cargo add chrono
cd ../..
```

`Cargo.toml` (リポジトリ直下) の members を次にする:

```toml
members = ["crates/core", "crates/cli"]
```

- [ ] **Step 2: main.rs を書く**

`crates/cli/src/main.rs`:

```rust
use std::path::PathBuf;

use chrono::{FixedOffset, Local, NaiveDate, Utc};
use clap::{Parser, Subcommand};
use infodesk_core::collector::{self, feed};
use infodesk_core::llm::{provider_from_settings, LlmRole};
use infodesk_core::model::{
    Category, DigestScope, DigestStatus, ItemFilter, ItemOrder, Period, SourceKind,
};
use infodesk_core::period::{period_start, utc_range};
use infodesk_core::{seed, summarizer, Error, Result, Store};

#[derive(Parser)]
#[command(name = "infodesk", about = "情報収集と要約")]
struct Cli {
    /// DBファイルのパス。既定は ~/Library/Application Support/InfoDesk/infodesk.db
    #[arg(long, global = true)]
    db: Option<PathBuf>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// 有効なソースをすべて取得する
    Collect,
    /// 今日取得した記事の上位に要約を付ける
    Summarize,
    /// 期間要約を生成して表示する
    Digest {
        /// day | week | month
        period: String,
        /// 対象日 (YYYY-MM-DD)。既定は今日
        #[arg(long)]
        date: Option<String>,
        /// トピック名。指定するとトピック別要約になる
        #[arg(long)]
        topic: Option<String>,
        /// 生成済みでも作り直す
        #[arg(long)]
        force: bool,
    },
    /// 記事を一覧する
    Items {
        #[arg(long)]
        category: Option<String>,
        #[arg(long)]
        topic: Option<String>,
        /// 直近N日に取得した記事に絞る
        #[arg(long)]
        days: Option<i64>,
        #[arg(long)]
        text: Option<String>,
        #[arg(long, default_value_t = 30)]
        limit: u32,
    },
    Source {
        #[command(subcommand)]
        command: SourceCommand,
    },
    Topic {
        #[command(subcommand)]
        command: TopicCommand,
    },
    Config {
        #[command(subcommand)]
        command: ConfigCommand,
    },
}

#[derive(Subcommand)]
enum SourceCommand {
    List,
    /// RSSフィードを検証して追加する
    AddRss {
        url: String,
        /// github | arxiv | huggingface | hackernews | tech_jp | tech_intl | medcare
        #[arg(long)]
        category: String,
        /// 表示名。省略するとフィードのタイトル
        #[arg(long)]
        name: Option<String>,
    },
    Remove { id: i64 },
    Enable { id: i64 },
    Disable { id: i64 },
}

#[derive(Subcommand)]
enum TopicCommand {
    List,
    Add {
        name: String,
        #[arg(required = true)]
        keywords: Vec<String>,
    },
    Remove { id: i64 },
}

#[derive(Subcommand)]
enum ConfigCommand {
    Get { key: String },
    Set { key: String, value: String },
}

fn default_db_path() -> Result<PathBuf> {
    let home = std::env::var("HOME").map_err(|_| Error::Invalid("HOME is not set".into()))?;
    Ok(PathBuf::from(home).join("Library/Application Support/InfoDesk/infodesk.db"))
}

fn local_tz() -> FixedOffset {
    *Local::now().offset()
}

fn today() -> NaiveDate {
    Local::now().date_naive()
}

fn parse_category(s: &str) -> Result<Category> {
    Category::parse(s).ok_or_else(|| Error::Invalid(format!("unknown category '{s}'")))
}

fn topic_id(store: &Store, name: &str) -> Result<i64> {
    store
        .find_topic_by_name(name)?
        .map(|t| t.id)
        .ok_or_else(|| Error::Invalid(format!("topic '{name}' not found")))
}

async fn collect(store: &Store) -> Result<()> {
    let http = collector::http_client()?;
    let outcomes = collector::fetch_all(&http, &store.list_sources()?).await;
    for r in collector::apply_outcomes(store, &outcomes, &Utc::now())? {
        match r.error {
            None => println!("ok    {:>4} new  {}", r.new_items, r.name),
            Some(e) => println!("FAIL             {}: {e}", r.name),
        }
    }
    Ok(())
}

async fn summarize(store: &Store) -> Result<()> {
    let llm = provider_from_settings(store, LlmRole::Items)?;
    let limit: u32 = store
        .get_setting_or("summary.top_n", "20")?
        .parse()
        .map_err(|_| Error::Invalid("summary.top_n must be a number".into()))?;
    let (since, _) = utc_range(Period::Day, today(), local_tz());
    for category in Category::ALL {
        let (ok, failed) =
            summarizer::summarize_top_items(store, llm.as_ref(), category, &since, limit).await?;
        println!("{:<12} summarized {ok}, failed {failed}", category.as_str());
    }
    Ok(())
}

async fn digest(
    store: &Store,
    period: &str,
    date: Option<String>,
    topic: Option<String>,
    force: bool,
) -> Result<()> {
    let period =
        Period::parse(period).ok_or_else(|| Error::Invalid(format!("unknown period '{period}'")))?;
    let date = match date {
        Some(s) => NaiveDate::parse_from_str(&s, "%Y-%m-%d")
            .map_err(|_| Error::Invalid(format!("invalid date '{s}'")))?,
        None => today(),
    };
    let scopes: Vec<DigestScope> = match topic {
        Some(name) => vec![DigestScope::Topic(topic_id(store, &name)?)],
        None => Category::ALL.into_iter().map(DigestScope::Category).collect(),
    };
    let llm = provider_from_settings(store, LlmRole::Digest)?;
    let start = period_start(period, date);
    for scope in &scopes {
        let status = summarizer::run_digest(
            store,
            llm.as_ref(),
            period,
            date,
            scope,
            local_tz(),
            force,
            &Utc::now(),
        )
        .await?;
        let saved = store.get_digest(period, start, scope)?;
        match (status, saved) {
            (Some(DigestStatus::Pending), _) | (Some(DigestStatus::Failed), _) => {
                println!("## {} ({start}): 生成に失敗。次回に再試行する\n", scope.key())
            }
            (_, Some(d)) if d.status == DigestStatus::Done => {
                println!(
                    "## {} ({start}) [{}、記事{}件]\n{}\n",
                    scope.key(),
                    d.provider,
                    d.item_ids.len(),
                    d.body
                )
            }
            _ => println!("## {} ({start}): 対象の記事なし\n", scope.key()),
        }
    }
    Ok(())
}

fn items(
    store: &Store,
    category: Option<String>,
    topic: Option<String>,
    days: Option<i64>,
    text: Option<String>,
    limit: u32,
) -> Result<()> {
    let filter = ItemFilter {
        category: category.as_deref().map(parse_category).transpose()?,
        topic_id: topic.as_deref().map(|n| topic_id(store, n)).transpose()?,
        since: days.map(|d| Utc::now() - chrono::Duration::days(d)),
        text,
        order: ItemOrder::Recent,
        limit,
        ..Default::default()
    };
    for item in store.query_items(&filter)? {
        let score = item.score.map(|s| format!(" [{s}]")).unwrap_or_default();
        println!("{}{score}  ({})", item.title, item.source_name);
        println!("  {}", item.url);
        if let Some(summary) = &item.ai_summary {
            println!("  {summary}");
        }
    }
    Ok(())
}

async fn source(store: &Store, command: SourceCommand) -> Result<()> {
    match command {
        SourceCommand::List => {
            for s in store.list_sources()? {
                println!(
                    "{:>3} {} {:<12} {:<16} {}  {}{}",
                    s.id,
                    if s.enabled { "on " } else { "off" },
                    s.category.as_str(),
                    s.kind.as_str(),
                    s.name,
                    s.config,
                    s.last_error.map(|e| format!("  ERROR: {e}")).unwrap_or_default()
                );
            }
        }
        SourceCommand::AddRss { url, category, name } => {
            let category = parse_category(&category)?;
            let info = feed::validate(&collector::http_client()?, &url).await?;
            let name = name.filter(|n| !n.trim().is_empty()).unwrap_or(info.title);
            let id = store.add_source(SourceKind::Rss, &name, category, &url)?;
            println!("added source {id}: {name} ({} items in feed)", info.item_count);
        }
        SourceCommand::Remove { id } => store.delete_source(id)?,
        SourceCommand::Enable { id } => store.set_source_enabled(id, true)?,
        SourceCommand::Disable { id } => store.set_source_enabled(id, false)?,
    }
    Ok(())
}

fn topic(store: &Store, command: TopicCommand) -> Result<()> {
    match command {
        TopicCommand::List => {
            for t in store.list_topics()? {
                println!("{:>3} {}: {}", t.id, t.name, t.keywords.join(", "));
            }
        }
        TopicCommand::Add { name, keywords } => {
            let id = store.add_topic(&name, &keywords)?;
            println!("added topic {id}: {name}");
        }
        TopicCommand::Remove { id } => store.delete_topic(id)?,
    }
    Ok(())
}

fn config(store: &Store, command: ConfigCommand) -> Result<()> {
    match command {
        ConfigCommand::Get { key } => match store.get_setting(&key)? {
            Some(v) => println!("{v}"),
            None => println!("(unset)"),
        },
        ConfigCommand::Set { key, value } => store.set_setting(&key, &value)?,
    }
    Ok(())
}

async fn run(cli: Cli) -> Result<()> {
    let path = match cli.db {
        Some(p) => p,
        None => default_db_path()?,
    };
    let store = Store::open(&path)?;
    seed::seed_defaults(&store)?;
    match cli.command {
        Command::Collect => collect(&store).await,
        Command::Summarize => summarize(&store).await,
        Command::Digest { period, date, topic, force } => {
            digest(&store, &period, date, topic, force).await
        }
        Command::Items { category, topic, days, text, limit } => {
            items(&store, category, topic, days, text, limit)
        }
        Command::Source { command } => source(&store, command).await,
        Command::Topic { command: c } => topic(&store, c),
        Command::Config { command: c } => config(&store, c),
    }
}

#[tokio::main]
async fn main() {
    if let Err(e) = run(Cli::parse()).await {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}
```

- [ ] **Step 3: ビルドとテスト**

Run: `cargo build && cargo test && cargo clippy --all-targets -- -D warnings`
Expected: ビルド成功、全テスト passed、警告なし。

- [ ] **Step 4: 実環境で収集を確認する**

一時DBを使う。実際のネットワークに接続する。

```bash
DB="$(mktemp -d)/smoke.db"
cargo run -q -p infodesk -- --db "$DB" collect
```

Expected: 22行出力される。全行が `ok` で始まる。`FAIL` の行がある場合は、そのソースのURLと応答を確認する。恒久的に取得できないソースは `seed.rs` の `DEFAULT_SOURCES` から外し、理由をコミットメッセージに書く (一時的な失敗の場合は外さない)。

```bash
cargo run -q -p infodesk -- --db "$DB" items --category medcare --limit 5
cargo run -q -p infodesk -- --db "$DB" topic add 認知症 認知症 dementia Alzheimer
cargo run -q -p infodesk -- --db "$DB" topic add 介護 介護
cargo run -q -p infodesk -- --db "$DB" items --topic 認知症 --limit 5
cargo run -q -p infodesk -- --db "$DB" items --topic 介護 --limit 5
```

Expected: それぞれ記事が表示される。トピック「認知症」の結果に日本語と英語の記事が混ざる。トピック「介護」(2文字) でも記事が出る。

- [ ] **Step 5: RSS追加を確認する**

```bash
cargo run -q -p infodesk -- --db "$DB" source add-rss https://gigazine.net/news/rss_2.0/ --category tech_jp
cargo run -q -p infodesk -- --db "$DB" source add-rss https://example.com/ --category tech_jp
cargo run -q -p infodesk -- --db "$DB" source list
```

Expected: 1行目は `added source 23: ...` (フィードのタイトルが表示名になる)。2行目は `error: invalid feed: ...` で終了コード1。一覧に23件あり、example.com はない。1行目のURLが取得できない場合は、設定画面から追加する想定の任意のRSS URLに置き換えて確認する。

- [ ] **Step 6: Ollamaでの要約を確認する**

Ollamaが起動していること (`ollama list` に `gemma4:26b` があること) を確認してから実行する。モデルの読み込みを含め数分かかる。

```bash
cargo run -q -p infodesk -- --db "$DB" config set summary.top_n 3
cargo run -q -p infodesk -- --db "$DB" summarize
cargo run -q -p infodesk -- --db "$DB" items --category hackernews --limit 30 | grep -c "^  [^h]"
cargo run -q -p infodesk -- --db "$DB" digest day
cargo run -q -p infodesk -- --db "$DB" digest week
cargo run -q -p infodesk -- --db "$DB" digest day --topic 認知症
```

Expected: `summarize` は各カテゴリーで `summarized 3, failed 0` (記事が3件未満のカテゴリーはその件数)。`grep -c` は3を返す (日本語の要約行)。`digest day` は7カテゴリーの日本語の要約を表示し、各見出しに `ollama:gemma4:26b` と記事件数が出る。`digest week` は日次要約から作った週次要約を表示する。トピック別要約も日本語で表示される。

要約が日本語でない、または前置きが付く場合は、`summarizer/prompts.rs` の `ITEM_SYSTEM` と `DIGEST_SYSTEM` の文面を調整し、同じ手順で再確認する。

- [ ] **Step 7: Claudeへの切替とOllama停止時の挙動を確認する**

```bash
cargo run -q -p infodesk -- --db "$DB" config set llm.digest_provider claude
cargo run -q -p infodesk -- --db "$DB" digest day --topic 介護
cargo run -q -p infodesk -- --db "$DB" config set llm.digest_provider ollama
cargo run -q -p infodesk -- --db "$DB" config set ollama.base_url http://127.0.0.1:1
cargo run -q -p infodesk -- --db "$DB" digest month --topic 介護
cargo run -q -p infodesk -- --db "$DB" summarize
```

Expected: Claudeでの要約は見出しに `claude` と出る。Ollamaに接続できない状態では、`digest` は「生成に失敗。次回に再試行する」と表示して終了コード0、`summarize` は `summarized 0, failed N` と表示して終了コード0。どちらもパニックしない。

- [ ] **Step 8: README を書く**

`README.md`:

````markdown
# InfoDesk

技術系と医療介護系の情報を集め、日次、週次、月次の日本語要約を作る。

設計は `docs/superpowers/specs/2026-10-06-infodesk-design.md` を参照。

## 構成

- `crates/core`: 収集、保存 (SQLite)、要約のライブラリ
- `crates/cli`: コマンド `infodesk`

## 使い方

```bash
cargo run -p infodesk -- collect              # 全ソースを取得
cargo run -p infodesk -- summarize            # 今日の上位記事に要約を付ける
cargo run -p infodesk -- digest day           # 今日のカテゴリー別要約
cargo run -p infodesk -- digest week --topic 認知症
cargo run -p infodesk -- items --category medcare --days 7
cargo run -p infodesk -- source add-rss <URL> --category tech_jp
cargo run -p infodesk -- topic add 認知症 認知症 dementia Alzheimer
```

DBの既定の場所は `~/Library/Application Support/InfoDesk/infodesk.db`。`--db <path>` で変更できる。

## 設定

`infodesk config set <key> <value>` で変更する。

| キー | 既定値 | 内容 |
|---|---|---|
| `llm.items_provider` | `ollama` | 記事要約に使うLLM (`ollama` または `claude`) |
| `llm.digest_provider` | `ollama` | 期間要約に使うLLM |
| `ollama.base_url` | `http://localhost:11434` | OllamaのURL |
| `ollama.model` | `gemma4:26b` | Ollamaのモデル |
| `claude.bin` | `claude` | claude CLIのパス |
| `summary.top_n` | `20` | カテゴリーごとに要約を付ける記事数 |

## テスト

```bash
cargo test
```

単体テストはネットワークに接続しない。`crates/core/tests/fixtures/` の実レスポンスは次で更新する。

```bash
cargo run -p infodesk-core --example fetch_fixtures
```
````

- [ ] **Step 9: コミット**

```bash
git add Cargo.toml Cargo.lock crates/cli README.md
git commit -m "feat(cli): add infodesk command"
```

---

## Plan 2 への引き継ぎ

Plan 1 の完了後、次を Plan 2 (`Tauri app`) として別の計画に書く。

- Tauri 2 のシェル。`crates/core` を依存に加える。DBは `Store` を専用スレッドまたは `Mutex` で保持する。
- scheduler: 3時間おきの収集、毎朝の日次要約、月曜の週次、1日の月次、起動時の不足分の補完、`pending_digests` の再試行。
- メニューバー常駐、ログイン時起動。
- 画面: ダイジェスト、フィード、設定 (RSSの追加は `feed::validate` と `add_source` を使う)。
- macOSのGUIアプリはシェルの `PATH` を引き継がない。`claude.bin` と Ollama への到達性を設定画面の接続テストで確認する。
