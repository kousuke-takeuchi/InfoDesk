# InfoDesk Thumbnails and Chat Progress (Plan 4) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** フィードの記事一覧にサムネイル画像を出し (spec 13節)、チャットにエージェントの途中経過 (作業計画、思考、ツールの詳細、経過時間) を出す (spec 14節)。

**Architecture:** 画像は収集の時に1記事1回だけ決めて `items.image_url` に保存する。フィードの画像 (RSS の Media RSS、GitHub の所有者アバター) を先に使い、無ければ新着記事のページから `og:image` を読む (本文取得と同じ、内部アドレスを拒否するクライアント)。画面は URL を `<img>` で直接読み、CSP の `img-src` に `https:` を足す。チャットでは、サイドカーが `update_plan` ツールと回答の開始/終了時刻 (メッセージのメタデータ) を送り、UI が回答ごとに状態行、チェックリスト、折りたためる作業 (思考とツール) を描く。

**Tech Stack:** Rust (core、CLI、Tauri 2)、feed-rs 3、scraper 0.27、reqwest 0.13、TypeScript、React 19、`ai` 7.0.136、`@assistant-ui/react` 0.15.25、vitest。

**Spec:** `docs/superpowers/specs/2026-10-06-infodesk-design.md` 13節 (記事の画像)、14節 (チャットの途中経過)。前提: Plan 1〜3 が `master` にマージ済み。ブランチは `plan-4-thumbnails`。

## Global Constraints

- 画像の URL は `https` だけを保存し、表示する。相対 URL はページの URL を基準に絶対 URL にする。
- 記事ページから画像を取るときは、本文取得と同じ内部アドレス拒否のクライアント (`crates/core/src/page.rs`) を使う。同時実行は4件、1件10秒、読むのは先頭の 512KB まで。
- 新しく入る記事だけを対象にする。既存の記事には遡って付けない。画像の取得の失敗は記事の保存を妨げず、収集のエラーとして記録しない。
- arXiv のソースと、ホスト名が `news.google.com` の記事は、記事ページを取りに行かない。
- 表示: 記事行の右端、スターの左に 96x64、角丸 5、`object-fit: cover`、`loading="lazy"`、`referrerpolicy="no-referrer"`。読み込みに失敗した画像は隠す。画像の無い記事は幅を詰める。
- CSP は `img-src 'self' data: https:` に変える。それ以外の CSP は変えない。
- チャットの回答の Markdown では画像を描画せず、代替テキストだけを表示する。
- `update_plan` の入力は `items: [{ title, status }]`、`status` は `pending` | `in_progress` | `done`、1〜8項目。データは保存しない。画面ではツール行に出さず、その回答で最後に呼ばれた内容をチェックリストとして出す。
- メタデータ `startedAt`、`finishedAt` は ISO 8601。回答中は「調査中 · 12秒 · ツール3回」を1秒ごとに更新する。完了後は「4ステップ · 18秒」に折りたたむ。ステップ数は思考とツール (計画の更新を除く) の数。時刻の無い古いメッセージは所要時間を省く。
- UI の文言は日本語、絵文字なし、色は既存のトークンだけ、アイコンは `lucide-react`。
- 依存は `cargo add` / `npm install` で入れる。版を手書きしない。
- コミットメッセージに `Co-Authored-By`、`Claude-Session` などのAI帰属トレーラーを付けない。
- 各タスクのコミット前に、触った範囲で `cargo test --workspace`、`cargo clippy --all-targets -- -D warnings`、`cargo fmt --check`、`cd app && npm test -- --run && npm run build`、`cd agent && npm test -- --run && npm run build` が通ること。

## Review Focus

1. 画像の URL が `http`、`data:`、`javascript:`、空、相対、プロトコル相対 (`//cdn.example.com/a.png`) の場合。https の絶対 URL だけが残る。Task 1 と Task 2 でテストする。
2. 記事ページが遅い、巨大、HTML でない (PDF)、404、内部アドレスに転送する場合。画像なしになり、収集は止まらず、記事は保存される。Task 2 でテストする。
3. 同じ収集の中で既に DB にある URL の記事。ページを取りに行かない。Task 2 でテストする。
4. 画像の URL が切れている (404) か、巨大な画像。行の配置は崩れず、画像は隠れる。Task 3 でテストする (onError)。
5. モデルが `update_plan` を呼ばない、項目が9件以上、状態の値が不正。チェックリストを出さないか、入力の検査でツールのエラーになる。回答の表示は崩れない。Task 4 と Task 5 でテストする。

## File Structure

```
crates/core/src/model.rs                NewItem と Item に image_url
crates/core/src/store/migrate.rs        版4: items.image_url
crates/core/src/store/items.rs          保存と読み出しに image_url、existing_urls
crates/core/src/collector/feed.rs       エントリーの画像 (Media RSS、enclosure)
crates/core/src/collector/github.rs     所有者アバター
crates/core/src/collector/images.rs     og:image の抽出と、新着記事への付与 (新規)
crates/core/src/collector/mod.rs        pub mod images
crates/core/src/page.rs                 fetch_html_head (ページの先頭を読む)
crates/cli/src/lib.rs                   collect で画像を付与
app/src-tauri/src/jobs.rs               collect で画像を付与
app/src-tauri/tauri.conf.json           CSP img-src
app/src/types.ts                        Item.image_url
app/src/views/FeedView.tsx              サムネイル
app/src/styles/layout.css               サムネイルの CSS
app/src/chat/ChatThread.tsx             Markdown の画像を代替テキストに、途中経過の組み立て
agent/src/tools.ts                      update_plan
agent/src/prompt.ts                     計画の指示
agent/src/chat.ts                       メタデータ (startedAt、finishedAt)、sendReasoning
app/src/chat/progress.ts                計画、ステップ数、経過時間の計算 (新規)
app/src/chat/toolDetails.ts             ツールの入力と結果の要点 (新規)
app/src/chat/WorkSummary.tsx            状態行、チェックリスト、折りたたみ (新規)
app/src/chat/Reasoning.tsx              思考の折りたたみ (新規)
app/src/chat/ToolCall.tsx               押すと詳細が開く
app/src/styles/chat.css                 途中経過の CSS
README.md                               画像とチャットの途中経過
```

既存のファイルの構造に合わせる。`NewItem { ... }` を組み立てている箇所は14か所ある (`grep -rn "NewItem {" --include=*.rs crates app`)。すべてに `image_url` を足す。

---

### Task 1: 画像の URL の保存と、フィードからの取り出し

**Files:**
- Modify: `crates/core/src/model.rs`、`crates/core/src/store/migrate.rs`、`crates/core/src/store/items.rs`、`crates/core/src/collector/feed.rs`、`crates/core/src/collector/github.rs`、`NewItem { ... }` を組み立てるすべての箇所 (`testutil.rs`、`hackernews.rs`、`huggingface.rs`、`crates/cli/src/json.rs`、`app/src-tauri/src/commands/{items,digests,sources}.rs`、`model.rs`)
- Test: 同じファイルの `#[cfg(test)]`

**Interfaces:**
- Produces:
  - `NewItem.image_url: Option<String>`、`Item.image_url: Option<String>` (`Serialize` で `image_url`)
  - DB 版4: `ALTER TABLE items ADD COLUMN image_url TEXT;`
  - `pub fn https_image_url(raw: &str, base: Option<&Url>) -> Option<String>` を `crates/core/src/collector/images.rs` に置く (Task 2 でもこのファイルに足す)。`https` の絶対 URL だけを返す。`base` があれば相対 URL とプロトコル相対 URL を解決する。前後の空白を除く。
  - `feed::parse_feed` がエントリーの画像を `image_url` に入れる。
  - `github::parse` が `https://github.com/<owner>.png` を入れる。
  - `Store::existing_urls(&self, urls: &[String]) -> Result<HashSet<String>>`

- [ ] **Step 1: 失敗するテストを書く**

`crates/core/src/collector/images.rs` (`collector/mod.rs` に `pub mod images;`):

```rust
//! 記事の画像 (spec 13節)。

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::Url;

    #[test]
    fn keeps_only_absolute_https_urls() {
        let base = Url::parse("https://news.example.com/a/b.html").unwrap();
        assert_eq!(https_image_url(" https://cdn.example.com/x.png ", None).as_deref(), Some("https://cdn.example.com/x.png"));
        assert_eq!(https_image_url("/img/x.png", Some(&base)).as_deref(), Some("https://news.example.com/img/x.png"));
        assert_eq!(https_image_url("x.png", Some(&base)).as_deref(), Some("https://news.example.com/a/x.png"));
        assert_eq!(https_image_url("//cdn.example.com/x.png", Some(&base)).as_deref(), Some("https://cdn.example.com/x.png"));
        for bad in ["http://cdn.example.com/x.png", "data:image/png;base64,AAAA", "javascript:alert(1)", "", "   ", "/img/x.png"] {
            assert_eq!(https_image_url(bad, None), None, "{bad}");
        }
        let http_base = Url::parse("http://news.example.com/a").unwrap();
        assert_eq!(https_image_url("/img/x.png", Some(&http_base)), None, "http のページの相対 URL は http になるので捨てる");
    }
}
```

