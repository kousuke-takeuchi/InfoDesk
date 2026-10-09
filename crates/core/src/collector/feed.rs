use std::collections::HashMap;

use feed_rs::model::{Link, Text};
use quick_xml::NsReader;
use quick_xml::XmlVersion;
use quick_xml::encoding::DecodingReader;
use quick_xml::escape::resolve_predefined_entity;
use quick_xml::events::{BytesStart, Event};
use quick_xml::name::ResolveResult;

use super::text::{collapse_ws, strip_html, truncate_chars};
use crate::error::{Error, Result};
use crate::model::NewItem;

const EXCERPT_MAX_CHARS: usize = 500;

#[derive(Debug, Clone, PartialEq)]
pub struct FeedInfo {
    pub title: String,
    pub item_count: usize,
}

/// HTMLとして配信された文字列 (text/html, XHTML) だけタグを除く。
/// プレーンテキストはパーサーが実体参照を戻した後の文字列なので、`Box<dyn Trait>` の
/// ような山括弧を残すため空白の整理だけにする。
fn clean_text(t: &Text) -> String {
    let ct = &t.content_type;
    let is_html = (ct.ty() == "text" && ct.subty() == "html")
        || (ct.ty() == "application" && ct.subty().as_str().starts_with("xhtml"));
    if is_html {
        strip_html(&t.content)
    } else {
        collapse_ws(&t.content)
    }
}

/// 記事のURL。rel が alternate または省略されたリンクを優先し、なければ最初のリンク。
fn entry_link(links: &[Link]) -> Option<&Link> {
    links
        .iter()
        .find(|l| l.rel.as_deref().is_none_or(|r| r == "alternate"))
        .or_else(|| links.first())
}

/// RSS / Atom を解析する。リンクまたはタイトルのないエントリーは捨てる。
pub fn parse_feed(bytes: &[u8]) -> Result<(Option<String>, Vec<NewItem>)> {
    let feed = feed_rs::parser::parse(bytes).map_err(|e| Error::InvalidFeed(e.to_string()))?;
    let feed_title = feed
        .title
        .as_ref()
        .map(clean_text)
        .filter(|t| !t.is_empty());
    let thumbs = media_thumbnails(bytes);
    let mut items = Vec::new();
    for e in &feed.entries {
        let url = match entry_link(&e.links) {
            Some(link) => link.href.trim().to_string(),
            None if e.id.starts_with("http") => e.id.clone(),
            None => continue,
        };
        let title = e.title.as_ref().map(clean_text).unwrap_or_default();
        if url.is_empty() || title.is_empty() {
            continue;
        }
        let excerpt = e
            .summary
            .as_ref()
            .map(clean_text)
            .or_else(|| {
                e.content
                    .as_ref()
                    .and_then(|c| c.body.as_deref())
                    .map(strip_html)
            })
            .unwrap_or_default();
        let image_url = entry_image(e).or_else(|| {
            thumbs
                .get(&url)?
                .iter()
                .find_map(|u| super::images::https_image_url(u, None))
        });
        items.push(NewItem {
            url,
            title,
            excerpt: truncate_chars(&excerpt, EXCERPT_MAX_CHARS),
            author: e.authors.first().and_then(|p| p.name.clone()),
            published_at: e.published.or(e.updated),
            score: None,
            image_url,
        });
    }
    Ok((feed_title, items))
}

/// Media RSS のサムネイル、画像の Media RSS コンテンツ、画像の enclosure の順に探す。
fn entry_image(e: &feed_rs::model::Entry) -> Option<String> {
    let thumbs = e
        .media
        .iter()
        .flat_map(|m| m.thumbnails.iter())
        .map(|t| t.image.uri.as_str());
    let contents = e
        .media
        .iter()
        .flat_map(|m| m.content.iter())
        .filter(|c| c.content_type.as_ref().is_some_and(|t| t.ty() == "image"))
        .filter_map(|c| c.url.as_ref().map(|u| u.as_str()));
    thumbs
        .chain(contents)
        .find_map(|u| super::images::https_image_url(u, None))
}

const MRSS_NS: &str = "http://search.yahoo.com/mrss/";
const ATOM_NS: &str = "http://www.w3.org/2005/Atom";

/// 読み取り中の `<item>` / `<entry>`。
#[derive(Default)]
struct ScanItem {
    /// RSS の `<item>` なら true (Atom の `<entry>` は false)。
    rss: bool,
    /// `<item>` / `<entry>` からの深さ (直下の子が 1)。
    depth: usize,
    /// rel が alternate または省略されたリンク。
    alternate: Option<String>,
    /// rel を問わない最初のリンク。
    first: Option<String>,
    /// `<guid>` / `<id>`。
    id: Option<String>,
    thumbnails: Vec<String>,
    /// 中身の文字列を集めている直下の子 (`link` / `guid` / `id`)。
    field: Option<&'static str>,
    text: String,
}

