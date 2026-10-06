pub mod feed;
pub mod github;
pub mod hackernews;
pub mod huggingface;
pub mod text;

use std::time::Duration;

use chrono::{DateTime, Utc};

use crate::error::Result;
use crate::model::{NewItem, Source, SourceKind};
use crate::store::Store;

pub fn http_client() -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .user_agent("InfoDesk/0.1")
        .timeout(Duration::from_secs(30))
        .build()?)
}

const HN_LIMIT: usize = 30;
const HF_LIMIT: usize = 30;

pub fn arxiv_url(category: &str) -> String {
    format!(
        "https://export.arxiv.org/api/query?search_query=cat:{category}&sortBy=submittedDate&sortOrder=descending&max_results=50"
    )
}

/// ソース1件を取得する。種類ごとの取得器に振り分ける。
pub async fn fetch_source(http: &reqwest::Client, source: &Source) -> Result<Vec<NewItem>> {
    match source.kind {
        SourceKind::Rss => feed::fetch(http, &source.config).await,
        SourceKind::Arxiv => feed::fetch(http, &arxiv_url(&source.config)).await,
        SourceKind::GithubTrending => github::fetch(http, github::BASE, &source.config).await,
        SourceKind::Hackernews => hackernews::fetch(http, hackernews::BASE, HN_LIMIT).await,
        SourceKind::HfModels => huggingface::fetch_models(http, huggingface::BASE, HF_LIMIT).await,
        SourceKind::HfPapers => huggingface::fetch_papers(http, huggingface::BASE, HF_LIMIT).await,
    }
}

pub struct FetchOutcome {
    pub source: Source,
    pub result: Result<Vec<NewItem>>,
}

/// 有効なソースを順に取得する。1件の失敗で他を止めない。DBには触れない。
pub async fn fetch_all(http: &reqwest::Client, sources: &[Source]) -> Vec<FetchOutcome> {
    let mut out = Vec::new();
    for source in sources.iter().filter(|s| s.enabled) {
        let result = fetch_source(http, source).await;
        out.push(FetchOutcome {
            source: source.clone(),
            result,
        });
    }
    out
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct CollectReport {
    pub source_id: i64,
    pub name: String,
    pub new_items: usize,
    pub error: Option<String>,
}

/// 取得結果をDBに保存し、ソースごとの成否を記録する。
pub fn apply_outcomes(
    store: &Store,
    outcomes: &[FetchOutcome],
    now: &DateTime<Utc>,
) -> Result<Vec<CollectReport>> {
    let mut reports = Vec::new();
    for outcome in outcomes {
        let (new_items, error) = match &outcome.result {
            Ok(items) => (store.upsert_items(&outcome.source, items, now)?, None),
            Err(e) => (0, Some(e.to_string())),
        };
        store.record_fetch(outcome.source.id, now, error.as_deref())?;
        reports.push(CollectReport {
            source_id: outcome.source.id,
            name: outcome.source.name.clone(),
            new_items,
            error,
        });
    }
    Ok(reports)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Category, ItemFilter, SourceKind};
    use crate::store::Store;
    use crate::testutil::at;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const RSS: &str = r#"<rss version="2.0"><channel><title>T</title>
<item><title>記事1</title><link>https://e.com/1</link></item>
<item><title>記事2</title><link>https://e.com/2</link></item></channel></rss>"#;

    #[test]
    fn arxiv_url_and_saved_response_parse() {
        assert_eq!(
            arxiv_url("cs.AI"),
            "https://export.arxiv.org/api/query?search_query=cat:cs.AI&sortBy=submittedDate&sortOrder=descending&max_results=50"
        );
        let (_, items) =
            feed::parse_feed(include_bytes!("../../tests/fixtures/arxiv_cs_ai.xml")).unwrap();
        assert!(!items.is_empty());
        assert!(
            items.iter().all(|i| i.url.contains("arxiv.org/abs/")),
            "{:?}",
            items[0].url
        );
        assert!(items.iter().all(|i| !i.title.contains('\n')));
        assert!(items.iter().all(|i| i.published_at.is_some()));
    }

    #[tokio::test]
    async fn one_failing_source_does_not_stop_the_others() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/ok"))
            .respond_with(ResponseTemplate::new(200).set_body_string(RSS))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/broken"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;

        let store = Store::open_in_memory().unwrap();
        let broken = store
            .add_source(
                SourceKind::Rss,
                "Broken",
                Category::TechJp,
                &format!("{}/broken", server.uri()),
            )
            .unwrap();
        let ok = store
            .add_source(
                SourceKind::Rss,
                "Ok",
                Category::TechJp,
                &format!("{}/ok", server.uri()),
            )
            .unwrap();
        let disabled = store
            .add_source(
                SourceKind::Rss,
                "Off",
                Category::TechJp,
                &format!("{}/off", server.uri()),
            )
            .unwrap();
        store.set_source_enabled(disabled, false).unwrap();

        let http = http_client().unwrap();
        let outcomes = fetch_all(&http, &store.list_sources().unwrap()).await;
        assert_eq!(outcomes.len(), 2);

        let now = at("2026-10-05T01:00:00Z");
        let reports = apply_outcomes(&store, &outcomes, &now).unwrap();
        assert_eq!(reports.len(), 2);
        assert_eq!(reports[0].source_id, broken);
        assert!(reports[0].error.is_some());
        assert_eq!(reports[0].new_items, 0);
        assert_eq!(reports[1].source_id, ok);
        assert_eq!(reports[1].error, None);
        assert_eq!(reports[1].new_items, 2);

        let s = store.get_source(broken).unwrap().unwrap();
        assert!(s.last_error.is_some());
        assert_eq!(s.last_fetched_at, Some(now));
        let s = store.get_source(ok).unwrap().unwrap();
        assert_eq!(s.last_error, None);
        assert_eq!(s.last_fetched_at, Some(now));
        assert!(
            store
                .get_source(disabled)
                .unwrap()
                .unwrap()
                .last_fetched_at
                .is_none()
        );
        assert_eq!(store.query_items(&ItemFilter::default()).unwrap().len(), 2);

        // 2回目は新規0件。
        let outcomes = fetch_all(&http, &store.list_sources().unwrap()).await;
        let reports = apply_outcomes(&store, &outcomes, &now).unwrap();
        assert_eq!(reports[1].new_items, 0);
    }
}
