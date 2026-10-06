pub mod feed;
pub mod github;
pub mod hackernews;
pub mod huggingface;
pub mod text;

use std::time::Duration;

use crate::error::Result;

pub fn http_client() -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .user_agent("InfoDesk/0.1")
        .timeout(Duration::from_secs(30))
        .build()?)
}
