# InfoDesk Visual Digest and In-App Reader (Plan 5) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** ダイジェストを画像と図で一目で把握できるようにし (spec 15節)、記事とリンクをアプリ内の閲覧ウィンドウで開く (spec 16節)。

**Architecture:** 期間の概観 (件数、日ごとの推移、トピックの増減) は core の集計 `Store::digest_stats` と Tauri コマンド `digest_stats` で取り、UI は SVG で描く。期間要約の1行目に「見出し: 」を書かせ、UI は見出し、番号付きの箇条書き、根拠記事の画像カードを描く。閲覧ウィンドウは Rust の `reader` モジュールが1つの窓に2つの WebView (アプリのツールバー `reader.html`、記事ページ) を置いて作り、記事ページには IPC を許さない。

**Tech Stack:** Rust (core、Tauri 2.12 `unstable` 機能)、React 19、TypeScript、Vite (2ページ構成)、vitest。

**Spec:** `docs/superpowers/specs/2026-10-06-infodesk-design.md` 15節、16節 (13節の `image_url` と 12節のチャットを前提にする)。前提: Plan 1〜4 が `master` にマージ済み。ブランチは `plan-5-visual-reader`。

## Global Constraints

- 期間の境界は期間要約と同じ: `period_start` と `utc_range(period, start, tz)` (`crates/core/src/period.rs`)、ローカル日付、週は月曜始まり。数えるのは `fetched_at` が期間に入る記事。
- 前の期間は同じ長さの直前の期間 (日: 前日、週: 前週、月: 前月)。
- グラフは SVG で描き、外部ライブラリを足さない。数値は文字でも出す (色だけに頼らない)。増減のアイコンは lucide の `TrendingUp` / `TrendingDown` / `Minus`。
- 見出し: 期間要約の1行目が「見出し: 」で始まる場合だけ見出しとして使う (40字以内を指示する)。無い要約は今の表示のまま。
- 画像カード: 根拠記事 (`item_ids` の順) のうち `image_url` のある上位3件。画像は 16:9、`loading="lazy"`、`referrerpolicy="no-referrer"`、読み込み失敗で隠す。https だけ。
- 閲覧ウィンドウ: 1つを使い回す。ラベルは窓 `reader`、ツールバーの WebView `reader-toolbar`、記事ページの WebView `reader-content`。開けるのは http と https だけ。ページ内の移動も他のスキームは止める。`target="_blank"` は同じ記事ページの WebView で開く。
- 記事ページの WebView には IPC を許さない (どの capability にも `reader-content` と、リモートの URL を含めない)。
- 記事を開くと既読になる (今の `open_item` と同じ)。
- UI の文言は日本語、絵文字なし、色は既存のトークンだけ。
- 依存は `cargo add` / `npm install` で入れる。版を手書きしない。
- コミットメッセージに `Co-Authored-By`、`Claude-Session` などのAI帰属トレーラーを付けない。
- 各タスクのコミット前に、触った範囲で `cargo test --workspace`、`cargo clippy --all-targets -- -D warnings`、`cargo fmt --check`、`cd app && npm test -- --run && npm run build` が通ること。

## Spec からの変更点 (計画で決めたこと)

- ツールバーの WebView は、アプリに同梱した自分のページなので、既定の capability (`default`) に加える。アプリの独自コマンドを WebView ごとに絞るには、すべてのコマンドを権限ファイルで宣言し直す必要があり (tauri-build の app manifest)、この計画の範囲を超えるため。外部のページを表示する `reader-content` には、spec のとおり何の権限も与えない。spec 16節の「ツールバーには閲覧用のコマンドだけを許す」は、この計画では「ツールバーは同梱のページだけを読み込む」に置き換え、仕様書も同じコミットで直す (Task 5)。

## Review Focus

1. 期間に記事が0件、トピックが0件、前の期間が0件 (増減の分母)。空の図か「この期間に記事がありません」が出て、0除算や NaN が出ない。Task 1 と Task 3 でテストする。
2. 23:30 と 0:30 の境界、月曜 0:30 の週、月末。日ごとの件数が正しい日に入る。Task 1 でテストする。
3. LLM が「見出し: 」を書かない、2行目に書く、全角コロンで書く、空にする。見出しなしで今の表示になり、見出しの行が箇条書きに混ざらない。Task 1 (指示文) と Task 4 でテストする。
4. 閲覧ウィンドウが開いている間に別の記事を開く、閉じてから開く、`javascript:` や `file:` のリンク、ページが新しい窓を開こうとする。同じ窓で開き、危険なスキームは開かない。Task 5 でテストする。
5. 根拠記事のどれにも画像が無い、画像の読み込みに失敗する。カードが無ければ今の文字のリンクだけになり、行が崩れない。Task 4 でテストする。

## File Structure

```
crates/core/src/model.rs                 DigestStats、CategoryStat、TopicStat
crates/core/src/store/stats.rs           Store::digest_stats (新規)
crates/core/src/store/mod.rs             mod stats
crates/core/src/summarizer/prompts.rs    見出しの指示
app/src-tauri/src/commands/digests.rs    digest_stats コマンド
app/src-tauri/src/reader.rs              閲覧ウィンドウ (新規)
app/src-tauri/src/commands/items.rs      open_item を閲覧ウィンドウへ、open_in_reader
app/src-tauri/src/lib.rs                 登録、mod reader
app/src-tauri/Cargo.toml                 tauri の unstable 機能
app/src-tauri/capabilities/default.json  webviews に reader-toolbar
app/index.html, app/reader.html          2ページ
app/vite.config.ts                       build.rollupOptions.input に reader.html
app/src/reader/main.tsx                  ツールバーの入口 (新規)
app/src/reader/ReaderToolbar.tsx         ツールバー (新規)
app/src/types.ts, app/src/api.ts         DigestStats、digestStats、openInReader、reader の操作
app/src/lib/digest.ts                    見出しの取り出し
app/src/components/charts/BarChart.tsx   横棒グラフ (新規)
app/src/components/charts/Sparkline.tsx  折れ線 (新規)
app/src/views/digest/Overview.tsx        期間の概観 (新規)
app/src/views/digest/ImageCards.tsx      画像カード (新規)
app/src/views/DigestView.tsx             概観、見出し、番号付き箇条書き、カード
app/src/chat/ChatThread.tsx              回答内のリンクを閲覧ウィンドウで開く
app/src/styles/digest.css                概観とカードの CSS (新規)
README.md
```

---

### Task 1: 期間の集計と見出しの指示 (core)

**Files:**
- Create: `crates/core/src/store/stats.rs`
- Modify: `crates/core/src/model.rs`、`crates/core/src/store/mod.rs`、`crates/core/src/summarizer/prompts.rs`
- Test: `stats.rs`、`prompts.rs` の `#[cfg(test)]`

