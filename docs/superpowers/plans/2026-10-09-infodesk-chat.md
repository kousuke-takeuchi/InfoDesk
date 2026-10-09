# InfoDesk Chat (Plan 3) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 収集した記事と要約についてエージェントに質問し、要約や調査をさせる右パネルのチャット (spec 12節、デザイン A-4、A-12) を作る。

**Architecture:** エージェントは Node のサイドカー (`agent/`、esbuild で1ファイルにまとめる) で動く。Tauri アプリがサイドカーを起動し、127.0.0.1 の空きポートとトークンを UI に渡す。UI (assistant-ui + AI SDK の `useChat`) はサイドカーへ HTTP で質問を送り、AI SDK の UI message stream を受けて表示する。サイドカーのツールは `infodesk` CLI を JSON 出力で呼ぶ。Ollama は AI SDK の `ToolLoopAgent`、Claude は `ai-sdk-provider-claude-code` と MCP 橋渡しで動かす。スレッドとメッセージは Rust 側の SQLite に保存し、UI が Tauri コマンドで読み書きする。

**Tech Stack:** Rust (core、CLI、Tauri 2)、Node 22 以上 (利用者の環境は 26)、TypeScript、`ai` 7、`ai-sdk-ollama` 4、`ai-sdk-provider-claude-code` 4、`zod` 4、`@assistant-ui/react` 0.15、`@assistant-ui/react-ai-sdk` 1.4、`@assistant-ui/react-markdown` 0.14、`@ai-sdk/react` 4、esbuild、vitest。

**Spec:** `docs/superpowers/specs/2026-10-06-infodesk-design.md` 12節 (チャット)、7節 (チャットパネル)、8節、9節。デザインは `design/exports/4-feed-chat-light.png`、`4-feed-chat-dark.png`、`12-chat-states.png`。前提: Plan 1、2a、2b が `master` にマージ済み。

## Global Constraints

- 対象OSはmacOSのみ。Node は利用者の環境のもの (設定 `node.bin`、既定 `node`) を使い、アプリに同梱しない。Node 22 未満は起動しない (`ai` と2つのプロバイダーの `engines.node` が `>=22`)。
- npm の依存は次の版に固定する (2026-10-09 に npm で確認し、型定義を読んで API を確かめた)。`npm install --save-exact <名前>@<版>` で入れる。
  - `ai` 7.0.136、`@ai-sdk/react` 4.0.139、`ai-sdk-ollama` 4.4.0、`ai-sdk-provider-claude-code` 4.3.3、`zod` 4.6.5
  - `@assistant-ui/react` 0.15.25、`@assistant-ui/react-ai-sdk` 1.4.16、`@assistant-ui/react-markdown` 0.14.19
  - `esbuild` 0.28.2 (agent の開発依存)
- Rust の依存は `cargo add` で入れる。版を手書きしない。
- 確認済みの API 名 (これ以外の名前を推測で使わない):
  - `ai`: `ToolLoopAgent`、`tool`、`isStepCount` (旧名 `stepCountIs` も残っている)、`streamText`、`pipeAgentUIStreamToResponse`、`convertToModelMessages`、`createUIMessageStream`、`createUIMessageStreamResponse`、`simulateReadableStream`、`DefaultChatTransport`。UI message stream のオプション `onError: (error: unknown) => string`。エラーの chunk は `{ type: "error", errorText }`。
  - `ai/test`: `MockLanguageModelV4`。stream の部品は `text-start` / `text-delta` / `text-end` (`id`、`delta`)、`tool-call` (`toolCallId`、`toolName`、`input` は JSON 文字列)、`finish` (`finishReason: { unified: "stop" | "tool-calls", raw: undefined }`、`usage`)。
  - `ai-sdk-ollama`: `createOllama({ baseURL })` が `(modelId) => LanguageModelV4` を返す。
  - `ai-sdk-provider-claude-code`: `createClaudeCode`、`claudeCode(modelId, settings)`、`createAiSdkMcpServer(name, tools)`。設定は `pathToClaudeCodeExecutable`、`systemPrompt`、`mcpServers`、`allowedTools`、`tools`、`persistSession`、`maxTurns`。MCP 経由のツールは `mcp__<server>__<tool>` の名前で、動的なツール部品として流れる。`settingSources` を省くとファイルの設定を読まない (この挙動を使う)。
  - `@assistant-ui/react`: `AssistantRuntimeProvider`、`ThreadPrimitive`、`MessagePrimitive` (`Parts` の `components` に `Text` と `tools: { Fallback }`)、`ComposerPrimitive`。ツール部品の props は `toolName`、`args`、`result`、`isError`、`status.type`。
  - `@assistant-ui/react-ai-sdk`: `useAISDKRuntime(chatHelpers, adapter?)`。`@assistant-ui/react-markdown`: `MarkdownTextPrimitive`。`@ai-sdk/react`: `useChat({ id, messages, transport, onFinish, onError })`。
- Tauri: macOS の WebView のオリジンは `tauri://localhost` (tauri 2.12.1 の `ipc/protocol.rs` で確認)。開発時は `http://localhost:1420`。`bundle.resources` と `bundle.externalBin` は tauri-build がコンパイル時に存在を確かめる (tauri-build 2.7.1 で確認) ので、基本の `tauri.conf.json` には書かない。バンドル時だけ `tauri build --config src-tauri/tauri.bundle.json` で `agent.mjs` を resources に足す (`--config` は tauri-cli 2.12.1 の `build --help` で確認)。これで `cargo test` は agent のビルドに依存しない。
- サイドカーのツールが呼ぶ CLI はアプリ本体に組み込む。アプリの実行ファイルを `--cli <infodesk の引数>` で起動すると、`infodesk` CLI と同じ処理をして終了する。単体の `infodesk` コマンドも今までどおり残る。
- サイドカーは 127.0.0.1 だけで待ち受ける。全ての要求に `Authorization: Bearer <token>` を必要とし、トークンはアプリが起動ごとに作る。CORS は `tauri://localhost` と `http://localhost:1420` だけに許す。
- CSP の `connect-src` に `http://127.0.0.1:*` を足す。それ以外の CSP は変えない。
- プロバイダーの自動切替をしない (spec 9節)。LLM エラー時の「Claude で送る」「Ollama で送る」は利用者が押した場合だけ動く。
- Web検索: Claude は組み込みの `WebSearch`。Ollama は設定 `ollama.web_search_api_key` がある場合だけツールを有効にし、キーは `https://ollama.com/api/web_search` への要求にだけ付ける。ローカルの Ollama にはキーを送らない。
- 記事本文の取得: `http` と `https` だけ。受信は最大 2,000,000 バイト、返す本文は既定 20,000 文字。DB に保存しない。
- メッセージは AI SDK の `UIMessage` の JSON のまま保存する。
- UI の文言は日本語。絵文字を使わない。アイコンは `lucide-react`。
- コミットメッセージに `Co-Authored-By`、`Claude-Session` などのAI帰属トレーラーを付けない。
- 各タスクのコミット前に、触った範囲のテストとビルドが通ること: `cargo test --workspace`、`cargo clippy --all-targets -- -D warnings`、`cargo fmt --check`、`cd app && npm test -- --run && npm run build`、`cd agent && npm test -- --run && npm run build`。

## Spec からの変更点 (計画で決めたこと)

- ライブラリの版を spec の表 (2026-10-06) から上げた。`ai` 7.0.128→7.0.136、`@ai-sdk/react` 4.0.131→4.0.139、`@assistant-ui/react` 0.15.23→0.15.25、`@assistant-ui/react-ai-sdk` 1.4.14→1.4.16。Markdown 表示のため `@assistant-ui/react-markdown` 0.14.19 を足す。
- CLI はアプリの実行ファイルに組み込み、`<アプリ> --cli ...` で呼ぶ (spec 12節の「`infodesk` CLI」と同じコード)。単体の CLI を外部バイナリとして同梱すると、ビルドとテストの順序が複雑になるため。
- CLI のコマンド名: 期間要約の参照は `infodesk digests <period> --json` とする。既存の `infodesk digest <period>` が生成用で、`digest show` は位置引数と衝突するため。記事1件は `infodesk item <id> --json`、設定の一覧は `infodesk config list --json`、本文取得は `infodesk page <url> --json`。
- Ollama の Web検索は `ai-sdk-ollama` の `webSearch` ツールを使わず、自前のツールで `https://ollama.com/api/web_search` を呼ぶ。キーの送り先を1か所に固定し、テストで確かめるため。
- 回答が参照した記事のリンクは、その回答のツール結果 (記事検索、記事取得) に含まれる記事から作る。モデルの本文中の URL には頼らない。

## Review Focus

1. サイドカーが起動できない (Node がない、Node 21、`agent.mjs` がない、CLI がない、起動直後に終了)。パネルは理由と「Node のパスを設定」「再試行」を出し、ダイジェストとフィードは普段どおり動く。Task 6 と Task 7 でテストする。
2. LLM が応答しない (Ollama 未起動、モデル名の誤り、Claude の認証切れ)。回答の代わりにエラー箱が出て、質問の吹き出しは残り、「再試行」で同じ質問を送り直せる。Task 5 (サイドカーがエラーを `errorText` で返す) と Task 8 でテストする。
3. ツールが失敗する (CLI が非0で終わる、本文取得が 404 や PDF)。サイドカーは落ちず、失敗がツール結果としてモデルに返り、ツール行が「失敗」になる。Task 4 と Task 8 でテストする。
4. 長い本文や巨大なページ。本文取得は 2,000,000 バイトで読むのをやめ、20,000 文字で切り、`truncated: true` を返す。Task 2 でテストする。
5. 古いスレッドを開く、スレッドを消す、送信中にパネルを閉じる。保存済みのメッセージがそのまま表示され、消したスレッドは一覧から消え、閉じても保存は最後の完了分で止まる。Task 3 と Task 8 でテストする。

## File Structure

```
crates/core/src/store/chat.rs          スレッドとメッセージの保存 (新規)
crates/core/src/store/migrate.rs       版3: chat_threads、chat_messages
crates/core/src/store/settings.rs      list_settings
crates/core/src/page.rs                URL の本文テキスト取得 (新規)
crates/core/src/lib.rs                 pub mod page
crates/core/src/model.rs               ChatThread
crates/cli/src/main.rs                 --json、item、digests、config list、page
crates/cli/src/json.rs                 JSON 出力の組み立て (新規、単体テスト)
agent/package.json                     サイドカー (Node、ESM)
agent/tsconfig.json
agent/vitest.config.ts
agent/build.mjs                        esbuild で dist/agent.mjs にまとめる
agent/src/env.ts                       起動時の環境変数
agent/src/cli.ts                       infodesk CLI の呼び出し
agent/src/tools.ts                     ツール定義 (zod)
agent/src/prompt.ts                    指示文と文脈の組み立て
agent/src/models.ts                    Ollama / Claude のモデル作成
agent/src/chat.ts                      1回の質問を流す (Ollama: ToolLoopAgent、Claude: streamText + MCP)
agent/src/server.ts                    HTTP (認証、CORS、/health、/chat)
agent/src/main.ts                      起動と準備完了行の出力
agent/test/*.test.ts                   テスト
agent/test/fake-infodesk.mjs           テスト用の偽 CLI
crates/cli/src/lib.rs                  CLI の本体 (main.rs から移す。アプリからも呼ぶ)
app/package.json                       chat 依存、bundle スクリプト
app/src-tauri/tauri.conf.json          CSP
app/src-tauri/tauri.bundle.json        バンドル時だけ足す resources (agent.mjs) (新規)
app/src-tauri/src/main.rs              --cli で CLI として動く
app/src-tauri/src/sidecar.rs           サイドカーの起動、停止、状態 (新規)
app/src-tauri/src/commands/chat.rs     スレッド、メッセージ、接続先のコマンド (新規)
app/src-tauri/src/lib.rs               登録、終了時の停止
app/src-tauri/src/state.rs             EVENT_CHAT
app/src/types.ts                       ChatThread、ChatEndpoint、ChatContext
app/src/api.ts                         chat コマンド
app/src/chat/context.ts                文脈の型と表示名 (新規)
app/src/chat/toolLabels.ts             ツール名の表示 (新規)
app/src/chat/references.ts             回答が参照した記事の抽出 (新規)
app/src/chat/useChatEndpoint.ts        接続先の取得と再起動 (新規)
app/src/chat/ChatPanel.tsx             パネル枠: ヘッダー、文脈、状態 (新規)
app/src/chat/ThreadMenu.tsx            スレッド一覧、切替、削除 (新規)
app/src/chat/ChatThread.tsx            会話: assistant-ui、保存、エラー (新規)
app/src/chat/ToolCall.tsx              ツール行 (新規)
app/src/chat/*.test.ts(x)              テスト
app/src/App.tsx                        パネルの開閉、文脈、「チャットで質問」
app/src/views/FeedView.tsx             開閉ボタン、行の「チャットで質問」、文脈の通知
app/src/views/DigestView.tsx           開閉ボタン、節の「チャットで質問」、文脈の通知
app/src/styles/chat.css                パネルのスタイル (新規)
README.md                              チャットの節
```

---

### Task 1: CLI の JSON 出力

**Files:**
- Create: `crates/cli/src/json.rs`
- Modify: `crates/cli/src/main.rs`、`crates/core/src/store/settings.rs`
- Test: `crates/cli/src/json.rs` (単体テスト)、`crates/core/src/store/settings.rs` (単体テスト)

**Interfaces:**
- Consumes: `Store::query_items`、`Store::get_item`、`Store::get_digest`、`Store::find_topic_by_name`、`Store::list_topics`、`period_start`、`Category::ALL`、`Category::label_ja`
- Produces (サイドカーが使う CLI の契約。全て標準出力に JSON 1つ、失敗は終了コード1と標準エラーに `error: <理由>`):
  - `infodesk --db <path> items --json [--category <c>] [--topic <名前>] [--days <n>] [--from YYYY-MM-DD] [--to YYYY-MM-DD] [--text <語>] [--limit <n>]` → `{"items":[ItemJson...],"total":<件数>}`。`limit` は既定 30、最大 100。`--from` はその日の0時 (ローカル) 以降、`--to` はその日の終わりまで (翌日0時未満)。
  - `ItemJson` = `{"id","title","url","source","category","category_label","fetched_at","published_at","score","summary"}`。`summary` は記事要約、なければ概要を 300 文字で切ったもの、どちらもなければ `null`。
  - `infodesk item <id> --json` → `{"item":ItemJson & {"excerpt","author","read","starred"}}`。ない id は終了コード1、`error: item <id> not found`。
  - `infodesk digests <day|week|month> --json [--date YYYY-MM-DD] [--category <c> | --topic <名前>]` → `{"period","period_start","digests":[DigestJson...]}`。`DigestJson` = `{"scope","label","status","body","item_ids","provider","created_at","last_error"}`。保存されていない scope は `status: "none"`、`body: ""`、`item_ids: []`、`provider: null`、`created_at: null`。生成はしない。
  - `infodesk config list --json` → `{"settings":{"<key>":"<value>",...}}`
  - `Store::list_settings(&self) -> Result<Vec<(String, String)>>` (キー順)

- [ ] **Step 1: `list_settings` の失敗するテストを書く**

`crates/core/src/store/settings.rs` の末尾に追加:

```rust
#[cfg(test)]
mod tests {
    use crate::store::Store;

    #[test]
    fn list_settings_returns_all_pairs_sorted_by_key() {
        let store = Store::open_in_memory().unwrap();
        store.set_setting("b.key", "2").unwrap();
        store.set_setting("a.key", "1").unwrap();
        assert_eq!(
            store.list_settings().unwrap(),
            vec![
                ("a.key".to_string(), "1".to_string()),
                ("b.key".to_string(), "2".to_string())
            ]
        );
    }
}
```

- [ ] **Step 2: 失敗を確かめる**

Run: `cargo test -p infodesk-core list_settings`
Expected: FAIL (`no method named list_settings`)

- [ ] **Step 3: `list_settings` を実装する**

`impl Store` の中に追加:

```rust
    /// 全ての設定をキー順に返す。
    pub fn list_settings(&self) -> Result<Vec<(String, String)>> {
        let mut stmt = self
            .conn
            .prepare("SELECT key, value FROM settings ORDER BY key")?;
        let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }
```

Run: `cargo test -p infodesk-core list_settings` → PASS

- [ ] **Step 4: JSON 組み立ての失敗するテストを書く**

`crates/cli/src/json.rs` を作る。テスト用のデータ作成には公開 API (`Store::add_source`、`Store::get_source`、`Store::upsert_items`、`Store::save_digest_done`) を使う。`main.rs` に `mod json;` を足す。

```rust
//! サイドカーのツールが読む JSON 出力。形は計画 (Plan 3 Task 1) の契約どおりにする。

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{NaiveDate, TimeZone, Utc};
    use infodesk_core::model::{Category, NewItem, Period, SourceKind, DigestScope};
    use infodesk_core::store::NewDigest;
    use infodesk_core::Store;

    fn tz() -> chrono::FixedOffset {
        chrono::FixedOffset::east_opt(9 * 3600).unwrap()
    }

    fn item(url: &str, title: &str, excerpt: &str) -> NewItem {
        NewItem {
            url: url.into(),
            title: title.into(),
            excerpt: excerpt.into(),
            author: None,
            published_at: None,
            score: Some(10),
        }
    }

    fn seeded() -> Store {
        let store = Store::open_in_memory().unwrap();
        let id = store
            .add_source(SourceKind::Rss, "介護ニュース", Category::Medcare, "https://example.com/feed")
            .unwrap();
        let source = store.get_source(id).unwrap().unwrap();
        // 2026-10-07 10:00 JST と 2026-10-08 10:00 JST に取得した記事
        let d7 = Utc.with_ymd_and_hms(2026, 10, 7, 1, 0, 0).unwrap();
        let d8 = Utc.with_ymd_and_hms(2026, 10, 8, 1, 0, 0).unwrap();
        store.upsert_items(&source, &[item("https://a/1", "認知症の新薬", &"長い概要。".repeat(100))], &d7).unwrap();
        store.upsert_items(&source, &[item("https://a/2", "介護報酬の改定", "")], &d8).unwrap();
        store
    }

    #[test]
    fn items_json_filters_by_local_dates_and_shapes_fields() {
        let store = seeded();
        let args = ItemsArgs {
            from: Some(NaiveDate::from_ymd_opt(2026, 10, 8).unwrap()),
            to: Some(NaiveDate::from_ymd_opt(2026, 10, 8).unwrap()),
            limit: 30,
            ..Default::default()
        };
        let v = items_json(&store, &args, tz()).unwrap();
        assert_eq!(v["total"], 1);
        let first = &v["items"][0];
        assert_eq!(first["title"], "介護報酬の改定");
        assert_eq!(first["source"], "介護ニュース");
        assert_eq!(first["category"], "medcare");
        assert_eq!(first["category_label"], "医療介護");
        assert!(first["summary"].is_null(), "空の概要は null");
        assert!(first["id"].is_i64());
    }

    #[test]
    fn items_json_truncates_excerpt_summary_and_caps_limit() {
        let store = seeded();
        let args = ItemsArgs { limit: 500, ..Default::default() };
        let v = items_json(&store, &args, tz()).unwrap();
        assert_eq!(v["items"].as_array().unwrap().len(), 2);
        let long = v["items"].as_array().unwrap().iter().find(|i| i["title"] == "認知症の新薬").unwrap();
        assert_eq!(long["summary"].as_str().unwrap().chars().count(), 301, "300文字 + …");
        assert_eq!(clamp_limit(500), 100);
        assert_eq!(clamp_limit(0), 30);
    }

    #[test]
    fn item_json_includes_detail_and_rejects_unknown_id() {
        let store = seeded();
        let id = store.query_items(&Default::default()).unwrap()[0].id;
        let v = item_json(&store, id).unwrap();
        assert_eq!(v["item"]["id"], id);
        assert!(v["item"]["excerpt"].is_string());
        assert_eq!(v["item"]["read"], false);
        let err = item_json(&store, 9999).unwrap_err().to_string();
        assert!(err.contains("item 9999 not found"), "{err}");
    }

    #[test]
    fn digests_json_reports_saved_and_missing_scopes_without_generating() {
        let store = seeded();
        let now = Utc.with_ymd_and_hms(2026, 10, 8, 1, 0, 0).unwrap();
        store
            .save_digest_done(
                &NewDigest {
                    period: Period::Day,
                    period_start: NaiveDate::from_ymd_opt(2026, 10, 8).unwrap(),
                    scope: DigestScope::Category(Category::Medcare),
                    body: "- 介護報酬の議論が始まった".into(),
                    item_ids: vec![2],
                    provider: "ollama:gemma4:26b".into(),
                },
                &now,
            )
            .unwrap();
        let date = NaiveDate::from_ymd_opt(2026, 10, 8).unwrap();
        let v = digests_json(&store, Period::Day, date, None, None).unwrap();
        assert_eq!(v["period"], "day");
        assert_eq!(v["period_start"], "2026-10-08");
        let all = v["digests"].as_array().unwrap();
        assert_eq!(all.len(), Category::ALL.len());
        let med = all.iter().find(|d| d["scope"] == "category:medcare").unwrap();
        assert_eq!(med["status"], "done");
        assert_eq!(med["label"], "医療介護");
        let gh = all.iter().find(|d| d["scope"] == "category:github").unwrap();
        assert_eq!(gh["status"], "none");
        assert!(gh["provider"].is_null());
        // 1カテゴリーだけ
        let one = digests_json(&store, Period::Day, date, Some(Category::Medcare), None).unwrap();
        assert_eq!(one["digests"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn digests_json_for_a_topic_uses_the_topic_name_as_label() {
        let store = seeded();
        let tid = store.add_topic("認知症", &["認知症".to_string()]).unwrap();
        let date = NaiveDate::from_ymd_opt(2026, 10, 8).unwrap();
        let v = digests_json(&store, Period::Week, date, None, Some(tid)).unwrap();
        assert_eq!(v["period_start"], "2026-10-05", "週は月曜始まり");
        let d = &v["digests"][0];
        assert_eq!(d["scope"], format!("topic:{tid}"));
        assert_eq!(d["label"], "認知症");
    }

    #[test]
    fn config_json_lists_settings() {
        let store = seeded();
        store.set_setting("ollama.model", "gemma4:26b").unwrap();
        let v = config_json(&store).unwrap();
        assert_eq!(v["settings"]["ollama.model"], "gemma4:26b");
    }
}
```

`save_digest_done` と `upsert_items` の引数は `crates/core/src/store/digests.rs:48` と `crates/core/src/store/items.rs:85` のとおり。

- [ ] **Step 5: 失敗を確かめる**

Run: `cargo test -p infodesk json::`
Expected: FAIL (`cannot find function items_json` など)

- [ ] **Step 6: `json.rs` を実装する**

`crates/cli/Cargo.toml` に `cargo add -p infodesk serde_json serde --features serde/derive` で依存を足す。

