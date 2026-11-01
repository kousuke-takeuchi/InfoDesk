//! 取得の空白を埋める: ある日に取得した記事を、公開日に取得したことにする。

use std::collections::BTreeMap;

use chrono::{DateTime, FixedOffset, NaiveDate, Utc};
use rusqlite::params;

use super::sync::day_bounds;
use super::{Store, ts};
use crate::error::{Error, Result};

/// `fetched_on` の日に取得し、`from`〜`to` の日に公開された記事が対象。日付はすべて現地時間。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RedateRange {
    pub fetched_on: NaiveDate,
    pub from: NaiveDate,
    pub to: NaiveDate,
}

impl RedateRange {
    /// `from <= to < fetched_on` でなければエラー。
    pub fn validate(&self) -> Result<()> {
        if self.from <= self.to && self.to < self.fetched_on {
            Ok(())
        } else {
            Err(Error::Invalid(format!(
                "need --from <= --to < --fetched-on (got --from {} --to {} --fetched-on {})",
                self.from, self.to, self.fetched_on
            )))
        }
    }
}

/// 移す先の日 (公開日) とカテゴリーごとの件数。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RedateCount {
    pub day: NaiveDate,
    pub category: String,
    pub count: usize,
}

impl Store {
    /// 移す記事の件数。何も変えない。
    pub fn redate_preview(&self, r: &RedateRange, tz: FixedOffset) -> Result<Vec<RedateCount>> {
        Ok(counts(&self.redate_targets(r, tz)?, tz))
    }

    /// 対象の記事の `fetched_at` を `published_at` にする。1 つのトランザクションで行う。
    pub fn redate_apply(&self, r: &RedateRange, tz: FixedOffset) -> Result<Vec<RedateCount>> {
        let targets = self.redate_targets(r, tz)?;
        let tx = self.conn.unchecked_transaction()?;
        for (id, _, _) in &targets {
            tx.execute(
                "UPDATE items SET fetched_at = published_at WHERE id = ?1",
                params![id],
            )?;
        }
        tx.commit()?;
        Ok(counts(&targets, tz))
    }

    fn redate_targets(
        &self,
        r: &RedateRange,
        tz: FixedOffset,
    ) -> Result<Vec<(i64, String, DateTime<Utc>)>> {
        r.validate()?;
        let (fetched_from, fetched_until) = day_bounds(r.fetched_on, tz)?;
        let (published_from, _) = day_bounds(r.from, tz)?;
        let (_, published_until) = day_bounds(r.to, tz)?;
        let mut stmt = self.conn.prepare(
            "SELECT id, category, published_at FROM items \
             WHERE fetched_at >= ?1 AND fetched_at < ?2 \
             AND published_at IS NOT NULL AND published_at >= ?3 AND published_at < ?4 \
             ORDER BY published_at, id",
        )?;
        let rows = stmt.query_map(
            params![
                ts(&fetched_from),
                ts(&fetched_until),
                ts(&published_from),
                ts(&published_until)
            ],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            },
        )?;
        let mut out = Vec::new();
        for row in rows {
            let (id, category, published) = row?;
            let published = DateTime::parse_from_rfc3339(&published)
                .map_err(|e| {
                    Error::Invalid(format!("item {id}: bad published_at {published}: {e}"))
                })?
                .with_timezone(&Utc);
            out.push((id, category, published));
        }
        Ok(out)
    }
}

