use super::text::{strip_html, truncate_chars};
use crate::error::{Error, Result};
use crate::model::NewItem;

const EXCERPT_MAX_CHARS: usize = 500;

#[derive(Debug, Clone, PartialEq)]
pub struct FeedInfo {
    pub title: String,
    pub item_count: usize,
}

/// RSS / Atom を解析する。リンクまたはタイトルのないエントリーは捨てる。
pub fn parse_feed(bytes: &[u8]) -> Result<(Option<String>, Vec<NewItem>)> {
    let feed = feed_rs::parser::parse(bytes).map_err(|e| Error::InvalidFeed(e.to_string()))?;
    let feed_title = feed
        .title
        .as_ref()
        .map(|t| strip_html(&t.content))
        .filter(|t| !t.is_empty());
    let mut items = Vec::new();
    for e in &feed.entries {
        let url = match e.links.first() {
            Some(link) => link.href.trim().to_string(),
            None if e.id.starts_with("http") => e.id.clone(),
            None => continue,
        };
        let title = e
            .title
            .as_ref()
            .map(|t| strip_html(&t.content))
            .unwrap_or_default();
        if url.is_empty() || title.is_empty() {
            continue;
        }
        let raw_excerpt = e
            .summary
            .as_ref()
            .map(|t| t.content.clone())
            .or_else(|| e.content.as_ref().and_then(|c| c.body.clone()))
            .unwrap_or_default();
        items.push(NewItem {
            url,
            title,
            excerpt: truncate_chars(&strip_html(&raw_excerpt), EXCERPT_MAX_CHARS),
            author: e.authors.first().and_then(|p| p.name.clone()),
            published_at: e.published.or(e.updated),
            score: None,
        });
    }
    Ok((feed_title, items))
}

async fn get_bytes(http: &reqwest::Client, url: &str) -> Result<Vec<u8>> {
    let resp = http.get(url).send().await?.error_for_status()?;
    Ok(resp.bytes().await?.to_vec())
}

pub async fn fetch(http: &reqwest::Client, url: &str) -> Result<Vec<NewItem>> {
    let bytes = get_bytes(http, url).await?;
    Ok(parse_feed(&bytes)?.1)
}