```rust
use chrono::{Duration, FixedOffset, NaiveDate, TimeZone, Utc};
use infodesk_core::collector::text::truncate_chars;
use infodesk_core::model::{Category, DigestScope, Item, ItemFilter, ItemOrder, Period};
use infodesk_core::period::period_start;
use infodesk_core::{Error, Result, Store};
use serde_json::{Value, json};

const DEFAULT_LIMIT: u32 = 30;
const MAX_LIMIT: u32 = 100;
const SUMMARY_CHARS: usize = 300;

#[derive(Debug, Default, Clone)]
pub struct ItemsArgs {
    pub category: Option<Category>,
    pub topic_id: Option<i64>,
    pub days: Option<i64>,
    pub from: Option<NaiveDate>,
    pub to: Option<NaiveDate>,
    pub text: Option<String>,
    pub limit: u32,
}

pub fn clamp_limit(limit: u32) -> u32 {
    match limit {
        0 => DEFAULT_LIMIT,
        n => n.min(MAX_LIMIT),
    }
}

fn local_midnight_utc(date: NaiveDate, tz: FixedOffset) -> chrono::DateTime<Utc> {
    tz.from_local_datetime(&date.and_hms_opt(0, 0, 0).expect("midnight"))
        .single()
        .expect("fixed offset is unambiguous")
        .with_timezone(&Utc)
}

fn item_value(item: &Item) -> Value {
    let summary = item
        .ai_summary
        .clone()
        .filter(|s| !s.trim().is_empty())
        .or_else(|| {
            let e = item.excerpt.trim();
            (!e.is_empty()).then(|| truncate_chars(e, SUMMARY_CHARS))
        });
    json!({
        "id": item.id,
        "title": item.title,
        "url": item.url,
        "source": item.source_name,
        "category": item.category.as_str(),
        "category_label": item.category.label_ja(),
        "fetched_at": item.fetched_at,
        "published_at": item.published_at,
        "score": item.score,
        "summary": summary,
    })
}

pub fn items_json(store: &Store, args: &ItemsArgs, tz: FixedOffset) -> Result<Value> {
    let mut filter = ItemFilter {
        category: args.category,
        topic_id: args.topic_id,
        text: args.text.clone(),
        order: ItemOrder::Recent,
        limit: clamp_limit(args.limit),
        ..Default::default()
    };
    if let Some(d) = args.days {
        filter.since = Some(Utc::now() - Duration::days(d));
    }
    if let Some(from) = args.from {
        filter.since = Some(local_midnight_utc(from, tz));
    }
    if let Some(to) = args.to {
        filter.until = Some(local_midnight_utc(to + Duration::days(1), tz));
    }
    let total = store.count_items(&filter)?;
    let items: Vec<Value> = store.query_items(&filter)?.iter().map(item_value).collect();
    Ok(json!({ "items": items, "total": total }))
}

pub fn item_json(store: &Store, id: i64) -> Result<Value> {
    let item = store
        .get_item(id)?
        .ok_or_else(|| Error::Invalid(format!("item {id} not found")))?;
    let mut v = item_value(&item);
    let obj = v.as_object_mut().expect("object");
    obj.insert("excerpt".into(), json!(item.excerpt));
    obj.insert("author".into(), json!(item.author));
    obj.insert("read".into(), json!(item.read));
    obj.insert("starred".into(), json!(item.starred));
    Ok(json!({ "item": v }))
}

pub fn digests_json(
    store: &Store,
    period: Period,
    date: NaiveDate,
    category: Option<Category>,
    topic_id: Option<i64>,
) -> Result<Value> {
    let start = period_start(period, date);
    let scopes: Vec<(DigestScope, String)> = match (category, topic_id) {
        (_, Some(id)) => {
            let topic = store
                .get_topic(id)?
                .ok_or_else(|| Error::Invalid(format!("topic {id} not found")))?;
            vec![(DigestScope::Topic(id), topic.name)]
        }
        (Some(c), None) => vec![(DigestScope::Category(c), c.label_ja().to_string())],
        (None, None) => Category::ALL
            .into_iter()
            .map(|c| (DigestScope::Category(c), c.label_ja().to_string()))
            .collect(),
    };
    let mut out = Vec::new();
    for (scope, label) in scopes {
        let v = match store.get_digest(period, start, &scope)? {
            Some(d) => json!({
                "scope": scope.key(),
                "label": label,
                "status": d.status.as_str(),
                "body": d.body,
                "item_ids": d.item_ids,
                "provider": d.provider,
                "created_at": d.created_at,
                "last_error": d.last_error,
            }),
            None => json!({
                "scope": scope.key(),
                "label": label,
                "status": "none",
                "body": "",
                "item_ids": [],
                "provider": null,
                "created_at": null,
                "last_error": null,
            }),
        };
        out.push(v);
    }
    Ok(json!({
        "period": period.as_str(),
        "period_start": start.format("%Y-%m-%d").to_string(),
        "digests": out,
    }))
}

pub fn config_json(store: &Store) -> Result<Value> {
    let map: serde_json::Map<String, Value> = store
        .list_settings()?
        .into_iter()
        .map(|(k, v)| (k, Value::String(v)))
        .collect();
    Ok(json!({ "settings": map }))
}
```

`DigestStatus::as_str` (`model.rs:134`) と `DigestScope::key` (`model.rs:162`、`category:<c>` / `topic:<id>`) は既にある。`collector::text` が `pub mod` で公開されていることは `crates/core/src/collector/mod.rs:5` で確認済み。

Run: `cargo test -p infodesk json::` → PASS

- [ ] **Step 7: CLI に組み込む**

`main.rs`:
- `Command::Items` に `#[arg(long)] from: Option<String>`、`#[arg(long)] to: Option<String>`、`#[arg(long)] json: bool` を足す。`--json` のときは `json::items_json` を `println!("{}", serde_json::to_string(&v)?)` で出す (`serde_json::Error` は `Error::Invalid(e.to_string())` に写す)。`--json` なしの出力は今のまま。
- 新しいコマンド `Item { id: i64, #[arg(long)] json: bool }` (`--json` なしはタイトルと URL と要約を3行で出す)。
- 新しいコマンド `Digests { period: String, #[arg(long)] date: Option<String>, #[arg(long)] category: Option<String>, #[arg(long)] topic: Option<String>, #[arg(long)] json: bool }`。`--json` なしは scope ごとに `## <label> [<status>]` と本文を出す。
- `ConfigCommand::List { #[arg(long)] json: bool }` (`--json` なしは `key = value` を1行ずつ)。
- 日付の解析は既存の `digest` と同じ `NaiveDate::parse_from_str(s, "%Y-%m-%d")`。不正なら `Error::Invalid(format!("invalid date '{s}'"))`。

- [ ] **Step 8: 手で確かめる**

```bash
cargo build -p infodesk
DB=$(mktemp -d)/t.db
./target/debug/infodesk --db $DB items --json --limit 3
./target/debug/infodesk --db $DB digests day --json --category medcare
./target/debug/infodesk --db $DB config list --json
./target/debug/infodesk --db $DB item 1 --json; echo "exit=$?"
```

Expected: 1〜3行目は JSON 1行 (`{"items":[],"total":0}` など)。最後は `error: item 1 not found` と `exit=1`。

- [ ] **Step 9: 全体を確かめてコミットする**

```bash
cargo test --workspace && cargo clippy --all-targets -- -D warnings && cargo fmt --check
git add crates/cli crates/core/src/store/settings.rs Cargo.lock
git commit -m "feat(cli): add JSON output for items, item, digests and config"
```

---

### Task 2: 記事本文の取得 (`page`)

**Files:**
- Create: `crates/core/src/page.rs`
- Modify: `crates/core/src/lib.rs` (`pub mod page;`)、`crates/cli/src/main.rs`、`crates/cli/src/json.rs`
- Test: `crates/core/src/page.rs` (wiremock)

**Interfaces:**
- Consumes: `collector::http_client()`、`collector::text::collapse_ws`、`truncate_chars`
- Produces:
  - `pub struct PageText { pub url: String, pub final_url: String, pub title: Option<String>, pub text: String, pub truncated: bool }` (`Serialize`)
  - `pub const MAX_BYTES: usize = 2_000_000;` `pub const DEFAULT_MAX_CHARS: usize = 20_000;`
  - `pub async fn fetch_page(http: &reqwest::Client, url: &str, max_chars: usize) -> Result<PageText>`
  - CLI: `infodesk page <url> --json [--max-chars <n>]` → `{"page": PageText}`。失敗は終了コード1。

- [ ] **Step 1: 失敗するテストを書く**

`crates/core/src/page.rs`:

```rust
//! チャットのエージェントが求めた URL の本文をテキストにする。DB には保存しない (spec 12節)。

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    async fn serve(body: &str, content_type: &str) -> MockServer {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/a"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", content_type)
                    .set_body_string(body.to_string()),
            )
            .mount(&server)
            .await;
        server
    }

    fn client() -> reqwest::Client {
        crate::collector::http_client().unwrap()
    }

    #[tokio::test]
    async fn extracts_title_and_article_text_without_scripts_or_navigation() {
        let html = r#"<html><head><title> 新薬の記事 </title><style>p{}</style></head>
            <body><nav>メニュー</nav><header>ヘッダー</header>
            <article><h1>見出し</h1><p>本文の<b>一段落</b>目。</p><script>alert(1)</script><p>二段落目。</p></article>
            <footer>フッター</footer></body></html>"#;
        let server = serve(html, "text/html; charset=utf-8").await;
        let page = fetch_page(&client(), &format!("{}/a", server.uri()), 20_000).await.unwrap();
        assert_eq!(page.title.as_deref(), Some("新薬の記事"));
        assert!(page.text.contains("見出し"));
        assert!(page.text.contains("本文の 一段落 目。") || page.text.contains("本文の一段落目。"), "{}", page.text);
        assert!(page.text.contains("二段落目。"));
        for noise in ["メニュー", "ヘッダー", "フッター", "alert", "p{}"] {
            assert!(!page.text.contains(noise), "{noise} が残っている: {}", page.text);
        }
        assert!(!page.truncated);
    }

    #[tokio::test]
    async fn falls_back_to_body_without_article_and_accepts_plain_text() {
        let server = serve("<html><body><div>本文だけ</div></body></html>", "text/html").await;
        let page = fetch_page(&client(), &format!("{}/a", server.uri()), 100).await.unwrap();
        assert_eq!(page.text, "本文だけ");
        let server = serve("plain  text\nbody", "text/plain").await;
        let page = fetch_page(&client(), &format!("{}/a", server.uri()), 100).await.unwrap();
        assert_eq!(page.text, "plain text body");
        assert_eq!(page.title, None);
    }

    #[tokio::test]
    async fn truncates_long_text_and_marks_it() {
        let long = format!("<article><p>{}</p></article>", "あ".repeat(50));
        let server = serve(&long, "text/html").await;
        let page = fetch_page(&client(), &format!("{}/a", server.uri()), 10).await.unwrap();
        assert!(page.truncated);
        assert_eq!(page.text.chars().count(), 11, "10文字 + …");
    }

    #[tokio::test]
    async fn stops_reading_after_max_bytes() {
        let huge = format!("<article><p>{}</p></article>", "a".repeat(MAX_BYTES + 500_000));
        let server = serve(&huge, "text/html").await;
        let page = fetch_page(&client(), &format!("{}/a", server.uri()), usize::MAX).await.unwrap();
        assert!(page.truncated, "上限で読むのをやめたら truncated");
        assert!(page.text.len() <= MAX_BYTES);
    }

    #[tokio::test]
    async fn rejects_unsupported_types_errors_and_schemes() {
        let server = serve("%PDF-1.7", "application/pdf").await;
        let err = fetch_page(&client(), &format!("{}/a", server.uri()), 100).await.unwrap_err();
        assert!(err.to_string().contains("unsupported content type: application/pdf"), "{err}");

        let server = MockServer::start().await;
        Mock::given(method("GET")).respond_with(ResponseTemplate::new(404)).mount(&server).await;
        let err = fetch_page(&client(), &format!("{}/a", server.uri()), 100).await.unwrap_err();
        assert!(err.to_string().contains("404"), "{err}");

        for bad in ["file:///etc/passwd", "ftp://example.com/x", "not a url"] {
            let err = fetch_page(&client(), bad, 100).await.unwrap_err();
            assert!(err.to_string().contains("only http and https"), "{bad}: {err}");
        }
    }
}
```

- [ ] **Step 2: 失敗を確かめる**

Run: `cargo test -p infodesk-core page::`
Expected: FAIL (`cannot find function fetch_page`)

- [ ] **Step 3: 実装する**

`page.rs` のテストの上に:

```rust
use scraper::{Html, Node, Selector};
use serde::Serialize;

use crate::collector::text::{collapse_ws, truncate_chars};
use crate::error::{Error, Result};

pub const MAX_BYTES: usize = 2_000_000;
pub const DEFAULT_MAX_CHARS: usize = 20_000;

/// 本文から除く要素。
const SKIP: &[&str] = &[
    "script", "style", "noscript", "nav", "header", "footer", "aside", "form", "svg", "template",
];

#[derive(Debug, Clone, Serialize)]
pub struct PageText {
    pub url: String,
    pub final_url: String,
    pub title: Option<String>,
    pub text: String,
    pub truncated: bool,
}

pub async fn fetch_page(http: &reqwest::Client, url: &str, max_chars: usize) -> Result<PageText> {
    let parsed = reqwest::Url::parse(url)
        .ok()
        .filter(|u| matches!(u.scheme(), "http" | "https"))
        .ok_or_else(|| Error::Invalid(format!("only http and https URLs can be fetched: {url}")))?;
    let mut resp = http.get(parsed).send().await?;
    let status = resp.status();
    if !status.is_success() {
        return Err(Error::Invalid(format!("fetch failed: HTTP {}", status.as_u16())));
    }
    let content_type = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("text/html")
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    let is_html = content_type == "text/html" || content_type == "application/xhtml+xml";
    if !is_html && content_type != "text/plain" {
        return Err(Error::Invalid(format!("unsupported content type: {content_type}")));
    }
    let final_url = resp.url().to_string();
    let mut bytes: Vec<u8> = Vec::new();
    let mut cut = false;
    while let Some(chunk) = resp.chunk().await? {
        let room = MAX_BYTES - bytes.len();
        if chunk.len() >= room {
            bytes.extend_from_slice(&chunk[..room]);
            cut = true;
            break;
        }
        bytes.extend_from_slice(&chunk);
    }
    let raw = String::from_utf8_lossy(&bytes);
    let (title, text) = if is_html { html_to_text(&raw) } else { (None, collapse_ws(&raw)) };
    let truncated_by_chars = text.chars().count() > max_chars;
    let text = if truncated_by_chars { truncate_chars(&text, max_chars) } else { text };
    Ok(PageText {
        url: url.to_string(),
        final_url,
        title,
        text,
        truncated: cut || truncated_by_chars,
    })
}

fn html_to_text(raw: &str) -> (Option<String>, String) {
    let doc = Html::parse_document(raw);
    let title = Selector::parse("title")
        .ok()
        .and_then(|s| doc.select(&s).next())
        .map(|t| collapse_ws(&t.text().collect::<String>()))
        .filter(|t| !t.is_empty());
    let root = ["article", "main", "body"]
        .iter()
        .filter_map(|tag| Selector::parse(tag).ok())
        .find_map(|s| doc.select(&s).next());
    let Some(root) = root else {
        return (title, String::new());
    };
    let mut parts: Vec<&str> = Vec::new();
    for node in root.descendants() {
        let Node::Text(text) = node.value() else { continue };
        let skipped = node.ancestors().any(|a| {
            a.value()
                .as_element()
                .is_some_and(|e| SKIP.contains(&e.name()))
        });
        if !skipped {
            parts.push(text);
        }
    }
    (title, collapse_ws(&parts.join(" ")))
}
```

`scraper` 0.27 の `ElementRef::descendants`、`Node::Text`、`NodeRef::ancestors`、`Element::name` の名前は `~/.cargo/registry/src/*/scraper-0.27*/src` と `ego-tree` のソースで確かめる。名前が違えば同じ意味の API に置き換え、計画との差として報告する。`reqwest::Response::chunk` は reqwest 0.13 にある (`~/.cargo/registry/src/*/reqwest-0.13*/src/async_impl/response.rs` で確かめる)。

Run: `cargo test -p infodesk-core page::` → PASS

- [ ] **Step 4: CLI に `page` を足す**

`main.rs` に `Page { url: String, #[arg(long, default_value_t = infodesk_core::page::DEFAULT_MAX_CHARS)] max_chars: usize, #[arg(long)] json: bool }`。`--json` は `{"page": PageText}`、なしは1行目にタイトル、空行、本文。DB を開かずに動くよう、`run` の中で `Command::Page` を `Store::open` より先に処理する。`json.rs` に `pub fn page_json(p: &PageText) -> Value { json!({ "page": p }) }` を足し、テスト1件 (フィールド名 `final_url` と `truncated` が出る) を書く。

Run: `cargo run -p infodesk -- page https://example.com/ --json` (ネットワークがある場合だけ) で `{"page":{"url":"https://example.com/",...}}` が出る。

- [ ] **Step 5: コミット**

```bash
cargo test --workspace && cargo clippy --all-targets -- -D warnings && cargo fmt --check
git add crates
git commit -m "feat(core): fetch page text for the chat agent"
```

---

### Task 3: スレッドとメッセージの保存

**Files:**
- Create: `crates/core/src/store/chat.rs`、`app/src-tauri/src/commands/chat.rs`
- Modify: `crates/core/src/store/migrate.rs`、`crates/core/src/store/mod.rs`、`crates/core/src/model.rs`、`app/src-tauri/src/commands/mod.rs`、`app/src-tauri/src/lib.rs`、`app/src/types.ts`、`app/src/api.ts`
- Test: `crates/core/src/store/chat.rs`、`crates/core/src/store/migrate.rs`、`app/src-tauri/src/commands/chat.rs`、`app/src/api.test.ts`

**Interfaces:**
- Produces (core):
  - `pub struct ChatThread { pub id: i64, pub title: String, pub created_at: DateTime<Utc>, pub updated_at: DateTime<Utc> }` (`Serialize`、`Deserialize`、`Clone`、`Debug`、`PartialEq`)
  - `Store::create_chat_thread(&self, title: &str, now: &DateTime<Utc>) -> Result<ChatThread>`
  - `Store::list_chat_threads(&self) -> Result<Vec<ChatThread>>` (更新の新しい順)
  - `Store::rename_chat_thread(&self, id: i64, title: &str) -> Result<()>`
  - `Store::delete_chat_thread(&self, id: i64) -> Result<()>` (メッセージも消す)
  - `Store::get_chat_messages(&self, thread_id: i64) -> Result<Vec<serde_json::Value>>` (保存順)
  - `Store::save_chat_messages(&self, thread_id: i64, messages: &[serde_json::Value], now: &DateTime<Utc>) -> Result<()>` (スレッドの全メッセージを置き換え、`updated_at` を `now` にする。ないスレッドは `Error::Invalid("chat thread <id> not found")`)
- Produces (Tauri コマンド。引数は camelCase で受ける):
  - `list_chat_threads() -> Vec<ChatThread>`、`create_chat_thread(title) -> ChatThread`、`rename_chat_thread(id, title)`、`delete_chat_thread(id)`、`get_chat_messages(threadId) -> Vec<Value>`、`save_chat_messages(threadId, messages: Vec<Value>)`
- Produces (UI): `types.ts` に `ChatThread`。`api.ts` に `listChatThreads`、`createChatThread(title)`、`renameChatThread(id, title)`、`deleteChatThread(id)`、`getChatMessages(threadId)`、`saveChatMessages(threadId, messages)`。メッセージの型は `UIMessage` (`import type { UIMessage } from "ai"`、Task 7 で依存を入れるまでは `unknown[]`、Task 7 で `UIMessage[]` に変える)。

- [ ] **Step 1: マイグレーションの失敗するテストを書く**

`migrate.rs` のテストに追加:

```rust
    #[test]
    fn version_3_adds_chat_tables() {
        let conn = Connection::open_in_memory().unwrap();
        migrate(&conn).unwrap();
        assert_eq!(version(&conn), 3);
        assert!(has_column(&conn, "chat_threads", "updated_at"));
        assert!(has_column(&conn, "chat_messages", "content"));
    }
```

- [ ] **Step 2: 失敗を確かめ、版3を足す**

Run: `cargo test -p infodesk-core version_3` → FAIL。

`SCHEMA_VERSION` を 3 にし、`MIGRATIONS` に追加:

```rust
    (
        3,
        "CREATE TABLE IF NOT EXISTS chat_threads (
             id INTEGER PRIMARY KEY,
             title TEXT NOT NULL,
             created_at TEXT NOT NULL,
             updated_at TEXT NOT NULL
         );
         CREATE TABLE IF NOT EXISTS chat_messages (
             thread_id INTEGER NOT NULL REFERENCES chat_threads(id) ON DELETE CASCADE,
             seq INTEGER NOT NULL,
             id TEXT NOT NULL,
             role TEXT NOT NULL,
             content TEXT NOT NULL,
             created_at TEXT NOT NULL,
             PRIMARY KEY (thread_id, seq)
         );",
    ),
```

既存テスト `fresh_database_reaches_current_version` は `SCHEMA_VERSION` を見るので変えない。Run: `cargo test -p infodesk-core migrate` → PASS。

- [ ] **Step 3: 保存の失敗するテストを書く**

`crates/core/src/store/chat.rs` (`mod.rs` に `mod chat;`、`model.rs` に `ChatThread`):

```rust
#[cfg(test)]
mod tests {
    use crate::store::Store;
    use chrono::{TimeZone, Utc};
    use serde_json::json;

    fn t(h: u32) -> chrono::DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 9, h, 0, 0).unwrap()
    }

    fn msg(id: &str, role: &str, text: &str) -> serde_json::Value {
        json!({ "id": id, "role": role, "parts": [{ "type": "text", "text": text }] })
    }

    #[test]
    fn creates_lists_newest_first_and_renames() {
        let s = Store::open_in_memory().unwrap();
        let a = s.create_chat_thread("古い", &t(1)).unwrap();
        let b = s.create_chat_thread("新しい", &t(2)).unwrap();
        assert_eq!(s.list_chat_threads().unwrap().iter().map(|x| x.id).collect::<Vec<_>>(), vec![b.id, a.id]);
        s.rename_chat_thread(a.id, "  名前を変えた  ").unwrap();
        assert_eq!(s.list_chat_threads().unwrap()[1].title, "名前を変えた");
    }

    #[test]
    fn save_replaces_messages_in_order_and_bumps_updated_at() {
        let s = Store::open_in_memory().unwrap();
        let a = s.create_chat_thread("A", &t(1)).unwrap();
        let b = s.create_chat_thread("B", &t(2)).unwrap();
        s.save_chat_messages(a.id, &[msg("m1", "user", "質問"), msg("m2", "assistant", "回答")], &t(3)).unwrap();
        assert_eq!(s.get_chat_messages(a.id).unwrap()[1]["parts"][0]["text"], "回答");
        // 置き換え: 2件 → 1件
        s.save_chat_messages(a.id, &[msg("m1", "user", "質問だけ")], &t(4)).unwrap();
        let got = s.get_chat_messages(a.id).unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0]["parts"][0]["text"], "質問だけ");
        // 更新したスレッドが先頭に来る
        assert_eq!(s.list_chat_threads().unwrap()[0].id, a.id);
        assert_eq!(s.list_chat_threads().unwrap()[0].updated_at, t(4));
        assert!(s.get_chat_messages(b.id).unwrap().is_empty());
    }

    #[test]
    fn delete_removes_thread_and_its_messages() {
        let s = Store::open_in_memory().unwrap();
        let a = s.create_chat_thread("A", &t(1)).unwrap();
        s.save_chat_messages(a.id, &[msg("m1", "user", "x")], &t(2)).unwrap();
        s.delete_chat_thread(a.id).unwrap();
        assert!(s.list_chat_threads().unwrap().is_empty());
        assert!(s.get_chat_messages(a.id).unwrap().is_empty());
        let err = s.save_chat_messages(a.id, &[], &t(3)).unwrap_err().to_string();
        assert!(err.contains("chat thread"), "{err}");
    }

    #[test]
    fn rejects_messages_without_id_or_role() {
        let s = Store::open_in_memory().unwrap();
        let a = s.create_chat_thread("A", &t(1)).unwrap();
        let err = s.save_chat_messages(a.id, &[json!({ "parts": [] })], &t(2)).unwrap_err().to_string();
        assert!(err.contains("message without id or role"), "{err}");
        assert!(s.get_chat_messages(a.id).unwrap().is_empty(), "失敗したら何も書かない");
    }
}
```

Run: `cargo test -p infodesk-core chat::` → FAIL

- [ ] **Step 4: 実装する**

