# テスト

3つのスイート(Rust、アプリ、サイドカー)がある。リモートのCIは無いので、変更したらローカルで該当のスイートを通す。実際のOllamaやClaudeとの結合、実ウィンドウでの表示は手動で確かめる(設計書10節、12節、16節)。

## 1. 実行方法

| スイート | ディレクトリ | コマンド | 内容 |
|---|---|---|---|
| Rust(core、cli) | リポジトリ直下 | `cargo test -p infodesk-core -p infodesk` | 収集、保存、要約、ページ取得、CLI |
| Rust(アプリ) | リポジトリ直下 | `cargo test -p infodesk-app` | コマンド、ジョブ、スケジューラー、サイドカー、閲覧ウィンドウ、メニューバー |
| 画面 | `app/` | `npm test -- --run` | Vitest + jsdom。コンポーネントとロジック |
| 画面の型 | `app/` | `npm run build` | `tsc` が通ることを確かめる(`vite build` も走る) |
| サイドカー | `agent/` | `npm test -- --run` | Vitest(Node環境)。`TZ=Asia/Tokyo` はスクリプトが付ける |
| サイドカーの型 | `agent/` | `npm run typecheck` | `tsc --noEmit` |
| サイドカーのビルド | `agent/` | `npm run build` | 型検査のあと `dist/agent.mjs` を作る。`node dist/agent.mjs --check` が `ok` を出せば読み込みは通る |
| モバイル | `mobile/` | `flutter test` | Flutterのユニットとウィジェットのテスト。偽のGoogle、偽のTailscale、偽のNAS、`sqflite_common_ffi` のSQLite |
| モバイルの静的解析 | `mobile/` | `flutter analyze` | lintと型 |

`app/package.json` と `agent/package.json` の `test` は `vitest`(`--run` なしでは監視モード)なので、1回だけ走らせるときは `--run` を付ける。Rustの全体は、リポジトリ直下で `cargo test`(ワークスペース全体)でも走る。

アプリを動かして確かめるときは、先に `agent` で `npm run build` を実行しておく(デバッグビルドは `agent/dist/agent.mjs` を探す)。手順は [02-getting-started](../02-getting-started.md)。

モバイルのコマンドは、Rosetta上のシェルでは `arch -arm64` で包む(`uname -m` が `x86_64` のとき)。例: `arch -arm64 /bin/bash -c 'export PATH=/opt/homebrew/bin:$HOME/develop/flutter/bin:/usr/bin:/bin:/usr/sbin:/sbin; cd mobile && flutter test'`。iOSシミュレーター向けのビルドは `cd mobile && flutter build ios --simulator --debug`(同じ包み方)。詳細は `mobile/README.md`。Googleのログイン、本物のNASとTailscale、Macとの印の往復は自動テストにできず、手動で確かめる([../14-mobile-app.md](../14-mobile-app.md) の10節)。

## 2. 方針

設計書10節の方針に従う。

| 対象 | 方針 |
|---|---|
| collector | ソースごとに保存した実レスポンスをフィクスチャにして、パーサーを単体テストする。ネットワークには出ない |
| store | インメモリSQLite(`Store::open_in_memory`)。重複排除、絞り込み、FTS5によるトピック照合(日本語と英語)、マイグレーション |
| summarizer | モックの `LlmProvider`。階層式の入力、保留と再試行、記事なしの省略 |
| ページ取得 | `wiremock`。本文抽出、文字コード、上限、内部アドレスの拒否、リダイレクトの検査 |
| アプリのRust | `*_impl` を `state::test_state()` で試す。イベントの発行も記録して確かめる |
| 画面 | ロジックを持つ部分と、状態ごとの表示。Tauri IPCはモックする |
| サイドカー | モックのモデルで、ツール呼び出しのループとストリームの形式。偽のCLIでツールを試す |

## 3. フィクスチャとテスト用の部品

### Rust

