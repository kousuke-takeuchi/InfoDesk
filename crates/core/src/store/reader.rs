//! 読むだけのモード (サーバーが書き、この Mac は読む) のための DB 操作。

use rusqlite::{Connection, OptionalExtension, params};

use super::Store;
use crate::error::Result;
use crate::sync::mode::{MODE_READER, MODE_WRITER, SETTING_MODE};
use crate::sync::nas::SETTING_ENABLED;

/// 呼び出し側のトランザクションの中で設定を書く。
pub(super) fn put_setting(conn: &Connection, key: &str, value: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO settings(key, value) VALUES (?1, ?2) \
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![key, value],
    )?;
    Ok(())
}

fn mode_in(conn: &Connection) -> Result<Option<String>> {
    Ok(conn
        .query_row(
            "SELECT value FROM settings WHERE key = ?1",
            params![SETTING_MODE],
            |r| r.get(0),
        )
        .optional()?)
}

impl Store {
    /// 読むだけのモードにする。1 つのトランザクションで、新しい端末 id を作り (写した DB の id は
    /// サーバーのものなので、そのままだとこの Mac の ops ファイルをサーバーが自分のものとして飛ばす)、
    /// 送っていない操作をすべて捨て、生成済みでないダイジェストを消す (読むだけでは再試行しない)。
    /// もう読むだけなら何もしない。
    pub fn switch_to_reader(&self) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        if mode_in(&tx)?.as_deref() == Some(MODE_READER) {
            return Ok(());
        }
        put_setting(&tx, SETTING_MODE, MODE_READER)?;
        put_setting(&tx, "sync.device_id", &uuid::Uuid::new_v4().to_string())?;
        tx.execute("DELETE FROM sync_ops", [])?;
        tx.execute("DELETE FROM digests WHERE status != 'done'", [])?;
        tx.commit()?;
        Ok(())
    }

    /// 書き手に戻す。同期も同時にオフにする (サーバーと 2 台で書かないよう、オンは手で戻す)。
    /// 読むだけでなければ何もしない。
    pub fn switch_to_writer(&self) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        if mode_in(&tx)?.as_deref() != Some(MODE_READER) {
            return Ok(());
        }
        put_setting(&tx, SETTING_MODE, MODE_WRITER)?;
        put_setting(&tx, SETTING_ENABLED, "false")?;
        tx.commit()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::model::{Category, DigestScope, ItemFilter, Period};
    use crate::store::{NewDigest, Store};
    use crate::sync::mode::{MODE_READER, MODE_WRITER, SETTING_MODE, is_reader};
    use crate::testutil::{at, new_item, store_with_source};
    use chrono::NaiveDate;

    /// 同期が有効で、端末 id がサーバーと同じ (写した DB) ストア。印を1つ付け、送っていない操作が1行ある。
    fn copied_store() -> Store {
        let s = store_with_source(Category::TechJp).0;
        s.set_setting("sync.enabled", "true").unwrap();
        s.set_setting("sync.device_id", "server-and-old-mac")
            .unwrap();
        let src = s.list_sources().unwrap().remove(0);
        s.upsert_items(
            &src,
            &[new_item("https://a/1", "t", "", None)],
            &at("2026-10-13T00:00:00Z"),
        )
        .unwrap();
        let id = s.query_items(&ItemFilter::default()).unwrap()[0].id;
        s.set_read(id, true).unwrap();
        s
    }

    fn d(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    #[test]
    fn switch_to_reader_gives_a_new_device_and_drops_journal_and_unfinished_digests() {
        let s = copied_store();
        assert_eq!(s.pending_local_ops("x").unwrap().len(), 1);
        let now = at("2026-10-13T00:00:00Z");
        s.save_digest_done(
            &NewDigest {
                period: Period::Day,
                period_start: d("2026-10-12"),
                scope: DigestScope::Category(Category::Github),
                body: "done".into(),
                item_ids: vec![],
                provider: "p".into(),
            },
            &now,
        )
        .unwrap();
        s.mark_digest_attempt_failed(
            Period::Day,
            d("2026-10-12"),
            &DigestScope::Category(Category::Arxiv),
            "down",
            &now,
        )
        .unwrap();

        s.switch_to_reader().unwrap();

        assert!(is_reader(&s).unwrap());
        let device = s.get_setting("sync.device_id").unwrap().unwrap();
        assert_ne!(device, "server-and-old-mac");
        assert_eq!(device.len(), 36, "uuid v4: {device}");
        assert!(s.pending_local_ops("x").unwrap().is_empty());
        let digests = s.list_digests(Period::Day, d("2026-10-12")).unwrap();
        assert_eq!(digests.len(), 1, "only the done digest stays");
        assert_eq!(digests[0].body, "done");
        // 印そのもの (記事の列) は残る。
        assert!(s.query_items(&ItemFilter::default()).unwrap()[0].read);
    }

    #[test]
    fn a_second_switch_to_reader_keeps_the_device_id() {
        let s = copied_store();
        s.switch_to_reader().unwrap();
        let first = s.get_setting("sync.device_id").unwrap();
        s.switch_to_reader().unwrap();
        assert_eq!(s.get_setting("sync.device_id").unwrap(), first);
    }

    #[test]
    fn switch_to_writer_turns_sync_off_only_when_leaving_reader() {
        let s = copied_store();
        s.switch_to_writer().unwrap();
        assert_eq!(
            s.get_setting("sync.enabled").unwrap().as_deref(),
            Some("true"),
            "already a writer"
        );
        s.switch_to_reader().unwrap();
        s.switch_to_writer().unwrap();
        assert_eq!(
            s.get_setting(SETTING_MODE).unwrap().as_deref(),
            Some(MODE_WRITER)
        );
        assert_eq!(
            s.get_setting("sync.enabled").unwrap().as_deref(),
            Some("false")
        );
        assert_ne!(
            s.get_setting(SETTING_MODE).unwrap().as_deref(),
            Some(MODE_READER)
        );
    }
}
