# InfoDesk への貢献

[English (canonical)](CONTRIBUTING.md)

関心を持っていただきありがとうございます。InfoDesk は、技術系と医療介護系のニュースを集めて日本語のダイジェストを作る、ローカルファーストのデスクトップリーダー (Tauri 2 + React 19 + Rust) です。**仕様先行、デザイン先行、テスト先行**で開発しています。ここは短い案内で、詳しくは [`docs/internal/`](docs/internal/README.md) (日本語) を参照してください。

## 基本ルール

1. **仕様と計画が先。** 全体設計は `docs/superpowers/specs/2026-10-06-infodesk-design.md` です。計画は `docs/superpowers/plans/` に日付付きで置きます。挙動を変えるときは、先に仕様を更新します。
2. **UI の前にデザイン。** 画面は Pencil (`design.pen`、書き出しは `design/exports/`) で設計してから実装します。既存のトークンとコンポーネントを再利用し、新しいものは先にデザインへ足します。`design.pen` はテキストとして読まず、Pencil のツール経由でのみ扱います。
3. **TDD。** 失敗するテストを書き、通し、整理します。テストの無い挙動は存在しないものとして扱います。
4. **単体テストはネットワークに接続しません。** フィクスチャ (`crates/core/tests/fixtures/`)、モック HTTP サーバー、注入したクライアントを使います。フィクスチャ用の実レスポンスは `cargo run -p infodesk-core --example fetch_fixtures` で意図して更新します。
5. **UI は日本語で、i18n はありません。** 日本語の UI 文字列は React コンポーネントに直接書きます。無関係な変更のついでにメッセージカタログの層を足さないでください。
6. **ドキュメントは英語が正本**、日本語版は `*.ja.md` です。公開ドキュメントは両方を同じ構成で書きます。`docs/internal/` の開発者向け文書は日本語のみです。
7. **コミットに AI の帰属表記を付けません。** `Co-Authored-By:` などの行をコミットメッセージに足さないでください。
8. ドキュメントに**絵文字とエムダッシュを使いません**。

## ワークフロー

1. 触る領域の仕様の節を読みます。
2. 小さな修正を超える変更では、`docs/superpowers/plans/` に計画を足し、タスクごとに red、green のコミットで進めます。
3. コミット前にチェックを走らせます。

   ```bash
   cargo test                                   # Rust: core、CLI、アプリの各クレート
   cd app && npx vitest run                     # React UI
   cd agent && npx vitest run && npm run typecheck   # Node サイドカー
   ```

   agent の `npm test` は `TZ=Asia/Tokyo` を設定します。日付に依存するテストが落ちたときはこちらを使ってください。
4. コミットは小さく、内容が分かるようにします。`feat`、`fix`、`docs` などの Conventional Commits の接頭辞を使います。
5. どの仕様の節や計画に従ったかを書きます。

## どこに何があるか

| 領域 | パス |
|---|---|
| 収集、保存 (SQLite)、要約、ページ取得 | `crates/core` |
| `infodesk` CLI | `crates/cli` |
| Tauri シェル: コマンド、閲覧ウィンドウ、ジョブ、スケジューラー | `app/src-tauri` |
| React UI | `app/src` |
| チャットのエージェント (Node サイドカー) | `agent/` |
| デザインと書き出し | `design.pen`、`design/exports/` |
| 仕様と計画 | `docs/superpowers/` |

## 問題の報告

公開の課題管理はまだありません。セキュリティの問題は [SECURITY.md](SECURITY.md) を参照してください。問題を説明するときは、個人情報や API キーを含めないでください。

## ライセンス

貢献したものは、プロジェクトの他の部分と同じ MIT OR Apache-2.0 で提供されるものとします ([README](README.ja.md#ライセンス) を参照)。
