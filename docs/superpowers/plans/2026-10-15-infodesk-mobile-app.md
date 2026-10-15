# モバイルアプリ (Flutter、まず iOS) 実装計画 (Plan 7B)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Mac の InfoDesk が Google ドライブか NAS に書き出したデータを、iPhone で読めるようにする。既読とスターは双方向に同期する。Android でも動く作り (Flutter) にする。

**Architecture:** `mobile/` に Flutter アプリを置く。データの形と既読・スターの合成は Mac (`crates/core/src/sync/format.rs`) と同じ規則を Dart で書き、同じテスト値で確かめる。保存先は Mac と同じく抽象 (`DriveStore`) の向こうに隠し、Google ドライブ (googleapis + google_sign_in) と NAS (WebDAV、アプリ内 Tailscale の HTTP クライアント) を差し替える。端末内の控えは SQLite。

**Tech Stack:** Flutter 3.44.7 / Dart 3.12.2、google_sign_in 7.2.0、googleapis 17.0.0、extension_google_sign_in_as_googleapis_auth 3.0.0、tailscale 0.9.0 (tailscale_dart。Go の tsnet を組み込む。ビルドに Go が要る)、sqflite 2.4.4+2、http 1.6.0、url_launcher 6.3.3、flutter_secure_storage 11.2.0、path_provider 2.1.6、archive 4.5.2 (gzip)。版は pub.dev で 2026-10-15 に確かめた最新。

**Spec:** `docs/superpowers/specs/2026-10-13-infodesk-cloud-sync-design.md` (3、4、6、10、11節。6節の「スナップショットは差分として重ねる」「db_id が変わったら取り直す」を含む)。デザインは `design.pen` の M-1〜M-5 (書き出し `design/exports/m1-*.png`〜`m5-*.png`)。NAS の準備の案内は A-15 と同じ文言。

## Global Constraints

- 置き場所 `mobile/`。バンドル ID / applicationId は `jp.infodesk.mobile`。iOS の最低バージョンは tailscale パッケージと google_sign_in が求めるものに合わせる (Task 0 で確かめて決める)。
- データの形は Mac と同じ (spec 3節)。`StateSnapshot.state` のキーは文字列で届く。`ops-*` の `at` は書くときに `YYYY-MM-DDTHH:MM:SS.mmmZ` (UTC、ミリ秒)、読むときは RFC3339 を受けて同じ形に直す。合成は `(at, device, value)` の大きい方が勝つ。
- スマホは Mac の書いたファイル (`manifest.json`、`meta.json.gz`、`day-*`、`digest-*`、`state-snapshot.json.gz`、他端末の `ops-*`) を読むだけ。書くのは自分の `ops-<端末ID>-*` だけ。ほかのファイルを消さない。`parse_file_name` で認識できない名前は無視する。
- スナップショットは手元の控えに差分として重ねる。スナップショットだけで状態を作り直さない。`manifest.db_id` が変わったら、控えと、まだ書き出していない自分の操作を捨てて取り直す。
- Google ドライブ: 権限は `drive.appdata` と `openid`、`email`。iOS の OAuth クライアント ID は git 管理外のファイルからビルドに渡す (`mobile/ios/Flutter/Secrets.xcconfig`。ダウンロードした plist から作るスクリプトを用意する)。
- NAS: 通信はアプリ内 Tailscale の HTTP クライアントだけを通す。URL のホストが tailnet の端末 (HostName、DNSName、Tailscale IP) でなければ断る (Mac と同じ「NAS が Tailscale の中に見つかりません」)。`http://` だけを受け付ける。Tailscale の Go ログの送信は止める (`noLogsNoSupport: true`)。ログイン URL はボタンを押したときだけ開く。NAS のパスワードは `flutter_secure_storage`。
- UI の文言は日本語で、M-1〜M-5、A-15 に合わせる。絵文字とダッシュ記号を使わない。
- テスト: Dart の単体テストとウィジェットテストはネットワークにつながない (偽の DriveStore を使う)。本物の Google、Tailscale、NAS は手動で確かめる。
- コミットに AI の署名行を付けない。`design.pen` をコミットしない。
- ビルド環境: このMacのシェルは Rosetta (x86_64) で動いていることがある。Flutter と Xcode は `arch -arm64` で動かす。Go は `/opt/homebrew/bin/go` (シェルの `go` は別名なので使わない)。

## Review Focus

1. Mac の Rust と Dart で、同じ入力に対して同じファイル名の解析、同じ合成結果になること (共通のテスト値を JSON に置いて両方で読む)。
2. 同期の途中で失敗したとき、取り込み済みの印や書き出し済みの印が付かず、次回やり直せること。
3. db_id が変わったときに、未送信の自分の操作を捨て、古い記事 id に印を付けないこと。
4. NAS の URL が tailnet の外を指すとき、通信せずに断ること。
5. アプリを閉じている間にたまった既読・スターが、次に開いたときに書き出されること。

---

### Task 0: 土台と、ビルドの確認

