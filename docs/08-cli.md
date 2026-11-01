**日本語: [08-cli.ja.md](08-cli.ja.md)** | ← [Settings](07-settings.md) | Next: [Privacy and Security →](09-privacy-and-security.md)

# CLI

`infodesk` is a command line tool built on the same core library as the app. You can collect, summarize, read digests and edit sources, topics and settings from a terminal. The chat agent uses the same CLI to read your data.

Command help text and the default `--db` text are in Japanese. This page shows the exact usage lines.

## Run it

From the repository:

```sh
cargo run -q -p infodesk -- --help
cargo run -q -p infodesk -- items --days 3
```

Or build the binary once:

```sh
cargo build --release -p infodesk
./target/release/infodesk --help
```

## The database path and `--db`

Every command except `page` opens a SQLite database. The default is `~/Library/Application Support/InfoDesk/infodesk.db` on macOS, and `$XDG_DATA_HOME/InfoDesk/infodesk.db` on Linux (`~/.local/share/InfoDesk/infodesk.db` when `XDG_DATA_HOME` is unset or empty).

The app uses a different file: `~/Library/Application Support/jp.infodesk.app/infodesk.db`. So the CLI does not see the app's data unless you point it there:

```sh
infodesk --db "$HOME/Library/Application Support/jp.infodesk.app/infodesk.db" items --days 1
```

`--db` is a global option and can go before or after the subcommand. On first use of a new path, the CLI creates the database and seeds default settings and the built-in sources.

`page` does not use the database. It accepts `--db` but does not open it.

## Commands

```
Usage: infodesk [OPTIONS] <COMMAND>

Commands:
  collect    有効なソースをすべて取得する
  summarize  今日取得した記事の上位に要約を付ける
  digest     期間要約を生成して表示する
  items      記事を一覧する
  item       記事を1件表示する
  sync       NAS へ今すぐ1回同期する (保存先が NAS のときだけ)
  tick       予定を1回だけ評価して、収集・要約・同期のうち時刻が来たものを実行する (k3s の CronWorkflow 用)
  digests    保存済みの期間要約を表示する (生成はしない)
  redate     取得の空白を埋める: 指定日に取得した記事を公開日に移し、作るべき期間要約を表示する
  page       URL の本文をテキストで取得する (DB は使わない)
  source
  topic
  config
```

### collect

```
infodesk collect
```

Fetches every enabled source, saves new articles, and fills article images. Prints one line per source: `ok <n> new <name>` or `FAIL <name>: <error>`.

### summarize

```
infodesk summarize [--date <DATE>]
```

Adds an article summary to the top articles collected today, per category. With `--date YYYY-MM-DD` it uses the articles collected on that day instead (used after `redate`). The count per category is `summary.top_n` (default 20). The provider is `llm.items_provider`.

### sync

```
infodesk sync
```

Syncs once to the NAS now. It works only when the storage is the NAS. When sync is off it prints `同期は無効です (sync.enabled = true と sync.backend = nas を確かめてください)`. Otherwise it prints `ok  取り込み <n>  反映 <n>  送信 <n>  書き込み <n>  削除 <n>`, then a `warn <message>` line for each warning. A failure exits with 1. The NAS password comes from the environment variable `INFODESK_NAS_PASSWORD`, and the NAS URL must be a private IPv4 address. See [Home Server](15-home-server.md).

### tick

```
infodesk tick
```

Checks the schedule once and runs what is due: collection, article summaries, day, week and month digests, and sync to the NAS, by the same rules as the Mac app. It prints one line per step (`collect: ...`, `summaries: ...`, `digest daily <date>: ...`, `sync: ...`) as soon as the step ends, or `tick: nothing due`. While it summarizes articles it also prints `summaries: <n> so far` after every 20 summaries. A step that fails prints its error in the line, for example `summaries: error ANTHROPIC_API_KEY is not set` or `digest daily <date>: error ANTHROPIC_API_KEY is not set`, and the command still exits 0. It exits with 1 only when the database cannot be read. It is made for the k3s CronWorkflow that runs it every 5 minutes; see [Home Server](15-home-server.md).

### digest

```
infodesk digest [OPTIONS] <PERIOD>
```

Generates period digests and prints them. `<PERIOD>` is `day`, `week` or `month`. This calls the LLM from `llm.digest_provider`.

| Option | Meaning |
|---|---|
| `--date <DATE>` | Target date, `YYYY-MM-DD`. Default is today. |
| `--topic <TOPIC>` | Topic name. Makes a topic digest instead of the category digests. |
| `--force` | Regenerate even if already generated. |

Without `--topic` it runs every category. Pending or failed digests print a failure line and are retried on a later run.

