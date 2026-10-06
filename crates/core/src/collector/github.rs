use scraper::{Html, Selector};

use super::text::collapse_ws;
use crate::error::{Error, Result};
use crate::model::NewItem;

pub const BASE: &str = "https://github.com";

const STAR_MARKERS: [&str; 6] = [
    " stars today",
    " stars this week",
    " stars this month",
    " star today",
    " star this week",
    " star this month",
];

/// 本文中の `1,430 stars today` から数値を取り出す。
fn stars_in_period(text: &str) -> Option<i64> {
    for marker in STAR_MARKERS {
        if let Some(idx) = text.find(marker) {
            let number = text[..idx].rsplit(' ').next()?;
            return number.replace(',', "").parse().ok();
        }
    }
    None
}

pub fn parse(html: &str) -> Result<Vec<NewItem>> {
    let doc = Html::parse_document(html);
    let article = Selector::parse("article.Box-row").expect("valid selector");
    let link = Selector::parse("h2 a").expect("valid selector");
    let desc = Selector::parse("p").expect("valid selector");
    let mut out = Vec::new();
    for el in doc.select(&article) {
        let Some(href) = el.select(&link).next().and_then(|a| a.value().attr("href")) else {
            continue;
        };
        let repo = href.trim_matches('/');
        if repo.split('/').count() != 2 {
            continue;
        }
        let excerpt = el
            .select(&desc)
            .next()
            .map(|p| collapse_ws(&p.text().collect::<String>()))
            .unwrap_or_default();
        let text = collapse_ws(&el.text().collect::<Vec<_>>().join(" "));
        out.push(NewItem {
            url: format!("{BASE}/{repo}"),
            title: repo.to_string(),
            excerpt,
            author: repo.split('/').next().map(String::from),
            published_at: None,
            score: stars_in_period(&text),
        });
    }
    if out.is_empty() {
        return Err(Error::Parse(
            "github trending: no repositories found (page structure may have changed)".into(),
        ));
    }
    Ok(out)
}

pub async fn fetch(http: &reqwest::Client, base: &str, since: &str) -> Result<Vec<NewItem>> {
    let url = format!("{base}/trending?since={since}");
    let html = http.get(&url).send().await?.error_for_status()?.text().await?;
    parse(&html)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"<html><body>
<article class="Box-row">
  <h2 class="h3 lh-condensed"><a href="/tester-army/e2e"> tester-army / e2e </a></h2>
  <p class="col-9 color-fg-muted my-1">End to end
     testing agent</p>
  <a href="/tester-army/e2e/stargazers">12,345</a>
  <span class="d-inline-block float-sm-right"> 1,430 stars today </span>
</article>
<article class="Box-row">
  <h2 class="h3 lh-condensed"><a href="/solo/repo">solo / repo</a></h2>
  <span class="d-inline-block float-sm-right"> 87 stars this week </span>
</article>
<article class="Box-row"><h2><a href="/not-a-repo">x</a></h2></article>
</body></html>"#;

    #[test]
    fn parses_repositories() {
        let items = parse(SAMPLE).unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].url, "https://github.com/tester-army/e2e");
        assert_eq!(items[0].title, "tester-army/e2e");
        assert_eq!(items[0].excerpt, "End to end testing agent");
        assert_eq!(items[0].score, Some(1430));
        assert_eq!(items[0].author.as_deref(), Some("tester-army"));
        assert_eq!(items[1].title, "solo/repo");
        assert_eq!(items[1].excerpt, "");
        assert_eq!(items[1].score, Some(87));
    }

    #[test]
    fn zero_repositories_is_an_error() {
        // Review Focus 4
        let err = parse("<html><body><div>redesigned page</div></body></html>").unwrap_err();
        assert!(matches!(err, Error::Parse(_)));
        assert!(matches!(parse("").unwrap_err(), Error::Parse(_)));
    }

    #[test]
    fn parses_saved_real_page() {
        let html = include_str!("../../tests/fixtures/github_trending_daily.html");
        let items = parse(html).unwrap();
        assert!(items.len() >= 5, "got {}", items.len());
        for it in &items {
            assert!(it.url.starts_with("https://github.com/"), "{}", it.url);
            assert_eq!(it.title.split('/').count(), 2, "{}", it.title);
        }
        assert!(items.iter().any(|i| i.score.is_some()));
    }
}