```rust
use chrono::{DateTime, Utc};
use rusqlite::{OptionalExtension, params};
use serde_json::Value;

use super::{Store, col_ts, ts};
use crate::error::{Error, Result};
use crate::model::ChatThread;

fn row_to_thread(r: &rusqlite::Row) -> rusqlite::Result<ChatThread> {
    Ok(ChatThread {
        id: r.get(0)?,
        title: r.get(1)?,
        created_at: col_ts(r, 2)?,
        updated_at: col_ts(r, 3)?,
    })
}

impl Store {
    pub fn create_chat_thread(&self, title: &str, now: &DateTime<Utc>) -> Result<ChatThread> {
        self.conn.execute(
            "INSERT INTO chat_threads(title, created_at, updated_at) VALUES (?1, ?2, ?2)",
            params![title.trim(), ts(now)],
        )?;
        let id = self.conn.last_insert_rowid();
        Ok(ChatThread { id, title: title.trim().to_string(), created_at: *now, updated_at: *now })
    }

    pub fn list_chat_threads(&self) -> Result<Vec<ChatThread>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, title, created_at, updated_at FROM chat_threads ORDER BY updated_at DESC, id DESC",
        )?;
        let rows = stmt.query_map([], row_to_thread)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn rename_chat_thread(&self, id: i64, title: &str) -> Result<()> {
        self.conn.execute("UPDATE chat_threads SET title = ?2 WHERE id = ?1", params![id, title.trim()])?;
        Ok(())
    }

    pub fn delete_chat_thread(&self, id: i64) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        tx.execute("DELETE FROM chat_messages WHERE thread_id = ?1", params![id])?;
        tx.execute("DELETE FROM chat_threads WHERE id = ?1", params![id])?;
        tx.commit()?;
        Ok(())
    }

    pub fn get_chat_messages(&self, thread_id: i64) -> Result<Vec<Value>> {
        let mut stmt = self
            .conn
            .prepare("SELECT content FROM chat_messages WHERE thread_id = ?1 ORDER BY seq")?;
        let rows = stmt.query_map(params![thread_id], |r| r.get::<_, String>(0))?;
        let mut out = Vec::new();
        for s in rows {
            let s = s?;
            out.push(serde_json::from_str(&s).map_err(|e| Error::Invalid(format!("stored chat message is not JSON: {e}")))?);
        }
        Ok(out)
    }

    pub fn save_chat_messages(&self, thread_id: i64, messages: &[Value], now: &DateTime<Utc>) -> Result<()> {
        let exists: Option<i64> = self
            .conn
            .query_row("SELECT id FROM chat_threads WHERE id = ?1", params![thread_id], |r| r.get(0))
            .optional()?;
        if exists.is_none() {
            return Err(Error::Invalid(format!("chat thread {thread_id} not found")));
        }
        let mut rows = Vec::with_capacity(messages.len());
        for m in messages {
            let (Some(id), Some(role)) = (m.get("id").and_then(Value::as_str), m.get("role").and_then(Value::as_str)) else {
                return Err(Error::Invalid("message without id or role".into()));
            };
            rows.push((id.to_string(), role.to_string(), m.to_string()));
        }
        let tx = self.conn.unchecked_transaction()?;
        tx.execute("DELETE FROM chat_messages WHERE thread_id = ?1", params![thread_id])?;
        for (seq, (id, role, content)) in rows.iter().enumerate() {
            tx.execute(
                "INSERT INTO chat_messages(thread_id, seq, id, role, content, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![thread_id, seq as i64, id, role, content, ts(now)],
            )?;
        }
        tx.execute("UPDATE chat_threads SET updated_at = ?2 WHERE id = ?1", params![thread_id, ts(now)])?;
        tx.commit()?;
        Ok(())
    }
}
```

`Error::Invalid` の文言の書き方と `col_ts`、`ts` の可視性は `crates/core/src/store/mod.rs` のとおり (`pub(crate)`)。Run: `cargo test -p infodesk-core chat::` → PASS。

- [ ] **Step 5: Tauri コマンドのテストを書いて実装する**

`app/src-tauri/src/commands/chat.rs`。既存の `commands/topics.rs` と同じく、本体を `_impl(state: &AppState, ...)` に分け、`#[tauri::command]` はそれを呼ぶだけにする。テストは `crate::state::test_state()` を使う。戻り値は `(AppState, Arc<Mutex<Vec<String>>>)` (2つ目は発火したイベント名)。`DigestStatus::as_str` は既にある (`model.rs:134`)。

```rust
use chrono::Utc;
use serde_json::Value;
use tauri::State;

use crate::error::AppResult;
use crate::state::AppState;
use infodesk_core::model::ChatThread;

pub fn create_impl(state: &AppState, title: &str) -> AppResult<ChatThread> {
    state.with_store(|s| s.create_chat_thread(title, &Utc::now()))
}
pub fn list_impl(state: &AppState) -> AppResult<Vec<ChatThread>> {
    state.with_store(|s| s.list_chat_threads())
}
pub fn save_impl(state: &AppState, thread_id: i64, messages: &[Value]) -> AppResult<()> {
    state.with_store(|s| s.save_chat_messages(thread_id, messages, &Utc::now()))
}

#[tauri::command]
pub fn list_chat_threads(state: State<'_, AppState>) -> AppResult<Vec<ChatThread>> { list_impl(&state) }
#[tauri::command]
pub fn create_chat_thread(state: State<'_, AppState>, title: String) -> AppResult<ChatThread> { create_impl(&state, &title) }
#[tauri::command]
pub fn rename_chat_thread(state: State<'_, AppState>, id: i64, title: String) -> AppResult<()> {
    state.with_store(|s| s.rename_chat_thread(id, &title))
}
#[tauri::command]
pub fn delete_chat_thread(state: State<'_, AppState>, id: i64) -> AppResult<()> {
    state.with_store(|s| s.delete_chat_thread(id))
}
#[tauri::command]
pub fn get_chat_messages(state: State<'_, AppState>, thread_id: i64) -> AppResult<Vec<Value>> {
    state.with_store(|s| s.get_chat_messages(thread_id))
}
#[tauri::command]
pub fn save_chat_messages(state: State<'_, AppState>, thread_id: i64, messages: Vec<Value>) -> AppResult<()> {
    save_impl(&state, thread_id, &messages)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::test_state;
    use serde_json::json;

    #[test]
    fn create_save_and_list_round_trip() {
        let (state, _events) = test_state();
        let t = create_impl(&state, "新しいスレッド").unwrap();
        save_impl(&state, t.id, &[json!({ "id": "m1", "role": "user", "parts": [] })]).unwrap();
        let threads = list_impl(&state).unwrap();
        assert_eq!(threads.len(), 1);
        assert_eq!(threads[0].title, "新しいスレッド");
        let err = save_impl(&state, 999, &[]).unwrap_err();
        assert!(err.message.contains("chat thread 999 not found"));
    }

    #[test]
    fn thread_serializes_with_snake_case_fields() {
        let (state, _events) = test_state();
        let t = create_impl(&state, "x").unwrap();
        let v = serde_json::to_value(&t).unwrap();
        assert!(v.get("created_at").is_some() && v.get("updated_at").is_some());
    }
}
```

`mod.rs` に `pub mod chat;`、`lib.rs` の `generate_handler!` に6つを足す。Run: `cargo test -p infodesk-app chat` → PASS。

- [ ] **Step 6: UI の型とラッパー**

`types.ts`:

```ts
export interface ChatThread {
  id: number;
  title: string;
  created_at: string;
  updated_at: string;
}
```

`api.ts` の `api` に:

```ts
  listChatThreads: () => invoke<ChatThread[]>("list_chat_threads"),
  createChatThread: (title: string) => invoke<ChatThread>("create_chat_thread", { title }),
  renameChatThread: (id: number, title: string) => invoke<void>("rename_chat_thread", { id, title }),
  deleteChatThread: (id: number) => invoke<void>("delete_chat_thread", { id }),
  getChatMessages: (threadId: number) => invoke<unknown[]>("get_chat_messages", { threadId }),
  saveChatMessages: (threadId: number, messages: unknown[]) =>
    invoke<void>("save_chat_messages", { threadId, messages }),
```

`api.test.ts` に、`getChatMessages(3)` が `get_chat_messages` を `{ threadId: 3 }` で呼び、`saveChatMessages(3, [m])` が `{ threadId: 3, messages: [m] }` で呼ぶテストを足す (既存の `api.test.ts` の書き方に合わせる)。

- [ ] **Step 7: コミット**

```bash
cargo test --workspace && cargo clippy --all-targets -- -D warnings && cargo fmt --check
cd app && npm test -- --run && npm run build && cd ..
git add crates app/src-tauri/src app/src/types.ts app/src/api.ts app/src/api.test.ts
git commit -m "feat(app): store chat threads and messages"
```

---
### Task 4: サイドカーの土台、CLI 呼び出し、ツール

**Files:**
- Create: `agent/package.json`、`agent/tsconfig.json`、`agent/vitest.config.ts`、`agent/src/env.ts`、`agent/src/cli.ts`、`agent/src/context.ts`、`agent/src/tools.ts`、`agent/test/fake-infodesk.mjs`、`agent/test/cli.test.ts`、`agent/test/tools.test.ts`
- Modify: `.gitignore` (`agent/node_modules/`、`agent/dist/`)

**Interfaces:**
- Consumes: Task 1 と Task 2 の CLI の契約
- Produces:
  - `env.ts`: `interface AgentEnv { cli: string[]; db: string; token: string; port: number }`、`readEnv(env?: NodeJS.ProcessEnv): AgentEnv` (必須: `INFODESK_CLI` (実行ファイル)、`INFODESK_DB`、`INFODESK_AGENT_TOKEN`。任意: `INFODESK_CLI_ARG` (実行ファイルの直後に付ける引数1つ。アプリ本体を CLI として使うときは `--cli`)、`INFODESK_AGENT_PORT` (既定 0))。足りなければ `Error("missing environment variable: <名前>")`。
  - `cli.ts`: `class CliError extends Error`、`type CliRunner = (args: string[], signal?: AbortSignal) => Promise<unknown>`、`createCliRunner(command: string[], db: string, timeoutMs?: number): CliRunner` (`command` は実行ファイルと前置きの引数。その後に `--db <db>` を付け、標準出力の JSON を返す。非0終了は `CliError(<標準エラーから "error: " を除いた文>)`)。
  - `context.ts`: `ChatContext` 型 (UI と同じ形。Task 7 の `app/src/chat/context.ts` と一致させる)。
  - `tools.ts`: `createInfodeskTools(run: CliRunner, opts: { webSearchKey?: string; fetchImpl?: typeof fetch }): Record<string, Tool>`。ツール名は `search_articles`、`get_article`、`read_digests`、`fetch_page`、`web_search` (キーがある場合だけ)。`TOOL_NAMES` 定数で名前を公開する。

- [ ] **Step 1: パッケージを作る**

```bash
mkdir -p agent/src agent/test
cd agent
npm init -y
npm pkg set name=infodesk-agent private=true type=module engines.node=">=22"
npm pkg set scripts.test=vitest scripts.typecheck="tsc --noEmit" scripts.build="tsc --noEmit && node build.mjs"
npm install --save-exact ai@7.0.136 ai-sdk-ollama@4.4.0 ai-sdk-provider-claude-code@4.3.3 zod@4.6.5
npm install --save-dev --save-exact esbuild@0.28.2
npm install --save-dev typescript@~6.0.3 vitest@^5 @types/node@^22
```

`ai-sdk-provider-claude-code` の依存 `@anthropic-ai/claude-agent-sdk` は、環境の CPU に合う `claude` 実行ファイルを任意依存として入れる (数百MB)。InfoDesk は設定 `claude.bin` の CLI を使うので、この実行ファイルは使わない。`npm install` が終わったら `du -sh node_modules` を報告に書く。

`agent/tsconfig.json`:

```json
{
  "compilerOptions": {
    "target": "ES2023",
    "module": "NodeNext",
    "moduleResolution": "NodeNext",
    "strict": true,
    "skipLibCheck": true,
    "noEmit": true,
    "types": ["node"]
  },
  "include": ["src", "test"]
}
```

`agent/vitest.config.ts`:

```ts
import { defineConfig } from "vitest/config";

export default defineConfig({ test: { environment: "node", include: ["test/**/*.test.ts"] } });
```

`.gitignore` に `agent/node_modules/` と `agent/dist/` を足す。

- [ ] **Step 2: 偽の CLI と、CLI 呼び出しの失敗するテストを書く**

`agent/test/fake-infodesk.mjs` (実行権限を付ける: `chmod +x`):

```js
#!/usr/bin/env node
// テスト用の偽 infodesk。受け取った引数を JSON で返す。特定の引数では失敗や不正な出力を返す。
const argv = process.argv.slice(2);
if (argv.includes("item") && argv.includes("404")) {
  process.stderr.write("error: item 404 not found\n");
  process.exit(1);
}
if (argv.includes("bad-json")) {
  process.stdout.write("not json");
  process.exit(0);
}
if (argv.includes("sleep")) {
  setTimeout(() => process.stdout.write("{}"), 5000);
} else {
  process.stdout.write(JSON.stringify({ argv }));
}
```

`agent/test/cli.test.ts`:

```ts
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";
import { CliError, createCliRunner } from "../src/cli";

const FAKE = fileURLToPath(new URL("./fake-infodesk.mjs", import.meta.url));

describe("createCliRunner", () => {
  it("passes --db first and parses stdout as JSON", async () => {
    const run = createCliRunner([FAKE], "/tmp/x.db");
    await expect(run(["items", "--json"])).resolves.toEqual({ argv: ["--db", "/tmp/x.db", "items", "--json"] });
  });

  it("turns a non-zero exit into CliError with the CLI's message", async () => {
    const run = createCliRunner([FAKE], "/tmp/x.db");
    const err = await run(["item", "404", "--json"]).catch((e) => e);
    expect(err).toBeInstanceOf(CliError);
    expect(err.message).toBe("item 404 not found");
  });

  it("rejects output that is not JSON", async () => {
    const run = createCliRunner([FAKE], "/tmp/x.db");
    await expect(run(["bad-json"])).rejects.toThrow("infodesk CLI returned invalid JSON");
  });

  it("times out and can be aborted", async () => {
    const run = createCliRunner([FAKE], "/tmp/x.db", 200);
    await expect(run(["sleep"])).rejects.toThrow(CliError);
    const ac = new AbortController();
    const p = createCliRunner([FAKE], "/tmp/x.db")(["sleep"], ac.signal);
    ac.abort();
    await expect(p).rejects.toThrow(CliError);
  });

  it("reports a missing binary clearly", async () => {
    const run = createCliRunner(["/nonexistent/infodesk"], "/tmp/x.db");
    await expect(run(["items"])).rejects.toThrow("infodesk CLI not found: /nonexistent/infodesk");
  });
});
```

Run: `cd agent && npm test -- --run` → FAIL (`Cannot find module '../src/cli'`)

- [ ] **Step 3: `env.ts` と `cli.ts` を実装する**

`agent/src/env.ts`:

```ts
export interface AgentEnv {
  /** 実行ファイルと前置きの引数。例: ["/Applications/InfoDesk.app/Contents/MacOS/infodesk-app", "--cli"] */
  cli: string[];
  db: string;
  token: string;
  port: number;
}

function required(env: NodeJS.ProcessEnv, name: string): string {
  const v = env[name];
  if (!v) throw new Error(`missing environment variable: ${name}`);
  return v;
}

export function readEnv(env: NodeJS.ProcessEnv = process.env): AgentEnv {
  const port = Number(env.INFODESK_AGENT_PORT ?? "0");
  return {
    cli: [required(env, "INFODESK_CLI"), ...(env.INFODESK_CLI_ARG ? [env.INFODESK_CLI_ARG] : [])],
    db: required(env, "INFODESK_DB"),
    token: required(env, "INFODESK_AGENT_TOKEN"),
    port: Number.isInteger(port) && port >= 0 ? port : 0,
  };
}
```

`agent/src/cli.ts`:

```ts
import { execFile } from "node:child_process";

export class CliError extends Error {}

export type CliRunner = (args: string[], signal?: AbortSignal) => Promise<unknown>;

const MAX_BUFFER = 8 * 1024 * 1024;

export function createCliRunner(command: string[], db: string, timeoutMs = 60_000): CliRunner {
  const [bin, ...prefix] = command;
  return (args, signal) =>
    new Promise((resolve, reject) => {
      execFile(
        bin,
        [...prefix, "--db", db, ...args],
        { timeout: timeoutMs, maxBuffer: MAX_BUFFER, signal, encoding: "utf8" },
        (err, stdout, stderr) => {
          if (err) {
            const code = (err as NodeJS.ErrnoException).code;
            if (code === "ENOENT") return reject(new CliError(`infodesk CLI not found: ${bin}`));
            if (err.name === "AbortError") return reject(new CliError("infodesk CLI was aborted"));
            if ((err as { killed?: boolean }).killed) return reject(new CliError("infodesk CLI timed out"));
            const msg = stderr.trim().replace(/^error:\s*/, "");
            return reject(new CliError(msg || err.message));
          }
          try {
            resolve(JSON.parse(stdout));
          } catch {
            reject(new CliError("infodesk CLI returned invalid JSON"));
          }
        },
      );
    });
}
```

Run: `npm test -- --run test/cli.test.ts` → PASS

- [ ] **Step 4: 文脈の型を書く**

`agent/src/context.ts` (Task 7 の UI 側と同じ形。片方を変えたらもう片方も変える):

```ts
export type DigestPeriod = "day" | "week" | "month";
export type FeedRange = "today" | "3d" | "week" | "month" | "all";

export type ChatScope =
  | { kind: "all" }
  | { kind: "category"; category: string; label: string }
  | { kind: "topic"; topicId: number; label: string };

export type ChatPeriod =
  | { view: "digest"; period: DigestPeriod; date: string }
  | { view: "feed"; range: FeedRange };

export type ChatAttachment =
  | { type: "item"; id: number; title: string }
  | { type: "digest"; period: DigestPeriod; date: string; scope: string; label: string };

export interface ChatContext {
  scope: ChatScope;
  period: ChatPeriod | null;
  attachments: ChatAttachment[];
}
```

- [ ] **Step 5: ツールの失敗するテストを書く**

`agent/test/tools.test.ts`:

```ts
import { fileURLToPath } from "node:url";
import { describe, expect, it, vi } from "vitest";
import { createCliRunner } from "../src/cli";
import { createInfodeskTools, TOOL_NAMES } from "../src/tools";

const FAKE = fileURLToPath(new URL("./fake-infodesk.mjs", import.meta.url));
const run = createCliRunner([FAKE], "/tmp/x.db");
const opts = { toolCallId: "t1", messages: [] } as never;

async function exec(tools: Record<string, any>, name: string, input: unknown) {
  return tools[name].execute(input, opts);
}

describe("createInfodeskTools", () => {
  it("offers web_search only when a key is set", () => {
    expect(Object.keys(createInfodeskTools(run, {}))).toEqual([
      "search_articles", "get_article", "read_digests", "fetch_page",
    ]);
    expect(Object.keys(createInfodeskTools(run, { webSearchKey: "k" }))).toContain("web_search");
    expect(TOOL_NAMES).toEqual(["search_articles", "get_article", "read_digests", "fetch_page", "web_search"]);
  });

  it("maps search_articles input to CLI flags", async () => {
    const out = await exec(createInfodeskTools(run, {}), "search_articles", {
      query: "認知症", category: "medcare", topic: "介護", from: "2026-10-01", to: "2026-10-07", limit: 5,
    });
    expect(out).toEqual({
      argv: ["--db", "/tmp/x.db", "items", "--json", "--text", "認知症", "--category", "medcare",
        "--topic", "介護", "--from", "2026-10-01", "--to", "2026-10-07", "--limit", "5"],
    });
    const plain = await exec(createInfodeskTools(run, {}), "search_articles", {});
    expect(plain).toEqual({ argv: ["--db", "/tmp/x.db", "items", "--json", "--limit", "20"] });
  });

  it("maps the other tools to their commands", async () => {
    const tools = createInfodeskTools(run, {});
    expect(await exec(tools, "get_article", { id: 12 })).toEqual({ argv: ["--db", "/tmp/x.db", "item", "12", "--json"] });
    expect(await exec(tools, "read_digests", { period: "week", date: "2026-10-07", topic: "認知症" })).toEqual({
      argv: ["--db", "/tmp/x.db", "digests", "week", "--json", "--date", "2026-10-07", "--topic", "認知症"],
    });
    expect(await exec(tools, "fetch_page", { url: "https://example.com/a" })).toEqual({
      argv: ["--db", "/tmp/x.db", "page", "https://example.com/a", "--json", "--max-chars", "20000"],
    });
  });

  it("returns a tool failure as an error the model can read", async () => {
    const tools = createInfodeskTools(run, {});
    await expect(exec(tools, "get_article", { id: 404 })).rejects.toThrow("item 404 not found");
  });

  it("calls ollama.com web search with the key and trims results", async () => {
    const fetchImpl = vi.fn(async () =>
      new Response(JSON.stringify({ results: [
        { title: "A", url: "https://a", content: "x".repeat(2000) },
        { title: "B", url: "https://b", content: "b" },
        { url: "https://c" },
      ] }), { status: 200 }),
    );
    const tools = createInfodeskTools(run, { webSearchKey: "secret", fetchImpl: fetchImpl as never });
    const out = await exec(tools, "web_search", { query: "認知症 新薬", max_results: 2 });
    const [url, init] = fetchImpl.mock.calls[0] as unknown as [string, RequestInit];
    expect(url).toBe("https://ollama.com/api/web_search");
    expect(init.method).toBe("POST");
    expect((init.headers as Record<string, string>).Authorization).toBe("Bearer secret");
    expect(JSON.parse(String(init.body))).toEqual({ query: "認知症 新薬" });
    expect(out.results).toHaveLength(2);
    expect(out.results[0].content.length).toBe(1001);
  });

  it("reports web search HTTP errors without the key", async () => {
    const fetchImpl = vi.fn(async () => new Response("unauthorized", { status: 401 }));
    const tools = createInfodeskTools(run, { webSearchKey: "secret", fetchImpl: fetchImpl as never });
    const err = await exec(tools, "web_search", { query: "x" }).catch((e: Error) => e);
    expect(err.message).toBe("web search failed: HTTP 401");
    expect(err.message).not.toContain("secret");
  });
});
```

Run: `npm test -- --run test/tools.test.ts` → FAIL

- [ ] **Step 6: ツールを実装する**

`agent/src/tools.ts`:

```ts
import { tool, type Tool } from "ai";
import { z } from "zod";
import type { CliRunner } from "./cli";

export const TOOL_NAMES = ["search_articles", "get_article", "read_digests", "fetch_page", "web_search"] as const;

const CATEGORIES = ["github", "arxiv", "huggingface", "hackernews", "tech_jp", "tech_intl", "medcare"] as const;
const DATE = z.string().regex(/^\d{4}-\d{2}-\d{2}$/, "YYYY-MM-DD");
const WEB_SEARCH_URL = "https://ollama.com/api/web_search";
const WEB_SNIPPET_CHARS = 1000;

function flag(args: string[], name: string, value: string | number | undefined) {
  if (value !== undefined && value !== "") args.push(name, String(value));
}

export function createInfodeskTools(
  run: CliRunner,
  opts: { webSearchKey?: string; fetchImpl?: typeof fetch },
): Record<string, Tool> {
  const tools: Record<string, Tool> = {
    search_articles: tool({
      description:
        "収集済みの記事を検索する。キーワード、カテゴリー、トピック名、取得日の範囲で絞り込める。新しい順に返す。",
      inputSchema: z.object({
        query: z.string().min(1).max(200).optional().describe("タイトルと概要に含まれる語"),
        category: z.enum(CATEGORIES).optional(),
        topic: z.string().min(1).optional().describe("トピック名 (例: 認知症)"),
        days: z.number().int().min(1).max(365).optional().describe("直近N日に取得した記事"),
        from: DATE.optional().describe("この日以降に取得した記事"),
        to: DATE.optional().describe("この日までに取得した記事"),
        limit: z.number().int().min(1).max(50).optional(),
      }),
      execute: async (input, { abortSignal }) => {
        const args = ["items", "--json"];
        flag(args, "--text", input.query);
        flag(args, "--category", input.category);
        flag(args, "--topic", input.topic);
        flag(args, "--days", input.days);
        flag(args, "--from", input.from);
        flag(args, "--to", input.to);
        flag(args, "--limit", input.limit ?? 20);
        return run(args, abortSignal);
      },
    }),
    get_article: tool({
      description: "収集済みの記事1件の詳細 (概要、記事要約、URL) を読む。",
      inputSchema: z.object({ id: z.number().int().positive() }),
      execute: async ({ id }, { abortSignal }) => run(["item", String(id), "--json"], abortSignal),
    }),
    read_digests: tool({
      description:
        "生成済みの期間要約 (日次、週次、月次) を読む。生成はしない。category か topic を省くと全カテゴリー。",
      inputSchema: z.object({
        period: z.enum(["day", "week", "month"]),
        date: DATE.optional().describe("期間に含まれる日。省くと今日"),
        category: z.enum(CATEGORIES).optional(),
        topic: z.string().min(1).optional(),
      }),
      execute: async (input, { abortSignal }) => {
        const args = ["digests", input.period, "--json"];
        flag(args, "--date", input.date);
        flag(args, "--category", input.category);
        flag(args, "--topic", input.topic);
        return run(args, abortSignal);
      },
    }),
    fetch_page: tool({
      description: "記事の URL を取得して本文のテキストを返す。長い本文は切り詰める。保存はしない。",
      inputSchema: z.object({
        url: z.string().url(),
        max_chars: z.number().int().min(1000).max(20000).optional(),
      }),
      execute: async ({ url, max_chars }, { abortSignal }) =>
        run(["page", url, "--json", "--max-chars", String(max_chars ?? 20000)], abortSignal),
    }),
  };
  if (opts.webSearchKey) {
    const key = opts.webSearchKey;
    const doFetch = opts.fetchImpl ?? fetch;
    tools.web_search = tool({
      description: "収集した記事の外を Web で検索する。収集済みの記事で足りないときだけ使う。",
      inputSchema: z.object({
        query: z.string().min(1).max(500),
        max_results: z.number().int().min(1).max(10).optional(),
      }),
      execute: async ({ query, max_results }, { abortSignal }) => {
        const res = await doFetch(WEB_SEARCH_URL, {
          method: "POST",
          headers: { "Content-Type": "application/json", Authorization: `Bearer ${key}` },
          body: JSON.stringify({ query }),
          signal: abortSignal,
        });
        if (!res.ok) throw new Error(`web search failed: HTTP ${res.status}`);
        const data = (await res.json()) as { results?: Array<Record<string, unknown>> };
        const results = (data.results ?? []).slice(0, max_results ?? 5).map((r) => {
          const content = String(r.content ?? r.snippet ?? "");
          return {
            title: String(r.title ?? ""),
            url: String(r.url ?? ""),
            content: content.length > WEB_SNIPPET_CHARS ? `${content.slice(0, WEB_SNIPPET_CHARS)}…` : content,
          };
        });
        return { query, results };
      },
    });
  }
  return tools;
}
```

