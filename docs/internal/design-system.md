# デザインシステム

実装の正は `app/src/styles/theme.css`(トークンと基本部品)。ビジュアルの元は `design.pen` と、その書き出し `design/exports/`。値は2026-10-11時点の `theme.css` から読んだ。

## 1. 元データ

| 物 | 内容 | 扱い方 |
|---|---|---|
| `design.pen` | Pencilのデザインファイル(リポジトリ直下) | 暗号化されているため、`Read` や `grep` で開かない。PencilのMCPツール経由でだけ読み書きする |
| `design/exports/` | `design.pen` の各フレームのPNG | 画面の確認と実装の参照用(下表) |
| `docs/images/` | 公開ドキュメント用のスクリーンショット | 実装を撮ったもの(サンプルデータ)。デザインの元ではない |

`design/exports/` の対応(設計書7節の画面番号):

| ファイル | 画面 |
|---|---|
| `1-digest-light.png` / `1-digest-dark.png` | ダイジェスト(A-1) |
| `2-feed-light.png` / `2-feed-dark.png` | フィード(A-2) |
| `3-settings-sources-light.png` / `3-settings-sources-dark.png` | 設定: ソース(A-3) |
| `4-feed-chat-light.png` / `4-feed-chat-dark.png` | チャットを開いたフィード(A-4) |
| `5-settings-topics.png` | 設定: トピック(A-5) |
| `6-settings-llm.png` | 設定: LLM(A-6) |
| `7-settings-schedule.png` | 設定: スケジュール(A-7) |
| `8-rss-add-dialog.png` / `8b-rss-dialog-states.png` | RSS追加ダイアログと検証の状態(A-8) |
| `9-menubar.png` | メニューバー(A-9) |
| `10-digest-states.png` | ダイジェストの状態: 生成中、保留、失敗、記事なし(A-10) |
| `11-first-launch.png` | 初回起動の空状態(A-11) |
| `12-chat-states.png` | チャットの状態: 空、LLMエラー、サイドカー停止(A-12) |

設計書13〜16節で後から足した画面(サムネイル、チャットの途中経過、期間の概観と画像カード、閲覧ウィンドウ)の書き出しは `design/exports/` に無い。その見た目の正はコード(`styles/` とコンポーネント)。

## 2. トークン(`theme.css` の `:root`)

外観はOSの設定に追従する。`color-scheme: light dark` と `@media (prefers-color-scheme: dark)` で切り替え、アプリ内の切替は無い。

| トークン | ライト | ダーク | 用途 |
|---|---|---|---|
| `--bg-window` | `#ffffff` | `#1e1e1e` | 本体の背景、入力欄、カード内 |
| `--bg-sidebar` | `#f2f2f4` | `#2a2a2c` | サイドバー、ダイアログ、閲覧ツールバー |
| `--bg-subtle` | `#f5f5f7` | `#2c2c2e` | 淡い面(チャットパネル、状態ボックス、カード、チップ) |
| `--border` | `#e0e0e3` | `#3a3a3c` | 線 |
| `--text-primary` | `#1d1d1f` | `#f5f5f7` | 本文 |
| `--text-secondary` | `#6e6e73` | `#a1a1a6` | 補足 |
| `--text-tertiary` | `#8e8e93` | `#7c7c80` | 注釈、時刻、無効 |
| `--accent` | `#0a6cff` | `#3b8cff` | 強調、リンク、主ボタン、未読の点 |
| `--selection` | `#0a6cff1f` | `#3b8cff33` | 選択行、フォーカスの輪 |
| `--on-accent` | `#ffffff` | (同じ) | 強調色の上の文字 |
| `--success` | `#1e9e48` | `#32d074` | 成功、増加 |
| `--danger` | `#d93025` | `#ff6961` | エラー、減少、削除 |
| `--star` | `#f5a623` | `#ffd60a` | スター |
| `--seg-selected` | `#ffffff` | `#636366` | セグメントの選択中 |
| `--toggle-off` | `#d1d1d6` | `#48484a` | トグルのオフ |
| `--radius-sm` | `5px` | (同じ) | 小さい部品、サムネイル |
| `--radius-md` | `7px` | (同じ) | ボタン、入力欄 |
| `--font` | `-apple-system, BlinkMacSystemFont, "Hiragino Sans", "Hiragino Kaku Gothic ProN", sans-serif` | (同じ) | |

設計書7節のトークン名(`bg-window` など)は `design.pen` の変数名で、CSSでは先頭に `--` を付けた同名にしてある。

文字の大きさ: 本文13px(`body`)、補足は12px(`.small`)と11px(`.tiny`)、画面の見出し22px(`.view-header h1`)、節の見出し15px、フィードの記事タイトル14px、ダイジェストの見出し16px。書体はシステムフォント。

規則:

- 色は必ずトークンで書く。16進の直書きは増減の色など、トークンに無いものを新設するときだけ。足すときはライトとダークの両方を `theme.css` に書く。
- 色だけで状態を伝えない。ダイジェストの増減は矢印アイコンと数値、グラフは数値を文字でも出す(設計書15節)。
- 画面のテキストは日本語。アイコンは `lucide-react`。

