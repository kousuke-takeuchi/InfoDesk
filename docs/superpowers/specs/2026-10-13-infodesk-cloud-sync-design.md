# InfoDesk クラウド同期とモバイルアプリ 設計書

日付: 2026-10-13
状態: レビュー待ち
親の設計書: [2026-10-06-infodesk-design.md](2026-10-06-infodesk-design.md)

## 1. 目的

Mac の InfoDesk を収集とダイジェスト生成のサーバーとして使い、その結果を利用者本人の Google Drive に置く。Flutter 製のモバイルアプリ (まず iOS、のちに Android) で、ダイジェストと記事を読めるようにする。既読とスターは Mac とモバイルの間で双方向に同期する。

### 成功条件

- Mac で収集した記事と生成したダイジェストが、次の同期 (収集の直後) で Drive に上がり、モバイルアプリで引き下げると見られる。
- モバイルで付けた既読とスターが Mac に反映され、Mac で付けたものもモバイルに反映される。
- ネットワークが切れても、Mac とモバイルはそれぞれ単独で動き、つながった後の同期で追いつく。
- Drive 上のファイルを複数の端末が同時に書き換えることがない (各ファイルの書き手は1台)。

### 対象外 (この設計)

- モバイルからの操作の依頼 (収集、再生成、トピックやソースの編集)
- モバイルでのチャット
- 記事画像の Drive への保存 (モバイルも元のサイトから読む)
- 複数利用者、共有
- Mac 以外の書き出し元 (収集するのは Mac 1台)

## 2. 構成

```
Mac (InfoDesk.app)                Google Drive (個人, appDataFolder)          モバイル (Flutter)
 収集・要約・ダイジェスト
 同期エンジン ──書き出し──>  manifest.json, day-*, digest-*, meta ──取得──> 端末内の控え (SQLite)
   ↑ 取り込み <────────────  ops-<モバイル端末ID>-*            <──書き出し── 既読・スターの操作
   書き出し ──────────────>  ops-<Mac端末ID>-*, state-snapshot ──取得──>
```

- Mac もモバイルも Google Drive API v3 を直接使う。
- 置き場所は Drive のアプリ専用フォルダ `appDataFolder`。Drive の権限は `https://www.googleapis.com/auth/drive.appdata` だけを求める。ほかに、ログイン中のアカウントのメールアドレスを表示するため `openid` と `email` を求める。どれも Google の区分で non-sensitive であり、アプリの審査は要らない。
- 同じ Google Cloud プロジェクトの OAuth クライアント (Mac 用はデスクトップ、モバイル用は iOS と Android) を使う。`appDataFolder` はプロジェクト単位で共有されるので、Mac が書いたファイルをモバイルが読める。

## 3. Drive 上のファイル

すべて `appDataFolder` の直下に置く。名前で種類を分ける。JSON は UTF-8。`.json.gz` は gzip 圧縮。

| 名前 | 書き手 | 内容 |
|---|---|---|
| `manifest.json` | Mac | スキーマ版、DB 識別子、各ファイルの名前とハッシュ (sha256) と更新時刻、最終書き出し時刻 |
| `meta.json.gz` | Mac | カテゴリー一覧、トピック (id、名前)、ソース (id、名前、カテゴリー) |
| `day-YYYY-MM-DD.json.gz` | Mac | その日 (Mac のタイムゾーン) に取得した記事の一覧 |
| `digest-{day,week,month}-YYYY-MM-DD.json.gz` | Mac | 期間要約。scope ごとの本文、状態、根拠記事 id、その期間の概観 (DigestStats) |
| `state-snapshot.json.gz` | Mac | 既読とスターの確定状態 (後述)。含めた操作の範囲 |
| `ops-<端末ID>-<UTC時刻>-<連番>.json` | 各端末 (自分の分だけ) | 既読とスターの操作の束。一度書いたら変えない |

### 3.1 記事

`day-*.json.gz` の各記事:

- `id` (Mac の `items.id`)、`url`、`title`、`excerpt`、`ai_summary`、`image_url`、`source_id`、`source_name`、`category`、`score`、`published_at`、`fetched_at`
- `topic_ids`: 書き出し時に Mac がトピックのキーワード照合で求めた、その記事に当たるトピック id の一覧

既読とスターは記事ファイルに含めない。過去の日のファイルを変えずに済むようにするため。

### 3.2 既読とスターの操作