impl ScanItem {
    fn add_link(&mut self, href: &str, rel: Option<&str>) {
        let href = href.trim();
        if href.is_empty() {
            return;
        }
        if self.first.is_none() {
            self.first = Some(href.to_string());
        }
        if self.alternate.is_none() && rel.is_none_or(|r| r == "alternate") {
            self.alternate = Some(href.to_string());
        }
    }

    /// `parse_feed` が記事の URL にするのと同じリンク。
    fn key(self) -> Option<(String, Vec<String>)> {
        let link = self
            .alternate
            .or(self.first)
            .or(self.id.filter(|i| i.starts_with("http")))?;
        Some((link, self.thumbnails))
    }
}

/// 属性の値を実体参照を戻して返す。名前空間の接頭辞つきの属性は見ない。
fn attr(e: &BytesStart, name: &str) -> Option<String> {
    e.attributes()
        .flatten()
        .find(|a| a.key.as_ref() == name)
        .and_then(|a| a.normalized_value(XmlVersion::Implicit1_0).ok())
        .map(|v| v.into_owned())
}

/// feed-rs が `link.href` に入れる形 (URL として解釈して直した文字列)。解釈できなければそのまま。
fn normalize_link(raw: &str) -> String {
    reqwest::Url::parse(raw).map_or_else(|_| raw.trim().to_string(), |u| u.to_string())
}

