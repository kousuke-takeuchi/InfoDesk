use rusqlite::Connection;

use super::schema::SCHEMA;
use crate::error::Result;

/// 現在のスキーマ版。`MIGRATIONS` の最後の版と一致させる。
pub const SCHEMA_VERSION: i32 = 5;

/// (適用後の版, SQL)。版の順に並べる。版1は Plan 1 のスキーマ全体。
const MIGRATIONS: &[(i32, &str)] = &[
    (1, SCHEMA),
    (2, "ALTER TABLE digests ADD COLUMN last_error TEXT;"),
    (
        3,
        "CREATE TABLE IF NOT EXISTS chat_threads (
             id INTEGER PRIMARY KEY,
             title TEXT NOT NULL,
             created_at TEXT NOT NULL,
             updated_at TEXT NOT NULL
         );
         CREATE TABLE IF NOT EXISTS chat_messages (
             thread_id INTEGER NOT NULL REFERENCES chat_threads(id) ON DELETE CASCADE,
             seq INTEGER NOT NULL,
             id TEXT NOT NULL,
             role TEXT NOT NULL,
             content TEXT NOT NULL,
             created_at TEXT NOT NULL,
             PRIMARY KEY (thread_id, seq)
         );",
    ),
    (4, "ALTER TABLE items ADD COLUMN image_url TEXT;"),
    (
        5,
        "ALTER TABLE items ADD COLUMN read_changed_at TEXT;
         ALTER TABLE items ADD COLUMN starred_changed_at TEXT;
         ALTER TABLE items ADD COLUMN read_changed_by TEXT;
         ALTER TABLE items ADD COLUMN starred_changed_by TEXT;
         UPDATE items SET read_changed_at = '1970-01-01T00:00:00.000Z' WHERE read = 1;
         UPDATE items SET starred_changed_at = '1970-01-01T00:00:00.000Z' WHERE starred = 1;
         CREATE TABLE IF NOT EXISTS sync_ops (
             id INTEGER PRIMARY KEY,
             item_id INTEGER NOT NULL,
             field TEXT NOT NULL,
             value INTEGER NOT NULL,
             at TEXT NOT NULL,
             uploaded INTEGER NOT NULL DEFAULT 0
         );
         CREATE TABLE IF NOT EXISTS sync_files (
             name TEXT PRIMARY KEY,
             kind TEXT NOT NULL,
             drive_id TEXT,
             sha256 TEXT,
             updated_at TEXT NOT NULL,
             fingerprint TEXT
         );",
    ),
];

fn user_version(conn: &Connection) -> Result<i32> {
    Ok(conn.query_row("PRAGMA user_version", [], |r| r.get(0))?)
}

