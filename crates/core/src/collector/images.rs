//! 記事の画像 (spec 13節)。

use std::collections::{HashMap, HashSet};
use std::time::Duration;

use reqwest::Url;
use scraper::{Html, Selector};

use super::FetchOutcome;
use crate::model::SourceKind;

pub const PAGE_IMAGE_CONCURRENCY: usize = 4;
pub const PAGE_IMAGE_TIMEOUT: Duration = Duration::from_secs(10);
pub const PAGE_IMAGE_MAX_BYTES: usize = 512 * 1024;
pub const PAGE_IMAGE_BUDGET: Duration = Duration::from_secs(60);

const META_SELECTORS: [&str; 4] = [
    r#"meta[property="og:image"]"#,
    r#"meta[property="og:image:secure_url"]"#,
    r#"meta[name="twitter:image"]"#,
    r#"meta[property="twitter:image"]"#,
];

const MAX_URL_LEN: usize = 2048;

/// https の絶対 URL だけを返す。`base` があれば相対 URL を解決する。
/// ユーザー情報 (`user:pass@`) を含むものと、2048 文字を超えるものは捨てる。
pub fn https_image_url(raw: &str, base: Option<&Url>) -> Option<String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    let url = match base {
        Some(b) => b.join(raw).ok()?,
        None => Url::parse(raw).ok()?,
    };
    let ok = url.scheme() == "https"
        && url.host().is_some()
        && url.username().is_empty()
        && url.password().is_none()
        && url.as_str().len() <= MAX_URL_LEN;
    ok.then(|| url.to_string())
}

/// 記事ページの og:image (なければ twitter:image)。https の絶対 URL だけ返す。
pub fn og_image(html: &str, page_url: &Url) -> Option<String> {
    let doc = Html::parse_document(html);
    META_SELECTORS
        .iter()
        .filter_map(|s| Selector::parse(s).ok())
        .find_map(|sel| {
            doc.select(&sel)
                .filter_map(|m| m.value().attr("content"))
                .find_map(|c| https_image_url(c, Some(page_url)))
        })
}

pub fn wants_page_image(kind: SourceKind, item_url: &str) -> bool {
    if kind == SourceKind::Arxiv {
        return false;
    }
    !Url::parse(item_url)
        .ok()
        .and_then(|u| u.host_str().map(str::to_string))
        .is_some_and(|h| h == "news.google.com")
}

/// 新しく入る記事のうち画像の無いものに、記事ページの og:image を付ける。失敗しても何もしない。
pub async fn fill_page_images(outcomes: &mut [FetchOutcome], known: &HashSet<String>) {
    let Ok(http) = crate::page::page_client(false) else {
        return;
    };
    fill_page_images_with(
        outcomes,
        known,
        &http,
        false,
        PAGE_IMAGE_TIMEOUT,
        PAGE_IMAGE_BUDGET,
    )
    .await;
}

