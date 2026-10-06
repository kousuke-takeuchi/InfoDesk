pub const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS sources (
    id INTEGER PRIMARY KEY,
    kind TEXT NOT NULL,
    name TEXT NOT NULL,
    category TEXT NOT NULL,
    config TEXT NOT NULL,
    enabled INTEGER NOT NULL DEFAULT 1,
    last_fetched_at TEXT,
    last_error TEXT,
    UNIQUE(kind, config)
);
CREATE TABLE IF NOT EXISTS items (
    id INTEGER PRIMARY KEY,
    source_id INTEGER,
    source_name TEXT NOT NULL,
    category TEXT NOT NULL,
    url TEXT NOT NULL UNIQUE,
    title TEXT NOT NULL,
    excerpt TEXT NOT NULL DEFAULT '',
    author TEXT,
    published_at TEXT,
    fetched_at TEXT NOT NULL,
    score INTEGER,
    ai_summary TEXT,
    read INTEGER NOT NULL DEFAULT 0,
    starred INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS idx_items_category_fetched ON items(category, fetched_at);
CREATE VIRTUAL TABLE IF NOT EXISTS items_fts USING fts5(title, excerpt, tokenize='trigram');
CREATE TABLE IF NOT EXISTS topics (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    keywords TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS digests (
    id INTEGER PRIMARY KEY,
    period TEXT NOT NULL,
    period_start TEXT NOT NULL,
    scope TEXT NOT NULL,
    body TEXT NOT NULL DEFAULT '',
    item_ids TEXT NOT NULL DEFAULT '[]',
    provider TEXT NOT NULL DEFAULT '',
    status TEXT NOT NULL,
    attempts INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL,
    UNIQUE(period, period_start, scope)
);
CREATE TABLE IF NOT EXISTS settings (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
"#;
