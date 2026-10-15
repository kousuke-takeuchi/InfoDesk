**English: [07-settings.md](07-settings.md)** | ← [前: AI チャット](06-ai-chat.ja.md) | 次: [CLI →](08-cli.ja.md)

# 設定

設定は、サイドバーから、またはメニューバーの「設定…」(Cmd+,) から開きます。タブは、ソース、トピック、LLM、スケジュール、同期の5つです。変更は、項目を編集したときに保存されます。

## タブ

| タブ | 内容 |
|---|---|
| ソース | 組み込みソースの有効と無効、RSS フィードの追加。[ソースとトピック](03-sources-and-topics.ja.md)を参照。 |
| トピック | トピックとそのキーワード。[ソースとトピック](03-sources-and-topics.ja.md)を参照。 |
| LLM | 各処理で使う LLM、Ollama と Claude の設定、Node のパス、接続テスト。 |
| スケジュール | 収集の間隔、要約の時刻、起動の設定、保留中の要約の再試行。 |
| 同期 | 保存先 (Google ドライブか NAS) の選択、Google または Tailscale のログイン、NAS のアドレスとユーザー、同期のオンとオフ、この Mac の名前、最終同期の時刻とエラー、「今すぐ同期」。[クラウド同期](12-cloud-sync.ja.md)を参照。 |

### LLM タブ

![LLM タブ (サンプルデータ)](images/settings-llm.png)

*LLM タブ。モックしたバックエンドのサンプルデータです。*

- 「要約に使うLLM」: 記事要約、期間要約、チャットで、Ollama と Claude (claude CLI) を別々に選びます。Claude を選んでも、失敗したときに Ollama へ自動で切り替えません。
- 「Ollama」: URL、モデル (Ollama に入っているモデルのメニュー。「再読み込み」ボタンで取り直します。Ollama に接続できないときは入力欄になり、「再読み込み」は残ります)、接続テスト、チャット用の Web 検索 API キー。
- 「Claude」: `claude` コマンドのパス、モデル (`sonnet`、`opus`、`fable`、または「その他 (名前を入力)」で完全なモデル名)、接続テスト、Node のパス (チャットのエージェントの実行に使います)。
- パスの欄は、コマンド名 (英数字と `_`、`.`、`-`) か、存在するファイルの絶対パスを受け付けます。GUI アプリはシェルの PATH を引き継がないので、絶対パスが確実です。

### スケジュールタブ

![スケジュールタブ (サンプルデータ)](images/settings-schedule.png)

*スケジュールタブ。モックしたバックエンドのサンプルデータです。*

- 「収集」: 収集間隔 (メニューは1分、5分、15分、30分、1時間、3時間、6時間、12時間)、次回の収集時刻、「今すぐ収集」ボタン。
- 「要約の生成」: 日次の時刻、週次の曜日と時刻、月次の時刻 (毎月1日)。日次の時刻に前日の日次要約を確定し、現在の週次と月次を作り直します。当日の日次要約は収集では作り直しません。「生成」または「再生成」で作ります。
- 「起動」: ログイン時に起動、ウィンドウを閉じても常駐する。
- 「保留中の要約」: 最後のエラーと、保留中または失敗した要約を再試行するボタン。保留中の要約は、他のジョブの有無にかかわらず、スケジューラーも15分に1回まで自動で再試行します。失敗になった要約は、このボタンかダイジェストのボタンでだけ再試行します。

実行されなかったジョブ (Mac がスリープしていた、アプリを閉じていたなど) は、次に起動または復帰したときに1回だけ補います。

## 設定キー

設定はすべて、アプリの DB に文字列で保存します。既定値は初回の起動で書き込まれます。キーがなければ既定値を使います。

