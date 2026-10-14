# NAS 同期とアプリ内 Tailscale (Mac 側) 実装計画 (Plan 7A2)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 同期の保存先に Synology NAS (WebDAV) を選べるようにし、NAS へはアプリに組み込んだ Tailscale を通してつなぐ。NAS の準備はアプリ内の案内と文書で1ページにまとめる。

**Architecture:** 同期の中身 (`crates/core/src/sync/engine.rs`) は変えない。`DriveStore` の新しい実装 `WebDavStore` を足し、HTTP は組み込みの Tailscale が用意する Mac 内の SOCKS5 プロキシを通す。Tailscale は `libtailscale` クレート (Go の tsnet を C ライブラリにしたもの) でアプリの中に1台分の端末として動かす。どの保存先を使うかは設定 `sync.backend` で決める。

**Tech Stack:** Rust 2024、reqwest 0.13 (`socks` 機能を足す)、quick-xml 0.42 (PROPFIND の応答)、libtailscale 0.2.2 (+ Go 1.25.5)、Tauri 2.12、React 19。

**Spec:** `docs/superpowers/specs/2026-10-13-infodesk-cloud-sync-design.md` の 10、11、12節 (と既存の 3〜5節)。デザインは `design/exports/14-settings-sync-nas.png` (A-14) と `design/exports/15-nas-setup-guide.png` (A-15)。

## Global Constraints

- 設定キー: `sync.backend` (`gdrive` 既定 / `nas`)、`sync.nas.url`、`sync.nas.user`。NAS のパスワードはキーチェーン (サービス `jp.infodesk.app.nas`、アカウント `password`)。
- Tailscale の状態は `~/Library/Application Support/jp.infodesk.app/tailscale/`。端末名 `infodesk-mac`。ephemeral にしない (状態を残し、次の起動でログイン不要にする)。
- NAS への通信は必ず組み込みの Tailscale を通す。組み込みの Tailscale が動いていないときは NAS へ接続しない (システムのネットワークで直接つながない)。
- WebDAV: 一覧 `PROPFIND` (Depth: 1)、取得 `GET`、作成と更新 `PUT`、削除 `DELETE` (404 は成功)。Basic 認証。ファイル名がそのまま id。
- エラー文 (画面に出る日本語、「同期できませんでした: 」は画面側で付く):
  - NAS が 401: 「NAS のユーザー名かパスワードが違います」(再試行しない。設定を直すまで止める)
  - Tailscale 未ログイン: 「Tailscale にログインしてください」
  - 接続できない、5xx、タイムアウト: 既存の「ネットワークに接続できません。次の同期で再試行します。」
- 保存先を切り替えたら `sync_files` をすべて消し、次の同期で新しい保存先に最初から書き出す。前の保存先のファイルは消さない。
- ビルド: libtailscale は Go を使う。`.cargo/config.toml` の `[env]` で `GOTOOLCHAIN = "go1.25.5"` を指定する。Go が PATH にないときのエラーを文書に書く。
- 単体テストはネットワークにつながず、本物の Tailscale も起動しない (trait と偽物を使う)。
- UI の文言は A-14、A-15 のとおり。絵文字とダッシュ記号を使わない。コミットに AI の署名行を付けない。

## Review Focus

1. PROPFIND の応答: 名前の URL エンコード (日本語や空白)、フォルダ自身の行を除くこと、`DAV:` 名前空間の接頭辞が `D:` 以外 (例 `d:`、接頭辞なし) でも読めること。
2. 組み込み Tailscale が未ログインや停止中のとき、同期がシステムのネットワークで NAS に直接つながず、決まったエラーで止まること。
3. 保存先を切り替えた直後の同期で、前の保存先の `sync_files` (drive_id など) を使わないこと。
4. NAS の URL の末尾のスラッシュの有無、フォルダ名の大文字小文字、ポートの省略を正しく扱うこと。
5. アプリ終了時やログアウト時に Tailscale の端末をきちんと止め、プロセスが残らないこと。

---

### Task 1: WebDAV の DriveStore (core)

**Files:** Create `crates/core/src/sync/webdav.rs`。Modify `crates/core/src/sync/mod.rs`、`crates/core/src/sync/drive.rs` (必要なら `DriveError` に `BadCredentials` を足す)、`crates/core/Cargo.toml` (reqwest に `socks` 機能)。

