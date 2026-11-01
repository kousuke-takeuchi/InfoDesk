**日本語: [15-home-server.ja.md](15-home-server.ja.md)** | ← [Mobile App](14-mobile-app.md)

# Home Server (k3s and Argo Workflows)

InfoDesk can run without the Mac app, on a home k3s cluster. An Argo CronWorkflow starts the `infodesk` CLI every 5 minutes. This page explains what runs, how to move from the Mac to the cluster, how to update it, and what to do when it fails.

For the CLI commands see [CLI](08-cli.md). For the sync folder and the NAS see [Cloud Sync](12-cloud-sync.md) and [NAS Setup](13-nas-setup.md).

## 1. What it does

- The CronWorkflow `infodesk-tick` runs `infodesk tick` every 5 minutes on the NUC node (`takeuchi-nuc11atkc2`). Each run collects, summarizes, makes digests and syncs to the NAS when it is due, by the same rules as the Mac app.
- The summaries and digests come from the Claude API (`claude_api`), not from Ollama or the `claude` command.
- The data is a SQLite database on the PVC `infodesk-data`. The iPhone app reads the NAS as before, and so does the Mac app in reader mode (section 2).
- Run history and logs are in the Argo UI, in the namespace `infodesk`.

## 2. Reading on the Mac (reader mode)

The cluster is the only writer of the NAS folder. The Mac app must never write it as well: the database you copy to the cluster carries the Mac's sync device id, and two writers overwrite each other's files. The Mac app reads the folder in reader mode instead, like the iPhone app.

To switch the Mac to reader mode, after the cluster's first successful sync (section 3):

1. Open InfoDesk, then Settings, 同期. Check that 保存先 is NAS and the NAS address and user are set.
2. If you want the Mac app to start at login, turn on ログイン時に起動 in the スケジュール tab now. That tab is locked in reader mode.
3. In the 同期 tab, turn on 読むだけ (サーバーが収集). The Mac gets a new sync device id and drops its unsent marks (the cluster already has them in the copied database).
4. Turn on 同期する if it is off.

In reader mode the Mac:

- syncs when it starts, every 10 minutes, on 今すぐ同期 and on ⌘R (⌘R and the sidebar button sync instead of collecting);
- never collects, summarizes or makes digests; the collect and regenerate buttons are hidden, and the menu bar item becomes 今すぐ同期;
- writes only its own change batches (`ops-<device id>-...json`) with the read and starred marks made on the Mac; the cluster folds them in at its next sync and the iPhone sees them;
- keeps articles older than 60 days that are still in its database: it never deletes an article because its day left the folder.

The ソース, トピック and スケジュール tabs are locked in reader mode; change the cluster's settings with the CLI (section 6). The LLM tab stays editable because AI chat still runs on the Mac.

Turning 読むだけ off also turns 同期する off. Do not turn 同期する on again while the cluster runs: the Mac would write the folder as a second writer.

## 3. Set up

kubectl and Docker run on the WSL node of the Windows PC: `ssh "Kousuke Takeuchi@192.168.0.175"`, then `wsl -d Ubuntu-24.04`. The commands below that start with `kubectl` run there. The repository files must be there too (clone the repository in WSL, or copy `deploy/k8s/`).

Apply the files one by one as below. Do not run `kubectl apply -f deploy/k8s/` on the whole folder: `shell.yaml` and `run-once.yaml` are meant to be applied only at the steps that need them, and `TAG` must be filled in first.

1. On the Mac, build and push the image from the repository root. It prints the image name:

   ```sh
   deploy/k8s/build-and-push.sh
   ```

2. Put the printed tag (the part after `infodesk:`) in place of `TAG` in `deploy/k8s/workflowtemplate.yaml` and `deploy/k8s/shell.yaml`.
3. Create the namespace, the volume and the permissions:

   ```sh
   kubectl apply -f deploy/k8s/namespace.yaml -f deploy/k8s/pvc.yaml -f deploy/k8s/rbac.yaml
   ```

4. Create the Secret. Type each value at its prompt; it is not shown on the screen and does not go into the shell history or any file in git. Only the Secret in the cluster keeps it (see `deploy/secret.example.yaml`):

   ```sh
   read -rsp 'ANTHROPIC_API_KEY: ' ANTHROPIC_API_KEY && echo
   read -rsp 'INFODESK_NAS_PASSWORD: ' INFODESK_NAS_PASSWORD && echo
   kubectl -n infodesk create secret generic infodesk-env \
     --from-literal=ANTHROPIC_API_KEY="$ANTHROPIC_API_KEY" \
     --from-literal=INFODESK_NAS_PASSWORD="$INFODESK_NAS_PASSWORD"
   unset ANTHROPIC_API_KEY INFODESK_NAS_PASSWORD
   ```