/// `page_timeout` は 1 ページあたり、`budget` はこの段階全体の締め切り。
/// 締め切りを過ぎたら新しく取りに行かず、取得中のものも打ち切る (記事は画像なしで入る)。
pub(crate) async fn fill_page_images_with(
    outcomes: &mut [FetchOutcome],
    known: &HashSet<String>,
    http: &reqwest::Client,
    allow_loopback: bool,
    page_timeout: Duration,
    budget: Duration,
) {
    let deadline = tokio::time::Instant::now() + budget;
    // 取りに行く URL ごとに、それを使う (outcome の番号, item の番号) をまとめる。
    let mut targets: Vec<(String, Vec<(usize, usize)>)> = Vec::new();
    let mut index: HashMap<String, usize> = HashMap::new();
    for (oi, o) in outcomes.iter().enumerate() {
        let Ok(items) = &o.result else { continue };
        for (ii, it) in items.iter().enumerate() {
            if it.image_url.is_none()
                && !known.contains(&it.url)
                && wants_page_image(o.source.kind, &it.url)
            {
                let ti = *index.entry(it.url.clone()).or_insert_with(|| {
                    targets.push((it.url.clone(), Vec::new()));
                    targets.len() - 1
                });
                targets[ti].1.push((oi, ii));
            }
        }
    }

    let mut set = tokio::task::JoinSet::new();
    let mut pending = targets.iter().map(|(url, _)| url.clone()).enumerate();
    let targets = &targets;
    let drain = async {
        loop {
            // 空いた枠を埋めて、常に最大 PAGE_IMAGE_CONCURRENCY 件を取得中にする。
            while set.len() < PAGE_IMAGE_CONCURRENCY {
                let Some((ti, url)) = pending.next() else {
                    break;
                };
                let http = http.clone();
                set.spawn(async move {
                    let found = tokio::time::timeout(
                        page_timeout,
                        crate::page::fetch_html_head(
                            &http,
                            &url,
                            PAGE_IMAGE_MAX_BYTES,
                            allow_loopback,
                        ),
                    )
                    .await
                    .ok()
                    .and_then(|r| r.ok())
                    .and_then(|(final_url, html)| og_image(&html, &final_url));
                    (ti, found)
                });
            }
            match set.join_next().await {
                None => break,
                Some(Ok((ti, Some(img)))) => {
                    for &(oi, ii) in &targets[ti].1 {
                        if let Ok(items) = &mut outcomes[oi].result {
                            items[ii].image_url = Some(img.clone());
                        }
                    }
                }
                // 画像なし、または task が panic した: 次へ。
                Some(Ok((_, None))) | Some(Err(_)) => continue,
            }
        }
    };
    let _ = tokio::time::timeout_at(deadline, drain).await;
    set.abort_all();
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::Url;
    use std::collections::HashSet;

    #[test]
    fn rejects_userinfo_and_overlong_urls() {
        for bad in [
            "https://user@cdn.example.com/x.png",
            "https://user:pw@cdn.example.com/x.png",
            "https://:pw@cdn.example.com/x.png",
        ] {
            assert_eq!(https_image_url(bad, None), None, "{bad}");
        }
        let prefix = "https://cdn.example.com/";
        let ok = format!("{prefix}{}", "a".repeat(2048 - prefix.len()));
        assert_eq!(https_image_url(&ok, None).as_deref(), Some(ok.as_str()));
        let long = format!("{ok}a");
        assert_eq!(https_image_url(&long, None), None);
    }

    #[test]
    fn keeps_only_absolute_https_urls() {
        let base = Url::parse("https://news.example.com/a/b.html").unwrap();
        assert_eq!(
            https_image_url(" https://cdn.example.com/x.png ", None).as_deref(),
            Some("https://cdn.example.com/x.png")
        );
        assert_eq!(
            https_image_url("/img/x.png", Some(&base)).as_deref(),
            Some("https://news.example.com/img/x.png")
        );
        assert_eq!(
            https_image_url("x.png", Some(&base)).as_deref(),
            Some("https://news.example.com/a/x.png")
        );
        assert_eq!(
            https_image_url("//cdn.example.com/x.png", Some(&base)).as_deref(),
            Some("https://cdn.example.com/x.png")
        );
        for bad in [
            "http://cdn.example.com/x.png",
            "data:image/png;base64,AAAA",
            "javascript:alert(1)",
            "",
            "   ",
            "/img/x.png",
        ] {
            assert_eq!(https_image_url(bad, None), None, "{bad}");
        }
        let http_base = Url::parse("http://news.example.com/a").unwrap();
        assert_eq!(
            https_image_url("/img/x.png", Some(&http_base)),
            None,
            "http のページの相対 URL は http になるので捨てる"
        );
    }

    #[test]
    fn reads_og_and_twitter_images() {
        let page = Url::parse("https://news.example.com/a/b.html").unwrap();
        let html = |head: &str| format!("<html><head>{head}</head><body></body></html>");
        assert_eq!(
            og_image(
                &html(r#"<meta property="og:image" content="https://img.example.com/og.jpg">"#),
                &page
            )
            .as_deref(),
            Some("https://img.example.com/og.jpg")
        );
        assert_eq!(
            og_image(
                &html(r#"<meta name="twitter:image" content="/tw.png">"#),
                &page
            )
            .as_deref(),
            Some("https://news.example.com/tw.png")
        );
        assert_eq!(
            og_image(
                &html(
                    r#"<meta property="og:image" content="http://img.example.com/x.jpg"><meta name="twitter:image" content="https://img.example.com/t.jpg">"#
                ),
                &page
            )
            .as_deref(),
            Some("https://img.example.com/t.jpg"),
            "http の og:image は捨てて次を見る"
        );
        assert_eq!(og_image(&html(""), &page), None);
    }

    #[test]
    fn skips_arxiv_and_google_news() {
        use crate::model::SourceKind;
        assert!(!wants_page_image(
            SourceKind::Arxiv,
            "https://arxiv.org/abs/2610.00001"
        ));
        assert!(!wants_page_image(
            SourceKind::Rss,
            "https://news.google.com/rss/articles/CBMi?oc=5"
        ));
        assert!(wants_page_image(
            SourceKind::Rss,
            "https://zenn.dev/a/articles/x"
        ));
        assert!(wants_page_image(
            SourceKind::Hackernews,
            "https://example.com/post"
        ));
    }

    fn outcome_with(items: Vec<crate::model::NewItem>, config: String) -> super::FetchOutcome {
        use crate::model::{Category, Source, SourceKind};
        super::FetchOutcome {
            source: Source {
                id: 1,
                kind: SourceKind::Rss,
                name: "S".into(),
                category: Category::TechJp,
                config,
                enabled: true,
                last_fetched_at: None,
                last_error: None,
            },
            result: Ok(items),
        }
    }

    #[tokio::test]
    async fn fills_new_items_only_and_survives_failures() {
        use crate::model::NewItem;
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        let page = |img: &str| {
            format!(r#"<html><head><meta property="og:image" content="{img}"></head></html>"#)
        };
        Mock::given(method("GET"))
            .and(path("/new"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(
                page("https://img.example.com/new.jpg").into_bytes(),
                "text/html",
            ))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/known"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(
                page("https://img.example.com/k.jpg").into_bytes(),
                "text/html",
            ))
            .expect(0)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/has-image"))
            .respond_with(ResponseTemplate::new(200))
            .expect(0)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/missing"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/pdf"))
            .respond_with(
                ResponseTemplate::new(200).set_body_raw(b"%PDF".to_vec(), "application/pdf"),
            )
            .mount(&server)
            .await;

        let base = server.uri();
        let item = |p: &str, img: Option<&str>| NewItem {
            url: format!("{base}{p}"),
            title: p.into(),
            excerpt: String::new(),
            author: None,
            published_at: None,
            score: None,
            image_url: img.map(String::from),
        };
        let mut outcomes = vec![outcome_with(
            vec![
                item("/new", None),
                item("/known", None),
                item("/has-image", Some("https://img.example.com/feed.jpg")),
                item("/missing", None),
                item("/pdf", None),
            ],
            format!("{base}/feed"),
        )];
        let known: HashSet<String> = [format!("{base}/known")].into_iter().collect();
        let http = crate::page::page_client(true).unwrap();
        fill_page_images_with(
            &mut outcomes,
            &known,
            &http,
            true,
            PAGE_IMAGE_TIMEOUT,
            PAGE_IMAGE_BUDGET,
        )
        .await;
        let items = outcomes[0].result.as_ref().unwrap();
        let img: Vec<Option<&str>> = items.iter().map(|i| i.image_url.as_deref()).collect();
        assert_eq!(
            img,
            vec![
                Some("https://img.example.com/new.jpg"),
                None,
                Some("https://img.example.com/feed.jpg"),
                None,
                None,
            ]
        );
    }

    #[tokio::test]
    async fn public_path_refuses_local_pages() {
        use crate::model::NewItem;
        let mut outcomes = vec![outcome_with(
            vec![NewItem {
                url: "http://127.0.0.1:9/x".into(),
                title: "x".into(),
                excerpt: String::new(),
                author: None,
                published_at: None,
                score: None,
                image_url: None,
            }],
            "https://e.com/feed".into(),
        )];
        fill_page_images(&mut outcomes, &HashSet::new()).await;
        assert_eq!(outcomes[0].result.as_ref().unwrap()[0].image_url, None);
    }

    fn plain_item(url: String) -> crate::model::NewItem {
        crate::model::NewItem {
            url,
            title: "t".into(),
            excerpt: String::new(),
            author: None,
            published_at: None,
            score: None,
            image_url: None,
        }
    }

    /// `/{name}` に og:image 付きのページを置く。`delay` があれば応答を遅らせる。
    async fn mount_page(server: &wiremock::MockServer, name: &str, delay: Option<Duration>) {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, ResponseTemplate};
        let html = format!(
            r#"<html><head><meta property="og:image" content="https://img.example.com/{name}.jpg"></head></html>"#
        );
        let mut resp = ResponseTemplate::new(200).set_body_raw(html.into_bytes(), "text/html");
        if let Some(d) = delay {
            resp = resp.set_delay(d);
        }
        Mock::given(method("GET"))
            .and(path(format!("/{name}")))
            .respond_with(resp)
            .mount(server)
            .await;
    }

    fn images_of(outcomes: &[super::FetchOutcome]) -> Vec<Option<String>> {
        outcomes
            .iter()
            .flat_map(|o| o.result.as_ref().unwrap().iter())
            .map(|i| i.image_url.clone())
            .collect()
    }

    #[tokio::test]
    async fn budget_stops_slow_pages_without_losing_fast_ones() {
        let server = wiremock::MockServer::start().await;
        mount_page(&server, "fast1", None).await;
        mount_page(&server, "slow", Some(Duration::from_secs(5))).await;
        mount_page(&server, "fast2", None).await;
        let base = server.uri();
        let mut outcomes = vec![outcome_with(
            ["fast1", "slow", "fast2"]
                .iter()
                .map(|n| plain_item(format!("{base}/{n}")))
                .collect(),
            format!("{base}/feed"),
        )];
        let http = crate::page::page_client(true).unwrap();
        let budget = Duration::from_millis(600);
        let started = std::time::Instant::now();
        fill_page_images_with(
            &mut outcomes,
            &HashSet::new(),
            &http,
            true,
            PAGE_IMAGE_TIMEOUT,
            budget,
        )
        .await;
        let elapsed = started.elapsed();
        assert!(
            elapsed < budget + Duration::from_millis(400),
            "returned after {elapsed:?}"
        );
        assert_eq!(
            images_of(&outcomes),
            vec![
                Some("https://img.example.com/fast1.jpg".to_string()),
                None,
                Some("https://img.example.com/fast2.jpg".to_string()),
            ]
        );
    }

    #[tokio::test]
    async fn a_slow_page_does_not_stall_the_rest_of_the_window() {
        // 8 件のうち先頭だけが遅い。4 件ずつの塊なら後ろの 4 件は先頭を待つが、
        // 窓なら空いた枠からすぐ次を取りに行くので、予算内に 7 件とも取れる。
        let server = wiremock::MockServer::start().await;
        mount_page(&server, "slow", Some(Duration::from_secs(5))).await;
        let fast: Vec<String> = (1..=7).map(|i| format!("fast{i}")).collect();
        for n in &fast {
            mount_page(&server, n, None).await;
        }
        let base = server.uri();
        let mut items = vec![plain_item(format!("{base}/slow"))];
        items.extend(fast.iter().map(|n| plain_item(format!("{base}/{n}"))));
        let mut outcomes = vec![outcome_with(items, format!("{base}/feed"))];
        let http = crate::page::page_client(true).unwrap();
        fill_page_images_with(
            &mut outcomes,
            &HashSet::new(),
            &http,
            true,
            PAGE_IMAGE_TIMEOUT,
            Duration::from_millis(800),
        )
        .await;
        let mut want = vec![None];
        want.extend(
            fast.iter()
                .map(|n| Some(format!("https://img.example.com/{n}.jpg"))),
        );
        assert_eq!(images_of(&outcomes), want);
    }

    #[tokio::test]
    async fn a_page_slower_than_the_page_timeout_gets_no_image() {
        // 予算は十分にあるが、1 ページの待ち時間を短くすると、遅いページだけ画像なしで、
        // 段階はそのページを待たずに終わり、ほかのページの画像は入る。
        let server = wiremock::MockServer::start().await;
        mount_page(&server, "slow", Some(Duration::from_secs(3))).await;
        mount_page(&server, "fast1", None).await;
        mount_page(&server, "fast2", Some(Duration::from_millis(20))).await;
        let base = server.uri();
        let mut outcomes = vec![outcome_with(
            ["slow", "fast1", "fast2"]
                .iter()
                .map(|n| plain_item(format!("{base}/{n}")))
                .collect(),
            format!("{base}/feed"),
        )];
        let http = crate::page::page_client(true).unwrap();
        let started = std::time::Instant::now();
        fill_page_images_with(
            &mut outcomes,
            &HashSet::new(),
            &http,
            true,
            Duration::from_millis(250),
            Duration::from_secs(10),
        )
        .await;
        let elapsed = started.elapsed();
        assert!(
            elapsed < Duration::from_millis(900),
            "returned after {elapsed:?}"
        );
        assert_eq!(
            images_of(&outcomes),
            vec![
                None,
                Some("https://img.example.com/fast1.jpg".to_string()),
                Some("https://img.example.com/fast2.jpg".to_string()),
            ]
        );
    }

    #[tokio::test]
    async fn same_url_in_two_sources_is_fetched_once() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, ResponseTemplate};
        let server = wiremock::MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/dup"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(
                br#"<html><head><meta property="og:image" content="https://img.example.com/dup.jpg"></head></html>"#.to_vec(),
                "text/html",
            ))
            .expect(1)
            .mount(&server)
            .await;
        let base = server.uri();
        let url = format!("{base}/dup");
        let mut outcomes = vec![
            outcome_with(vec![plain_item(url.clone())], format!("{base}/a")),
            outcome_with(vec![plain_item(url.clone())], format!("{base}/b")),
        ];
        let http = crate::page::page_client(true).unwrap();
        fill_page_images_with(
            &mut outcomes,
            &HashSet::new(),
            &http,
            true,
            PAGE_IMAGE_TIMEOUT,
            PAGE_IMAGE_BUDGET,
        )
        .await;
        let want = Some("https://img.example.com/dup.jpg".to_string());
        assert_eq!(images_of(&outcomes), vec![want.clone(), want]);
    }
}
