//! チャットのエージェントが求めた URL の本文をテキストにする。DB には保存しない (spec 12節)。

use scraper::{Html, Node, Selector};
use serde::Serialize;

use crate::collector::text::{collapse_ws, truncate_chars};
use crate::error::{Error, Result};

pub const MAX_BYTES: usize = 2_000_000;
pub const DEFAULT_MAX_CHARS: usize = 20_000;

/// 本文から除く要素。
const SKIP: &[&str] = &[
    "script", "style", "noscript", "nav", "header", "footer", "aside", "form", "svg", "template",
];

#[derive(Debug, Clone, Serialize)]
pub struct PageText {
    pub url: String,
    pub final_url: String,
    pub title: Option<String>,
    pub text: String,
    pub truncated: bool,
}

pub async fn fetch_page(http: &reqwest::Client, url: &str, max_chars: usize) -> Result<PageText> {
    let parsed = reqwest::Url::parse(url)
        .ok()
        .filter(|u| matches!(u.scheme(), "http" | "https"))
        .ok_or_else(|| Error::Invalid(format!("only http and https URLs can be fetched: {url}")))?;
    let mut resp = http.get(parsed).send().await?;
    let status = resp.status();
    if !status.is_success() {
        return Err(Error::Invalid(format!(
            "fetch failed: HTTP {}",
            status.as_u16()
        )));
    }
    let content_type = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("text/html")
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    let is_html = content_type == "text/html" || content_type == "application/xhtml+xml";
    if !is_html && content_type != "text/plain" {
        return Err(Error::Invalid(format!(
            "unsupported content type: {content_type}"
        )));
    }
    let final_url = resp.url().to_string();
    let mut bytes: Vec<u8> = Vec::new();
    let mut cut = false;
    while let Some(chunk) = resp.chunk().await? {
        let room = MAX_BYTES - bytes.len();
        if chunk.len() >= room {
            bytes.extend_from_slice(&chunk[..room]);
            cut = true;
            break;
        }
        bytes.extend_from_slice(&chunk);
    }
    let raw = String::from_utf8_lossy(&bytes);
    let (title, text) = if is_html {
        html_to_text(&raw)
    } else {
        (None, collapse_ws(&raw))
    };
    let truncated_by_chars = text.chars().count() > max_chars;
    let text = if truncated_by_chars {
        truncate_chars(&text, max_chars)
    } else {
        text
    };
    Ok(PageText {
        url: url.to_string(),
        final_url,
        title,
        text,
        truncated: cut || truncated_by_chars,
    })
}

fn html_to_text(raw: &str) -> (Option<String>, String) {
    let doc = Html::parse_document(raw);
    let title = Selector::parse("title")
        .ok()
        .and_then(|s| doc.select(&s).next())
        .map(|t| collapse_ws(&t.text().collect::<String>()))
        .filter(|t| !t.is_empty());
    let root = ["article", "main", "body"]
        .iter()
        .filter_map(|tag| Selector::parse(tag).ok())
        .find_map(|s| doc.select(&s).next());
    let Some(root) = root else {
        return (title, String::new());
    };
    let mut parts: Vec<&str> = Vec::new();
    for node in root.descendants() {
        let Node::Text(text) = node.value() else {
            continue;
        };
        let skipped = node.ancestors().any(|a| {
            a.value()
                .as_element()
                .is_some_and(|e| SKIP.contains(&e.name()))
        });
        if !skipped {
            parts.push(text);
        }
    }
    (title, collapse_ws(&parts.join(" ")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    async fn serve(body: &str, content_type: &str) -> MockServer {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/a"))
            .respond_with(
                ResponseTemplate::new(200).set_body_raw(body.as_bytes().to_vec(), content_type),
            )
            .mount(&server)
            .await;
        server
    }

    fn client() -> reqwest::Client {
        crate::collector::http_client().unwrap()
    }

    #[tokio::test]
    async fn extracts_title_and_article_text_without_scripts_or_navigation() {
        let html = r#"<html><head><title> 新薬の記事 </title><style>p{}</style></head>
            <body><nav>メニュー</nav><header>ヘッダー</header>
            <article><h1>見出し</h1><p>本文の<b>一段落</b>目。</p><script>alert(1)</script><p>二段落目。</p></article>
            <footer>フッター</footer></body></html>"#;
        let server = serve(html, "text/html; charset=utf-8").await;
        let page = fetch_page(&client(), &format!("{}/a", server.uri()), 20_000)
            .await
            .unwrap();
        assert_eq!(page.title.as_deref(), Some("新薬の記事"));
        assert!(page.text.contains("見出し"));
        assert!(
            page.text.contains("本文の 一段落 目。") || page.text.contains("本文の一段落目。"),
            "{}",
            page.text
        );
        assert!(page.text.contains("二段落目。"));
        for noise in ["メニュー", "ヘッダー", "フッター", "alert", "p{}"] {
            assert!(
                !page.text.contains(noise),
                "{noise} が残っている: {}",
                page.text
            );
        }
        assert!(!page.truncated);
    }

    #[tokio::test]
    async fn falls_back_to_body_without_article_and_accepts_plain_text() {
        let server = serve("<html><body><div>本文だけ</div></body></html>", "text/html").await;
        let page = fetch_page(&client(), &format!("{}/a", server.uri()), 100)
            .await
            .unwrap();
        assert_eq!(page.text, "本文だけ");
        let server = serve("plain  text\nbody", "text/plain").await;
        let page = fetch_page(&client(), &format!("{}/a", server.uri()), 100)
            .await
            .unwrap();
        assert_eq!(page.text, "plain text body");
        assert_eq!(page.title, None);
    }

    #[tokio::test]
    async fn truncates_long_text_and_marks_it() {
        let long = format!("<article><p>{}</p></article>", "あ".repeat(50));
        let server = serve(&long, "text/html").await;
        let page = fetch_page(&client(), &format!("{}/a", server.uri()), 10)
            .await
            .unwrap();
        assert!(page.truncated);
        assert_eq!(page.text.chars().count(), 11, "10文字 + …");
    }

    #[tokio::test]
    async fn stops_reading_after_max_bytes() {
        let huge = format!(
            "<article><p>{}</p></article>",
            "a".repeat(MAX_BYTES + 500_000)
        );
        let server = serve(&huge, "text/html").await;
        let page = fetch_page(&client(), &format!("{}/a", server.uri()), usize::MAX)
            .await
            .unwrap();
        assert!(page.truncated, "上限で読むのをやめたら truncated");
        assert!(page.text.len() <= MAX_BYTES);
    }

    #[tokio::test]
    async fn rejects_unsupported_types_errors_and_schemes() {
        let server = serve("%PDF-1.7", "application/pdf").await;
        let err = fetch_page(&client(), &format!("{}/a", server.uri()), 100)
            .await
            .unwrap_err();
        assert!(
            err.to_string()
                .contains("unsupported content type: application/pdf"),
            "{err}"
        );

        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&server)
            .await;
        let err = fetch_page(&client(), &format!("{}/a", server.uri()), 100)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("404"), "{err}");

        for bad in ["file:///etc/passwd", "ftp://example.com/x", "not a url"] {
            let err = fetch_page(&client(), bad, 100).await.unwrap_err();
            assert!(
                err.to_string().contains("only http and https"),
                "{bad}: {err}"
            );
        }
    }
}
