use chrono::{DateTime, Datelike, Duration, FixedOffset, NaiveDate, TimeZone, Utc};

use crate::model::Period;

/// 日付を含む期間の開始日。週は月曜始まり。
pub fn period_start(period: Period, date: NaiveDate) -> NaiveDate {
    match period {
        Period::Day => date,
        Period::Week => date - Duration::days(date.weekday().num_days_from_monday() as i64),
        Period::Month => date.with_day(1).expect("day 1 always exists"),
    }
}

/// 期間の終了日 (その日を含まない)。`start` は `period_start` の戻り値を渡す。
pub fn period_end(period: Period, start: NaiveDate) -> NaiveDate {
    match period {
        Period::Day => start + Duration::days(1),
        Period::Week => start + Duration::days(7),
        Period::Month => {
            let (y, m) = if start.month() == 12 {
                (start.year() + 1, 1)
            } else {
                (start.year(), start.month() + 1)
            };
            NaiveDate::from_ymd_opt(y, m, 1).expect("first of month always exists")
        }
    }
}

fn local_midnight_utc(date: NaiveDate, tz: FixedOffset) -> DateTime<Utc> {
    let naive = date.and_hms_opt(0, 0, 0).expect("midnight always exists");
    tz.from_local_datetime(&naive)
        .single()
        .expect("fixed offset has no gaps")
        .with_timezone(&Utc)
}

/// 期間をUTCの半開区間 `[from, to)` に変換する。日付の境界は `tz` の0時。
pub fn utc_range(
    period: Period,
    start: NaiveDate,
    tz: FixedOffset,
) -> (DateTime<Utc>, DateTime<Utc>) {
    (
        local_midnight_utc(start, tz),
        local_midnight_utc(period_end(period, start), tz),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    #[test]
    fn starts() {
        // 2026-10-06 は火曜日。
        assert_eq!(period_start(Period::Day, d("2026-10-06")), d("2026-10-06"));
        assert_eq!(period_start(Period::Week, d("2026-10-06")), d("2026-10-05"));
        assert_eq!(period_start(Period::Week, d("2026-10-05")), d("2026-10-05"));
        assert_eq!(period_start(Period::Week, d("2026-10-11")), d("2026-10-05"));
        assert_eq!(
            period_start(Period::Month, d("2026-10-06")),
            d("2026-10-01")
        );
    }

    #[test]
    fn ends_are_exclusive() {
        assert_eq!(period_end(Period::Day, d("2026-10-06")), d("2026-10-07"));
        assert_eq!(period_end(Period::Week, d("2026-10-05")), d("2026-10-12"));
        assert_eq!(period_end(Period::Month, d("2026-10-01")), d("2026-11-01"));
        assert_eq!(period_end(Period::Month, d("2026-12-01")), d("2027-01-01"));
    }

    #[test]
    fn utc_range_uses_local_midnight() {
        let jst = FixedOffset::east_opt(9 * 3600).unwrap();
        let (from, to) = utc_range(Period::Day, d("2026-10-06"), jst);
        assert_eq!(from.to_rfc3339(), "2026-10-05T15:00:00+00:00");
        assert_eq!(to.to_rfc3339(), "2026-10-06T15:00:00+00:00");
        let (from, to) = utc_range(Period::Month, d("2026-10-01"), jst);
        assert_eq!(from.to_rfc3339(), "2026-09-30T15:00:00+00:00");
        assert_eq!(to.to_rfc3339(), "2026-10-31T15:00:00+00:00");
    }
}