| 物 | 場所 | 内容 |
|---|---|---|
| 実レスポンスのフィクスチャ | `crates/core/tests/fixtures/` | `arxiv_cs_ai.xml`、`github_trending_daily.html`、`hf_models.json`、`hf_papers.json`、`hn_topstories.json` |
| フィクスチャの取り直し | `crates/core/examples/fetch_fixtures.rs` | `cargo run -p infodesk-core --example fetch_fixtures`。ネットワークに出る。取り直したらパーサーのテストが通るか確かめる |
| テスト用の部品 | `crates/core/src/testutil.rs` | `at("...")`(時刻)、`new_item(...)`、ソースの作成。`#[cfg(test)]` のみ |
| モックのLLM | `crates/core/src/llm/mod.rs` の `mock::MockProvider` | `ok(...)` / `failing(...)` と、呼ばれた引数の記録 |
| アプリの状態 | `app/src-tauri/src/state.rs` の `test_state()` | 初期ソースと設定を入れたインメモリDB、発行されたイベントの記録 |
| HTTPのモック | `wiremock` | `collector`、`page.rs`、`llm/ollama.rs`、`commands/sources.rs`、`jobs.rs` のテスト |

取得先のHTMLやAPIの形が変わったときは、フィクスチャを取り直し、パーサーとテストを直す。

### 画面(`app/src/test/`)

| ファイル | 内容 |
|---|---|
| `setup.ts` | `jest-dom` の登録、`ResizeObserver` と `scrollTo` の代替、各テストの後に `cleanup()` と `clearMocks()` |
| `ipc.ts` | `setupIpc(handlers)`: `mockIPC` でTauriのコマンドを差し替える。未登録のコマンドは例外にして `unmocked` に記録する(取りこぼしの検出)。`emitEvent(name)`: Tauriのイベントを発火する |
| `stream.ts` | サイドカーの `/chat` の応答を、与えたchunk列から作る(`streamResponse`) |
| `ipc.test.ts` | モック自体のテスト |

テストファイルは対象の隣に `*.test.ts(x)` で置く。コマンドを足したら、そのコマンドを呼ぶ画面のテストの `setupIpc` にハンドラを足す。

### サイドカー(`agent/test/`)

| ファイル | 内容 |
|---|---|
| `fake-infodesk.mjs` | 偽の `infodesk` CLI。受け取った引数をJSONで返し、特定の引数(`404`、`bad-json`、`huge`、`noisy` など)で失敗や不正な出力を返す |
| `tools.test.ts` | ツールの入力検査とCLI引数の組み立て(`web_search` は `fetch` を差し替える) |
| `chat.test.ts` | モックのモデルでループとストリーム、メタデータ(開始と終了の時刻、`finishReason`)、処理回数の上限 |
| `server.test.ts` | トークン、Origin、本文サイズ、`/health` |
| `cli.test.ts` / `errors.test.ts` / `models.test.ts` / `prompt.test.ts` | CLI実行、エラーの分類、モデルの作成、指示文 |

## 4. 何を書くか

| 変更 | 足すテスト |
|---|---|
| 新しい取得器 | フィクスチャのパース、`wiremock` での取得、1件の失敗が他を止めないこと |
| スキーマの変更 | 旧版から上げても既存の行が残ること(`migrate.rs`)、`SCHEMA_VERSION` |
| 設定キー | 既定値の一覧と未知のキーの拒否(`commands/settings.rs`)、画面で保存されること |
| スケジュールの判定 | `due_jobs` に時刻を決めた入力を与える(日付の境界、停止中に逃した分) |
| Tauriコマンド | `*_impl` の結果と、発行されるイベント |
| チャットのツール | 入力検査、CLI引数、画面の表示名と詳細 |
| 閲覧ウィンドウ | URLの検査(`http` / `https` だけ、ローカルのホストの拒否)。IPCが無いことは `capabilities/default.json` を確かめる(手動を含む) |

## 5. スクリーンショット(モックバックエンド)

`docs/images/` の画像は、実アプリではなく、**モックのバックエンドを付けたフロントエンド**を `scripts/screenshots/` の仕組みで撮ったもの。画像のデータはサンプルで、実在の記事ではない。

