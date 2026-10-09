//! チャットのエージェントが求めた URL の本文をテキストにする。DB には保存しない (spec 12節)。
//!
//! URL はフィードや Web ページの文面を読んだ LLM が選ぶので、攻撃者が決められるものとして扱う。
//! loopback・プライベート・リンクローカルなどのアドレスには接続しない (SSRF 対策)。

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::time::Duration;

use reqwest::dns::{Addrs, Name, Resolve, Resolving};
use reqwest::{Url, redirect};
use scraper::{Html, Node, Selector};
use serde::Serialize;

use crate::collector::text::{collapse_ws, truncate_chars};
use crate::error::{Error, Result};

pub const MAX_BYTES: usize = 2_000_000;
pub const DEFAULT_MAX_CHARS: usize = 20_000;

/// リダイレクトをたどる回数の上限。
const MAX_REDIRECTS: usize = 5;

/// ローカルやプライベートのアドレスを拒んだときのエラー文。
const REFUSED: &str = "refusing to fetch private or local address";

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

/// URL の本文を取得する。ローカルやプライベートのアドレスへは接続しない。
pub async fn fetch_page(url: &str, max_chars: usize) -> Result<PageText> {
    let http = page_client(false)?;
    fetch_with(&http, url, max_chars, false).await
}

/// ページ取得用のクライアント。名前解決でローカルやプライベートのアドレスを落とし、
/// リダイレクトのたびに URL を検査する。`allow_loopback` はテスト (wiremock は 127.0.0.1)
/// のためだけにあり、名前解決で loopback を許す。リダイレクトの検査は常に本番の規則。
pub(crate) fn page_client(allow_loopback: bool) -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .user_agent("InfoDesk/0.1")
        .timeout(Duration::from_secs(30))
        .dns_resolver(GuardedResolver { allow_loopback })
        .redirect(redirect::Policy::custom(|attempt| {
            match check_redirect(attempt.url(), attempt.previous().len()) {
                Ok(()) => attempt.follow(),
                Err(msg) => attempt.error(msg),
            }
        }))
        .build()?)
}