**Interfaces:**
- `pub struct WebDavConfig { pub url: String, pub user: String, pub password: String, pub proxy: Option<String> /* socks5h://user:pass@127.0.0.1:port */ }`
- `pub struct WebDavStore` (`DriveStore` を実装)、`pub fn new(cfg: WebDavConfig) -> Result<Self, DriveError>`。reqwest の Client は Drive と同じ timeout (connect 10s、read 60s)。`proxy` があれば `reqwest::Proxy::all(proxy)` を使い、なければプロキシなし (テスト用)。
- `RemoteFile.id` はファイル名。`modified_time` は `getlastmodified` (RFC 1123) を RFC3339 に直したもの。`size` は `getcontentlength`。
- `DriveError::BadCredentials` (表示「NAS のユーザー名かパスワードが違います」) を足し、既存の `match` を更新する。

- [ ] **Step 1: テストを書く** (wiremock。`method("PROPFIND")` で受ける)
  - `list_parses_propfind_and_skips_the_folder_itself` (Synology 形式の `D:multistatus` 応答を用意。フォルダ自身の `href` は除く)
  - `list_accepts_other_dav_prefixes` (接頭辞なし、`d:`)
  - `names_are_percent_decoded` (`day-2026-10-14.json.gz` と、日本語や空白を含む名前)
  - `put_creates_and_updates_by_name`、`get_downloads`、`delete_treats_404_as_done`
  - `unauthorized_is_bad_credentials`、`server_errors_and_timeouts_are_retryable`
  - `url_with_or_without_trailing_slash_resolves_the_same_files`
- [ ] **Step 2:** 失敗を確かめる。**Step 3:** 実装。**Step 4:** `cargo test -p infodesk-core sync::webdav`
- [ ] **Step 5: コミット** `feat(core): WebDAV storage for sync`

### Task 2: アプリ内 Tailscale (app)

**Files:** Create `app/src-tauri/src/tailnet.rs` (と tests)。Modify `app/src-tauri/Cargo.toml` (`libtailscale = "0.2.2"`)、`.cargo/config.toml` (`[env] GOTOOLCHAIN`)、`app/src-tauri/src/state.rs`。

**Interfaces:**
- `pub enum TailnetState { Stopped, NeedsLogin { url: String }, Starting, Running { name: String, ip: String } }`
- `pub trait Tailnet: Send + Sync { fn state(&self) -> TailnetState; fn start(&self) -> AppResult<()>; fn proxy_url(&self) -> Option<String>; fn logout(&self) -> AppResult<()>; fn stop(&self); }`
- `pub struct EmbeddedTailnet` (libtailscale): `start` は別スレッドで `set_dir`、`set_hostname("infodesk-mac")`、`set_logfd(-1)`、`start()` をし、`loopback()` の LocalAPI (`/localapi/v0/status`、Basic 認証 `credential`) を数秒おきに見て、`BackendState` と `AuthURL`、自分の名前と IP を状態に反映する。`Running` になったら `proxy_url` は `socks5h://tsnet:<proxy_credential>@<address>`。ログイン用 URL が出たら一度だけ既定のブラウザで開く。`logout` は LocalAPI の logout を呼び、状態ディレクトリを消す。`stop` は端末を閉じる (アプリ終了時に呼ぶ)。
- 状態が変わったら `sync-changed` を出す。
- `FakeTailnet` (テスト用、状態を外から変えられる)。

- [ ] **Step 1: テストを書く**: LocalAPI の status の JSON から状態を作る純粋関数 (`NeedsLogin` と `AuthURL`、`Running` と `Self.DNSName`/`TailscaleIPs`、`Stopped`) を wiremock なしの単体テストで。`proxy_url` の組み立て。Fake で「NeedsLogin のときは proxy_url が None」。
- [ ] **Step 2〜4:** 失敗、実装、成功。本物の起動は単体テストでしない。手動確認の手順を報告に書く (試作と同じく、ログイン URL を開いて承認し、`Running` になること)。
- [ ] **Step 5: コミット** `feat(app): an in-app Tailscale node for reaching the NAS`

### Task 3: 保存先の切り替えと NAS の設定 (app)

**Files:** Modify `app/src-tauri/src/sync/mod.rs`、`keychain.rs` (NAS のパスワード)、`commands/sync.rs`、`lib.rs`。

