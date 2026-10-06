use rusqlite::{Connection, OptionalExtension, Row, params};

use super::{Store, conv_err};
use crate::error::Result;
use crate::model::{DigestScope, Topic};

fn row_to_topic(r: &Row) -> rusqlite::Result<Topic> {
    let raw: String = r.get(2)?;
    Ok(Topic {
        id: r.get(0)?,
        name: r.get(1)?,
        keywords: serde_json::from_str(&raw).map_err(|e| conv_err(2, e.to_string()))?,
    })
}

fn clean_keywords(keywords: &[String]) -> Result<String> {
    let cleaned: Vec<&str> = keywords
        .iter()
        .map(|k| k.trim())
        .filter(|k| !k.is_empty())
        .collect();
    Ok(serde_json::to_string(&cleaned)?)
}

fn delete_topic_digests(conn: &Connection, id: i64) -> Result<()> {
    conn.execute(
        "DELETE FROM digests WHERE scope = ?1",
        params![DigestScope::Topic(id).key()],
    )?;
    Ok(())
}

impl Store {
    pub fn add_topic(&self, name: &str, keywords: &[String]) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO topics(name, keywords) VALUES (?1, ?2)",
            params![name.trim(), clean_keywords(keywords)?],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn list_topics(&self) -> Result<Vec<Topic>> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, name, keywords FROM topics ORDER BY id")?;
        let rows = stmt
            .query_map([], row_to_topic)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn get_topic(&self, id: i64) -> Result<Option<Topic>> {
        Ok(self
            .conn
            .query_row(
                "SELECT id, name, keywords FROM topics WHERE id = ?1",
                params![id],
                row_to_topic,
            )
            .optional()?)
    }

    pub fn find_topic_by_name(&self, name: &str) -> Result<Option<Topic>> {
        Ok(self
            .conn
            .query_row(
                "SELECT id, name, keywords FROM topics WHERE name = ?1",
                params![name.trim()],
                row_to_topic,
            )
            .optional()?)
    }

    /// キーワードが変わった場合は、古いキーワードで作ったこのトピックのダイジェストを消す。
    pub fn update_topic(&self, id: i64, name: &str, keywords: &[String]) -> Result<()> {
        let cleaned = clean_keywords(keywords)?;
        let tx = self.conn.unchecked_transaction()?;
        let stored: Option<String> = tx
            .query_row(
                "SELECT keywords FROM topics WHERE id = ?1",
                params![id],
                |r| r.get(0),
            )
            .optional()?;
        tx.execute(
            "UPDATE topics SET name = ?2, keywords = ?3 WHERE id = ?1",
            params![id, name.trim(), cleaned],
        )?;
        if stored.is_some_and(|s| s != cleaned) {
            delete_topic_digests(&tx, id)?;
        }
        tx.commit()?;
        Ok(())
    }

    /// トピックと、そのトピックのダイジェストを消す。
    pub fn delete_topic(&self, id: i64) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        tx.execute("DELETE FROM topics WHERE id = ?1", params![id])?;
        delete_topic_digests(&tx, id)?;
        tx.commit()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Period;
    use crate::store::NewDigest;
    use chrono::NaiveDate;

    fn kws(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn topic_crud() {
        let store = Store::open_in_memory().unwrap();
        let id = store
            .add_topic("認知症", &kws(&["認知症", "dementia", "Alzheimer"]))
            .unwrap();
        let t = store.get_topic(id).unwrap().unwrap();
        assert_eq!(t.name, "認知症");
        assert_eq!(t.keywords, kws(&["認知症", "dementia", "Alzheimer"]));
        assert_eq!(store.find_topic_by_name("認知症").unwrap().unwrap().id, id);
        assert!(store.find_topic_by_name("なし").unwrap().is_none());

        store
            .update_topic(id, "認知症ケア", &kws(&["介護"]))
            .unwrap();
        let all = store.list_topics().unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].name, "認知症ケア");
        assert_eq!(all[0].keywords, kws(&["介護"]));

        store.delete_topic(id).unwrap();
        assert!(store.get_topic(id).unwrap().is_none());
    }

    #[test]
    fn deleted_topic_id_is_not_reused() {
        let store = Store::open_in_memory().unwrap();
        store.add_topic("a", &kws(&["x"])).unwrap();
        let b = store.add_topic("b", &kws(&["y"])).unwrap();
        store.delete_topic(b).unwrap();
        let c = store.add_topic("c", &kws(&["z"])).unwrap();
        assert_ne!(c, b);
    }

    fn save_topic_digest(store: &Store, topic: i64) {
        let d = NewDigest {
            period: Period::Day,
            period_start: NaiveDate::from_ymd_opt(2026, 10, 5).unwrap(),
            scope: DigestScope::Topic(topic),
            body: "body".into(),
            item_ids: vec![],
            provider: "mock".into(),
        };
        store
            .save_digest_done(&d, &crate::testutil::at("2026-10-06T00:00:00Z"))
            .unwrap();
    }

    fn has_topic_digest(store: &Store, topic: i64) -> bool {
        store
            .get_digest(
                Period::Day,
                NaiveDate::from_ymd_opt(2026, 10, 5).unwrap(),
                &DigestScope::Topic(topic),
            )
            .unwrap()
            .is_some()
    }

    #[test]
    fn topic_digests_follow_topic_lifecycle() {
        let store = Store::open_in_memory().unwrap();
        let id = store.add_topic("t", &kws(&["AI", "LLM"])).unwrap();
        let other = store.add_topic("o", &kws(&["Rust"])).unwrap();
        save_topic_digest(&store, id);
        save_topic_digest(&store, other);

        // 名前だけの変更 (キーワードの空白の違いを含む) ではダイジェストを残す。
        store
            .update_topic(id, "renamed", &kws(&[" AI", "LLM "]))
            .unwrap();
        assert!(has_topic_digest(&store, id));

        // キーワードが変わると古いダイジェストは別物なので消す。
        store.update_topic(id, "renamed", &kws(&["AI"])).unwrap();
        assert!(!has_topic_digest(&store, id));

        save_topic_digest(&store, id);
        store.delete_topic(id).unwrap();
        assert!(!has_topic_digest(&store, id));
        assert!(has_topic_digest(&store, other));
    }

    #[test]
    fn keywords_are_trimmed_and_blank_ones_dropped() {
        let store = Store::open_in_memory().unwrap();
        let id = store.add_topic("t", &kws(&["  AI ", "", "   "])).unwrap();
        assert_eq!(store.get_topic(id).unwrap().unwrap().keywords, kws(&["AI"]));
    }
}
