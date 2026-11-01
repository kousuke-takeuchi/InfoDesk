**English: [15-home-server.md](15-home-server.md)** | ← [前: モバイルアプリ](14-mobile-app.ja.md)

# 自宅サーバー (k3s と Argo Workflows)

InfoDesk は、Mac のアプリがなくても、自宅の k3s クラスターで動かせます。Argo の CronWorkflow が 5 分ごとに `infodesk` の CLI を起動します。このページでは、何が動くか、Mac からクラスターへ移す手順、更新のしかた、困ったときの対処を説明します。

CLI のコマンドは [CLI](08-cli.ja.md) を、同期フォルダと NAS は[クラウド同期](12-cloud-sync.ja.md)と [NAS の準備](13-nas-setup.ja.md)を参照してください。

## 1. しくみ

- CronWorkflow `infodesk-tick` が、NUC のノード (`takeuchi-nuc11atkc2`) で 5 分ごとに `infodesk tick` を実行します。1 回ごとに、時刻が来た収集、要約、ダイジェストの生成、NAS への同期を、Mac のアプリと同じ決まりで行います。
- 要約とダイジェストは、Ollama や `claude` コマンドではなく、Claude API (`claude_api`) で作ります。
- データは PVC `infodesk-data` の SQLite の DB です。iPhone のアプリは、これまでどおり NAS を読みます。
- 実行の履歴とログは、Argo の画面 (名前空間 `infodesk`) で見られます。

## 2. Mac を止める理由

クラスターへ写す DB には、Mac の同期の端末 id が入っています。Mac のアプリとクラスターが同じ id で同期すると、NAS のファイルを互いに上書きしてしまいます。Mac を読み取り専用にする機能ができるまでは、Mac のアプリの「同期する」をオフにして、アプリを閉じたままにしてください。

## 3. 準備の手順

kubectl と Docker は、Windows PC の WSL で動きます。`ssh "Kousuke Takeuchi@192.168.0.175"` でつないで、`wsl -d Ubuntu-24.04` に入ります。下の `kubectl` で始まるコマンドは、そこで実行します。リポジトリのファイルも、そこに必要です (WSL でリポジトリを clone するか、`deploy/k8s/` を写します)。

1. Mac で、リポジトリのルートからイメージを作って送ります。イメージの名前が出ます。

   ```sh
   deploy/k8s/build-and-push.sh
   ```

2. 出たタグ (`infodesk:` の後ろの部分) を、`deploy/k8s/workflowtemplate.yaml` と `deploy/k8s/shell.yaml` の `TAG` の代わりに書きます。
3. 名前空間、ボリューム、権限を作ります。

   ```sh
   kubectl apply -f deploy/k8s/namespace.yaml -f deploy/k8s/pvc.yaml -f deploy/k8s/rbac.yaml
   ```

4. Secret を作ります。値は自分で入力します。ファイルにも git にも書きません (`deploy/k8s/secret.example.yaml` を参照)。

   ```sh
   kubectl -n infodesk create secret generic infodesk-env \
     --from-literal=ANTHROPIC_API_KEY='<Claude Console のキー>' \
     --from-literal=INFODESK_NAS_PASSWORD='<WebDAV ユーザーの DSM のパスワード>'
   ```

5. Mac で、設定の「同期する」をオフにして、InfoDesk を終了します。そのあと、DB の整合したコピーを作ります。

   ```sh
   sqlite3 "$HOME/Library/Application Support/jp.infodesk.app/infodesk.db" ".backup /tmp/infodesk.db"
   ```

   `/tmp/infodesk.db` を `scp` で Windows PC へ送ります。WSL からは Windows のディスクが `/mnt/c/...` に見えるので、そこから WSL の中へコピーします。
6. シェル用の Pod を起動して、DB を入れます。

   ```sh
   kubectl apply -f deploy/k8s/shell.yaml
   kubectl -n infodesk cp /tmp/infodesk.db infodesk-shell:/data/infodesk.db
   ```

7. クラスター用に設定を変えます。

   ```sh
   cli="kubectl -n infodesk exec infodesk-shell -- infodesk --db /data/infodesk.db"
   $cli config set llm.items_provider claude_api
   $cli config set llm.digest_provider claude_api
   $cli config set sync.nas.url http://192.168.0.151:5005/infodesk/
   $cli config set sync.device_name infodesk-k3s
   ```

