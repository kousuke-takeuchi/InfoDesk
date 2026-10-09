# InfoDesk

技術系と医療介護系の情報を集め、日次、週次、月次の日本語要約を作る。

設計は `docs/superpowers/specs/2026-10-06-infodesk-design.md` を参照。

## 構成

- `crates/core`: 収集、保存 (SQLite)、要約のライブラリ
- `crates/cli`: コマンド `infodesk`
- `app/`: Tauri アプリ (`app/src-tauri` がRust側、`app/src` が画面)

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

## アプリ

```bash
cd app && npm install && npm run tauri dev
```

アプリのDBは `~/Library/Application Support/jp.infodesk.app/infodesk.db`。CLIで同じDBを見るには `--db` で指定する。

画面は `app/src` (React)。テストは `cd app && npm test`。デザインは `design.pen` と `design/exports/`。

フィードの記事には、収集のときに見つけた画像を表示する (フィードの画像、なければ記事ページの og:image)。画像は表示のたびに元のサイトから読み込む。

## 設定

`infodesk config set <key> <value>` で変更する。

| キー | 既定値 | 内容 |
|---|---|---|
| `llm.items_provider` | `ollama` | 記事要約に使うLLM (`ollama` または `claude`) |
| `llm.digest_provider` | `ollama` | 期間要約に使うLLM |
| `ollama.base_url` | `http://localhost:11434` | OllamaのURL |
| `ollama.model` | `gemma4:26b` | Ollamaのモデル |
| `claude.bin` | `claude` | claude CLIのパス |
| `node.bin` | `node` | Node のパス (チャットのエージェントに使う) |
| `ollama.web_search_api_key` | (空) | ollama.com の API キー。空なら Ollama の Web検索は無効 |
| `summary.top_n` | `20` | カテゴリーごとに要約を付ける記事数 |

## チャット

右のパネルで、収集した記事と要約についてエージェントに質問できる。エージェントは Node のサイドカー (`agent/`) で動き、アプリが起動する。

回答中はエージェントの作業計画、思考、ツールの実行と経過時間を表示し、回答の後は1行にまとめる。

- Node 22 以上が必要。場所は設定の LLM タブ「Node のパス」で指定する (既定 `node`。`/opt/homebrew/bin` と `/usr/local/bin` も探す)。
- LLM はパネル下のモデル選択で Ollama と Claude を切り替える。Claude は `claude` CLI (設定「claude コマンドのパス」) を使い、モデルは `sonnet`。
- Ollama で Web検索を使うには、設定に ollama.com の API キーを入れる。
- 開発時は `cd agent && npm install && npm run build` のあと `cd app && npm run tauri dev`。バンドルは `cd app && npm run bundle`。
- サイドカーのツールは、アプリ本体を `--cli` 付きで起動して `infodesk` CLI として使う。例: `InfoDesk.app/Contents/MacOS/infodesk-app --cli --db <DB> items --json`。
- エージェントは外部の文章 (フィードの記事、取得したページ) を読む。その中に指示が隠れていると、会話や収集した記事から取った語で URL を取得したり Web検索したりすることがありうる。エージェントは何も書き込まず、設定や API キーにも手が届かないが、気になる場合は機密情報をチャットに書かない。

## テスト

```bash
cargo test
```

単体テストはネットワークに接続しない。`crates/core/tests/fixtures/` の実レスポンスは次で更新する。

```bash
cargo run -p infodesk-core --example fetch_fixtures
```