**Interfaces:**
- Produces (すべて `Serialize`、`Clone`、`Debug`、`PartialEq`):
  - `pub struct CategoryStat { pub category: Category, pub count: i64, pub daily: Vec<i64> }` (`daily` は日の期間では空。週は7要素、月はその月の日数の要素。期間の初日から順。未来の日も 0 で入れる)
  - `pub struct TopicStat { pub topic_id: i64, pub name: String, pub count: i64, pub prev_count: i64 }`
  - `pub struct DigestStats { pub period: Period, pub period_start: NaiveDate, pub total: i64, pub categories: Vec<CategoryStat>, pub topics: Vec<TopicStat> }` (`categories` は `Category::ALL` の順で全7件。`topics` はトピックの id 順。`topic_id` を指定したときはそのトピックだけ)
  - `Store::digest_stats(&self, period: Period, date: NaiveDate, tz: FixedOffset, topic_id: Option<i64>) -> Result<DigestStats>`
  - `prompts::DIGEST_SYSTEM` に見出しの指示を足す。`pub const HEADLINE_PREFIX: &str = "見出し: ";`

- [ ] **Step 1: 失敗するテストを書く**

`crates/core/src/store/stats.rs` (`store/mod.rs` に `mod stats;`)。データの作り方は既存の `testutil.rs` (`store_with_source`、`new_item`、`at`) を読んで合わせる。`upsert_items` の `now` が `fetched_at` になる。

```rust
#[cfg(test)]
mod tests {
    use crate::model::{Category, Period, SourceKind};
    use crate::store::Store;
    use crate::testutil::{at, new_item};
    use chrono::{FixedOffset, NaiveDate};

    fn jst() -> FixedOffset {
        FixedOffset::east_opt(9 * 3600).unwrap()
    }

    /// (カテゴリー, 取得時刻 UTC) の記事を入れる。
    fn store_with(rows: &[(Category, &str)]) -> Store {
        let store = Store::open_in_memory().unwrap();
        let mut sources = std::collections::HashMap::new();
        for (i, (cat, when)) in rows.iter().enumerate() {
            let source_id = *sources.entry(*cat).or_insert_with(|| {
                store.add_source(SourceKind::Rss, cat.as_str(), *cat, &format!("https://e.com/{}", cat.as_str())).unwrap()
            });
            let source = store.get_source(source_id).unwrap().unwrap();
            store.upsert_items(&source, &[new_item(&format!("https://e.com/{i}"), &format!("記事{i}"), "", None)], &at(when)).unwrap();
        }
        store
    }

    #[test]
    fn day_counts_by_category_with_local_boundaries() {
        // 2026-10-10 JST = 2026-10-09T15:00Z .. 2026-10-10T15:00Z
        let store = store_with(&[
            (Category::Medcare, "2026-10-09T15:00:00Z"), // 10/10 00:00 JST: 含む
            (Category::Medcare, "2026-10-10T14:59:59Z"), // 10/10 23:59 JST: 含む
            (Category::Medcare, "2026-10-10T15:00:00Z"), // 10/11 00:00 JST: 含まない
            (Category::Github, "2026-10-09T14:59:59Z"),  // 10/9 23:59 JST: 含まない
        ]);
        let s = store.digest_stats(Period::Day, NaiveDate::from_ymd_opt(2026, 10, 10).unwrap(), jst(), None).unwrap();
        assert_eq!(s.period_start, NaiveDate::from_ymd_opt(2026, 10, 10).unwrap());
        assert_eq!(s.total, 2);
        assert_eq!(s.categories.len(), Category::ALL.len());
        let med = s.categories.iter().find(|c| c.category == Category::Medcare).unwrap();
        assert_eq!(med.count, 2);
        assert!(med.daily.is_empty(), "日の期間では日ごとの件数を出さない");
        assert_eq!(s.categories.iter().find(|c| c.category == Category::Github).unwrap().count, 0);
    }

    #[test]
    fn week_daily_buckets_start_on_monday() {
        // 2026-10-05 は月曜。10/05 00:30 JST と 10/11 23:30 JST (日曜) と前週日曜 23:30 JST。
        let store = store_with(&[
            (Category::TechJp, "2026-10-04T15:30:00Z"),
            (Category::TechJp, "2026-10-11T14:30:00Z"),
            (Category::TechJp, "2026-10-04T14:30:00Z"),
        ]);
        let s = store.digest_stats(Period::Week, NaiveDate::from_ymd_opt(2026, 10, 8).unwrap(), jst(), None).unwrap();
        assert_eq!(s.period_start, NaiveDate::from_ymd_opt(2026, 10, 5).unwrap());
        let jp = s.categories.iter().find(|c| c.category == Category::TechJp).unwrap();
        assert_eq!(jp.count, 2);
        assert_eq!(jp.daily, vec![1, 0, 0, 0, 0, 0, 1]);
    }

    #[test]
    fn month_has_one_bucket_per_day() {
        let store = store_with(&[(Category::Arxiv, "2026-10-31T10:00:00Z")]);
        let s = store.digest_stats(Period::Month, NaiveDate::from_ymd_opt(2026, 10, 15).unwrap(), jst(), None).unwrap();
        let ax = s.categories.iter().find(|c| c.category == Category::Arxiv).unwrap();
        assert_eq!(ax.daily.len(), 31);
        assert_eq!(ax.daily[30], 1);
        assert_eq!(ax.count, 1);
    }

    #[test]
    fn topics_compare_with_the_previous_period() {
        let store = store_with(&[
            (Category::Medcare, "2026-10-10T01:00:00Z"),
            (Category::Medcare, "2026-10-09T01:00:00Z"),
            (Category::Medcare, "2026-10-09T02:00:00Z"),
        ]);
        // 記事のタイトルは「記事N」。全部に合うキーワードのトピックと、どれにも合わないトピック。
        let all = store.add_topic("全部", &["記事".to_string()]).unwrap();
        let none = store.add_topic("なし", &["存在しない語".to_string()]).unwrap();
        let s = store.digest_stats(Period::Day, NaiveDate::from_ymd_opt(2026, 10, 10).unwrap(), jst(), None).unwrap();
        let t = |id: i64| s.topics.iter().find(|t| t.topic_id == id).unwrap().clone();
        assert_eq!((t(all).count, t(all).prev_count), (1, 2));
        assert_eq!((t(none).count, t(none).prev_count), (0, 0));
        assert_eq!(t(all).name, "全部");
        let one = store.digest_stats(Period::Day, NaiveDate::from_ymd_opt(2026, 10, 10).unwrap(), jst(), Some(all)).unwrap();
        assert_eq!(one.topics.len(), 1);
        assert_eq!(one.topics[0].topic_id, all);
    }

    #[test]
    fn empty_period_is_all_zero() {
        let store = Store::open_in_memory().unwrap();
        let s = store.digest_stats(Period::Week, NaiveDate::from_ymd_opt(2026, 10, 8).unwrap(), jst(), None).unwrap();
        assert_eq!(s.total, 0);
        assert!(s.categories.iter().all(|c| c.count == 0 && c.daily.iter().all(|d| *d == 0)));
        assert!(s.topics.is_empty());
    }
}
```

トピックの照合 (キーワード「記事」が「記事1」に合うか) は、既存の `ItemFilter.topic_id` の照合規則 (FTS5 trigram と単語索引) に従う。2文字のキーワードが trigram で合わない場合は、テストのキーワードとタイトルを3文字以上に変える (例: タイトル「新しい記事N」、キーワード「新しい」)。