| ファイル | 内容 |
|---|---|
| `scripts/screenshots/reshoot.sh` | ビルド、配信、撮影をまとめて行う。引数なしで全画面、`"名前\|幅x高さ\|?クエリ"` を並べるとその画面だけ |
| `scripts/screenshots/mock.js` | `window.__TAURI_INTERNALS__` を定義し、`invoke` の各コマンド名に対してサンプルのソース、記事、トピック、要約、チャットのスレッドを返す |
| `scripts/screenshots/cdp.mjs` | ヘッドレスのChromeをDevToolsプロトコルで操作し、画面ごとにサイズを決めてPNGを保存する |

```bash
bash scripts/screenshots/reshoot.sh                          # 全画面
bash scripts/screenshots/reshoot.sh "llm|1200x1000|?go=設定,LLM"   # 1画面だけ
```

出力は `scripts/screenshots/out/`(`.gitignore` 済み)。`out/shots/<名前>.png` が画像で、`out/site/` と `out/chrome-profile/` は作業用。`docs/images/` へは必要な画像を選んでコピーする。

仕組み:

1. `app/` で `npm run build` し、`dist/` を `out/site/` へコピーする。
2. `index.html` の `<head>` の先頭に `mock.js` を差し込む。
3. `out/site/` を静的サーバー(`python3 -m http.server`、`127.0.0.1`)で配信する。
4. ヘッドレスのChromeをリモートデバッグで起動し、`cdp.mjs` が指定サイズ(1200 x 800、最小の960 x 640 など)にして撮る。終わるとサーバーとChromeを止める。
5. 撮影用の約束はクエリ文字列に持たせ、`mock.js` が解釈する(アプリ本体は見ない):
   - `?dark=1`: `prefers-color-scheme: dark` のルールを通常のルールとして追加し、ダークで描く。
   - `?go=フィード,設定,...`: 読み込み後に、名前(`aria-label` か表示文字列)の一致するボタンを順にクリックして画面を遷移させる(例: `?go=設定,LLM`)。
   - `?s=first`: 初回起動(記事が無い)の状態。`?s=err` は最後のエラーが出た状態、`?s=chaterr` はチャットのサイドカーが起動できない状態。

環境変数:

| 変数 | 既定 | 内容 |
|---|---|---|
| `CHROME` | `/Applications/Google Chrome.app/Contents/MacOS/Google Chrome` | Chromeの実行ファイル |
| `PORT` | `4789` | 静的サーバーのポート。使用中なら止まるので、別の値を渡す |
| `CDP_PORT` | `9333` | Chromeのリモートデバッグのポート |
| `WAIT` | `3500` | 画面を開いてから撮るまでのミリ秒(`cdp.mjs`)。データが出る前に撮れたときは増やす |

ルール:

- `mock.js` の記事の `image_url` は `https://picsum.photos/...` を指す。撮影にはネットワークが要り、読めない画像はアプリの動作どおり隠れる。
- UIを変えたら、最小サイズ(960 x 640)とダークでも撮り、はみ出しや折り返しの崩れを確かめる。
- 公開ドキュメントに載せる画像のキャプションには「サンプルデータ」と書く。
- 新しいコマンドを呼ぶ画面を撮るときは、`mock.js` の `handlers` に足す。未登録のコマンドはコンソールに `unmocked` と出て `null` を返す。

## 6. 手動で確かめること

| 対象 | 確かめる内容 |
|---|---|
| Ollama / Claude | 実際の要約と、チャットの会話(ツール、思考、途中経過) |
| 閲覧ウィンドウ | 表示、戻る/進む、既定のブラウザで開く、ローカルのリンクがブラウザで開くこと |
| 常駐とログイン時起動 | ウィンドウを閉じても残ること、`--hidden` で窓を出さずに起動すること |
| バンドル | `cd app && npm run bundle` で作った `.app` が、同梱の `agent.mjs` でチャットを起動できること |
