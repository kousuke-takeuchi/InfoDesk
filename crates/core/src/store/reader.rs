//! 読むだけのモード (サーバーが書き、この Mac は読む) のための DB 操作。

use std::collections::HashSet;

use chrono::{DateTime, FixedOffset, NaiveDate, Utc};
use rusqlite::{Connection, OptionalExtension, params};

use super::items::word_index_text;
use super::sync::day_bounds;
use super::{Store, ts};
use crate::error::{Error, Result};
use crate::model::{Category, DigestScope, Period};
use crate::sync::format::{DayFile, DigestRecord, ItemRecord};
use crate::sync::mode::{MODE_READER, MODE_WRITER, SETTING_MODE};
use crate::sync::nas::SETTING_ENABLED;

/// 日ファイルの取り込みの結果。`skipped` は取り込めなかった記事の説明 (画面の警告になる)。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DayImport {
    pub upserted: usize,
    pub removed: usize,
    pub skipped: Vec<String>,
}

/// ダイジェストファイルの取り込みの結果。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DigestImport {
    pub saved: usize,
    pub removed: usize,
    pub skipped: Vec<String>,
}

/// RFC 3339 の時刻を DB の形 (`ts`: 秒まで、UTC、末尾 Z) にする。読めなければ None。
fn norm_ts(s: &str) -> Option<String> {
    DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|d| ts(&d.with_timezone(&Utc)))
}

fn unindex_item(conn: &Connection, id: i64) -> Result<()> {
    conn.execute("DELETE FROM items_fts WHERE rowid = ?1", params![id])?;
    conn.execute("DELETE FROM items_words WHERE rowid = ?1", params![id])?;
    Ok(())
}

/// 記事を消す。検索の索引 (`items_fts`、`items_words`) からも消す。
fn delete_item(conn: &Connection, id: i64) -> Result<()> {
    unindex_item(conn, id)?;
    conn.execute("DELETE FROM items WHERE id = ?1", params![id])?;
    Ok(())
}

