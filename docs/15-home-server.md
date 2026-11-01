**日本語: [15-home-server.ja.md](15-home-server.ja.md)** | ← [Mobile App](14-mobile-app.md)

# Home Server (k3s and Argo Workflows)

InfoDesk can run without the Mac app, on a home k3s cluster. An Argo CronWorkflow starts the `infodesk` CLI every 5 minutes. This page explains what runs, how to move from the Mac to the cluster, how to update it, and what to do when it fails.

For the CLI commands see [CLI](08-cli.md). For the sync folder and the NAS see [Cloud Sync](12-cloud-sync.md) and [NAS Setup](13-nas-setup.md).

## 1. What it does

- The CronWorkflow `infodesk-tick` runs `infodesk tick` every 5 minutes on the NUC node (`takeuchi-nuc11atkc2`). Each run collects, summarizes, makes digests and syncs to the NAS when it is due, by the same rules as the Mac app.
- The summaries and digests come from the Claude API (`claude_api`), not from Ollama or the `claude` command.
- The data is a SQLite database on the PVC `infodesk-data`. The iPhone app reads the NAS as before.
- Run history and logs are in the Argo UI, in the namespace `infodesk`.

## 2. Why the Mac must stop

The database you copy to the cluster carries the Mac's sync device id. If the Mac app and the cluster both sync with that id, they overwrite each other's files on the NAS. Until a Mac reader mode exists, turn off 同期する in the Mac app and keep the app closed.

## 3. Set up

kubectl and Docker run on the WSL node of the Windows PC: `ssh "Kousuke Takeuchi@192.168.0.175"`, then `wsl -d Ubuntu-24.04`. The commands below that start with `kubectl` run there. The repository files must be there too (clone the repository in WSL, or copy `deploy/k8s/`).

1. On the Mac, build and push the image from the repository root. It prints the image name:

   ```sh
   deploy/k8s/build-and-push.sh
   ```

2. Put the printed tag (the part after `infodesk:`) in place of `TAG` in `deploy/k8s/workflowtemplate.yaml` and `deploy/k8s/shell.yaml`.
3. Create the namespace, the volume and the permissions:

   ```sh
   kubectl apply -f deploy/k8s/namespace.yaml -f deploy/k8s/pvc.yaml -f deploy/k8s/rbac.yaml
   ```

4. Create the Secret. Type the values yourself; they are never written to a file or to git (see `deploy/k8s/secret.example.yaml`):

   ```sh
   kubectl -n infodesk create secret generic infodesk-env \
     --from-literal=ANTHROPIC_API_KEY='<key from the Claude Console>' \
     --from-literal=INFODESK_NAS_PASSWORD='<DSM password of the WebDAV user>'
   ```

5. On the Mac, turn off 同期する in Settings and quit InfoDesk. Then make a consistent copy of the database:

   ```sh
   sqlite3 "$HOME/Library/Application Support/jp.infodesk.app/infodesk.db" ".backup /tmp/infodesk.db"
   ```

   Copy `/tmp/infodesk.db` to the Windows PC with `scp`. WSL sees the Windows disk under `/mnt/c/...`, so copy it from there into WSL.
6. Start the shell pod and copy the database in:

   ```sh
   kubectl apply -f deploy/k8s/shell.yaml
   kubectl -n infodesk cp /tmp/infodesk.db infodesk-shell:/data/infodesk.db
   ```

7. Change the settings for the cluster:

   ```sh
   cli="kubectl -n infodesk exec infodesk-shell -- infodesk --db /data/infodesk.db"
   $cli config set llm.items_provider claude_api
   $cli config set llm.digest_provider claude_api
   $cli config set sync.nas.url http://192.168.0.151:5005/infodesk/
   $cli config set sync.device_name infodesk-k3s
   ```

8. Run one sync to check the NAS connection, then remove the shell pod:

   ```sh
   $cli sync
   kubectl -n infodesk delete pod infodesk-shell
   ```

9. Apply the WorkflowTemplate and run it once by hand. Read the log:

   ```sh
   kubectl apply -f deploy/k8s/workflowtemplate.yaml
   kubectl create -f deploy/k8s/run-once.yaml
   kubectl -n infodesk logs -l workflows.argoproj.io/workflow --tail=100
   ```

10. When the log looks right, start the schedule:

    ```sh
    kubectl apply -f deploy/k8s/cronworkflow.yaml
    ```

## 4. Updating

Run `deploy/k8s/build-and-push.sh` on the Mac, put the new tag in `deploy/k8s/workflowtemplate.yaml`, and apply it:

```sh
kubectl apply -f deploy/k8s/workflowtemplate.yaml
```

The next run uses the new image.

## 5. Claude API settings

| Key | Default | Meaning |
|---|---|---|
| `claude_api.model` | `claude-sonnet-5-5` | Model for summaries and digests. |
| `claude_api.items_effort` | `low` | Effort for article summaries: `low`, `medium`, `high`, `xhigh`, `max`, or empty. |
| `claude_api.digest_effort` | empty | Effort for digests. Empty means the API default. |

The API key is only in the Secret `infodesk-env`. It is never stored in the database, printed or logged.

Cost estimate: about 140 article summaries a day (7 categories times `summary.top_n` 20), about 3,000 input tokens and 500 output tokens each, plus a few digests. At Sonnet 5.5 prices ($2 and $10 per 1M tokens) that is about $1.5 a day, or roughly $45 a month. Thinking tokens are billed as output, so the real figure depends on how much the model thinks. `low` effort keeps that small, and lowering `summary.top_n` lowers the cost in proportion. After the first week, check the real spend in the Claude Console and the quality of the summaries. If the summaries are too thin, set `claude_api.items_effort` to `medium`.

## 6. Changing sources, topics and settings

Apply the shell pod, run the CLI in it, then delete the pod:

```sh
kubectl apply -f deploy/k8s/shell.yaml
kubectl -n infodesk exec infodesk-shell -- infodesk --db /data/infodesk.db source list
kubectl -n infodesk exec infodesk-shell -- infodesk --db /data/infodesk.db config set summary.top_n 10
kubectl -n infodesk delete pod infodesk-shell
```

The commands are in [CLI](08-cli.md) (`source`, `topic`, `config`); the keys are in [Settings](07-settings.md).

## 7. NAS address

The NAS URL must be a LAN IP address: `http://192.168.0.151:5005/infodesk/`. A name is refused. Give the NAS a fixed address (a DHCP reservation in the router) so the address does not change.

## 8. Troubleshooting

The `tick` command writes one line per step to the log. It exits 0 unless the database cannot be read, so a failed step shows in the log, not as a failed run.

| Message | Meaning and fix |
|---|---|
| `sync: error NAS の URL は LAN の IP アドレスで書いてください (例: http://192.168.0.151:5005/infodesk/)` | Set `sync.nas.url` to the IP form. |
| `sync: error NAS の URL、ユーザー名、パスワードを入力してください` | Check that the Secret has `INFODESK_NAS_PASSWORD` and that `sync.nas.user` is set. |
| `sync: error ネットワークに接続できません…` | The NAS is asleep or its address changed. |
| `summaries: error ANTHROPIC_API_KEY is not set` | Recreate the Secret. The next run reads it. |
| `claude_api 401 ... the API key was rejected` | Create a new key in the Claude Console and recreate the Secret. |
| `claude_api 429` or `529` | Temporary. Digests stay pending and are retried every 15 minutes. |
| A run stays `Pending` | The PVC is bound to the NUC node. Check that the node is Ready. |
