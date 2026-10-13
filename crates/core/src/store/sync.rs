use std::collections::{BTreeMap, HashSet};

use chrono::{DateTime, FixedOffset, NaiveDate, TimeZone, Utc};
use rusqlite::types::Value;
use rusqlite::{Connection, OptionalExtension, Row, params, params_from_iter};

use super::items::keyword_clause;
use super::{Store, ts};
use crate::error::{Error, Result};
use crate::sync::format::{Field, ItemRecord, ItemState, Op, Stamp, normalize_op, op_time, wins};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncFileRow {
    pub name: String,
    pub kind: String,
    pub drive_id: Option<String>,
    pub sha256: Option<String>,
    pub updated_at: String,
}

fn field_name(f: Field) -> &'static str {
    match f {
        Field::Read => "read",
        Field::Starred => "starred",
    }
}

fn parse_field(s: &str) -> Option<Field> {
    match s {
        "read" => Some(Field::Read),
        "starred" => Some(Field::Starred),
        _ => None,
    }
}

fn row_to_sync_file(r: &Row) -> rusqlite::Result<SyncFileRow> {
    Ok(SyncFileRow {
        name: r.get(0)?,
        kind: r.get(1)?,
        drive_id: r.get(2)?,
        sha256: r.get(3)?,
        updated_at: r.get(4)?,
    })
}

/// tz の日付 `d` の [始まり, 終わり) を UTC で返す。
fn day_bounds(d: NaiveDate, tz: FixedOffset) -> Result<(DateTime<Utc>, DateTime<Utc>)> {
    let local = |d: NaiveDate| {
        tz.from_local_datetime(&d.and_hms_opt(0, 0, 0).expect("midnight"))
            .single()
            .map(|t| t.with_timezone(&Utc))
            .ok_or_else(|| Error::Invalid(format!("invalid local date {d}")))
    };
    let next = d
        .succ_opt()
        .ok_or_else(|| Error::Invalid(format!("date out of range {d}")))?;
    Ok((local(d)?, local(next)?))
}

fn changed_by_col(field: Field) -> &'static str {
    match field {
        Field::Read => "read_changed_by",
        Field::Starred => "starred_changed_by",
    }
}

/// `sync.device_id` を (なければ作って) 返す。
fn device_id_in(conn: &Connection) -> Result<String> {
    let cur: Option<String> = conn
        .query_row(
            "SELECT value FROM settings WHERE key = 'sync.device_id'",
            [],
            |r| r.get(0),
        )
        .optional()?;
    match cur {
        Some(v) if !v.is_empty() => Ok(v),
        _ => {
            let v = uuid::Uuid::new_v4().to_string();
            conn.execute(
                "INSERT INTO settings(key, value) VALUES ('sync.device_id', ?1) \
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                params![v],
            )?;
            Ok(v)
        }
    }
}

/// 呼び出し側のトランザクションの中で使う `set_flag`。
pub(super) fn set_flag_in(conn: &Connection, id: i64, field: Field, value: bool) -> Result<()> {
    let (col, at_col) = match field {
        Field::Read => ("read", "read_changed_at"),
        Field::Starred => ("starred", "starred_changed_at"),
    };
    let current: Option<bool> = conn
        .query_row(
            &format!("SELECT {col} FROM items WHERE id = ?1"),
            params![id],
            |r| r.get(0),
        )
        .optional()?;
    if current.is_none_or(|c| c == value) {
        return Ok(());
    }
    let now = op_time(Utc::now());
    let device = device_id_in(conn)?;
    let by_col = changed_by_col(field);
    conn.execute(
        &format!("UPDATE items SET {col} = ?2, {at_col} = ?3, {by_col} = ?4 WHERE id = ?1"),
        params![id, value, now, device],
    )?;
    conn.execute(
        "INSERT INTO sync_ops(item_id, field, value, at) VALUES (?1, ?2, ?3, ?4)",
        params![id, field_name(field), value, now],
    )?;
    Ok(())
}