5. On the Mac, in Settings, turn off 同期する and ログイン時に起動, then quit InfoDesk. Make a consistent copy of the database and send it to the Windows user's home folder:

   ```sh
   sqlite3 "$HOME/Library/Application Support/jp.infodesk.app/infodesk.db" ".backup /tmp/infodesk.db"
   scp /tmp/infodesk.db "Kousuke Takeuchi@192.168.0.175:infodesk.db"
   ```

   Then, in WSL, copy it from the Windows disk into WSL:

   ```sh
   cp "/mnt/c/Users/Kousuke Takeuchi/infodesk.db" /tmp/infodesk.db
   ```

   The Windows profile folder name may differ from the user name. If the `cp` fails, check the name with `ls /mnt/c/Users`.
6. Start the shell pod, wait until it is ready, and copy the database in:

   ```sh
   kubectl apply -f deploy/k8s/shell.yaml
   kubectl -n infodesk wait --for=condition=Ready pod/infodesk-shell --timeout=5m
   kubectl -n infodesk cp /tmp/infodesk.db infodesk-shell:/data/infodesk.db
   ```

7. Change the settings for the cluster. Turning off 同期する in step 5 was copied into the database, so turn it back on here:

   ```sh
   cli="kubectl -n infodesk exec infodesk-shell -- infodesk --db /data/infodesk.db"
   $cli config set llm.items_provider claude_api
   $cli config set llm.digest_provider claude_api
   $cli config set sync.nas.url http://192.168.0.151:5005/infodesk/
   $cli config set sync.device_name infodesk-k3s
   $cli config set sync.enabled true
   ```

8. Check that the storage is the NAS, run one sync to check the NAS connection, then remove the shell pod:

   ```sh
   $cli config get sync.backend
   ```

   It must print `nas`. If it prints anything else, stop here: remove the shell pod (`kubectl -n infodesk delete pod infodesk-shell`), switch 保存先 to NAS in the Mac app's Sync tab (the app clears the old sync records when the storage changes; `config set sync.backend` does not), and start again from step 5. Do not set `sync.backend` with the CLI.

   ```sh
   $cli sync
   ```

   A good sync prints one line that starts with `ok  取り込み` and then the counts. If it prints `同期は無効です (...)`, check `sync.enabled` and `sync.backend` and run it again. When the sync is good, remove the shell pod:

   ```sh
   kubectl -n infodesk delete pod infodesk-shell
   ```
9. Apply the WorkflowTemplate and run it once by hand. Wait until that run finishes and read its log:

   ```sh
   kubectl apply -f deploy/k8s/workflowtemplate.yaml
   wf="$(kubectl create -f deploy/k8s/run-once.yaml -o name)"
   wf="${wf#*/}"
   until [ -n "$(kubectl -n infodesk get workflow "$wf" -o jsonpath='{.status.finishedAt}')" ]; do sleep 10; done
   pod="$(kubectl -n infodesk get pods -l workflows.argoproj.io/workflow="$wf" -o name)"
   kubectl -n infodesk logs "$pod" -c main
   ```

10. When the log looks right, start the schedule:

    ```sh
    kubectl apply -f deploy/k8s/cronworkflow.yaml
    ```

11. If the NAS address is not fixed yet, add a DHCP reservation for `192.168.0.151` in the router (see section 7).

When the first `infodesk-tick` run has synced to the NAS, switch the Mac to reader mode (section 2).

## 4. Updating

Run `deploy/k8s/build-and-push.sh` on the Mac, put the new tag in `deploy/k8s/workflowtemplate.yaml` and `deploy/k8s/shell.yaml`, and apply the WorkflowTemplate:

```sh
kubectl apply -f deploy/k8s/workflowtemplate.yaml
```