/// フィードを1回取得して検証する。タイトルがなければURLを表示名に使う。
pub async fn validate(http: &reqwest::Client, url: &str) -> Result<FeedInfo> {
    let bytes = get_bytes(http, url).await?;
    let (title, items) = parse_feed(&bytes)?;
    Ok(FeedInfo {
        title: title.unwrap_or_else(|| url.to_string()),
        item_count: items.len(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collector::http_client;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const RSS: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<rss version="2.0"><channel><title>テストフィード</title>
<item><title>認知症の新薬 &amp; 治療</title><link>https://e.com/a</link>
  <description>&lt;p&gt;本文 &lt;b&gt;太字&lt;/b&gt;&lt;/p&gt;</description>
  <pubDate>Mon, 05 Oct 2026 10:00:00 +0900</pubDate><author>x@y.z (山田)</author></item>
<item><title>日付なし</title><link>https://e.com/b</link></item>
<item><title>リンクなし</title><description>捨てる</description></item>
<item><link>https://e.com/notitle</link></item>
</channel></rss>"#;

    const ATOM: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<feed xmlns="http://www.w3.org/2005/Atom"><title>Atom Feed</title>
<entry><id>http://arxiv.org/abs/2610.00001v1</id>
  <title>A Paper
   With Newline</title>
  <link href="http://arxiv.org/abs/2610.00001v1" rel="alternate" type="text/html"/>
  <link href="http://arxiv.org/pdf/2610.00001v1" rel="related" type="application/pdf"/>
  <summary>Abstract text.</summary><published>2026-10-05T01:00:00Z</published>
  <updated>2026-10-05T02:00:00Z</updated><author><name>Alice</name></author></entry>
</feed>"#;

    #[test]
    fn parses_rss_and_cleans_entries() {
        // Review Focus 3
        let (title, items) = parse_feed(RSS.as_bytes()).unwrap();
        assert_eq!(title.as_deref(), Some("テストフィード"));
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].url, "https://e.com/a");
        assert_eq!(items[0].title, "認知症の新薬 & 治療");
        assert_eq!(items[0].excerpt, "本文 太字");
        assert_eq!(
            items[0].published_at.unwrap().to_rfc3339(),
            "2026-10-05T01:00:00+00:00"
        );
        assert_eq!(items[0].score, None);
        assert_eq!(items[1].title, "日付なし");
        assert_eq!(items[1].published_at, None);
        assert_eq!(items[1].excerpt, "");
    }

    #[test]
    fn parses_atom_first_link_and_author() {
        let (title, items) = parse_feed(ATOM.as_bytes()).unwrap();
        assert_eq!(title.as_deref(), Some("Atom Feed"));
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].url, "http://arxiv.org/abs/2610.00001v1");
        assert_eq!(items[0].title, "A Paper With Newline");
        assert_eq!(items[0].excerpt, "Abstract text.");
        assert_eq!(items[0].author.as_deref(), Some("Alice"));
        assert_eq!(
            items[0].published_at.unwrap().to_rfc3339(),
            "2026-10-05T01:00:00+00:00"
        );
    }

    #[test]
    fn long_excerpt_is_truncated() {
        let body = "あ".repeat(600);
        let xml = format!(
            r#"<rss version="2.0"><channel><title>T</title><item><title>t</title><link>https://e.com/x</link><description>{body}</description></item></channel></rss>"#
        );
        let (_, items) = parse_feed(xml.as_bytes()).unwrap();
        assert_eq!(items[0].excerpt.chars().count(), 501);
    }

    #[test]
    fn non_feed_content_is_invalid_feed() {
        let err = parse_feed(b"<html><body>not a feed</body></html>").unwrap_err();
        assert!(matches!(err, Error::InvalidFeed(_)));
        assert!(matches!(
            parse_feed(b"").unwrap_err(),
            Error::InvalidFeed(_)
        ));
    }

    #[tokio::test]
    async fn validate_accepts_rss_and_atom() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/rss"))
            .respond_with(ResponseTemplate::new(200).set_body_string(RSS))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/atom"))
            .respond_with(ResponseTemplate::new(200).set_body_string(ATOM))
            .mount(&server)
            .await;
        let http = http_client().unwrap();
        let info = validate(&http, &format!("{}/rss", server.uri()))
            .await
            .unwrap();
        assert_eq!(
            info,
            FeedInfo {
                title: "テストフィード".into(),
                item_count: 2
            }
        );
        let info = validate(&http, &format!("{}/atom", server.uri()))
            .await
            .unwrap();
        assert_eq!(info.title, "Atom Feed");
        assert_eq!(
            fetch(&http, &format!("{}/rss", server.uri()))
                .await
                .unwrap()
                .len(),
            2
        );
    }

    #[tokio::test]
    async fn validate_rejects_html_404_and_unreachable() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/html"))
            .respond_with(ResponseTemplate::new(200).set_body_string("<html></html>"))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/missing"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&server)
            .await;
        let http = http_client().unwrap();
        let err = validate(&http, &format!("{}/html", server.uri()))
            .await
            .unwrap_err();
        assert!(matches!(err, Error::InvalidFeed(_)));
        let err = validate(&http, &format!("{}/missing", server.uri()))
            .await
            .unwrap_err();
        assert!(matches!(err, Error::Http(_)));
        let err = validate(&http, "http://127.0.0.1:1/feed")
            .await
            .unwrap_err();
        assert!(matches!(err, Error::Http(_)));
        let err = validate(&http, "not a url").await.unwrap_err();
        assert!(matches!(err, Error::Http(_)));
    }

    #[tokio::test]
    async fn validate_uses_url_when_feed_has_no_title() {
        let server = MockServer::start().await;
        let xml = r#"<rss version="2.0"><channel><item><title>t</title><link>https://e.com/x</link></item></channel></rss>"#;
        Mock::given(method("GET"))
            .and(path("/untitled"))
            .respond_with(ResponseTemplate::new(200).set_body_string(xml))
            .mount(&server)
            .await;
        let url = format!("{}/untitled", server.uri());
        let info = validate(&http_client().unwrap(), &url).await.unwrap();
        assert_eq!(info.title, url);
    }
}
