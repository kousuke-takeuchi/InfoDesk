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
