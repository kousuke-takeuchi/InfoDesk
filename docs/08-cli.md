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

Every command except `page` opens a SQLite database. The default is `~/Library/Application Support/InfoDesk/infodesk.db`.

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
  digests    保存済みの期間要約を表示する (生成はしない)
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
infodesk summarize
```

Adds an article summary to the top articles collected today, per category. The count per category is `summary.top_n` (default 20). The provider is `llm.items_provider`.

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
