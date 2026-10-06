use rusqlite::Connection;

use super::schema::SCHEMA;
use crate::error::Result;

/// 現在のスキーマ版。`MIGRATIONS` の最後の版と一致させる。
pub const SCHEMA_VERSION: i32 = 2;

/// (適用後の版, SQL)。版の順に並べる。版1は Plan 1 のスキーマ全体。
const MIGRATIONS: &[(i32, &str)] = &[
    (1, SCHEMA),
    (2, "ALTER TABLE digests ADD COLUMN last_error TEXT;"),
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
    fn newer_database_is_rejected() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA user_version = 99").unwrap();
        assert!(migrate(&conn).is_err());
    }
}