```sh
infodesk digest day
infodesk digest week --date 2026-10-05 --force
infodesk digest month --topic 認知症
```

### items

```
infodesk items [OPTIONS]
```

Lists collected articles, newest first.

| Option | Meaning |
|---|---|
| `--category <CATEGORY>` | One of `github`, `arxiv`, `huggingface`, `hackernews`, `tech_jp`, `tech_intl`, `medcare`. |
| `--topic <TOPIC>` | Topic name. |
| `--days <DAYS>` | Articles collected in the last N days. |
| `--text <TEXT>` | Text to match. |
| `--limit <LIMIT>` | Maximum count. Default 30. |
| `--from <FROM>` | From 00:00 of this local date (`YYYY-MM-DD`). Needs `--json`. |
| `--to <TO>` | Until the end of this local date. Needs `--json`. |
| `--json` | Print JSON. |

```sh
infodesk items --category medcare --days 7 --limit 10
infodesk items --text Rust --from 2026-10-01 --to 2026-10-07 --json
```

### item

```
infodesk item [OPTIONS] <ID>
```

Shows one article by id. `--json` prints JSON.

### digests

```
infodesk digests [OPTIONS] <PERIOD>
```

Shows stored digests. It does not generate them (use `digest` for that). Options: `--date <DATE>` (default today), `--category <CATEGORY>`, `--topic <TOPIC>` and `--json`. `--category` and `--topic` cannot be used together. Without either, it shows every category.

### redate

```
infodesk redate --fetched-on <DATE> --from <DATE> --to <DATE> [--apply]
```

Fills a collection gap. Articles collected on `--fetched-on` whose publish date falls on a day from `--from` to `--to` (local time) are moved to their publish date (`fetched_at` becomes `published_at`). Articles with no publish date stay. Without `--apply` it only prints the counts. It then prints the commands to run: `summarize --date` for each day that received articles, the digest commands (day digests first, then week and month, with `--force` where a digest already exists) and `sync`. `--from <= --to < --fetched-on` is required. See [Home Server](15-home-server.md) for the steps on the cluster.

```sh
infodesk redate --fetched-on 2026-11-01 --from 2026-10-27 --to 2026-10-31
infodesk redate --fetched-on 2026-11-01 --from 2026-10-27 --to 2026-10-31 --apply
```

### page

```
infodesk page [OPTIONS] <URL>
```

Fetches a URL and prints its body text. Nothing is saved. The fetch refuses local and private addresses (see [Privacy and Security](09-privacy-and-security.md)).

| Option | Meaning |
|---|---|
| `--max-chars <MAX_CHARS>` | Maximum characters of body text. Default 20000. |
| `--json` | Print JSON. |

### source

```
infodesk source list
infodesk source add-rss [OPTIONS] --category <CATEGORY> <URL>
infodesk source remove <ID>
infodesk source enable <ID>
infodesk source disable <ID>
```

`list` shows id, on or off, category, kind, name, config and the last error. `add-rss` checks the feed first and then adds it. `--name <NAME>` sets the display name (default: the feed title). `--category` takes the same values as `items`.

```sh
infodesk source add-rss https://example.com/feed.xml --category tech_intl --name "Example"
infodesk source disable 3
```

### topic

```
infodesk topic list
infodesk topic add <NAME> <KEYWORDS>...
infodesk topic remove <ID>
```

`add` needs at least one keyword.

```sh
infodesk topic add 認知症 認知症 アルツハイマー
```

### config

```
infodesk config list [--json]
infodesk config get <KEY>
infodesk config set <KEY> <VALUE>
```

`list` prints `key = value` lines, or JSON of the form `{"settings": {...}}` with `--json`. `get` prints the value, or `(unset)`. `set` writes the value without checking the key or value. The keys and defaults are in [Settings](07-settings.md).

```sh
infodesk config get ollama.model
infodesk config set llm.digest_provider claude
```

`config list` shows `ollama.web_search_api_key` in plain text.

## Exit codes and errors

A successful command exits with 0. A failure prints `error: <reason>` to standard error and exits with 1. A usage error (an unknown option, a missing argument) prints the usage and exits with the code clap gives, which is 2.

## The app binary in `--cli` mode

The app binary can act as this CLI. When its first argument is `--cli`, the rest of the arguments are handled as `infodesk` arguments:

```sh
/Applications/InfoDesk.app/Contents/MacOS/infodesk-app --cli items --days 1 --json
```

This is how the chat agent calls the CLI, so an installed app needs no separate `infodesk` binary. Without `--db`, this mode also uses the CLI default path, not the app's database. The chat agent passes the app's database explicitly.
