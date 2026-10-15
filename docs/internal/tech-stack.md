# 技術スタック

バージョンは `Cargo.toml` / `Cargo.lock` / `app/package.json` / `agent/package.json` が正で、本書は選定理由の記録。ここに書いた値は2026-10-11時点でそれらから読んだ。

## 全体構成

```
┌──────────────────────── Tauri 2 アプリ (jp.infodesk.app) ────────────────────────┐
│  WebView (app/src : React 19 + TS)        Rust (app/src-tauri)                    │
│  ├ サイドバー / ダイジェスト / フィード    ├ commands/  Tauriコマンド              │
│  ├ 設定 / チャットパネル(assistant-ui)     ├ jobs.rs / scheduler.rs  収集と要約    │
│  └ 閲覧ウィンドウのツールバー(reader.html) ├ reader.rs  アプリ内ブラウザ           │
│                                           ├ sidecar.rs チャットのサイドカー管理   │
│                                           └ tray.rs    メニューバー常駐           │
└─────────────────────────────────┬────────────────────────────────────────────────┘
                                  │ 同一ワークスペース
              crates/core (infodesk-core)   crates/cli (infodesk)
              収集 / SQLite / 要約 / ページ取得   clap CLI。アプリは --cli で同じ実装を呼ぶ
                                  │
        agent/ (Nodeサイドカー、esbuildで agent.mjs に束ねる。HTTP 127.0.0.1)
```

## Rust

ワークスペースは `crates/core`、`crates/cli`、`app/src-tauri` の3メンバー(ルート `Cargo.toml`)。全クレートが edition 2024、`rust-version = "1.89"`、ライセンス `MIT OR Apache-2.0`。開発環境のRustは1.97.1。

| 技術 | バージョン(`Cargo.toml` → lock) | 用途と選定理由 |
|---|---|---|
| Tauri | `2` → 2.12.1。features: `tray-icon`、`unstable` | デスクトップシェル。`unstable` は閲覧ウィンドウで1つの窓に2つのWebViewを置く `Window::add_child` のため(設計書16節) |
| tauri-plugin-opener | `2` → 2.7.0 | 既定のブラウザで開く |
| tauri-plugin-autostart | `2` → 2.7.0 | ログイン時に起動(LaunchAgent、引数 `--hidden`) |
| rusqlite | 0.40.2(`bundled`)→ libsqlite3-sys 0.38.2 | SQLite。同梱ビルドなのでFTS5とtrigramトークナイザーが環境に依らず使える |
| reqwest | 0.13.5(`json`) | HTTPクライアント(収集、Ollama、ページ取得) |
| feed-rs | 3.0.0 | RSS / Atom / arXivの解析。Media RSSの画像も `entry.media` から読む |
| scraper | 0.27.0 | HTMLの解析(GitHub trending、`og:image`、本文抽出) |
| quick-xml | 0.42.0(`encoding`) | フィード解析の補助(`collector/feed.rs` の名前空間付き読み取りと文字コード) |
| encoding_rs | 0.8.42 | ページ本文の文字コード変換 |
| chrono | 0.4.45(`serde`) | 日時と期間の境界 |
| libtailscale | 0.2.2 | アプリ内のTailscale端末(NAS同期、`app/src-tauri` だけ)。Go製のc-archiveをビルド時に作るので、PATH に Go が要る。`.cargo/config.toml` の `GOTOOLCHAIN=go1.25.5`。バイナリは約26MB増える |
| tokio | 1.53.2 | 非同期実行、子プロセス(`claude`、Node) |
| serde / serde_json | 1.0.229 / 1.0.151 | シリアライズ |
| thiserror | 2.0.21 | coreのエラー型 |
| async-trait | 0.1.92 | `LlmProvider` trait |
| clap | 4.6.7(`derive`) | CLI(`crates/cli`) |
| wiremock / tempfile | 0.6.5 / 3.27.0(dev) | HTTPモックと一時ファイル |

## フロントエンド(`app/`)

| 技術 | バージョン(`package.json`の指定) | 選定理由 |
|---|---|---|
| React / React DOM | ^19.1.0(導入済み 19.3.0) | 設計書3節の指定。画面は関数コンポーネントとフック |
| TypeScript | ~6.0.3 | `npm run build` が `tsc` を通す |
| Vite | ^8.0.16 | 開発サーバー(ポート1420固定)。`index.html`(本体)と `reader.html`(閲覧ツールバー)の2エントリー |
| @vitejs/plugin-react | ^6.0.2 | |
| Vitest | ^5.0.3(jsdom ^30.1.2) | 画面のテスト |
| @testing-library/react / user-event / jest-dom | ^16.3.3 / ^14.6.7 / ^7.0.1 | |
| @tauri-apps/api / plugin-opener | ^2 | `invoke` とイベント |
| @tauri-apps/cli | ^2 | `npm run tauri` |
| lucide-react | ^1.52.0 | アイコン(`design.pen` のlucide名と対応) |
| @assistant-ui/react | 0.15.25 | チャットUI部品(ストリーミング、Markdown、スレッド) |
| @assistant-ui/react-ai-sdk | 1.4.16 | AI SDKとのアダプター |
| @assistant-ui/react-markdown | 0.14.19 | 回答のMarkdown描画 |
| ai / @ai-sdk/react | 7.0.136 / 4.0.139 | UIメッセージストリーム |

