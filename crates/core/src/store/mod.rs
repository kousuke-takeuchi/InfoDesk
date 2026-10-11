mod chat;
mod digests;
mod items;
mod migrate;
mod schema;
mod settings;
mod sources;
mod stats;
mod topics;

pub use digests::NewDigest;

use std::path::Path;

use chrono::{DateTime, SecondsFormat, Utc};
use rusqlite::{Connection, Row};

use crate::error::Result;

pub struct Store {
    pub(crate) conn: Connection,
}

impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let conn = Connection::open(path)?;
        // CLI、アプリ、将来のサイドカーが同じファイルを使うため。
        conn.execute_batch("PRAGMA journal_mode = WAL; PRAGMA busy_timeout = 5000;")?;
        Self::init(conn)
    }

    pub fn open_in_memory() -> Result<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Self> {
        migrate::migrate(&conn)?;
        Ok(Self { conn })
    }

    #[cfg(test)]
    pub(crate) fn conn_execute_for_test(&self, sql: &str) {
        self.conn.execute(sql, []).unwrap();
    }
}

/// DBに保存する時刻文字列。秒精度、UTC、末尾Z。文字列比較が時刻順と一致する。
pub(crate) fn ts(dt: &DateTime<Utc>) -> String {
    dt.to_rfc3339_opts(SecondsFormat::Secs, true)
}

pub(crate) fn conv_err(col: usize, msg: String) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(col, rusqlite::types::Type::Text, msg.into())
}

pub(crate) fn col_parse<T>(
    row: &Row,
    col: usize,
    parse: impl Fn(&str) -> Option<T>,
) -> rusqlite::Result<T> {
    let s: String = row.get(col)?;
    parse(&s).ok_or_else(|| conv_err(col, format!("unexpected value: {s}")))
}

fn parse_ts(s: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|d| d.with_timezone(&Utc))
}

pub(crate) fn col_ts(row: &Row, col: usize) -> rusqlite::Result<DateTime<Utc>> {
    col_parse(row, col, parse_ts)
}

pub(crate) fn col_opt_ts(row: &Row, col: usize) -> rusqlite::Result<Option<DateTime<Utc>>> {
    let s: Option<String> = row.get(col)?;
    match s {
        None => Ok(None),
        Some(s) => parse_ts(&s)
            .map(Some)
            .ok_or_else(|| conv_err(col, format!("unexpected value: {s}"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Category, DigestScope, Period, SourceKind};

    #[test]
    fn settings_roundtrip_and_default() {
        let store = Store::open_in_memory().unwrap();
        assert_eq!(store.get_setting("k").unwrap(), None);
        assert_eq!(store.get_setting_or("k", "d").unwrap(), "d");
        store.set_setting("k", "v1").unwrap();
        store.set_setting("k", "v2").unwrap();
        assert_eq!(store.get_setting("k").unwrap().as_deref(), Some("v2"));
    }

    #[test]
    fn open_twice_keeps_data() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.db");
        Store::open(&path).unwrap().set_setting("k", "v").unwrap();
        let again = Store::open(&path).unwrap();
        assert_eq!(again.get_setting("k").unwrap().as_deref(), Some("v"));
    }

    #[test]
    fn file_database_uses_wal() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("w.db")).unwrap();
        let mode: String = store
            .conn
            .query_row("PRAGMA journal_mode", [], |r| r.get(0))
            .unwrap();
        assert_eq!(mode, "wal");
    }

    #[test]
    fn enum_strings_roundtrip() {
        for c in Category::ALL {
            assert_eq!(Category::parse(c.as_str()), Some(c));
        }
        for k in SourceKind::ALL {
            assert_eq!(SourceKind::parse(k.as_str()), Some(k));
        }
        assert_eq!(Period::parse("week"), Some(Period::Week));
        assert_eq!(Category::parse("nope"), None);
        let s = DigestScope::Topic(3);
        assert_eq!(DigestScope::parse(&s.key()), Some(s));
        let s = DigestScope::Category(Category::Medcare);
        assert_eq!(s.key(), "category:medcare");
        assert_eq!(DigestScope::parse(&s.key()), Some(s));
        assert_eq!(DigestScope::parse("garbage"), None);
    }

    #[test]
    fn ts_is_second_precision_utc_z() {
        let t: DateTime<Utc> = "2026-10-05T10:00:00.123+09:00".parse().unwrap();
        assert_eq!(ts(&t), "2026-10-05T01:00:00Z");
    }
}