1件の操作:

```json
{"item_id": 123, "field": "read", "value": true, "at": "2026-10-13T01:02:03.456Z", "device": "<端末ID>"}
```

- `field` は `read` か `starred`。
- 端末 ID は各端末が初回に作る UUID。Mac は設定 `sync.device_id` に保存する。
- 操作の束 (`ops-*`) は、端末が同期するたびに、前回の同期以降の自分の操作を1ファイルにまとめて作る。

### 3.3 ぶつかったときの決め方

同じ記事の同じ項目について、`at` が新しい操作を採る。`at` が同じなら端末 ID の文字列が大きい方を採る。端末の時計のずれは許容する (利用者は本人1名で、同じ記事を数秒の差で別の端末から変えることはまれ)。

### 3.4 まとめ (compaction)

Mac は1日1回 (日次ダイジェストの時刻)、全端末の操作を `state-snapshot.json.gz` にまとめる。

- 中身: 記事 id ごとの `read` と `starred` の最終値、その `at` と端末 ID。保持期間 (4節) を過ぎた記事は含めない。
- `covers`: 含めた `ops-*` ファイル名の一覧。
- まとめた後、Mac は `covers` に入った `ops-*` を消す。モバイルは、スナップショットと、`covers` に入っていない `ops-*` を合わせて状態を作る。

### 3.5 DB の作り直し

Mac の DB を作り直すと記事 id が変わる。`manifest.json` の `db_id` (Mac の DB 作成時に作る UUID、設定 `sync.db_id`) が変わったら、モバイルは控えを捨てて取り直す。

## 4. 保持期間

- Drive に置く記事と日次ダイジェストは直近60日 (Mac の日付で)。古い `day-*` と `digest-day-*` は Mac が消す。
- 週次と月次のダイジェストは直近400日。
- モバイルは端末内の控えを消さない (利用者が設定から消せる)。

## 5. Mac 側

### 5.1 認証

- OAuth 2.0 のデスクトップアプリの流れ。ブラウザで Google にログインし、`http://127.0.0.1:<空きポート>` で戻りを受ける。PKCE を使う。
- 利用者が操作するのは「Google でログイン」ボタンだけ。クライアント ID やシークレットを入力する欄は作らない。
- デスクトップ用の OAuth クライアント ID とシークレットは、ビルド時にアプリへ埋め込む。Google の扱いでは、配布するデスクトップアプリのシークレットは秘密とみなされない。ただし公開リポジトリには入れず、git 管理外のファイル `app/src-tauri/google-oauth.json` (ひな形 `google-oauth.example.json` を置く) から、ビルド時に読み込む。ファイルがなければ同期の機能を無効にしてビルドし、同期タブに「このビルドでは同期を使えません」と出す。
- 更新用トークンは macOS のキーチェーンに置く。DB には置かない。
- 「ログアウト」でトークンを失効させ (revoke)、キーチェーンから消す。

### 5.2 同期の流れ

収集が終わるたび、ダイジェストを生成したとき、Ctrl+R / ⌘R のとき、同期を1回行う。同期中なら重ねて走らせない。

1. Drive の `appDataFolder` のファイル一覧を取る。
2. 他の端末の `ops-*` のうち未取り込みのものを読み、3.3 の決め方で Mac の DB に反映する。反映は Mac 自身の操作として記録しない。
3. 前回の同期以降の Mac の操作を `ops-<Mac端末ID>-*` として書く。
4. 中身が変わった `day-*`、`digest-*`、`meta.json.gz` を書く (ハッシュで判断する)。
5. 1日1回、まとめ (3.4) と保持期間 (4節) の削除を行う。
6. 最後に `manifest.json` を書く。

### 5.3 DB の変更 (migration v5)

- `items` に `read_changed_at`、`starred_changed_at` (RFC3339、NULL 可) を足す。
- 表 `sync_ops`: Mac で起きた既読とスターの操作 (`item_id`, `field`, `value`, `at`, `uploaded`)。`set_read`、`set_starred`、`mark_all_read` が書く。
- 表 `sync_files`: 取り込み済みの `ops-*` の名前、書き出し済みファイルの名前とハッシュと Drive のファイル id。

### 5.4 画面

設定に「同期」タブを足す。