fn counts(targets: &[(i64, String, DateTime<Utc>)], tz: FixedOffset) -> Vec<RedateCount> {
    let mut by: BTreeMap<(NaiveDate, String), usize> = BTreeMap::new();
    for (_, category, published) in targets {
        *by.entry((published.with_timezone(&tz).date_naive(), category.clone()))
            .or_default() += 1;
    }
    by.into_iter()
        .map(|((day, category), count)| RedateCount {
            day,
            category,
            count,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use chrono::{FixedOffset, NaiveDate};

    use super::{RedateCount, RedateRange};
    use crate::model::{Category, ItemFilter};
    use crate::store::Store;
    use crate::testutil::{at, new_item, store_with_source};

    fn tz() -> FixedOffset {
        FixedOffset::east_opt(9 * 3600).unwrap()
    }

    fn d(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    fn add(s: &Store, url: &str, fetched: &str, published: Option<&str>) -> i64 {
        let src = s.list_sources().unwrap().remove(0);
        let mut it = new_item(url, "t", "", None);
        it.published_at = published.map(at);
        s.upsert_items(&src, &[it], &at(fetched)).unwrap();
        s.query_items(&ItemFilter::default())
            .unwrap()
            .into_iter()
            .find(|i| i.url == url)
            .unwrap()
            .id
    }

    fn range() -> RedateRange {
        RedateRange {
            fetched_on: d("2026-11-01"),
            from: d("2026-10-27"),
            to: d("2026-10-31"),
        }
    }

    const FETCHED: &str = "2026-11-01T04:00:00Z"; // JST 11-01 13:00

    struct Ids {
        moves_30: i64,
        moves_27: i64,
        before_from: i64,
        after_to: i64,
        no_date: i64,
        other_day: i64,
    }

    fn fixture() -> (Store, Ids) {
        let s = store_with_source(Category::TechJp).0;
        let ids = Ids {
            moves_30: add(&s, "https://a/1", FETCHED, Some("2026-10-30T03:00:00Z")),
            // UTC 10-26 15:30 は JST 10-27 00:30。
            moves_27: add(&s, "https://a/2", FETCHED, Some("2026-10-26T15:30:00Z")),
            // UTC 10-26 14:30 は JST 10-26 23:30。
            before_from: add(&s, "https://a/3", FETCHED, Some("2026-10-26T14:30:00Z")),
            // UTC 10-31 15:00 は JST 11-01 00:00。
            after_to: add(&s, "https://a/4", FETCHED, Some("2026-10-31T15:00:00Z")),
            no_date: add(&s, "https://a/5", FETCHED, None),
            other_day: add(
                &s,
                "https://a/6",
                "2026-10-31T04:00:00Z",
                Some("2026-10-30T00:00:00Z"),
            ),
        };
        (s, ids)
    }

    fn expected() -> Vec<RedateCount> {
        vec![
            RedateCount {
                day: d("2026-10-27"),
                category: "tech_jp".into(),
                count: 1,
            },
            RedateCount {
                day: d("2026-10-30"),
                category: "tech_jp".into(),
                count: 1,
            },
        ]
    }

    #[test]
    fn preview_counts_by_local_publish_day_and_changes_nothing() {
        let (s, ids) = fixture();
        assert_eq!(s.redate_preview(&range(), tz()).unwrap(), expected());
        let it = s.get_item(ids.moves_30).unwrap().unwrap();
        assert_eq!(it.fetched_at, at(FETCHED));
    }

    #[test]
    fn apply_moves_only_matching_articles_and_keeps_their_state() {
        let (s, ids) = fixture();
        s.set_ai_summary(ids.moves_30, "要約").unwrap();
        s.set_read(ids.moves_30, true).unwrap();
        s.set_starred(ids.moves_30, true).unwrap();

        assert_eq!(s.redate_apply(&range(), tz()).unwrap(), expected());

        let moved = s.get_item(ids.moves_30).unwrap().unwrap();
        assert_eq!(moved.fetched_at, at("2026-10-30T03:00:00Z"));
        assert_eq!(moved.ai_summary.as_deref(), Some("要約"));
        assert!(moved.read && moved.starred);
        assert_eq!(
            s.get_item(ids.moves_27).unwrap().unwrap().fetched_at,
            at("2026-10-26T15:30:00Z")
        );
        for id in [ids.before_from, ids.after_to, ids.no_date] {
            assert_eq!(
                s.get_item(id).unwrap().unwrap().fetched_at,
                at(FETCHED),
                "id {id}"
            );
        }
        assert_eq!(
            s.get_item(ids.other_day).unwrap().unwrap().fetched_at,
            at("2026-10-31T04:00:00Z")
        );

        let on_30: Vec<i64> = s
            .items_fetched_on(d("2026-10-30"), tz())
            .unwrap()
            .iter()
            .map(|r| r.id)
            .collect();
        assert_eq!(on_30, vec![ids.moves_30]);
        let on_01: Vec<i64> = s
            .items_fetched_on(d("2026-11-01"), tz())
            .unwrap()
            .iter()
            .map(|r| r.id)
            .collect();
        assert!(!on_01.contains(&ids.moves_30));
    }

    #[test]
    fn second_apply_finds_nothing() {
        let (s, _) = fixture();
        s.redate_apply(&range(), tz()).unwrap();
        assert!(s.redate_apply(&range(), tz()).unwrap().is_empty());
    }

    #[test]
    fn bad_ranges_fail_before_any_write() {
        let (s, ids) = fixture();
        let bad = [
            RedateRange {
                fetched_on: d("2026-11-01"),
                from: d("2026-10-31"),
                to: d("2026-10-27"),
            },
            RedateRange {
                fetched_on: d("2026-11-01"),
                from: d("2026-10-27"),
                to: d("2026-11-01"),
            },
            RedateRange {
                fetched_on: d("2026-11-01"),
                from: d("2026-10-27"),
                to: d("2026-11-02"),
            },
        ];
        for r in bad {
            assert!(r.validate().is_err(), "{r:?}");
            assert!(s.redate_preview(&r, tz()).is_err(), "{r:?}");
            assert!(s.redate_apply(&r, tz()).is_err(), "{r:?}");
        }
        assert_eq!(
            s.get_item(ids.moves_30).unwrap().unwrap().fetched_at,
            at(FETCHED)
        );
    }
}