`crates/core/src/collector/feed.rs` のテストに追加:

```rust
    #[test]
    fn takes_entry_images_from_media_rss_and_enclosures() {
        let rss = br#"<rss version="2.0" xmlns:media="http://search.yahoo.com/mrss/"><channel><title>T</title>
<item><title>thumb</title><link>https://e.com/1</link><media:thumbnail url="https://img.e.com/1.jpg"/></item>
<item><title>content</title><link>https://e.com/2</link><media:content url="https://img.e.com/2.png" type="image/png"/></item>
<item><title>enclosure</title><link>https://e.com/3</link><enclosure url="https://img.e.com/3.jpg" type="image/jpeg" length="1"/></item>
<item><title>audio</title><link>https://e.com/4</link><enclosure url="https://e.com/4.mp3" type="audio/mpeg" length="1"/></item>
<item><title>http</title><link>https://e.com/5</link><media:thumbnail url="http://img.e.com/5.jpg"/></item>
<item><title>none</title><link>https://e.com/6</link></item>
</channel></rss>"#;
        let (_, items) = parse_feed(rss).unwrap();
        let img: Vec<Option<&str>> = items.iter().map(|i| i.image_url.as_deref()).collect();
        assert_eq!(img, vec![
            Some("https://img.e.com/1.jpg"),
            Some("https://img.e.com/2.png"),
            Some("https://img.e.com/3.jpg"),
            None,
            None,
            None,
        ]);
    }
```

`crates/core/src/collector/github.rs` の `parses_repositories` に追加:

```rust
        assert_eq!(items[0].image_url.as_deref(), Some("https://github.com/tester-army.png"));
```

`crates/core/src/store/migrate.rs` のテストに追加:

```rust
    #[test]
    fn version_4_adds_image_url_and_keeps_items() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(SCHEMA).unwrap();
        conn.execute_batch("PRAGMA user_version = 3;").unwrap();
        conn.execute(
            "INSERT INTO sources(kind, name, category, config) VALUES ('rss', 'S', 'tech_jp', 'https://e.com/feed')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO items(source_id, source_name, category, url, title, excerpt, fetched_at) \
             VALUES (1, 'S', 'tech_jp', 'https://e.com/1', '残る記事', '', '2026-10-09T00:00:00Z')",
            [],
        )
        .unwrap();
        migrate(&conn).unwrap();
        assert_eq!(version(&conn), 4);
        assert!(has_column(&conn, "items", "image_url"));
        let title: String = conn.query_row("SELECT title FROM items", [], |r| r.get(0)).unwrap();
        assert_eq!(title, "残る記事");
    }
```

版3の DB を作るのに `SCHEMA` だけでは chat の表が無いが、版4の移行は `items` だけを触るので足りる。`migrate` が版3以下の移行を `user_version` で飛ばすことは既存のコードのとおり (`migrate.rs`)。もし chat の表が無いことで失敗するなら、`SCHEMA` の後に版2と版3の SQL も流してから `user_version = 3` にする。

`crates/core/src/store/items.rs` のテストに追加 (既存の `store_with_source` と `new_item` を使う):

```rust
    #[test]
    fn saves_and_reads_image_url_and_reports_existing_urls() {
        let (store, source) = store_with_source(Category::TechJp);
        let mut a = new_item("https://e.com/a", "A", "", None);
        a.image_url = Some("https://img.e.com/a.jpg".into());
        let b = new_item("https://e.com/b", "B", "", None);
        store.upsert_items(&source, &[a, b], &at("2026-10-10T00:00:00Z")).unwrap();
        let items = store.query_items(&Default::default()).unwrap();
        let img = |t: &str| items.iter().find(|i| i.title == t).unwrap().image_url.clone();
        assert_eq!(img("A").as_deref(), Some("https://img.e.com/a.jpg"));
        assert_eq!(img("B"), None);
        let known = store
            .existing_urls(&["https://e.com/a".into(), "https://e.com/zzz".into()])
            .unwrap();
        assert!(known.contains("https://e.com/a"));
        assert!(!known.contains("https://e.com/zzz"));
    }
```

`new_item` と `store_with_source` の正確な名前と引数は `crates/core/src/testutil.rs` を読んで合わせる。

- [ ] **Step 2: 失敗を確かめる**

Run: `cargo test -p infodesk-core` → FAIL (`image_url` が無い、`https_image_url` が無い)

- [ ] **Step 3: 実装する**

- `model.rs`: `NewItem` と `Item` の最後に `pub image_url: Option<String>,` を足す。`NewItem { ... }` を組み立てる14か所すべてに `image_url: None,` を足す (feed と github は下記の値)。
- `migrate.rs`: `SCHEMA_VERSION = 4`、`MIGRATIONS` に `(4, "ALTER TABLE items ADD COLUMN image_url TEXT;")`。
- `items.rs`: `ITEM_COLS` の最後に `items.image_url`、`row_to_item` で読む (列番号は既存の最後の列の次)。`upsert_items` の INSERT に `image_url` を足す。既存の記事 (`ON CONFLICT`) では画像を上書きしない。`existing_urls`:

```rust
    /// 渡した URL のうち、既に保存されているものを返す。
    pub fn existing_urls(&self, urls: &[String]) -> Result<HashSet<String>> {
        let mut out = HashSet::new();
        let mut stmt = self.conn.prepare("SELECT 1 FROM items WHERE url = ?1")?;
        for u in urls {
            if stmt.exists(params![u])? {
                out.insert(u.clone());
            }
        }
        Ok(out)
    }
```

- `images.rs`:

```rust
use reqwest::Url;

/// https の絶対 URL だけを返す。`base` があれば相対 URL を解決する。
pub fn https_image_url(raw: &str, base: Option<&Url>) -> Option<String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    let url = match base {
        Some(b) => b.join(raw).ok()?,
        None => Url::parse(raw).ok()?,
    };
    (url.scheme() == "https" && url.host().is_some()).then(|| url.to_string())
}
```

- `feed.rs`: エントリーの画像を取り出す関数を足し、`NewItem` に入れる。

```rust
/// Media RSS のサムネイル、画像の Media RSS コンテンツ、画像の enclosure の順に探す。
fn entry_image(e: &feed_rs::model::Entry) -> Option<String> {
    let thumbs = e.media.iter().flat_map(|m| m.thumbnails.iter()).map(|t| t.image.uri.as_str());
    let contents = e.media.iter().flat_map(|m| m.content.iter()).filter(|c| {
        c.content_type.as_ref().is_some_and(|t| t.ty() == "image")
    }).filter_map(|c| c.url.as_ref().map(|u| u.as_str()));
    thumbs.chain(contents).find_map(|u| super::images::https_image_url(u, None))
}
```

feed-rs 3.0 では enclosure は `entry.media` の `content` に入る (`feed-rs-3.0.0/src/parser/rss2/mod.rs:192` のコメント)。`MediaContent.content_type` の型 (`MediaTypeBuf`) の `ty()` の名前は `~/.cargo/registry/src/*/feed-rs-3.0.0/src/model/mod.rs:879` と `mediatype` クレートで確かめる。Step 1 の enclosure のテストが通らない場合は、enclosure がどこに入るかを feed-rs のソースで確かめて合わせる。

- `github.rs`: `author` と同じ所有者名で `image_url: repo.split('/').next().map(|o| format!("{BASE}/{o}.png"))`。

- [ ] **Step 4: 通ることを確かめる**

Run: `cargo test --workspace && cargo clippy --all-targets -- -D warnings && cargo fmt --check` → PASS

- [ ] **Step 5: コミット**

```bash
git add crates app/src-tauri/src
git commit -m "feat(core): store article image URLs from feeds and GitHub"
```

---

### Task 2: 記事ページの og:image と、収集への組み込み

**Files:**
- Modify: `crates/core/src/page.rs`、`crates/core/src/collector/images.rs`、`crates/core/Cargo.toml` (tokio の `rt` 機能)、`crates/cli/src/lib.rs`、`app/src-tauri/src/jobs.rs`
- Test: `page.rs`、`images.rs`、`jobs.rs` の既存テストが通ること

