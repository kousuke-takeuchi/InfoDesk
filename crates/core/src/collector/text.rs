use scraper::Html;

/// 連続する空白と改行を1つの空白にまとめ、前後の空白を除く。
pub fn collapse_ws(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// HTMLタグを除き、実体参照を文字に戻す。
pub fn strip_html(s: &str) -> String {
    let fragment = Html::parse_fragment(s);
    let text = fragment.root_element().text().collect::<Vec<_>>().join(" ");
    collapse_ws(&text)
}

/// `max` 文字を超える場合、`max` 文字で切って `…` を付ける。
pub fn truncate_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max).collect();
    out.push('…');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_tags_and_entities() {
        assert_eq!(
            strip_html("<p>本文 <b>太字</b>&amp;記号</p>\n<br> 次の行"),
            "本文 太字 &記号 次の行"
        );
        assert_eq!(strip_html("plain  text\n here"), "plain text here");
        assert_eq!(strip_html(""), "");
    }

    #[test]
    fn truncates_on_char_boundary() {
        assert_eq!(truncate_chars("あいうえお", 3), "あいう…");
        assert_eq!(truncate_chars("あいう", 3), "あいう");
        assert_eq!(truncate_chars("abc", 10), "abc");
    }
}
