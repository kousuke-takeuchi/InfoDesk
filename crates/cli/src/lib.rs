mod json;

use std::path::PathBuf;

use chrono::{FixedOffset, Local, NaiveDate, Utc};
use clap::{Parser, Subcommand};
use infodesk_core::collector::{self, feed};
use infodesk_core::llm::{LlmRole, provider_from_settings};
use infodesk_core::model::{
    Category, DigestScope, DigestStatus, ItemFilter, ItemOrder, Period, SourceKind,
};
use infodesk_core::period::{period_start, utc_range};
use infodesk_core::{Error, Result, Store, seed, summarizer};

#[derive(Parser)]
#[command(name = "infodesk", about = "情報収集と要約")]
struct Cli {
    /// DBファイルのパス。既定は ~/Library/Application Support/InfoDesk/infodesk.db
    #[arg(long, global = true)]
    db: Option<PathBuf>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// 有効なソースをすべて取得する
    Collect,
    /// 今日取得した記事の上位に要約を付ける
    Summarize,
    /// 期間要約を生成して表示する
    Digest {
        /// day | week | month
        period: String,
        /// 対象日 (YYYY-MM-DD)。既定は今日
        #[arg(long)]
        date: Option<String>,
        /// トピック名。指定するとトピック別要約になる
        #[arg(long)]
        topic: Option<String>,
        /// 生成済みでも作り直す
        #[arg(long)]
        force: bool,
    },
    /// 記事を一覧する
    Items {
        #[arg(long)]
        category: Option<String>,
        #[arg(long)]
        topic: Option<String>,
        /// 直近N日に取得した記事に絞る
        #[arg(long)]
        days: Option<i64>,
        #[arg(long)]
        text: Option<String>,
        #[arg(long, default_value_t = 30)]
        limit: u32,
        /// この日 (YYYY-MM-DD、ローカル) の0時以降
        #[arg(long, requires = "json")]
        from: Option<String>,
        /// この日 (YYYY-MM-DD、ローカル) の終わりまで
        #[arg(long, requires = "json")]
        to: Option<String>,
        /// JSONで出力する
        #[arg(long)]
        json: bool,
    },
    /// 記事を1件表示する
    Item {
        id: i64,
        /// JSONで出力する
        #[arg(long)]
        json: bool,
    },
    /// 保存済みの期間要約を表示する (生成はしない)
    Digests {
        /// day | week | month
        period: String,
        /// 対象日 (YYYY-MM-DD)。既定は今日
        #[arg(long)]
        date: Option<String>,
        #[arg(long, conflicts_with = "topic")]
        category: Option<String>,
        /// トピック名
        #[arg(long)]
        topic: Option<String>,
        /// JSONで出力する
        #[arg(long)]
        json: bool,
    },
    /// URL の本文をテキストで取得する (DB は使わない)
    Page {
        url: String,
        /// 本文の最大文字数
        #[arg(long, default_value_t = infodesk_core::page::DEFAULT_MAX_CHARS)]
        max_chars: usize,
        /// JSONで出力する
        #[arg(long)]
        json: bool,
    },
    Source {
        #[command(subcommand)]
        command: SourceCommand,
    },
    Topic {
        #[command(subcommand)]
        command: TopicCommand,
    },
    Config {
        #[command(subcommand)]
        command: ConfigCommand,
    },
}

#[derive(Subcommand)]
enum SourceCommand {
    List,
    /// RSSフィードを検証して追加する
    AddRss {
        url: String,
        /// github | arxiv | huggingface | hackernews | tech_jp | tech_intl | medcare
        #[arg(long)]
        category: String,
        /// 表示名。省略するとフィードのタイトル
        #[arg(long)]
        name: Option<String>,
    },
    Remove {
        id: i64,
    },
    Enable {
        id: i64,
    },
    Disable {
        id: i64,
    },
}

#[derive(Subcommand)]
enum TopicCommand {
    List,
    Add {
        name: String,
        #[arg(required = true)]
        keywords: Vec<String>,
    },
    Remove {
        id: i64,
    },
}

#[derive(Subcommand)]
enum ConfigCommand {
    List {
        /// JSONで出力する
        #[arg(long)]
        json: bool,
    },
    Get {
        key: String,
    },
    Set {
        key: String,
        value: String,
    },
}

fn default_db_path() -> Result<PathBuf> {
    let home = std::env::var("HOME").map_err(|_| Error::Invalid("HOME is not set".into()))?;
    Ok(PathBuf::from(home).join("Library/Application Support/InfoDesk/infodesk.db"))
}