## 3. 寸法

| 対象 | 値 | 出どころ |
|---|---|---|
| メインウィンドウ | 1200 x 800(最小 960 x 640) | `tauri.conf.json` |
| サイドバー | 幅220 | `layout.css` の `.sidebar` |
| チャットパネル | 幅400 | `chat.css` の `.chat-panel` |
| ダイジェストの節 | 最大幅720 | `.digest-section` |
| 画面の余白 | 上下24、左右48(`.view`)。フィードは 20 / 32 | `layout.css` |
| フィードのサムネイル | 96 x 64、角丸5、`object-fit: cover` | `.item-row .thumb` |
| 画像カード | 16:9、3列(幅1139px以下は2列) | `digest.css` |
| 設定のカード | 最大幅560 | `.card` |
| ダイアログ | 幅480 | `.dialog` |
| 閲覧ツールバー | 高さ44 | `reader.css`、`reader.rs` の `TOOLBAR_HEIGHT` |

## 4. 部品

基本部品は `theme.css` のクラス、状態を持つ部品は `app/src/components/` のコンポーネント。

| 部品 | 実体 | 備考 |
|---|---|---|
| ボタン | `.btn`、`.btn-primary`、`.btn-ghost`、`.btn-danger`、`.link-btn` | 無効時は不透明度0.5 |
| 入力欄 | `.field`、`.field-error` | フォーカスで `--selection` の輪と `--accent` の線 |
| セグメント | `Segmented`(`.segmented`) | 選択中は `aria-pressed="true"` |
| トグル | `Toggle`(`.toggle`) | `aria-checked`。幅32 x 高さ18 |
| 状態の点 | `.dot-success`、`.dot-danger`、`.dot-muted` | |
| エラー行 | `ErrorLine`(`.error-line`) | メッセージが `null` なら何も描かない |
| 状態ボックス | `.state-box` | 保留、失敗などの帯 |
| 空状態 | `.empty` | 初回起動など |
| チップ | `.chip` | キーワード、チャットの文脈チップは `chat.css` で丸くする |
| カード | `.card`、`RevealCard` | 開いたとき見える位置までスクロールする |
| サイドバー | `Sidebar` | 「すべて」、カテゴリー7件(アイコンは下)、トピック、収集状態、設定 |
| 棒グラフ | `charts/BarChart` | 自前のSVG。`aria-label` を必ず渡す |
| 折れ線 | `charts/Sparkline` | 同上 |
| 画像カード | `digest/ImageCards` | `https` の画像だけ。読み込み失敗は隠す |
| 概観 | `digest/Overview` | 棒グラフ、日ごとの折れ線、トピックの増減 |
| 設定の入力 | `settings/shared.tsx` の `SettingField`、`useSettingsState` | 変換確定のEnterを送信と取り違えない(`isImeEnter`) |
| チャット | `chat/` | `ChatThread`、`WorkSummary`(途中経過)、`ToolCall`、`Reasoning` |
| 閲覧ツールバー | `reader/ReaderToolbar` | 戻る、進む、再読み込み、ブラウザで開く、閉じる、タイトルとURL |

カテゴリーのアイコン(`lucide-react`、`Sidebar.tsx`): GitHub `GitBranch`、arXiv `FileText`、Hugging Face `Box`、Hacker News `Flame`、国内tech `Newspaper`、海外tech `Globe`、医療介護 `HeartPulse`。他に `Layers`(すべて)、`Hash`(トピック)、`RefreshCw`(収集)、`Settings`(設定)。

## 5. 操作の規約

- ウィンドウ全体は文字を選択できない(`user-select: none`)。`.view` の中は選択できる。ボタンは常に選択不可。
- 重い操作の確認は、行の下に全幅で出す(`.cell-confirm`)。ダイアログは現状RSS追加(検証が要る入力)だけ。
- 保存は項目ごとに即時。入力欄はフォーカスが外れた時とEnterで確定し、同じ値の二重送信を避ける。失敗したら再送できる状態に戻す。
- ボタンや見出しの文字は折り返さない。幅が足りないときはツールバーごと折り返す(`.toolbar { flex-wrap: wrap }`)。
- 画像は表示のたびに元のサイトから読む(`loading="lazy"`、`referrerpolicy="no-referrer"`)。読み込みに失敗した画像は隠す。チャットの回答では画像を描画せず、代替テキストだけを出す(外部への要求を防ぐため)。
- 最小ウィンドウ(960 x 640)でも崩れないことを、スクリーンショットで確かめる([testing.md](testing.md))。

## 6. デザインを変えるとき

1. 変更が見た目だけなら、まず `design.pen` をPencilのMCP経由で直し、`design/exports/` を書き出し直す。
2. `theme.css` のトークンを更新する(ライトとダークの両方)。設計書7節のトークン一覧も合わせる。
3. コンポーネントとCSSを直し、最小サイズとダークで撮って確かめる。
