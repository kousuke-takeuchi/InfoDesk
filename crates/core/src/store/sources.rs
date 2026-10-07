use std::collections::HashMap;

use chrono::{DateTime, Utc};
use rusqlite::{OptionalExtension, Row, params};

use super::{Store, col_opt_ts, col_parse, ts};
use crate::error::Result;
use crate::model::{Category, Source, SourceKind};

const SOURCE_COLS: &str = "id, kind, name, category, config, enabled, last_fetched_at, last_error";

fn row_to_source(r: &Row) -> rusqlite::Result<Source> {
    Ok(Source {
        id: r.get(0)?,
        kind: col_parse(r, 1, SourceKind::parse)?,
        name: r.get(2)?,
        category: col_parse(r, 3, Category::parse)?,
        config: r.get(4)?,
        enabled: r.get(5)?,
        last_fetched_at: col_opt_ts(r, 6)?,
        last_error: r.get(7)?,
    })
}

impl Store {
    pub fn add_source(
        &self,
        kind: SourceKind,
        name: &str,
        category: Category,
        config: &str,
    ) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO sources(kind, name, category, config) VALUES (?1, ?2, ?3, ?4)",
            params![kind.as_str(), name, category.as_str(), config],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn list_sources(&self) -> Result<Vec<Source>> {
        let mut stmt = self
            .conn
            .prepare(&format!("SELECT {SOURCE_COLS} FROM sources ORDER BY id"))?;
        let rows = stmt
            .query_map([], row_to_source)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn get_source(&self, id: i64) -> Result<Option<Source>> {
        Ok(self
            .conn
            .query_row(
                &format!("SELECT {SOURCE_COLS} FROM sources WHERE id = ?1"),
                params![id],
                row_to_source,
            )
            .optional()?)
    }

    pub fn update_source(
        &self,
        id: i64,
        name: &str,
        category: Category,
        config: &str,
    ) -> Result<()> {
        self.conn.execute(
            "UPDATE sources SET name = ?2, category = ?3, config = ?4 WHERE id = ?1",
            params![id, name, category.as_str(), config],
        )?;
        Ok(())
    }

    pub fn set_source_enabled(&self, id: i64, enabled: bool) -> Result<()> {
        self.conn.execute(
            "UPDATE sources SET enabled = ?2 WHERE id = ?1",
            params![id, enabled],
        )?;
        Ok(())
    }

    /// ソースを削除する。取得済みの記事は残す。
    pub fn delete_source(&self, id: i64) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        tx.execute(
            "UPDATE items SET source_id = NULL WHERE source_id = ?1",
            params![id],
        )?;
        tx.execute("DELETE FROM sources WHERE id = ?1", params![id])?;
        tx.commit()?;
        Ok(())
    }