/// `<media:thumbnail url="...">` を、それを含む `<item>` / `<entry>` のリンクごとに集める。
/// feed-rs 3.0.0 は `<media:content>` の無いエントリーのサムネイルを捨てるので、ここで拾う。
/// サムネイルは名前空間 `http://search.yahoo.com/mrss/` に解決される `thumbnail` 要素だけ
/// (接頭辞の名前は問わない)。CDATA とコメントの中身は要素として読まない。
fn media_thumbnails(bytes: &[u8]) -> HashMap<String, Vec<String>> {
    let mut reader = NsReader::from_reader(DecodingReader::new(bytes));
    let mut buf = Vec::new();
    let mut out: HashMap<String, Vec<String>> = HashMap::new();
    let mut item: Option<ScanItem> = None;
    loop {
        buf.clear();
        let Ok((ns, event)) = reader.read_resolved_event_into(&mut buf) else {
            break;
        };
        let is_mrss = matches!(ns, ResolveResult::Bound(n) if n.as_ref() == MRSS_NS);
        let is_atom = matches!(ns, ResolveResult::Bound(n) if n.as_ref() == ATOM_NS);
        match event {
            Event::Eof => break,
            Event::Decl(d) => {
                if let Some(enc) = d.encoder() {
                    reader.get_mut().set_encoding(enc);
                }
            }
            Event::Start(ref e) | Event::Empty(ref e) => {
                let empty = matches!(event, Event::Empty(_));
                let local = e.local_name();
                let local = local.as_ref();
                let Some(it) = item.as_mut() else {
                    if !empty && (local == "item" || local == "entry") {
                        item = Some(ScanItem {
                            rss: local == "item",
                            ..ScanItem::default()
                        });
                    }
                    continue;
                };
                if !empty {
                    it.depth += 1;
                }
                if is_mrss && local == "thumbnail" {
                    if let Some(u) = attr(e, "url") {
                        it.thumbnails.push(u);
                    }
                    continue;
                }
                let direct = if empty { it.depth == 0 } else { it.depth == 1 };
                if !direct {
                    continue;
                }
                match local {
                    // href を読むのは Atom の `<entry>` の中の Atom 名前空間の link だけ。
                    // RSS の `<item>` では feed-rs が `<link>` の中身だけを読む
                    // (`<atom:link href>` は無視する)ので、こちらも合わせる。
                    "link" => match attr(e, "href").filter(|_| is_atom && !it.rss) {
                        Some(href) => it.add_link(&href, attr(e, "rel").as_deref()),
                        None if !empty => it.field = Some("link"),
                        None => {}
                    },
                    "guid" | "id" if !empty => it.field = Some("id"),
                    _ => {}
                }
                if it.field.is_some() {
                    it.text.clear();
                }
            }
            Event::Text(ref t) => {
                if let Some(it) = item.as_mut().filter(|i| i.field.is_some()) {
                    it.text.push_str(&t.xml10_content());
                }
            }
            Event::CData(ref t) => {
                if let Some(it) = item.as_mut().filter(|i| i.field.is_some()) {
                    it.text.push_str(&t.xml10_content());
                }
            }
            Event::GeneralRef(ref r) => {
                if let Some(it) = item.as_mut().filter(|i| i.field.is_some()) {
                    if let Ok(Some(c)) = r.resolve_char_ref() {
                        it.text.push(c);
                    } else if let Some(s) = resolve_predefined_entity(&r.xml10_content()) {
                        it.text.push_str(s);
                    }
                }
            }
            Event::End(_) => {
                let Some(it) = item.as_mut() else { continue };
                if it.depth == 0 {
                    if let Some((link, thumbs)) = item.take().and_then(ScanItem::key) {
                        out.entry(normalize_link(&link)).or_default().extend(thumbs);
                    }
                    continue;
                }
                if it.depth == 1 {
                    let text = std::mem::take(&mut it.text);
                    match it.field.take() {
                        Some("link") => it.add_link(&text, None),
                        Some(_) if it.id.is_none() => it.id = Some(text.trim().to_string()),
                        _ => {}
                    }
                }
                it.depth -= 1;
            }
            _ => {}
        }
    }
    out.retain(|_, v| !v.is_empty());
    out
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
    fn plain_text_title_keeps_angle_brackets() {
        let xml = r#"<rss version="2.0"><channel><title>T</title>
<item><title>Rust の Box&lt;dyn Trait&gt; と Vec&lt;u8&gt;</title><link>https://e.com/x</link>
  <description>&lt;p&gt;Vec&lt;/p&gt; の  &lt;b&gt;話&lt;/b&gt;</description></item>
</channel></rss>"#;
        let (_, items) = parse_feed(xml.as_bytes()).unwrap();
        assert_eq!(items[0].title, "Rust の Box<dyn Trait> と Vec<u8>");
        assert_eq!(items[0].excerpt, "Vec の 話");
    }

    #[test]
    fn atom_prefers_alternate_link() {
        let xml = r#"<?xml version="1.0" encoding="utf-8"?>
<feed xmlns="http://www.w3.org/2005/Atom"><title>A</title>
<entry><id>tag:e.com,2026:1</id><title type="html">Post &lt;b&gt;bold&lt;/b&gt;</title>
  <link href="https://e.com/1/comments" rel="replies"/>
  <link href="https://e.com/1" rel="alternate"/></entry>
<entry><id>tag:e.com,2026:2</id><title>No rel</title>
  <link href="https://e.com/2/edit" rel="edit"/>
  <link href="https://e.com/2"/></entry>
<entry><id>tag:e.com,2026:3</id><title>Only replies</title>
  <link href="https://e.com/3/comments" rel="replies"/></entry>
</feed>"#;
        let (_, items) = parse_feed(xml.as_bytes()).unwrap();
        let urls: Vec<&str> = items.iter().map(|i| i.url.as_str()).collect();
        assert_eq!(
            urls,
            vec![
                "https://e.com/1",
                "https://e.com/2",
                "https://e.com/3/comments"
            ]
        );
        assert_eq!(items[0].title, "Post bold");
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

    fn images(xml: &str) -> Vec<(String, Option<String>)> {
        let (_, items) = parse_feed(xml.as_bytes()).unwrap();
        items.into_iter().map(|i| (i.url, i.image_url)).collect()
    }

    #[test]
    fn atom_entry_takes_media_thumbnail() {
        let xml = r#"<?xml version="1.0" encoding="utf-8"?>
<feed xmlns="http://www.w3.org/2005/Atom" xmlns:m="http://search.yahoo.com/mrss/"><title>A</title>
<entry><id>tag:e.com,2026:1</id><title>One</title>
  <link href="https://e.com/1/comments" rel="replies"/>
  <link href="https://e.com/1" rel="alternate"/>
  <m:thumbnail url="https://img.e.com/1.jpg"/></entry>
<entry><id>https://e.com/2</id><title>Two</title></entry>
</feed>"#;
        assert_eq!(
            images(xml),
            vec![
                (
                    "https://e.com/1".into(),
                    Some("https://img.e.com/1.jpg".into())
                ),
                ("https://e.com/2".into(), None),
            ]
        );
    }

    #[test]
    fn thumbnail_goes_to_its_own_item_when_an_earlier_item_is_dropped() {
        let xml = r#"<rss version="2.0" xmlns:media="http://search.yahoo.com/mrss/"><channel><title>T</title>
<item><title>no link</title><description><![CDATA[<item><media:thumbnail url="https://img.e.com/fake.jpg"/></item>]]></description></item>
<!-- <item><link>https://e.com/1</link><media:thumbnail url="https://img.e.com/comment.jpg"/></item> -->
<item><title>plain</title><link>https://e.com/1</link></item>
<item><title>thumb</title><link>https://e.com/2</link><media:thumbnail url="https://img.e.com/2.jpg"/></item>
</channel></rss>"#;
        assert_eq!(
            images(xml),
            vec![
                ("https://e.com/1".into(), None),
                (
                    "https://e.com/2".into(),
                    Some("https://img.e.com/2.jpg".into())
                ),
            ]
        );
    }

    #[test]
    fn thumbnail_inside_media_group_without_content() {
        let xml = r#"<rss version="2.0" xmlns:media="http://search.yahoo.com/mrss/"><channel><title>T</title>
<item><title>g</title><link>https://e.com/1</link>
  <media:group><media:title>x</media:title><media:thumbnail url="https://img.e.com/g.jpg"/></media:group></item>
</channel></rss>"#;
        assert_eq!(
            images(xml),
            vec![(
                "https://e.com/1".into(),
                Some("https://img.e.com/g.jpg".into())
            )]
        );
    }

    #[test]
    fn skips_http_thumbnail_for_a_later_https_one() {
        let xml = r#"<rss version="2.0" xmlns:media="http://search.yahoo.com/mrss/"><channel><title>T</title>
<item><title>t</title><link>https://e.com/1</link>
  <media:thumbnail url="http://img.e.com/a.jpg"/><media:thumbnail url="https://img.e.com/b.jpg"/></item>
</channel></rss>"#;
        assert_eq!(
            images(xml),
            vec![(
                "https://e.com/1".into(),
                Some("https://img.e.com/b.jpg".into())
            )]
        );
    }

    #[test]
    fn thumbnail_url_entities_are_decoded() {
        let xml = r#"<rss version="2.0" xmlns:media="http://search.yahoo.com/mrss/"><channel><title>T</title>
<item><title>t</title><link>https://e.com/p?a=1&amp;b=2</link>
  <media:thumbnail url="https://img.e.com/x.jpg?w=1&amp;h=2&#38;q=3"/></item>
</channel></rss>"#;
        assert_eq!(
            images(xml),
            vec![(
                "https://e.com/p?a=1&b=2".into(),
                Some("https://img.e.com/x.jpg?w=1&h=2&q=3".into())
            )]
        );
    }

    #[test]
    fn takes_entry_images_from_media_rss_and_enclosures() {
        let rss = br#"<rss version="2.0" xmlns:media="http://search.yahoo.com/mrss/"><channel><title>T</title>
<item><title>thumb</title><link>https://e.com/1</link><media:thumbnail url="https://img.e.com/1.jpg"/></item>
<item><title>content</title><link>https://e.com/2</link><media:content url="https://img.e.com/2.png" type="image/png"/></item>
<item><title>enclosure</title><link>https://e.com/3</link><enclosure url="https://img.e.com/3.jpg" type="image/jpeg" length="1"/></item>
<item><title>audio</title><link>https://e.com/4</link><enclosure url="https://e.com/4.mp3" type="audio/mpeg" length="1"/></item>
<item><title>http</title><link>https://e.com/5</link><media:thumbnail url="http://img.e.com/5.jpg"/></item>
<item><title>none</title><link>https://e.com/6</link></item>
</channel></rss>"#;
        let (_, items) = parse_feed(rss).unwrap();
        let img: Vec<Option<&str>> = items.iter().map(|i| i.image_url.as_deref()).collect();
        assert_eq!(
            img,
            vec![
                Some("https://img.e.com/1.jpg"),
                Some("https://img.e.com/2.png"),
                Some("https://img.e.com/3.jpg"),
                None,
                None,
                None,
            ]
        );
    }

    #[test]
    fn thumbnail_keys_match_the_serialized_link() {
        let xml = r#"<rss version="2.0" xmlns:media="http://search.yahoo.com/mrss/"><channel><title>T</title>
<item><title>kanji</title><link>https://e.com/記事</link><media:thumbnail url="https://img.e.com/k.jpg"/></item>
<item><title>bare</title><link>https://e.com</link><media:thumbnail url="https://img.e.com/b.jpg"/></item>
</channel></rss>"#;
        assert_eq!(
            images(xml),
            vec![
                (
                    "https://e.com/%E8%A8%98%E4%BA%8B".into(),
                    Some("https://img.e.com/k.jpg".into())
                ),
                (
                    "https://e.com/".into(),
                    Some("https://img.e.com/b.jpg".into())
                ),
            ]
        );
    }

    #[test]
    fn rss_item_ignores_atom_link_href_for_its_key() {
        let xml = r#"<rss version="2.0" xmlns:atom="http://www.w3.org/2005/Atom" xmlns:media="http://search.yahoo.com/mrss/"><channel><title>T</title>
<item><title>t</title><atom:link href="https://e.com/self"/><link>https://e.com/y</link><media:thumbnail url="https://img.e.com/y.jpg"/></item>
</channel></rss>"#;
        assert_eq!(
            images(xml),
            vec![(
                "https://e.com/y".into(),
                Some("https://img.e.com/y.jpg".into())
            )]
        );
    }
}