fn local_tz() -> FixedOffset {
    *Local::now().offset()
}

fn today() -> NaiveDate {
    Local::now().date_naive()
}

fn parse_date(s: &str) -> Result<NaiveDate> {
    NaiveDate::parse_from_str(s, "%Y-%m-%d")
        .map_err(|_| Error::Invalid(format!("invalid date '{s}'")))
}

fn print_json(v: &serde_json::Value) -> Result<()> {
    let s = serde_json::to_string(v).map_err(|e| Error::Invalid(e.to_string()))?;
    println!("{s}");
    Ok(())
}

fn parse_category(s: &str) -> Result<Category> {
    Category::parse(s).ok_or_else(|| Error::Invalid(format!("unknown category '{s}'")))
}

fn topic_id(store: &Store, name: &str) -> Result<i64> {
    store
        .find_topic_by_name(name)?
        .map(|t| t.id)
        .ok_or_else(|| Error::Invalid(format!("topic '{name}' not found")))
}

async fn collect(store: &Store) -> Result<()> {
    let http = collector::http_client()?;
    let mut outcomes = collector::fetch_all(&http, &store.list_sources()?).await;
    let urls: Vec<String> = outcomes
        .iter()
        .filter_map(|o| o.result.as_ref().ok())
        .flatten()
        .map(|i| i.url.clone())
        .collect();
    let known = store.existing_urls(&urls)?;
    collector::images::fill_page_images(&mut outcomes, &known).await;
    for r in collector::apply_outcomes(store, &outcomes, &Utc::now())? {
        match r.error {
            None => println!("ok    {:>4} new  {}", r.new_items, r.name),
            Some(e) => println!("FAIL             {}: {e}", r.name),
        }
    }
    Ok(())
}

async fn summarize(store: &Store) -> Result<()> {
    let llm = provider_from_settings(store, LlmRole::Items)?;
    let limit: u32 = store
        .get_setting_or("summary.top_n", "20")?
        .parse()
        .map_err(|_| Error::Invalid("summary.top_n must be a number".into()))?;
    let (since, _) = utc_range(Period::Day, today(), local_tz());
    for category in Category::ALL {
        let report =
            summarizer::summarize_top_items(store, llm.as_ref(), category, &since, limit).await?;
        let detail = report.error.map(|e| format!(" ({e})")).unwrap_or_default();
        println!(
            "{:<12} summarized {}, failed {}{detail}",
            category.as_str(),
            report.ok,
            report.failed
        );
    }
    Ok(())
}

async fn digest(
    store: &Store,
    period: &str,
    date: Option<String>,
    topic: Option<String>,
    force: bool,
) -> Result<()> {
    let period = Period::parse(period)
        .ok_or_else(|| Error::Invalid(format!("unknown period '{period}'")))?;
    let date = match date {
        Some(s) => parse_date(&s)?,
        None => today(),
    };
    let scopes: Vec<DigestScope> = match topic {
        Some(name) => vec![DigestScope::Topic(topic_id(store, &name)?)],
        None => Category::ALL
            .into_iter()
            .map(DigestScope::Category)
            .collect(),
    };
    let llm = provider_from_settings(store, LlmRole::Digest)?;
    let start = period_start(period, date);
    for scope in &scopes {
        let outcome = summarizer::run_digest(
            store,
            llm.as_ref(),
            period,
            date,
            scope,
            local_tz(),
            force,
            &Utc::now(),
        )
        .await?;
        let saved = store.get_digest(period, start, scope)?;
        let status = outcome.as_ref().map(|o| o.status);
        match (status, saved) {
            (Some(DigestStatus::Pending), _) | (Some(DigestStatus::Failed), _) => {
                let reason = outcome
                    .and_then(|o| o.error)
                    .map(|e| format!(" ({e})"))
                    .unwrap_or_default();
                println!(
                    "## {} ({start}): 生成に失敗。次回に再試行する{reason}\n",
                    scope.key()
                )
            }
            (_, Some(d)) if d.status == DigestStatus::Done => {
                println!(
                    "## {} ({start}) [{}、記事{}件]\n{}\n",
                    scope.key(),
                    d.provider,
                    d.item_ids.len(),
                    d.body
                )
            }
            _ => println!("## {} ({start}): 対象の記事なし\n", scope.key()),
        }
    }
    Ok(())
}

struct ItemsOpts {
    category: Option<String>,
    topic: Option<String>,
    days: Option<i64>,
    text: Option<String>,
    limit: u32,
    from: Option<String>,
    to: Option<String>,
    json: bool,
}