**Interfaces:**
- Consumes: Task 1 の `https_image_url`、`Store::existing_urls`、`NewItem.image_url`、`page::page_client`
- Produces:
  - `page.rs`: `pub(crate) async fn fetch_html_head(http: &reqwest::Client, url: &str, max_bytes: usize, allow_loopback: bool) -> Result<(Url, String)>` (最終 URL と、先頭 `max_bytes` を文字コードに従って読んだ HTML。HTML でなければ `Error::Invalid`)
  - `images.rs`:
    - `pub const PAGE_IMAGE_CONCURRENCY: usize = 4;`、`PAGE_IMAGE_TIMEOUT: Duration = 10秒`、`PAGE_IMAGE_MAX_BYTES: usize = 512 * 1024;`
    - `pub fn og_image(html: &str, page_url: &Url) -> Option<String>` (`meta[property="og:image"]`、`meta[property="og:image:secure_url"]`、`meta[name="twitter:image"]`、`meta[property="twitter:image"]` の順。`https_image_url` で検査)
    - `pub fn wants_page_image(kind: SourceKind, item_url: &str) -> bool` (`Arxiv` と `news.google.com` は false)
    - `pub async fn fill_page_images(outcomes: &mut [FetchOutcome], known: &HashSet<String>)` (本番のクライアント)
    - `pub(crate) async fn fill_page_images_with(outcomes: &mut [FetchOutcome], known: &HashSet<String>, http: &reqwest::Client, allow_loopback: bool)` (テスト用に loopback を許せる)
  - 収集: `fetch_all` → `existing_urls` → `fill_page_images` → `apply_outcomes`。CLI の `collect` とアプリの `collect_inner` の両方。ストアのロックは `existing_urls` の間だけ持ち、`await` の間は持たない。

- [ ] **Step 1: 失敗するテストを書く**

`images.rs` のテストに追加:

```rust
    #[test]
    fn reads_og_and_twitter_images() {
        let page = Url::parse("https://news.example.com/a/b.html").unwrap();
        let html = |head: &str| format!("<html><head>{head}</head><body></body></html>");
        assert_eq!(og_image(&html(r#"<meta property="og:image" content="https://img.example.com/og.jpg">"#), &page).as_deref(), Some("https://img.example.com/og.jpg"));
        assert_eq!(og_image(&html(r#"<meta name="twitter:image" content="/tw.png">"#), &page).as_deref(), Some("https://news.example.com/tw.png"));
        assert_eq!(og_image(&html(r#"<meta property="og:image" content="http://img.example.com/x.jpg"><meta name="twitter:image" content="https://img.example.com/t.jpg">"#), &page).as_deref(), Some("https://img.example.com/t.jpg"), "http の og:image は捨てて次を見る");
        assert_eq!(og_image(&html(""), &page), None);
    }

    #[test]
    fn skips_arxiv_and_google_news() {
        use crate::model::SourceKind;
        assert!(!wants_page_image(SourceKind::Arxiv, "https://arxiv.org/abs/2610.00001"));
        assert!(!wants_page_image(SourceKind::Rss, "https://news.google.com/rss/articles/CBMi?oc=5"));
        assert!(wants_page_image(SourceKind::Rss, "https://zenn.dev/a/articles/x"));
        assert!(wants_page_image(SourceKind::Hackernews, "https://example.com/post"));
    }

    #[tokio::test]
    async fn fills_new_items_only_and_survives_failures() {
        use crate::collector::FetchOutcome;
        use crate::model::{Category, NewItem, Source, SourceKind};
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        let page = |img: &str| format!(r#"<html><head><meta property="og:image" content="{img}"></head></html>"#);
        Mock::given(method("GET")).and(path("/new"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(page("https://img.example.com/new.jpg").into_bytes(), "text/html"))
            .expect(1).mount(&server).await;
        Mock::given(method("GET")).and(path("/known"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(page("https://img.example.com/k.jpg").into_bytes(), "text/html"))
            .expect(0).mount(&server).await;
        Mock::given(method("GET")).and(path("/has-image"))
            .respond_with(ResponseTemplate::new(200))
            .expect(0).mount(&server).await;
        Mock::given(method("GET")).and(path("/missing"))
            .respond_with(ResponseTemplate::new(404)).mount(&server).await;
        Mock::given(method("GET")).and(path("/pdf"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(b"%PDF".to_vec(), "application/pdf")).mount(&server).await;

        let base = server.uri();
        let item = |p: &str, img: Option<&str>| NewItem {
            url: format!("{base}{p}"), title: p.into(), excerpt: String::new(), author: None,
            published_at: None, score: None, image_url: img.map(String::from),
        };
        let source = Source {
            id: 1, kind: SourceKind::Rss, name: "S".into(), category: Category::TechJp,
            config: format!("{base}/feed"), enabled: true, last_fetched_at: None, last_error: None,
        };
        let mut outcomes = vec![FetchOutcome {
            source,
            result: Ok(vec![
                item("/new", None),
                item("/known", None),
                item("/has-image", Some("https://img.example.com/feed.jpg")),
                item("/missing", None),
                item("/pdf", None),
            ]),
        }];
        let known: HashSet<String> = [format!("{base}/known")].into_iter().collect();
        let http = crate::page::page_client(true).unwrap();
        fill_page_images_with(&mut outcomes, &known, &http, true).await;
        let items = outcomes[0].result.as_ref().unwrap();
        let img: Vec<Option<&str>> = items.iter().map(|i| i.image_url.as_deref()).collect();
        assert_eq!(img, vec![
            Some("https://img.example.com/new.jpg"),
            None,
            Some("https://img.example.com/feed.jpg"),
            None,
            None,
        ]);
    }

    #[tokio::test]
    async fn public_path_refuses_local_pages() {
        use crate::collector::FetchOutcome;
        use crate::model::{Category, NewItem, Source, SourceKind};
        let source = Source {
            id: 1, kind: SourceKind::Rss, name: "S".into(), category: Category::TechJp,
            config: "https://e.com/feed".into(), enabled: true, last_fetched_at: None, last_error: None,
        };
        let mut outcomes = vec![FetchOutcome {
            source,
            result: Ok(vec![NewItem {
                url: "http://127.0.0.1:9/x".into(), title: "x".into(), excerpt: String::new(), author: None,
                published_at: None, score: None, image_url: None,
            }]),
        }];
        fill_page_images(&mut outcomes, &HashSet::new()).await;
        assert_eq!(outcomes[0].result.as_ref().unwrap()[0].image_url, None);
    }
```

`Source` のフィールド名は `crates/core/src/model.rs` を読んで合わせる。`page_client` が `pub(crate)` であることは確認済み (`page.rs:49`)。

`page.rs` のテストに追加:

```rust
    #[tokio::test]
    async fn html_head_reads_only_the_first_bytes() {
        let body = format!("<html><head><title>t</title></head><body>{}</body></html>", "a".repeat(10_000));
        let server = serve(&body, "text/html").await;
        let http = page_client(true).unwrap();
        let (url, html) = fetch_html_head(&http, &format!("{}/a", server.uri()), 100, true).await.unwrap();
        assert!(url.as_str().ends_with("/a"));
        assert!(html.len() <= 100, "{}", html.len());
        assert!(html.starts_with("<html><head><title>t"));
        let pdf = serve("%PDF", "application/pdf").await;
        assert!(fetch_html_head(&http, &format!("{}/a", pdf.uri()), 100, true).await.is_err());
    }
```

`serve` は `page.rs` の既存のテスト補助 (`page.rs:352`)。

- [ ] **Step 2: 失敗を確かめる**

Run: `cargo test -p infodesk-core images:: page::html_head` → FAIL

- [ ] **Step 3: 実装する**

`page.rs`: `fetch_with` の前半 (URL の検査、`check_host`、送信、状態コード、Content-Type の判定、文字コードの決定) を使い回して `fetch_html_head` を書く。HTML 以外 (text/plain を含む) は `Error::Invalid("not html")`。読むのは `max_bytes` まで (それを超えたら読むのをやめる)。`decode(&bytes, &content_type_header, true)` で文字列にする。重複を避けるため、`fetch_with` と共通の前半を `async fn open(http, url, allow_loopback) -> Result<(reqwest::Response, String content_type_header, String content_type)>` に切り出してよい。

`images.rs`:

```rust
use std::collections::HashSet;
use std::time::Duration;

use scraper::{Html, Selector};

use super::FetchOutcome;
use crate::model::SourceKind;

pub const PAGE_IMAGE_CONCURRENCY: usize = 4;
pub const PAGE_IMAGE_TIMEOUT: Duration = Duration::from_secs(10);
pub const PAGE_IMAGE_MAX_BYTES: usize = 512 * 1024;

const META_SELECTORS: [&str; 4] = [
    r#"meta[property="og:image"]"#,
    r#"meta[property="og:image:secure_url"]"#,
    r#"meta[name="twitter:image"]"#,
    r#"meta[property="twitter:image"]"#,
];

pub fn og_image(html: &str, page_url: &Url) -> Option<String> {
    let doc = Html::parse_document(html);
    META_SELECTORS.iter().filter_map(|s| Selector::parse(s).ok()).find_map(|sel| {
        doc.select(&sel)
            .filter_map(|m| m.value().attr("content"))
            .find_map(|c| https_image_url(c, Some(page_url)))
    })
}

pub fn wants_page_image(kind: SourceKind, item_url: &str) -> bool {
    if kind == SourceKind::Arxiv {
        return false;
    }
    !Url::parse(item_url).ok().and_then(|u| u.host_str().map(str::to_string)).is_some_and(|h| h == "news.google.com")
}

/// 新しく入る記事のうち画像の無いものに、記事ページの og:image を付ける。失敗しても何もしない。
pub async fn fill_page_images(outcomes: &mut [FetchOutcome], known: &HashSet<String>) {
    let Ok(http) = crate::page::page_client(false) else { return };
    fill_page_images_with(outcomes, known, &http, false).await;
}

pub(crate) async fn fill_page_images_with(
    outcomes: &mut [FetchOutcome],
    known: &HashSet<String>,
    http: &reqwest::Client,
    allow_loopback: bool,
) {
    // (outcome の番号, item の番号, URL)
    let mut targets: Vec<(usize, usize, String)> = Vec::new();
    for (oi, o) in outcomes.iter().enumerate() {
        let Ok(items) = &o.result else { continue };
        for (ii, it) in items.iter().enumerate() {
            if it.image_url.is_none() && !known.contains(&it.url) && wants_page_image(o.source.kind, &it.url) {
                targets.push((oi, ii, it.url.clone()));
            }
        }
    }
    for chunk in targets.chunks(PAGE_IMAGE_CONCURRENCY) {
        let mut set = tokio::task::JoinSet::new();
        for (oi, ii, url) in chunk.iter().cloned() {
            let http = http.clone();
            set.spawn(async move {
                let found = tokio::time::timeout(
                    PAGE_IMAGE_TIMEOUT,
                    crate::page::fetch_html_head(&http, &url, PAGE_IMAGE_MAX_BYTES, allow_loopback),
                )
                .await
                .ok()
                .and_then(|r| r.ok())
                .and_then(|(final_url, html)| og_image(&html, &final_url));
                (oi, ii, found)
            });
        }
        while let Some(Ok((oi, ii, found))) = set.join_next().await {
            if let (Some(img), Ok(items)) = (found, &mut outcomes[oi].result) {
                items[ii].image_url = Some(img);
            }
        }
    }
}
```

`JoinSet` には tokio の `rt` 機能が要る (`cargo add -p infodesk-core tokio --features rt`)。`SourceKind` が `PartialEq` でなければ `matches!` にする。`FetchOutcome` は `collector/mod.rs` で `pub`。

CLI の `collect` (`crates/cli/src/lib.rs`):

```rust
    let mut outcomes = collector::fetch_all(&http, &store.list_sources()?).await;
    let urls: Vec<String> = outcomes.iter().filter_map(|o| o.result.as_ref().ok()).flatten().map(|i| i.url.clone()).collect();
    let known = store.existing_urls(&urls)?;
    collector::images::fill_page_images(&mut outcomes, &known).await;
```

アプリの `collect_inner` (`app/src-tauri/src/jobs.rs`): 同じ順で、`existing_urls` は `state.with_store(|s| s.existing_urls(&urls))?` で呼ぶ (ロックは同期の closure の中だけ)。既存の `jobs.rs` のテスト (wiremock で RSS を返す) は、記事の URL がテスト用のサーバー (127.0.0.1) を指すので、本番のクライアントでは画像の取得が拒否されて `None` になる。収集の結果は変わらないことを既存のテストで確かめる。

- [ ] **Step 4: 通ることを確かめる**

Run: `cargo test --workspace && cargo clippy --all-targets -- -D warnings && cargo fmt --check` → PASS

手で確かめる (ネットワークがある場合):

```bash
DB=$(mktemp -d)/t.db
cargo run -p infodesk -- --db $DB source add-rss https://zenn.dev/feed --category tech_jp
cargo run -p infodesk -- --db $DB collect
sqlite3 $DB "SELECT count(*), count(image_url) FROM items;"
```

Expected: 2つ目の数が 0 より大きい。

- [ ] **Step 5: コミット**

```bash
git add crates app/src-tauri/src Cargo.lock
git commit -m "feat(core): find og:image for new articles during collection"
```

---

### Task 3: 記事行のサムネイルと CSP

**Files:**
- Modify: `app/src/types.ts`、`app/src/views/FeedView.tsx`、`app/src/views/FeedView.test.tsx`、`app/src/styles/layout.css`、`app/src-tauri/tauri.conf.json`、`app/src/chat/ChatThread.tsx`、`app/src/chat/ChatThread.test.tsx`
- Test: `FeedView.test.tsx`、`ChatThread.test.tsx`

**Interfaces:**
- Consumes: Task 1 の `Item.image_url` (JSON `image_url: string | null`)
- Produces: `Item.image_url: string | null` (types.ts)

- [ ] **Step 1: 失敗するテストを書く**

`FeedView.test.tsx` に追加 (既存のフィクスチャの記事に `image_url` を足す。既存の記事は `image_url: null`):

```tsx
  it("shows a thumbnail for articles with an https image and hides it when it fails to load", async () => {
    // 既存の setupIpc と記事のフィクスチャの作り方に合わせる。1件目に画像、2件目は null。
    const items = [
      { ...ITEM_A, image_url: "https://img.example.com/a.jpg" },
      { ...ITEM_B, image_url: null },
    ];
    setupIpc({ ...BASE_HANDLERS, list_items: () => items, count_items: () => 2 });
    renderFeed();
    const img = await screen.findByRole("img", { hidden: true });
    expect(img).toHaveAttribute("src", "https://img.example.com/a.jpg");
    expect(img).toHaveAttribute("loading", "lazy");
    expect(img).toHaveAttribute("referrerpolicy", "no-referrer");
    expect(img).toHaveAttribute("alt", "");
    expect(screen.getAllByRole("img", { hidden: true })).toHaveLength(1);
    fireEvent.error(img);
    expect(screen.queryByRole("img", { hidden: true })).toBeNull();
  });

  it("does not render non-https image URLs", async () => {
    setupIpc({ ...BASE_HANDLERS, list_items: () => [{ ...ITEM_A, image_url: "http://img.example.com/a.jpg" }], count_items: () => 1 });
    renderFeed();
    await screen.findByText(ITEM_A.title);
    expect(screen.queryByRole("img", { hidden: true })).toBeNull();
  });
```

`ITEM_A`、`ITEM_B`、`BASE_HANDLERS`、`renderFeed` は `FeedView.test.tsx` の既存の補助の名前に置き換える。`alt=""` の画像は装飾として扱われ、`getByRole("img")` で見えないため `{ hidden: true }` を付ける。

`ChatThread.test.tsx` に追加 (既存の `mockFetch`、`streamResponse`、`answerChunks`、`renderThread`、`ask` を使う):

```tsx
  it("does not load images written in an answer", async () => {
    setupIpc({});
    mockFetch(() => streamResponse(answerChunks("結果 ![図](https://evil.example.com/?q=secret) です")));
    renderThread();
    await ask("質問");
    expect(await screen.findByText(/結果/)).toBeInTheDocument();
    expect(document.querySelector(".chat-markdown img")).toBeNull();
    expect(screen.getByText("図")).toBeInTheDocument();
  });
```

- [ ] **Step 2: 失敗を確かめる**

Run: `cd app && npm test -- --run src/views/FeedView.test.tsx src/chat/ChatThread.test.tsx` → FAIL

- [ ] **Step 3: 実装する**

`types.ts` の `Item` に `image_url: string | null;`。

`FeedView.tsx`: 記事行で、`.content` の後、`row-actions` の前にサムネイルを置く。読み込みに失敗した画像の id を state に持って隠す。

```tsx
function Thumbnail({ src }: { src: string }) {
  const [failed, setFailed] = useState(false);
  if (failed || !/^https:\/\//i.test(src)) return null;
  return (
    <img className="thumb" src={src} alt="" loading="lazy" decoding="async"
      referrerPolicy="no-referrer" onError={() => setFailed(true)} />
  );
}
```

行の中: `{it.image_url && <Thumbnail key={it.image_url} src={it.image_url} />}`。React の `referrerPolicy` は DOM の `referrerpolicy` 属性になる。

`layout.css`:

```css
.item-row .thumb { width: 96px; height: 64px; flex: none; border-radius: 5px; object-fit: cover; background: var(--bg-subtle); align-self: flex-start; }
```

