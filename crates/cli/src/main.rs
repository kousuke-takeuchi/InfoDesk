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
    Get { key: String },
    Set { key: String, value: String },
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
    let outcomes = collector::fetch_all(&http, &store.list_sources()?).await;
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
        Some(s) => NaiveDate::parse_from_str(&s, "%Y-%m-%d")
            .map_err(|_| Error::Invalid(format!("invalid date '{s}'")))?,
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

fn items(
    store: &Store,
    category: Option<String>,
    topic: Option<String>,
    days: Option<i64>,
    text: Option<String>,
    limit: u32,
) -> Result<()> {
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
        ConfigCommand::Get { key } => match store.get_setting(&key)? {
            Some(v) => println!("{v}"),
            None => println!("(unset)"),
        },
        ConfigCommand::Set { key, value } => store.set_setting(&key, &value)?,
    }
    Ok(())
}

async fn run(cli: Cli) -> Result<()> {
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
        } => items(&store, category, topic, days, text, limit),
        Command::Source { command } => source(&store, command).await,
        Command::Topic { command: c } => topic(&store, c),
        Command::Config { command: c } => config(&store, c),
    }
}

#[tokio::main]
async fn main() {
    if let Err(e) = run(Cli::parse()).await {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}