- 「Google でログイン」ボタン (ログイン後はメールアドレスと「ログアウト」)
- 同期のオン・オフ
- 最終同期の時刻、同期のエラー (収集のエラーとは別の行)
- 端末名 (既定は Mac のホスト名)
- 「今すぐ同期」

### 5.5 エラー

- ネットワークのエラー、Drive の 429 と 5xx、`userRateLimitExceeded`: その回をやめ、次の同期でやり直す。エラーは同期の行に出す。
- 401 で更新用トークンも使えないとき: 同期を止め、「再ログインが必要です」と出す。
- 同期のエラーは収集やダイジェストのエラー表示に影響しない。

## 6. モバイルアプリ (Flutter)

- 置き場所: リポジトリの `mobile/`。バンドル ID は `jp.infodesk.mobile`。
- 画面 (日本語):
  - ダイジェスト: 今日、週、月。概観 (カテゴリー別の件数、トピックの増減)、見出しと番号付きの要点、画像カード。カテゴリーとトピックで切り替える。
  - フィード: 期間、カテゴリー、トピック、未読のみ、スター、検索。記事の右に小さな画像。
  - 記事: 画像、カテゴリー、ソースと時刻、AI の要約、抜粋。上部にスター、既読と未読の切り替え、共有。「記事を開く」でアプリ内ブラウザ (iOS は SFSafariViewController 相当) を開き、既読にする。SFSafariViewController には独自のボタンを足せないので、スターと既読はこの画面で操作する。
  - タブ: ダイジェスト、フィード、スター (スター付きの記事だけのフィード)、設定。
  - 初回 (ログイン前): 「Google でログイン」ボタンだけの画面。
  - デザインは `design.pen` の M-1 から M-5、Mac の同期タブは A-13 (書き出しは `design/exports/`)。
  - 設定: Google ログインとログアウト、最終同期の時刻、控えの削除。
- 同期: 起動時、前面に戻ったとき、下に引っ張ったときに行う。手順は 5.2 の 1 から 3 と同じで、Mac の書いたファイルは取得だけする。
- 控え: 端末内の SQLite。`manifest.json` のハッシュが変わったファイルだけ取り直す。
- 既読とスターの状態は、スナップショットと操作を**差分として**手元の控えに重ねる。スナップショットだけで手元の状態を作り直さない (スナップショットは保持期間の60日より古い記事を含まないため、手元の古い記事の印が消える)。
- `manifest.json` の `db_id` が変わったら、控えと、まだ書き出していない自分の操作を捨てて取り直す。
- 使うパッケージ (`google_sign_in`、`googleapis` など) と版は、計画を書くときに pub.dev で確かめる。

## 7. Google 側の準備 (開発者が1回だけ行う作業)

手順書は `docs/` に置く。アプリを使う人はこの作業をしない。

1. 個人の Google アカウントで Google Cloud のプロジェクトを作る。
2. Google Drive API を有効にする。
3. OAuth 同意画面を作り、権限に `drive.appdata` (と `openid`、`email`) だけを入れ、公開状態を「本番」にする。公開状態が「テスト」だと更新用トークンが7日で切れる。
4. OAuth クライアントを作る: 種類「デスクトップ アプリ」(Mac 用)、「iOS」(モバイル用、バンドル ID `jp.infodesk.mobile`)。Android は後で足す。
5. デスクトップ用の ID とシークレットを `app/src-tauri/google-oauth.json` に書く (git 管理外)。iOS 用の ID は `mobile/` の設定ファイルに書く (同じく git 管理外。計画Bで決める)。

## 8. テスト

- Rust: Drive のクライアントを trait にし、メモリ上の偽物で同期の流れを試す。書き出す形、差分の判断、操作の取り込み、ぶつかったときの決め方、まとめ、保持期間の削除、エラーで次回に回ることを単体テストにする。ネットワークにはつながない。
- Flutter: 同じく偽の Drive で、控えの更新、操作の書き出し、状態の合成を試す。画面はウィジェットテスト。
- 本物の Drive とログインは手動で確かめる (確認一覧を計画に付ける)。

## 9. 進め方

計画を2つに分ける。

- 計画A: データの形 (3節)、Mac 側 (5節)、手順書 (7節)。
- 計画B: モバイルアプリ (6節)。計画A の後に書く。

どちらも Sonnet が実装し、Opus がレビューと修正をする。完了したら master にマージして push し、Mac のアプリを入れ直す。

## 10. 保存先の選択 (Google ドライブか NAS か)