`tauri.conf.json` の CSP の `img-src 'self' data:` を `img-src 'self' data: https:` にする。

`ChatThread.tsx`: `MARKDOWN_COMPONENTS` に画像の代わりを足す。

```tsx
/** 回答の中の画像は読み込まない (CSP が https の画像を許すため、表示だけで外部へ要求が出てしまう)。代替テキストだけを出す。 */
function AnswerImage({ alt }: ComponentPropsWithoutRef<"img"> & { node?: unknown }) {
  return alt ? <span className="chat-img-alt">{alt}</span> : null;
}

const MARKDOWN_COMPONENTS = { a: AnswerLink, img: AnswerImage };
```

- [ ] **Step 4: 通ることを確かめる**

Run: `cd app && npm test -- --run && npm run build` → PASS (警告なし)

- [ ] **Step 5: 画面で確かめる**

スクリーンショットの仕組みがある場合 (`/private/tmp/claude-502/-Users-ktakeuchi-Documents-InfoDesk/efc0adfa-185e-4966-8a22-cfb8e7e7f6dc/scratchpad/reshoot.sh`、モックは同じディレクトリの `mock.js`)、`mock.js` の記事に `image_url` を足し (外部の画像は読めないので、`data:` ではなく、モック用に `https://` の URL を入れ、読み込み失敗で隠れることも確かめる)、`feed-light|1200x800|?go=フィード` を撮って行の配置を見る。仕組みが無い場合はこの手順を飛ばし、報告に書く。

- [ ] **Step 6: コミット**

```bash
git add app/src app/src-tauri/tauri.conf.json
git commit -m "feat(ui): show article thumbnails in the feed"
```

---

### Task 4: サイドカーの作業計画と時刻

**Files:**
- Modify: `agent/src/tools.ts`、`agent/src/prompt.ts`、`agent/src/chat.ts`、`agent/test/tools.test.ts`、`agent/test/prompt.test.ts`、`agent/test/server.test.ts`

**Interfaces:**
- Consumes: 既存の `createInfodeskTools`、`buildInstructions`、`streamChat`
- Produces:
  - ツール `update_plan`、入力 `{ items: { title: string (1〜80字), status: "pending" | "in_progress" | "done" }[] (1〜8件) }`、結果 `{ ok: true, count: <件数> }`。`TOOL_NAMES` に足す。Claude では `mcp__infodesk__update_plan` (`allowedTools` に足す)。
  - メッセージのメタデータ: 回答の `start` で `{ startedAt: <ISO> }`、`finish` で `{ finishedAt: <ISO> }`。両方の経路 (Ollama の `pipeAgentUIStreamToResponse`、Claude の `pipeUIMessageStreamToResponse`) で `messageMetadata` と `sendReasoning: true` を渡す。
  - 指示文: 「ツールを2回以上使う見込みの質問では、最初に update_plan で手順を3〜6項目で示し、進むたびに状態を更新する。最後は全項目を done にする」。

- [ ] **Step 1: 失敗するテストを書く**

`tools.test.ts`:

```ts
  it("accepts a plan update and validates it", async () => {
    const tools = createInfodeskTools(run, {});
    const plan = { items: [{ title: "記事を検索", status: "done" }, { title: "まとめる", status: "in_progress" }] };
    expect(await exec(tools, "update_plan", plan)).toEqual({ ok: true, count: 2 });
    const schema = (tools.update_plan as any).inputSchema;
    expect(schema.safeParse({ items: [] }).success).toBe(false);
    expect(schema.safeParse({ items: Array.from({ length: 9 }, () => ({ title: "x", status: "pending" })) }).success).toBe(false);
    expect(schema.safeParse({ items: [{ title: "x", status: "doing" }] }).success).toBe(false);
    expect(TOOL_NAMES).toContain("update_plan");
  });
```

既存の `offers web_search only when a key is set` の名前の一覧に `update_plan` を足す (並びは `TOOL_NAMES` と同じ)。

`prompt.test.ts`:

```ts
  it("asks for a plan on multi-step questions", () => {
    const s = buildInstructions(null, NOW, [...ALL, "update_plan"]);
    expect(s).toContain("update_plan");
    expect(s).toContain("done");
  });
```

`server.test.ts` (既存の Ollama のテストに追加):

```ts
  it("sends start and finish times as message metadata", async () => {
    const model = new MockLanguageModelV4({
      doStream: async () => ({ stream: simulateReadableStream({ chunks: [
        { type: "text-start", id: "t" }, { type: "text-delta", id: "t", delta: "はい" }, { type: "text-end", id: "t" },
        { type: "finish", finishReason: { unified: "stop", raw: undefined }, usage },
      ] as never }) }),
    });
    const d = deps(model);
    const { base } = await start((req, res, signal) => streamChat(req, res, d, signal));
    const res = await fetch(`${base}/chat`, { method: "POST", headers: { Authorization: `Bearer ${TOKEN}` }, body: body() });
    const chunks = sseChunks(await res.text());
    const meta = chunks.filter((c) => c.messageMetadata).map((c) => c.messageMetadata);
    expect(meta.some((m) => typeof m.startedAt === "string")).toBe(true);
    expect(meta.some((m) => typeof m.finishedAt === "string")).toBe(true);
  });
```

メタデータが UI message stream のどの chunk に乗るか (`start`、`finish`、`message-metadata` の `messageMetadata` フィールド) は `node_modules/ai/dist/index.d.ts` の `UIMessageChunk` で確かめ、テストの取り出し方を合わせる。Claude の経路の既存テスト (`allowedTools` の一覧) に `mcp__infodesk__update_plan` を足す。

- [ ] **Step 2: 失敗を確かめる**

Run: `cd agent && npm test -- --run` → FAIL

- [ ] **Step 3: 実装する**

`tools.ts` (`TOOL_NAMES` に `"update_plan"` を足す。順は `search_articles, get_article, read_digests, fetch_page, update_plan, web_search`):

```ts
    update_plan: tool({
      description:
        "作業計画を示す、または更新する。ツールを2回以上使う質問で最初に呼び、進むたびに各項目の状態を更新する。",
      inputSchema: z.object({
        items: z.array(z.object({
          title: z.string().min(1).max(80),
          status: z.enum(["pending", "in_progress", "done"]),
        })).min(1).max(8),
      }),
      execute: async ({ items }) => ({ ok: true, count: items.length }),
    }),
```

`prompt.ts`: ツールの使い方の行に、`tools.includes("update_plan")` のとき「- ツールを2回以上使う見込みの質問では、最初に update_plan で手順を3〜6項目で示し、進むたびに状態 (pending / in_progress / done) を更新する。最後は全項目を done にする。」を足す。Claude の経路は `[...names, "web_search"]` を渡しているので `update_plan` も含まれる。

`chat.ts`: 両方の pipe に次を渡す。

```ts
const metadata = ({ part }: { part: { type: string } }) =>
  part.type === "start" ? { startedAt: new Date().toISOString() }
  : part.type === "finish" ? { finishedAt: new Date().toISOString() }
  : undefined;
// pipeAgentUIStreamToResponse({ ..., messageMetadata: metadata, sendReasoning: true })
// result.pipeUIMessageStreamToResponse(res, { onError, messageMetadata: metadata, sendReasoning: true })
```

時刻は `deps.now()` を使ってテストで固定できるようにしてもよい (その場合 `ChatDeps.now` を使う)。

- [ ] **Step 4: 通ることを確かめる**

Run: `cd agent && npm test -- --run && npm run build && node dist/agent.mjs --check` → PASS、`ok`

- [ ] **Step 5: コミット**

```bash
git add agent/src agent/test
git commit -m "feat(agent): plan updates and answer timing for the chat progress"
```

---

### Task 5: 途中経過の計算 (純粋な関数)

**Files:**
- Create: `app/src/chat/progress.ts`、`app/src/chat/progress.test.ts`、`app/src/chat/toolDetails.ts`、`app/src/chat/toolDetails.test.ts`
- Modify: `app/src/chat/toolLabels.ts` (`update_plan` を通常のツールとして扱わない印)

