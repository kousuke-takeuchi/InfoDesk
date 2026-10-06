use crate::error::Result;
use crate::model::{Category, SourceKind};
use crate::store::Store;

/// (種類, 表示名, カテゴリー, config)。URLは2026-10-06に取得と解析を確認した。
pub const DEFAULT_SOURCES: &[(SourceKind, &str, Category, &str)] = &[
    (
        SourceKind::GithubTrending,
        "GitHub Trending (日)",
        Category::Github,
        "daily",
    ),
    (
        SourceKind::GithubTrending,
        "GitHub Trending (週)",
        Category::Github,
        "weekly",
    ),
    (
        SourceKind::GithubTrending,
        "GitHub Trending (月)",
        Category::Github,
        "monthly",
    ),
    (SourceKind::Arxiv, "arXiv cs.AI", Category::Arxiv, "cs.AI"),
    (SourceKind::Arxiv, "arXiv cs.CL", Category::Arxiv, "cs.CL"),
    (SourceKind::Arxiv, "arXiv cs.LG", Category::Arxiv, "cs.LG"),
    (
        SourceKind::HfModels,
        "Hugging Face Trending Models",
        Category::Huggingface,
        "trending",
    ),
    (
        SourceKind::HfPapers,
        "Hugging Face Daily Papers",
        Category::Huggingface,
        "daily",
    ),
    (
        SourceKind::Hackernews,
        "Hacker News",
        Category::Hackernews,
        "top",
    ),
    (
        SourceKind::Rss,
        "はてなブックマーク IT",
        Category::TechJp,
        "https://b.hatena.ne.jp/hotentry/it.rss",
    ),
    (
        SourceKind::Rss,
        "Publickey",
        Category::TechJp,
        "https://www.publickey1.jp/atom.xml",
    ),
    (
        SourceKind::Rss,
        "ITmedia",
        Category::TechJp,
        "https://rss.itmedia.co.jp/rss/2.0/itmedia_all.xml",
    ),
    (
        SourceKind::Rss,
        "Zenn",
        Category::TechJp,
        "https://zenn.dev/feed",
    ),
    (
        SourceKind::Rss,
        "TechCrunch",
        Category::TechIntl,
        "https://techcrunch.com/feed/",
    ),
    (
        SourceKind::Rss,
        "The Verge",
        Category::TechIntl,
        "https://www.theverge.com/rss/index.xml",
    ),
    (
        SourceKind::Rss,
        "Ars Technica",
        Category::TechIntl,
        "https://feeds.arstechnica.com/arstechnica/index",
    ),
    (
        SourceKind::Rss,
        "MIT Technology Review",
        Category::TechIntl,
        "https://www.technologyreview.com/feed/",
    ),
    (
        SourceKind::Rss,
        "joint-kaigo.com",
        Category::Medcare,
        "https://www.joint-kaigo.com/feed/",
    ),
    (
        SourceKind::Rss,
        "care-news.jp",
        Category::Medcare,
        "http://www.care-news.jp/media.rss",
    ),
    (
        SourceKind::Rss,
        "Google News: 認知症",
        Category::Medcare,
        "https://news.google.com/rss/search?q=%E8%AA%8D%E7%9F%A5%E7%97%87&hl=ja&gl=JP&ceid=JP:ja",
    ),
    (
        SourceKind::Rss,
        "Google News: 介護",
        Category::Medcare,
        "https://news.google.com/rss/search?q=%E4%BB%8B%E8%AD%B7&hl=ja&gl=JP&ceid=JP:ja",
    ),
    (
        SourceKind::Rss,
        "Google News: dementia",
        Category::Medcare,
        "https://news.google.com/rss/search?q=dementia&hl=en-US&gl=US&ceid=US:en",
    ),
];