`prompts.rs` のテストに追加:

```rust
    #[test]
    fn digest_system_asks_for_a_headline_line() {
        assert!(DIGEST_SYSTEM.contains(HEADLINE_PREFIX));
        assert!(DIGEST_SYSTEM.contains("40字"));
    }
```

- [ ] **Step 2: 失敗を確かめる**

Run: `cargo test -p infodesk-core stats:: prompts::` → FAIL

- [ ] **Step 3: 実装する**

`model.rs` に3つの構造体。`stats.rs`:

```rust
use chrono::{Datelike, Duration, FixedOffset, NaiveDate};

use super::Store;
use crate::error::Result;
use crate::model::{Category, CategoryStat, DigestStats, ItemFilter, Period, TopicStat};
use crate::period::{period_start, utc_range};

fn previous_start(period: Period, start: NaiveDate) -> NaiveDate {
    match period {
        Period::Day => start - Duration::days(1),
        Period::Week => start - Duration::days(7),
        Period::Month => {
            let (y, m) = if start.month() == 1 { (start.year() - 1, 12) } else { (start.year(), start.month() - 1) };
            NaiveDate::from_ymd_opt(y, m, 1).expect("first of month")
        }
    }
}

fn days_in_period(period: Period, start: NaiveDate) -> usize {
    match period {
        Period::Day => 0,
        Period::Week => 7,
        Period::Month => {
            let (y, m) = if start.month() == 12 { (start.year() + 1, 1) } else { (start.year(), start.month() + 1) };
            (NaiveDate::from_ymd_opt(y, m, 1).expect("first of next month") - start).num_days() as usize
        }
    }
}

impl Store {
    pub fn digest_stats(&self, period: Period, date: NaiveDate, tz: FixedOffset, topic_id: Option<i64>) -> Result<DigestStats> {
        let start = period_start(period, date);
        let (since, until) = utc_range(period, start, tz);
        let n_days = days_in_period(period, start);
        let mut categories = Vec::new();
        let mut total = 0;
        for category in Category::ALL {
            let mut daily = vec![0i64; n_days];
            let mut stmt = self.conn.prepare(
                "SELECT fetched_at FROM items WHERE category = ?1 AND fetched_at >= ?2 AND fetched_at < ?3",
            )?;
            let rows = stmt.query_map(rusqlite::params![category.as_str(), super::ts(&since), super::ts(&until)], |r| r.get::<_, String>(0))?;
            let mut count = 0;
            for r in rows {
                count += 1;
                if n_days > 0 {
                    let t = chrono::DateTime::parse_from_rfc3339(&r?)
                        .map_err(|e| crate::error::Error::Invalid(e.to_string()))?;
                    let local = t.with_timezone(&tz).date_naive();
                    let idx = (local - start).num_days();
                    if (0..n_days as i64).contains(&idx) {
                        daily[idx as usize] += 1;
                    }
                }
            }
            total += count;
            categories.push(CategoryStat { category, count, daily });
        }
        let prev = previous_start(period, start);
        let (prev_since, prev_until) = utc_range(period, prev, tz);
        let mut topics = Vec::new();
        for t in self.list_topics()?.into_iter().filter(|t| topic_id.is_none_or(|id| id == t.id)) {
            let count = self.count_items(&ItemFilter { topic_id: Some(t.id), since: Some(since), until: Some(until), ..Default::default() })?;
            let prev_count = self.count_items(&ItemFilter { topic_id: Some(t.id), since: Some(prev_since), until: Some(prev_until), ..Default::default() })?;
            topics.push(TopicStat { topic_id: t.id, name: t.name, count, prev_count });
        }
        Ok(DigestStats { period, period_start: start, total, categories, topics })
    }
}
```

`days_in_period` の月は、翌月1日から開始日を引いた日数。`utc_range` の戻り値の型 (`DateTime<Utc>` の組) と `ts` の可視性は `period.rs` と `store/mod.rs` を読んで合わせる。`fetched_at` は `ts()` で秒精度の UTC 文字列として保存されているので、文字列比較で範囲を取れる (既存の `build_where` と同じ前提)。

`prompts.rs`:

```rust
pub const HEADLINE_PREFIX: &str = "見出し: ";

pub const DIGEST_SYSTEM: &str = "あなたは情報収集アシスタントです。\
与えられた記事一覧、または下位期間の要約から、指定された期間の動向を日本語でまとめます。\
1行目に「見出し: 」に続けて、期間の要点を40字以内の1行で書きます。\
続けて重要な話題を3〜6個の箇条書き (各1〜2文) にし、最後に全体の傾向を1文で述べます。\
入力にない事実を加えません。前置きを付けません。";
```

既存の summarizer のテストが `DIGEST_SYSTEM` の文字列に依存していないことを確かめる (依存していれば合わせる)。週と月の要約は日次の要約を入力にするので、入力に下位の見出し行が含まれる。指示はそのままでよい (上位の見出しを新しく書かせる)。

- [ ] **Step 4: 通ることを確かめる**

Run: `cargo test --workspace && cargo clippy --all-targets -- -D warnings && cargo fmt --check` → PASS

- [ ] **Step 5: コミット**

```bash
git add crates
git commit -m "feat(core): digest period stats and a headline line in digests"
```

---

### Task 2: `digest_stats` コマンドと UI の型

**Files:**
- Modify: `app/src-tauri/src/commands/digests.rs`、`app/src-tauri/src/lib.rs`、`app/src/types.ts`、`app/src/api.ts`、`app/src/api.test.ts`

**Interfaces:**
- Consumes: Task 1 の `Store::digest_stats`
- Produces:
  - Tauri コマンド `digest_stats(period: String, date: String, topic_id: Option<i64>) -> AppResult<DigestStats>` (`period` と `date` の解析は既存の `parse_period` / `parse_date` を使う。`tz` は `state.tz`)
  - `types.ts`: `CategoryStat { category: Category; count: number; daily: number[] }`、`TopicStat { topic_id: number; name: string; count: number; prev_count: number }`、`DigestStats { period: Period; period_start: string; total: number; categories: CategoryStat[]; topics: TopicStat[] }`
  - `api.digestStats(period: Period, date: string, topicId: number | null) => invoke<DigestStats>("digest_stats", { period, date, topicId })`

- [ ] **Step 1: 失敗するテストを書く**

`digests.rs` のテスト (既存の `get_digests` のテストの作り方に合わせ、`crate::state::test_state()` を使う。戻り値は `(AppState, Arc<Mutex<Vec<String>>>)`):

```rust
    #[test]
    fn digest_stats_parses_arguments_and_returns_all_categories() {
        let (state, _events) = test_state();
        let s = digest_stats_impl(&state, "week", "2026-10-08", None).unwrap();
        assert_eq!(s.period_start.to_string(), "2026-10-05");
        assert_eq!(s.categories.len(), infodesk_core::model::Category::ALL.len());
        assert!(digest_stats_impl(&state, "year", "2026-10-08", None).is_err());
        assert!(digest_stats_impl(&state, "day", "10/08", None).is_err());
    }
```

