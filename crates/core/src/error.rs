#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("database: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("http: {0}")]
    Http(#[from] reqwest::Error),
    #[error("invalid feed: {0}")]
    InvalidFeed(String),
    #[error("parse: {0}")]
    Parse(String),
    #[error("llm: {0}")]
    Llm(String),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("invalid value: {0}")]
    Invalid(String),
}

pub type Result<T> = std::result::Result<T, Error>;