| キー | 既定値 | 内容 |
|---|---|---|
| `llm.items_provider` | `ollama` | 記事要約のプロバイダー。`ollama` または `claude`。 |
| `llm.digest_provider` | `ollama` | 日次、週次、月次の要約のプロバイダー。`ollama` または `claude`。 |
| `llm.chat_provider` | `ollama` | チャットの既定のプロバイダー。`ollama` または `claude`。チャットパネルでパネルごとに切り替えられます。 |
| `ollama.base_url` | `http://localhost:11434` | Ollama サーバーの URL。 |
| `ollama.model` | `gemma4:26b` | Ollama のモデル名。 |
| `ollama.web_search_api_key` | 空 | ollama.com の API キー。プロバイダーが Ollama のとき、チャットの Web 検索を有効にします。 |
| `claude.bin` | `claude` | Claude CLI のコマンドまたは絶対パス。 |
| `claude.model` | `sonnet` | CLI に `--model` として渡す Claude のモデル。`sonnet`、`opus`、`fable` または完全なモデル名。要約、期間要約、チャットで使います。空のときは、要約では CLI の既定、チャットでは `sonnet` です。使える文字は英数字と `. _ : - [ ]` で、先頭に `-` は使えません。 |
| `node.bin` | `node` | チャットのエージェント用の Node (22 以上) のコマンドまたは絶対パス。 |
| `summary.top_n` | `20` | 収集のたびに記事要約を付ける、カテゴリーごとの上位の件数。設定画面には出ません。数値が必要です。 |
| `schedule.collect_interval_minutes` | `180` | 収集の間隔 (分)。1〜10080の範囲外は丸めます。不正な値は180になります。 |
| `schedule.collect_interval_hours` | `3` | 旧設定。分の設定がないときだけ読みます (1〜168時間に丸めます)。起動時に分の設定がなければ、この値から作ります。設定画面には出ません。 |
| `schedule.daily_time` | `07:00` | 前日の日次要約を確定する時刻 (HH:MM)。 |
| `schedule.weekly_day` | `mon` | 週次要約の曜日。`mon`、`tue`、`wed`、`thu`、`fri`、`sat`、`sun`。 |
| `schedule.weekly_time` | `07:30` | 週次要約の時刻 (HH:MM)。 |
| `schedule.monthly_time` | `08:00` | 月次要約の時刻 (HH:MM)。毎月1日。 |
| `app.launch_at_login` | `false` | ログイン時に InfoDesk を起動する。ウィンドウは開かず、メニューバーに常駐します。 |
| `app.stay_resident` | `true` | メインウィンドウを閉じても、メニューバーで動き続ける。 |
| `sync.enabled` | `false` | 収集とダイジェストのあと、選んだ保存先 (Google ドライブか NAS) へ同期する。Google へのログイン、または Tailscale のログインと NAS のアドレスが要る。[クラウド同期](12-cloud-sync.ja.md)を参照。 |

不正な時刻や曜日の値は無視され、既定値が使われます。

スケジューラーは、実行済みの記録として `schedule.last_collect_at`、`schedule.last_daily_for`、`schedule.last_weekly_for`、`schedule.last_monthly_for` も持ちます。編集しないでください。

同期も専用のキーを持ちます。`sync.device_name` (この Mac の名前。同期タブで編集)、`sync.device_id` (最初に既読かスターを変えたとき、または初回の同期の早いほうで作るランダムな id)、`sync.db_id` (初回の同期で作るランダムな id)、`sync.account_email`、`sync.last_sync_at`、`sync.last_compaction_at` です。保存先の選択で、さらにキーが増えます。`sync.backend` (`gdrive` が既定、または `nas`。それ以外の値は `gdrive` として読む)、`sync.nas.url`、`sync.nas.user`、`sync.nas.password_set` (パスワードを保存したかどうかだけ)、`sync.reset_pending` (保存先が変わると立ち、次の同期が新しい保存先で最初からやり直す印) です。NAS のパスワード本体は macOS のキーチェーン (サービス `jp.infodesk.app.nas`)、Tailscale の状態はアプリのデータフォルダの `tailscale/` にあります。[NAS の準備](13-nas-setup.ja.md)を参照。普通の設定は `sync.enabled` だけで、ほかは同期タブが管理します。編集しないでください。Google の更新用トークンは DB ではなく macOS のキーチェーンにあります。

アプリは上のキーだけを受け付けます。`app.launch_at_login` は macOS の LaunchAgent の登録も行うため、切替スイッチで変更します。[CLI](08-cli.ja.md) の `config set` は、キーを確かめずに何でも書き込むので、入力に注意してください。

## メニューバー

InfoDesk は macOS のメニューバーにアイコンを置きます。押すと次のメニューが開きます。

- 状態の行: 最後の収集の時刻 (「最終収集 HH:MM」)、収集中は「収集中…」、未収集なら「未収集」。
- 未読の件数と当日の要約の状態。例: 「未読 12件 · 日次要約 生成済み」。
- 「今すぐ収集」: すぐに収集します。
- 「ウィンドウを開く」(Cmd+O): メインウィンドウを出します。
- 「設定…」(Cmd+,): 設定を開きます。
- 「InfoDesk を終了」(Cmd+Q): 終了します。

## 常駐とログイン時の起動

- **常駐** (`app.stay_resident`、既定はオン): メインウィンドウを閉じると隠すだけです。収集と要約のジョブはメニューバーで動き続けます。オフにすると、ウィンドウを閉じたときに終了します。閲覧ウィンドウは常に普通に閉じます。
- **ログイン時に起動** (`app.launch_at_login`、既定はオフ): ログイン時に、ウィンドウなしでメニューバーに InfoDesk を起動します。`--hidden` 付きでアプリを起動する macOS の LaunchAgent を登録します。
- 完全に終了するには、メニューバーの「InfoDesk を終了」を使います。