8. 同期を 1 回行って NAS への接続を確かめ、シェル用の Pod を消します。

   ```sh
   $cli sync
   kubectl -n infodesk delete pod infodesk-shell
   ```

9. WorkflowTemplate を適用し、手で 1 回実行して、ログを読みます。

   ```sh
   kubectl apply -f deploy/k8s/workflowtemplate.yaml
   kubectl create -f deploy/k8s/run-once.yaml
   kubectl -n infodesk logs -l workflows.argoproj.io/workflow --tail=100
   ```

10. ログに問題がなければ、スケジュールを始めます。

    ```sh
    kubectl apply -f deploy/k8s/cronworkflow.yaml
    ```

## 4. 更新

Mac で `deploy/k8s/build-and-push.sh` を実行し、新しいタグを `deploy/k8s/workflowtemplate.yaml` に書いて、適用します。

```sh
kubectl apply -f deploy/k8s/workflowtemplate.yaml
```

次の実行から新しいイメージを使います。

## 5. Claude API の設定

| キー | 既定 | 内容 |
|---|---|---|
| `claude_api.model` | `claude-sonnet-5-5` | 要約とダイジェストのモデル。 |
| `claude_api.items_effort` | `low` | 記事要約の effort。`low`、`medium`、`high`、`xhigh`、`max`、または空。 |
| `claude_api.digest_effort` | 空 | ダイジェストの effort。空は API の既定です。 |

API キーは Secret `infodesk-env` にだけあります。DB に保存せず、表示もログ出力もしません。

費用の見積もり: 1 日に記事要約が約 140 件 (7 カテゴリー × `summary.top_n` の 20)、1 件あたり入力が約 3,000 トークン、出力が約 500 トークン、これに数件のダイジェスト。Sonnet 5.5 の料金 (100 万トークンあたり $2 と $10) で、1 日あたり約 $1.5、1 か月で約 $45 です。考えるトークンは出力として数えるので、実際の額はモデルがどれだけ考えるかで変わります。`low` の effort ならそこを小さく抑えられ、`summary.top_n` を下げれば費用も比例して下がります。最初の 1 週間のあとで、Claude Console で実際の費用と要約の質を確かめてください。要約が薄いときは、`claude_api.items_effort` を `medium` にします。

## 6. ソース、トピック、設定を変える

シェル用の Pod を起動して、その中で CLI を実行し、終わったら Pod を消します。

```sh
kubectl apply -f deploy/k8s/shell.yaml
kubectl -n infodesk exec infodesk-shell -- infodesk --db /data/infodesk.db source list
kubectl -n infodesk exec infodesk-shell -- infodesk --db /data/infodesk.db config set summary.top_n 10
kubectl -n infodesk delete pod infodesk-shell
```

コマンドは [CLI](08-cli.ja.md) (`source`、`topic`、`config`)、キーは[設定](07-settings.ja.md)を参照してください。

## 7. NAS のアドレス

NAS の URL は LAN の IP アドレスで書きます: `http://192.168.0.151:5005/infodesk/`。名前は受け付けません。アドレスが変わらないように、NAS にはルーターの DHCP の固定割り当てなどで、固定のアドレスを与えてください。

## 8. 困ったとき

`tick` は、手順ごとに 1 行をログに出します。DB を読めないとき以外は終了コード 0 なので、失敗した手順は、実行の失敗ではなくログに出ます。

| メッセージ | 意味と対処 |
|---|---|
| `sync: error NAS の URL は LAN の IP アドレスで書いてください (例: http://192.168.0.151:5005/infodesk/)` | `sync.nas.url` を IP アドレスの形にします。 |
| `sync: error NAS の URL、ユーザー名、パスワードを入力してください` | Secret に `INFODESK_NAS_PASSWORD` があるか、`sync.nas.user` が入っているかを確かめます。 |
| `sync: error ネットワークに接続できません…` | NAS がスリープしているか、アドレスが変わっています。 |
| `summaries: error ANTHROPIC_API_KEY is not set` | Secret を作り直します。次の実行から読み込みます。 |
| `claude_api 401 ... the API key was rejected` | Claude Console で新しいキーを作り、Secret を作り直します。 |
| `claude_api 429` または `529` | 一時的なものです。ダイジェストは未生成のまま残り、15 分ごとにやり直します。 |
| 実行が `Pending` のまま | PVC は NUC のノードに結び付いています。そのノードが Ready か確かめます。 |
