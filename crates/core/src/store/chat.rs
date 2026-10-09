use chrono::{DateTime, Utc};
use rusqlite::{OptionalExtension, params};
use serde_json::Value;

use super::{Store, col_ts, ts};
use crate::error::{Error, Result};
use crate::model::ChatThread;

fn row_to_thread(r: &rusqlite::Row) -> rusqlite::Result<ChatThread> {
    Ok(ChatThread {
        id: r.get(0)?,
        title: r.get(1)?,
        created_at: col_ts(r, 2)?,
        updated_at: col_ts(r, 3)?,
    })
}

impl Store {
    pub fn create_chat_thread(&self, title: &str, now: &DateTime<Utc>) -> Result<ChatThread> {
        self.conn.execute(
            "INSERT INTO chat_threads(title, created_at, updated_at) VALUES (?1, ?2, ?2)",
            params![title.trim(), ts(now)],
        )?;
        let id = self.conn.last_insert_rowid();
        Ok(ChatThread {
            id,
            title: title.trim().to_string(),
            created_at: *now,
            updated_at: *now,
        })
    }

    pub fn list_chat_threads(&self) -> Result<Vec<ChatThread>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, title, created_at, updated_at FROM chat_threads ORDER BY updated_at DESC, id DESC",
        )?;
        let rows = stmt.query_map([], row_to_thread)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn rename_chat_thread(&self, id: i64, title: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE chat_threads SET title = ?2 WHERE id = ?1",
            params![id, title.trim()],
        )?;
        Ok(())
    }

    pub fn delete_chat_thread(&self, id: i64) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        tx.execute(
            "DELETE FROM chat_messages WHERE thread_id = ?1",
            params![id],
        )?;
        tx.execute("DELETE FROM chat_threads WHERE id = ?1", params![id])?;
        tx.commit()?;
        Ok(())
    }

    pub fn get_chat_messages(&self, thread_id: i64) -> Result<Vec<Value>> {
        let mut stmt = self
            .conn
            .prepare("SELECT content FROM chat_messages WHERE thread_id = ?1 ORDER BY seq")?;
        let rows = stmt.query_map(params![thread_id], |r| r.get::<_, String>(0))?;
        let mut out = Vec::new();
        for s in rows {
            let s = s?;
            out.push(
                serde_json::from_str(&s)
                    .map_err(|e| Error::Invalid(format!("stored chat message is not JSON: {e}")))?,
            );
        }
        Ok(out)
    }

    pub fn save_chat_messages(
        &self,
        thread_id: i64,
        messages: &[Value],
        now: &DateTime<Utc>,
    ) -> Result<()> {
        let exists: Option<i64> = self
            .conn
            .query_row(
                "SELECT id FROM chat_threads WHERE id = ?1",
                params![thread_id],
                |r| r.get(0),
            )
            .optional()?;
        if exists.is_none() {
            return Err(Error::Invalid(format!("chat thread {thread_id} not found")));
        }
        let mut rows = Vec::with_capacity(messages.len());
        for m in messages {
            let (Some(id), Some(role)) = (
                m.get("id").and_then(Value::as_str),
                m.get("role").and_then(Value::as_str),
            ) else {
                return Err(Error::Invalid("message without id or role".into()));
            };
            rows.push((id.to_string(), role.to_string(), m.to_string()));
        }
        let tx = self.conn.unchecked_transaction()?;
        tx.execute(
            "DELETE FROM chat_messages WHERE thread_id = ?1",
            params![thread_id],
        )?;
        for (seq, (id, role, content)) in rows.iter().enumerate() {
            tx.execute(
                "INSERT INTO chat_messages(thread_id, seq, id, role, content, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![thread_id, seq as i64, id, role, content, ts(now)],
            )?;
        }
        tx.execute(
            "UPDATE chat_threads SET updated_at = ?2 WHERE id = ?1",
            params![thread_id, ts(now)],
        )?;
        tx.commit()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::store::Store;
    use chrono::{TimeZone, Utc};
    use serde_json::json;

    fn t(h: u32) -> chrono::DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 9, h, 0, 0).unwrap()
    }

    fn msg(id: &str, role: &str, text: &str) -> serde_json::Value {
        json!({ "id": id, "role": role, "parts": [{ "type": "text", "text": text }] })
    }

    #[test]
    fn creates_lists_newest_first_and_renames() {
        let s = Store::open_in_memory().unwrap();
        let a = s.create_chat_thread("古い", &t(1)).unwrap();
        let b = s.create_chat_thread("新しい", &t(2)).unwrap();
        assert_eq!(
            s.list_chat_threads()
                .unwrap()
                .iter()
                .map(|x| x.id)
                .collect::<Vec<_>>(),
            vec![b.id, a.id]
        );
        s.rename_chat_thread(a.id, "  名前を変えた  ").unwrap();
        assert_eq!(s.list_chat_threads().unwrap()[1].title, "名前を変えた");
    }

    #[test]
    fn save_replaces_messages_in_order_and_bumps_updated_at() {
        let s = Store::open_in_memory().unwrap();
        let a = s.create_chat_thread("A", &t(1)).unwrap();
        let b = s.create_chat_thread("B", &t(2)).unwrap();
        s.save_chat_messages(
            a.id,
            &[msg("m1", "user", "質問"), msg("m2", "assistant", "回答")],
            &t(3),
        )
        .unwrap();
        assert_eq!(
            s.get_chat_messages(a.id).unwrap()[1]["parts"][0]["text"],
            "回答"
        );
        // 置き換え: 2件 → 1件
        s.save_chat_messages(a.id, &[msg("m1", "user", "質問だけ")], &t(4))
            .unwrap();
        let got = s.get_chat_messages(a.id).unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0]["parts"][0]["text"], "質問だけ");
        // 更新したスレッドが先頭に来る
        assert_eq!(s.list_chat_threads().unwrap()[0].id, a.id);
        assert_eq!(s.list_chat_threads().unwrap()[0].updated_at, t(4));
        assert!(s.get_chat_messages(b.id).unwrap().is_empty());
    }

    #[test]
    fn delete_removes_thread_and_its_messages() {
        let s = Store::open_in_memory().unwrap();
        let a = s.create_chat_thread("A", &t(1)).unwrap();
        s.save_chat_messages(a.id, &[msg("m1", "user", "x")], &t(2))
            .unwrap();
        s.delete_chat_thread(a.id).unwrap();
        assert!(s.list_chat_threads().unwrap().is_empty());
        assert!(s.get_chat_messages(a.id).unwrap().is_empty());
        let err = s
            .save_chat_messages(a.id, &[], &t(3))
            .unwrap_err()
            .to_string();
        assert!(err.contains("chat thread"), "{err}");
    }

    #[test]
    fn rejects_messages_without_id_or_role() {
        let s = Store::open_in_memory().unwrap();
        let a = s.create_chat_thread("A", &t(1)).unwrap();
        let err = s
            .save_chat_messages(a.id, &[json!({ "parts": [] })], &t(2))
            .unwrap_err()
            .to_string();
        assert!(err.contains("message without id or role"), "{err}");
        assert!(
            s.get_chat_messages(a.id).unwrap().is_empty(),
            "失敗したら何も書かない"
        );
    }
}