`api.test.ts` に、`digestStats("week", "2026-10-08", 3)` が `digest_stats` を `{ period: "week", date: "2026-10-08", topicId: 3 }` で呼ぶテストを足す (既存の書き方に合わせる)。

- [ ] **Step 2: 失敗を確かめる** → FAIL

- [ ] **Step 3: 実装する**

`digest_stats_impl(state, period, date, topic_id)` と `#[tauri::command] pub fn digest_stats(...)`。`lib.rs` の `generate_handler!` に登録。型とラッパーを足す。

- [ ] **Step 4: 通ることを確かめる**

Run: `cargo test --workspace && cargo clippy --all-targets -- -D warnings && cargo fmt --check && cd app && npm test -- --run && npm run build` → PASS

- [ ] **Step 5: コミット**

```bash
git add app/src-tauri/src app/src/types.ts app/src/api.ts app/src/api.test.ts
git commit -m "feat(app): expose digest period stats"
```

---

### Task 3: 期間の概観 (グラフと話題の強さ)

**Files:**
- Create: `app/src/components/charts/BarChart.tsx`、`app/src/components/charts/Sparkline.tsx`、`app/src/views/digest/Overview.tsx`、`app/src/styles/digest.css`、それぞれのテスト
- Modify: `app/src/views/DigestView.tsx`、`app/src/views/DigestView.test.tsx`、`app/src/main.tsx` (`digest.css` を読み込む)

**Interfaces:**
- Consumes: Task 2 の `api.digestStats`、`DigestStats`
- Produces:
  - `BarChart({ rows: { label: string; value: number }[]; max?: number; ariaLabel: string })`: 横棒。各行に名前、棒、数値の文字。`max` は既定で行の最大値 (0 なら棒は幅0)。
  - `Sparkline({ values: number[]; ariaLabel: string })`: 幅 96、高さ 20 の折れ線。値が全部0なら平らな線。
  - `Overview({ stats: DigestStats | null; error: string | null })`: 見出し「期間の概観」、合計件数、カテゴリーの横棒 (週と月では各行に Sparkline)、トピックの増減の一覧。`stats.total === 0` なら「この期間に記事がありません」だけ。
  - `trend(count: number, prev: number): "up" | "down" | "flat"` (`Overview.tsx` から export)

- [ ] **Step 1: 失敗するテストを書く**

`BarChart.test.tsx`:

```tsx
import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { BarChart } from "./BarChart";

describe("BarChart", () => {
  it("draws one bar per row with the value as text", () => {
    render(<BarChart ariaLabel="カテゴリー別" rows={[{ label: "医療介護", value: 14 }, { label: "GitHub", value: 7 }, { label: "arXiv", value: 0 }]} />);
    const fig = screen.getByRole("img", { name: "カテゴリー別" });
    expect(fig).toBeInTheDocument();
    expect(screen.getByText("医療介護")).toBeInTheDocument();
    expect(screen.getByText("14")).toBeInTheDocument();
    const bars = document.querySelectorAll("[data-bar]");
    expect(bars).toHaveLength(3);
    expect(Number(bars[0].getAttribute("data-ratio"))).toBe(1);
    expect(Number(bars[1].getAttribute("data-ratio"))).toBe(0.5);
    expect(Number(bars[2].getAttribute("data-ratio"))).toBe(0);
  });

  it("handles all zeros without NaN", () => {
    render(<BarChart ariaLabel="x" rows={[{ label: "a", value: 0 }]} />);
    expect(document.querySelector("[data-bar]")!.getAttribute("data-ratio")).toBe("0");
  });
});
```

`Sparkline.test.tsx`:

```tsx
import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { Sparkline } from "./Sparkline";

describe("Sparkline", () => {
  it("draws a polyline through every value", () => {
    render(<Sparkline ariaLabel="日ごとの件数" values={[1, 0, 3, 2]} />);
    const svg = screen.getByRole("img", { name: "日ごとの件数" });
    const points = svg.querySelector("polyline")!.getAttribute("points")!.trim().split(/\s+/);
    expect(points).toHaveLength(4);
    expect(points.every((p) => !p.includes("NaN"))).toBe(true);
  });

  it("is flat for all zeros and empty for no values", () => {
    const { container, rerender } = render(<Sparkline ariaLabel="x" values={[0, 0, 0]} />);
    const ys = container.querySelector("polyline")!.getAttribute("points")!.trim().split(/\s+/).map((p) => p.split(",")[1]);
    expect(new Set(ys).size).toBe(1);
    rerender(<Sparkline ariaLabel="x" values={[]} />);
    expect(container.querySelector("polyline")).toBeNull();
  });
});
```

`Overview.test.tsx`:

```tsx
import { render, screen, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import type { DigestStats } from "../../types";
import { Overview, trend } from "./Overview";

const cat = (category: string, count: number, daily: number[] = []) => ({ category, count, daily }) as DigestStats["categories"][number];

const WEEK: DigestStats = {
  period: "week", period_start: "2026-10-05", total: 21,
  categories: [cat("github", 5, [1, 0, 1, 1, 0, 2, 0]), cat("medcare", 14, [2, 2, 2, 2, 2, 2, 2]), cat("arxiv", 2, [0, 0, 0, 0, 0, 2, 0])],
  topics: [
    { topic_id: 1, name: "認知症", count: 6, prev_count: 2 },
    { topic_id: 2, name: "介護", count: 1, prev_count: 4 },
    { topic_id: 3, name: "ローカルLLM", count: 3, prev_count: 3 },
  ],
};

describe("Overview", () => {
  it("shows totals, category bars with sparklines for a week, and topic trends", () => {
    render(<Overview stats={WEEK} error={null} />);
    expect(screen.getByText("21件")).toBeInTheDocument();
    expect(screen.getByRole("img", { name: "カテゴリー別の記事数" })).toBeInTheDocument();
    expect(screen.getAllByRole("img", { name: /日ごとの件数/ })).toHaveLength(3);
    const topics = screen.getByRole("list", { name: "話題の強さ" });
    const row = (name: string) => within(topics).getByText(name).closest("li")!;
    expect(row("認知症")).toHaveAttribute("data-trend", "up");
    expect(row("認知症")).toHaveTextContent("6件");
    expect(row("認知症")).toHaveTextContent("+4");
    expect(row("介護")).toHaveAttribute("data-trend", "down");
    expect(row("介護")).toHaveTextContent("-3");
    expect(row("ローカルLLM")).toHaveAttribute("data-trend", "flat");
  });

  it("omits sparklines for a day and says when the period is empty", () => {
    const { rerender } = render(<Overview stats={{ ...WEEK, period: "day", categories: WEEK.categories.map((c) => ({ ...c, daily: [] })) }} error={null} />);
    expect(screen.queryAllByRole("img", { name: /日ごとの件数/ })).toHaveLength(0);
    rerender(<Overview stats={{ ...WEEK, total: 0, categories: [], topics: [] }} error={null} />);
    expect(screen.getByText("この期間に記事がありません")).toBeInTheDocument();
  });

  it("classifies trends", () => {
    expect(trend(3, 1)).toBe("up");
    expect(trend(1, 3)).toBe("down");
    expect(trend(2, 2)).toBe("flat");
    expect(trend(0, 0)).toBe("flat");
  });
});
```

