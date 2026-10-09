//! 実レスポンスを tests/fixtures に保存する。パーサーのテストが読む。
//! 実行: cargo run -p infodesk-core --example fetch_fixtures

use std::path::Path;

const TARGETS: &[(&str, &str)] = &[
    (
        "github_trending_daily.html",
        "https://github.com/trending?since=daily",
    ),
    (
        "arxiv_cs_ai.xml",
        "https://export.arxiv.org/api/query?search_query=cat:cs.AI&sortBy=submittedDate&sortOrder=descending&max_results=5",
    ),
    (
        "hf_models.json",
        "https://huggingface.co/api/models?sort=trendingScore&limit=5",
    ),
    (
        "hf_papers.json",
        "https://huggingface.co/api/daily_papers?limit=5",
    ),
    (
        "hn_topstories.json",
        "https://hacker-news.firebaseio.com/v0/topstories.json",
    ),
];

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    std::fs::create_dir_all(&dir)?;
    let http = infodesk_core::collector::http_client()?;
    for (name, url) in TARGETS {
        let bytes = http
            .get(*url)
            .send()
            .await?
            .error_for_status()?
            .bytes()
            .await?;
        std::fs::write(dir.join(name), &bytes)?;
        println!("{name}: {} bytes", bytes.len());
    }
    Ok(())
}
