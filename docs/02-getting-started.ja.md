**English: [02-getting-started.md](02-getting-started.md)** | ← [概要](01-overview.ja.md) | 次: [ソースとトピック →](03-sources-and-topics.ja.md)

# はじめに

InfoDesk は macOS 専用です。動作を確認しているのは Apple Silicon です。配布用のビルドはないので、ソースからビルドします。

## 1. 必要なもの

| ソフトウェア | 必須 | 入手先 | 確認 |
|---|---|---|---|
| Xcode Command Line Tools | はい | `xcode-select --install` | `xcode-select -p` |
| Rust 1.89 以上 (rustup) | はい | https://rustup.rs | `cargo --version` |
| Node.js 22 以上 | はい | https://nodejs.org | `node --version` |
| Ollama | どちらか一方 | https://ollama.com | `ollama list` |
| Claude Code CLI (`claude`) | どちらか一方 | https://claude.com/claude-code | `claude --version` |

- **LLM のプロバイダーが少なくとも1つ必要です。** 既定は Ollama とモデル `gemma4:26b` です (`ollama pull gemma4:26b`)。モデルは設定で変えられます。`claude` CLI も、要約とチャットの代わりに使えます。
- **Node.js** はチャットのエージェント (アプリが起動する小さなサイドカー) が使います。収集とダイジェストには要りません。Node が無くてもアプリは動き、使えないのはチャットだけです。
- rustup は `cargo` を `~/.cargo/bin` に入れます。必要なら PATH に加えてください。

## 2. 開発モードで動かす

```bash
git clone <this repository>
cd InfoDesk
cd agent && npm install && npm run build && cd ..   # チャットのエージェント (チャットを使わないなら省略可)
cd app && npm install
npm run tauri dev
```

Vite を起動して Rust 側をビルドし、アプリのウィンドウを開きます。

テスト:

```bash
cargo test                # Rust のクレート。ネットワークには接続しない
cd app && npm test -- --run   # 画面のテスト (npm test だけでは監視モードになる)
```

## 3. アプリのバンドルを作る

```bash
cd app && npm run bundle
```

エージェントをビルドしてから `tauri build --debug --bundles app` を実行します。結果はデバッグビルドのアプリで、リポジトリ直下の `target/debug/bundle/macos/InfoDesk.app` にできます。

## 4. /Applications に入れる

```bash
cp -R target/debug/bundle/macos/InfoDesk.app /Applications/
```

リポジトリの直下で実行してください。その後、Launchpad か Finder から InfoDesk を開きます。バンドル識別子は `jp.infodesk.app` です。

## 5. 初回の起動

1. アプリが `~/Library/Application Support/jp.infodesk.app/infodesk.db` にデータベースを作ります。
2. 最初の1回だけ、初期ソースと既定の設定を入れます。[ソースとトピック](03-sources-and-topics.ja.md)を参照してください。
3. まだ何も収集していないので、起動するとすぐに最初の収集が自動で始まります。**今すぐ収集** (空のフィードかメニューバーのボタン) を押して始めることもできます。初回の収集は1〜2分かかり、進行状況はサイドバーの下に出ます。
4. 各カテゴリーの上位の記事に要約が付き、今日のダイジェストが作られます。**ダイジェスト**の画面で読めます。
5. 以降は、スケジュールに従って収集します (既定は3時間おき)。日次の要約は 07:00 に確定します。スケジュールは[ソースとトピック](03-sources-and-topics.ja.md#5-収集のスケジュール)にあります。

ウィンドウを閉じても、アプリはメニューバーに残って動き続けます。設定の「常駐」で切り替えられます。

## 6. トラブルシューティング

| 症状 | 原因と対処 |
|---|---|
| `cargo: command not found` | rustup が無いか PATH に無い: `export PATH="$HOME/.cargo/bin:$PATH"` |
| ダイジェストが「保留中」のまま、または「失敗」になる | LLM に届いていません。次の2行を確認し、ダイジェストの「今すぐ再試行」を押します |
| 「Ollama に接続できません」 | Ollama が動いていません。Ollama アプリを起動するか `ollama serve` を実行します。設定の URL (既定 `http://localhost:11434`) も確認します |
| 「モデルが見つかりません」 | モデルが入っていません。設定のモデル (既定 `gemma4:26b`) を `ollama pull <モデル名>` で入れます |
| 「Node が見つかりません」 | Node 22 以上を入れるか、設定の LLM タブで Node のパスを指定します。`/opt/homebrew/bin` と `/usr/local/bin` も探します |
| 開発モードでチャットが始まらない | 先にエージェントをビルドします: `cd agent && npm install && npm run build` |
| すべて初期化する | アプリを終了し、`~/Library/Application Support/jp.infodesk.app/infodesk.db` を削除します。次の起動でソースと設定が入り直します |

コマンドラインツールの既定のデータベースの場所は、アプリとは違います。`--db` については CLI のページを参照してください。