**Interfaces:**
- Consumes: Task 4 のツール名と出力の形、メタデータ
- Produces:
  - `progress.ts`:
    - `type PlanItem = { title: string; status: "pending" | "in_progress" | "done" }`
    - `isPlanTool(name: string): boolean` (`update_plan` または `mcp__infodesk__update_plan`)
    - `latestPlan(message: UIMessage): PlanItem[] | null` (その回答で最後の `update_plan` の入力。入力の形が不正なら null)
    - `workStats(message: UIMessage): { steps: number; tools: number }` (`steps` = 思考の部品 + 計画以外のツール部品、`tools` = 計画以外のツール部品)
    - `answerTimes(message: UIMessage): { startedAt: number | null; finishedAt: number | null }` (`message.metadata` の ISO を数値に。無ければ null)
    - `formatSeconds(ms: number): string` (`12秒`、`1分05秒`)
    - `statusLine(args: { running: boolean; elapsedMs: number | null; steps: number; tools: number }): string` (回答中: `調査中 · 12秒 · ツール3回`。ツール0回なら `調査中 · 12秒`。経過不明なら `調査中`。完了: `4ステップ · 18秒`。時刻不明なら `4ステップ`)
  - `toolDetails.ts`: `toolDetails(toolName: string, args: unknown, result: unknown, errorText?: string): { label: string; value: string }[]` (spec 14節の表のとおり。記事検索のタイトルは最大20件。本文取得の冒頭は300字)

- [ ] **Step 1: 失敗するテストを書く**

`progress.test.ts`:

```ts
import type { UIMessage } from "ai";
import { describe, expect, it } from "vitest";
import { answerTimes, formatSeconds, isPlanTool, latestPlan, statusLine, workStats } from "./progress";

const msg = (parts: unknown[], metadata?: unknown): UIMessage => ({ id: "a", role: "assistant", parts, metadata } as UIMessage);
const plan = (items: unknown, toolCallId = "p") => ({ type: "tool-update_plan", toolCallId, state: "output-available", input: { items }, output: { ok: true } });

describe("progress", () => {
  it("recognizes the plan tool on both providers", () => {
    expect(isPlanTool("update_plan")).toBe(true);
    expect(isPlanTool("mcp__infodesk__update_plan")).toBe(true);
    expect(isPlanTool("search_articles")).toBe(false);
  });

  it("returns the latest valid plan", () => {
    const m = msg([
      plan([{ title: "検索", status: "in_progress" }], "p1"),
      { type: "tool-search_articles", toolCallId: "s", state: "output-available", input: {}, output: { items: [], total: 0 } },
      plan([{ title: "検索", status: "done" }, { title: "まとめる", status: "in_progress" }], "p2"),
    ]);
    expect(latestPlan(m)).toEqual([{ title: "検索", status: "done" }, { title: "まとめる", status: "in_progress" }]);
    expect(latestPlan(msg([plan([{ title: "x", status: "doing" }])]))).toBeNull();
    expect(latestPlan(msg([{ type: "text", text: "x" }]))).toBeNull();
    const claude = msg([{ type: "dynamic-tool", toolName: "mcp__infodesk__update_plan", toolCallId: "c", state: "input-available", input: { items: [{ title: "a", status: "pending" }] } }]);
    expect(latestPlan(claude)).toEqual([{ title: "a", status: "pending" }]);
  });

  it("counts steps without plan updates", () => {
    const m = msg([
      { type: "reasoning", text: "考え" },
      plan([{ title: "x", status: "done" }]),
      { type: "tool-search_articles", toolCallId: "s", state: "output-available", input: {}, output: {} },
      { type: "dynamic-tool", toolName: "mcp__infodesk__fetch_page", toolCallId: "f", state: "output-available", input: {}, output: {} },
      { type: "text", text: "答え" },
    ]);
    expect(workStats(m)).toEqual({ steps: 3, tools: 2 });
  });

  it("reads times from metadata", () => {
    expect(answerTimes(msg([], { startedAt: "2026-10-10T00:00:00.000Z", finishedAt: "2026-10-10T00:00:18.000Z" })))
      .toEqual({ startedAt: Date.parse("2026-10-10T00:00:00.000Z"), finishedAt: Date.parse("2026-10-10T00:00:18.000Z") });
    expect(answerTimes(msg([]))).toEqual({ startedAt: null, finishedAt: null });
    expect(answerTimes(msg([], { startedAt: "not a date" }))).toEqual({ startedAt: null, finishedAt: null });
  });

  it("formats the status line", () => {
    expect(formatSeconds(12_400)).toBe("12秒");
    expect(formatSeconds(65_000)).toBe("1分05秒");
    expect(statusLine({ running: true, elapsedMs: 12_000, steps: 3, tools: 3 })).toBe("調査中 · 12秒 · ツール3回");
    expect(statusLine({ running: true, elapsedMs: 2_000, steps: 0, tools: 0 })).toBe("調査中 · 2秒");
    expect(statusLine({ running: true, elapsedMs: null, steps: 1, tools: 1 })).toBe("調査中 · ツール1回");
    expect(statusLine({ running: false, elapsedMs: 18_000, steps: 4, tools: 3 })).toBe("4ステップ · 18秒");
    expect(statusLine({ running: false, elapsedMs: null, steps: 4, tools: 3 })).toBe("4ステップ");
  });
});
```

`toolDetails.test.ts`:

```ts
import { describe, expect, it } from "vitest";
import { toolDetails } from "./toolDetails";

describe("toolDetails", () => {
  it("shows search conditions and up to 20 titles", () => {
    const items = Array.from({ length: 25 }, (_, i) => ({ id: i, title: `記事${i}` }));
    const rows = toolDetails("search_articles", { query: "認知症", category: "medcare", days: 7 }, { items, total: 25 });
    expect(rows[0]).toEqual({ label: "条件", value: "認知症 · 医療介護 · 直近7日" });
    expect(rows[1].label).toBe("記事 (25件)");
    expect(rows[1].value.split("\n")).toHaveLength(20);
  });

  it("shows an article, digests, a page and web results", () => {
    expect(toolDetails("get_article", { id: 1 }, { item: { title: "T", source: "S", summary: "要約" } }))
      .toEqual([{ label: "記事", value: "T" }, { label: "ソース", value: "S" }, { label: "要約", value: "要約" }]);
    expect(toolDetails("read_digests", { period: "week" }, { period: "week", period_start: "2026-10-05", digests: [{ label: "医療介護", status: "done" }, { label: "GitHub", status: "none" }] }))
      .toEqual([{ label: "期間", value: "週次 (2026-10-05〜)" }, { label: "要約", value: "医療介護: 生成済み\nGitHub: なし" }]);
    const page = toolDetails("mcp__infodesk__fetch_page", { url: "https://e.com/a" }, { page: { title: "P", text: "あ".repeat(400), truncated: true } });
    expect(page[0]).toEqual({ label: "ページ", value: "P" });
    expect(page[1].value.length).toBe(301);
    expect(page[2]).toEqual({ label: "切り詰め", value: "あり" });
    expect(toolDetails("web_search", { query: "q" }, { results: [{ title: "R", url: "https://r.com" }] }))
      .toEqual([{ label: "検索語", value: "q" }, { label: "結果", value: "R · https://r.com" }]);
  });

  it("shows the error text for a failed tool", () => {
    expect(toolDetails("get_article", { id: 404 }, undefined, "item 404 not found")).toEqual([{ label: "エラー", value: "item 404 not found" }]);
  });
});
```

Web検索の結果は1件1行で「タイトル · URL」と書く。

- [ ] **Step 2: 失敗を確かめる**

Run: `cd app && npm test -- --run src/chat/progress.test.ts src/chat/toolDetails.test.ts` → FAIL

- [ ] **Step 3: 実装する**

`progress.ts` と `toolDetails.ts` をテストのとおりに書く。カテゴリーの表示名は `types.ts` の `CATEGORIES`、期間の表示名は `day: 日次`、`week: 週次`、`month: 月次`、要約の状態は `done: 生成済み`、`pending: 保留`、`failed: 失敗`、`none: なし`。Claude のツール結果が MCP のテキスト (`{ content: [{ type: "text", text: "<JSON>" }] }`) や JSON 文字列で来る場合に備え、`references.ts` の `unwrap` と同じ処理で結果を開く (共通の関数に切り出して両方から使う)。ツール名の `mcp__infodesk__` は `toolLabels.ts` と同じく外す。

- [ ] **Step 4: 通ることを確かめる**

Run: `cd app && npm test -- --run src/chat` → PASS

- [ ] **Step 5: コミット**

```bash
git add app/src/chat
git commit -m "feat(ui): compute chat progress and tool details"
```

---

### Task 6: 途中経過の表示

**Files:**
- Create: `app/src/chat/WorkSummary.tsx`、`app/src/chat/Reasoning.tsx`、`app/src/chat/WorkSummary.test.tsx`
- Modify: `app/src/chat/ChatThread.tsx`、`app/src/chat/ToolCall.tsx`、`app/src/chat/ToolCall.test.tsx`、`app/src/chat/ChatThread.test.tsx`、`app/src/styles/chat.css`