利用者は同期の保存先を、Google ドライブか、自宅の NAS (Synology の WebDAV Server) から1つ選ぶ。置くファイルの名前、中身、差分の判断、既読とスターの同期 (3節から5節) は、保存先によらず同じ。違うのは、ファイルを読み書きする部分 (`DriveStore` の実装) だけである。

- 設定キー `sync.backend`: `gdrive` (既定) か `nas`。
- NAS の設定: `sync.nas.url` (WebDAV のフォルダの URL。例 `http://takeuchi-nas:5005/infodesk/`)、`sync.nas.user`。パスワードはキーチェーン (サービス `jp.infodesk.app.nas`、アカウント `password`) に置く。
- WebDAV の操作: 一覧は `PROPFIND` (Depth: 1)、取得は `GET`、作成と更新は `PUT` (ファイル名がそのまま id になる)、削除は `DELETE` (404 は成功とみなす)。認証は Basic 認証。
- エラー: 401 は「NAS のユーザー名かパスワードが違います」(同期を止め、設定を直すまで再試行しない)。接続できない、5xx は 5.5 と同じく次の同期で再試行する。
- 保存先を切り替えたときは、新しい保存先に最初から書き出す (`sync_files` をすべて消し、5.2 の手順で書き出す)。前の保存先のファイルは消さない。

## 11. アプリ内の Tailscale

NAS には、外のネットワークからも Tailscale でつなぐ。Mac とモバイルに Tailscale のアプリを入れて常に起動しておく必要はなく、InfoDesk の中に Tailscale の端末を1台組み込む。システムの VPN は使わず、InfoDesk の通信だけが tailnet を通る。

- Mac: Tailscale 公式の Go 実装 (tsnet) を C ライブラリにした libtailscale を、Rust の `libtailscale` クレート (0.2.2) から使う。ビルドに Go が要る (libtailscale が求める Go 1.25.5 を `GOTOOLCHAIN` で使う)。Tailscale の Rust 実装 (`tailscale` クレート) は、公式に「本番には使わないで」とされているので使わない。
- 状態 (鍵など) は `~/Library/Application Support/jp.infodesk.app/tailscale/` に置く。端末名は `infodesk-mac`。
- ログイン: 設定の同期タブで「Tailscale にログイン」を押すと、ログイン用の URL を既定のブラウザで開く。ログイン後は状態が残るので、次からは不要。ログアウトで状態を消す。
- NAS への HTTP 通信は、libtailscale が用意する Mac 内だけの SOCKS5 プロキシを通す (`loopback()`)。ログイン用 URL と接続状態は、同じく LocalAPI から取る。
- NAS を保存先にしたときは、通信はかならず組み込みの Tailscale を通す。tailnet の中は WireGuard で暗号化されるので、WebDAV は `http://` でよい。tailnet の外 (Tailscale を通らない宛先) には同期のデータを送らない。
- 試作 (2026-10-14) で、組み込みの Tailscale から NAS の WebDAV (`takeuchi-nas:5005`) に届くことを確かめた。
- モバイル: 同じく Go の tsnet を組み込んだ Flutter のパッケージを使う。候補は `tailscale` (tailscale_dart) と `tsnet_flutter`。計画B で比べて決める。

## 12. NAS の準備の案内

NAS 側の準備を1ページにまとめ、`docs/13-nas-setup.md` (と日本語版) に置き、同じ内容をアプリの中でも案内する (同期タブで保存先に NAS を選ぶと「NAS の準備」を開ける)。

1. DSM のパッケージセンターで「WebDAV Server」を入れ、HTTP (ポート 5005) を有効にする。
2. 共有フォルダ `infodesk` を作る。
3. InfoDesk 用のユーザーを作り、`infodesk` だけに読み書きの権限を与える (ほかの共有フォルダは「アクセス不可」)。
4. パッケージセンターで「Tailscale」を入れ、Tailscale にログインする。NAS の名前 (例 `takeuchi-nas`) を控える。
5. InfoDesk の同期タブで、保存先に NAS を選び、「Tailscale にログイン」(NAS と同じ Tailscale アカウント)、URL (`http://<NAS の名前>:5005/infodesk/`)、ユーザー名、パスワードを入れて「接続を確かめる」。

画面は `design.pen` の A-14 (同期タブで NAS を選んだとき) と A-15 (NAS の準備)。