グラフは外部ライブラリを使わず自前のSVG(`components/charts/`)。状態管理ライブラリは使わず、`useState` と Tauriのイベント(`status-changed` など)で再取得する。

## エージェント(`agent/`)

| 技術 | バージョン | 選定理由 |
|---|---|---|
| Node.js | `engines.node >= 22`。サイドカー起動時に22未満を拒否(`sidecar.rs` の `MIN_NODE_MAJOR`) | `ai-sdk-provider-claude-code` の要件。アプリには同梱せず、設定 `node.bin` のNodeを使う |
| ai(Vercel AI SDK) | 7.0.136 | `ToolLoopAgent` によるツール呼び出しのループ |
| ai-sdk-ollama | 4.4.0 | Ollama接続。`num_ctx` は16384 |
| ai-sdk-provider-claude-code | 4.3.3 | `claude` CLI経由のClaude。ツールは `createAiSdkMcpServer` のMCP橋渡し |
| zod | 4.6.5 | ツールの入力スキーマ |
| esbuild | 0.28.2 | `agent/dist/agent.mjs` に1ファイルへ束ねる(`build.mjs`、target node22) |
| TypeScript / Vitest | ~6.0.3 / ^5.0.3 | |

## モバイル(`mobile/`)

バージョンは `mobile/pubspec.yaml` と `pubspec.lock` から読んだ(2026-10-15時点)。Flutter 3.44.7(Dart 3.12.2)、Xcode 27。

| パッケージ | 版(lock) | 用途 |
|---|---|---|
| `google_sign_in` | 7.2.0 | Googleのサインイン(`drive.appdata`) |
| `googleapis` | 17.0.0 | Drive API |
| `googleapis_auth` | 2.3.4 | Drive用の認証つきHTTPクライアント |
| `extension_google_sign_in_as_googleapis_auth` | 3.0.0 | `google_sign_in` と `googleapis_auth` の橋渡し |
| `tailscale` | 0.9.0 | アプリ内のTailscale端末。ビルド時にGoでネイティブのライブラリを作る |
| `sqflite` | 2.4.4+2 | 端末内のSQLite |
| `http` | 1.6.0 | WebDAV(NAS) |
| `url_launcher` | 6.3.3 | アプリ内のブラウザとシステムのブラウザ |
| `flutter_secure_storage` | 11.2.0 | NASのパスワード(キーチェーン) |
| `path_provider` | 2.1.6 | アプリのサポートフォルダ |
| `archive` | 4.5.2 | gzipのJSONの展開 |
| `crypto` | 3.0.7 | SHA-256(ハッシュの照合) |
| `xml` | 7.0.1 | WebDAVのPROPFIND応答の解析 |
| `cupertino_icons` | 1.0.9 | アイコン |
| `flutter_lints`(dev) | 6.0.0 | lint |
| `sqflite_common_ffi`(dev) | 2.4.4 | テストでのSQLite |

Goは1.26以降をPATHに置く(MacのRust側は1.25.5に固定している点と別)。iOSのデプロイターゲットは15.0。

## 外部ツール

| ツール | 使い方 |
|---|---|
| Go 1.25.5 | ビルド時だけ。libtailscale のビルドスクリプトが使う。`GOTOOLCHAIN=go1.25.5` で固定(`.cargo/config.toml`)し、初回のビルドでツールチェーンを取得することがある |
| Ollama | 要約とチャット。既定のURLは `http://localhost:11434`、既定のモデルは `gemma4:26b` |
| `claude` CLI | Claudeを選んだときの要約(ヘッドレス実行)とチャット。パスは設定 `claude.bin` |
| `ollama.com` のWeb検索API | 設定 `ollama.web_search_api_key` があるときだけチャットの `web_search` を有効にする |

## 不採用にしたもの

設計書12節の記録。

| 候補 | 不採用の理由 |
|---|---|
| CopilotKit | 独自ランタイムのサーバーが前提 |
| chatscope | 更新が止まり、ストリーミングとツール表示が無い |
| Mastra / LangGraph.js | 単一エージェントには過剰 |
| rig(Rust) | UIとの接続を全て自作する必要がある |
| 意味検索(`bge-m3`) | 初版の対象外。キーワード照合で不足したら検討 |
| 記事本文の収集 | 初版の対象外。本文はチャットで必要な記事に限り取得し、保存しない |