**Interfaces:**
- Consumes: Task 5 の `latestPlan`、`workStats`、`answerTimes`、`statusLine`、`isPlanTool`、`toolDetails`、`ChatThread.tsx` の既存の `ChatMessagesContext` (元の `UIMessage`)
- Produces:
  - `WorkSummary` (回答の上): 状態行 (回答中は1秒ごとに更新)、チェックリスト。完了後は状態行がボタンになり、作業 (思考とツール) の開閉を切り替える。
  - `WorkCollapsedContext` (`boolean`): `true` のとき `ToolCall` と `Reasoning` は何も描かない。回答中は `false`。完了後の初期値は `true`。
  - `ToolCall`: 行がボタンになり (`aria-expanded`)、押すと `toolDetails` の行を `<dl>` で出す。`update_plan` は描かない。
  - `Reasoning`: 「考え中…」(回答中) / 「考えた内容」(完了後) の見出しのボタンと、開くと全文。

- [ ] **Step 1: 失敗するテストを書く**

`WorkSummary.test.tsx` (純粋な表示部品として、`UIMessage` と `running` を props で渡す):

```tsx
import { act, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { UIMessage } from "ai";
import { afterEach, describe, expect, it, vi } from "vitest";
import { WorkSummary } from "./WorkSummary";

const plan = { type: "tool-update_plan", toolCallId: "p", state: "output-available", input: { items: [{ title: "記事を検索", status: "done" }, { title: "まとめる", status: "in_progress" }] }, output: { ok: true } };
const search = { type: "tool-search_articles", toolCallId: "s", state: "output-available", input: {}, output: { items: [], total: 0 } };
const msg = (parts: unknown[], metadata?: unknown): UIMessage => ({ id: "a", role: "assistant", parts, metadata } as UIMessage);

afterEach(() => vi.useRealTimers());

describe("WorkSummary", () => {
  it("shows the live status and ticks every second while running", () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-10-10T00:00:05.000Z"));
    render(<WorkSummary message={msg([plan, search], { startedAt: "2026-10-10T00:00:00.000Z" })} running collapsed={false} onToggle={() => {}} />);
    expect(screen.getByText("調査中 · 5秒 · ツール1回")).toBeInTheDocument();
    act(() => { vi.advanceTimersByTime(2000); });
    expect(screen.getByText("調査中 · 7秒 · ツール1回")).toBeInTheDocument();
  });

  it("shows the latest plan as a checklist", () => {
    render(<WorkSummary message={msg([plan])} running collapsed={false} onToggle={() => {}} />);
    const list = screen.getByRole("list", { name: "作業計画" });
    expect(list).toHaveTextContent("記事を検索");
    expect(screen.getByText("記事を検索").closest("li")).toHaveAttribute("data-status", "done");
    expect(screen.getByText("まとめる").closest("li")).toHaveAttribute("data-status", "in_progress");
  });

  it("collapses into a summary button after the answer", async () => {
    const onToggle = vi.fn();
    render(<WorkSummary message={msg([{ type: "reasoning", text: "考え" }, plan, search], { startedAt: "2026-10-10T00:00:00.000Z", finishedAt: "2026-10-10T00:00:18.000Z" })} running={false} collapsed onToggle={onToggle} />);
    const btn = screen.getByRole("button", { name: "2ステップ · 18秒" });
    expect(btn).toHaveAttribute("aria-expanded", "false");
    await userEvent.setup().click(btn);
    expect(onToggle).toHaveBeenCalled();
  });

  it("omits the duration for old messages and renders nothing without work", () => {
    const { rerender, container } = render(<WorkSummary message={msg([search])} running={false} collapsed onToggle={() => {}} />);
    expect(screen.getByRole("button", { name: "1ステップ" })).toBeInTheDocument();
    rerender(<WorkSummary message={msg([{ type: "text", text: "答えだけ" }])} running={false} collapsed onToggle={() => {}} />);
    expect(container).toBeEmptyDOMElement();
  });
});
```

`ToolCall.test.tsx` に追加:

```tsx
  it("opens the input and result details", async () => {
    render(<ToolCall {...props({ toolName: "search_articles", args: { query: "認知症" }, result: { items: [{ id: 1, title: "記事A" }], total: 1 }, status: { type: "complete" } })} />);
    const row = screen.getByRole("button", { name: /記事検索/ });
    expect(row).toHaveAttribute("aria-expanded", "false");
    await userEvent.setup().click(row);
    expect(row).toHaveAttribute("aria-expanded", "true");
    expect(screen.getByText("記事A")).toBeInTheDocument();
  });

  it("renders nothing for plan updates", () => {
    const { container } = render(<ToolCall {...props({ toolName: "update_plan", args: { items: [] }, status: { type: "complete" } })} />);
    expect(container).toBeEmptyDOMElement();
  });
```

`props(...)` は `ToolCall.test.tsx` の既存の補助に合わせる (無ければ `ToolCallMessagePartProps` に必要な最小のフィールドを組み立てる関数を足す)。

`ChatThread.test.tsx` に追加 (ストリームに思考、計画、検索、本文を流す):

```tsx
  it("shows the plan and work while answering, then folds the work into one line", async () => {
    setupIpc({ open_item: () => null });
    mockFetch(() => streamResponse([
      { type: "start", messageMetadata: { startedAt: new Date(Date.now() - 3000).toISOString() } },
      { type: "reasoning-start", id: "r" }, { type: "reasoning-delta", id: "r", delta: "まず検索する" }, { type: "reasoning-end", id: "r" },
      { type: "tool-input-available", toolCallId: "p", toolName: "update_plan", input: { items: [{ title: "検索", status: "done" }, { title: "まとめる", status: "done" }] } },
      { type: "tool-output-available", toolCallId: "p", output: { ok: true, count: 2 } },
      { type: "tool-input-available", toolCallId: "s", toolName: "search_articles", input: { query: "認知症" } },
      { type: "tool-output-available", toolCallId: "s", output: { items: [{ id: 1, title: "記事A" }], total: 1 } },
      { type: "text-start", id: "t" }, { type: "text-delta", id: "t", delta: "答えです" }, { type: "text-end", id: "t" },
      { type: "finish", messageMetadata: { finishedAt: new Date().toISOString() } },
    ] as never));
    renderThread();
    await ask("質問");
    expect(await screen.findByText("答えです")).toBeInTheDocument();
    expect(screen.getByRole("list", { name: "作業計画" })).toHaveTextContent("まとめる");
    const summary = screen.getByRole("button", { name: /^2ステップ · \d+秒$/ });
    expect(summary).toHaveAttribute("aria-expanded", "false");
    expect(screen.queryByText("記事検索")).toBeNull();
    expect(screen.queryByText("update_plan")).toBeNull();
    await userEvent.setup().click(summary);
    expect(screen.getByText("記事検索")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "考えた内容" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "記事A" })).toBeInTheDocument();
  });
```

`messageMetadata` が `start` / `finish` の chunk に乗る形は Task 4 で確かめた形に合わせる。参照記事のボタン (`記事A`) は折りたたみの外で、常に見える。

- [ ] **Step 2: 失敗を確かめる**

Run: `cd app && npm test -- --run src/chat` → FAIL

- [ ] **Step 3: 実装する**

`WorkSummary.tsx`:

```tsx
import type { UIMessage } from "ai";
import { Check, ChevronRight, Circle, LoaderCircle } from "lucide-react";
import { createContext, useEffect, useState } from "react";
import { answerTimes, latestPlan, statusLine, workStats } from "./progress";

/** 完了した回答で作業 (思考とツール) を隠すか。回答中は false。 */
export const WorkCollapsedContext = createContext(false);

function useNow(active: boolean): number {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    if (!active) return;
    const id = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(id);
  }, [active]);
  return now;
}

const ICON = { done: Check, in_progress: LoaderCircle, pending: Circle } as const;

export function WorkSummary({ message, running, collapsed, onToggle }: {
  message: UIMessage; running: boolean; collapsed: boolean; onToggle(): void;
}) {
  const now = useNow(running);
  const plan = latestPlan(message);
  const { steps, tools } = workStats(message);
  const { startedAt, finishedAt } = answerTimes(message);
  const end = running ? now : finishedAt;
  const elapsedMs = startedAt !== null && end !== null ? Math.max(0, end - startedAt) : null;
  if (!running && steps === 0 && !plan) return null;
  const line = statusLine({ running, elapsedMs, steps, tools });
  return (
    <div className="work-summary">
      {running ? (
        <div className="work-status" role="status"><LoaderCircle size={13} className="spin" aria-hidden />{line}</div>
      ) : steps > 0 ? (
        <button className="work-status" aria-expanded={!collapsed} onClick={onToggle}>
          <ChevronRight size={13} className={collapsed ? "" : "open"} aria-hidden />{line}
        </button>
      ) : null}
      {plan && (
        <ul className="work-plan" aria-label="作業計画">
          {plan.map((p, i) => {
            const Icon = ICON[p.status];
            return (
              <li key={i} data-status={p.status}>
                <Icon size={13} aria-hidden className={p.status === "in_progress" ? "spin" : undefined} />{p.title}
              </li>
            );
          })}
        </ul>
      )}
    </div>
  );
}
```