`DigestView.test.tsx` に追加 (既存の補助に合わせる。`digest_stats` のハンドラーを全テストに足す。既存の assertion は弱めない):

```tsx
  it("shows the period overview above the sections", async () => {
    const ipc = setupIpc({ ...HANDLERS, digest_stats: () => WEEK_STATS });
    renderDigest();
    expect(await screen.findByRole("img", { name: "カテゴリー別の記事数" })).toBeInTheDocument();
    expect(ipc.last("digest_stats")).toEqual({ period: "day", date: expect.any(String), topicId: null });
  });
```

期間を切り替えると `digest_stats` も新しい期間で呼ばれ、`items-changed` で取り直すことも既存のテストの書き方で足す。

- [ ] **Step 2: 失敗を確かめる** → FAIL

- [ ] **Step 3: 実装する**

`BarChart.tsx`:

```tsx
export function BarChart({ rows, max, ariaLabel }: { rows: { label: string; value: number; extra?: React.ReactNode }[]; max?: number; ariaLabel: string }) {
  const top = max ?? Math.max(0, ...rows.map((r) => r.value));
  return (
    <div className="bar-chart" role="img" aria-label={ariaLabel}>
      {rows.map((r) => {
        const ratio = top > 0 ? r.value / top : 0;
        return (
          <div key={r.label} className="bar-row">
            <span className="bar-label">{r.label}</span>
            <span className="bar-track"><span className="bar-fill" data-bar data-ratio={ratio} style={{ width: `${ratio * 100}%` }} /></span>
            <span className="bar-value">{r.value}</span>
            {r.extra}
          </div>
        );
      })}
    </div>
  );
}
```

`role="img"` の中の文字は支援技術から読めなくなるため、`aria-label` に要約 (例: 「カテゴリー別の記事数: 医療介護 14件、GitHub 5件…」) を入れることを検討する。テストは `name` を部分一致 (`/カテゴリー別の記事数/`) に変えてもよい。数値は画面上も文字で出す。

`Sparkline.tsx` (幅 96、高さ 20、余白 2):

```tsx
export function Sparkline({ values, ariaLabel }: { values: number[]; ariaLabel: string }) {
  const W = 96, H = 20, P = 2;
  if (values.length === 0) return <svg width={W} height={H} role="img" aria-label={ariaLabel} />;
  const top = Math.max(0, ...values);
  const step = values.length > 1 ? (W - P * 2) / (values.length - 1) : 0;
  const points = values.map((v, i) => {
    const x = P + i * step;
    const y = top > 0 ? H - P - (v / top) * (H - P * 2) : H - P;
    return `${x.toFixed(1)},${y.toFixed(1)}`;
  }).join(" ");
  return (
    <svg width={W} height={H} role="img" aria-label={ariaLabel} className="sparkline">
      <polyline points={points} fill="none" stroke="currentColor" strokeWidth={1.5} />
    </svg>
  );
}
```

`Overview.tsx`: カテゴリーの表示名は `types.ts` の `CATEGORIES`。週と月では各行の `extra` に `<Sparkline ariaLabel={`${label}の日ごとの件数`} values={c.daily} />`。トピックは `<ul aria-label="話題の強さ">` の `<li data-trend=...>` に名前、`{count}件`、増減の数 (`+4` / `-3` / `±0`) とアイコン (`TrendingUp` / `TrendingDown` / `Minus`、`aria-hidden`)。読み込み中は何も出さず、`error` があればエラー行。

`DigestView.tsx`: 期間とトピックの選択が変わるたび、また `items-changed` で `api.digestStats(period, date, topicId)` を取り、最新の要求だけを使う (既存の要約の読み込みと同じ古い応答の破棄)。見出しの下、節の上に `<Overview>` を置く。

`digest.css` (トークンだけを使う):

```css
.overview { display: grid; grid-template-columns: minmax(0, 1fr) minmax(0, 260px); gap: 16px 24px; padding: 12px 0 20px; border-bottom: 1px solid var(--border); margin-bottom: 8px; }
.overview h3 { font-size: 13px; margin: 0 0 8px; color: var(--text-secondary); font-weight: 600; }
.bar-chart { display: flex; flex-direction: column; gap: 6px; }
.bar-row { display: grid; grid-template-columns: 88px minmax(0, 1fr) 36px auto; align-items: center; gap: 8px; font-size: 12px; }
.bar-label { color: var(--text-secondary); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
.bar-track { height: 8px; background: var(--bg-subtle); border-radius: 4px; overflow: hidden; }
.bar-fill { display: block; height: 100%; background: var(--accent); border-radius: 4px; }
.bar-value { text-align: right; font-variant-numeric: tabular-nums; }
.sparkline { color: var(--text-tertiary); }
.topic-trends { list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; gap: 6px; font-size: 12px; }
.topic-trends li { display: grid; grid-template-columns: minmax(0, 1fr) auto auto; align-items: center; gap: 8px; }
.topic-trends li[data-trend="up"] .delta { color: var(--success); }
.topic-trends li[data-trend="down"] .delta { color: var(--danger); }
.topic-trends .delta { display: inline-flex; align-items: center; gap: 2px; font-variant-numeric: tabular-nums; color: var(--text-tertiary); }
@media (max-width: 1139px) { .overview { grid-template-columns: minmax(0, 1fr); } }
```

`--success` が `theme.css` に無ければ、既存の状態色のトークンを使う。

- [ ] **Step 4: 通ることを確かめる**

Run: `cd app && npm test -- --run && npm run build` → PASS (警告なし)

- [ ] **Step 5: コミット**

```bash
git add app/src
git commit -m "feat(ui): period overview with category bars, daily trends and topic strength"
```

---

### Task 4: 見出し、番号付きの箇条書き、画像カード

**Files:**
- Create: `app/src/views/digest/ImageCards.tsx`、`app/src/views/digest/ImageCards.test.tsx`
- Modify: `app/src/lib/digest.ts`、`app/src/lib/digest.test.ts`、`app/src/views/DigestView.tsx`、`app/src/views/DigestView.test.tsx`、`app/src/styles/digest.css`

**Interfaces:**
- Consumes: Task 1 の見出しの形 (`見出し: ` で始まる1行目)、`Item.image_url`
- Produces:
  - `parseDigestBody(body)` の戻り値に `headline: string | null` を足す (1行目の空白を除いたものが `見出し:` または `見出し：` で始まるとき、その後ろを trim した文字列。空なら null。見出しの行は箇条書きにも段落にも入れない)
  - `ImageCards({ items: Item[]; onOpen(id: number): void })`: 画像のある記事を最大3件、カードで出す (`image_url` が https のものだけ)

- [ ] **Step 1: 失敗するテストを書く**

`digest.test.ts` に追加:

```ts
  it("takes a headline from the first line", () => {
    const r = parseDigestBody("見出し: 認知症の新薬が国内で適用拡大へ\n- 新薬の話題\n- 介護報酬\n全体の傾向。");
    expect(r.headline).toBe("認知症の新薬が国内で適用拡大へ");
    expect(r.bullets).toEqual(["新薬の話題", "介護報酬"]);
    expect(r.paragraphs).toEqual(["全体の傾向。"]);
  });

  it("accepts a full-width colon and surrounding spaces, ignores a headline that is not first", () => {
    expect(parseDigestBody("  見出し：要点  \n- a").headline).toBe("要点");
    expect(parseDigestBody("- a\n見出し: 後ろ").headline).toBeNull();
    expect(parseDigestBody("見出し: \n- a").headline).toBeNull();
    expect(parseDigestBody("- a\n- b").headline).toBeNull();
  });
```

`ImageCards.test.tsx`:

```tsx
import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { Item } from "../../types";
import { ImageCards } from "./ImageCards";

const item = (id: number, image_url: string | null): Item => ({
  id, source_id: 1, source_name: `ソース${id}`, category: "medcare", url: `https://e.com/${id}`, title: `記事${id}`,
  excerpt: "", author: null, published_at: null, fetched_at: "2026-10-10T00:00:00Z", score: null, ai_summary: null,
  read: false, starred: false, image_url,
});