`tool()` の `execute` の第2引数の名前 (`abortSignal`) は `node_modules/ai/dist/index.d.ts` の `ToolExecutionOptions` で確かめる。

Run: `npm test -- --run` → PASS。`npm run typecheck` → エラーなし。

- [ ] **Step 7: コミット**

```bash
cd agent && npm test -- --run && npm run typecheck && cd ..
git add agent/package.json agent/package-lock.json agent/tsconfig.json agent/vitest.config.ts agent/src agent/test .gitignore
git commit -m "feat(agent): add the chat sidecar package with CLI-backed tools"
```

---

### Task 5: サイドカーの会話と HTTP サーバー

**Files:**
- Create: `agent/src/prompt.ts`、`agent/src/models.ts`、`agent/src/errors.ts`、`agent/src/chat.ts`、`agent/src/server.ts`、`agent/src/main.ts`、`agent/build.mjs`、`agent/test/prompt.test.ts`、`agent/test/errors.test.ts`、`agent/test/server.test.ts`
- Test: 同上

**Interfaces:**
- Consumes: Task 4 の `createCliRunner`、`createInfodeskTools`、`TOOL_NAMES`、`ChatContext`
- Produces:
  - `prompt.ts`: `buildInstructions(context: ChatContext | null, now: Date, tools: string[]): string`
  - `errors.ts`: `interface ChatErrorInfo { kind: "llm_unreachable" | "model_missing" | "auth" | "tool" | "other"; provider: "ollama" | "claude"; message: string }`、`describeError(error: unknown, provider, settings): string` (`JSON.stringify(ChatErrorInfo)` を返す。UI はこれを `errorText` として受け取る)
  - `models.ts`: `type ProviderId = "ollama" | "claude"`、`type Settings = Record<string, string>`、`interface ModelFactory { ollama(s: Settings): LanguageModel; claude(s: Settings, o: { instructions: string; mcpServers: Record<string, unknown>; allowedTools: string[] }): LanguageModel }`、`defaultModels: ModelFactory`、`CLAUDE_CHAT_MODEL = "sonnet"`
  - `chat.ts`: `interface ChatRequest { messages: UIMessage[]; provider: ProviderId; context: ChatContext | null }`、`interface ChatDeps { run: CliRunner; loadSettings(): Promise<Settings>; models: ModelFactory; now(): Date }`、`streamChat(req: ChatRequest, res: ServerResponse, deps: ChatDeps, signal: AbortSignal): Promise<void>`
  - `server.ts`: `createAgentServer(opts: { token: string; allowedOrigins: string[]; chat: (req: ChatRequest, res: ServerResponse, signal: AbortSignal) => Promise<void> }): http.Server`。経路: `OPTIONS *`、`GET /health` → `{"ok":true}`、`POST /chat`。
  - `main.ts`: 起動して待ち受けたら標準出力に1行 `INFODESK_AGENT_READY {"port":<n>}` を書く。`--check` を付けると `ok` を出して終了する (バンドルの読み込み確認用)。標準入力が閉じたら終了する (アプリが落ちたときに残らないため)。
  - `build.mjs`: `dist/agent.mjs` (ESM、Node 22 向け、1ファイル)

- [ ] **Step 1: 指示文の失敗するテストを書く**

`agent/test/prompt.test.ts`:

```ts
import { describe, expect, it } from "vitest";
import { buildInstructions } from "../src/prompt";

const NOW = new Date("2026-10-09T10:00:00+09:00");
const ALL = ["search_articles", "get_article", "read_digests", "fetch_page"];

describe("buildInstructions", () => {
  it("states the role, the local date and the tool rules", () => {
    const s = buildInstructions(null, NOW, ALL);
    expect(s).toContain("日本語");
    expect(s).toContain("今日: 2026-10-09 (金)");
    expect(s).toContain("search_articles");
    expect(s).toContain("推測で");
    expect(s).not.toContain("web_search");
  });

  it("mentions web_search only when the tool is available", () => {
    expect(buildInstructions(null, NOW, [...ALL, "web_search"])).toContain("web_search");
  });

  it("describes the scope, the period and the attachments", () => {
    const s = buildInstructions(
      {
        scope: { kind: "topic", topicId: 3, label: "認知症" },
        period: { view: "digest", period: "week", date: "2026-10-09" },
        attachments: [
          { type: "item", id: 12, title: "認知症の新薬、国内での適用拡大へ" },
          { type: "digest", period: "day", date: "2026-10-09", scope: "category:medcare", label: "医療介護" },
        ],
      },
      NOW,
      ALL,
    );
    expect(s).toContain("トピック「認知症」");
    expect(s).toContain("週次 (2026-10-09 を含む週)");
    expect(s).toContain("記事 id 12「認知症の新薬、国内での適用拡大へ」");
    expect(s).toContain("医療介護の日次要約 (2026-10-09)");
  });

  it("describes a feed range and the whole collection", () => {
    const s = buildInstructions({ scope: { kind: "all" }, period: { view: "feed", range: "3d" }, attachments: [] }, NOW, ALL);
    expect(s).toContain("すべての記事");
    expect(s).toContain("直近3日");
  });
});
```

Run: `npm test -- --run test/prompt.test.ts` → FAIL

- [ ] **Step 2: `prompt.ts` を実装する**

```ts
import type { ChatContext, ChatPeriod, ChatScope, FeedRange } from "./context";

const WEEKDAYS = ["日", "月", "火", "水", "木", "金", "土"];
const FEED_LABEL: Record<FeedRange, string> = { today: "今日", "3d": "直近3日", week: "直近1週間", month: "直近1ヶ月", all: "全期間" };
const PERIOD_LABEL = { day: "日次", week: "週次", month: "月次" } as const;

function localDate(d: Date): string {
  const p = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}`;
}

function scopeText(s: ChatScope): string {
  switch (s.kind) {
    case "all": return "すべての記事";
    case "category": return `カテゴリー「${s.label}」 (${s.category})`;
    case "topic": return `トピック「${s.label}」`;
  }
}

function periodText(p: ChatPeriod): string {
  if (p.view === "feed") return `フィードの期間: ${FEED_LABEL[p.range]}`;
  const unit = p.period === "day" ? `${p.date}` : p.period === "week" ? `${p.date} を含む週` : `${p.date.slice(0, 7)} の月`;
  return `ダイジェストの期間: ${PERIOD_LABEL[p.period]} (${unit})`;
}

export function buildInstructions(context: ChatContext | null, now: Date, tools: string[]): string {
  const lines = [
    "あなたは InfoDesk のリサーチアシスタントです。利用者が収集した技術ニュースと医療介護ニュースについて、日本語で答えます。",
    `今日: ${localDate(now)} (${WEEKDAYS[now.getDay()]})`,
    "",
    "ツールの使い方:",
    "- 収集済みの記事は search_articles で探し、get_article で1件を詳しく読む。",
    "- 生成済みの期間要約は read_digests で読む。",
    "- 記事の詳しい中身が必要なときだけ fetch_page で本文を取る。",
  ];
  if (tools.includes("web_search")) {
    lines.push("- 収集した記事の外の情報が必要なときだけ Web 検索 (web_search または WebSearch) を使う。");
  }
  lines.push(
    "",
    "答え方:",
    "- 推測で記事や数字を作らない。ツールで確かめた内容だけを書き、分からないことは分からないと書く。",
    "- 記事に触れるときはタイトルを書く。URL は書かなくてよい (画面が参照した記事のリンクを出す)。",
    "- 見出しと箇条書きで短くまとめる。",
  );
  if (context) {
    lines.push("", "利用者が今見ている画面:", `- ${scopeText(context.scope)}`);
    if (context.period) lines.push(`- ${periodText(context.period)}`);
    for (const a of context.attachments) {
      if (a.type === "item") lines.push(`- 質問に添えた記事 id ${a.id}「${a.title}」 (get_article で読める)`);
      else lines.push(`- 質問に添えた要約: ${a.label}の${PERIOD_LABEL[a.period]}要約 (${a.date})`);
    }
  }
  return lines.join("\n");
}
```

`now` はサイドカーのローカル時刻 (利用者の Mac の時刻) で日付を出す。テストの `NOW` は JST で書いてあるので、テストは `TZ=Asia/Tokyo` で動かす。`agent/package.json` の `scripts.test` を `TZ=Asia/Tokyo vitest` にする。

Run: `npm test -- --run test/prompt.test.ts` → PASS

- [ ] **Step 3: エラー変換の失敗するテストを書く**

`agent/test/errors.test.ts`:

```ts
import { describe, expect, it } from "vitest";
import { describeError } from "../src/errors";

const S = { "ollama.base_url": "http://localhost:11434", "ollama.model": "gemma4:26b" };
const parse = (s: string) => JSON.parse(s);

describe("describeError", () => {
  it("recognizes an unreachable Ollama", () => {
    const e = Object.assign(new TypeError("fetch failed"), { cause: { code: "ECONNREFUSED" } });
    expect(parse(describeError(e, "ollama", S))).toEqual({
      kind: "llm_unreachable", provider: "ollama",
      message: "http://localhost:11434 が応答しません。Ollama が起動しているか確認してください。",
    });
  });

  it("recognizes a missing Ollama model", () => {
    const e = new Error("model 'gemma4:26b' not found");
    expect(parse(describeError(e, "ollama", S))).toMatchObject({ kind: "model_missing", message: expect.stringContaining("gemma4:26b") });
  });

  it("recognizes a Claude login problem", () => {
    const e = new Error("Authentication failed: please run claude login");
    expect(parse(describeError(e, "claude", S))).toMatchObject({ kind: "auth", provider: "claude" });
  });

  it("passes other errors through", () => {
    expect(parse(describeError(new Error("boom"), "claude", S))).toEqual({ kind: "other", provider: "claude", message: "boom" });
    expect(parse(describeError("text", "ollama", S))).toEqual({ kind: "other", provider: "ollama", message: "text" });
  });
});
```

Run → FAIL

- [ ] **Step 4: `errors.ts` を実装する**

```ts
import { isAuthenticationError } from "ai-sdk-provider-claude-code";
import type { ProviderId, Settings } from "./models";

export interface ChatErrorInfo {
  kind: "llm_unreachable" | "model_missing" | "auth" | "tool" | "other";
  provider: ProviderId;
  message: string;
}

function causeCode(e: unknown): string | undefined {
  let cur: unknown = e;
  for (let i = 0; i < 4 && cur && typeof cur === "object"; i++) {
    const code = (cur as { code?: unknown }).code;
    if (typeof code === "string") return code;
    cur = (cur as { cause?: unknown }).cause;
  }
  return undefined;
}

export function describeError(error: unknown, provider: ProviderId, settings: Settings): string {
  const message = error instanceof Error ? error.message : String(error);
  const info = ((): ChatErrorInfo => {
    if (provider === "ollama") {
      const code = causeCode(error);
      if (code === "ECONNREFUSED" || code === "ENOTFOUND" || /fetch failed|ECONNREFUSED/.test(message)) {
        const url = settings["ollama.base_url"] ?? "http://localhost:11434";
        return { kind: "llm_unreachable", provider, message: `${url} が応答しません。Ollama が起動しているか確認してください。` };
      }
      if (/model .* not found|not found, try pulling/i.test(message)) {
        const model = settings["ollama.model"] ?? "";
        return { kind: "model_missing", provider, message: `モデル ${model} が Ollama にありません。ollama pull ${model} を実行するか、設定でモデル名を直してください。` };
      }
    }
    if (provider === "claude" && (isAuthenticationError(error) || /authentication|log ?in|401/i.test(message))) {
      return { kind: "auth", provider, message: "Claude にログインしていません。ターミナルで claude を実行してログインしてください。" };
    }
    return { kind: "other", provider, message };
  })();
  return JSON.stringify(info);
}
```

`isAuthenticationError` は `ai-sdk-provider-claude-code` の公開関数 (型定義の `export {...}` で確認済み)。Run → PASS。

- [ ] **Step 5: モデルの作成**

`agent/src/models.ts`:

```ts
import type { LanguageModel } from "ai";
import { createOllama } from "ai-sdk-ollama";
import { claudeCode } from "ai-sdk-provider-claude-code";

export type ProviderId = "ollama" | "claude";
export type Settings = Record<string, string>;

/** チャットで使う Claude のモデル。InfoDesk の他の設定と同じく CLI 経由で使う。 */
export const CLAUDE_CHAT_MODEL = "sonnet";

export interface ClaudeModelOptions {
  instructions: string;
  mcpServers: Record<string, unknown>;
  allowedTools: string[];
}

export interface ModelFactory {
  ollama(settings: Settings): LanguageModel;
  claude(settings: Settings, opts: ClaudeModelOptions): LanguageModel;
}

export const defaultModels: ModelFactory = {
  ollama: (s) =>
    createOllama({ baseURL: s["ollama.base_url"] || "http://localhost:11434" })(s["ollama.model"] || "gemma4:26b"),
  claude: (s, o) =>
    claudeCode(CLAUDE_CHAT_MODEL, {
      pathToClaudeCodeExecutable: s["claude.bin"] || "claude",
      systemPrompt: o.instructions,
      mcpServers: o.mcpServers as never,
      tools: ["WebSearch"],
      allowedTools: o.allowedTools,
      persistSession: false,
      maxTurns: 12,
    }),
};
```

`tools: ["WebSearch"]` は、Claude Code の組み込みツールのうち WebSearch だけを使えるようにする設定 (`ClaudeCodeSettings.tools` は Agent SDK の `Options['tools']`)。型が合わない場合は `node_modules/@anthropic-ai/claude-agent-sdk/sdk.d.ts` の `tools` の型を読んで合わせ、報告に書く。`settingSources` は付けない (付けないとファイルの設定を読まない。型定義のコメントで確認済み)。

- [ ] **Step 6: HTTP サーバーと会話の失敗するテストを書く**

`agent/test/server.test.ts`。サーバーは `chat` 関数を差し替えられるので、認証と CORS は単体で、会話は `streamChat` にモックのモデルを渡して確かめる。

```ts
import type { AddressInfo } from "node:net";
import { fileURLToPath } from "node:url";
import { afterEach, describe, expect, it, vi } from "vitest";
import { simulateReadableStream } from "ai";
import { MockLanguageModelV4 } from "ai/test";
import { createCliRunner } from "../src/cli";
import { streamChat, type ChatDeps } from "../src/chat";
import { createAgentServer } from "../src/server";

const FAKE = fileURLToPath(new URL("./fake-infodesk.mjs", import.meta.url));
const TOKEN = "t0ken";
const ORIGIN = "tauri://localhost";
const usage = {
  inputTokens: { total: 1, noCache: 1, cacheRead: undefined, cacheWrite: undefined },
  outputTokens: { total: 1, text: 1, reasoning: undefined },
};

let server: ReturnType<typeof createAgentServer> | null = null;
afterEach(() => { server?.close(); server = null; });

async function start(chat = vi.fn(async (_req, res) => { res.end("ok"); })) {
  server = createAgentServer({ token: TOKEN, allowedOrigins: [ORIGIN], chat });
  await new Promise<void>((r) => server!.listen(0, "127.0.0.1", r));
  const { port } = server.address() as AddressInfo;
  return { base: `http://127.0.0.1:${port}`, chat };
}

const userMessage = { id: "u1", role: "user", parts: [{ type: "text", text: "今週の認知症の動きは?" }] };
const body = (extra: object = {}) => JSON.stringify({ messages: [userMessage], provider: "ollama", context: null, ...extra });

function sseChunks(text: string): any[] {
  return text.split("\n").filter((l) => l.startsWith("data: ") && l !== "data: [DONE]").map((l) => JSON.parse(l.slice(6)));
}

describe("createAgentServer", () => {
  it("requires the bearer token", async () => {
    const { base, chat } = await start();
    expect((await fetch(`${base}/health`)).status).toBe(401);
    expect((await fetch(`${base}/chat`, { method: "POST", body: body(), headers: { Authorization: "Bearer wrong" } })).status).toBe(401);
    expect(chat).not.toHaveBeenCalled();
    const ok = await fetch(`${base}/health`, { headers: { Authorization: `Bearer ${TOKEN}` } });
    expect(await ok.json()).toEqual({ ok: true });
  });

  it("answers CORS preflight only for allowed origins", async () => {
    const { base } = await start();
    const pre = await fetch(`${base}/chat`, { method: "OPTIONS", headers: { Origin: ORIGIN, "Access-Control-Request-Method": "POST" } });
    expect(pre.status).toBe(204);
    expect(pre.headers.get("access-control-allow-origin")).toBe(ORIGIN);
    expect(pre.headers.get("access-control-allow-headers")).toContain("authorization");
    const bad = await fetch(`${base}/chat`, { method: "OPTIONS", headers: { Origin: "https://evil.example" } });
    expect(bad.status).toBe(403);
  });

  it("validates the request body", async () => {
    const { base, chat } = await start();
    const h = { Authorization: `Bearer ${TOKEN}`, "Content-Type": "application/json" };
    expect((await fetch(`${base}/chat`, { method: "POST", headers: h, body: "{" })).status).toBe(400);
    expect((await fetch(`${base}/chat`, { method: "POST", headers: h, body: body({ provider: "gpt" }) })).status).toBe(400);
    expect((await fetch(`${base}/chat`, { method: "POST", headers: h, body: JSON.stringify({ provider: "ollama" }) })).status).toBe(400);
    expect((await fetch(`${base}/nope`, { headers: h })).status).toBe(404);
    expect(chat).not.toHaveBeenCalled();
  });
});

describe("streamChat (Ollama)", () => {
  function deps(model: MockLanguageModelV4): ChatDeps {
    return {
      run: createCliRunner([FAKE], "/tmp/x.db"),
      loadSettings: async () => ({ "ollama.base_url": "http://localhost:11434", "ollama.model": "gemma4:26b" }),
      models: { ollama: () => model, claude: () => { throw new Error("not used"); } },
      now: () => new Date("2026-10-09T10:00:00+09:00"),
    };
  }

  it("runs the tool loop and streams tool parts then text", async () => {
    let call = 0;
    const model = new MockLanguageModelV4({
      doStream: async () => {
        call += 1;
        const chunks = call === 1
          ? [
              { type: "tool-call", toolCallId: "c1", toolName: "search_articles", input: JSON.stringify({ query: "認知症" }) },
              { type: "finish", finishReason: { unified: "tool-calls", raw: undefined }, usage },
            ]
          : [
              { type: "text-start", id: "t" },
              { type: "text-delta", id: "t", delta: "新薬の話題が中心です。" },
              { type: "text-end", id: "t" },
              { type: "finish", finishReason: { unified: "stop", raw: undefined }, usage },
            ];
        return { stream: simulateReadableStream({ chunks: chunks as never }) };
      },
    });
    const d = deps(model);
    const { base } = await start((req, res, signal) => streamChat(req, res, d, signal));
    const res = await fetch(`${base}/chat`, { method: "POST", headers: { Authorization: `Bearer ${TOKEN}`, Origin: ORIGIN }, body: body() });
    expect(res.status).toBe(200);
    expect(res.headers.get("access-control-allow-origin")).toBe(ORIGIN);
    const chunks = sseChunks(await res.text());
    const types = chunks.map((c) => c.type);
    expect(types).toContain("tool-input-available");
    const out = chunks.find((c) => c.type === "tool-output-available");
    expect(out.output.argv).toContain("--text");
    expect(chunks.filter((c) => c.type === "text-delta").map((c) => c.delta).join("")).toBe("新薬の話題が中心です。");
    expect(types.at(-1)).toBe("finish");
    // 指示文が渡っている
    const firstPrompt = (model.doStreamCalls[0] as any).prompt;
    expect(JSON.stringify(firstPrompt)).toContain("InfoDesk");
  });

  it("streams an LLM failure as a structured error chunk", async () => {
    const model = new MockLanguageModelV4({
      doStream: async () => { throw Object.assign(new TypeError("fetch failed"), { cause: { code: "ECONNREFUSED" } }); },
    });
    const d = deps(model);
    const { base } = await start((req, res, signal) => streamChat(req, res, d, signal));
    const res = await fetch(`${base}/chat`, { method: "POST", headers: { Authorization: `Bearer ${TOKEN}` }, body: body() });
    const err = sseChunks(await res.text()).find((c) => c.type === "error");
    expect(JSON.parse(err.errorText)).toMatchObject({ kind: "llm_unreachable", provider: "ollama" });
  });

  it("returns a failing tool to the model instead of crashing", async () => {
    let call = 0;
    const model = new MockLanguageModelV4({
      doStream: async () => {
        call += 1;
        const chunks = call === 1
          ? [
              { type: "tool-call", toolCallId: "c1", toolName: "get_article", input: JSON.stringify({ id: 404 }) },
              { type: "finish", finishReason: { unified: "tool-calls", raw: undefined }, usage },
            ]
          : [
              { type: "text-start", id: "t" }, { type: "text-delta", id: "t", delta: "見つかりませんでした。" }, { type: "text-end", id: "t" },
              { type: "finish", finishReason: { unified: "stop", raw: undefined }, usage },
            ];
        return { stream: simulateReadableStream({ chunks: chunks as never }) };
      },
    });
    const d = deps(model);
    const { base } = await start((req, res, signal) => streamChat(req, res, d, signal));
    const res = await fetch(`${base}/chat`, { method: "POST", headers: { Authorization: `Bearer ${TOKEN}` }, body: body() });
    const chunks = sseChunks(await res.text());
    const toolErr = chunks.find((c) => c.type === "tool-output-error");
    expect(toolErr.errorText).toContain("item 404 not found");
    expect(chunks.some((c) => c.type === "text-delta")).toBe(true);
  });
});

describe("streamChat (Claude)", () => {
  it("bridges the tools over MCP and allows only them plus WebSearch", async () => {
    const claude = vi.fn(() => new MockLanguageModelV4({
      doStream: async () => ({
        stream: simulateReadableStream({ chunks: [
          { type: "text-start", id: "t" }, { type: "text-delta", id: "t", delta: "はい" }, { type: "text-end", id: "t" },
          { type: "finish", finishReason: { unified: "stop", raw: undefined }, usage },
        ] as never }),
      }),
    }));
    const d: ChatDeps = {
      run: createCliRunner([FAKE], "/tmp/x.db"),
      loadSettings: async () => ({ "claude.bin": "/usr/local/bin/claude", "ollama.web_search_api_key": "k" }),
      models: { ollama: () => { throw new Error("not used"); }, claude },
      now: () => new Date("2026-10-09T10:00:00+09:00"),
    };
    const { base } = await start((req, res, signal) => streamChat(req, res, d, signal));
    const res = await fetch(`${base}/chat`, { method: "POST", headers: { Authorization: `Bearer ${TOKEN}` }, body: body({ provider: "claude" }) });
    expect(sseChunks(await res.text()).some((c) => c.type === "text-delta")).toBe(true);
    const [settings, opts] = claude.mock.calls[0] as any;
    expect(settings["claude.bin"]).toBe("/usr/local/bin/claude");
    expect(Object.keys(opts.mcpServers)).toEqual(["infodesk"]);
    expect(opts.allowedTools).toEqual([
      "mcp__infodesk__search_articles", "mcp__infodesk__get_article", "mcp__infodesk__read_digests",
      "mcp__infodesk__fetch_page", "WebSearch",
    ]);
    expect(opts.instructions).toContain("InfoDesk");
  });
});
```

`MockLanguageModelV4` の `doStreamCalls` の名前は `node_modules/ai/dist/test/index.d.ts` で確かめる。違えば同じ意味のプロパティを使う。

Run: `npm test -- --run test/server.test.ts` → FAIL

- [ ] **Step 7: `chat.ts` と `server.ts` を実装する**

`agent/src/chat.ts`:

```ts
import type { ServerResponse } from "node:http";
import { convertToModelMessages, isStepCount, streamText, ToolLoopAgent, pipeAgentUIStreamToResponse, type UIMessage } from "ai";
import { createAiSdkMcpServer } from "ai-sdk-provider-claude-code";
import type { CliRunner } from "./cli";
import type { ChatContext } from "./context";
import { describeError } from "./errors";
import type { ModelFactory, ProviderId, Settings } from "./models";
import { buildInstructions } from "./prompt";
import { createInfodeskTools } from "./tools";

