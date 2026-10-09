//! 記事の画像 (spec 13節)。

use reqwest::Url;

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

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::Url;

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
}
