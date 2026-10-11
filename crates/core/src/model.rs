use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    Github,
    Arxiv,
    Huggingface,
    Hackernews,
    TechJp,
    TechIntl,
    Medcare,
}

impl Category {
    pub const ALL: [Category; 7] = [
        Category::Github,
        Category::Arxiv,
        Category::Huggingface,
        Category::Hackernews,
        Category::TechJp,
        Category::TechIntl,
        Category::Medcare,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Category::Github => "github",
            Category::Arxiv => "arxiv",
            Category::Huggingface => "huggingface",
            Category::Hackernews => "hackernews",
            Category::TechJp => "tech_jp",
            Category::TechIntl => "tech_intl",
            Category::Medcare => "medcare",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|c| c.as_str() == s)
    }

    pub fn label_ja(self) -> &'static str {
        match self {
            Category::Github => "GitHub",
            Category::Arxiv => "arXiv",
            Category::Huggingface => "Hugging Face",
            Category::Hackernews => "Hacker News",
            Category::TechJp => "国内tech",
            Category::TechIntl => "海外tech",
            Category::Medcare => "医療介護",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceKind {
    GithubTrending,
    Arxiv,
    HfModels,
    HfPapers,
    Hackernews,
    Rss,
}

impl SourceKind {
    pub const ALL: [SourceKind; 6] = [
        SourceKind::GithubTrending,
        SourceKind::Arxiv,
        SourceKind::HfModels,
        SourceKind::HfPapers,
        SourceKind::Hackernews,
        SourceKind::Rss,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            SourceKind::GithubTrending => "github_trending",
            SourceKind::Arxiv => "arxiv",
            SourceKind::HfModels => "hf_models",
            SourceKind::HfPapers => "hf_papers",
            SourceKind::Hackernews => "hackernews",
            SourceKind::Rss => "rss",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.as_str() == s)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Period {
    Day,
    Week,
    Month,
}

impl Period {
    pub fn as_str(self) -> &'static str {
        match self {
            Period::Day => "day",
            Period::Week => "week",
            Period::Month => "month",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        [Period::Day, Period::Week, Period::Month]
            .into_iter()
            .find(|p| p.as_str() == s)
    }

    pub fn label_ja(self) -> &'static str {
        match self {
            Period::Day => "今日",
            Period::Week => "今週",
            Period::Month => "今月",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DigestStatus {
    Pending,
    Done,
    Failed,
}

impl DigestStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            DigestStatus::Pending => "pending",
            DigestStatus::Done => "done",
            DigestStatus::Failed => "failed",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        [
            DigestStatus::Pending,
            DigestStatus::Done,
            DigestStatus::Failed,
        ]
        .into_iter()
        .find(|p| p.as_str() == s)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DigestScope {
    Category(Category),
    Topic(i64),
}

impl DigestScope {
    /// DBの `digests.scope` 列に入れる文字列。例: `category:github`、`topic:3`。
    pub fn key(&self) -> String {
        match self {
            DigestScope::Category(c) => format!("category:{}", c.as_str()),
            DigestScope::Topic(id) => format!("topic:{id}"),
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        let (kind, rest) = s.split_once(':')?;
        match kind {
            "category" => Category::parse(rest).map(DigestScope::Category),
            "topic" => rest.parse().ok().map(DigestScope::Topic),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Source {
    pub id: i64,
    pub kind: SourceKind,
    pub name: String,
    pub category: Category,
    /// RSSはフィードURL。arxivは分類 (`cs.AI`)。github_trendingは `daily|weekly|monthly`。他は識別用の固定文字列。
    pub config: String,
    pub enabled: bool,
    pub last_fetched_at: Option<DateTime<Utc>>,
    pub last_error: Option<String>,
}

/// collectorが返す、保存前の記事。
#[derive(Debug, Clone, PartialEq)]
pub struct NewItem {
    pub url: String,
    pub title: String,
    pub excerpt: String,
    pub author: Option<String>,
    pub published_at: Option<DateTime<Utc>>,
    pub score: Option<i64>,
    pub image_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Item {
    pub id: i64,
    /// ソースが削除された記事は None。
    pub source_id: Option<i64>,
    pub source_name: String,
    pub category: Category,
    pub url: String,
    pub title: String,
    pub excerpt: String,
    pub author: Option<String>,
    pub published_at: Option<DateTime<Utc>>,
    pub fetched_at: DateTime<Utc>,
    pub score: Option<i64>,
    pub ai_summary: Option<String>,
    pub read: bool,
    pub starred: bool,
    pub image_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChatThread {
    pub id: i64,
    pub title: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Topic {
    pub id: i64,
    pub name: String,
    pub keywords: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Digest {
    pub id: i64,
    pub period: Period,
    pub period_start: NaiveDate,
    pub scope: DigestScope,
    pub body: String,
    pub item_ids: Vec<i64>,
    pub provider: String,
    pub status: DigestStatus,
    pub attempts: i64,
    pub last_error: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ItemOrder {
    /// 公開日時 (なければ取得日時) の新しい順。
    #[default]
    Recent,
    /// 指標の大きい順。指標なしは後ろ。
    Score,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ItemFilter {
    pub category: Option<Category>,
    pub topic_id: Option<i64>,
    pub source_id: Option<i64>,
    /// `fetched_at >= since`
    pub since: Option<DateTime<Utc>>,
    /// `fetched_at < until`
    pub until: Option<DateTime<Utc>>,
    pub text: Option<String>,
    pub unread_only: bool,
    pub starred_only: bool,
    pub order: ItemOrder,
    /// 0 のときは 100 件。
    pub limit: u32,
    pub offset: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CategoryStat {
    pub category: Category,
    pub count: i64,
    /// 日の期間では空。週は7要素、月はその月の日数。期間の初日から順。
    pub daily: Vec<i64>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TopicStat {
    pub topic_id: i64,
    pub name: String,
    pub count: i64,
    pub prev_count: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DigestStats {
    pub period: Period,
    pub period_start: NaiveDate,
    pub total: i64,
    pub categories: Vec<CategoryStat>,
    pub topics: Vec<TopicStat>,
}