describe("ImageCards", () => {
  it("shows up to three https-image articles as cards in order and opens on click", () => {
    const onOpen = vi.fn();
    render(<ImageCards items={[item(1, null), item(2, "https://i/2.jpg"), item(3, "http://i/3.jpg"), item(4, "https://i/4.jpg"), item(5, "https://i/5.jpg"), item(6, "https://i/6.jpg")]} onOpen={onOpen} />);
    const cards = screen.getAllByRole("button");
    expect(cards.map((c) => c.textContent)).toEqual([expect.stringContaining("記事2"), expect.stringContaining("記事4"), expect.stringContaining("記事5")]);
    const img = cards[0].querySelector("img")!;
    expect(img).toHaveAttribute("src", "https://i/2.jpg");
    expect(img).toHaveAttribute("loading", "lazy");
    expect(img).toHaveAttribute("referrerpolicy", "no-referrer");
    fireEvent.click(cards[1]);
    expect(onOpen).toHaveBeenCalledWith(4);
  });

  it("hides a card whose image fails and renders nothing without images", () => {
    const { container, rerender } = render(<ImageCards items={[item(2, "https://i/2.jpg")]} onOpen={() => {}} />);
    fireEvent.error(container.querySelector("img")!);
    expect(container.querySelector(".image-card")).toBeNull();
    rerender(<ImageCards items={[item(1, null)]} onOpen={() => {}} />);
    expect(container).toBeEmptyDOMElement();
  });
});
```

`DigestView.test.tsx` に追加: 見出しのある要約で、見出しが節の見出しの下に大きく出る (`.digest-headline`)、箇条書きが `<ol>` になる、根拠記事のうち画像のある記事がカードで出て、カードにした記事は文字のリンクから除かれる、見出しの無い既存の要約は今の表示 (`<ul>` かは問わないが見出しの要素が無い)。

- [ ] **Step 2: 失敗を確かめる** → FAIL

- [ ] **Step 3: 実装する**

`digest.ts` の `parseDigestBody` で、最初の空でない行が `/^見出し\s*[:：]\s*(.*)$/` に合えば `headline` にして、残りの行で今の処理をする。

`ImageCards.tsx`: 画像の読み込み失敗を記事 id ごとに覚えて隠す。https だけ。カードは `<button className="image-card">` の中に画像 (16:9、`object-fit: cover`)、タイトル (2行で切る)、ソース名。

`DigestView.tsx`:
- 見出し: `display.digest` の本文から `headline` があれば、節の見出しの下に `<p className="digest-headline">` で出す。
- 箇条書きを `<ol className="digest-points">` にする (番号は CSS のカウンターで丸い数字の印にする)。
- `References` を作り直す: 根拠記事の先頭 12件を `getItem` で取り、画像のある上位3件を `ImageCards` に、残りを今のリンクの並び (最大5件 + 「他 N件」) に出す。カードにした記事はリンクから除く。開く操作は今と同じ `onOpen(id)`。

`digest.css` に追加:

```css
.digest-headline { font-size: 16px; font-weight: 600; line-height: 1.5; margin: 4px 0 10px; color: var(--text-primary); }
.digest-points { list-style: none; counter-reset: point; padding: 0; margin: 0 0 8px; display: flex; flex-direction: column; gap: 8px; }
.digest-points li { counter-increment: point; display: grid; grid-template-columns: 22px minmax(0, 1fr); gap: 8px; line-height: 1.6; }
.digest-points li::before { content: counter(point); width: 20px; height: 20px; border-radius: 50%; background: var(--selection); color: var(--accent); font-size: 11px; font-weight: 600; display: grid; place-items: center; margin-top: 2px; }
.image-cards { display: grid; grid-template-columns: repeat(3, minmax(0, 1fr)); gap: 10px; margin: 8px 0; }
.image-card { display: flex; flex-direction: column; gap: 6px; text-align: left; background: var(--bg-window); border: 1px solid var(--border); border-radius: 7px; overflow: hidden; padding: 0 0 8px; cursor: pointer; }
.image-card img { width: 100%; aspect-ratio: 16 / 9; object-fit: cover; background: var(--bg-subtle); }
.image-card .card-title { padding: 0 8px; font-size: 12px; font-weight: 600; line-height: 1.4; display: -webkit-box; -webkit-line-clamp: 2; -webkit-box-orient: vertical; overflow: hidden; }
.image-card .card-source { padding: 0 8px; font-size: 11px; color: var(--text-tertiary); }
@media (max-width: 1139px) { .image-cards { grid-template-columns: repeat(2, minmax(0, 1fr)); } }
```

- [ ] **Step 4: 通ることを確かめる**

Run: `cd app && npm test -- --run && npm run build` → PASS

- [ ] **Step 5: 画面で確かめる**

スクリーンショットの仕組み (`/private/tmp/claude-502/-Users-ktakeuchi-Documents-InfoDesk/efc0adfa-185e-4966-8a22-cfb8e7e7f6dc/scratchpad/reshoot.sh`、モック `mock.js`) がある場合、`mock.js` に `digest_stats` と、見出し付きの要約を足して (画像の URL は https で、読めずに隠れる前提)、`digest-light|1200x800|?x=1`、`digest-dark|1200x800|?dark=1`、`digest-small|960x640|?x=1` を撮り、概観と見出しと番号付き箇条書きの配置を見る。撮影後に `mock.js` を元に戻す。仕組みが無ければ飛ばして報告に書く。

- [ ] **Step 6: コミット**

```bash
git add app/src
git commit -m "feat(ui): digest headlines, numbered points and image cards"
```

---

### Task 5: 閲覧ウィンドウ (Rust と権限)

**Files:**
- Create: `app/src-tauri/src/reader.rs`
- Modify: `app/src-tauri/Cargo.toml` (`tauri` に `unstable` 機能)、`app/src-tauri/src/lib.rs`、`app/src-tauri/src/commands/items.rs`、`app/src-tauri/capabilities/default.json`、`docs/superpowers/specs/2026-10-06-infodesk-design.md` (16節の権限の文)

**Interfaces:**
- Consumes: 既存の `ensure_http_url`、`open_item_url`
- Produces:
  - 定数: `READER_WINDOW = "reader"`、`READER_TOOLBAR = "reader-toolbar"`、`READER_CONTENT = "reader-content"`、`TOOLBAR_HEIGHT: f64 = 44.0`、イベント `EVENT_READER = "reader-state"` (内容 `{ url: string, title: string }`)
  - 純粋関数: `pub fn reader_layout(width: f64, height: f64) -> ((f64, f64, f64, f64), (f64, f64, f64, f64))` (ツールバーと記事ページの x, y, w, h。高さが足りなくても負にしない)、`pub fn allow_navigation(url: &Url) -> bool` (http/https だけ)
  - `pub fn open_in_reader(app: &AppHandle, url: &str) -> AppResult<()>`: URL を検査し、窓が無ければ作り、あれば記事ページを `navigate` して前面に出す
  - Tauri コマンド: `open_in_reader(url)`、`reader_back()`、`reader_forward()`、`reader_reload()`、`reader_open_external()` (表示中の URL を既定のブラウザで開く)、`reader_close()`
  - `open_item(id)` は既読にしたうえで閲覧ウィンドウで開く (今の既定のブラウザの代わり)。`open_link(url)` は残す (既定のブラウザで開く。ツールバーの「既定のブラウザで開く」が使う)
  - `default.json`: `"windows": ["main"]` はそのまま、`"webviews": ["reader-toolbar"]` を足す (WebView ラベルで capability を当てる。`reader-content` は含めない)

- [ ] **Step 1: 失敗するテストを書く**

`reader.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_splits_toolbar_and_content() {
        let (bar, content) = reader_layout(1000.0, 800.0);
        assert_eq!(bar, (0.0, 0.0, 1000.0, TOOLBAR_HEIGHT));
        assert_eq!(content, (0.0, TOOLBAR_HEIGHT, 1000.0, 800.0 - TOOLBAR_HEIGHT));
        let (_, tiny) = reader_layout(300.0, 20.0);
        assert!(tiny.3 >= 0.0, "高さが足りなくても負にしない");
    }

    #[test]
    fn navigation_allows_only_http_and_https() {
        for ok in ["https://e.com/a", "http://e.com/", "HTTPS://E.COM/"] {
            assert!(allow_navigation(&Url::parse(ok).unwrap()), "{ok}");
        }
        for bad in ["javascript:alert(1)", "file:///etc/passwd", "data:text/html,x", "tauri://localhost", "mailto:a@b.c"] {
            assert!(!allow_navigation(&Url::parse(bad).unwrap()), "{bad}");
        }
    }

    #[test]
    fn capability_never_grants_the_content_webview() {
        let cap: serde_json::Value = serde_json::from_str(include_str!("../capabilities/default.json")).unwrap();
        let text = cap.to_string();
        assert!(!text.contains(READER_CONTENT), "記事ページに権限を与えない");
        assert!(cap.get("remote").is_none(), "リモートの URL に IPC を許さない");
        assert!(cap["webviews"].as_array().unwrap().iter().any(|w| w == READER_TOOLBAR));
    }
}
```

`Url` は `tauri::Url` (url クレートの再公開) を使う。

- [ ] **Step 2: 失敗を確かめる** → FAIL

- [ ] **Step 3: 実装する**

`cargo add -p infodesk-app tauri --features unstable` (既存の `tray-icon` は残る)。

`reader.rs` の窓の作成 (API の名前は `~/.cargo/registry/src/*/tauri-2.12*/src/{window,webview}/mod.rs` で確かめる。`WindowBuilder::new(app, label)`、`Window::add_child(WebviewBuilder, Position, Size)`、`WebviewBuilder::new(label, WebviewUrl::App(..) | WebviewUrl::External(url))`、`on_navigation`、`on_new_window` (`NewWindowResponse` の値)、`on_page_load`、`on_document_title_changed`、`Webview::navigate`、`Webview::reload`、`Webview::eval`、`Webview::url`):

```rust
pub fn open_in_reader(app: &AppHandle, url: &str) -> AppResult<()> {
    crate::commands::items::ensure_http_url(url)?;
    let target = Url::parse(url).map_err(|e| e.to_string())?;
    if let Some(content) = app.get_webview(READER_CONTENT) {
        content.navigate(target)?;
        if let Some(w) = app.get_window(READER_WINDOW) {
            let _ = w.show();
            let _ = w.set_focus();
        }
        return Ok(());
    }
    let window = tauri::window::WindowBuilder::new(app, READER_WINDOW)
        .title("閲覧")
        .inner_size(1100.0, 820.0)
        .min_inner_size(640.0, 400.0)
        .build()?;
    let size = window.inner_size()?.to_logical::<f64>(window.scale_factor()?);
    let (bar, content) = reader_layout(size.width, size.height);
    let handle = app.clone();
    let toolbar = WebviewBuilder::new(READER_TOOLBAR, WebviewUrl::App("reader.html".into()));
    window.add_child(toolbar, LogicalPosition::new(bar.0, bar.1), LogicalSize::new(bar.2, bar.3))?;
    let page = WebviewBuilder::new(READER_CONTENT, WebviewUrl::External(target))
        .on_navigation(allow_navigation)
        .on_new_window(move |url, _features| {
            // target="_blank" は同じ記事ページで開く
            if allow_navigation(&url) && let Some(c) = handle.get_webview(READER_CONTENT) {
                let _ = c.navigate(url);
            }
            NewWindowResponse::Deny
        })
        .on_page_load(|webview, _payload| emit_state(&webview))
        .on_document_title_changed(|webview, _title| emit_state(&webview));
    window.add_child(page, LogicalPosition::new(content.0, content.1), LogicalSize::new(content.2, content.3))?;
    // 窓の大きさが変わったら2つの WebView を並べ直す
    let w2 = window.clone();
    window.on_window_event(move |e| {
        if let tauri::WindowEvent::Resized(_) = e { relayout(&w2); }
    });
    Ok(())
}
```

`emit_state` は記事ページの `url()` と、タイトル (`on_document_title_changed` の値を `Mutex<String>` に覚える、または URL を代わりに使う) を `EVENT_READER` でツールバーへ送る (`app.emit_to(READER_TOOLBAR, ...)` の名前は確かめる)。ツールバーが読み込まれた直後にも最新の状態を送れるよう、`reader_state()` コマンドで最新の状態を返す。

コマンド: `reader_back` は `content.eval("history.back()")`、`reader_forward` は `history.forward()`、`reader_reload` は `content.reload()`、`reader_open_external` は `content.url()` を `ensure_http_url` してから既存の opener で開く、`reader_close` は窓を閉じる。

`open_item`: `open_item_url` で既読にして URL を得たあと、`open_in_reader(&app, &url)`。

`default.json` に `"webviews": ["reader-toolbar"]`。spec 16節の「ツールバーには閲覧用のコマンドだけを許す」を「ツールバーはアプリに同梱したページだけを読み込み、記事ページの WebView には権限を与えない」に直す (この計画の「Spec からの変更点」)。

- [ ] **Step 4: 通ることを確かめる**

Run: `cargo test --workspace && cargo clippy --all-targets -- -D warnings && cargo fmt --check` → PASS

- [ ] **Step 5: コミット**

```bash
git add app/src-tauri docs/superpowers/specs/2026-10-06-infodesk-design.md Cargo.lock
git commit -m "feat(app): in-app reader window with an isolated article webview"
```

---

### Task 6: 閲覧ウィンドウのツールバーと、開く操作の切り替え

**Files:**
- Create: `app/reader.html`、`app/src/reader/main.tsx`、`app/src/reader/ReaderToolbar.tsx`、`app/src/reader/ReaderToolbar.test.tsx`
- Modify: `app/vite.config.ts`、`app/src/api.ts`、`app/src/chat/ChatThread.tsx`、`app/src/chat/ChatThread.test.tsx`、`app/src/styles/` (ツールバーの CSS)

**Interfaces:**
- Consumes: Task 5 のコマンドとイベント
- Produces:
  - `api.openInReader(url)`、`api.readerBack()`、`api.readerForward()`、`api.readerReload()`、`api.readerOpenExternal()`、`api.readerClose()`、`api.readerState()`、`EVENTS.reader = "reader-state"`
  - `ReaderToolbar`: 戻る、進む、再読み込み、タイトルと URL (ホストを強調)、「ブラウザで開く」、閉じる。ボタンは lucide のアイコンと `aria-label`
  - チャットの回答内のリンク (`AnswerLink`) は `api.openInReader(href)` に変える。参照記事のボタン、フィード、ダイジェストは `api.openItem(id)` のまま (Rust 側で閲覧ウィンドウに変わる)

- [ ] **Step 1: 失敗するテストを書く**

`ReaderToolbar.test.tsx`:

```tsx
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import { emitEvent, setupIpc } from "../test/ipc";
import { ReaderToolbar } from "./ReaderToolbar";

