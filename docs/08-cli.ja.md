**English: [08-cli.md](08-cli.md)** | ← [前: 設定](07-settings.ja.md) | 次: [プライバシーとセキュリティ →](09-privacy-and-security.ja.md)

# CLI

`infodesk` は、アプリと同じ core ライブラリの上に作ったコマンドラインツールです。端末から、収集、要約、ダイジェストの表示、ソースとトピックと設定の編集ができます。チャットのエージェントも、この CLI でデータを読みます。

コマンドのヘルプと `--db` の既定値の説明は日本語です。このページには使い方の行をそのまま載せています。

## 実行する

リポジトリから:

```sh
cargo run -q -p infodesk -- --help
cargo run -q -p infodesk -- items --days 3
```

または、一度ビルドしてバイナリを使います。

```sh
cargo build --release -p infodesk
./target/release/infodesk --help
```

## DB のパスと `--db`

`page` 以外のコマンドは、SQLite の DB を開きます。既定は `~/Library/Application Support/InfoDesk/infodesk.db` です。

アプリは別のファイル `~/Library/Application Support/jp.infodesk.app/infodesk.db` を使います。そのため、CLI は、そこを指定しない限りアプリのデータを見ません。

```sh
infodesk --db "$HOME/Library/Application Support/jp.infodesk.app/infodesk.db" items --days 1
```

`--db` はグローバルオプションで、サブコマンドの前でも後でも指定できます。新しいパスを初めて使うとき、CLI は DB を作り、既定の設定と組み込みソースを入れます。

`page` は DB を使いません。`--db` は受け取りますが、開きません。

## コマンド

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

有効なソースをすべて取得し、新しい記事を保存し、記事の画像を補います。ソースごとに1行を出します。`ok <n> new <name>` または `FAIL <name>: <error>` です。

### summarize

```
infodesk summarize
```

今日取得した記事の上位に、カテゴリーごとに記事要約を付けます。カテゴリーごとの件数は `summary.top_n` (既定20) です。プロバイダーは `llm.items_provider` です。

### digest

```
infodesk digest [OPTIONS] <PERIOD>
```

期間要約を生成して表示します。`<PERIOD>` は `day`、`week`、`month` です。`llm.digest_provider` の LLM を呼びます。

| オプション | 内容 |
|---|---|
| `--date <DATE>` | 対象日 (`YYYY-MM-DD`)。既定は今日。 |
| `--topic <TOPIC>` | トピック名。指定するとカテゴリー別ではなくトピック別の要約になります。 |
| `--force` | 生成済みでも作り直します。 |

`--topic` を省くと、全カテゴリーを順に処理します。保留中や失敗した要約は失敗の行を出し、次回以降に再試行されます。

```sh
infodesk digest day
infodesk digest week --date 2026-10-05 --force
infodesk digest month --topic 認知症
```

### items

```
infodesk items [OPTIONS]
```

収集した記事を新しい順に一覧します。

| オプション | 内容 |
|---|---|
| `--category <CATEGORY>` | `github`、`arxiv`、`huggingface`、`hackernews`、`tech_jp`、`tech_intl`、`medcare` のいずれか。 |
| `--topic <TOPIC>` | トピック名。 |
| `--days <DAYS>` | 直近N日に取得した記事。 |
| `--text <TEXT>` | 一致させる語。 |
| `--limit <LIMIT>` | 最大件数。既定30。 |
| `--from <FROM>` | このローカル日付 (`YYYY-MM-DD`) の0時以降。`--json` が必要です。 |
| `--to <TO>` | このローカル日付の終わりまで。`--json` が必要です。 |
| `--json` | JSON で出力します。 |

```sh
infodesk items --category medcare --days 7 --limit 10
infodesk items --text Rust --from 2026-10-01 --to 2026-10-07 --json
```

### item

```
infodesk item [OPTIONS] <ID>
```

id で記事を1件表示します。`--json` で JSON 出力です。

### digests

```
infodesk digests [OPTIONS] <PERIOD>
```

保存済みの期間要約を表示します。生成はしません (生成は `digest` を使います)。オプションは `--date <DATE>` (既定は今日)、`--category <CATEGORY>`、`--topic <TOPIC>`、`--json` です。`--category` と `--topic` は同時に使えません。どちらも省くと全カテゴリーを表示します。

### page

```
infodesk page [OPTIONS] <URL>
```

URL を取得して本文のテキストを出力します。何も保存しません。ローカルとプライベートのアドレスは拒みます ([プライバシーとセキュリティ](09-privacy-and-security.ja.md)を参照)。

| オプション | 内容 |
|---|---|
| `--max-chars <MAX_CHARS>` | 本文の最大文字数。既定20000。 |
| `--json` | JSON で出力します。 |

### source

```
infodesk source list
infodesk source add-rss [OPTIONS] --category <CATEGORY> <URL>
infodesk source remove <ID>
infodesk source enable <ID>
infodesk source disable <ID>
```

`list` は、id、on か off、カテゴリー、種類、名前、設定、最後のエラーを出します。`add-rss` は、フィードを検証してから追加します。`--name <NAME>` で表示名を付けます (省略するとフィードのタイトル)。`--category` の値は `items` と同じです。

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

`add` にはキーワードが1つ以上必要です。

```sh
infodesk topic add 認知症 認知症 アルツハイマー
```

### config

```
infodesk config list [--json]
infodesk config get <KEY>
infodesk config set <KEY> <VALUE>
```

`list` は `key = value` の行を出力します。`--json` では `{"settings": {...}}` の形の JSON です。`get` は値を出力し、なければ `(unset)` と出力します。`set` は、キーも値も確かめずに書き込みます。キーと既定値は[設定](07-settings.ja.md)にあります。

```sh
infodesk config get ollama.model
infodesk config set llm.digest_provider claude
```

`config list` は `ollama.web_search_api_key` を平文で出力します。

## 終了コードとエラー

成功したコマンドは 0 で終了します。失敗すると、標準エラーに `error: <理由>` を出して 1 で終了します。使い方の誤り (未知のオプション、引数の不足) では、使い方を出して clap の終了コード (2) で終了します。

## アプリ本体の `--cli` モード

アプリ本体は、この CLI としても動きます。最初の引数が `--cli` のとき、残りの引数を `infodesk` の引数として処理します。

```sh
/Applications/InfoDesk.app/Contents/MacOS/infodesk-app --cli items --days 1 --json
```

チャットのエージェントはこの方法で CLI を呼ぶので、インストールしたアプリに別の `infodesk` バイナリは要りません。`--db` を省くと、このモードも CLI の既定のパスを使い、アプリの DB は使いません。チャットのエージェントは、アプリの DB を明示して渡します。