pub(crate) async fn fetch_with(
    http: &reqwest::Client,
    url: &str,
    max_chars: usize,
    allow_loopback: bool,
) -> Result<PageText> {
    let (mut resp, content_type_header, content_type) = open(http, url, allow_loopback).await?;
    let is_html = content_type == "text/html" || content_type == "application/xhtml+xml";
    let final_url = resp.url().to_string();
    let mut bytes: Vec<u8> = Vec::new();
    let mut cut = false;
    while let Some(chunk) = resp.chunk().await.map_err(http_error)? {
        let room = MAX_BYTES - bytes.len();
        if chunk.len() > room {
            bytes.extend_from_slice(&chunk[..room]);
            cut = true;
            break;
        }
        bytes.extend_from_slice(&chunk);
    }
    let raw = decode(&bytes, &content_type_header, is_html);
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

/// URL を検査して送り、状態コードと Content-Type を確かめる。
/// 戻り値は (応答, Content-Type ヘッダーの全文, 小文字にした MIME 型)。
async fn open(
    http: &reqwest::Client,
    url: &str,
    allow_loopback: bool,
) -> Result<(reqwest::Response, String, String)> {
    let parsed = Url::parse(url)
        .ok()
        .filter(|u| matches!(u.scheme(), "http" | "https"))
        .ok_or_else(|| Error::Invalid(format!("only http and https URLs can be fetched: {url}")))?;
    check_host(&parsed, allow_loopback).map_err(Error::Invalid)?;
    let resp = http.get(parsed).send().await.map_err(http_error)?;
    let status = resp.status();
    if !status.is_success() {
        return Err(Error::Invalid(format!(
            "fetch failed: HTTP {}",
            status.as_u16()
        )));
    }
    let content_type_header = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("text/html")
        .to_string();
    let content_type = content_type_header
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
    Ok((resp, content_type_header, content_type))
}

/// 記事ページの先頭 `max_bytes` を文字コードに従って読む。HTML 以外は `Error::Invalid`。
/// 戻り値は (最終 URL, HTML)。
pub(crate) async fn fetch_html_head(
    http: &reqwest::Client,
    url: &str,
    max_bytes: usize,
    allow_loopback: bool,
) -> Result<(Url, String)> {
    let (mut resp, content_type_header, content_type) = open(http, url, allow_loopback).await?;
    if content_type != "text/html" && content_type != "application/xhtml+xml" {
        return Err(Error::Invalid("not html".into()));
    }
    let final_url = resp.url().clone();
    let mut bytes: Vec<u8> = Vec::new();
    while let Some(chunk) = resp.chunk().await.map_err(http_error)? {
        let room = max_bytes - bytes.len();
        if chunk.len() >= room {
            bytes.extend_from_slice(&chunk[..room]);
            break;
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok((final_url, decode(&bytes, &content_type_header, true)))
}

/// reqwest のエラーの原因をたどり、アドレスを拒んだものなら分かる文にする。
fn http_error(e: reqwest::Error) -> Error {
    let mut source: Option<&(dyn std::error::Error + 'static)> = Some(&e);
    while let Some(err) = source {
        let msg = err.to_string();
        if msg.contains(REFUSED) {
            return Error::Invalid(msg);
        }
        source = err.source();
    }
    Error::Http(e)
}

/// 接続してはいけないアドレスなら true。IPv4 射影の IPv6 は IPv4 として判定する。
pub(crate) fn is_blocked_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => is_blocked_v4(v4),
        IpAddr::V6(v6) => match v6.to_ipv4_mapped() {
            Some(v4) => is_blocked_v4(v4),
            None => is_blocked_v6(v6),
        },
    }
}

fn is_blocked_v4(ip: Ipv4Addr) -> bool {
    let o = ip.octets();
    ip.is_loopback()
        || ip.is_private()
        || ip.is_link_local()
        || ip.is_unspecified()
        || ip.is_multicast()
        || ip.is_broadcast()
        || o[0] == 0 // 0.0.0.0/8
        || (o[0] == 100 && (o[1] & 0xc0) == 64) // CGNAT 100.64.0.0/10
}

fn is_blocked_v6(ip: Ipv6Addr) -> bool {
    let first = ip.segments()[0];
    ip.is_loopback()
        || ip.is_unspecified()
        || ip.is_multicast()
        || (first & 0xfe00) == 0xfc00 // fc00::/7
        || (first & 0xffc0) == 0xfe80 // fe80::/10
}

fn refused(ip: IpAddr, allow_loopback: bool) -> bool {
    let loopback = match ip {
        IpAddr::V4(v4) => v4.is_loopback(),
        IpAddr::V6(v6) => {
            v6.is_loopback() || v6.to_ipv4_mapped().is_some_and(|v4| v4.is_loopback())
        }
    };
    is_blocked_ip(ip) && !(allow_loopback && loopback)
}

/// ホストが IP リテラルなら名前解決を通らないので、ここで検査する。localhost も拒む。
fn check_host(url: &Url, allow_loopback: bool) -> std::result::Result<(), String> {
    // host_str は正規化済み (例: 2130706433 は 127.0.0.1、IPv6 は [..] 付き)。
    let blocked = match url.host_str() {
        None => true,
        Some(host) => {
            let bare = host.trim_start_matches('[').trim_end_matches(']');
            match bare.parse::<IpAddr>() {
                Ok(ip) => refused(ip, allow_loopback),
                Err(_) => {
                    let d = host.trim_end_matches('.').to_ascii_lowercase();
                    !allow_loopback && (d == "localhost" || d.ends_with(".localhost"))
                }
            }
        }
    };
    if blocked {
        Err(format!("{REFUSED}: {url}"))
    } else {
        Ok(())
    }
}

/// リダイレクト先の検査。`previous` はこれまでに要求した URL の数 (最初の URL を含む)。
fn check_redirect(next: &Url, previous: usize) -> std::result::Result<(), String> {
    if previous > MAX_REDIRECTS {
        return Err(format!("too many redirects (more than {MAX_REDIRECTS})"));
    }
    if !matches!(next.scheme(), "http" | "https") {
        return Err(format!("only http and https URLs can be fetched: {next}"));
    }
    check_host(next, false)
}

/// 名前解決の結果からローカルやプライベートのアドレスを落とす。全部落ちたら失敗にする。
struct GuardedResolver {
    allow_loopback: bool,
}

impl Resolve for GuardedResolver {
    fn resolve(&self, name: Name) -> Resolving {
        let host = name.as_str().to_string();
        let allow_loopback = self.allow_loopback;
        Box::pin(async move {
            let addrs: Vec<SocketAddr> = tokio::net::lookup_host((host.as_str(), 0))
                .await?
                .filter(|a| !refused(a.ip(), allow_loopback))
                .collect();
            if addrs.is_empty() {
                return Err(format!("{REFUSED}: {host}").into());
            }
            let addrs: Addrs = Box::new(addrs.into_iter());
            Ok(addrs)
        })
    }
}

/// 宣言された文字コードで本文を文字列にする。Content-Type の charset、なければ先頭
/// 1024 バイトの meta、どちらもなければ UTF-8。知らない名前も UTF-8 として扱う。
fn decode(bytes: &[u8], content_type: &str, is_html: bool) -> String {
    let label = charset_param(content_type).or_else(|| {
        if is_html {
            meta_charset(&bytes[..bytes.len().min(1024)])
        } else {
            None
        }
    });
    let encoding = label
        .and_then(|l| encoding_rs::Encoding::for_label(l.as_bytes()))
        .unwrap_or(encoding_rs::UTF_8);
    let (text, _, _) = encoding.decode(bytes);
    text.into_owned()
}

fn charset_param(content_type: &str) -> Option<String> {
    content_type.split(';').skip(1).find_map(|param| {
        let (key, value) = param.split_once('=')?;
        key.trim()
            .eq_ignore_ascii_case("charset")
            .then(|| {
                value
                    .trim()
                    .trim_matches(|c| c == '"' || c == '\'')
                    .to_string()
            })
            .filter(|v| !v.is_empty())
    })
}

/// `<meta charset=...>` と `<meta http-equiv="Content-Type" content="...charset=...">` を探す。
fn meta_charset(head: &[u8]) -> Option<String> {
    let lower = head.to_ascii_lowercase();
    let mut from = 0;
    while let Some(pos) = find(&lower[from..], b"<meta") {
        let start = from + pos;
        let end = find(&lower[start..], b">").map_or(lower.len(), |e| start + e);
        let tag = &lower[start..end];
        if let Some(c) = find(tag, b"charset") {
            let rest = &tag[c + b"charset".len()..];
            let rest = trim_start(rest);
            if let Some(rest) = rest.strip_prefix(b"=") {
                let rest = trim_start(rest);
                let rest = rest
                    .strip_prefix(b"\"")
                    .or_else(|| rest.strip_prefix(b"'"))
                    .unwrap_or(rest);
                let label: Vec<u8> = rest
                    .iter()
                    .copied()
                    .take_while(|b| b.is_ascii_alphanumeric() || b"-_.:".contains(b))
                    .collect();
                if !label.is_empty() {
                    return String::from_utf8(label).ok();
                }
            }
        }
        from = end;
    }
    None
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

fn trim_start(bytes: &[u8]) -> &[u8] {
    let n = bytes.iter().take_while(|b| b.is_ascii_whitespace()).count();
    &bytes[n..]
}

fn html_to_text(raw: &str) -> (Option<String>, String) {
    let doc = Html::parse_document(raw);
    let title = ["head > title", "title"]
        .iter()
        .filter_map(|s| Selector::parse(s).ok())
        .find_map(|s| doc.select(&s).next())
        .map(|t| collapse_ws(&t.text().collect::<String>()))
        .filter(|t| !t.is_empty());
    let root = ["article", "main", "body"]
        .iter()
        .filter_map(|tag| Selector::parse(tag).ok())
        .find_map(|s| doc.select(&s).next());
    let Some(root) = root else {
        return (title, String::new());
    };
    // 選んだ根から下だけを見る。除く要素の中には入らない (根より上の form などは関係ない)。
    let mut parts: Vec<&str> = Vec::new();
    let mut stack: Vec<_> = root.children().rev().collect();
    while let Some(node) = stack.pop() {
        match node.value() {
            Node::Text(text) => parts.push(text),
            Node::Element(e) if !SKIP.contains(&e.name()) => stack.extend(node.children().rev()),
            _ => {}
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

    /// テスト用: wiremock (127.0.0.1) に届くよう loopback だけ許す。
    async fn fetch(url: &str, max_chars: usize) -> Result<PageText> {
        fetch_with(&page_client(true).unwrap(), url, max_chars, true).await
    }

    #[tokio::test]
    async fn extracts_title_and_article_text_without_scripts_or_navigation() {
        let html = r#"<html><head><title> 新薬の記事 </title><style>p{}</style></head>
            <body><nav>メニュー</nav><header>ヘッダー</header>
            <article><h1>見出し</h1><p>本文の<b>一段落</b>目。</p><script>alert(1)</script><p>二段落目。</p></article>
            <footer>フッター</footer></body></html>"#;
        let server = serve(html, "text/html; charset=utf-8").await;
        let page = fetch(&format!("{}/a", server.uri()), 20_000).await.unwrap();
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
        let page = fetch(&format!("{}/a", server.uri()), 100).await.unwrap();
        assert_eq!(page.text, "本文だけ");
        let server = serve("plain  text\nbody", "text/plain").await;
        let page = fetch(&format!("{}/a", server.uri()), 100).await.unwrap();
        assert_eq!(page.text, "plain text body");
        assert_eq!(page.title, None);
    }

    #[tokio::test]
    async fn truncates_long_text_and_marks_it() {
        let long = format!("<article><p>{}</p></article>", "あ".repeat(50));
        let server = serve(&long, "text/html").await;
        let page = fetch(&format!("{}/a", server.uri()), 10).await.unwrap();
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
        let page = fetch(&format!("{}/a", server.uri()), usize::MAX)
            .await
            .unwrap();
        assert!(page.truncated, "上限で読むのをやめたら truncated");
        assert!(page.text.chars().all(|c| c == 'a'), "本文は a だけ");
        assert!(page.text.len() <= MAX_BYTES);
        assert!(page.text.len() >= MAX_BYTES - 100, "{}", page.text.len());
    }

    async fn serve_bytes(body: Vec<u8>, content_type: &str) -> MockServer {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/a"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(body, content_type))
            .mount(&server)
            .await;
        server
    }

    #[tokio::test]
    async fn decodes_charset_from_content_type_header() {
        let html = "<html><head><title>日本語の題名</title></head><body><p>シフトJISの本文です。</p></body></html>";
        let (bytes, _, _) = encoding_rs::SHIFT_JIS.encode(html);
        let server = serve_bytes(bytes.into_owned(), "text/html; charset=Shift_JIS").await;
        let page = fetch(&format!("{}/a", server.uri()), 1000).await.unwrap();
        assert_eq!(page.title.as_deref(), Some("日本語の題名"));
        assert_eq!(page.text, "シフトJISの本文です。");
    }

    #[tokio::test]
    async fn decodes_charset_from_meta_tag() {
        let html = r#"<html><head><meta charset="euc-jp"><title>EUCの題名</title></head><body><p>日本語の本文です。</p></body></html>"#;
        let (bytes, _, _) = encoding_rs::EUC_JP.encode(html);
        let server = serve_bytes(bytes.into_owned(), "text/html").await;
        let page = fetch(&format!("{}/a", server.uri()), 1000).await.unwrap();
        assert_eq!(page.title.as_deref(), Some("EUCの題名"));
        assert_eq!(page.text, "日本語の本文です。");

        let html = r#"<html><head><META HTTP-EQUIV="Content-Type" CONTENT="text/html; charset=Shift_JIS"></head><body><p>本文</p></body></html>"#;
        let (bytes, _, _) = encoding_rs::SHIFT_JIS.encode(html);
        let server = serve_bytes(bytes.into_owned(), "text/html").await;
        let page = fetch(&format!("{}/a", server.uri()), 1000).await.unwrap();
        assert_eq!(page.text, "本文");
    }

    #[tokio::test]
    async fn keeps_content_inside_a_form_that_wraps_the_root() {
        let server = serve(
            "<html><body><form><main><p>本文</p></main></form></body></html>",
            "text/html",
        )
        .await;
        let page = fetch(&format!("{}/a", server.uri()), 1000).await.unwrap();
        assert_eq!(page.text, "本文");
    }

    #[test]
    fn title_prefers_head_over_inline_svg_title() {
        let doc = "<html><head><title>本当の題名</title></head><body><svg><title>アイコン</title></svg><p>本文</p></body></html>";
        let (title, text) = html_to_text(doc);
        assert_eq!(title.as_deref(), Some("本当の題名"));
        assert_eq!(text, "本文");
    }

    #[tokio::test]
    async fn body_of_exactly_max_bytes_is_not_truncated() {
        let server = serve(&"a".repeat(MAX_BYTES), "text/plain").await;
        let page = fetch(&format!("{}/a", server.uri()), usize::MAX)
            .await
            .unwrap();
        assert_eq!(page.text.len(), MAX_BYTES);
        assert!(!page.truncated);
    }

    #[tokio::test]
    async fn public_fetch_refuses_local_and_private_addresses_without_sending() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(b"x".to_vec(), "text/plain"))
            .expect(0)
            .mount(&server)
            .await;
        let port = server.address().port();
        let urls = [
            format!("http://127.0.0.1:{port}/a"),
            format!("http://localhost:{port}/a"),
            format!("http://LocalHost.:{port}/a"),
            format!("http://[::1]:{port}/a"),
            format!("http://[::ffff:127.0.0.1]:{port}/a"),
            format!("http://2130706433:{port}/a"),
            "http://10.0.0.1/".to_string(),
            "http://169.254.169.254/latest/meta-data/".to_string(),
        ];
        for url in urls {
            let err = fetch_page(&url, 100).await.unwrap_err();
            assert!(err.to_string().contains(REFUSED), "{url}: {err}");
        }
    }

    #[test]
    fn classifies_private_and_local_addresses() {
        let blocked = [
            "127.0.0.1",
            "127.255.0.9",
            "10.1.2.3",
            "172.16.0.1",
            "172.31.255.255",
            "192.168.1.1",
            "169.254.169.254",
            "100.64.0.1",
            "100.127.255.255",
            "0.0.0.0",
            "224.0.0.1",
            "255.255.255.255",
            "::1",
            "::",
            "fc00::1",
            "fd12:3456::1",
            "fe80::1",
            "ff02::1",
            "::ffff:127.0.0.1",
            "::ffff:10.0.0.1",
            "::ffff:169.254.169.254",
        ];
        for ip in blocked {
            assert!(is_blocked_ip(ip.parse().unwrap()), "{ip} は拒否する");
        }
        for ip in [
            "93.184.216.34",
            "8.8.8.8",
            "172.32.0.1",
            "100.128.0.1",
            "2606:4700::1",
            "::ffff:93.184.216.34",
        ] {
            assert!(!is_blocked_ip(ip.parse().unwrap()), "{ip} は許す");
        }
    }

    #[tokio::test]
    async fn resolver_refuses_names_that_only_resolve_to_local_addresses() {
        use reqwest::dns::Resolve;
        let resolver = GuardedResolver {
            allow_loopback: false,
        };
        let err = match resolver.resolve("localhost".parse().unwrap()).await {
            Ok(addrs) => panic!("resolved: {:?}", addrs.collect::<Vec<_>>()),
            Err(e) => e,
        };
        assert!(err.to_string().contains(REFUSED), "{err}");
    }

    #[tokio::test]
    async fn redirect_to_a_local_address_is_refused() {
        let target = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(b"secret".to_vec(), "text/plain"))
            .expect(0)
            .mount(&target)
            .await;
        let origin = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/a"))
            .respond_with(
                ResponseTemplate::new(302).insert_header("location", format!("{}/", target.uri())),
            )
            .mount(&origin)
            .await;
        // 最初の接続だけテスト用に loopback を許す。リダイレクトの判定は本番と同じ。
        let http = page_client(true).unwrap();
        let err = fetch_with(&http, &format!("{}/a", origin.uri()), 100, true)
            .await
            .unwrap_err();
        assert!(err.to_string().contains(REFUSED), "{err}");
    }

    #[test]
    fn redirect_check_limits_hops_and_schemes() {
        let url = |s: &str| reqwest::Url::parse(s).unwrap();
        assert!(check_redirect(&url("https://example.com/x"), 5).is_ok());
        let err = check_redirect(&url("https://example.com/x"), 6).unwrap_err();
        assert!(err.contains("too many redirects"), "{err}");
        let err = check_redirect(&url("file:///etc/passwd"), 1).unwrap_err();
        assert!(err.contains("only http and https"), "{err}");
        for local in [
            "http://127.0.0.1/",
            "http://localhost./",
            "http://[fe80::1]/",
        ] {
            let err = check_redirect(&url(local), 1).unwrap_err();
            assert!(err.contains(REFUSED), "{local}: {err}");
        }
    }

    #[tokio::test]
    async fn rejects_unsupported_types_errors_and_schemes() {
        let server = serve("%PDF-1.7", "application/pdf").await;
        let err = fetch(&format!("{}/a", server.uri()), 100)
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
        let err = fetch(&format!("{}/a", server.uri()), 100)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("404"), "{err}");

        for bad in ["file:///etc/passwd", "ftp://example.com/x", "not a url"] {
            let err = fetch(bad, 100).await.unwrap_err();
            assert!(
                err.to_string().contains("only http and https"),
                "{bad}: {err}"
            );
        }
    }

    #[tokio::test]
    async fn html_head_reads_only_the_first_bytes() {
        let body = format!(
            "<html><head><title>t</title></head><body>{}</body></html>",
            "a".repeat(10_000)
        );
        let server = serve(&body, "text/html").await;
        let http = page_client(true).unwrap();
        let (url, html) = fetch_html_head(&http, &format!("{}/a", server.uri()), 100, true)
            .await
            .unwrap();
        assert!(url.as_str().ends_with("/a"));
        assert!(html.len() <= 100, "{}", html.len());
        assert!(html.starts_with("<html><head><title>t"));
        let pdf = serve("%PDF", "application/pdf").await;
        assert!(
            fetch_html_head(&http, &format!("{}/a", pdf.uri()), 100, true)
                .await
                .is_err()
        );
    }
}