export interface ChatRequest {
  messages: UIMessage[];
  provider: ProviderId;
  context: ChatContext | null;
}

export interface ChatDeps {
  run: CliRunner;
  loadSettings(): Promise<Settings>;
  models: ModelFactory;
  now(): Date;
}

const MAX_STEPS = 12;

export async function streamChat(req: ChatRequest, res: ServerResponse, deps: ChatDeps, signal: AbortSignal): Promise<void> {
  const settings = await deps.loadSettings();
  const onError = (e: unknown) => describeError(e, req.provider, settings);
  if (req.provider === "ollama") {
    const tools = createInfodeskTools(deps.run, { webSearchKey: settings["ollama.web_search_api_key"] || undefined });
    const agent = new ToolLoopAgent({
      model: deps.models.ollama(settings),
      instructions: buildInstructions(req.context, deps.now(), Object.keys(tools)),
      tools,
      stopWhen: isStepCount(MAX_STEPS),
    });
    await pipeAgentUIStreamToResponse({ response: res, agent, uiMessages: req.messages, abortSignal: signal, onError });
    return;
  }
  // Claude: Claude Code CLI が自分でツールを実行するので、ツールは MCP で渡す。Web検索は組み込みの WebSearch。
  const tools = createInfodeskTools(deps.run, {});
  const names = Object.keys(tools);
  const instructions = buildInstructions(req.context, deps.now(), [...names, "web_search"]);
  const model = deps.models.claude(settings, {
    instructions,
    mcpServers: { infodesk: createAiSdkMcpServer("infodesk", tools as never) },
    allowedTools: [...names.map((n) => `mcp__infodesk__${n}`), "WebSearch"],
  });
  const result = streamText({
    model,
    messages: await convertToModelMessages(req.messages, { ignoreIncompleteToolCalls: true }),
    abortSignal: signal,
  });
  await result.pipeUIMessageStreamToResponse(res, { onError });
}
```

Claude 側の指示文で Web 検索の行が出るのは、組み込みの WebSearch があるため (`prompt.ts` の文言は両方の名前を書いてある)。

`agent/src/server.ts`:

```ts
import http, { type IncomingMessage, type ServerResponse } from "node:http";
import type { ChatRequest } from "./chat";

const MAX_BODY = 5 * 1024 * 1024;
const PROVIDERS = new Set(["ollama", "claude"]);

export interface AgentServerOptions {
  token: string;
  allowedOrigins: string[];
  chat: (req: ChatRequest, res: ServerResponse, signal: AbortSignal) => Promise<void>;
}

function send(res: ServerResponse, status: number, body?: unknown) {
  res.statusCode = status;
  if (body === undefined) return res.end();
  res.setHeader("Content-Type", "application/json");
  res.end(JSON.stringify(body));
}

async function readBody(req: IncomingMessage): Promise<string | null> {
  let size = 0;
  const chunks: Buffer[] = [];
  for await (const c of req) {
    size += (c as Buffer).length;
    if (size > MAX_BODY) return null;
    chunks.push(c as Buffer);
  }
  return Buffer.concat(chunks).toString("utf8");
}

function parseChat(raw: string): ChatRequest | null {
  try {
    const v = JSON.parse(raw);
    if (!v || !Array.isArray(v.messages) || !PROVIDERS.has(v.provider)) return null;
    return { messages: v.messages, provider: v.provider, context: v.context ?? null };
  } catch {
    return null;
  }
}

export function createAgentServer(opts: AgentServerOptions): http.Server {
  return http.createServer(async (req, res) => {
    const origin = req.headers.origin;
    if (origin) {
      if (!opts.allowedOrigins.includes(origin)) return send(res, 403);
      res.setHeader("Access-Control-Allow-Origin", origin);
      res.setHeader("Vary", "Origin");
    }
    if (req.method === "OPTIONS") {
      res.setHeader("Access-Control-Allow-Methods", "GET, POST, OPTIONS");
      res.setHeader("Access-Control-Allow-Headers", "authorization, content-type");
      res.setHeader("Access-Control-Max-Age", "600");
      return send(res, 204);
    }
    if (req.headers.authorization !== `Bearer ${opts.token}`) return send(res, 401, { error: "unauthorized" });
    const url = new URL(req.url ?? "/", "http://127.0.0.1");
    if (req.method === "GET" && url.pathname === "/health") return send(res, 200, { ok: true });
    if (req.method !== "POST" || url.pathname !== "/chat") return send(res, 404, { error: "not found" });
    const raw = await readBody(req);
    if (raw === null) return send(res, 413, { error: "request too large" });
    const chat = parseChat(raw);
    if (!chat) return send(res, 400, { error: "invalid chat request" });
    const ac = new AbortController();
    res.on("close", () => { if (!res.writableFinished) ac.abort(); });
    try {
      await opts.chat(chat, res, ac.signal);
    } catch (e) {
      if (!res.headersSent) send(res, 500, { error: e instanceof Error ? e.message : String(e) });
      else res.end();
    }
  });
}
```

Run: `npm test -- --run` → PASS

- [ ] **Step 8: 起動処理とバンドル**

`agent/src/main.ts`:

```ts
import { createCliRunner } from "./cli";
import { streamChat } from "./chat";
import { readEnv } from "./env";
import { defaultModels, type Settings } from "./models";
import { createAgentServer } from "./server";

const ORIGINS = ["tauri://localhost", "http://localhost:1420"];

async function main() {
  if (process.argv.includes("--check")) {
    process.stdout.write("ok\n");
    return;
  }
  const env = readEnv();
  const run = createCliRunner(env.cli, env.db);
  const loadSettings = async (): Promise<Settings> => ((await run(["config", "list", "--json"])) as { settings: Settings }).settings;
  const server = createAgentServer({
    token: env.token,
    allowedOrigins: ORIGINS,
    chat: (req, res, signal) => streamChat(req, res, { run, loadSettings, models: defaultModels, now: () => new Date() }, signal),
  });
  server.listen(env.port, "127.0.0.1", () => {
    const address = server.address();
    const port = typeof address === "object" && address ? address.port : env.port;
    process.stdout.write(`INFODESK_AGENT_READY ${JSON.stringify({ port })}\n`);
  });
  // アプリが終了または異常終了したら標準入力が閉じる。そのとき一緒に終わる。
  process.stdin.on("end", () => process.exit(0));
  process.stdin.resume();
  process.on("SIGTERM", () => server.close(() => process.exit(0)));
}

main().catch((e) => {
  process.stderr.write(`${e instanceof Error ? e.message : String(e)}\n`);
  process.exit(1);
});
```

`agent/build.mjs`:

```js
import { build } from "esbuild";

await build({
  entryPoints: ["src/main.ts"],
  outfile: "dist/agent.mjs",
  bundle: true,
  platform: "node",
  target: "node22",
  format: "esm",
  // ESM の出力で、依存が使う require を使えるようにする。
  banner: { js: "import { createRequire as __cr } from 'node:module'; const require = __cr(import.meta.url);" },
  // Agent SDK が CPU ごとに入れる claude 実行ファイルは使わない (設定 claude.bin の CLI を使う)。
  external: ["@anthropic-ai/claude-agent-sdk-*"],
  logLevel: "info",
});
```

Run:

```bash
cd agent && npm run build && node dist/agent.mjs --check && ls -lh dist/agent.mjs
INFODESK_CLI=$PWD/test/fake-infodesk.mjs INFODESK_DB=/tmp/x.db INFODESK_AGENT_TOKEN=t node dist/agent.mjs < /dev/null
```

Expected: `ok`。2つ目は `INFODESK_AGENT_READY {"port":<数>}` を出したあと、標準入力が閉じているのですぐ終了する。`build` で解決できない依存の警告やエラーが出たら、その名前と対処 (`external` への追加など) を報告に書く。バンドルが `import.meta.url` からの相対パスでファイルを読む依存を含む場合も報告する (Task 9 の手動確認で Claude の会話を確かめる)。

- [ ] **Step 9: コミット**

```bash
cd agent && npm test -- --run && npm run build && cd ..
git add agent
git commit -m "feat(agent): stream chat answers over HTTP with Ollama and Claude"
```

---
### Task 6: アプリからサイドカーを起動する

**Files:**
- Create: `crates/cli/src/lib.rs`、`app/src-tauri/src/sidecar.rs`、`app/src-tauri/src/commands/chat_endpoint.rs`
- Modify: `crates/cli/Cargo.toml`、`crates/cli/src/main.rs`、`app/src-tauri/Cargo.toml`、`app/src-tauri/src/main.rs`、`app/src-tauri/src/lib.rs`、`app/src-tauri/src/state.rs`、`app/src-tauri/src/commands/mod.rs`、`app/src-tauri/tauri.conf.json`
- Test: `crates/cli/src/lib.rs`、`app/src-tauri/src/sidecar.rs`

**Interfaces:**
- Consumes: Task 5 の起動の契約 (環境変数 `INFODESK_CLI`、`INFODESK_CLI_ARG`、`INFODESK_DB`、`INFODESK_AGENT_TOKEN`、準備完了行 `INFODESK_AGENT_READY {"port":<n>}`、標準入力が閉じたら終了)
- Produces:
  - `infodesk_cli::run_from<I: IntoIterator<Item = String>>(args: I) -> i32` (先頭は argv0。終了コードを返す)
  - アプリの実行ファイル: `<exe> --cli <infodesk の引数>` は CLI として動いて終了する
  - `sidecar.rs`:
    - `#[serde(tag = "status", rename_all = "snake_case")] enum ChatEndpoint { Starting, Ready { url: String, token: String }, Error { kind: String, message: String } }`
    - `kind` は `node_missing` | `node_too_old` | `agent_missing` | `start_failed`
    - `struct LaunchConfig { node: String, agent: PathBuf, cli: PathBuf, db: PathBuf, token: String, extra_path: Vec<PathBuf> }`
    - `parse_node_major(out: &str) -> Option<u32>`、`parse_ready_line(line: &str) -> Option<u16>`、`resolve_bin(name: &str, path_env: &str, extra: &[PathBuf]) -> Option<PathBuf>`、`new_token() -> String` (32文字の16進)
    - `async fn launch(cfg: &LaunchConfig, ready_timeout: Duration) -> Result<(tokio::process::Child, u16), LaunchError>`、`struct LaunchError { kind: &'static str, message: String }`
    - `struct Sidecar` (`AppState.chat`)。`endpoint() -> ChatEndpoint`、`async fn restart(&self, app: &AppHandle)`、`fn stop(&self)`
  - `state.rs`: `pub const EVENT_CHAT: &str = "chat-endpoint-changed";`
  - Tauri コマンド: `chat_endpoint() -> ChatEndpoint`、`restart_chat()` (再起動を始めて、すぐ戻る。結果は `chat-endpoint-changed` で知らせる)
  - `tauri.conf.json` の CSP: `connect-src ipc: http://ipc.localhost http://127.0.0.1:*`

- [ ] **Step 1: CLI をライブラリにする**

`crates/cli/Cargo.toml` に:

```toml
[lib]
name = "infodesk_cli"
path = "src/lib.rs"

[[bin]]
name = "infodesk"
path = "src/main.rs"
```

`main.rs` の中身 (`Cli`、各コマンド、`run`) を `lib.rs` に移し、`json.rs` は `lib.rs` から `mod json;` で読む。`lib.rs` に入口を足す:

```rust
/// argv (先頭は argv0) を受け取り、CLI として実行して終了コードを返す。アプリ本体の `--cli` からも呼ぶ。
pub fn run_from<I: IntoIterator<Item = String>>(args: I) -> i32 {
    let cli = match Cli::try_parse_from(args) {
        Ok(c) => c,
        Err(e) => {
            let _ = e.print();
            return e.exit_code();
        }
    };
    let rt = match tokio::runtime::Runtime::new() {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("error: {e}");
            return 1;
        }
    };
    match rt.block_on(run(cli)) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("error: {e}");
            1
        }
    }
}
```

`main.rs` は次だけにする:

```rust
fn main() {
    std::process::exit(infodesk_cli::run_from(std::env::args()));
}
```

`#[tokio::main]` は使わなくなるので、`tokio` の `macros` 機能が他で要らなければ外す (テストで `#[tokio::test]` を使う場合は残す)。

テストを `lib.rs` に足す:

```rust
#[cfg(test)]
mod entry_tests {
    use super::run_from;

    fn args(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn runs_a_command_and_reports_errors_by_exit_code() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("t.db").display().to_string();
        assert_eq!(run_from(args(&["infodesk", "--db", &db, "config", "list", "--json"])), 0);
        assert_eq!(run_from(args(&["infodesk", "--db", &db, "item", "1", "--json"])), 1);
        assert_eq!(run_from(args(&["infodesk", "no-such-command"])), 2);
    }
}
```

`tempfile` は `cargo add -p infodesk --dev tempfile` で足す。clap の使い方の誤りの終了コードは 2 (`clap::Error::exit_code`)。Run: `cargo test -p infodesk` → PASS。`cargo run -p infodesk -- --db /tmp/t.db items --json` が今までどおり動く。

- [ ] **Step 2: アプリ本体の `--cli`**

`cargo add -p infodesk-app infodesk --path crates/cli` で依存を足す。`app/src-tauri/src/main.rs`:

```rust
// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    let args: Vec<String> = std::env::args().collect();
    // チャットのサイドカーは、アプリ本体を `--cli` 付きで起動して infodesk CLI として使う。
    if args.get(1).map(String::as_str) == Some("--cli") {
        let cli_args = std::iter::once("infodesk".to_string()).chain(args.into_iter().skip(2));
        std::process::exit(infodesk_cli::run_from(cli_args));
    }
    infodesk_app_lib::run()
}
```

既存の `main.rs` の先頭の属性はそのまま残す (読んでから書き換える)。Run:

```bash
cargo build -p infodesk-app
./target/debug/infodesk-app --cli --db /tmp/t.db config list --json
```

Expected: `{"settings":{...}}` が出て、ウィンドウは開かない。

- [ ] **Step 3: 純粋な部品の失敗するテストを書く**

`app/src-tauri/src/sidecar.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn parses_node_versions() {
        assert_eq!(parse_node_major("v26.5.0\n"), Some(26));
        assert_eq!(parse_node_major("v22.0.0"), Some(22));
        assert_eq!(parse_node_major("node: command not found"), None);
    }

    #[test]
    fn parses_the_ready_line() {
        assert_eq!(parse_ready_line(r#"INFODESK_AGENT_READY {"port":4321}"#), Some(4321));
        assert_eq!(parse_ready_line("INFODESK_AGENT_READY {}"), None);
        assert_eq!(parse_ready_line("some log line"), None);
    }

    #[test]
    fn resolves_bare_names_on_path_and_extra_dirs() {
        let a = tempfile::tempdir().unwrap();
        let b = tempfile::tempdir().unwrap();
        let node = b.path().join("node");
        std::fs::write(&node, "").unwrap();
        std::fs::set_permissions(&node, std::fs::Permissions::from_mode(0o755)).unwrap();
        let path_env = a.path().display().to_string();
        assert_eq!(resolve_bin("node", &path_env, &[b.path().to_path_buf()]), Some(node.clone()));
        assert_eq!(resolve_bin("node", &path_env, &[]), None);
        assert_eq!(resolve_bin(&node.display().to_string(), "", &[]), Some(node.clone()));
        assert_eq!(resolve_bin("/no/such/node", "", &[]), None);
    }

    #[test]
    fn tokens_are_random_hex() {
        let a = new_token();
        assert_eq!(a.len(), 32);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(a, new_token());
    }
}
```

`tempfile` は app の開発依存に既にある (`app/src-tauri/Cargo.toml`)。Run: `cargo test -p infodesk-app sidecar` → FAIL。

- [ ] **Step 4: 純粋な部品を実装する**

```rust
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum ChatEndpoint {
    Starting,
    Ready { url: String, token: String },
    Error { kind: String, message: String },
}

/// GUI から起動したアプリの PATH は短いので、Homebrew の場所も探す。
pub fn default_extra_path() -> Vec<PathBuf> {
    vec![PathBuf::from("/opt/homebrew/bin"), PathBuf::from("/usr/local/bin")]
}

pub fn parse_node_major(out: &str) -> Option<u32> {
    out.trim().strip_prefix('v')?.split('.').next()?.parse().ok()
}

pub fn parse_ready_line(line: &str) -> Option<u16> {
    let json = line.trim().strip_prefix("INFODESK_AGENT_READY ")?;
    serde_json::from_str::<serde_json::Value>(json).ok()?.get("port")?.as_u64()?.try_into().ok()
}

fn is_executable(p: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(p).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

pub fn resolve_bin(name: &str, path_env: &str, extra: &[PathBuf]) -> Option<PathBuf> {
    if name.contains('/') {
        let p = PathBuf::from(name);
        return is_executable(&p).then_some(p);
    }
    std::env::split_paths(path_env)
        .chain(extra.iter().cloned())
        .map(|dir| dir.join(name))
        .find(|p| is_executable(p))
}

pub fn new_token() -> String {
    use std::io::Read;
    let mut bytes = [0u8; 16];
    std::fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut bytes))
        .expect("read /dev/urandom");
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
```

Run: `cargo test -p infodesk-app sidecar` → PASS

- [ ] **Step 5: 起動の失敗するテストを書く**

偽の `node` (シェルスクリプト) で起動の流れを確かめる。`tests` モジュールに追加:

```rust
    fn fake_node(dir: &Path, body: &str) -> String {
        let p = dir.join("node");
        std::fs::write(&p, format!("#!/bin/sh\nif [ \"$1\" = \"--version\" ]; then echo \"${{FAKE_VERSION:-v26.5.0}}\"; exit 0; fi\n{body}\n")).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        p.display().to_string()
    }

    fn config(dir: &Path, node: String) -> LaunchConfig {
        let agent = dir.join("agent.mjs");
        std::fs::write(&agent, "").unwrap();
        LaunchConfig {
            node,
            agent,
            cli: PathBuf::from("/Applications/InfoDesk.app/Contents/MacOS/infodesk-app"),
            db: dir.join("t.db"),
            token: "abc".into(),
            extra_path: vec![],
        }
    }

    #[tokio::test]
    async fn launch_waits_for_the_ready_line_and_passes_the_environment() {
        let dir = tempfile::tempdir().unwrap();
        let node = fake_node(dir.path(), r#"
[ "$INFODESK_AGENT_TOKEN" = "abc" ] && [ "$INFODESK_CLI_ARG" = "--cli" ] && [ -n "$INFODESK_DB" ] || exit 9
echo "starting"
echo 'INFODESK_AGENT_READY {"port":4321}'
cat > /dev/null"#);
        let (mut child, port) = launch(&config(dir.path(), node), Duration::from_secs(5)).await.unwrap();
        assert_eq!(port, 4321);
        assert!(child.try_wait().unwrap().is_none(), "サイドカーは動き続ける");
        child.kill().await.unwrap();
    }

    #[tokio::test]
    async fn launch_reports_missing_and_old_node() {
        let dir = tempfile::tempdir().unwrap();
        let err = launch(&config(dir.path(), "/no/such/node".into()), Duration::from_secs(1)).await.unwrap_err();
        assert_eq!(err.kind, "node_missing");
        assert!(err.message.contains("/no/such/node"));
        let node = fake_node(dir.path(), "exit 0");
        let old = std::fs::read_to_string(&node).unwrap().replace("v26.5.0", "v20.11.1");
        std::fs::write(&node, old).unwrap();
        let err = launch(&config(dir.path(), node), Duration::from_secs(1)).await.unwrap_err();
        assert_eq!(err.kind, "node_too_old");
        assert!(err.message.contains("20"), "{}", err.message);
    }

    #[tokio::test]
    async fn launch_reports_a_missing_agent_bundle() {
        let dir = tempfile::tempdir().unwrap();
        let mut cfg = config(dir.path(), fake_node(dir.path(), "exit 0"));
        cfg.agent = dir.path().join("missing.mjs");
        let err = launch(&cfg, Duration::from_secs(1)).await.unwrap_err();
        assert_eq!(err.kind, "agent_missing");
    }

    #[tokio::test]
    async fn launch_reports_an_early_exit_with_stderr_and_a_timeout() {
        let dir = tempfile::tempdir().unwrap();
        let node = fake_node(dir.path(), "echo 'Error: Cannot find module zod' >&2\nexit 1");
        let err = launch(&config(dir.path(), node), Duration::from_secs(5)).await.unwrap_err();
        assert_eq!(err.kind, "start_failed");
        assert!(err.message.contains("Cannot find module zod"), "{}", err.message);
        let node = fake_node(dir.path(), "sleep 30");
        let err = launch(&config(dir.path(), node), Duration::from_millis(300)).await.unwrap_err();
        assert_eq!(err.kind, "start_failed");
        assert!(err.message.contains("timed out"), "{}", err.message);
    }
```

Run → FAIL

- [ ] **Step 6: 起動を実装する**

`app/src-tauri/Cargo.toml` の `tokio` に `process` と `io-util` の機能を足す (`cargo add -p infodesk-app tokio --features process,io-util`)。

```rust
use tokio::io::{AsyncBufReadExt, AsyncReadExt, BufReader};
use tokio::process::{Child, Command};

const MIN_NODE_MAJOR: u32 = 22;

pub struct LaunchConfig {
    pub node: String,
    pub agent: PathBuf,
    pub cli: PathBuf,
    pub db: PathBuf,
    pub token: String,
    pub extra_path: Vec<PathBuf>,
}

#[derive(Debug)]
pub struct LaunchError {
    pub kind: &'static str,
    pub message: String,
}

fn err(kind: &'static str, message: String) -> LaunchError {
    LaunchError { kind, message }
}

fn child_path(extra: &[PathBuf]) -> String {
    let current = std::env::var("PATH").unwrap_or_default();
    let mut dirs: Vec<PathBuf> = std::env::split_paths(&current).collect();
    for d in extra {
        if !dirs.contains(d) {
            dirs.push(d.clone());
        }
    }
    std::env::join_paths(dirs).map(|p| p.to_string_lossy().into_owned()).unwrap_or(current)
}

pub async fn launch(cfg: &LaunchConfig, ready_timeout: Duration) -> Result<(Child, u16), LaunchError> {
    let path_env = std::env::var("PATH").unwrap_or_default();
    let node = resolve_bin(&cfg.node, &path_env, &cfg.extra_path).ok_or_else(|| {
        err("node_missing", format!("Node が見つかりません ({})。チャットには Node {MIN_NODE_MAJOR} 以上が必要です。", cfg.node))
    })?;
    let version = tokio::time::timeout(Duration::from_secs(5), Command::new(&node).arg("--version").output())
        .await
        .map_err(|_| err("node_missing", format!("Node の版を確かめられません ({})", node.display())))?
        .map_err(|e| err("node_missing", format!("Node を実行できません ({}): {e}", node.display())))?;
    let text = String::from_utf8_lossy(&version.stdout).to_string();
    match parse_node_major(&text) {
        Some(m) if m >= MIN_NODE_MAJOR => {}
        Some(m) => {
            return Err(err("node_too_old", format!("Node {m} は古すぎます ({})。チャットには Node {MIN_NODE_MAJOR} 以上が必要です。", node.display())));
        }
        None => return Err(err("node_missing", format!("Node の版を読めません ({}): {}", node.display(), text.trim()))),
    }
    if !cfg.agent.is_file() {
        return Err(err("agent_missing", format!("エージェントのファイルがありません ({})。agent で npm run build を実行してください。", cfg.agent.display())));
    }
    let mut child = Command::new(&node)
        .arg(&cfg.agent)
        .env("INFODESK_CLI", &cfg.cli)
        .env("INFODESK_CLI_ARG", "--cli")
        .env("INFODESK_DB", &cfg.db)
        .env("INFODESK_AGENT_TOKEN", &cfg.token)
        .env("INFODESK_AGENT_PORT", "0")
        .env("PATH", child_path(&cfg.extra_path))
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| err("start_failed", format!("エージェントを起動できません: {e}")))?;
    let stdout = child.stdout.take().expect("piped stdout");
    let mut lines = BufReader::new(stdout).lines();
    let wait_ready = async {
        while let Ok(Some(line)) = lines.next_line().await {
            if let Some(port) = parse_ready_line(&line) {
                return Some(port);
            }
        }
        None
    };
    match tokio::time::timeout(ready_timeout, wait_ready).await {
        Ok(Some(port)) => Ok((child, port)),
        Ok(None) => {
            let mut stderr = String::new();
            if let Some(mut e) = child.stderr.take() {
                let _ = tokio::time::timeout(Duration::from_secs(1), e.read_to_string(&mut stderr)).await;
            }
            let tail: Vec<&str> = stderr.lines().rev().take(5).collect::<Vec<_>>().into_iter().rev().collect();
            Err(err("start_failed", format!("エージェントが起動直後に終了しました: {}", tail.join(" / "))))
        }
        Err(_) => {
            let _ = child.kill().await;
            Err(err("start_failed", "エージェントの起動が timed out しました".to_string()))
        }
    }
}
```