`ChatThread.tsx` の `AssistantMessage`: 元の `UIMessage` (既存の `ChatMessagesContext` から取る) を `WorkSummary` に渡し、折りたたみの状態を持つ。

```tsx
function AssistantMessage() {
  const id = useAuiState((s) => s.message.id);
  const empty = useAuiState((s) => s.message.content.length === 0);
  const running = useAuiState((s) => s.message.status?.type === "running");
  const original = useContext(ChatMessagesContext).find((m) => m.id === id);
  const [expanded, setExpanded] = useState(false);
  if (empty) return null;
  const collapsed = !running && !expanded;
  return (
    <MessagePrimitive.Root className="chat-msg assistant">
      {original && <WorkSummary message={original} running={running} collapsed={collapsed} onToggle={() => setExpanded((v) => !v)} />}
      <WorkCollapsedContext.Provider value={collapsed}>
        <MessagePrimitive.Parts components={{ Text: MarkdownText, Reasoning, tools: { Fallback: ToolCall } }} />
      </WorkCollapsedContext.Provider>
      {original && !running && <References message={original} />}
    </MessagePrimitive.Root>
  );
}
```

`Reasoning` の props の型は `@assistant-ui/react` の `ReasoningMessagePartProps` (無ければ `@assistant-ui/core` の `ReasoningMessagePartComponent` から取る)。`MessagePrimitive.Parts` の `components.Reasoning` の名前は `node_modules/@assistant-ui/core/dist/react/primitives/message/MessageParts.d.ts:93` で確認済み。

`Reasoning.tsx`:

```tsx
import { ChevronRight } from "lucide-react";
import { useContext, useState } from "react";
import { WorkCollapsedContext } from "./WorkSummary";

export function Reasoning({ text, status }: { text: string; status?: { type: string } }) {
  const collapsed = useContext(WorkCollapsedContext);
  const [open, setOpen] = useState(false);
  if (collapsed || !text.trim()) return null;
  const thinking = status?.type === "running";
  return (
    <div className="reasoning">
      <button className="reasoning-head" aria-expanded={open} onClick={() => setOpen((v) => !v)}>
        <ChevronRight size={13} className={open ? "open" : ""} aria-hidden />{thinking ? "考え中…" : "考えた内容"}
      </button>
      {open && <div className="reasoning-body">{text}</div>}
    </div>
  );
}
```

`ToolCall.tsx`: 先頭で `isPlanTool(toolName)` なら `null`、`WorkCollapsedContext` が `true` なら `null`。行を `<button className="tool-call" aria-expanded={open}>` にし、開いたら `toolDetails(toolName, args, result, isError ? String(result ?? "") : undefined)` を `<dl className="tool-details">` で出す。失敗したツールのエラー文が `result` に入らない場合は、assistant-ui のツール部品の props でエラー文が入る場所を型定義で確かめて渡す。既存の ToolCall のテスト (状態の表示) が通ることを確かめる (行の名前が変わる場合は `getByRole("button", { name: /記事検索/ })` で取る)。

`chat.css`:

```css
.work-summary { display: flex; flex-direction: column; gap: 6px; margin-bottom: 4px; }
.work-status { display: inline-flex; align-items: center; gap: 6px; font-size: 12px; color: var(--text-secondary); background: none; border: 0; padding: 0; align-self: flex-start; }
.work-status .open, .reasoning-head .open { transform: rotate(90deg); }
.work-plan { list-style: none; margin: 0; padding: 8px 10px; border: 1px solid var(--border); border-radius: 7px; background: var(--bg-window); display: flex; flex-direction: column; gap: 4px; font-size: 12px; }
.work-plan li { display: flex; align-items: center; gap: 6px; color: var(--text-secondary); }
.work-plan li[data-status="done"] { color: var(--text-tertiary); text-decoration: line-through; }
.work-plan li[data-status="in_progress"] { color: var(--text-primary); font-weight: 600; }
.reasoning-head { display: inline-flex; align-items: center; gap: 6px; font-size: 12px; color: var(--text-secondary); background: none; border: 0; padding: 0; }
.reasoning-body { font-size: 12px; color: var(--text-secondary); white-space: pre-wrap; border-left: 2px solid var(--border); padding-left: 10px; margin: 4px 0; }
.tool-call { width: 100%; background: none; border: 0; text-align: left; cursor: pointer; }
.tool-details { margin: 2px 0 6px 19px; font-size: 12px; display: grid; grid-template-columns: auto 1fr; gap: 2px 10px; color: var(--text-secondary); }
.tool-details dt { color: var(--text-tertiary); }
.tool-details dd { margin: 0; white-space: pre-wrap; overflow-wrap: anywhere; }
```

`.spin` は既存の全体のクラス (`theme.css`)。`.tool-call` の既存の規則 (flex、gap) は残し、ボタンにしたことで付く既定の見た目だけを消す。

- [ ] **Step 4: 通ることを確かめる**

Run: `cd app && npm test -- --run && npm run build` → PASS (警告なし)

- [ ] **Step 5: 画面で確かめる**

スクリーンショットの仕組みがある場合、`mock.js` の保存済みの会話 (`get_chat_messages`) に思考、`update_plan`、メタデータを足し、`chat-light|1200x800|?go=フィード,チャットを開く,履歴,認知症の今週の動き` と同じ条件のダークを撮って、要約行、チェックリスト、折りたたみを見る。要約行を押して開いた状態も撮る (`go` に要約行の名前を足す)。仕組みが無い場合は飛ばして報告に書く。

- [ ] **Step 6: コミット**

```bash
git add app/src
git commit -m "feat(ui): show the agent's plan, thinking, tool details and timing"
```

---

### Task 7: 確認と README

**Files:**
- Modify: `README.md`

- [ ] **Step 1: 全体を確かめる**

```bash
cargo test --workspace && cargo clippy --all-targets -- -D warnings && cargo fmt --check
cd app && npm test -- --run && npm run build && cd ..
cd agent && npm test -- --run && npm run build && node dist/agent.mjs --check && cd ..
cd app && npm run bundle && cd ..
```

- [ ] **Step 2: 実データで画像の取得を確かめる (読み取り専用の確認)**

利用者の DB には書き込まない。一時 DB で収集する:

```bash
DB=$(mktemp -d)/t.db
cargo run -p infodesk -- --db $DB collect
sqlite3 $DB "SELECT source_name, count(*), count(image_url) FROM items GROUP BY source_name;"
```

ソースごとの画像の付き方を報告に書く (arXiv と Google News は 0 のはず)。

- [ ] **Step 3: サイドカーで計画と時刻を確かめる**

Plan 3 Task 9 と同じ手順でサイドカーを単体で起動し (`INFODESK_DB` は上の一時 DB)、Ollama で「今週の医療介護の動きを調べて、記事を2本読んでまとめて」を送る。流れに `tool-input-available` の `update_plan`、`reasoning-delta` (gemma4 の場合)、`messageMetadata` の `startedAt`/`finishedAt` が含まれるかを報告に書く。モデルが計画を出さなかった場合もそのまま書く (指示文の調整は利用者に判断を仰ぐ)。

- [ ] **Step 4: README を更新する**

「## アプリ」に「フィードの記事には、収集のときに見つけた画像を表示する (フィードの画像、なければ記事ページの og:image)。画像は表示のたびに元のサイトから読み込む。」を足す。「## チャット」に「回答中はエージェントの作業計画、思考、ツールの実行と経過時間を表示し、回答の後は1行にまとめる。」を足す。

- [ ] **Step 5: 画面の確認一覧 (利用者向け)**

報告に次の一覧を書き、各項目を「未確認 (画面)」とする。

1. 収集の後、フィードの記事に画像が出る。画像の無い記事は幅が詰まる。
2. 読み込めない画像は隠れる。
3. チャットで複数のツールを使う質問をすると、作業計画のチェックリストが出て、進むにつれて完了に印が付く。
4. 回答中は「調査中 · N秒 · ツールN回」が1秒ごとに進む。
5. 回答の後は「Nステップ · N秒」の1行になり、押すと思考とツールが開く。ツール行を押すと詳細が開く。
6. 過去のスレッドを開いても、要約行と計画が出る。

- [ ] **Step 6: コミット**

```bash
git add README.md
git commit -m "docs: describe article thumbnails and chat progress"
```

確かめた結果をコミットメッセージの本文に書く。
