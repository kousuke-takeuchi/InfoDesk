**English: [13-nas-setup.md](13-nas-setup.md)** | ← [前: クラウド同期](12-cloud-sync.ja.md)

# NAS の準備 (Synology の WebDAV を Tailscale で使う)

InfoDesk は、Google ドライブの代わりに、利用者本人の Synology NAS へ同期できます。Mac は InfoDesk の中にある Tailscale の端末を通して NAS につながります。このページでは、最初の1回だけ行う準備、Tailscale の管理画面での端末の承認と削除、同期タブに出るメッセージの意味、ソースからビルドするときの注意を説明します。

同期の働きとファイルの構成は[クラウド同期](12-cloud-sync.ja.md)を参照してください。NAS に書くファイルは Drive に書くものと同じです。

## 1. しくみ

- NAS へは、InfoDesk の中の Tailscale の端末だけを通ります。システムの VPN は使わず、Mac に Tailscale のアプリは要りません。Mac のほかのアプリの通信には影響せず、ルーターのポートも開けません。
- 端末は Tailscale の管理画面 (Machines の一覧) に `infodesk-mac` として現れます。NAS と同じ Tailscale のアカウント (tailnet) にログインする必要があります。
- WebDAV のアドレスは `http://<NAS の名前>:5005/<フォルダ>/` の形です。例: `http://takeuchi-nas:5005/infodesk/`。認証は HTTP の Basic 認証です。NAS への通信は Tailscale の WireGuard のトンネルの中を通るので、NAS 側が HTTP でも、通信路で生のまま流れることはありません。
- パスワードは DB ではなく、macOS のキーチェーン (サービス `jp.infodesk.app.nas`) に保存します。
- Tailscale の状態 (端末の鍵) は、アプリのデータフォルダ (`~/Library/Application Support/jp.infodesk.app/`) の `tailscale/` に、権限 0700 で保存します。

![NAS を保存先にした同期タブ (デザインのモックアップ)](../design/exports/14-settings-sync-nas.png)

## 2. 5つの手順

アプリの「NAS の準備」ダイアログにも同じ5つの手順があります。DSM (Synology の NAS のソフト) の画面名は版によって少し違うので、名前が違うときは近い項目を探してください。

![NAS の準備ダイアログ (デザインのモックアップ)](../design/exports/15-nas-setup-guide.png)

1. **NAS に WebDAV Server を入れる。** パッケージセンターで「WebDAV Server」を入れます。開いて、HTTP をポート 5005 で有効にします。通信路は Tailscale が暗号化するので、HTTPS は要りません。
2. **共有フォルダを作る。** コントロールパネルの共有フォルダで「infodesk」を作ります。InfoDesk はこのフォルダにファイルを書きます。
3. **InfoDesk 用のユーザーを作る。** コントロールパネルのユーザーで新しいユーザーを作ります。infodesk だけ「読み取り/書き込み」、ほかの共有フォルダは「アクセス不可」にします。パスワードはほかで使っていないものにします。
4. **NAS に Tailscale を入れる。** パッケージセンターで「Tailscale」を入れてログインします。Tailscale の管理画面に出る NAS の名前 (例: `takeuchi-nas`) を控えます。
5. **InfoDesk でつなぐ。**
   1. 設定 (Cmd+,) を開き、「同期」タブで保存先に「NAS (WebDAV)」を選びます。
   2. 「Tailscale にログイン」を押します。ブラウザで Tailscale の画面が開くので、NAS と同じアカウントでログインし、端末を許可します。
   3. URL (`http://<NAS の名前>:5005/infodesk/`)、ユーザー名、パスワードを入れます。
   4. 「確かめる」を押します。InfoDesk は NAS に一時ファイルを書き、読み、消します。「読み書きできました」と出たら「同期する」をオンにします。

ヒント: Mac や iPhone に Tailscale のアプリは要りません。InfoDesk の中で Tailscale を使います。

## 3. Tailscale の管理画面で端末を承認する、削除する

ブラウザで Tailscale の管理画面を開き、Machines のページに進みます。