準備完了のあと、子プロセスの標準出力と標準エラーは読まれなくなる。ログが多いとパイプが詰まるので、準備完了後に `tokio::spawn` で残りの行を読み捨てる (標準エラーは `eprintln!("[agent] {line}")` で開発時のログに出す)。この処理を `launch` の `Ok` の手前に足す。

Run: `cargo test -p infodesk-app sidecar` → PASS

- [ ] **Step 7: `Sidecar` の状態、コマンド、起動と終了**

`sidecar.rs` に:

```rust
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Manager};

pub struct Sidecar {
    endpoint: Mutex<ChatEndpoint>,
    child: tokio::sync::Mutex<Option<Child>>,
}

impl Default for Sidecar {
    fn default() -> Self {
        Self { endpoint: Mutex::new(ChatEndpoint::Starting), child: tokio::sync::Mutex::new(None) }
    }
}

impl Sidecar {
    pub fn endpoint(&self) -> ChatEndpoint {
        self.endpoint.lock().expect("endpoint lock").clone()
    }

    fn set(&self, app: &AppHandle, e: ChatEndpoint) {
        *self.endpoint.lock().expect("endpoint lock") = e;
        let _ = app.emit(crate::state::EVENT_CHAT, ());
    }

    /// 動いているサイドカーを止めて、起動し直す。
    pub async fn restart(&self, app: &AppHandle) {
        let mut slot = self.child.lock().await;
        if let Some(mut old) = slot.take() {
            let _ = old.kill().await;
        }
        self.set(app, ChatEndpoint::Starting);
        let cfg = match launch_config(app) {
            Ok(c) => c,
            Err(e) => return self.set(app, ChatEndpoint::Error { kind: e.kind.into(), message: e.message }),
        };
        match launch(&cfg, Duration::from_secs(20)).await {
            Ok((child, port)) => {
                *slot = Some(child);
                self.set(app, ChatEndpoint::Ready { url: format!("http://127.0.0.1:{port}"), token: cfg.token });
            }
            Err(e) => self.set(app, ChatEndpoint::Error { kind: e.kind.into(), message: e.message }),
        }
    }

    /// アプリの終了時に呼ぶ。
    pub fn stop(&self) {
        if let Ok(mut slot) = self.child.try_lock()
            && let Some(child) = slot.as_mut()
        {
            let _ = child.start_kill();
        }
    }
}

fn agent_path(app: &AppHandle) -> PathBuf {
    // バンドルでは resources/agent/agent.mjs。開発時はリポジトリの agent/dist/agent.mjs。
    if let Ok(p) = app.path().resolve("agent/agent.mjs", tauri::path::BaseDirectory::Resource)
        && p.is_file()
    {
        return p;
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../agent/dist/agent.mjs")
}

fn launch_config(app: &AppHandle) -> Result<LaunchConfig, LaunchError> {
    let state = app.state::<crate::state::AppState>();
    let node = state
        .with_store(|s| s.get_setting_or("node.bin", "node"))
        .map_err(|e| err("start_failed", e.message))?;
    let db = app
        .path()
        .app_data_dir()
        .map_err(|e| err("start_failed", e.to_string()))?
        .join("infodesk.db");
    let cli = std::env::current_exe().map_err(|e| err("start_failed", e.to_string()))?;
    Ok(LaunchConfig { node, agent: agent_path(app), cli, db, token: new_token(), extra_path: default_extra_path() })
}
```

`state.rs`: `pub const EVENT_CHAT: &str = "chat-endpoint-changed";`、`AppState` に `pub chat: crate::sidecar::Sidecar` を足し、`AppState::new` で `Sidecar::default()`。`lib.rs` に `mod sidecar;`。DB のパスは `lib.rs` の `setup` と同じ (`app_data_dir()/infodesk.db`)。

`commands/chat_endpoint.rs`:

```rust
use tauri::{AppHandle, Manager, State};

use crate::sidecar::ChatEndpoint;
use crate::state::AppState;

#[tauri::command]
pub fn chat_endpoint(state: State<'_, AppState>) -> ChatEndpoint {
    state.chat.endpoint()
}

#[tauri::command]
pub fn restart_chat(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        app.state::<AppState>().chat.restart(&app).await;
    });
}
```

`lib.rs`:
- `setup` の最後に、サイドカーの起動を始める:

```rust
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                handle.state::<AppState>().chat.restart(&handle).await;
            });
```

- `generate_handler!` に `commands::chat_endpoint::chat_endpoint`、`commands::chat_endpoint::restart_chat`。
- `.run(|app, event| ...)` で `tauri::RunEvent::Exit` のとき `app.state::<AppState>().chat.stop()` を呼ぶ (既存の `Reopen` の処理は残す。変数名 `_app`、`_event` を使っている箇所は読んでから直す)。

`tauri.conf.json` の CSP の `connect-src` を `ipc: http://ipc.localhost http://127.0.0.1:*` にする。

- [ ] **Step 8: 手で確かめる**

```bash
cd agent && npm run build && cd ../app && npm run tauri dev
```

Expected: 起動後、開発者ツールのコンソールで `await window.__TAURI_INTERNALS__.invoke("chat_endpoint")` が `{status:"ready", url:"http://127.0.0.1:<port>", token:"..."}` を返す。設定 `node.bin` を `/no/such/node` にして `invoke("restart_chat")` のあと `chat_endpoint` が `{status:"error", kind:"node_missing", ...}` になる (確かめたら `node` に戻す)。アプリを終了したあと `pgrep -fl agent.mjs` に何も出ない。この確認は画面を操作できる場合だけ行い、できない場合は報告に「未確認」と書く。

- [ ] **Step 9: コミット**

```bash
cargo test --workspace && cargo clippy --all-targets -- -D warnings && cargo fmt --check
git add crates/cli app/src-tauri Cargo.lock
git commit -m "feat(app): launch the chat sidecar and serve the CLI from the app binary"
```

---

### Task 7: 会話の表示 (`ChatThread`)

**Files:**
- Create: `app/src/chat/context.ts`、`app/src/chat/toolLabels.ts`、`app/src/chat/references.ts`、`app/src/chat/errors.ts`、`app/src/chat/ToolCall.tsx`、`app/src/chat/ChatThread.tsx`、`app/src/styles/chat.css`、`app/src/chat/toolLabels.test.ts`、`app/src/chat/references.test.ts`、`app/src/chat/errors.test.ts`、`app/src/chat/ChatThread.test.tsx`、`app/src/test/stream.ts`
- Modify: `app/package.json`、`app/src/main.tsx` (`chat.css` を読み込む)、`app/src/api.ts` (`getChatMessages` などの型を `UIMessage[]` に)、`app/src/test/setup.ts` (必要なら jsdom の不足分)

**Interfaces:**
- Consumes: Task 3 の `api.openItem`、Task 5 の HTTP の契約 (`POST <url>/chat`、`Authorization: Bearer <token>`、本文 `{ messages, provider, context }`、エラーは `errorText` に `ChatErrorInfo` の JSON)
- Produces:
  - `chat/context.ts`: `ChatContext`、`ChatScope`、`ChatPeriod`、`ChatAttachment` (Task 4 の `agent/src/context.ts` と同じ形)、`type ProviderId = "ollama" | "claude"`
  - `chat/toolLabels.ts`: `describeTool(toolName: string, args: unknown, result: unknown): { label: string; detail: string }`
  - `chat/references.ts`: `referencedItems(message: UIMessage, max?: number): { id: number; title: string }[]`
  - `chat/errors.ts`: `parseChatError(error: Error | undefined): { kind: string; provider: ProviderId | null; title: string; message: string } | null`
  - `ChatThread` props: `{ endpoint: { url: string; token: string }; chatKey: string; initialMessages: UIMessage[]; provider: ProviderId; modelLabel: string; context: ChatContext; onProviderChange(p: ProviderId): void; onPersist(messages: UIMessage[]): Promise<void>; onOpenSettings(): void; suggestions: string[] }`

- [ ] **Step 1: 依存を入れる**

```bash
cd app
npm install --save-exact ai@7.0.136 @ai-sdk/react@4.0.139 @assistant-ui/react@0.15.25 @assistant-ui/react-ai-sdk@1.4.16 @assistant-ui/react-markdown@0.14.19
```

`npm ls react` で React が1つ (19 系) だけであることを確かめる。`@assistant-ui/react` の peer は `react ^18 || ^19` (npm で確認済み)。

- [ ] **Step 2: 純粋なロジックの失敗するテストを書く**

`app/src/chat/context.ts` は `agent/src/context.ts` と同じ内容に、`export type ProviderId = "ollama" | "claude";` を足したもの。

`app/src/chat/toolLabels.test.ts`:

```ts
import { describe, expect, it } from "vitest";
import { describeTool } from "./toolLabels";

describe("describeTool", () => {
  it("labels the infodesk tools with or without the MCP prefix", () => {
    expect(describeTool("search_articles", { query: "認知症", days: 7 }, { items: [], total: 14 }))
      .toEqual({ label: "記事検索", detail: "認知症 · 直近7日 · 14件" });
    expect(describeTool("mcp__infodesk__search_articles", { category: "medcare" }, undefined))
      .toEqual({ label: "記事検索", detail: "医療介護" });
    expect(describeTool("get_article", { id: 12 }, { item: { id: 12, title: "新薬の記事" } }))
      .toEqual({ label: "記事を読む", detail: "新薬の記事" });
    expect(describeTool("fetch_page", { url: "https://www.example.com/a/b" }, { page: { title: "介護報酬改定の論点整理" } }))
      .toEqual({ label: "本文取得", detail: "介護報酬改定の論点整理" });
    expect(describeTool("fetch_page", { url: "https://www.example.com/a/b" }, undefined))
      .toEqual({ label: "本文取得", detail: "www.example.com" });
    expect(describeTool("read_digests", { period: "week" }, undefined)).toEqual({ label: "要約を読む", detail: "週次" });
    expect(describeTool("web_search", { query: "認知症 新薬" }, undefined)).toEqual({ label: "Web検索", detail: "認知症 新薬" });
    expect(describeTool("WebSearch", { query: "x" }, undefined)).toEqual({ label: "Web検索", detail: "x" });
  });

  it("falls back to the raw name", () => {
    expect(describeTool("mcp__other__thing", {}, undefined)).toEqual({ label: "mcp__other__thing", detail: "" });
  });
});
```

`app/src/chat/references.test.ts`:

```ts
import type { UIMessage } from "ai";
import { describe, expect, it } from "vitest";
import { referencedItems } from "./references";

const msg = (parts: unknown[]): UIMessage => ({ id: "a", role: "assistant", parts } as UIMessage);

describe("referencedItems", () => {
  it("collects items from search and article tool outputs, deduplicated", () => {
    const m = msg([
      { type: "tool-search_articles", toolCallId: "1", state: "output-available", input: {},
        output: { items: [{ id: 1, title: "A" }, { id: 2, title: "B" }], total: 2 } },
      { type: "tool-get_article", toolCallId: "2", state: "output-available", input: { id: 2 },
        output: { item: { id: 2, title: "B" } } },
      { type: "text", text: "回答" },
    ]);
    expect(referencedItems(m)).toEqual([{ id: 1, title: "A" }, { id: 2, title: "B" }]);
  });

  it("reads Claude MCP results that arrive as text content", () => {
    const m = msg([
      { type: "dynamic-tool", toolName: "mcp__infodesk__search_articles", toolCallId: "1", state: "output-available", input: {},
        output: { content: [{ type: "text", text: JSON.stringify({ items: [{ id: 7, title: "C" }], total: 1 }) }] } },
      { type: "dynamic-tool", toolName: "mcp__infodesk__get_article", toolCallId: "2", state: "output-available", input: {},
        output: JSON.stringify({ item: { id: 8, title: "D" } }) },
    ]);
    expect(referencedItems(m)).toEqual([{ id: 7, title: "C" }, { id: 8, title: "D" }]);
  });

  it("ignores failed or unfinished tools and caps the list", () => {
    const many = Array.from({ length: 12 }, (_, i) => ({ id: i + 1, title: `T${i + 1}` }));
    const m = msg([
      { type: "tool-search_articles", toolCallId: "1", state: "output-error", input: {}, errorText: "x" },
      { type: "tool-search_articles", toolCallId: "2", state: "input-available", input: {} },
      { type: "tool-search_articles", toolCallId: "3", state: "output-available", input: {}, output: { items: many, total: 12 } },
    ]);
    expect(referencedItems(m)).toHaveLength(8);
    expect(referencedItems(m, 3).map((x) => x.id)).toEqual([1, 2, 3]);
    expect(referencedItems(msg([{ type: "text", text: "x" }]))).toEqual([]);
  });
});
```

`app/src/chat/errors.test.ts`:

```ts
import { describe, expect, it } from "vitest";
import { parseChatError } from "./errors";

describe("parseChatError", () => {
  it("reads the structured error from the sidecar", () => {
    const e = new Error(JSON.stringify({ kind: "llm_unreachable", provider: "ollama", message: "http://localhost:11434 が応答しません。" }));
    expect(parseChatError(e)).toEqual({ kind: "llm_unreachable", provider: "ollama", title: "Ollama に接続できません", message: "http://localhost:11434 が応答しません。" });
    const auth = new Error(JSON.stringify({ kind: "auth", provider: "claude", message: "ログインしていません。" }));
    expect(parseChatError(auth)?.title).toBe("Claude にログインしていません");
    const model = new Error(JSON.stringify({ kind: "model_missing", provider: "ollama", message: "m" }));
    expect(parseChatError(model)?.title).toBe("モデルが見つかりません");
  });

  it("treats anything else as a generic error", () => {
    expect(parseChatError(new Error("Failed to fetch"))).toEqual({ kind: "other", provider: null, title: "回答を受け取れませんでした", message: "Failed to fetch" });
    expect(parseChatError(undefined)).toBeNull();
  });
});
```

Run: `npm test -- --run src/chat` → FAIL

- [ ] **Step 3: 純粋なロジックを実装する**

`app/src/chat/toolLabels.ts`:

```ts
import { CATEGORIES } from "../types";

const MCP_PREFIX = "mcp__infodesk__";
const PERIOD = { day: "日次", week: "週次", month: "月次" } as Record<string, string>;
const LABELS: Record<string, string> = {
  search_articles: "記事検索",
  get_article: "記事を読む",
  read_digests: "要約を読む",
  fetch_page: "本文取得",
  web_search: "Web検索",
  WebSearch: "Web検索",
};

type Obj = Record<string, any>;
const obj = (v: unknown): Obj => (v && typeof v === "object" ? (v as Obj) : {});

export function describeTool(toolName: string, args: unknown, result: unknown): { label: string; detail: string } {
  const name = toolName.startsWith(MCP_PREFIX) ? toolName.slice(MCP_PREFIX.length) : toolName;
  const label = LABELS[name];
  if (!label) return { label: toolName, detail: "" };
  const a = obj(args);
  const r = obj(result);
  const parts: string[] = [];
  switch (name) {
    case "search_articles":
      if (a.query) parts.push(String(a.query));
      if (a.category) parts.push(CATEGORIES.find((c) => c.id === a.category)?.label ?? String(a.category));
      if (a.topic) parts.push(String(a.topic));
      if (a.days) parts.push(`直近${a.days}日`);
      if (typeof r.total === "number") parts.push(`${r.total}件`);
      break;
    case "get_article":
      parts.push(obj(r.item).title ? String(obj(r.item).title) : `id ${a.id ?? "?"}`);
      break;
    case "fetch_page":
      if (obj(r.page).title) parts.push(String(obj(r.page).title));
      else {
        try { parts.push(new URL(String(a.url)).host); } catch { /* URL でなければ何も出さない */ }
      }
      break;
    case "read_digests":
      parts.push(PERIOD[String(a.period)] ?? "");
      break;
    default:
      if (a.query) parts.push(String(a.query));
  }
  return { label, detail: parts.filter(Boolean).join(" · ") };
}
```

カテゴリーの表示名は `types.ts` の既存の `CATEGORIES` (`{ id, label }[]`) を使う。新しく重複した表を作らない。

`app/src/chat/references.ts`:

```ts
import type { UIMessage } from "ai";

type Obj = Record<string, any>;

function unwrap(output: unknown): Obj | null {
  let v: unknown = output;
  if (v && typeof v === "object" && Array.isArray((v as Obj).content)) {
    const text = (v as Obj).content.find((c: Obj) => c?.type === "text")?.text;
    v = typeof text === "string" ? text : null;
  }
  if (typeof v === "string") {
    try { v = JSON.parse(v); } catch { return null; }
  }
  return v && typeof v === "object" ? (v as Obj) : null;
}

export function referencedItems(message: UIMessage, max = 8): { id: number; title: string }[] {
  const seen = new Map<number, string>();
  for (const part of message.parts as Obj[]) {
    const isTool = typeof part.type === "string" && (part.type.startsWith("tool-") || part.type === "dynamic-tool");
    if (!isTool || part.state !== "output-available") continue;
    const out = unwrap(part.output);
    if (!out) continue;
    const list: Obj[] = Array.isArray(out.items) ? out.items : out.item ? [out.item] : [];
    for (const it of list) {
      if (typeof it?.id === "number" && typeof it?.title === "string" && !seen.has(it.id)) seen.set(it.id, it.title);
    }
  }
  return [...seen].slice(0, max).map(([id, title]) => ({ id, title }));
}
```

`app/src/chat/errors.ts`:

```ts
import type { ProviderId } from "./context";

const TITLES: Record<string, string> = {
  llm_unreachable: "Ollama に接続できません",
  model_missing: "モデルが見つかりません",
  auth: "Claude にログインしていません",
};

export interface ChatErrorView {
  kind: string;
  provider: ProviderId | null;
  title: string;
  message: string;
}

export function parseChatError(error: Error | undefined): ChatErrorView | null {
  if (!error) return null;
  try {
    const v = JSON.parse(error.message);
    if (v && typeof v.kind === "string" && typeof v.message === "string") {
      const provider = v.provider === "ollama" || v.provider === "claude" ? v.provider : null;
      return { kind: v.kind, provider, title: TITLES[v.kind] ?? "回答を受け取れませんでした", message: v.message };
    }
  } catch { /* JSON でなければ下へ */ }
  return { kind: "other", provider: null, title: "回答を受け取れませんでした", message: error.message };
}
```

Run: `npm test -- --run src/chat` → PASS (3ファイル)

- [ ] **Step 4: 会話の失敗するテストを書く**

テスト用に、サイドカーの応答 (UI message stream) を本物の `ai` の関数で作る。`app/src/test/stream.ts`:

```ts
import { createUIMessageStream, createUIMessageStreamResponse, type UIMessageChunk } from "ai";
import { vi } from "vitest";

/** サイドカーの /chat の応答を、与えた chunk の列で作る。 */
export function streamResponse(chunks: UIMessageChunk[]): Response {
  return createUIMessageStreamResponse({
    stream: createUIMessageStream({ execute: ({ writer }) => { for (const c of chunks) writer.write(c); } }),
  });
}

export const answerChunks = (text: string, extra: UIMessageChunk[] = []): UIMessageChunk[] => [
  { type: "start" },
  ...extra,
  { type: "text-start", id: "t1" },
  { type: "text-delta", id: "t1", delta: text },
  { type: "text-end", id: "t1" },
  { type: "finish" },
] as UIMessageChunk[];

/** fetch を差し替え、呼ばれた要求を記録する。 */
export function mockFetch(...responses: Array<() => Response | Promise<Response>>) {
  const calls: { url: string; init: RequestInit; body: any }[] = [];
  let i = 0;
  const fn = vi.fn(async (url: string, init: RequestInit) => {
    calls.push({ url: String(url), init, body: init.body ? JSON.parse(String(init.body)) : null });
    const make = responses[Math.min(i++, responses.length - 1)];
    return make();
  });
  vi.stubGlobal("fetch", fn);
  return calls;
}
```

`app/src/chat/ChatThread.test.tsx`:

```tsx
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { UIMessage } from "ai";
import { afterEach, describe, expect, it, vi } from "vitest";
import { setupIpc } from "../test/ipc";
import { answerChunks, mockFetch, streamResponse } from "../test/stream";
import { ChatThread } from "./ChatThread";
import type { ChatContext } from "./context";

const CONTEXT: ChatContext = { scope: { kind: "category", category: "medcare", label: "医療介護" }, period: { view: "feed", range: "week" }, attachments: [] };
const ENDPOINT = { url: "http://127.0.0.1:4321", token: "tok" };

function renderThread(over: Partial<React.ComponentProps<typeof ChatThread>> = {}) {
  const props = {
    endpoint: ENDPOINT, chatKey: "new-1", initialMessages: [] as UIMessage[], provider: "ollama" as const,
    modelLabel: "Ollama · gemma4:26b", context: CONTEXT,
    onProviderChange: vi.fn(), onPersist: vi.fn(async () => {}), onOpenSettings: vi.fn(),
    suggestions: ["今日のGitHubで伸びたリポジトリを3つ選んで理由を説明して"],
    ...over,
  };
  render(<ChatThread {...props} />);
  return props;
}

async function ask(text: string) {
  const user = userEvent.setup();
  await user.type(screen.getByRole("textbox", { name: "質問" }), text);
  await user.click(screen.getByRole("button", { name: "送信" }));
}

afterEach(() => vi.unstubAllGlobals());

describe("ChatThread", () => {
  it("sends the question with the provider, the context and the token", async () => {
    setupIpc({});
    const calls = mockFetch(() => streamResponse(answerChunks("答えです。")));
    renderThread();
    await ask("今週の認知症の動きは?");
    expect(await screen.findByText("答えです。")).toBeInTheDocument();
    expect(calls[0].url).toBe("http://127.0.0.1:4321/chat");
    expect(new Headers(calls[0].init.headers).get("authorization")).toBe("Bearer tok");
    expect(calls[0].body.provider).toBe("ollama");
    expect(calls[0].body.context).toEqual(CONTEXT);
    expect(calls[0].body.messages.at(-1).parts[0].text).toBe("今週の認知症の動きは?");
  });

  it("shows tool lines, markdown and the referenced articles", async () => {
    const ipc = setupIpc({ open_item: () => null });
    mockFetch(() => streamResponse(answerChunks("## 新薬\n- 適用拡大の検討", [
      { type: "tool-input-available", toolCallId: "c1", toolName: "search_articles", input: { query: "認知症", days: 7 } },
      { type: "tool-output-available", toolCallId: "c1", output: { items: [{ id: 12, title: "認知症の新薬、適用拡大へ" }], total: 14 } },
    ] as never)));
    renderThread();
    await ask("まとめて");
    expect(await screen.findByText("記事検索")).toBeInTheDocument();
    expect(screen.getByText("認知症 · 直近7日 · 14件")).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "新薬" })).toBeInTheDocument();
    expect(screen.getByText("適用拡大の検討")).toBeInTheDocument();
    await userEvent.setup().click(screen.getByRole("button", { name: "認知症の新薬、適用拡大へ" }));
    expect(ipc.last("open_item")).toEqual({ id: 12 });
  });

  it("persists the conversation after an answer completes", async () => {
    setupIpc({});
    mockFetch(() => streamResponse(answerChunks("答え")));
    const props = renderThread();
    await ask("質問");
    await waitFor(() => expect(props.onPersist).toHaveBeenCalled());
    const saved = (props.onPersist as any).mock.calls.at(-1)[0] as UIMessage[];
    expect(saved.map((m) => m.role)).toEqual(["user", "assistant"]);
  });

  it("shows an LLM error with retry, switch provider and settings, keeping the question", async () => {
    setupIpc({});
    const errorText = JSON.stringify({ kind: "llm_unreachable", provider: "ollama", message: "http://localhost:11434 が応答しません。" });
    const calls = mockFetch(
      () => streamResponse([{ type: "start" }, { type: "error", errorText }] as never),
      () => streamResponse(answerChunks("今度は答えた")),
    );
    const props = renderThread();
    await ask("質問A");
    expect(await screen.findByText("Ollama に接続できません")).toBeInTheDocument();
    expect(screen.getByText("http://localhost:11434 が応答しません。")).toBeInTheDocument();
    expect(screen.getByText("質問A")).toBeInTheDocument();
    const user = userEvent.setup();
    await user.click(screen.getByRole("button", { name: "LLM設定" }));
    expect(props.onOpenSettings).toHaveBeenCalled();
    await user.click(screen.getByRole("button", { name: "再試行" }));
    expect(await screen.findByText("今度は答えた")).toBeInTheDocument();
    expect(calls).toHaveLength(2);
    expect(calls[1].body.messages.at(-1).parts[0].text).toBe("質問A");
  });

  it("offers to send with the other provider only when the user asks", async () => {
    setupIpc({});
    const errorText = JSON.stringify({ kind: "llm_unreachable", provider: "ollama", message: "x" });
    const calls = mockFetch(() => streamResponse([{ type: "start" }, { type: "error", errorText }] as never));
    const props = renderThread();
    await ask("質問B");
    await screen.findByText("Ollama に接続できません");
    expect(calls).toHaveLength(1);
    await userEvent.setup().click(screen.getByRole("button", { name: "Claude で送る" }));
    expect(props.onProviderChange).toHaveBeenCalledWith("claude");
  });

  it("renders a saved conversation and the empty state", async () => {
    setupIpc({});
    const saved: UIMessage[] = [
      { id: "u", role: "user", parts: [{ type: "text", text: "前の質問" }] },
      { id: "a", role: "assistant", parts: [{ type: "text", text: "前の回答" }] },
    ];
    renderThread({ initialMessages: saved, chatKey: "thread-3" });
    expect(screen.getByText("前の質問")).toBeInTheDocument();
    expect(screen.getByText("前の回答")).toBeInTheDocument();
  });

  it("shows suggestions when empty and sends one on click", async () => {
    setupIpc({});
    const calls = mockFetch(() => streamResponse(answerChunks("はい")));
    renderThread();
    expect(screen.getByText("収集した記事について質問できます")).toBeInTheDocument();
    await userEvent.setup().click(screen.getByRole("button", { name: /今日のGitHubで伸びた/ }));
    await waitFor(() => expect(calls).toHaveLength(1));
    expect(calls[0].body.messages[0].parts[0].text).toContain("今日のGitHubで伸びた");
  });

  it("shows the model and lets the user switch provider", async () => {
    setupIpc({});
    const props = renderThread();
    expect(screen.getByRole("combobox", { name: "LLM" })).toHaveValue("ollama");
    await userEvent.setup().selectOptions(screen.getByRole("combobox", { name: "LLM" }), "claude");
    expect(props.onProviderChange).toHaveBeenCalledWith("claude");
  });
});
```

assistant-ui の部品が jsdom に無い API (`ResizeObserver`、`Element.prototype.scrollTo`、`matchMedia`) を使って落ちる場合は、`app/src/test/setup.ts` に最小の代替 (何もしない関数) を足す。足したものは報告に書く。

Run: `npm test -- --run src/chat/ChatThread.test.tsx` → FAIL (`Cannot find module './ChatThread'`)

- [ ] **Step 5: `ToolCall` と `ChatThread` を実装する**

`app/src/chat/ToolCall.tsx`:

```tsx
import type { ToolCallMessagePartProps } from "@assistant-ui/react";
import { FileText, Globe, Newspaper, Search, Wrench } from "lucide-react";
import { describeTool } from "./toolLabels";

const ICONS: Record<string, typeof Search> = { 記事検索: Search, 記事を読む: Newspaper, 要約を読む: FileText, 本文取得: FileText, Web検索: Globe };

export function ToolCall({ toolName, args, result, isError, status }: ToolCallMessagePartProps) {
  const { label, detail } = describeTool(toolName, args, result);
  const Icon = ICONS[label] ?? Wrench;
  const state = isError || status.type === "incomplete" ? "失敗" : status.type === "running" ? "実行中" : "完了";
  return (
    <div className={`tool-call${state === "失敗" ? " is-error" : ""}`}>
      <Icon size={13} aria-hidden />
      <span className="tool-label">{label}</span>
      {detail && <span className="tool-detail">: {detail}</span>}
      <span className={`tool-state${state === "実行中" ? " running" : ""}`}>{state}</span>
    </div>
  );
}
```

`ToolCallMessagePartProps` が `@assistant-ui/react` から型として出ていない場合は、`@assistant-ui/core` の `react/types/MessagePartComponentTypes.d.ts` にある名前を `@assistant-ui/react` の `index.d.ts` で探して使う。

`app/src/chat/ChatThread.tsx`:

```tsx
import { useChat } from "@ai-sdk/react";
import { AssistantRuntimeProvider, ComposerPrimitive, MessagePrimitive, ThreadPrimitive } from "@assistant-ui/react";
import { MarkdownTextPrimitive } from "@assistant-ui/react-markdown";
import { useAISDKRuntime } from "@assistant-ui/react-ai-sdk";
import { DefaultChatTransport, type UIMessage } from "ai";
import { AlertCircle, ArrowUp, CornerDownRight, Sparkles } from "lucide-react";
import { useMemo, useRef } from "react";
import { api } from "../api";
import type { ChatContext, ProviderId } from "./context";
import { parseChatError } from "./errors";
import { referencedItems } from "./references";
import { ToolCall } from "./ToolCall";

export interface ChatThreadProps {
  endpoint: { url: string; token: string };
  chatKey: string;
  initialMessages: UIMessage[];
  provider: ProviderId;
  modelLabel: string;
  context: ChatContext;
  onProviderChange(p: ProviderId): void;
  onPersist(messages: UIMessage[]): Promise<void>;
  onOpenSettings(): void;
  suggestions: string[];
}

const OTHER: Record<ProviderId, ProviderId> = { ollama: "claude", claude: "ollama" };
const PROVIDER_NAME: Record<ProviderId, string> = { ollama: "Ollama", claude: "Claude" };

function MarkdownText() {
  return <MarkdownTextPrimitive className="chat-markdown" />;
}

function References({ message }: { message: UIMessage }) {
  const refs = referencedItems(message);
  if (refs.length === 0) return null;
  return (
    <div className="chat-refs">
      {refs.map((r) => (
        <button key={r.id} className="chat-ref" onClick={() => void api.openItem(r.id)}>{r.title}</button>
      ))}
    </div>
  );
}

export function ChatThread(props: ChatThreadProps) {
  const { endpoint, chatKey, initialMessages, provider, context, onPersist } = props;
  // 送信時の値を、transport を作り直さずに読むための参照。
  const latest = useRef({ provider, context });
  latest.current = { provider, context };
  const transport = useMemo(
    () => new DefaultChatTransport<UIMessage>({
      api: `${endpoint.url}/chat`,
      headers: { Authorization: `Bearer ${endpoint.token}` },
      body: () => ({ provider: latest.current.provider, context: latest.current.context }),
    }),
    [endpoint.url, endpoint.token],
  );
  const chat = useChat<UIMessage>({
    id: chatKey,
    messages: initialMessages,
    transport,
    onFinish: ({ messages, isError, isAbort }) => {
      if (!isError && !isAbort) void onPersist(messages);
    },
  });
  const runtime = useAISDKRuntime(chat);
  const error = parseChatError(chat.error);
  const lastAssistant = [...chat.messages].reverse().find((m) => m.role === "assistant");

  const retry = () => { chat.clearError(); void chat.regenerate(); };
  const switchAndRetry = (p: ProviderId) => {
    props.onProviderChange(p);
    latest.current = { ...latest.current, provider: p };
    retry();
  };

  return (
    <AssistantRuntimeProvider runtime={runtime}>
      <ThreadPrimitive.Root className="chat-thread">
        <ThreadPrimitive.Viewport className="chat-viewport">
          <ThreadPrimitive.Empty>
            <div className="chat-empty">
              <Sparkles size={22} aria-hidden />
              <h3>収集した記事について質問できます</h3>
              <p className="text-secondary">表示中のカテゴリーと期間を文脈にして答えます。例:</p>
              {props.suggestions.map((s) => (
                <button key={s} className="chat-suggestion" onClick={() => void chat.sendMessage({ text: s })}>
                  <CornerDownRight size={13} aria-hidden />{s}
                </button>
              ))}
            </div>
          </ThreadPrimitive.Empty>
          <ThreadPrimitive.Messages
            components={{
              UserMessage: () => (
                <MessagePrimitive.Root className="chat-msg user"><MessagePrimitive.Parts /></MessagePrimitive.Root>
              ),
              AssistantMessage: () => (
                <MessagePrimitive.Root className="chat-msg assistant">
                  <MessagePrimitive.Parts components={{ Text: MarkdownText, tools: { Fallback: ToolCall } }} />
                </MessagePrimitive.Root>
              ),
            }}
          />
          {lastAssistant && chat.status === "ready" && <References message={lastAssistant} />}
          {error && (
            <div className="chat-error" role="alert">
              <div className="chat-error-title"><AlertCircle size={15} aria-hidden />{error.title}</div>
              <p>{error.message} 質問はそのまま残っています。</p>
              <div className="chat-error-actions">
                <button className="btn btn-primary" onClick={retry}>再試行</button>
                <button className="btn" onClick={() => switchAndRetry(OTHER[error.provider ?? provider])}>
                  {PROVIDER_NAME[OTHER[error.provider ?? provider]]} で送る
                </button>
                <button className="btn" onClick={props.onOpenSettings}>LLM設定</button>
              </div>
            </div>
          )}
        </ThreadPrimitive.Viewport>
        <ComposerPrimitive.Root className="chat-composer">
          <ComposerPrimitive.Input aria-label="質問" placeholder="記事や要約について質問…" className="chat-input" />
          <div className="chat-composer-row">
            <select aria-label="LLM" value={provider} onChange={(e) => props.onProviderChange(e.target.value as ProviderId)}>
              <option value="ollama">{provider === "ollama" ? props.modelLabel : "Ollama"}</option>
              <option value="claude">Claude</option>
            </select>
            <ComposerPrimitive.Send aria-label="送信" className="chat-send"><ArrowUp size={15} /></ComposerPrimitive.Send>
          </div>
        </ComposerPrimitive.Root>
      </ThreadPrimitive.Root>
    </AssistantRuntimeProvider>
  );
}
```

ここで使う部品名 (`ThreadPrimitive.Root`/`Viewport`/`Empty`/`Messages`、`MessagePrimitive.Root`/`Parts`、`ComposerPrimitive.Root`/`Input`/`Send`) と `Messages` の `components` の形は、`node_modules/@assistant-ui/core/dist/react/primitives/` の型定義で確かめる。名前が違えば型定義に合わせ、計画との差を報告に書く。`chat.regenerate`、`chat.clearError`、`chat.sendMessage({ text })` は `@ai-sdk/react` の `UseChatHelpers` (型定義で確認済み)。

`app/src/styles/chat.css` (トークンは `theme.css` のものを使う。新しい色を作らない):

```css
.chat-thread { display: flex; flex-direction: column; flex: 1; min-height: 0; }
.chat-viewport { flex: 1; overflow-y: auto; padding: 12px 16px; display: flex; flex-direction: column; gap: 10px; }
.chat-msg.user { align-self: flex-end; max-width: 85%; background: var(--selection); border-radius: 7px; padding: 8px 12px; white-space: pre-wrap; }
.chat-msg.assistant { align-self: stretch; line-height: 1.7; }
.chat-markdown h1, .chat-markdown h2, .chat-markdown h3 { font-size: 13px; font-weight: 600; margin: 10px 0 4px; }
.chat-markdown ul { padding-left: 18px; margin: 4px 0; }
.chat-markdown p { margin: 4px 0; }
.tool-call { display: flex; align-items: center; gap: 6px; font-size: 12px; color: var(--text-secondary); padding: 2px 0; min-width: 0; }
.tool-call .tool-detail { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; flex: 1; }
.tool-call .tool-state { margin-left: auto; color: var(--text-tertiary); flex-shrink: 0; }
.tool-call .tool-state.running { color: var(--accent); }
.tool-call.is-error .tool-state { color: var(--danger); }
.chat-refs { display: flex; flex-direction: column; align-items: flex-start; gap: 6px; }
.chat-ref { border: 1px solid var(--border); border-radius: 5px; padding: 3px 8px; font-size: 12px; color: var(--accent); background: var(--bg-window); max-width: 100%; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.chat-empty { margin: auto; display: flex; flex-direction: column; align-items: center; gap: 8px; text-align: center; color: var(--text-secondary); }
.chat-empty h3 { font-size: 13px; color: var(--text-primary); margin: 0; }
.chat-suggestion { display: flex; gap: 8px; align-items: flex-start; text-align: left; width: 100%; border: 1px solid var(--border); border-radius: 7px; padding: 8px 12px; background: var(--bg-window); color: var(--text-primary); }
.chat-error { border: 1px solid var(--danger); border-radius: 7px; padding: 12px; background: var(--bg-window); }
.chat-error-title { display: flex; align-items: center; gap: 6px; color: var(--danger); font-weight: 600; }
.chat-error p { color: var(--text-secondary); font-size: 12px; overflow-wrap: anywhere; }
.chat-error-actions { display: flex; gap: 8px; flex-wrap: wrap; }
.chat-composer { margin: 8px 12px 12px; border: 1px solid var(--border); border-radius: 7px; background: var(--bg-window); padding: 8px 10px; }
.chat-input { width: 100%; border: 0; resize: none; background: transparent; color: var(--text-primary); font: inherit; outline: none; min-height: 36px; }
.chat-composer-row { display: flex; align-items: center; justify-content: space-between; gap: 8px; }
.chat-send { width: 28px; height: 28px; border-radius: 50%; border: 0; background: var(--accent); color: var(--on-accent); display: grid; place-items: center; }
.chat-send:disabled { opacity: 0.4; }
```

`main.tsx` で `./styles/chat.css` を `layout.css` の後に読み込む。`api.ts` の `getChatMessages` を `invoke<UIMessage[]>`、`saveChatMessages` の引数を `UIMessage[]` にする (`import type { UIMessage } from "ai"`)。

Run: `npm test -- --run` → PASS。`npm run build` → エラーなし。

- [ ] **Step 6: コミット**

```bash
cd app && npm test -- --run && npm run build && cd ..
git add app/package.json app/package-lock.json app/src
git commit -m "feat(ui): render chat conversations with assistant-ui"
```

---

### Task 8: チャットパネルとアプリへの組み込み

**Files:**
- Create: `app/src/chat/useChatEndpoint.ts`、`app/src/chat/ChatPanel.tsx`、`app/src/chat/ThreadMenu.tsx`、`app/src/chat/ChatPanel.test.tsx`
- Modify: `app/src/types.ts`、`app/src/api.ts`、`app/src/App.tsx`、`app/src/App.test.tsx`、`app/src/views/FeedView.tsx`、`app/src/views/DigestView.tsx`、`app/src/views/SettingsView.tsx`、それぞれのテスト、`app/src/styles/chat.css`、`app/src/styles/layout.css`
- Test: `app/src/chat/ChatPanel.test.tsx`、`App.test.tsx`、`FeedView.test.tsx`、`DigestView.test.tsx`

**Interfaces:**
- Consumes: Task 3 の chat コマンド、Task 6 の `chat_endpoint`、`restart_chat`、`chat-endpoint-changed`、Task 7 の `ChatThread`、`ChatContext`
- Produces:
  - `types.ts`: `type ChatEndpoint = { status: "starting" } | { status: "ready"; url: string; token: string } | { status: "error"; kind: "node_missing" | "node_too_old" | "agent_missing" | "start_failed"; message: string }`
  - `api.ts`: `chatEndpoint()`、`restartChat()`、`EVENTS.chat = "chat-endpoint-changed"`
  - `ChatPanel` props: `{ scope: ChatScope; period: ChatPeriod | null; attachments: ChatAttachment[]; onRemoveAttachment(i: number): void; onClose(): void; onOpenSettings(tab: "llm"): void }`
  - `FeedView` / `DigestView` に追加する props: `chatOpen: boolean`、`onToggleChat(): void`、`onAskChat(a: ChatAttachment): void`、`onPeriodChange(p: ChatPeriod): void`
  - `SettingsView` に `initialTab?: "sources" | "topics" | "llm" | "schedule"` を戻す (チャットの「LLM設定」「Node のパスを設定」から LLM タブを開くため)

- [ ] **Step 1: パネルの失敗するテストを書く**

`app/src/chat/ChatPanel.test.tsx`。`ChatThread` は Task 7 で確かめたので、ここでは `vi.mock("./ChatThread", ...)` で受け取った props を表示する代役に替え、パネルの責務 (接続先の状態、スレッドの作成と切替と削除、文脈の表示) だけを確かめる。

```tsx
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { emitEvent, setupIpc } from "../test/ipc";
import { ChatPanel } from "./ChatPanel";

let lastThreadProps: any = null;
vi.mock("./ChatThread", () => ({
  ChatThread: (p: any) => {
    lastThreadProps = p;
    return <div data-testid="thread">{p.chatKey}|{p.initialMessages.length}|{p.provider}</div>;
  },
}));

const READY = { status: "ready", url: "http://127.0.0.1:4321", token: "tok" };
const SETTINGS = { "llm.chat_provider": "ollama", "ollama.model": "gemma4:26b" };
const THREADS = [
  { id: 2, title: "認知症の今週の動き", created_at: "2026-10-08T00:00:00Z", updated_at: "2026-10-09T00:00:00Z" },
  { id: 1, title: "GitHub の話題", created_at: "2026-10-07T00:00:00Z", updated_at: "2026-10-07T00:00:00Z" },
];

function base(over: Record<string, (a: any) => unknown> = {}) {
  return setupIpc({
    chat_endpoint: () => READY,
    get_settings: () => SETTINGS,
    list_chat_threads: () => THREADS,
    get_chat_messages: () => [{ id: "u", role: "user", parts: [{ type: "text", text: "x" }] }],
    ...over,
  });
}

function renderPanel(over: Partial<React.ComponentProps<typeof ChatPanel>> = {}) {
  const props = {
    scope: { kind: "category", category: "medcare", label: "医療介護" } as const,
    period: { view: "feed", range: "week" } as const,
    attachments: [],
    onRemoveAttachment: vi.fn(), onClose: vi.fn(), onOpenSettings: vi.fn(),
    ...over,
  };
  render(<ChatPanel {...(props as any)} />);
  return props;
}

describe("ChatPanel", () => {
  it("starts on a new thread with the context chips and the default provider", async () => {
    base();
    renderPanel();
    expect(await screen.findByTestId("thread")).toHaveTextContent("|0|ollama");
    expect(screen.getByRole("button", { name: "新しいスレッド" })).toHaveAttribute("aria-expanded", "false");
    const chips = screen.getByRole("group", { name: "文脈" });
    expect(within(chips).getByText("医療介護")).toBeInTheDocument();
    expect(within(chips).getByText("期間: 1週間")).toBeInTheDocument();
    expect(lastThreadProps.modelLabel).toBe("Ollama · gemma4:26b");
    expect(lastThreadProps.context).toEqual({ scope: { kind: "category", category: "medcare", label: "医療介護" }, period: { view: "feed", range: "week" }, attachments: [] });
  });

  it("creates the thread on the first save, titled from the question", async () => {
    const ipc = base({ create_chat_thread: (a) => ({ id: 9, title: a.title, created_at: "x", updated_at: "x" }), save_chat_messages: () => null });
    renderPanel();
    await screen.findByTestId("thread");
    const long = "今週の認知症に関する記事をまとめて。新薬と介護現場の話題を分けて、それぞれ3行で";
    await lastThreadProps.onPersist([
      { id: "u", role: "user", parts: [{ type: "text", text: long }] },
      { id: "a", role: "assistant", parts: [{ type: "text", text: "..." }] },
    ]);
    expect(ipc.last("create_chat_thread")).toEqual({ title: long.slice(0, 30) });
    expect(ipc.last("save_chat_messages").threadId).toBe(9);
    // 2回目は作らずに保存だけ
    await lastThreadProps.onPersist([]);
    expect(ipc.count("create_chat_thread")).toBe(1);
    expect(ipc.count("save_chat_messages")).toBe(2);
  });

  it("switches to a saved thread and deletes one after confirming", async () => {
    const ipc = base({ delete_chat_thread: () => null });
    renderPanel();
    await screen.findByTestId("thread");
    const user = userEvent.setup();
    await user.click(screen.getByRole("button", { name: "履歴" }));
    await user.click(screen.getByRole("button", { name: "認知症の今週の動き" }));
    await waitFor(() => expect(screen.getByTestId("thread")).toHaveTextContent("thread-2|1|"));
    expect(ipc.last("get_chat_messages")).toEqual({ threadId: 2 });
    await user.click(screen.getByRole("button", { name: "履歴" }));
    await user.click(screen.getByRole("button", { name: "認知症の今週の動き を削除" }));
    expect(ipc.count("delete_chat_thread")).toBe(0);
    await user.click(screen.getByRole("button", { name: "削除する" }));
    expect(ipc.last("delete_chat_thread")).toEqual({ id: 2 });
    // 開いていたスレッドを消したら新しいスレッドに戻る
    await waitFor(() => expect(screen.getByTestId("thread")).toHaveTextContent("|0|"));
  });

  it("starts a new thread with the new-thread button", async () => {
    base();
    renderPanel();
    await screen.findByTestId("thread");
    const user = userEvent.setup();
    await user.click(screen.getByRole("button", { name: "履歴" }));
    await user.click(screen.getByRole("button", { name: "GitHub の話題" }));
    await waitFor(() => expect(screen.getByTestId("thread")).toHaveTextContent("thread-1"));
    await user.click(screen.getByRole("button", { name: "新規スレッド" }));
    expect(screen.getByTestId("thread")).toHaveTextContent("|0|");
  });

  it("shows why the agent cannot start, with settings and retry", async () => {
    const ipc = base({
      chat_endpoint: () => ({ status: "error", kind: "node_missing", message: "Node が見つかりません (node)。" }),
      restart_chat: () => null,
    });
    const props = renderPanel();
    expect(await screen.findByText("エージェントを起動できません")).toBeInTheDocument();
    expect(screen.getByText("Node が見つかりません (node)。")).toBeInTheDocument();
    expect(screen.queryByTestId("thread")).toBeNull();
    const user = userEvent.setup();
    await user.click(screen.getByRole("button", { name: "Node のパスを設定" }));
    expect(props.onOpenSettings).toHaveBeenCalledWith("llm");
    await user.click(screen.getByRole("button", { name: "再試行" }));
    expect(ipc.count("restart_chat")).toBe(1);
  });

  it("follows endpoint changes", async () => {
    let endpoint: any = { status: "starting" };
    base({ chat_endpoint: () => endpoint });
    renderPanel();
    expect(await screen.findByText("エージェントを起動しています…")).toBeInTheDocument();
    endpoint = READY;
    await emitEvent("chat-endpoint-changed");
    expect(await screen.findByTestId("thread")).toBeInTheDocument();
  });

  it("lists and removes attachments", async () => {
    base();
    const props = renderPanel({ attachments: [{ type: "item", id: 12, title: "認知症の新薬" }] as any });
    await screen.findByTestId("thread");
    await userEvent.setup().click(screen.getByRole("button", { name: "認知症の新薬 を文脈から外す" }));
    expect(props.onRemoveAttachment).toHaveBeenCalledWith(0);
  });

  it("closes", async () => {
    base();
    const props = renderPanel();
    await userEvent.setup().click(screen.getByRole("button", { name: "チャットを閉じる" }));
    expect(props.onClose).toHaveBeenCalled();
  });
});
```

Run → FAIL

- [ ] **Step 2: 型、ラッパー、フックを書く**

`types.ts` に `ChatEndpoint`。`api.ts`:

```ts
  chatEndpoint: () => invoke<ChatEndpoint>("chat_endpoint"),
  restartChat: () => invoke<void>("restart_chat"),
```

`EVENTS` に `chat: "chat-endpoint-changed"`。

`app/src/chat/useChatEndpoint.ts`:

```ts
import { useCallback, useEffect, useState } from "react";
import { api, EVENTS } from "../api";
import { useTauriEvent } from "../hooks/useTauriEvent";
import { errorMessage, type ChatEndpoint } from "../types";

export function useChatEndpoint() {
  const [endpoint, setEndpoint] = useState<ChatEndpoint>({ status: "starting" });
  const refresh = useCallback(() => {
    api.chatEndpoint()
      .then(setEndpoint)
      .catch((e) => setEndpoint({ status: "error", kind: "start_failed", message: errorMessage(e) }));
  }, []);
  useEffect(refresh, [refresh]);
  useTauriEvent(EVENTS.chat, refresh);
  const restart = useCallback(() => {
    setEndpoint({ status: "starting" });
    api.restartChat().catch((e) => setEndpoint({ status: "error", kind: "start_failed", message: errorMessage(e) }));
  }, []);
  return { endpoint, restart };
}
```

- [ ] **Step 3: `ThreadMenu` と `ChatPanel` を実装する**

`app/src/chat/ThreadMenu.tsx`: スレッドの一覧 (ボタン、名前はタイトル)。各行に「<タイトル> を削除」(ゴミ箱)。押すとその行が「削除しますか?」「削除する」「キャンセル」になる (設定のソース一覧と同じ2段階。1行だけ)。一覧が空なら「保存したスレッドはありません」。

```tsx
import { Trash2 } from "lucide-react";
import { useState } from "react";
import type { ChatThread as Thread } from "../types";

interface Props {
  threads: Thread[];
  currentId: number | null;
  onSelect(id: number): void;
  onDelete(id: number): void;
}

export function ThreadMenu({ threads, currentId, onSelect, onDelete }: Props) {
  const [confirming, setConfirming] = useState<number | null>(null);
  if (threads.length === 0) return <div className="thread-menu empty text-secondary">保存したスレッドはありません</div>;
  return (
    <ul className="thread-menu" aria-label="スレッド">
      {threads.map((t) => (
        <li key={t.id} className={t.id === currentId ? "current" : undefined}>
          {confirming === t.id ? (
            <div className="thread-confirm">
              <span>削除しますか?</span>
              <button className="btn btn-danger" onClick={() => { setConfirming(null); onDelete(t.id); }}>削除する</button>
              <button className="btn" onClick={() => setConfirming(null)}>キャンセル</button>
            </div>
          ) : (
            <>
              <button className="thread-item" onClick={() => onSelect(t.id)}>{t.title}</button>
              <button className="icon-btn" aria-label={`${t.title} を削除`} onClick={() => setConfirming(t.id)}><Trash2 size={13} /></button>
            </>
          )}
        </li>
      ))}
    </ul>
  );
}
```

`app/src/chat/ChatPanel.tsx`:

```tsx
import type { UIMessage } from "ai";
import { ChevronDown, History, PlugZap, SquarePen, X } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { api } from "../api";
import { FEED_RANGES } from "../lib/filters";
import { errorMessage, type ChatThread as Thread } from "../types";
import { ErrorLine } from "../components/ErrorLine";
import { ChatThread } from "./ChatThread";
import type { ChatAttachment, ChatPeriod, ChatScope, ProviderId } from "./context";
import { ThreadMenu } from "./ThreadMenu";
import { useChatEndpoint } from "./useChatEndpoint";

interface Props {
  scope: ChatScope;
  period: ChatPeriod | null;
  attachments: ChatAttachment[];
  onRemoveAttachment(i: number): void;
  onClose(): void;
  onOpenSettings(tab: "llm"): void;
}

const PERIOD_LABEL = { day: "今日", week: "今週", month: "今月" } as const;
const TITLE_CHARS = 30;
const SUGGESTIONS: Record<string, string[]> = {
  default: ["今日の主な動きを3つにまとめて", "先週と比べて新しく出てきた話題は?", "気になる記事を3つ選んで理由を説明して"],
  github: ["今日のGitHubで伸びたリポジトリを3つ選んで理由を説明して", "エージェント関連のリポジトリだけまとめて", "先週と比べて新しく出てきた話題は?"],
  medcare: ["今週の認知症関連の動きをまとめて", "介護報酬の議論の論点を整理して", "新薬や治療法の話題だけ抜き出して"],
};

function periodChip(p: ChatPeriod | null): string | null {
  if (!p) return null;
  if (p.view === "digest") return `期間: ${PERIOD_LABEL[p.period]}`;
  return `期間: ${FEED_RANGES.find((r) => r.id === p.range)?.label ?? p.range}`;
}

function scopeChip(s: ChatScope): string {
  return s.kind === "all" ? "すべて" : s.label;
}

function firstUserText(messages: UIMessage[]): string {
  const m = messages.find((x) => x.role === "user");
  const part = m?.parts.find((p) => p.type === "text") as { text?: string } | undefined;
  return (part?.text ?? "").trim();
}

export function ChatPanel(props: Props) {
  const { endpoint, restart } = useChatEndpoint();
  const [threads, setThreads] = useState<Thread[]>([]);
  const [current, setCurrent] = useState<{ id: number | null; key: string; messages: UIMessage[] }>({ id: null, key: "new-0", messages: [] });
  const [menuOpen, setMenuOpen] = useState(false);
  const [provider, setProvider] = useState<ProviderId>("ollama");
  const [model, setModel] = useState("");
  const [error, setError] = useState<string | null>(null);

  const loadThreads = useCallback(() => {
    api.listChatThreads().then(setThreads).catch((e) => setError(errorMessage(e)));
  }, []);
  useEffect(() => {
    loadThreads();
    api.getSettings()
      .then((s) => {
        setProvider(s["llm.chat_provider"] === "claude" ? "claude" : "ollama");
        setModel(s["ollama.model"] ?? "");
      })
      .catch((e) => setError(errorMessage(e)));
  }, [loadThreads]);

  const newThread = () => {
    setMenuOpen(false);
    setCurrent((c) => ({ id: null, key: `new-${Number(c.key.split("-")[1] ?? 0) + 1}`, messages: [] }));
  };
  const openThread = (id: number) => {
    setMenuOpen(false);
    api.getChatMessages(id)
      .then((messages) => setCurrent({ id, key: `thread-${id}`, messages }))
      .catch((e) => setError(errorMessage(e)));
  };
  const deleteThread = (id: number) => {
    api.deleteChatThread(id)
      .then(() => {
        if (current.id === id) newThread();
        loadThreads();
      })
      .catch((e) => setError(errorMessage(e)));
  };

  // スレッドは最初の回答を保存するときに作る。空のスレッドを残さないため。
  const persist = useCallback(async (messages: UIMessage[]) => {
    try {
      let id = current.id;
      if (id === null) {
        const title = firstUserText(messages).slice(0, TITLE_CHARS) || "新しいスレッド";
        const t = await api.createChatThread(title);
        id = t.id;
        setCurrent((c) => ({ ...c, id: t.id }));
      }
      await api.saveChatMessages(id, messages);
      loadThreads();
    } catch (e) {
      setError(errorMessage(e));
    }
  }, [current.id, loadThreads]);

  const title = threads.find((t) => t.id === current.id)?.title ?? "新しいスレッド";
  const suggestions = SUGGESTIONS[props.scope.kind === "category" ? props.scope.category : "default"] ?? SUGGESTIONS.default;
  const chips = [scopeChip(props.scope), periodChip(props.period)].filter(Boolean) as string[];

  return (
    <aside className="chat-panel" aria-label="チャット">
      <header className="chat-header">
        <button className="chat-title" aria-expanded={menuOpen} onClick={() => setMenuOpen((v) => !v)}>
          {title}<ChevronDown size={13} aria-hidden />
        </button>
        <button className="icon-btn" aria-label="新規スレッド" onClick={newThread}><SquarePen size={15} /></button>
        <button className="icon-btn" aria-label="履歴" aria-expanded={menuOpen} onClick={() => setMenuOpen((v) => !v)}><History size={15} /></button>
        <button className="icon-btn" aria-label="チャットを閉じる" onClick={props.onClose}><X size={15} /></button>
      </header>
      {menuOpen && <ThreadMenu threads={threads} currentId={current.id} onSelect={openThread} onDelete={deleteThread} />}
      <div className="chat-context" role="group" aria-label="文脈">
        <span className="text-tertiary">文脈</span>
        {chips.map((c) => <span key={c} className="chip">{c}</span>)}
        {props.attachments.map((a, i) => {
          const label = a.type === "item" ? a.title : `${a.label} の要約`;
          return (
            <span key={`${a.type}-${i}`} className="chip">
              {label}
              <button className="chip-x" aria-label={`${label} を文脈から外す`} onClick={() => props.onRemoveAttachment(i)}><X size={11} /></button>
            </span>
          );
        })}
      </div>
      <ErrorLine message={error} />
      {endpoint.status === "starting" && <div className="chat-state text-secondary">エージェントを起動しています…</div>}
      {endpoint.status === "error" && (
        <div className="chat-state-box">
          <div className="chat-error-title"><PlugZap size={15} aria-hidden />エージェントを起動できません</div>
          <p>{endpoint.message}</p>
          <p className="text-secondary">ダイジェストとフィードはこのまま使えます。</p>
          <div className="chat-error-actions">
            <button className="btn btn-primary" onClick={() => props.onOpenSettings("llm")}>Node のパスを設定</button>
            <button className="btn" onClick={restart}>再試行</button>
          </div>
        </div>
      )}
      {endpoint.status === "ready" && (
        <ChatThread
          key={`${current.key}|${endpoint.url}`}
          endpoint={{ url: endpoint.url, token: endpoint.token }}
          chatKey={current.key}
          initialMessages={current.messages}
          provider={provider}
          modelLabel={model ? `Ollama · ${model}` : "Ollama"}
          context={{ scope: props.scope, period: props.period, attachments: props.attachments }}
          onProviderChange={setProvider}
          onPersist={persist}
          onOpenSettings={() => props.onOpenSettings("llm")}
          suggestions={suggestions}
        />
      )}
    </aside>
  );
}
```

`key` に `current.key` を使うので、スレッドを切り替えると `ChatThread` (と `useChat`) が作り直される。初回保存で `current.id` が決まっても `key` は変えない (会話の表示を保つため)。

`chat.css` に足す:

```css
.chat-panel { width: 400px; flex-shrink: 0; border-left: 1px solid var(--border); background: var(--bg-subtle); display: flex; flex-direction: column; min-height: 0; position: relative; }
.chat-header { display: flex; align-items: center; gap: 6px; padding: 12px 12px 8px 16px; border-bottom: 1px solid var(--border); }
.chat-title { flex: 1; min-width: 0; display: flex; align-items: center; gap: 4px; font-weight: 600; background: none; border: 0; color: var(--text-primary); text-align: left; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.chat-context { display: flex; flex-wrap: wrap; align-items: center; gap: 6px; padding: 8px 16px; font-size: 12px; }
.chip { display: inline-flex; align-items: center; gap: 4px; border: 1px solid var(--border); border-radius: 999px; padding: 2px 8px; background: var(--bg-window); max-width: 220px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.chip-x { border: 0; background: none; padding: 0; color: var(--text-tertiary); display: inline-grid; }
.chat-state { padding: 24px 16px; }
.chat-state-box { margin: 12px 16px; border: 1px solid var(--border); border-radius: 7px; padding: 12px; background: var(--bg-window); }
.chat-state-box p { font-size: 12px; overflow-wrap: anywhere; }
.thread-menu { position: absolute; top: 46px; left: 12px; right: 12px; z-index: 10; list-style: none; margin: 0; padding: 4px; background: var(--bg-window); border: 1px solid var(--border); border-radius: 7px; box-shadow: 0 6px 20px rgba(0, 0, 0, 0.15); max-height: 320px; overflow-y: auto; }
.thread-menu li { display: flex; align-items: center; gap: 4px; }
.thread-menu li.current .thread-item { font-weight: 600; }
.thread-item { flex: 1; min-width: 0; text-align: left; background: none; border: 0; padding: 6px 8px; border-radius: 5px; color: var(--text-primary); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.thread-item:hover { background: var(--bg-subtle); }
.thread-confirm { display: flex; align-items: center; gap: 6px; padding: 4px 8px; flex-wrap: wrap; font-size: 12px; }
.thread-menu.empty { padding: 12px; }
.icon-btn { width: 26px; height: 26px; display: inline-grid; place-items: center; border: 0; border-radius: 5px; background: none; color: var(--text-secondary); flex-shrink: 0; }
.icon-btn:hover { background: var(--bg-window); color: var(--text-primary); }
```

`.icon-btn` はこのタスクで新しく作るクラス (既存の CSS には無い)。`rgba(0, 0, 0, 0.15)` の影は、既存の `theme.css` に影のトークンがあればそれを使う。`.btn-danger` は既存の `layout.css` の `.cell-confirm .btn-danger` を一般化して `.btn-danger { color: var(--danger); }` にする (既存の表示が変わらないことを確かめる)。

Run: `npm test -- --run src/chat/ChatPanel.test.tsx` → PASS

- [ ] **Step 4: ビューとアプリに組み込む (先にテスト)**

テストを足す (既存のテストの書き方に合わせ、既存の props を渡しているテストには新しい props を足す。既存の assertion は弱めない):

- `FeedView.test.tsx`:
  - ツールバーに「チャットを開く」ボタン (`aria-pressed=false`)。`chatOpen` のときは「チャットを閉じる」(`aria-pressed=true`)。押すと `onToggleChat`。
  - 記事行の「チャットで質問」(名前は「<タイトル> についてチャットで質問」) を押すと `onAskChat({ type: "item", id, title })`。行のクリック (記事を開く) は起きない。
  - 期間を変えると `onPeriodChange({ view: "feed", range })` が呼ばれる。マウント時にも1回呼ばれる。
- `DigestView.test.tsx`:
  - 同じ開閉ボタン。
  - 節見出しの「チャットで質問」(名前は「<ラベル>の要約についてチャットで質問」、要約がある節だけ) が `onAskChat({ type: "digest", period, date, scope: "category:<c>" または "topic:<id>", label })`。
  - 期間の切替とマウントで `onPeriodChange({ view: "digest", period, date })`。
- `App.test.tsx` (ハンドラーに `chat_endpoint`、`list_chat_threads`、`get_settings` を足す):
  - 開閉ボタンでパネルが開き (`complementary` の名前「チャット」)、閉じるボタンで閉じる。
  - フィードで記事の「チャットで質問」を押すと、パネルが開き、文脈にその記事のチップが出る。同じ記事を2回押してもチップは1つ。
  - パネルの「Node のパスを設定」(接続先がエラーのとき) で設定の LLM タブが開く。
  - パネルを開いたまま設定を開くと、パネルは隠れる (設定画面は全幅)。設定から戻るとまた出る。
- `SettingsView` の既存テストに、`initialTab="llm"` で LLM タブが選ばれて開くテストを1件足す。

Run → FAIL

- [ ] **Step 5: 組み込みを実装する**

- `FeedView` と `DigestView`: ツールバーの右端に開閉ボタン (`MessageSquare` アイコン、`aria-label` は開いているかで「チャットを閉じる」/「チャットを開く」、`aria-pressed`)。`useEffect` で `period` (または `range`) と日付が変わるたびに `onPeriodChange` を呼ぶ。
- `FeedView` の記事行: 行の右端、スターの隣に `MessageSquarePlus` のボタン。ホバーまたはフォーカスで表示する (`.row-actions` を `opacity: 0`、行の `:hover` と `:focus-within` で 1)。`onClick` で `e.stopPropagation()` してから `onAskChat`。
- `DigestView` の節見出し: 「再生成」などの右側に同じアイコンのボタン。要約が `done` の節だけ出す。
- `SettingsView`: `initialTab` を受け取り、初期のタブにする。`key` で作り直すため、App は設定を開くたびに `settingsKey` を増やす。
- `App.tsx`:
  - state: `chatOpen`、`chatPeriod: ChatPeriod | null`、`attachments: ChatAttachment[]`、`settingsTab`。
  - `scope` は `selection` と `topics` から作る (`{ kind: "all" }` / `{ kind: "category", category, label: カテゴリーの表示名 }` / `{ kind: "topic", topicId, label: トピック名 }`)。
  - `askChat(a)`: 同じもの (`type` と `id`、または `type` と `scope` と `period` と `date`) が無ければ足し、`chatOpen` を true にする。
  - 選択 (カテゴリーやトピック) が変わったら添付を空にする。
  - レイアウト: `<main className="main">` の中を `<div className="content">{ビュー}</div>` と、設定以外のときの `{chatOpen && <ChatPanel .../>}` の横並びにする。`.main` を `display: flex; flex-direction: row`、`.content` を `flex: 1; min-width: 0; display: flex; flex-direction: column` にする (今の `.main` の縦並びのスタイルは `.content` に移す)。`.shell-errors` は `.content` の中の先頭に置く。
  - `onOpenSettings("llm")` は `settingsTab` を `"llm"` にして設定を開く。サイドバーの設定ボタンは今までどおりソースのタブ。
- 960 幅でパネルを開くと、本文は 960 - 220 - 400 = 340 px になる。フィードのツールバーは Plan 2b で折り返すようにしてあるが、本文が狭いと崩れる場合がある。本文の幅が 520 px 未満になるときはパネルを本文の上に重ねて表示する (`.app.narrow .chat-panel { position: absolute; right: 0; top: 0; bottom: 0; box-shadow: ... }`)。幅は `ResizeObserver` ではなく `window.innerWidth < 1140` の `matchMedia` で判定する (`(max-width: 1139px)`)。CSS の `@media (max-width: 1139px)` だけで済むならそうする。

Run: `npm test -- --run && npm run build` → PASS

- [ ] **Step 6: コミット**

```bash
cd app && npm test -- --run && npm run build && cd ..
git add app/src
git commit -m "feat(ui): add the chat panel with threads, context and ask-in-chat"
```

---

### Task 9: バンドル、確認、README

**Files:**
- Create: `app/src-tauri/tauri.bundle.json`
- Modify: `app/package.json`、`README.md`

**Interfaces:**
- Consumes: Task 1〜8 の全部
- Produces: `npm run bundle` (app で実行) で作った `InfoDesk.app` が、`Contents/Resources/agent/agent.mjs` を含み、チャットが動く状態

- [ ] **Step 1: バンドルの設定**

`app/src-tauri/tauri.bundle.json`:

```json
{
  "bundle": {
    "resources": { "../../agent/dist/agent.mjs": "agent/agent.mjs" }
  }
}
```

`app/package.json` の `scripts` に:

```json
"bundle": "npm --prefix ../agent run build && tauri build --debug --bundles app --config src-tauri/tauri.bundle.json"
```

`bundle.resources` の写し先の書き方 (`{ "元": "先" }`) は Tauri 2 の設定スキーマ (`app/src-tauri/gen/schemas/` または `node_modules/@tauri-apps/cli/config.schema.json` の `resources`) で確かめる。

Run:

```bash
cd app && npm run bundle
ls ../target/debug/bundle/macos/InfoDesk.app/Contents/Resources/agent/agent.mjs
../target/debug/bundle/macos/InfoDesk.app/Contents/MacOS/infodesk-app --cli --db /tmp/t.db config list --json
```

Expected: ファイルがあり、最後のコマンドが JSON を出す。実行ファイル名が `infodesk-app` でない場合は `ls Contents/MacOS` で確かめた名前を使う。

- [ ] **Step 2: 自動で確かめられることを確かめる**

```bash
cargo test --workspace && cargo clippy --all-targets -- -D warnings && cargo fmt --check
cd app && npm test -- --run && npm run build && cd ..
cd agent && npm test -- --run && npm run build && node dist/agent.mjs --check && cd ..
```

サイドカーを単体で起動し、本物の CLI と Ollama で1回質問する (Ollama が動いている場合):

```bash
APP=target/debug/bundle/macos/InfoDesk.app/Contents/MacOS/infodesk-app
DB="$HOME/Library/Application Support/jp.infodesk.app/infodesk.db"
INFODESK_CLI=$APP INFODESK_CLI_ARG=--cli INFODESK_DB="$DB" INFODESK_AGENT_TOKEN=t INFODESK_AGENT_PORT=47123 node agent/dist/agent.mjs &
sleep 2
curl -sN -X POST http://127.0.0.1:47123/chat -H 'Authorization: Bearer t' -H 'Content-Type: application/json' \
  -d '{"provider":"ollama","context":null,"messages":[{"id":"u1","role":"user","parts":[{"type":"text","text":"今日の医療介護の記事を3つ挙げて"}]}]}' | head -40
kill %1
```

Expected: `data: {"type":"tool-input-available",...,"toolName":"search_articles"...}` と `text-delta` が流れる。Claude でも同じ要求を `"provider":"claude"` で送り、`mcp__infodesk__search_articles` のツール部品と回答が流れることを確かめる (Claude CLI にログイン済みの場合)。動かない場合は、出力の最後の数行と原因の見立てを報告に書く。この確認は利用者の DB を読むだけで書き換えない。

- [ ] **Step 3: 画面で確かめる (手動)**

`open target/debug/bundle/macos/InfoDesk.app` で起動し、デザイン `design/exports/4-feed-chat-light.png`、`4-feed-chat-dark.png`、`12-chat-states.png` と並べて確かめる。画面を操作できない場合は、この一覧を報告に書いて利用者に渡す。

1. フィードのツールバーの開閉ボタンでパネルが開き、文脈に表示中のカテゴリーと期間が出る。
2. 空のスレッドに質問例が出る。押すと送られる。
3. 回答がストリーミングで表示され、ツール行 (記事検索、本文取得) が「実行中」から「完了」になる。回答の下に参照した記事のリンクが出て、押すと既定のブラウザで開く。
4. 記事行の「チャットで質問」で、その記事が文脈に入る。「この記事を詳しく要約して」で本文取得が動く。
5. 「履歴」でスレッドを切り替えられ、前の会話が表示される。削除は確認のあとで消える。アプリを再起動しても残っている。
6. 下のモデル選択で Claude に切り替えて質問できる。Web検索が使われた場合、ツール行が「Web検索」になる。
7. Ollama を止めて質問すると「Ollama に接続できません」が出て、質問の吹き出しは残る。「再試行」「Claude で送る」「LLM設定」が動く。
8. 設定 `node.bin` を存在しないパスにして「再試行」(またはアプリを再起動) すると「エージェントを起動できません」が出る。「Node のパスを設定」で設定の LLM タブが開く。戻したあと「再試行」で使えるようになる。
9. 960x640 でパネルを開いても、本文とパネルが重ならずに読める (狭いときはパネルが上に重なる)。
10. アプリを終了したあと `pgrep -fl agent.mjs` に何も出ない。

- [ ] **Step 4: README を更新する**

`README.md` に「## チャット」の節を足す:

````markdown
## チャット

右のパネルで、収集した記事と要約についてエージェントに質問できる。エージェントは Node のサイドカー (`agent/`) で動き、アプリが起動する。

- Node 22 以上が必要。場所は設定の LLM タブ「Node のパス」で指定する (既定 `node`。`/opt/homebrew/bin` と `/usr/local/bin` も探す)。
- LLM はパネル下のモデル選択で Ollama と Claude を切り替える。Claude は `claude` CLI (設定「claude コマンドのパス」) を使い、モデルは `sonnet`。
- Ollama で Web検索を使うには、設定に ollama.com の API キーを入れる。
- 開発時は `cd agent && npm install && npm run build` のあと `cd app && npm run tauri dev`。バンドルは `cd app && npm run bundle`。
- サイドカーのツールは、アプリ本体を `--cli` 付きで起動して `infodesk` CLI として使う。例: `InfoDesk.app/Contents/MacOS/infodesk-app --cli --db <DB> items --json`。
````

- [ ] **Step 5: コミット**

```bash
git add app/src-tauri/tauri.bundle.json app/package.json README.md
git commit -m "feat(app): bundle the chat agent and document the chat"
```

確かめた結果 (各項目の実際の挙動、未確認の項目) をコミットメッセージの本文に書く。

---

## Plan 3 のあとに残ること

- チャットの回答を要約として保存する機能、スレッドの検索、エクスポートは範囲外。
- `@tauri-apps/plugin-opener` の JS 依存と `opener:default`、`autostart:default` の権限の整理 (Plan 2b の最終レビューで先送り)。