/// `user_version` を見て、足りない移行を順に適用する。
/// 版0のDBは、空の新規DBか Plan 1 のスキーマのどちらかである。
/// 版1のSQLは `IF NOT EXISTS` だけなので、どちらにも適用できる。
pub fn migrate(conn: &Connection) -> Result<()> {
    let current = user_version(conn)?;
    if current > SCHEMA_VERSION {
        return Err(crate::error::Error::Invalid(format!(
            "database schema version {current} is newer than supported {SCHEMA_VERSION}"
        )));
    }
    for (version, sql) in MIGRATIONS {
        if *version <= current {
            continue;
        }
        conn.execute_batch(&format!(
            "BEGIN; {sql} PRAGMA user_version = {version}; COMMIT;"
        ))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn version(conn: &Connection) -> i32 {
        conn.query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap()
    }

    fn has_column(conn: &Connection, table: &str, col: &str) -> bool {
        let mut stmt = conn
            .prepare(&format!("PRAGMA table_info({table})"))
            .unwrap();
        stmt.query_map([], |r| r.get::<_, String>(1))
            .unwrap()
            .any(|c| c.unwrap() == col)
    }

    #[test]
    fn fresh_database_reaches_current_version() {
        let conn = Connection::open_in_memory().unwrap();
        migrate(&conn).unwrap();
        assert_eq!(version(&conn), SCHEMA_VERSION);
        assert!(has_column(&conn, "digests", "last_error"));
        // 2回目は何もしない。
        migrate(&conn).unwrap();
        assert_eq!(version(&conn), SCHEMA_VERSION);
    }

    #[test]
    fn plan1_database_without_version_gets_last_error_column_and_keeps_rows() {
        // Review Focus 5: Plan 1 のスキーマ (user_version = 0、last_error なし)。
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(SCHEMA).unwrap();
        conn.execute(
            "INSERT INTO digests(period, period_start, scope, body, status, created_at) \
             VALUES ('day', '2026-10-06', 'category:github', 'kept', 'done', '2026-10-06T00:00:00Z')",
            [],
        )
        .unwrap();
        assert!(!has_column(&conn, "digests", "last_error"));

        migrate(&conn).unwrap();
        assert_eq!(version(&conn), SCHEMA_VERSION);
        assert!(has_column(&conn, "digests", "last_error"));
        let body: String = conn
            .query_row("SELECT body FROM digests", [], |r| r.get(0))
            .unwrap();
        assert_eq!(body, "kept");
    }

    #[test]
    fn version_3_adds_chat_tables() {
        let conn = Connection::open_in_memory().unwrap();
        migrate(&conn).unwrap();
        assert!(version(&conn) >= 3);
        assert!(has_column(&conn, "chat_threads", "updated_at"));
        assert!(has_column(&conn, "chat_messages", "content"));
    }

    #[test]
    fn newer_database_is_rejected() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA user_version = 99").unwrap();
        assert!(migrate(&conn).is_err());
    }

    #[test]
    fn version_4_adds_image_url_and_keeps_items() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(SCHEMA).unwrap();
        conn.execute_batch("PRAGMA user_version = 3;").unwrap();
        conn.execute(
            "INSERT INTO sources(kind, name, category, config) VALUES ('rss', 'S', 'tech_jp', 'https://e.com/feed')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO items(source_id, source_name, category, url, title, excerpt, fetched_at) \
             VALUES (1, 'S', 'tech_jp', 'https://e.com/1', '残る記事', '', '2026-10-09T00:00:00Z')",
            [],
        )
        .unwrap();
        migrate(&conn).unwrap();
        assert!(version(&conn) >= 4);
        assert!(has_column(&conn, "items", "image_url"));
        let title: String = conn
            .query_row("SELECT title FROM items", [], |r| r.get(0))
            .unwrap();
        assert_eq!(title, "残る記事");
    }

    #[test]
    fn version_5_adds_sync_columns_and_tables_and_keeps_items() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(SCHEMA).unwrap();
        conn.execute_batch("ALTER TABLE items ADD COLUMN image_url TEXT; PRAGMA user_version = 4;")
            .unwrap();
        conn.execute(
            "INSERT INTO items(source_id, source_name, category, url, title, excerpt, fetched_at) \
             VALUES (NULL, 'S', 'tech_jp', 'https://e.com/1', '残る記事', '', '2026-10-09T00:00:00Z')",
            [],
        )
        .unwrap();
        migrate(&conn).unwrap();
        assert_eq!(version(&conn), SCHEMA_VERSION);
        assert!(has_column(&conn, "items", "read_changed_at"));
        assert!(has_column(&conn, "items", "starred_changed_at"));
        assert!(has_column(&conn, "items", "read_changed_by"));
        assert!(has_column(&conn, "items", "starred_changed_by"));
        assert!(has_column(&conn, "sync_ops", "uploaded"));
        assert!(has_column(&conn, "sync_files", "drive_id"));
        let title: String = conn
            .query_row("SELECT title FROM items", [], |r| r.get(0))
            .unwrap();
        assert_eq!(title, "残る記事");
    }

    #[test]
    fn version_5_stamps_marks_set_before_the_upgrade_with_the_epoch() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(SCHEMA).unwrap();
        conn.execute_batch("ALTER TABLE items ADD COLUMN image_url TEXT; PRAGMA user_version = 4;")
            .unwrap();
        for (url, read, starred) in [
            ("https://e.com/read", 1, 0),
            ("https://e.com/star", 0, 1),
            ("https://e.com/none", 0, 0),
        ] {
            conn.execute(
                "INSERT INTO items(source_id, source_name, category, url, title, excerpt, fetched_at, read, starred) \
                 VALUES (NULL, 'S', 'tech_jp', ?1, 't', '', '2026-10-09T00:00:00Z', ?2, ?3)",
                rusqlite::params![url, read, starred],
            )
            .unwrap();
        }
        migrate(&conn).unwrap();
        let row = |url: &str| -> (
            Option<String>,
            Option<String>,
            Option<String>,
            Option<String>,
        ) {
            conn.query_row(
                "SELECT read_changed_at, read_changed_by, starred_changed_at, starred_changed_by \
                 FROM items WHERE url = ?1",
                [url],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .unwrap()
        };
        let epoch = Some("1970-01-01T00:00:00.000Z".to_string());
        assert_eq!(row("https://e.com/read"), (epoch.clone(), None, None, None));
        assert_eq!(row("https://e.com/star"), (None, None, epoch, None));
        assert_eq!(row("https://e.com/none"), (None, None, None, None));
    }
}