- `flutter create --org jp.infodesk --project-name infodesk_mobile --platforms ios,android mobile` (バンドル ID が `jp.infodesk.mobile` になるよう調整)。
- iOS シミュレーター向けのビルドが、このMacで通ることを確かめる。2026-10-15 の試作では、`flutter build ios --simulator` が「Flutter.framework に arm64 x86_64 がない」(実際には両方入っている) で失敗した。Xcode 27 と Flutter 3.44.7 の組み合わせで起きている。原因を突き止め、`arch -arm64` での実行、`EXCLUDED_ARCHS`、`flutter run -d <シミュレーター>` などで回避し、手順を `mobile/README.md` に書く。
- tailscale パッケージを入れた状態で iOS シミュレーター向けにビルドが通ることを確かめる (Go の組み込みのビルド)。
- `mobile/ios/Flutter/Secrets.xcconfig` (git 管理外) と、ダウンロードした `client_*.plist` からそれを作る `mobile/tool/ios_oauth_from_plist.sh`。`Info.plist` で `GIDClientID` と URL スキーム (REVERSED_CLIENT_ID) を参照する。
- 確認: `flutter test` (雛形のテスト) と iOS シミュレーターのビルドが通る。
- コミット `feat(mobile): scaffold the Flutter app and its iOS build`

### Task 1: データの形と合成 (Dart)

- `mobile/lib/sync/format.dart`: Manifest、MetaFile、DayFile、ItemRecord、DigestFile、Op、OpsFile、StateSnapshot、Stamp、ItemState。`parseFileName`、`opsFileName`、`opTime`、`normalizeOp`、`wins`、`applyOp`、gzip の読み書き。
- 共通テスト値: `crates/core/tests/fixtures/sync/` に JSON を置き (ファイル名の例、操作の列と期待する状態、スナップショットの例)、Rust (既存の format のテストに足す) と Dart の両方のテストで読む。
- コミット `feat(mobile): sync file format and last-writer-wins merge shared with the Mac`

### Task 2: 端末内の控え (SQLite)

- 表: items (Mac の id、各項目、topic_ids、read/starred と、その時刻と端末)、digests、meta、synced_files (名前、ハッシュ、取り込み済みか)、pending_ops (自分の操作、送信済みか)、settings。
- 操作: 記事の読み出し (期間、カテゴリー、トピック、未読、スター、検索)、既読とスターの変更 (pending_ops に積む)、リモートの操作の適用 (正規化してから、勝つときだけ)、スナップショットを差分として重ねる、db_id が変わったら全部消す。
- コミット `feat(mobile): local store for items, digests and read/star state`

### Task 3: 保存先 (Google ドライブ / NAS)

- `abstract class DriveStore { list, download, create }` (スマホは更新と削除をしない) と、テスト用の `MemoryDrive`。
- `GoogleDriveStore`: googleapis の Drive v3、`spaces=appDataFolder`、ページ分けをたどる、401 で再ログインを促す、429/5xx は再試行扱い。
- `NasStore`: WebDAV (PROPFIND の解析は Mac と同じ規則: 207 必須、自分のフォルダと子フォルダを除く、名前の検査)。HTTP は tailscale の `http.client`。URL のホストを tailnet の端末に照らし、100.x に書き換える。`https` と tailnet 外は断る。
- コミット `feat(mobile): Google Drive and NAS storage for sync`

### Task 4: 同期の1回分 (スマホ)

- list (認識できる名前だけ) → manifest を読み、db_id を見る → ハッシュが変わったファイルだけ取って控えに入れる → スナップショットと他端末の ops を重ねる → 自分の pending_ops を `ops-<端末ID>-...json` で書き出し、成功したら送信済みにする。
- 途中の失敗では印を付けない。同時に2回走らせない。
- 開いたとき、前面に戻ったとき、下に引っ張ったときに同期する。
- コミット `feat(mobile): one sync pass on the phone`

### Task 5: ログインと Tailscale

- 保存先の選択 (Google ドライブ / NAS)。Google: google_sign_in v7 (`drive.appdata`、`openid`、`email`)、ログアウト。NAS: tailscale の init (`noLogsNoSupport: true`、状態はアプリのサポートディレクトリ)、「Tailscale にログイン」で URL を開く、状態と経過秒数の表示、URL・ユーザー名・パスワード (secure storage)、「確かめる」(読めることだけ確かめる。スマホは NAS に書くのは自分の ops だけなので、一時ファイルは書かない)、NAS の準備の案内 (A-15 と同じ文言)。
- コミット `feat(mobile): sign-in for Google Drive and the NAS over in-app Tailscale`

### Task 6: 画面 (M-1〜M-5)

- M-5 初回 (保存先の選択とログイン)、M-1 ダイジェスト (今日・週・月、概観の棒、トピックの増減、見出しと番号付きの要点、画像カード)、M-2 フィード (検索、絞り込み、未読の点、画像、全部既読)、M-3 記事 (スター、既読の切り替え、共有、「記事を開く」は SFSafariViewController 相当 = url_launcher の `inAppBrowserView`、開いたら既読)、M-4 設定 (保存先、アカウント、最終同期、Mac からの更新、今すぐ同期、控えの削除、バージョン)。下のタブ: ダイジェスト、フィード、スター、設定。
- ウィジェットテストで各画面の主な表示と操作を確かめる。
- コミット `feat(mobile): digest, feed, article and settings screens`

### Task 7: 文書と確認

- `mobile/README.md` (ビルド手順、OAuth の plist、Go、arch)、`docs/14-mobile-app.md` と `.ja.md` (使い方)、`docs/12-cloud-sync*` と README の表、`docs/internal/*`。
- 確認: `cd mobile && flutter test`、`flutter analyze`、iOS シミュレーター向けのビルドとシミュレーターでの起動。既存の Rust と app のテストも通ること。
- 利用者の手動確認一覧: シミュレーター (または実機) で Google ログインと同期、NAS で Tailscale ログインと同期、既読とスターが Mac に届くこと、Mac の既読がスマホに届くこと。
- コミット `docs: the mobile app`