- **承認。** tailnet で端末の承認がオンのとき、新しい端末は管理者の承認を待ちます。`infodesk-mac` を探し、メニューから承認します。承認までは、同期タブに 「Tailscale の管理画面でこの端末を承認してください」 と出ます。
- **確認。** 動いている端末は `infodesk-mac` が接続中と表示されます。
- **削除。** 端末を消すには、Machines の一覧でそのメニューから削除します。そのあと Mac では再ログインが必要です。同期タブの「ログアウト」でも、先に Tailscale へログアウトが送られるので、端末は消えます。
- **鍵の期限。** 端末の鍵には期限があることがあります。切れたら同期タブからログインし直すか、管理画面で `infodesk-mac` の鍵の期限をオフにします。

## 4. 保存先を切り替えるとき

Google ドライブと NAS を切り替えると、次の同期で新しい保存先へ全部を出し直します。古い保存先のファイルは残り、InfoDesk は消しません。要らなくなったら自分で消してください (NAS の `infodesk` フォルダ、または Google の「アプリを管理」にあるデータ)。

## 5. 困ったとき

同期タブには、最後の同期のエラー、または「確かめる」の結果として、次のメッセージが出ます。

| メッセージ | 意味と対処 |
|---|---|
| 「NAS のユーザー名かパスワードが違います」 | NAS が 401 を返しました。同期タブでユーザー名とパスワードを入れ直します。DSM にそのユーザーがあり、共有フォルダに権限があることも確かめます。 |
| 「Tailscale にログインしてください」 | InfoDesk の中の端末がログインしていません。「Tailscale にログイン」を押してログインします。起動に失敗し続けるときは、失敗のあとに出る「この Mac の Tailscale の状態を消す」を押し、ログインし直します。消えるのはこの Mac に保存した状態だけなので、管理画面の端末も削除してください。 |
| 「Tailscale の管理画面でこの端末を承認してください」 | tailnet で端末の承認がオンです。3節のとおり `infodesk-mac` を承認します。 |
| 「ネットワークに接続できません。次の同期で再試行します。」 | NAS につながりません (ネットワークの切断、NAS のスリープ、NAS が Tailscale にいない)。何も失われず、次の同期で再試行します。管理画面で NAS が接続中か、WebDAV Server がポート 5005 で動いているかを確かめます。 |
| 「NAS の空き容量が足りません」 | NAS のボリュームがいっぱいか、割り当てを超えています。空きを作って、もう一度同期します。 |
| 「NAS のフォルダに書き込む権限がありません」 | DSM のユーザーがそのフォルダに書けません。コントロールパネルで、そのユーザーに共有フォルダの「読み取り/書き込み」を与えます。 |
| 「NAS の URL を確かめてください」 | URL が違うか、NAS が転送しました。`http://<NAS の名前>:5005/<フォルダ>/` の形で、NAS にあるフォルダ名を使います。名前は Tailscale のもの、`https` ではなく `http` とポート 5005 です。 |

そのほか:

- **Tailscale が起動しない。** アプリは、起動時に自分を1回だけ再起動して Tailscale のログのアップロードを止めます。それに失敗すると Tailscale は起動しません ([プライバシーとセキュリティ](09-privacy-and-security.ja.md)を参照)。InfoDesk を終了して、開き直してください。
- **NAS の名前が引けない。** 名前は、InfoDesk の中の端末がログインしている間だけ使えます。先にログインしてから「確かめる」を押します。

## 6. 開発者向け: ソースからビルドする

Tailscale の端末はアプリに組み込まれ (libtailscale 0.2.2)、Go で書かれています。InfoDesk のビルドには、PATH に Go が必要です。

- `.cargo/config.toml` が `GOTOOLCHAIN=go1.25.5` を設定します。最初のビルドで、その Go のツールチェーンをダウンロードすることがあります。
- バイナリは約 26 MB 大きくなります。
- Go が無いと、ビルドスクリプトが `go build command failed to start` でパニックします。Go を入れて (例: `brew install go`)、もう一度ビルドしてください。

[はじめに](02-getting-started.ja.md)を参照。

## 7. 本物の NAS での手動の確認

自動テストは偽の WebDAV サーバーを使います。本物の NAS で確かめるには、2節の手順を行い、「確かめる」を押し、さらに「今すぐ同期」を押して、NAS の `infodesk` フォルダに `manifest.json`、`meta.json.gz`、`day-*.json.gz` など[クラウド同期](12-cloud-sync.ja.md)に載せたファイルができたことを確かめます。

設定のキーは[設定](07-settings.ja.md)、何がどこへ送られるかは[プライバシーとセキュリティ](09-privacy-and-security.ja.md)を参照。
