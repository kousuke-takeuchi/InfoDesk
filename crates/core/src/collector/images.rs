//! 記事の画像 (spec 13節)。

use reqwest::Url;

/// https の絶対 URL だけを返す。`base` があれば相対 URL を解決する。
pub fn https_image_url(raw: &str, base: Option<&Url>) -> Option<String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    let url = match base {
        Some(b) => b.join(raw).ok()?,
        None => Url::parse(raw).ok()?,
    };
    (url.scheme() == "https" && url.host().is_some()).then(|| url.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::Url;

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
}
