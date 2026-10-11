use chrono::{Datelike, Duration, FixedOffset, NaiveDate};

use super::Store;
use crate::error::Result;
use crate::model::{Category, CategoryStat, DigestStats, ItemFilter, Period, TopicStat};
use crate::period::{period_end, period_start, utc_range};

fn previous_start(period: Period, start: NaiveDate) -> NaiveDate {
    match period {
        Period::Day => start - Duration::days(1),
        Period::Week => start - Duration::days(7),
        Period::Month => {
            let (y, m) = if start.month() == 1 {
                (start.year() - 1, 12)
            } else {
                (start.year(), start.month() - 1)
            };
            NaiveDate::from_ymd_opt(y, m, 1).expect("first of month")
        }
    }
}

fn days_in_period(period: Period, start: NaiveDate) -> usize {
    match period {
        Period::Day => 0,
        Period::Week | Period::Month => (period_end(period, start) - start).num_days() as usize,
    }
}

impl Store {
    /// 期間の件数をカテゴリー別とトピック別に集計する。日の区切りは `tz` の0時。
    pub fn digest_stats(
        &self,
        period: Period,
        date: NaiveDate,
        tz: FixedOffset,
        topic_id: Option<i64>,
    ) -> Result<DigestStats> {
        let start = period_start(period, date);
        let (since, until) = utc_range(period, start, tz);
        let n_days = days_in_period(period, start);
        let mut categories = Vec::new();
        let mut total = 0;
        for category in Category::ALL {
            let mut daily = vec![0i64; n_days];
            let mut stmt = self.conn.prepare(
                "SELECT fetched_at FROM items WHERE category = ?1 AND fetched_at >= ?2 AND fetched_at < ?3",
            )?;
            let rows = stmt.query_map(
                rusqlite::params![category.as_str(), super::ts(&since), super::ts(&until)],
                |r| r.get::<_, String>(0),
            )?;
            let mut count = 0;
            for r in rows {
                let raw = r?;
                count += 1;
                if n_days > 0 {
                    let t = chrono::DateTime::parse_from_rfc3339(&raw)
                        .map_err(|e| crate::error::Error::Invalid(e.to_string()))?;
                    let local = t.with_timezone(&tz).date_naive();
                    let idx = (local - start).num_days();
                    if (0..n_days as i64).contains(&idx) {
                        daily[idx as usize] += 1;
                    }
                }
            }
            total += count;
            categories.push(CategoryStat {
                category,
                count,
                daily,
            });
        }
        let prev = previous_start(period, start);
        let (prev_since, prev_until) = utc_range(period, prev, tz);
        let mut topics = Vec::new();
        for t in self
            .list_topics()?
            .into_iter()
            .filter(|t| topic_id.is_none_or(|id| id == t.id))
        {
            let count = self.count_items(&ItemFilter {
                topic_id: Some(t.id),
                since: Some(since),
                until: Some(until),
                ..Default::default()
            })?;
            let prev_count = self.count_items(&ItemFilter {
                topic_id: Some(t.id),
                since: Some(prev_since),
                until: Some(prev_until),
                ..Default::default()
            })?;
            topics.push(TopicStat {
                topic_id: t.id,
                name: t.name,
                count,
                prev_count,
            });
        }
        Ok(DigestStats {
            period,
            period_start: start,
            total,
            categories,
            topics,
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::model::{Category, Period, SourceKind};
    use crate::store::Store;
    use crate::testutil::{at, new_item};
    use chrono::{FixedOffset, NaiveDate};

    fn jst() -> FixedOffset {
        FixedOffset::east_opt(9 * 3600).unwrap()
    }

    /// (カテゴリー, 取得時刻 UTC) の記事を入れる。
    fn store_with(rows: &[(Category, &str)]) -> Store {
        let store = Store::open_in_memory().unwrap();
        let mut sources = std::collections::HashMap::new();
        for (i, (cat, when)) in rows.iter().enumerate() {
            let source_id = *sources.entry(*cat).or_insert_with(|| {
                store
                    .add_source(
                        SourceKind::Rss,
                        cat.as_str(),
                        *cat,
                        &format!("https://e.com/{}", cat.as_str()),
                    )
                    .unwrap()
            });
            let source = store.get_source(source_id).unwrap().unwrap();
            store
                .upsert_items(
                    &source,
                    &[new_item(
                        &format!("https://e.com/{i}"),
                        &format!("記事{i}"),
                        "",
                        None,
                    )],
                    &at(when),
                )
                .unwrap();
        }
        store
    }

    #[test]
    fn day_counts_by_category_with_local_boundaries() {
        // 2026-10-10 JST = 2026-10-09T15:00Z .. 2026-10-10T15:00Z
        let store = store_with(&[
            (Category::Medcare, "2026-10-09T15:00:00Z"),
            (Category::Medcare, "2026-10-10T14:59:59Z"),
            (Category::Medcare, "2026-10-10T15:00:00Z"),
            (Category::Github, "2026-10-09T14:59:59Z"),
        ]);
        let s = store
            .digest_stats(
                Period::Day,
                NaiveDate::from_ymd_opt(2026, 10, 10).unwrap(),
                jst(),
                None,
            )
            .unwrap();
        assert_eq!(
            s.period_start,
            NaiveDate::from_ymd_opt(2026, 10, 10).unwrap()
        );
        assert_eq!(s.total, 2);
        assert_eq!(s.categories.len(), Category::ALL.len());
        let med = s
            .categories
            .iter()
            .find(|c| c.category == Category::Medcare)
            .unwrap();
        assert_eq!(med.count, 2);
        assert!(med.daily.is_empty(), "日の期間では日ごとの件数を出さない");
        assert_eq!(
            s.categories
                .iter()
                .find(|c| c.category == Category::Github)
                .unwrap()
                .count,
            0
        );
    }

    #[test]
    fn week_daily_buckets_start_on_monday() {
        // 2026-10-05 は月曜。10/05 00:30 JST と 10/11 23:30 JST (日曜) と前週日曜 23:30 JST。
        let store = store_with(&[
            (Category::TechJp, "2026-10-04T15:30:00Z"),
            (Category::TechJp, "2026-10-11T14:30:00Z"),
            (Category::TechJp, "2026-10-04T14:30:00Z"),
        ]);
        let s = store
            .digest_stats(
                Period::Week,
                NaiveDate::from_ymd_opt(2026, 10, 8).unwrap(),
                jst(),
                None,
            )
            .unwrap();
        assert_eq!(
            s.period_start,
            NaiveDate::from_ymd_opt(2026, 10, 5).unwrap()
        );
        let jp = s
            .categories
            .iter()
            .find(|c| c.category == Category::TechJp)
            .unwrap();
        assert_eq!(jp.count, 2);
        assert_eq!(jp.daily, vec![1, 0, 0, 0, 0, 0, 1]);
    }

    #[test]
    fn month_has_one_bucket_per_day() {
        let store = store_with(&[(Category::Arxiv, "2026-10-31T10:00:00Z")]);
        let s = store
            .digest_stats(
                Period::Month,
                NaiveDate::from_ymd_opt(2026, 10, 15).unwrap(),
                jst(),
                None,
            )
            .unwrap();
        let ax = s
            .categories
            .iter()
            .find(|c| c.category == Category::Arxiv)
            .unwrap();
        assert_eq!(ax.daily.len(), 31);
        assert_eq!(ax.daily[30], 1);
        assert_eq!(ax.count, 1);
    }

    #[test]
    fn topics_compare_with_the_previous_period() {
        let store = store_with(&[
            (Category::Medcare, "2026-10-10T01:00:00Z"),
            (Category::Medcare, "2026-10-09T01:00:00Z"),
            (Category::Medcare, "2026-10-09T02:00:00Z"),
        ]);
        // 記事のタイトルは「記事N」。全部に合うキーワードのトピックと、どれにも合わないトピック。
        let all = store.add_topic("全部", &["記事".to_string()]).unwrap();
        let none = store
            .add_topic("なし", &["存在しない語".to_string()])
            .unwrap();
        let s = store
            .digest_stats(
                Period::Day,
                NaiveDate::from_ymd_opt(2026, 10, 10).unwrap(),
                jst(),
                None,
            )
            .unwrap();
        let t = |id: i64| s.topics.iter().find(|t| t.topic_id == id).unwrap().clone();
        assert_eq!((t(all).count, t(all).prev_count), (1, 2));
        assert_eq!((t(none).count, t(none).prev_count), (0, 0));
        assert_eq!(t(all).name, "全部");
        let one = store
            .digest_stats(
                Period::Day,
                NaiveDate::from_ymd_opt(2026, 10, 10).unwrap(),
                jst(),
                Some(all),
            )
            .unwrap();
        assert_eq!(one.topics.len(), 1);
        assert_eq!(one.topics[0].topic_id, all);
    }

    #[test]
    fn empty_period_is_all_zero() {
        let store = Store::open_in_memory().unwrap();
        let s = store
            .digest_stats(
                Period::Week,
                NaiveDate::from_ymd_opt(2026, 10, 8).unwrap(),
                jst(),
                None,
            )
            .unwrap();
        assert_eq!(s.total, 0);
        assert!(
            s.categories
                .iter()
                .all(|c| c.count == 0 && c.daily.iter().all(|d| *d == 0))
        );
        assert!(s.topics.is_empty());
    }
}