fn items(store: &Store, o: ItemsOpts) -> Result<()> {
    let ItemsOpts {
        category,
        topic,
        days,
        text,
        limit,
        from,
        to,
        json,
    } = o;
    if json {
        let args = json::ItemsArgs {
            category: category.as_deref().map(parse_category).transpose()?,
            topic_id: topic.as_deref().map(|n| topic_id(store, n)).transpose()?,
            days,
            from: from.as_deref().map(parse_date).transpose()?,
            to: to.as_deref().map(parse_date).transpose()?,
            text,
            limit,
        };
        return print_json(&json::items_json(store, &args, local_tz())?);
    }
    let filter = ItemFilter {
        category: category.as_deref().map(parse_category).transpose()?,
        topic_id: topic.as_deref().map(|n| topic_id(store, n)).transpose()?,
        since: days.map(|d| Utc::now() - chrono::Duration::days(d)),
        text,
        order: ItemOrder::Recent,
        limit,
        ..Default::default()
    };
    for item in store.query_items(&filter)? {
        let score = item.score.map(|s| format!(" [{s}]")).unwrap_or_default();
        println!("{}{score}  ({})", item.title, item.source_name);
        println!("  {}", item.url);
        if let Some(summary) = &item.ai_summary {
            println!("  {summary}");
        }
    }
    Ok(())
}

fn item(store: &Store, id: i64, json: bool) -> Result<()> {
    if json {
        return print_json(&json::item_json(store, id)?);
    }
    let item = store
        .get_item(id)?
        .ok_or_else(|| Error::Invalid(format!("item {id} not found")))?;
    println!("{}", item.title);
    println!("{}", item.url);
    let summary = item
        .ai_summary
        .filter(|s| !s.trim().is_empty())
        .unwrap_or(item.excerpt);
    println!("{summary}");
    Ok(())
}

fn digests(
    store: &Store,
    period: &str,
    date: Option<String>,
    category: Option<String>,
    topic: Option<String>,
    json: bool,
) -> Result<()> {
    let period = Period::parse(period)
        .ok_or_else(|| Error::Invalid(format!("unknown period '{period}'")))?;
    let date = match date {
        Some(s) => parse_date(&s)?,
        None => today(),
    };
    let category = category.as_deref().map(parse_category).transpose()?;
    let topic_id = topic.as_deref().map(|n| topic_id(store, n)).transpose()?;
    let v = json::digests_json(store, period, date, category, topic_id)?;
    if json {
        return print_json(&v);
    }
    for d in v["digests"].as_array().into_iter().flatten() {
        println!(
            "## {} [{}]\n{}\n",
            d["label"].as_str().unwrap_or_default(),
            d["status"].as_str().unwrap_or_default(),
            d["body"].as_str().unwrap_or_default()
        );
    }
    Ok(())
}

async fn page(url: &str, max_chars: usize, json: bool) -> Result<()> {
    let p = infodesk_core::page::fetch_page(url, max_chars).await?;
    if json {
        return print_json(&json::page_json(&p));
    }
    println!("{}\n", p.title.as_deref().unwrap_or_default());
    println!("{}", p.text);
    Ok(())
}

async fn source(store: &Store, command: SourceCommand) -> Result<()> {
    match command {
        SourceCommand::List => {
            for s in store.list_sources()? {
                println!(
                    "{:>3} {} {:<12} {:<16} {}  {}{}",
                    s.id,
                    if s.enabled { "on " } else { "off" },
                    s.category.as_str(),
                    s.kind.as_str(),
                    s.name,
                    s.config,
                    s.last_error
                        .map(|e| format!("  ERROR: {e}"))
                        .unwrap_or_default()
                );
            }
        }
        SourceCommand::AddRss {
            url,
            category,
            name,
        } => {
            let category = parse_category(&category)?;
            let info = feed::validate(&collector::http_client()?, &url).await?;
            let name = name.filter(|n| !n.trim().is_empty()).unwrap_or(info.title);
            let id = store.add_source(SourceKind::Rss, &name, category, &url)?;
            println!(
                "added source {id}: {name} ({} items in feed)",
                info.item_count
            );
        }
        SourceCommand::Remove { id } => store.delete_source(id)?,
        SourceCommand::Enable { id } => store.set_source_enabled(id, true)?,
        SourceCommand::Disable { id } => store.set_source_enabled(id, false)?,
    }
    Ok(())
}

