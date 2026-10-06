use crate::collector::text::truncate_chars;
use crate::model::{Item, Period};

pub const ITEM_SYSTEM: &str = "あなたは技術と医療介護のニュースを要約する編集者です。\
入力された記事を日本語で1〜2文に要約します。英語の記事は日本語に訳して要約します。\
要約文だけを出力し、前置き、見出し、記号を付けません。入力にない事実を加えません。";

pub const DIGEST_SYSTEM: &str = "あなたは情報収集アシスタントです。\
与えられた記事一覧、または下位期間の要約から、指定された期間の動向を日本語でまとめます。\
重要な話題を3〜6個の箇条書き (各1〜2文) にし、最後に全体の傾向を1文で述べます。\
入力にない事実を加えません。前置きを付けません。";

const ITEM_EXCERPT_MAX_CHARS: usize = 400;

pub fn item_prompt(item: &Item) -> String {
    format!(
        "タイトル: {}\nソース: {}\n概要: {}",
        item.title,
        item.source_name,
        truncate_chars(&item.excerpt, ITEM_EXCERPT_MAX_CHARS)
    )
}

pub fn digest_prompt(label: &str, period: Period, lines: &[String]) -> String {
    format!(
        "対象: {label}\n期間: {}\n\n入力:\n{}",
        period.label_ja(),
        lines.join("\n")
    )
}
