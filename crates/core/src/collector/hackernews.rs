use chrono::DateTime;
use serde::Deserialize;

use crate::error::Result;
use crate::model::NewItem;

pub const BASE: &str = "https://hacker-news.firebaseio.com/v0";

#[derive(Deserialize)]
struct HnItem {
    id: u64,
    title: Option<String>,
    url: Option<String>,
    score: Option<i64>,
    by: Option<String>,
    time: Option<i64>,
}

/// 記事1件のJSONを変換する。null、削除済み、タイトルなしは None。
pub fn parse_item(bytes: &[u8]) -> Option<NewItem> {
    let item: HnItem = serde_json::from_slice::<Option<HnItem>>(bytes).ok()??;
    let title = item.title.filter(|t| !t.trim().is_empty())?;
    Some(NewItem {
        url: item
            .url
            .filter(|u| !u.is_empty())
            .unwrap_or_else(|| format!("https://news.ycombinator.com/item?id={}", item.id)),
        title,
        excerpt: String::new(),
        author: item.by,
        published_at: item.time.and_then(|t| DateTime::from_timestamp(t, 0)),
        score: item.score,
    })
}

/// 上位 `limit` 件を取得する。個別の記事の取得失敗は飛ばす。
pub async fn fetch(http: &reqwest::Client, base: &str, limit: usize) -> Result<Vec<NewItem>> {
    let ids: Vec<u64> = http
        .get(format!("{base}/topstories.json"))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let mut out = Vec::new();
    for id in ids.into_iter().take(limit) {
        let resp = match http.get(format!("{base}/item/{id}.json")).send().await {
            Ok(r) if r.status().is_success() => r,
            _ => continue,
        };
        if let Ok(bytes) = resp.bytes().await {
            if let Some(item) = parse_item(&bytes) {
                out.push(item);
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collector::http_client;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[test]
    fn parses_story_with_url() {
        let json = br#"{"by":"pg","descendants":3,"id":1,"kids":[15],"score":57,"time":1160418111,"title":"Y Combinator","type":"story","url":"http://ycombinator.com"}"#;
        let it = parse_item(json).unwrap();
        assert_eq!(it.url, "http://ycombinator.com");
        assert_eq!(it.title, "Y Combinator");
        assert_eq!(it.score, Some(57));
        assert_eq!(it.author.as_deref(), Some("pg"));
        assert_eq!(it.published_at.unwrap().timestamp(), 1160418111);
    }

    #[test]
    fn story_without_url_links_to_discussion() {
        let json = br#"{"by":"a","id":42,"score":5,"time":1,"title":"Ask HN: x","type":"story"}"#;
        assert_eq!(parse_item(json).unwrap().url, "https://news.ycombinator.com/item?id=42");
    }

    #[test]
    fn null_deleted_and_untitled_are_skipped() {
        assert!(parse_item(b"null").is_none());
        assert!(parse_item(br#"{"id":1,"deleted":true,"type":"story"}"#).is_none());
        assert!(parse_item(b"not json").is_none());
    }

    #[tokio::test]
    async fn fetch_takes_first_n_and_skips_broken_items() {
        let server = MockServer::start().await;
        Mock::given(method("GET")).and(path("/topstories.json"))
            .respond_with(ResponseTemplate::new(200).set_body_string("[1,2,3,4]"))
            .mount(&server).await;
        Mock::given(method("GET")).and(path("/item/1.json"))
            .respond_with(ResponseTemplate::new(200)
                .set_body_string(r#"{"id":1,"title":"one","score":10,"time":1,"type":"story"}"#))
            .mount(&server).await;
        Mock::given(method("GET")).and(path("/item/2.json"))
            .respond_with(ResponseTemplate::new(200).set_body_string("null"))
            .mount(&server).await;
        Mock::given(method("GET")).and(path("/item/3.json"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server).await;
        let items = fetch(&http_client().unwrap(), &server.uri(), 3).await.unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].title, "one");
    }

    #[tokio::test]
    async fn fetch_fails_when_list_fails() {
        let server = MockServer::start().await;
        Mock::given(method("GET")).and(path("/topstories.json"))
            .respond_with(ResponseTemplate::new(503))
            .mount(&server).await;
        assert!(fetch(&http_client().unwrap(), &server.uri(), 3).await.is_err());
    }
}
