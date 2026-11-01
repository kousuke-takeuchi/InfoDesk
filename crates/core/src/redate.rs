//! 記事の日付を付け直したあとに作る (作り直す) 期間要約を決める。

use std::collections::BTreeSet;

use chrono::{Duration, NaiveDate};

use crate::error::Result;
use crate::model::{DigestStatus, Period};
use crate::period::{period_end, period_start};
use crate::store::{RedateRange, Store};

/// 作る期間要約の一覧 `(期間, 開始日, force)`。日次、週次、月次の順で、それぞれ古い順。
/// 週次と月次は短い期間の要約から作るので、この順に実行する。
pub fn redate_digest_plan(
    store: &Store,
    r: &RedateRange,
    today: NaiveDate,
) -> Result<Vec<(Period, NaiveDate, bool)>> {
    let mut out = Vec::new();
    for period in [Period::Day, Period::Week, Period::Month] {
        let mut overlapping = BTreeSet::new();
        let mut d = r.from;
        while d <= r.fetched_on {
            overlapping.insert(period_start(period, d));
            d += Duration::days(1);
        }
        let next: BTreeSet<NaiveDate> = overlapping
            .iter()
            .map(|s| period_end(period, *s))
            .filter(|s| !overlapping.contains(s))
            .collect();
        let mut starts: Vec<(NaiveDate, bool)> = Vec::new();
        for start in overlapping.into_iter().chain(next.iter().copied()) {
            if start >= today {
                continue;
            }
            let has_done = store
                .list_digests(period, start)?
                .iter()
                .any(|d| d.status == DigestStatus::Done);
            if next.contains(&start) && !has_done {
                continue;
            }
            starts.push((start, has_done));
        }
        starts.sort();
        out.extend(starts.into_iter().map(|(s, force)| (period, s, force)));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use chrono::{NaiveDate, Utc};

    use super::redate_digest_plan;
    use crate::model::{Category, DigestScope, Period};
    use crate::store::{NewDigest, RedateRange, Store};

    fn d(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    fn done(s: &Store, period: Period, start: &str) {
        s.save_digest_done(
            &NewDigest {
                period,
                period_start: d(start),
                scope: DigestScope::Category(Category::TechJp),
                body: "b".into(),
                item_ids: vec![],
                provider: "p".into(),
            },
            &Utc::now(),
        )
        .unwrap();
    }

    fn range() -> RedateRange {
        RedateRange {
            fetched_on: d("2026-11-01"),
            from: d("2026-10-27"),
            to: d("2026-10-31"),
        }
    }

    #[test]
    fn the_2026_11_01_gap() {
        let s = Store::open_in_memory().unwrap();
        done(&s, Period::Day, "2026-10-26");
        done(&s, Period::Week, "2026-10-26");
        done(&s, Period::Month, "2026-10-01");
        let plan = redate_digest_plan(&s, &range(), d("2026-11-01")).unwrap();
        assert_eq!(
            plan,
            vec![
                (Period::Day, d("2026-10-27"), false),
                (Period::Day, d("2026-10-28"), false),
                (Period::Day, d("2026-10-29"), false),
                (Period::Day, d("2026-10-30"), false),
                (Period::Day, d("2026-10-31"), false),
                (Period::Week, d("2026-10-26"), true),
                (Period::Month, d("2026-10-01"), true),
            ]
        );
    }

    #[test]
    fn next_period_is_remade_only_when_it_has_a_digest() {
        let s = Store::open_in_memory().unwrap();
        // 11-02 は 11-01 の次の日。比較の数字が変わるので作り直す。
        done(&s, Period::Day, "2026-11-02");
        let plan = redate_digest_plan(&s, &range(), d("2026-11-10")).unwrap();
        let days: Vec<_> = plan
            .iter()
            .filter(|p| p.0 == Period::Day)
            .cloned()
            .collect();
        assert_eq!(days.first(), Some(&(Period::Day, d("2026-10-27"), false)));
        assert!(days.contains(&(Period::Day, d("2026-11-01"), false)));
        assert!(days.contains(&(Period::Day, d("2026-11-02"), true)));
        assert_eq!(days.len(), 7);
        // 週: 10-26 の週 (重なる) と、次の 11-02 の週は要約がないので出さない。
        let weeks: Vec<_> = plan
            .iter()
            .filter(|p| p.0 == Period::Week)
            .cloned()
            .collect();
        assert_eq!(weeks, vec![(Period::Week, d("2026-10-26"), false)]);
        // 月: 10 月と 11 月が重なる。
        let months: Vec<_> = plan
            .iter()
            .filter(|p| p.0 == Period::Month)
            .cloned()
            .collect();
        assert_eq!(
            months,
            vec![
                (Period::Month, d("2026-10-01"), false),
                (Period::Month, d("2026-11-01"), false)
            ]
        );
    }

    #[test]
    fn periods_starting_today_or_later_are_left_out() {
        let s = Store::open_in_memory().unwrap();
        let plan = redate_digest_plan(&s, &range(), d("2026-10-30")).unwrap();
        assert!(
            plan.iter().all(|(_, start, _)| *start < d("2026-10-30")),
            "{plan:?}"
        );
    }
}
