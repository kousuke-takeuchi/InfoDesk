<div align="center">

<img src="docs/images/icon.png" alt="InfoDesk" width="96">

# InfoDesk

**技術系と医療介護系のニュースを集め、日次・週次・月次の日本語ダイジェストを作る、ローカルファーストのデスクトップリーダー。**

InfoDesk は RSS、GitHub、Hugging Face、arXiv、Hacker News、Google News から記事を集め、
ローカルの LLM (Ollama) または `claude` CLI で上位の記事を要約し、ダイジェスト、フィード、チャットで見せます。
データはあなたの Mac 上の SQLite ファイルに保存されます。

![Status](https://img.shields.io/badge/status-alpha-orange)
![Platform](https://img.shields.io/badge/platform-macOS-lightgrey)
![Built with](https://img.shields.io/badge/built_with-Tauri_2_·_React_19_·_Rust-24C8DB)
![Docs](https://img.shields.io/badge/docs-EN_%7C_JA-informational)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-green)](#ライセンス)

[概要](docs/01-overview.ja.md) · [はじめに](docs/02-getting-started.ja.md) · [ドキュメント](#ドキュメント) · [CLI](docs/08-cli.ja.md) · [ロードマップ](docs/10-roadmap.ja.md) · [English](README.md)

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="docs/images/digest-week-dark.png">
  <img src="docs/images/digest-week-light.png" alt="InfoDesk の週次ダイジェスト" width="820">
</picture>

*週次ダイジェスト。スクリーンショットはサンプルデータです。*

</div>

---

## なぜ InfoDesk か

- 技術や医療介護のニュースを追うには、毎日たくさんのサイトを開く必要があります。InfoDesk は 1 か所に集めます。
- すべてを読むのは現実的ではありません。InfoDesk はカテゴリーごと、トピックごとに、日・週・月の単位で上位の記事を要約します。
- 要約は自分のマシンで動くモデルで書けます。クラウドサービスに送る必要はありません。

## 主な機能

- **収集**: 組み込みソース (RSS、GitHub、Hugging Face、arXiv、Hacker News、Google News)、自分で足す RSS、キーワード付きのトピック。[ソースとトピック](docs/03-sources-and-topics.ja.md)を参照。
- **日本語ダイジェスト**: 日・週・月の要約をカテゴリー別とトピック別に作成。期間の概観、見出し、番号付きの要点、画像カードを表示。[ダイジェスト](docs/04-digests.ja.md)を参照。
- **フィードとリーダー**: 期間、ソース、未読、スター、検索で絞り込み。記事はアプリ内の閲覧ウィンドウか既定のブラウザで開く。[フィードとリーダー](docs/05-feed-and-reader.ja.md)を参照。
- **AI チャット**: 収集した記事と要約について質問できる。エージェントは Node のサイドカーで動き、Ollama または `claude` CLI を使う。[AI チャット](docs/06-ai-chat.ja.md)を参照。
- **ローカル LLM 優先**: 記事の要約もダイジェストも、既定の提供元は Ollama。[設定](docs/07-settings.ja.md)を参照。
- **CLI**: `infodesk` コマンドでターミナルから収集、要約、ダイジェスト表示。[CLI](docs/08-cli.ja.md)を参照。
- **プライバシー**: ページ取得は private とローカルのアドレスを拒否し、閲覧ウィンドウからアプリの IPC は使えない。[プライバシーとセキュリティ](docs/09-privacy-and-security.ja.md)を参照。

## クイックスタート

必要なもの: macOS (動作確認は Apple Silicon)、[Rust](https://rustup.rs)、Node.js 22 以上、そしてモデルを取得済みの [Ollama](https://ollama.com) か [`claude` CLI](https://claude.com/claude-code) のどちらか。

```bash
git clone <this repository>
cd InfoDesk
cd agent && npm install && npm run build && cd ..
cd app && npm install && npm run tauri dev
```

アプリを使わず CLI だけ使う場合:

```bash
cargo run -p infodesk -- collect
cargo run -p infodesk -- digest day
```

詳しい手順、バンドルのビルド、トラブルシューティングは **[はじめに](docs/02-getting-started.ja.md)** を参照してください。

## ドキュメント

番号順にお読みください。

| # | ドキュメント | 内容 |
|---|---|---|
| 01 | [概要](docs/01-overview.ja.md) | InfoDesk とは何か、対象ユーザー、設計方針 |
| 02 | [はじめに](docs/02-getting-started.ja.md) | 必要なもの、実行、ビルド、インストール、トラブルシューティング |
| 03 | [ソースとトピック](docs/03-sources-and-topics.ja.md) | 組み込みソース、RSS、トピック、記事の画像、スケジュール |
| 04 | [ダイジェスト](docs/04-digests.ja.md) | 期間、カテゴリー別とトピック別の要約、概観、再生成 |
| 05 | [フィードとリーダー](docs/05-feed-and-reader.ja.md) | フィードの絞り込み、既読とスター、アプリ内リーダー |
| 06 | [AI チャット](docs/06-ai-chat.ja.md) | チャットパネル、提供元、ツール、進行表示、Web検索 |
| 07 | [設定](docs/07-settings.ja.md) | すべての設定と既定値、メニューバー、ログイン時の起動 |
| 08 | [CLI](docs/08-cli.ja.md) | `infodesk` コマンドとアプリの `--cli` モード |
| 09 | [プライバシーとセキュリティ](docs/09-privacy-and-security.ja.md) | ローカルに残るもの、ネットワークに出るもの、隔離 |
| 10 | [ロードマップ](docs/10-roadmap.ja.md) | 完了したこと、既知の制限、今後の案 |

各ページに英語版があります (`*.md`)。開発者向けのドキュメントは [`docs/internal/`](docs/internal/README.md) にあり、日本語で書かれています。設計仕様と実装計画は [`docs/superpowers/`](docs/superpowers/) にあります。

## アーキテクチャを一行で

```
React UI / チャットパネル / CLI  →  Tauri コマンド  →  infodesk-core (収集・保存・要約)  →  SQLite
```

処理は `infodesk-core` ライブラリが担い、`infodesk` CLI と Tauri アプリはその薄いフロントエンドです。チャットのエージェントは 127.0.0.1 に bind する Node のサイドカーです。詳細: [internal/architecture.md](docs/internal/architecture.md)。

## 状態とロードマップ

**アルファ版の個人プロジェクトです。** バージョンは 0.1.0、macOS のみ対応。コアライブラリ、CLI、デスクトップアプリ、チャットのエージェント、記事のサムネイルと進行表示、画像付きダイジェストとアプリ内リーダーを実装済みです。既知の制限と今後の案は[ロードマップ](docs/10-roadmap.ja.md)にあります。Tauri 2、React 19、Rust で作られています。

## コントリビュート

このプロジェクトは、仕様先行、デザイン先行 (`design.pen`)、TDD で進めます。まず [CONTRIBUTING.ja.md](CONTRIBUTING.ja.md) を読んでください。[行動規範](CODE_OF_CONDUCT.md)はすべてのプロジェクトの場に適用され、脆弱性の報告方法は [SECURITY.md](SECURITY.md) にあります。ドキュメントは英語が正本で、日本語版は `*.ja.md` として用意しています。

## ライセンス

次のいずれかのライセンスで提供されます。

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) または http://www.apache.org/licenses/LICENSE-2.0)
- MIT license ([LICENSE-MIT](LICENSE-MIT) または http://opensource.org/licenses/MIT)

どちらを選ぶかは利用者の自由です。

明示的に別段の定めをしない限り、Apache-2.0 ライセンスの定義に従い、このプロジェクトに含める目的で意図的に提出された貢献は、追加の条件なしに上記のデュアルライセンスで提供されるものとします。

### 商標

GitHub、Hugging Face、arXiv、Hacker News、Claude、Anthropic、Ollama は、それぞれの所有者の名称です。InfoDesk はこれらの所有者と提携しておらず、承認も受けておらず、これらに由来するものでもありません。これらの名称は、InfoDesk が読むソースと使えるツールを説明するためだけに使っています。