fn topic(store: &Store, command: TopicCommand) -> Result<()> {
    match command {
        TopicCommand::List => {
            for t in store.list_topics()? {
                println!("{:>3} {}: {}", t.id, t.name, t.keywords.join(", "));
            }
        }
        TopicCommand::Add { name, keywords } => {
            let id = store.add_topic(&name, &keywords)?;
            println!("added topic {id}: {name}");
        }
        TopicCommand::Remove { id } => store.delete_topic(id)?,
    }
    Ok(())
}

fn config(store: &Store, command: ConfigCommand) -> Result<()> {
    match command {
        ConfigCommand::List { json } => {
            if json {
                print_json(&json::config_json(store)?)?;
            } else {
                for (k, v) in store.list_settings()? {
                    println!("{k} = {v}");
                }
            }
        }
        ConfigCommand::Get { key } => match store.get_setting(&key)? {
            Some(v) => println!("{v}"),
            None => println!("(unset)"),
        },
        ConfigCommand::Set { key, value } => store.set_setting(&key, &value)?,
    }
    Ok(())
}

async fn run(cli: Cli) -> Result<()> {
    // page は DB を使わない。--db は受け取るだけで開かない。
    if let Command::Page {
        url,
        max_chars,
        json,
    } = &cli.command
    {
        return page(url, *max_chars, *json).await;
    }
    let path = match cli.db {
        Some(p) => p,
        None => default_db_path()?,
    };
    let store = Store::open(&path)?;
    seed::seed_defaults(&store)?;
    match cli.command {
        Command::Collect => collect(&store).await,
        Command::Summarize => summarize(&store).await,
        Command::Digest {
            period,
            date,
            topic,
            force,
        } => digest(&store, &period, date, topic, force).await,
        Command::Items {
            category,
            topic,
            days,
            text,
            limit,
            from,
            to,
            json,
        } => items(
            &store,
            ItemsOpts {
                category,
                topic,
                days,
                text,
                limit,
                from,
                to,
                json,
            },
        ),
        Command::Item { id, json } => item(&store, id, json),
        Command::Digests {
            period,
            date,
            category,
            topic,
            json,
        } => digests(&store, &period, date, category, topic, json),
        Command::Page { .. } => unreachable!("handled before the store is opened"),
        Command::Source { command } => source(&store, command).await,
        Command::Topic { command: c } => topic(&store, c),
        Command::Config { command: c } => config(&store, c),
    }
}

/// argv (先頭は argv0) を受け取り、CLI として実行して終了コードを返す。アプリ本体の `--cli` からも呼ぶ。
pub fn run_from<I: IntoIterator<Item = String>>(args: I) -> i32 {
    let cli = match Cli::try_parse_from(args) {
        Ok(c) => c,
        Err(e) => {
            let _ = e.print();
            return e.exit_code();
        }
    };
    let rt = match tokio::runtime::Runtime::new() {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("error: {e}");
            return 1;
        }
    };
    // サイドカーが読む契約は `error: <理由>`。
    match rt.block_on(run(cli)) {
        Ok(()) => 0,
        Err(e) => {
            // Invalid は Display の接頭辞なしで出す。
            match e {
                Error::Invalid(reason) => eprintln!("error: {reason}"),
                e => eprintln!("error: {e}"),
            }
            1
        }
    }
}

#[cfg(test)]
mod entry_tests {
    use super::run_from;

    fn args(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn runs_a_command_and_reports_errors_by_exit_code() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("t.db").display().to_string();
        assert_eq!(
            run_from(args(&["infodesk", "--db", &db, "config", "list", "--json"])),
            0
        );
        assert_eq!(
            run_from(args(&["infodesk", "--db", &db, "item", "1", "--json"])),
            1
        );
        assert_eq!(run_from(args(&["infodesk", "no-such-command"])), 2);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> std::result::Result<Cli, clap::Error> {
        Cli::try_parse_from(std::iter::once("infodesk").chain(args.iter().copied()))
    }

    #[test]
    fn items_from_and_to_require_json() {
        assert!(parse(&["items", "--from", "2026-10-01"]).is_err());
        assert!(parse(&["items", "--to", "2026-10-01"]).is_err());
        assert!(parse(&["items", "--from", "2026-10-01", "--json"]).is_ok());
    }

    #[test]
    fn digests_category_conflicts_with_topic() {
        assert!(parse(&["digests", "day", "--category", "github", "--topic", "t"]).is_err());
        assert!(parse(&["digests", "day", "--category", "github"]).is_ok());
    }

    #[test]
    fn page_accepts_global_db_before_the_subcommand() {
        let cli = parse(&["--db", "/tmp/x.db", "page", "https://a.example/", "--json"]).unwrap();
        assert!(matches!(cli.command, Command::Page { json: true, .. }));
    }
}