/// 設定のキーと既定値。
pub const DEFAULT_SETTINGS: &[(&str, &str)] = &[
    ("llm.items_provider", "ollama"),
    ("llm.digest_provider", "ollama"),
    ("llm.chat_provider", "ollama"),
    ("ollama.base_url", "http://localhost:11434"),
    ("ollama.model", "gemma4:26b"),
    ("ollama.web_search_api_key", ""),
    ("claude.bin", "claude"),
    ("node.bin", "node"),
    ("summary.top_n", "20"),
    ("schedule.collect_interval_hours", "3"),
    ("schedule.daily_time", "07:00"),
    ("schedule.weekly_day", "mon"),
    ("schedule.weekly_time", "07:30"),
    ("schedule.monthly_time", "08:00"),
    ("app.launch_at_login", "false"),
    ("app.stay_resident", "true"),
];

const SEEDED_KEY: &str = "seeded";

/// 不足している既定設定を補い、初回だけ初期ソースも入れる。
/// 戻り値は追加したソースの件数 (初回以外は 0)。
pub fn seed_defaults(store: &Store) -> Result<usize> {
    for (key, value) in DEFAULT_SETTINGS {
        if store.get_setting(key)?.is_none() {
            store.set_setting(key, value)?;
        }
    }
    if store.get_setting(SEEDED_KEY)?.is_some() {
        return Ok(0);
    }
    for (kind, name, category, config) in DEFAULT_SOURCES {
        store.add_source(*kind, name, *category, config)?;
    }
    store.set_setting(SEEDED_KEY, "1")?;
    Ok(DEFAULT_SOURCES.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seeds_once_and_covers_every_category() {
        let store = Store::open_in_memory().unwrap();
        let added = seed_defaults(&store).unwrap();
        assert_eq!(added, DEFAULT_SOURCES.len());
        let sources = store.list_sources().unwrap();
        for c in Category::ALL {
            assert!(
                sources.iter().any(|s| s.category == c),
                "no source for {c:?}"
            );
        }
        assert!(
            sources
                .iter()
                .filter(|s| s.kind == SourceKind::Rss)
                .all(|s| s.config.starts_with("http"))
        );
        assert_eq!(
            store.get_setting("ollama.model").unwrap().as_deref(),
            Some("gemma4:26b")
        );
        assert_eq!(
            store.get_setting("schedule.daily_time").unwrap().as_deref(),
            Some("07:00")
        );
        assert_eq!(
            store.get_setting("app.stay_resident").unwrap().as_deref(),
            Some("true")
        );

        assert_eq!(seed_defaults(&store).unwrap(), 0);
        assert_eq!(store.list_sources().unwrap().len(), DEFAULT_SOURCES.len());
    }

    #[test]
    fn reseeding_does_not_restore_deleted_sources_or_reset_settings() {
        let store = Store::open_in_memory().unwrap();
        seed_defaults(&store).unwrap();
        let first = store.list_sources().unwrap().remove(0);
        store.delete_source(first.id).unwrap();
        store.set_setting("ollama.model", "other").unwrap();
        seed_defaults(&store).unwrap();
        assert_eq!(
            store.list_sources().unwrap().len(),
            DEFAULT_SOURCES.len() - 1
        );
        assert_eq!(
            store.get_setting("ollama.model").unwrap().as_deref(),
            Some("other")
        );
    }

    #[test]
    fn reseeding_fills_missing_settings_without_touching_existing_ones() {
        let store = Store::open_in_memory().unwrap();
        seed_defaults(&store).unwrap();
        store.set_setting("ollama.model", "other").unwrap();
        store.conn_execute_for_test("DELETE FROM settings WHERE key = 'schedule.daily_time'");
        assert_eq!(seed_defaults(&store).unwrap(), 0);
        assert_eq!(
            store.get_setting("ollama.model").unwrap().as_deref(),
            Some("other")
        );
        assert_eq!(
            store.get_setting("schedule.daily_time").unwrap().as_deref(),
            Some("07:00")
        );
    }
}