The next run uses the new image. The shell pod uses the new tag the next time you apply `shell.yaml`.

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
kubectl -n infodesk wait --for=condition=Ready pod/infodesk-shell --timeout=5m
kubectl -n infodesk exec infodesk-shell -- infodesk --db /data/infodesk.db source list
kubectl -n infodesk exec infodesk-shell -- infodesk --db /data/infodesk.db config set summary.top_n 10
kubectl -n infodesk delete pod infodesk-shell
```

The commands are in [CLI](08-cli.md) (`source`, `topic`, `config`); the keys are in [Settings](07-settings.md).

## 7. NAS address

The NAS URL must be a LAN IP address: `http://192.168.0.151:5005/infodesk/`. A name is refused. Give the NAS a fixed address (a DHCP reservation in the router) so the address does not change.

## 8. Filling a collection gap

If nothing collected for some days (for example the Mac stopped before the cluster took over), the first collection afterwards files every new article under its own day, and the gap days get no articles and no daily digest. `infodesk redate` moves the articles published during the gap back to their publish day. Articles with no publish date (GitHub Trending) stay where they are. See [the CLI reference](08-cli.md#redate).

1. Stop the scheduled runs and wait until no run is in progress:

   ```bash
   kubectl -n infodesk patch cronworkflow infodesk-tick --type merge -p '{"spec":{"suspend":true}}'
   kubectl -n infodesk get workflows
   ```

   Wait until no workflow shows `Running`.

2. Start the shell pod (section 6) and check what would move:

   ```bash
   cli="kubectl -n infodesk exec infodesk-shell -- infodesk --db /data/infodesk.db"
   $cli redate --fetched-on 2026-11-01 --from 2026-10-27 --to 2026-10-31
   ```

3. If the counts look right, run it again with `--apply`, then run every command it prints under `then run:`, in order, with `kubectl -n infodesk exec infodesk-shell --` in front. They add article summaries to the moved days, make the digests, and end with `sync`. Keep a copy of this output: if a command fails partway, running `redate` again prints only `nothing to move`, because the articles have already moved.

4. Delete the shell pod and start the scheduled runs again:

   ```bash
   kubectl -n infodesk delete pod infodesk-shell
   kubectl -n infodesk patch cronworkflow infodesk-tick --type merge -p '{"spec":{"suspend":false}}'
   ```

The summarize and digest commands call the Claude API; digests make one call per category for each day, week and month listed.

## 9. Troubleshooting

The `tick` command writes one line per step to the log. It exits 0 unless the database cannot be read, so a failed step shows in the log, not as a failed run.

| Message | Meaning and fix |
|---|---|
| `sync: error NAS の URL は LAN の IP アドレスで書いてください (例: http://192.168.0.151:5005/infodesk/)` | Set `sync.nas.url` to the IP form. |
| `sync: error NAS の URL、ユーザー名、パスワードを入力してください` | Check that the Secret has `INFODESK_NAS_PASSWORD` and that `sync.nas.user` is set. |
| `sync: error ネットワークに接続できません。次の同期で再試行します。` | The NAS is asleep or its address changed. |
| `summaries: error ANTHROPIC_API_KEY is not set`, `digest daily <date>: error ANTHROPIC_API_KEY is not set` | The Secret has no API key. Recreate the Secret (`kubectl -n infodesk delete secret infodesk-env`, then the commands below). |
| `summaries: 0 ok, error llm: claude_api 401 Unauthorized: the API key was rejected`, `digest daily <date>: done, pending: ["llm: claude_api 401 Unauthorized: the API key was rejected"]` | Create a new key in the Claude Console and recreate the Secret (`kubectl -n infodesk delete secret infodesk-env`, then the commands below). |
| `... error llm: claude_api 429 Too Many Requests: ...` or `... claude_api 529 <unknown status code>: ...` | Temporary. Digests stay pending and are retried every 15 minutes. |
| A run stays `Pending` | The PVC is bound to the NUC node. Check that the node is Ready. |

### Recreating the Secret

A Secret is not changed in place. Delete it, then create it again with the same commands as in step 4:

```sh
kubectl -n infodesk delete secret infodesk-env
read -rsp 'ANTHROPIC_API_KEY: ' ANTHROPIC_API_KEY && echo
read -rsp 'INFODESK_NAS_PASSWORD: ' INFODESK_NAS_PASSWORD && echo
kubectl -n infodesk create secret generic infodesk-env \
  --from-literal=ANTHROPIC_API_KEY="$ANTHROPIC_API_KEY" \
  --from-literal=INFODESK_NAS_PASSWORD="$INFODESK_NAS_PASSWORD"
unset ANTHROPIC_API_KEY INFODESK_NAS_PASSWORD
```

The next run reads the new values.