impl Store {
    /// 値が変わったときだけ、列と `*_changed_at` を更新し、`sync_ops` に1行足す。
    pub(super) fn set_flag(&self, id: i64, field: Field, value: bool) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        set_flag_in(&tx, id, field, value)?;
        tx.commit()?;
        Ok(())
    }

    /// `sync.device_id` と `sync.db_id` を (なければ作って) 返す。
    pub fn ensure_sync_ids(&self) -> Result<(String, String)> {
        let mut out = Vec::new();
        for key in ["sync.device_id", "sync.db_id"] {
            let v = match self.get_setting(key)? {
                Some(v) if !v.is_empty() => v,
                _ => {
                    let v = uuid::Uuid::new_v4().to_string();
                    self.set_setting(key, &v)?;
                    v
                }
            };
            out.push(v);
        }
        let db = out.pop().expect("two ids");
        let device = out.pop().expect("two ids");
        Ok((device, db))
    }

    pub fn pending_local_ops(&self, device: &str) -> Result<Vec<(i64, Op)>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, item_id, field, value, at FROM sync_ops WHERE uploaded = 0 ORDER BY id",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, bool>(3)?,
                r.get::<_, String>(4)?,
            ))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (id, item_id, field, value, at) = row?;
            let field = parse_field(&field)
                .ok_or_else(|| Error::Invalid(format!("unknown sync field {field}")))?;
            out.push((
                id,
                Op {
                    item_id,
                    field,
                    value,
                    at,
                    device: device.to_string(),
                },
            ));
        }
        Ok(out)
    }

    pub fn mark_ops_uploaded(&self, ids: &[i64]) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        for id in ids {
            tx.execute(
                "UPDATE sync_ops SET uploaded = 1 WHERE id = ?1",
                params![id],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    /// 他の端末の操作を取り込む。`true` は印 (時刻と端末) が動いたことを表し、値が変わらないこともある。
    /// 記事がない、不正な操作、または負けたときは false。`sync_ops` には記録しない。
    pub fn apply_remote_op(&self, op: &Op, local_device: &str) -> Result<bool> {
        let Some(op) = normalize_op(op) else {
            return Ok(false);
        };
        let (col, at_col) = match op.field {
            Field::Read => ("read", "read_changed_at"),
            Field::Starred => ("starred", "starred_changed_at"),
        };
        let by_col = changed_by_col(op.field);
        let current: Option<(bool, Option<String>, Option<String>)> = self
            .conn
            .query_row(
                &format!("SELECT {col}, {at_col}, {by_col} FROM items WHERE id = ?1"),
                params![op.item_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?;
        let Some((value, changed_at, changed_by)) = current else {
            return Ok(false);
        };
        let current = Stamp {
            value,
            at: changed_at.unwrap_or_default(),
            device: changed_by.unwrap_or_else(|| local_device.to_string()),
        };
        let incoming = Stamp {
            value: op.value,
            at: op.at.clone(),
            device: op.device.clone(),
        };
        if !wins(&incoming, &current) {
            return Ok(false);
        }
        self.conn.execute(
            &format!("UPDATE items SET {col} = ?2, {at_col} = ?3, {by_col} = ?4 WHERE id = ?1"),
            params![op.item_id, op.value, op.at, op.device],
        )?;
        Ok(true)
    }

    /// 送信済みで `older_than` より前の `sync_ops` を消し、消した行数を返す。
    pub fn prune_uploaded_ops(&self, older_than: &str) -> Result<usize> {
        Ok(self.conn.execute(
            "DELETE FROM sync_ops WHERE uploaded = 1 AND at < ?1",
            params![older_than],
        )?)
    }

    /// 操作の時刻が付いている記事の状態。`item_ids` が None なら全件。
    /// 端末が記録されていない印には `device` を使う。
    pub fn local_state_stamps(
        &self,
        item_ids: Option<&[i64]>,
        device: &str,
    ) -> Result<BTreeMap<i64, ItemState>> {
        let filter: Option<HashSet<i64>> = item_ids.map(|ids| ids.iter().copied().collect());
        let mut stmt = self.conn.prepare(
            "SELECT id, read, read_changed_at, read_changed_by, starred, starred_changed_at, \
             starred_changed_by FROM items \
             WHERE read_changed_at IS NOT NULL OR starred_changed_at IS NOT NULL",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, bool>(1)?,
                r.get::<_, Option<String>>(2)?,
                r.get::<_, Option<String>>(3)?,
                r.get::<_, bool>(4)?,
                r.get::<_, Option<String>>(5)?,
                r.get::<_, Option<String>>(6)?,
            ))
        })?;
        let mut out = BTreeMap::new();
        for row in rows {
            let (id, read, read_at, read_by, starred, starred_at, starred_by) = row?;
            if filter.as_ref().is_some_and(|ids| !ids.contains(&id)) {
                continue;
            }
            let stamp = |value, at: Option<String>, by: Option<String>| {
                at.map(|at| Stamp {
                    value,
                    at,
                    device: by.unwrap_or_else(|| device.to_string()),
                })
            };
            out.insert(
                id,
                ItemState {
                    read: stamp(read, read_at, read_by),
                    starred: stamp(starred, starred_at, starred_by),
                },
            );
        }
        Ok(out)
    }

    /// tz の日付 `date` に取得した記事を、取得時刻の昇順で返す。
    pub fn items_fetched_on(&self, date: NaiveDate, tz: FixedOffset) -> Result<Vec<ItemRecord>> {
        let (start, end) = day_bounds(date, tz)?;
        let mut stmt = self.conn.prepare(
            "SELECT id, url, title, excerpt, ai_summary, image_url, source_id, source_name, \
             category, score, published_at, fetched_at FROM items \
             WHERE fetched_at >= ?1 AND fetched_at < ?2 ORDER BY fetched_at, id",
        )?;
        let mut records = stmt
            .query_map(params![ts(&start), ts(&end)], |r| {
                Ok(ItemRecord {
                    id: r.get(0)?,
                    url: r.get(1)?,
                    title: r.get(2)?,
                    excerpt: r.get(3)?,
                    ai_summary: r.get(4)?,
                    image_url: r.get(5)?,
                    source_id: r.get(6)?,
                    source_name: r.get(7)?,
                    category: r.get(8)?,
                    score: r.get(9)?,
                    published_at: r.get(10)?,
                    fetched_at: r.get(11)?,
                    topic_ids: Vec::new(),
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        for topic in self.list_topics()? {
            let mut args = vec![Value::Text(ts(&start)), Value::Text(ts(&end))];
            let clause = keyword_clause(&topic.keywords, &mut args);
            let sql = format!(
                "SELECT items.id FROM items WHERE items.fetched_at >= ? AND items.fetched_at < ? AND {clause}"
            );
            let mut stmt = self.conn.prepare(&sql)?;
            let ids = stmt
                .query_map(params_from_iter(args), |r| r.get::<_, i64>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            for rec in records.iter_mut().filter(|r| ids.contains(&r.id)) {
                rec.topic_ids.push(topic.id);
            }
        }
        Ok(records)
    }

    /// `since` (tz の日付) 以降で、記事を取得した日付を昇順で返す。
    pub fn days_with_items(&self, since: NaiveDate, tz: FixedOffset) -> Result<Vec<NaiveDate>> {
        let (start, _) = day_bounds(since, tz)?;
        let mut stmt = self
            .conn
            .prepare("SELECT DISTINCT fetched_at FROM items WHERE fetched_at >= ?1")?;
        let rows = stmt
            .query_map(params![ts(&start)], |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let mut days = std::collections::BTreeSet::new();
        for s in rows {
            let t = DateTime::parse_from_rfc3339(&s)
                .map_err(|e| Error::Invalid(format!("bad fetched_at {s}: {e}")))?;
            days.insert(t.with_timezone(&tz).date_naive());
        }
        Ok(days.into_iter().collect())
    }

    pub fn sync_file(&self, name: &str) -> Result<Option<SyncFileRow>> {
        Ok(self
            .conn
            .query_row(
                "SELECT name, kind, drive_id, sha256, updated_at FROM sync_files WHERE name = ?1",
                params![name],
                row_to_sync_file,
            )
            .optional()?)
    }

    pub fn put_sync_file(&self, row: &SyncFileRow) -> Result<()> {
        self.conn.execute(
            "INSERT INTO sync_files(name, kind, drive_id, sha256, updated_at) \
             VALUES (?1, ?2, ?3, ?4, ?5) \
             ON CONFLICT(name) DO UPDATE SET kind = excluded.kind, \
             drive_id = excluded.drive_id, sha256 = excluded.sha256, \
             updated_at = excluded.updated_at",
            params![row.name, row.kind, row.drive_id, row.sha256, row.updated_at],
        )?;
        Ok(())
    }

    pub fn delete_sync_file(&self, name: &str) -> Result<()> {
        self.conn
            .execute("DELETE FROM sync_files WHERE name = ?1", params![name])?;
        Ok(())
    }

    pub fn sync_files(&self, kind: &str) -> Result<Vec<SyncFileRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT name, kind, drive_id, sha256, updated_at FROM sync_files \
             WHERE kind = ?1 ORDER BY name",
        )?;
        let rows = stmt.query_map(params![kind], row_to_sync_file)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }
}

#[cfg(test)]
mod tests {
    use chrono::{FixedOffset, NaiveDate};

    use crate::model::{Category, ItemFilter};
    use crate::store::Store;
    use crate::store::sync::SyncFileRow;
    use crate::sync::format::{Field, Op};
    use crate::testutil::{at, new_item, store_with_source};

    fn insert_item(s: &Store, url: &str) -> i64 {
        insert_at(s, url, "t", "2026-10-13T00:00:00Z")
    }

    fn insert_at(s: &Store, url: &str, title: &str, fetched: &str) -> i64 {
        let src = s.list_sources().unwrap().remove(0);
        s.upsert_items(&src, &[new_item(url, title, "", None)], &at(fetched))
            .unwrap();
        s.query_items(&ItemFilter::default())
            .unwrap()
            .into_iter()
            .find(|i| i.url == url)
            .unwrap()
            .id
    }

    fn store() -> Store {
        store_with_source(Category::TechJp).0
    }

    #[test]
    fn local_changes_are_journaled_once() {
        let s = store();
        let id = insert_item(&s, "https://a/1");
        s.set_read(id, true).unwrap();
        s.set_read(id, true).unwrap();
        s.set_starred(id, true).unwrap();
        let ops = s.pending_local_ops("mac").unwrap();
        assert_eq!(ops.len(), 2);
        assert_eq!(ops[0].1.field, Field::Read);
        assert_eq!(ops[0].1.device, "mac");
        assert_eq!(ops[1].1.field, Field::Starred);
        s.mark_ops_uploaded(&ops.iter().map(|(i, _)| *i).collect::<Vec<_>>())
            .unwrap();
        assert!(s.pending_local_ops("mac").unwrap().is_empty());
    }

    #[test]
    fn mark_all_read_journals_each_item() {
        let s = store();
        insert_item(&s, "https://a/1");
        insert_item(&s, "https://a/2");
        assert_eq!(s.mark_all_read(&ItemFilter::default()).unwrap(), 2);
        assert_eq!(s.pending_local_ops("mac").unwrap().len(), 2);
    }

    #[test]
    fn remote_op_applies_only_when_newer_and_is_not_journaled() {
        let s = store();
        let id = insert_item(&s, "https://a/1");
        s.set_read(id, true).unwrap();
        let old = Op {
            item_id: id,
            field: Field::Read,
            value: false,
            at: "2000-01-01T00:00:00.000Z".into(),
            device: "phone".into(),
        };
        assert!(!s.apply_remote_op(&old, "mac").unwrap());
        assert!(s.get_item(id).unwrap().unwrap().read);
        let new = Op {
            at: "2999-01-01T00:00:00.000Z".into(),
            ..old.clone()
        };
        assert!(s.apply_remote_op(&new, "mac").unwrap());
        assert!(!s.get_item(id).unwrap().unwrap().read);
        assert_eq!(s.pending_local_ops("mac").unwrap().len(), 1);
    }

    #[test]
    fn remote_op_applies_when_local_stamp_is_null() {
        let s = store();
        let id = insert_item(&s, "https://a/1");
        let op = Op {
            item_id: id,
            field: Field::Starred,
            value: true,
            at: "2000-01-01T00:00:00.000Z".into(),
            device: "phone".into(),
        };
        assert!(s.apply_remote_op(&op, "mac").unwrap());
        assert!(s.get_item(id).unwrap().unwrap().starred);
        let stamps = s.local_state_stamps(None, "mac").unwrap();
        assert_eq!(stamps[&id].starred.as_ref().unwrap().at, op.at);
        assert!(stamps[&id].read.is_none());
        assert!(
            s.local_state_stamps(Some(&[id + 100]), "mac")
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn remote_op_for_unknown_item_is_ignored() {
        let s = store();
        let op = Op {
            item_id: 999,
            field: Field::Starred,
            value: true,
            at: "2026-10-13T00:00:00.000Z".into(),
            device: "phone".into(),
        };
        assert!(!s.apply_remote_op(&op, "mac").unwrap());
    }

    #[test]
    fn remote_op_is_normalized_before_storing() {
        let s = store();
        let id = insert_item(&s, "https://a/1");
        let op = Op {
            item_id: id,
            field: Field::Read,
            value: true,
            at: "2999-01-01T09:00:00+09:00".into(),
            device: "phone".into(),
        };
        assert!(s.apply_remote_op(&op, "mac").unwrap());
        let stamps = s.local_state_stamps(None, "mac").unwrap();
        assert_eq!(
            stamps[&id].read.as_ref().unwrap().at,
            "2999-01-01T00:00:00.000Z"
        );
    }

    #[test]
    fn remote_op_with_empty_device_is_rejected() {
        let s = store();
        let id = insert_item(&s, "https://a/1");
        let op = Op {
            item_id: id,
            field: Field::Read,
            value: true,
            at: "2999-01-01T00:00:00.000Z".into(),
            device: String::new(),
        };
        assert!(!s.apply_remote_op(&op, "mac").unwrap());
        assert!(!s.get_item(id).unwrap().unwrap().read);
    }

    #[test]
    fn changed_by_tracks_the_device_of_each_change() {
        let s = store();
        let id = insert_item(&s, "https://a/1");
        let (me, _) = s.ensure_sync_ids().unwrap();
        s.set_read(id, true).unwrap();
        let op = Op {
            item_id: id,
            field: Field::Starred,
            value: true,
            at: "2999-01-01T00:00:00.000Z".into(),
            device: "phone".into(),
        };
        assert!(s.apply_remote_op(&op, &me).unwrap());
        let st = s.local_state_stamps(None, "fallback").unwrap();
        assert_eq!(st[&id].read.as_ref().unwrap().device, me);
        assert_eq!(st[&id].starred.as_ref().unwrap().device, "phone");
    }

    #[test]
    fn stamps_fall_back_to_given_device_when_unrecorded() {
        let s = store();
        let id = insert_item(&s, "https://a/1");
        s.conn
            .execute(
                "UPDATE items SET read_changed_at = '2000-01-01T00:00:00.000Z' WHERE id = ?1",
                [id],
            )
            .unwrap();
        let st = s.local_state_stamps(None, "fallback").unwrap();
        assert_eq!(st[&id].read.as_ref().unwrap().device, "fallback");
    }

    #[test]
    fn prune_uploaded_ops_removes_only_old_uploaded_rows() {
        let s = store();
        let id = insert_item(&s, "https://a/1");
        s.set_read(id, true).unwrap();
        s.set_starred(id, true).unwrap();
        let ops = s.pending_local_ops("mac").unwrap();
        s.mark_ops_uploaded(&[ops[0].0]).unwrap();
        assert_eq!(s.prune_uploaded_ops("2000-01-01T00:00:00.000Z").unwrap(), 0);
        assert_eq!(s.prune_uploaded_ops("2999-01-01T00:00:00.000Z").unwrap(), 1);
        assert_eq!(s.pending_local_ops("mac").unwrap().len(), 1);
    }

    #[test]
    fn sync_ids_are_created_once() {
        let s = store();
        let a = s.ensure_sync_ids().unwrap();
        let b = s.ensure_sync_ids().unwrap();
        assert_eq!(a, b);
        assert_ne!(a.0, a.1);
        for id in [&a.0, &a.1] {
            assert_eq!(uuid::Uuid::parse_str(id).unwrap().get_version_num(), 4);
        }
    }

    #[test]
    fn items_fetched_on_uses_the_local_day_and_topic_matches() {
        let s = store();
        let tz = FixedOffset::east_opt(9 * 3600).unwrap();
        let topic = s.add_topic("認知症", &["認知症".to_string()]).unwrap();
        let _other = s.add_topic("Rust", &["Rust".to_string()]).unwrap();
        // UTC 10-12 14:30 は JST 10-12 23:30、UTC 10-12 15:30 は JST 10-13 00:30。
        insert_at(&s, "https://a/1", "ふつう", "2026-10-12T14:30:00Z");
        let id = insert_at(&s, "https://a/2", "認知症の新薬", "2026-10-12T15:30:00Z");
        let day = NaiveDate::from_ymd_opt(2026, 10, 13).unwrap();
        let recs = s.items_fetched_on(day, tz).unwrap();
        assert_eq!(recs.len(), 1);
        assert_eq!(recs[0].id, id);
        assert_eq!(recs[0].topic_ids, vec![topic]);
        assert_eq!(recs[0].fetched_at, "2026-10-12T15:30:00Z");
        assert_eq!(recs[0].category, "tech_jp");

        let days = s
            .days_with_items(NaiveDate::from_ymd_opt(2026, 10, 1).unwrap(), tz)
            .unwrap();
        assert_eq!(
            days,
            vec![
                NaiveDate::from_ymd_opt(2026, 10, 12).unwrap(),
                NaiveDate::from_ymd_opt(2026, 10, 13).unwrap()
            ]
        );
    }

    #[test]
    fn sync_file_rows_round_trip() {
        let s = store();
        assert!(s.sync_file("a").unwrap().is_none());
        let row = SyncFileRow {
            name: "a".into(),
            kind: "out".into(),
            drive_id: Some("d1".into()),
            sha256: None,
            updated_at: "2026-10-13T00:00:00Z".into(),
        };
        s.put_sync_file(&row).unwrap();
        let row2 = SyncFileRow {
            drive_id: Some("d2".into()),
            ..row.clone()
        };
        s.put_sync_file(&row2).unwrap();
        s.put_sync_file(&SyncFileRow {
            name: "b".into(),
            kind: "in".into(),
            ..row.clone()
        })
        .unwrap();
        assert_eq!(s.sync_file("a").unwrap(), Some(row2));
        assert_eq!(s.sync_files("out").unwrap().len(), 1);
        s.delete_sync_file("a").unwrap();
        assert!(s.sync_file("a").unwrap().is_none());
    }
}