    /// ソースごとの保存済み記事数。記事が1件もないソースは含まない。
    pub fn item_counts_by_source(&self) -> Result<HashMap<i64, i64>> {
        let mut stmt = self.conn.prepare(
            "SELECT source_id, COUNT(*) FROM items WHERE source_id IS NOT NULL GROUP BY source_id",
        )?;
        let rows = stmt
            .query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)))?
            .collect::<rusqlite::Result<HashMap<_, _>>>()?;
        Ok(rows)
    }

    pub fn record_fetch(&self, id: i64, at: &DateTime<Utc>, error: Option<&str>) -> Result<()> {
        self.conn.execute(
            "UPDATE sources SET last_fetched_at = ?2, last_error = ?3 WHERE id = ?1",
            params![id, ts(at), error],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_list_update_toggle() {
        let store = Store::open_in_memory().unwrap();
        let id = store
            .add_source(
                SourceKind::Rss,
                "Zenn",
                Category::TechJp,
                "https://zenn.dev/feed",
            )
            .unwrap();
        let s = store.get_source(id).unwrap().unwrap();
        assert_eq!(s.name, "Zenn");
        assert_eq!(s.kind, SourceKind::Rss);
        assert!(s.enabled);
        assert!(s.last_fetched_at.is_none());

        store
            .update_source(
                id,
                "Zenn トレンド",
                Category::TechIntl,
                "https://zenn.dev/feed2",
            )
            .unwrap();
        store.set_source_enabled(id, false).unwrap();
        let s = store.list_sources().unwrap().remove(0);
        assert_eq!(s.name, "Zenn トレンド");
        assert_eq!(s.category, Category::TechIntl);
        assert_eq!(s.config, "https://zenn.dev/feed2");
        assert!(!s.enabled);
    }

    #[test]
    fn duplicate_kind_and_config_is_rejected() {
        let store = Store::open_in_memory().unwrap();
        store
            .add_source(SourceKind::Rss, "A", Category::TechJp, "https://a/feed")
            .unwrap();
        let dup = store.add_source(SourceKind::Rss, "B", Category::Medcare, "https://a/feed");
        assert!(dup.is_err());
    }

    #[test]
    fn record_fetch_sets_time_and_error_then_clears_error() {
        let store = Store::open_in_memory().unwrap();
        let id = store
            .add_source(SourceKind::Hackernews, "HN", Category::Hackernews, "top")
            .unwrap();
        let t1: DateTime<Utc> = "2026-10-05T01:00:00Z".parse().unwrap();
        store.record_fetch(id, &t1, Some("timeout")).unwrap();
        let s = store.get_source(id).unwrap().unwrap();
        assert_eq!(s.last_fetched_at, Some(t1));
        assert_eq!(s.last_error.as_deref(), Some("timeout"));

        let t2: DateTime<Utc> = "2026-10-05T04:00:00Z".parse().unwrap();
        store.record_fetch(id, &t2, None).unwrap();
        let s = store.get_source(id).unwrap().unwrap();
        assert_eq!(s.last_fetched_at, Some(t2));
        assert_eq!(s.last_error, None);
    }

    #[test]
    fn delete_removes_source() {
        let store = Store::open_in_memory().unwrap();
        let id = store
            .add_source(SourceKind::Rss, "A", Category::TechJp, "https://a/feed")
            .unwrap();
        store.delete_source(id).unwrap();
        assert!(store.get_source(id).unwrap().is_none());
        assert!(store.list_sources().unwrap().is_empty());
    }

    #[test]
    fn item_counts_by_source_counts_per_source_and_omits_empty() {
        use crate::testutil::{at, new_item};
        let store = Store::open_in_memory().unwrap();
        let mut ids = Vec::new();
        for name in ["A", "B", "C"] {
            let url = format!("https://{name}/feed");
            ids.push(
                store
                    .add_source(SourceKind::Rss, name, Category::TechJp, &url)
                    .unwrap(),
            );
        }
        let now = at("2026-10-06T00:00:00Z");
        let a = store.get_source(ids[0]).unwrap().unwrap();
        let b = store.get_source(ids[1]).unwrap().unwrap();
        store
            .upsert_items(
                &a,
                &[
                    new_item("https://a/1", "a1", "", None),
                    new_item("https://a/2", "a2", "", None),
                ],
                &now,
            )
            .unwrap();
        store
            .upsert_items(&b, &[new_item("https://b/1", "b1", "", None)], &now)
            .unwrap();

        let counts = store.item_counts_by_source().unwrap();
        assert_eq!(counts.get(&ids[0]), Some(&2));
        assert_eq!(counts.get(&ids[1]), Some(&1));
        assert!(!counts.contains_key(&ids[2]), "no items, so absent");
        assert_eq!(counts.len(), 2);

        // 削除されたソースの記事 (source_id が NULL) は数えない。
        store.delete_source(ids[0]).unwrap();
        let counts = store.item_counts_by_source().unwrap();
        assert_eq!(counts.len(), 1);
        assert_eq!(counts.get(&ids[1]), Some(&1));
    }
}