**Interfaces:**
- `SyncStatus` に `backend: String`、`nas_url: String`、`nas_user: String`、`nas_password_set: bool`、`tailnet: TailnetStatusView { state: String /* stopped|needs_login|starting|running */, name: Option<String> }` を足す。
- コマンド: `sync_set_backend(backend: String)` (切り替えたら `sync_files` を全部消し、Drive クライアントのキャッシュも捨てる)、`sync_set_nas(url: String, user: String, password: Option<String>)` (`None` ならパスワードはそのまま)、`tailnet_login()` (start して、ログイン URL を開く)、`tailnet_logout()`、`sync_test_nas() -> String` (一時ファイル `infodesk-check-<uuid>.txt` を PUT、GET、DELETE。成功で「読み書きできました」、失敗でエラー文)。
- `make_drive`: `sync.backend == "nas"` なら、`tailnet.proxy_url()` が None のとき「Tailscale にログインしてください」で止める。あれば `WebDavStore` を作る。`BadCredentials` はエラー表示にし、`signed_in` は変えない。
- NAS を選んでいるときの `signed_in` 相当は「Tailscale が Running かつ URL とユーザー名とパスワードがそろっている」。同期の開始条件をこれに合わせる。
- アプリ終了時 (`RunEvent::Exit`) に `tailnet.stop()`。アプリ起動時、`sync.backend == "nas"` かつ `sync.enabled` なら tailnet を start する (保存済みの状態でログイン不要)。

- [ ] **Step 1: テストを書く** (`FakeTailnet` と `MemoryDrive` を使う): 切り替えで `sync_files` が消える、Tailscale 未ログインで決まったエラー、`BadCredentials` の表示、`sync_test_nas` の成功と失敗、NAS の値の保存 (パスワードはキーチェーンのメモリ版に入る)。
- [ ] **Step 2〜4:** 失敗、実装、成功。
- [ ] **Step 5: コミット** `feat(app): choose Google Drive or a NAS for sync`

### Task 4: 同期タブの NAS 設定と案内 (UI)

**Files:** Modify `app/src/views/settings/SyncTab.tsx` (+ test)、`app/src/api.ts`、`app/src/types.ts`。Create `app/src/views/settings/NasGuideDialog.tsx` (+ test)。

- A-14 のとおり: 「保存先」節に「保存先」(Google ドライブ / NAS (WebDAV) の切り替え)。NAS のときは Tailscale の行 (状態の点と文言「接続済み · {名前}」「ログインしてください」「起動中…」、ボタン「Tailscale にログイン」か「ログアウト」)、「NAS の URL」「ユーザー名」「パスワード」(保存済みなら「••••••••」を表示し、入力したときだけ送る)、「接続を確かめる」(結果を緑か赤の1行で)、リンク「NAS の準備を見る (初めての方へ)」。Google ドライブのときは今のログインの行。
- 期間の節の見出しは「保存先に置く期間」。
- A-15 のとおり: ダイアログ「NAS の準備」(5つの手順、注意書き、「詳しい手順 (docs/13-nas-setup)」は GitHub の文書を既定のブラウザで開く、「閉じる」。Esc でも閉じる。`role="dialog" aria-modal="true"`)。初めて NAS を選んだとき (URL が空のとき) は自動で開く。
- [ ] テスト先行。`cd app && npm test -- --run && npm run build`。
- [ ] コミット `feat(ui): NAS storage settings and the in-app setup guide`

### Task 5: 文書と確認

- Create `docs/13-nas-setup.md` と `.ja.md` (A-15 と同じ5手順をくわしく。DSM の画面名は一般的な言い方で。Tailscale の管理画面での端末の確認と削除。困ったとき: 401、Tailscale 未ログイン、接続できない)。
- Modify `docs/12-cloud-sync*` (保存先の選択)、`docs/02-getting-started*` と `CONTRIBUTING*` (ビルドに Go が要る、`GOTOOLCHAIN`)、`docs/07-settings*` (新しいキー)、`docs/09-privacy-and-security*` (Tailscale、NAS のパスワード)、README の表、`docs/internal/*`。
- 全体の確認 (`cargo test --workspace`、clippy、fmt、`npm test`、`npm run build`、agent のテスト、`npm run bundle` が通り、バンドルが起動すること)。
- コミット `docs: NAS setup guide and sync storage choice`
