use chrono::{DateTime, Utc};
use serde::Deserialize;

use super::text::{collapse_ws, truncate_chars};
use crate::error::Result;
use crate::model::NewItem;

pub const BASE: &str = "https://huggingface.co";

const EXCERPT_MAX_CHARS: usize = 500;

#[derive(Deserialize)]
struct Model {
    id: String,
    likes: Option<i64>,
    #[serde(rename = "trendingScore")]
    trending_score: Option<f64>,
    downloads: Option<i64>,
    pipeline_tag: Option<String>,
    #[serde(rename = "createdAt")]
    created_at: Option<DateTime<Utc>>,
}

#[derive(Deserialize)]
struct PaperEntry {
    paper: PaperRef,
    title: String,
    summary: Option<String>,
    #[serde(rename = "publishedAt")]
    published_at: Option<DateTime<Utc>>,
}

#[derive(Deserialize)]
struct PaperRef {
    id: String,
    upvotes: Option<i64>,
}

pub fn parse_models(bytes: &[u8]) -> Result<Vec<NewItem>> {
    let models: Vec<Model> = serde_json::from_slice(bytes)?;
    Ok(models
        .into_iter()
        .map(|m| {
            let mut parts = Vec::new();
            if let Some(tag) = &m.pipeline_tag {
                parts.push(tag.clone());
            }
            if let Some(likes) = m.likes {
                parts.push(format!("likes {likes}"));
            }
            if let Some(downloads) = m.downloads {
                parts.push(format!("downloads {downloads}"));
            }
            NewItem {
                url: format!("{BASE}/{}", m.id),
                author: m.id.split_once('/').map(|(owner, _)| owner.to_string()),
                title: m.id,
                excerpt: parts.join(" · "),
                published_at: m.created_at,
                score: m.trending_score.map(|s| s.round() as i64),
            }
        })
        .collect())
}

pub fn parse_papers(bytes: &[u8]) -> Result<Vec<NewItem>> {
    let entries: Vec<PaperEntry> = serde_json::from_slice(bytes)?;
    Ok(entries
        .into_iter()
        .map(|e| NewItem {
            url: format!("{BASE}/papers/{}", e.paper.id),
            title: collapse_ws(&e.title),
            excerpt: truncate_chars(
                &collapse_ws(e.summary.as_deref().unwrap_or_default()),
                EXCERPT_MAX_CHARS,
            ),
            author: None,
            published_at: e.published_at,
            score: e.paper.upvotes,
        })
        .collect())
}

async fn get_bytes(http: &reqwest::Client, url: &str) -> Result<Vec<u8>> {
    Ok(http
        .get(url)
        .send()
        .await?
        .error_for_status()?
        .bytes()
        .await?
        .to_vec())
}

pub async fn fetch_models(
    http: &reqwest::Client,
    base: &str,
    limit: usize,
) -> Result<Vec<NewItem>> {
    let url = format!("{base}/api/models?sort=trendingScore&limit={limit}");
    parse_models(&get_bytes(http, &url).await?)
}

pub async fn fetch_papers(
    http: &reqwest::Client,
    base: &str,
    limit: usize,
) -> Result<Vec<NewItem>> {
    let url = format!("{base}/api/daily_papers?limit={limit}");
    parse_papers(&get_bytes(http, &url).await?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_models() {
        let json = br#"[{"_id":"x","id":"Cloudflare/clef","likes":1452,"trendingScore":1353,
            "private":false,"downloads":5416,"tags":["transformers"],"pipeline_tag":"text-generation",
            "createdAt":"2026-09-30T21:15:35.000Z","modelId":"Cloudflare/clef"},
            {"id":"bare/model"}]"#;
        let items = parse_models(json).unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].url, "https://huggingface.co/Cloudflare/clef");
        assert_eq!(items[0].title, "Cloudflare/clef");
        assert_eq!(items[0].score, Some(1353));
        assert_eq!(
            items[0].excerpt,
            "text-generation · likes 1452 · downloads 5416"
        );
        assert_eq!(items[0].author.as_deref(), Some("Cloudflare"));
        assert_eq!(
            items[0].published_at.unwrap().to_rfc3339(),
            "2026-09-30T21:15:35+00:00"
        );
        assert_eq!(items[1].score, None);
        assert_eq!(items[1].excerpt, "");
    }

    #[test]
    fn parses_papers() {
        let json = br#"[{"paper":{"id":"2609.34826","upvotes":3,"title":"ignored"},
            "publishedAt":"2026-09-27T20:00:00.000Z","title":"A  Paper\n Title",
            "summary":"Line one.\nLine two.","numComments":0}]"#;
        let items = parse_papers(json).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].url, "https://huggingface.co/papers/2609.34826");
        assert_eq!(items[0].title, "A Paper Title");
        assert_eq!(items[0].excerpt, "Line one. Line two.");
        assert_eq!(items[0].score, Some(3));
    }

    #[test]
    fn invalid_json_is_an_error() {
        assert!(parse_models(b"<html>").is_err());
        assert!(parse_papers(b"{}").is_err());
    }

    #[test]
    fn parses_saved_real_responses() {
        let models = parse_models(include_bytes!("../../tests/fixtures/hf_models.json")).unwrap();
        assert!(!models.is_empty());
        assert!(
            models
                .iter()
                .all(|m| m.url.starts_with("https://huggingface.co/"))
        );
        let papers = parse_papers(include_bytes!("../../tests/fixtures/hf_papers.json")).unwrap();
        assert!(!papers.is_empty());
        assert!(
            papers
                .iter()
                .all(|p| p.url.starts_with("https://huggingface.co/papers/"))
        );
        assert!(papers.iter().all(|p| !p.title.is_empty()));
    }
}