/// 1 件を id で入れるか更新する。既読、スター、その時刻と端末、`author` は変えない。
/// 同じ URL を別の id で持っていたら、先に消す (url は UNIQUE)。索引は作り直す。
fn upsert_record(conn: &Connection, r: &ItemRecord, fetched_at: &str) -> Result<()> {
    let others: Vec<i64> = {
        let mut stmt = conn.prepare("SELECT id FROM items WHERE url = ?1 AND id != ?2")?;
        stmt.query_map(params![r.url, r.id], |row| row.get(0))?
            .collect::<rusqlite::Result<_>>()?
    };
    for id in others {
        delete_item(conn, id)?;
    }
    let published_at = r.published_at.as_deref().and_then(norm_ts);
    conn.execute(
        "INSERT INTO items(id, source_id, source_name, category, url, title, excerpt, \
         published_at, fetched_at, score, ai_summary, image_url) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12) \
         ON CONFLICT(id) DO UPDATE SET source_id = excluded.source_id, \
         source_name = excluded.source_name, category = excluded.category, url = excluded.url, \
         title = excluded.title, excerpt = excluded.excerpt, published_at = excluded.published_at, \
         fetched_at = excluded.fetched_at, score = excluded.score, \
         ai_summary = excluded.ai_summary, image_url = excluded.image_url",
        params![
            r.id,
            r.source_id,
            r.source_name,
            r.category,
            r.url,
            r.title,
            r.excerpt,
            published_at,
            fetched_at,
            r.score,
            r.ai_summary,
            r.image_url,
        ],
    )?;
    unindex_item(conn, r.id)?;
    conn.execute(
        "INSERT INTO items_fts(rowid, title, excerpt) VALUES (?1, ?2, ?3)",
        params![r.id, r.title, r.excerpt],
    )?;
    conn.execute(
        "INSERT INTO items_words(rowid, title, excerpt) VALUES (?1, ?2, ?3)",
        params![r.id, word_index_text(&r.title), word_index_text(&r.excerpt)],
    )?;
    Ok(())
}

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
    /// 日ファイルを取り込む。1 つのトランザクションで、まず全ファイルの記事を id で入れるか更新し、
    /// `remove_missing` ならそのあと、各日 (`tz` の日付) に取得した記事のうちその日のファイルにない
    /// id を消す。全部入れてから消すので、日をまたいで移った記事 (redate) は消えず、印も残る。
    /// カテゴリーを知らない、取得時刻が読めない記事は入れずに `skipped` に書く (その id の手元の
    /// 記事は消さない)。公開時刻が読めなければ NULL にする。
    pub fn import_days(
        &self,
        days: &[DayFile],
        tz: FixedOffset,
        remove_missing: bool,
    ) -> Result<DayImport> {
        let tx = self.conn.unchecked_transaction()?;
        let mut out = DayImport::default();
        let mut keep: Vec<(NaiveDate, HashSet<i64>)> = Vec::new();
        for file in days {
            let date = NaiveDate::parse_from_str(&file.date, "%Y-%m-%d")
                .map_err(|e| Error::Invalid(format!("bad day file date {}: {e}", file.date)))?;
            let mut ids = HashSet::new();
            for r in &file.items {
                ids.insert(r.id);
                if Category::parse(&r.category).is_none() {
                    out.skipped.push(format!(
                        "{} の記事 {} を取り込めませんでした (カテゴリー {} を知りません)",
                        file.date, r.id, r.category
                    ));
                    continue;
                }
                let Some(fetched_at) = norm_ts(&r.fetched_at) else {
                    out.skipped.push(format!(
                        "{} の記事 {} を取り込めませんでした (取得時刻が読めません)",
                        file.date, r.id
                    ));
                    continue;
                };
                upsert_record(&tx, r, &fetched_at)?;
                out.upserted += 1;
            }
            keep.push((date, ids));
        }
        if remove_missing {
            for (date, ids) in &keep {
                let (start, end) = day_bounds(*date, tz)?;
                let local: Vec<i64> = {
                    let mut stmt = tx.prepare(
                        "SELECT id FROM items WHERE fetched_at >= ?1 AND fetched_at < ?2",
                    )?;
                    stmt.query_map(params![ts(&start), ts(&end)], |r| r.get(0))?
                        .collect::<rusqlite::Result<_>>()?
                };
                for id in local.into_iter().filter(|id| !ids.contains(id)) {
                    delete_item(&tx, id)?;
                    out.removed += 1;
                }
            }
        }
        tx.commit()?;
        Ok(out)
    }

    /// 1 期間のダイジェストファイルを取り込む。期間ごと置き換える: 各要約を `done` で入れるか更新し
    /// (`created_at` と `provider` はファイルのまま、再試行の回数とエラーは消す)、ファイルにない scope の
    /// 行は消す。scope が読めない、`done` でない、時刻が読めない要約は `skipped` に書く。
    pub fn import_digests(
        &self,
        period: Period,
        start: NaiveDate,
        entries: &[DigestRecord],
    ) -> Result<DigestImport> {
        let start_s = start.format("%Y-%m-%d").to_string();
        let tx = self.conn.unchecked_transaction()?;
        let mut out = DigestImport::default();
        let mut keys = HashSet::new();
        for e in entries {
            let scope = serde_json::from_value::<DigestScope>(e.scope.clone()).ok();
            let created_at = norm_ts(&e.created_at);
            let (Some(scope), Some(created_at), "done") = (scope, created_at, e.status.as_str())
            else {
                out.skipped.push(format!(
                    "{} {} の要約を取り込めませんでした (scope {})",
                    period.as_str(),
                    start_s,
                    e.scope
                ));
                continue;
            };
            tx.execute(
                "INSERT INTO digests(period, period_start, scope, body, item_ids, provider, status, \
                 attempts, created_at, last_error) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'done', 0, ?7, NULL) \
                 ON CONFLICT(period, period_start, scope) DO UPDATE SET body = excluded.body, \
                 item_ids = excluded.item_ids, provider = excluded.provider, status = 'done', \
                 attempts = 0, created_at = excluded.created_at, last_error = NULL",
                params![
                    period.as_str(),
                    start_s,
                    scope.key(),
                    e.body,
                    serde_json::to_string(&e.item_ids)?,
                    e.provider,
                    created_at,
                ],
            )?;
            keys.insert(scope.key());
            out.saved += 1;
        }
        let local: Vec<(i64, String)> = {
            let mut stmt = tx
                .prepare("SELECT id, scope FROM digests WHERE period = ?1 AND period_start = ?2")?;
            stmt.query_map(params![period.as_str(), start_s], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })?
            .collect::<rusqlite::Result<_>>()?
        };
        for (id, scope) in local {
            if !keys.contains(&scope) {
                tx.execute("DELETE FROM digests WHERE id = ?1", params![id])?;
                out.removed += 1;
            }
        }
        tx.commit()?;
        Ok(out)
    }

    /// 保存先の `db_id` が変わった (サーバーの DB を作り直した)。記事、索引、ダイジェスト、送っていない
    /// 操作、ファイルの記録を消し、新しい `db_id` を覚える。ソース、トピック、設定、チャットは残す。
    pub fn reset_for_db_id(&self, db_id: &str) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        tx.execute_batch(
            "DELETE FROM items; DELETE FROM items_fts; DELETE FROM items_words; \
             DELETE FROM digests; DELETE FROM sync_ops; DELETE FROM sync_files;",
        )?;
        put_setting(&tx, "sync.db_id", db_id)?;
        tx.commit()?;
        Ok(())
    }

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
    use crate::store::{DayImport, NewDigest, Store};
    use crate::sync::format::{DayFile, DigestRecord, ItemRecord};
    use crate::sync::mode::{MODE_READER, MODE_WRITER, SETTING_MODE, is_reader};
    use crate::testutil::{at, new_item, store_with_source};
    use chrono::{FixedOffset, NaiveDate};

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

    fn utc() -> FixedOffset {
        FixedOffset::east_opt(0).unwrap()
    }

    fn rec(id: i64, url: &str, title: &str, fetched: &str) -> ItemRecord {
        ItemRecord {
            id,
            url: url.into(),
            title: title.into(),
            excerpt: String::new(),
            ai_summary: None,
            image_url: None,
            source_id: Some(1),
            source_name: "Test Feed".into(),
            category: "tech_jp".into(),
            score: None,
            published_at: None,
            fetched_at: fetched.into(),
            topic_ids: vec![],
        }
    }

    fn day(date: &str, items: Vec<ItemRecord>) -> DayFile {
        DayFile {
            date: date.into(),
            items,
        }
    }

    fn search(s: &Store, text: &str) -> Vec<i64> {
        s.query_items(&ItemFilter {
            text: Some(text.into()),
            ..Default::default()
        })
        .unwrap()
        .into_iter()
        .map(|i| i.id)
        .collect()
    }

    #[test]
    fn import_days_inserts_and_updates_articles_without_touching_marks() {
        let s = Store::open_in_memory().unwrap();
        s.set_setting("sync.enabled", "true").unwrap();
        let r = s
            .import_days(
                &[day(
                    "2026-10-13",
                    vec![rec(10, "https://a/10", "oldtitle", "2026-10-13T01:00:00Z")],
                )],
                utc(),
                true,
            )
            .unwrap();
        assert_eq!(
            r,
            DayImport {
                upserted: 1,
                removed: 0,
                skipped: vec![]
            }
        );
        s.set_starred(10, true).unwrap();
        let mut newer = rec(
            10,
            "https://a/10",
            "Rust release notes",
            "2026-10-13T01:00:00.000Z",
        );
        newer.ai_summary = Some("要約".into());
        s.import_days(&[day("2026-10-13", vec![newer])], utc(), true)
            .unwrap();
        let item = s.get_item(10).unwrap().unwrap();
        assert_eq!(item.title, "Rust release notes");
        assert_eq!(item.ai_summary.as_deref(), Some("要約"));
        assert!(item.starred, "marks come from ops, never from day files");
        assert_eq!(item.fetched_at, at("2026-10-13T01:00:00Z"));
        assert_eq!(
            search(&s, "release"),
            vec![10],
            "search index follows the new title"
        );
        assert!(
            search(&s, "oldtitle").is_empty(),
            "the old title is gone from the index"
        );
    }

    #[test]
    fn ids_can_move_between_days_in_one_import_whatever_the_order() {
        // Review Focus 1
        let s = Store::open_in_memory().unwrap();
        s.set_setting("sync.enabled", "true").unwrap();
        s.import_days(
            &[
                day(
                    "2026-10-12",
                    vec![rec(1, "https://a/1", "one", "2026-10-12T01:00:00Z")],
                ),
                day(
                    "2026-10-13",
                    vec![
                        rec(2, "https://a/2", "two", "2026-10-13T01:00:00Z"),
                        rec(3, "https://a/3", "three", "2026-10-13T02:00:00Z"),
                    ],
                ),
            ],
            utc(),
            true,
        )
        .unwrap();
        s.set_read(3, true).unwrap();
        // 3 は 12 日へ移った。新しい 13 日を先に渡しても、3 は消えない。
        let r = s
            .import_days(
                &[
                    day(
                        "2026-10-13",
                        vec![rec(2, "https://a/2", "two", "2026-10-13T01:00:00Z")],
                    ),
                    day(
                        "2026-10-12",
                        vec![
                            rec(1, "https://a/1", "one", "2026-10-12T01:00:00Z"),
                            rec(3, "https://a/3", "three", "2026-10-12T05:00:00Z"),
                        ],
                    ),
                ],
                utc(),
                true,
            )
            .unwrap();
        assert_eq!(r.removed, 0);
        let moved = s.get_item(3).unwrap().unwrap();
        assert_eq!(moved.fetched_at, at("2026-10-12T05:00:00Z"));
        assert!(moved.read);
        // 13 日のファイルが空になれば、13 日の記事は消える (索引からも)。
        let r = s
            .import_days(&[day("2026-10-13", vec![])], utc(), true)
            .unwrap();
        assert_eq!(r.removed, 1);
        assert!(s.get_item(2).unwrap().is_none());
        assert!(search(&s, "two").is_empty());
    }

    #[test]
    fn without_remove_missing_nothing_is_deleted() {
        let s = Store::open_in_memory().unwrap();
        s.import_days(
            &[day(
                "2026-10-13",
                vec![rec(2, "https://a/2", "two", "2026-10-13T01:00:00Z")],
            )],
            utc(),
            true,
        )
        .unwrap();
        let r = s
            .import_days(&[day("2026-10-13", vec![])], utc(), false)
            .unwrap();
        assert_eq!(r.removed, 0);
        assert!(s.get_item(2).unwrap().is_some());
    }

    #[test]
    fn a_url_held_by_another_id_is_replaced() {
        let (s, src) = store_with_source(Category::TechJp);
        s.upsert_items(
            &src,
            &[new_item("https://a/x", "local", "", None)],
            &at("2026-10-13T00:00:00Z"),
        )
        .unwrap();
        let local = s.query_items(&ItemFilter::default()).unwrap()[0].id;
        s.import_days(
            &[day(
                "2026-10-13",
                vec![rec(50, "https://a/x", "server", "2026-10-13T03:00:00Z")],
            )],
            utc(),
            true,
        )
        .unwrap();
        assert!(s.get_item(local).unwrap().is_none());
        assert_eq!(s.get_item(50).unwrap().unwrap().title, "server");
        assert!(search(&s, "local").is_empty());
    }

    #[test]
    fn bad_records_are_skipped_and_their_local_copies_kept() {
        // Review Focus 2
        let s = Store::open_in_memory().unwrap();
        s.import_days(
            &[day(
                "2026-10-13",
                vec![rec(7, "https://a/7", "seven", "2026-10-13T01:00:00Z")],
            )],
            utc(),
            true,
        )
        .unwrap();
        let mut unknown = rec(7, "https://a/7", "seven v2", "2026-10-13T01:00:00Z");
        unknown.category = "podcasts".into();
        let bad_time = rec(8, "https://a/8", "eight", "yesterday");
        let mut bad_published = rec(9, "https://a/9", "nine", "2026-10-13T02:00:00Z");
        bad_published.published_at = Some("soon".into());
        let r = s
            .import_days(
                &[day("2026-10-13", vec![unknown, bad_time, bad_published])],
                utc(),
                true,
            )
            .unwrap();
        assert_eq!(r.skipped.len(), 2, "{:?}", r.skipped);
        assert_eq!(r.removed, 0, "a skipped id still counts as present");
        assert_eq!(
            s.get_item(7).unwrap().unwrap().title,
            "seven",
            "local copy kept"
        );
        assert!(s.get_item(8).unwrap().is_none());
        assert_eq!(s.get_item(9).unwrap().unwrap().published_at, None);
        assert_eq!(
            s.query_items(&ItemFilter::default()).unwrap().len(),
            2,
            "the feed still loads"
        );
    }

    fn entry(scope: serde_json::Value, body: &str, created_at: &str) -> DigestRecord {
        DigestRecord {
            scope,
            body: body.into(),
            status: "done".into(),
            item_ids: vec![1, 2],
            provider: "claude_api:claude-sonnet-5-5".into(),
            created_at: created_at.into(),
        }
    }

    #[test]
    fn import_digests_replaces_the_period_and_keeps_file_times() {
        let s = Store::open_in_memory().unwrap();
        let start = d("2026-10-13");
        let now = at("2026-10-13T00:00:00Z");
        let github = DigestScope::Category(Category::Github);
        s.save_digest_done(
            &NewDigest {
                period: Period::Day,
                period_start: start,
                scope: github.clone(),
                body: "old".into(),
                item_ids: vec![],
                provider: "ollama".into(),
            },
            &now,
        )
        .unwrap();
        s.save_digest_done(
            &NewDigest {
                period: Period::Day,
                period_start: start,
                scope: DigestScope::Category(Category::Arxiv),
                body: "gone".into(),
                item_ids: vec![],
                provider: "ollama".into(),
            },
            &now,
        )
        .unwrap();
        let r = s
            .import_digests(
                Period::Day,
                start,
                &[
                    entry(
                        serde_json::json!({"category": "github"}),
                        "new",
                        "2026-10-13T22:00:00.000Z",
                    ),
                    entry(
                        serde_json::json!({"topic": 4}),
                        "topic",
                        "2026-10-13T22:00:00.000Z",
                    ),
                    entry(
                        serde_json::json!({"podcast": 1}),
                        "?",
                        "2026-10-13T22:00:00.000Z",
                    ),
                ],
            )
            .unwrap();
        assert_eq!((r.saved, r.removed, r.skipped.len()), (2, 1, 1), "{r:?}");
        let g = s.get_digest(Period::Day, start, &github).unwrap().unwrap();
        assert_eq!(g.body, "new");
        assert_eq!(g.provider, "claude_api:claude-sonnet-5-5");
        assert_eq!(g.created_at, at("2026-10-13T22:00:00Z"));
        assert_eq!(g.item_ids, vec![1, 2]);
        assert!(
            s.get_digest(Period::Day, start, &DigestScope::Category(Category::Arxiv))
                .unwrap()
                .is_none()
        );
        assert!(
            s.get_digest(Period::Day, start, &DigestScope::Topic(4))
                .unwrap()
                .is_some()
        );
    }

    #[test]
    fn reset_for_db_id_wipes_synced_data_and_keeps_the_rest() {
        let (s, _) = store_with_source(Category::TechJp);
        s.import_days(
            &[day(
                "2026-10-13",
                vec![rec(1, "https://a/1", "stale", "2026-10-13T01:00:00Z")],
            )],
            utc(),
            true,
        )
        .unwrap();
        s.import_digests(
            Period::Day,
            d("2026-10-13"),
            &[entry(
                serde_json::json!({"category": "github"}),
                "b",
                "2026-10-13T22:00:00.000Z",
            )],
        )
        .unwrap();
        s.add_topic("AI", &["AI".to_string()]).unwrap();
        s.create_chat_thread("話", &at("2026-10-13T00:00:00Z"))
            .unwrap();
        s.set_setting("sync.device_name", "Mac").unwrap();
        s.put_sync_file(&crate::store::SyncFileRow {
            name: "day-2026-10-13.json.gz".into(),
            kind: "in".into(),
            drive_id: Some("id1".into()),
            sha256: Some("x".into()),
            updated_at: "t".into(),
            fingerprint: None,
        })
        .unwrap();

        s.reset_for_db_id("db2").unwrap();

        assert!(s.query_items(&ItemFilter::default()).unwrap().is_empty());
        assert!(
            s.list_digests(Period::Day, d("2026-10-13"))
                .unwrap()
                .is_empty()
        );
        assert!(s.sync_files("in").unwrap().is_empty());
        assert_eq!(s.get_setting("sync.db_id").unwrap().as_deref(), Some("db2"));
        assert_eq!(
            s.get_setting("sync.device_name").unwrap().as_deref(),
            Some("Mac")
        );
        assert_eq!(s.list_topics().unwrap().len(), 1);
        assert_eq!(s.list_sources().unwrap().len(), 1);
        assert_eq!(s.list_chat_threads().unwrap().len(), 1);
        // 同じ id の記事を入れ直しても、古いタイトルが索引に残っていない。
        s.import_days(
            &[day(
                "2026-10-13",
                vec![rec(1, "https://b/1", "fresh", "2026-10-13T01:00:00Z")],
            )],
            utc(),
            true,
        )
        .unwrap();
        assert!(search(&s, "stale").is_empty());
    }
}