describe("ReaderToolbar", () => {
  it("shows the current page and calls the reader commands", async () => {
    const ipc = setupIpc({
      reader_state: () => ({ url: "https://news.example.com/a/b", title: "認知症の新薬" }),
      reader_back: () => null, reader_forward: () => null, reader_reload: () => null,
      reader_open_external: () => null, reader_close: () => null,
    });
    render(<ReaderToolbar />);
    expect(await screen.findByText("認知症の新薬")).toBeInTheDocument();
    expect(screen.getByText("news.example.com")).toBeInTheDocument();
    const user = userEvent.setup();
    for (const [name, cmd] of [["戻る", "reader_back"], ["進む", "reader_forward"], ["再読み込み", "reader_reload"], ["ブラウザで開く", "reader_open_external"], ["閉じる", "reader_close"]] as const) {
      await user.click(screen.getByRole("button", { name }));
      expect(ipc.count(cmd)).toBe(1);
    }
  });

  it("follows page changes", async () => {
    let state = { url: "https://a.example.com/", title: "A" };
    setupIpc({ reader_state: () => state });
    render(<ReaderToolbar />);
    await screen.findByText("A");
    state = { url: "https://b.example.com/x", title: "B" };
    await emitEvent("reader-state");
    expect(await screen.findByText("B")).toBeInTheDocument();
  });
});
```

`emitEvent` がイベントの中身を渡せない場合は、ツールバーがイベントを受けたら `reader_state` を取り直す作りにする (上のテストはその前提)。

`ChatThread.test.tsx` の「opens http(s) links…」のテストを、`open_in_reader` が `{ url }` で呼ばれる形に変える (既定のブラウザの `open_link` は呼ばれない)。

- [ ] **Step 2: 失敗を確かめる** → FAIL

- [ ] **Step 3: 実装する**

`app/reader.html` (`index.html` と同じ形で、`<script type="module" src="/src/reader/main.tsx">`)。`vite.config.ts` に:

```ts
  build: {
    rollupOptions: {
      input: { main: "index.html", reader: "reader.html" },
    },
  },
```

`__dirname` を使う書き方が必要な場合は `fileURLToPath(new URL("./reader.html", import.meta.url))` で絶対パスにする (`vite.config.ts` は Node の型を持たないので既存の `// @ts-expect-error` の書き方に合わせる)。

`reader/main.tsx` は `theme.css` と `reader.css` を読み込み、`<ReaderToolbar />` を描く。ツールバーは高さ 44px の1行: 左にボタン3つ、中央にタイトル (1行で切る) とホスト名、右に「ブラウザで開く」「閉じる」。

- [ ] **Step 4: 通ることを確かめる**

Run: `cd app && npm test -- --run && npm run build` → PASS。`ls app/dist/reader.html` で2ページ目が出力されることを確かめる。

- [ ] **Step 5: コミット**

```bash
git add app/reader.html app/vite.config.ts app/src
git commit -m "feat(ui): reader toolbar and open links in the reader"
```

---

### Task 7: 確認と README

**Files:**
- Modify: `README.md`

- [ ] **Step 1: 全体を確かめる**

```bash
cargo test --workspace && cargo clippy --all-targets -- -D warnings && cargo fmt --check
cd app && npm test -- --run && npm run build && ls dist/reader.html && cd ..
cd agent && npm test -- --run && cd ..
cd app && npm run bundle && cd ..
ls target/debug/bundle/macos/InfoDesk.app/Contents/Resources/agent/agent.mjs
```

- [ ] **Step 2: README を更新する**

「## アプリ」に「ダイジェストの上部に期間の概観 (カテゴリー別の記事数、週と月は日ごとの推移、トピックの増減) を出す。各カテゴリーの要約は見出しと番号付きの要点、画像のある根拠記事をカードで出す。」と「記事とリンクはアプリ内の閲覧ウィンドウで開く。ツールバーの『ブラウザで開く』で既定のブラウザに渡せる。」を足す。

- [ ] **Step 3: 画面の確認一覧 (利用者向け)**

報告に次の一覧を書き、各項目を「未確認 (画面)」とする。

1. ダイジェストの上部に、カテゴリー別の棒グラフと数値、週と月では日ごとの折れ線、トピックの増減の矢印が出る。
2. 新しく生成した要約に見出しが出て、要点が番号付きで並ぶ。既存の要約は今の表示のまま。
3. 画像のある根拠記事がカードで出て、押すと閲覧ウィンドウで開く。
4. フィード、ダイジェスト、チャットの記事とリンクが閲覧ウィンドウで開く。2件目を開くと同じ窓が切り替わる。
5. 閲覧ウィンドウの戻る、進む、再読み込み、ブラウザで開く、閉じるが動く。窓の大きさを変えるとツールバーと記事が並び直す。
6. 記事ページで新しい窓を開くリンクが同じ窓で開く。

- [ ] **Step 4: コミット**

```bash
git add README.md
git commit -m "docs: describe the visual digest and the in-app reader"
```

確かめた結果をコミットメッセージの本文に書く。
