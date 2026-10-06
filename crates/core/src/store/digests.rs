use chrono::{DateTime, NaiveDate, Utc};
use rusqlite::{params, OptionalExtension, Row};

use super::{col_parse, col_ts, conv_err, ts, Store};
use crate::error::Result;
use crate::model::{Digest, DigestScope, DigestStatus, Period};

#[derive(Debug, Clone)]
pub struct NewDigest {
    pub period: Period,
    pub period_start: NaiveDate,
    pub scope: DigestScope,
    pub body: String,
    pub item_ids: Vec<i64>,
    pub provider: String,
}

const DIGEST_COLS: &str =
    "id, period, period_start, scope, body, item_ids, provider, status, attempts, created_at";

const MAX_ATTEMPTS: i64 = 3;

fn parse_date(s: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(s, "%Y-%m-%d").ok()
}

fn date_str(d: NaiveDate) -> String {
    d.format("%Y-%m-%d").to_string()
}

fn row_to_digest(r: &Row) -> rusqlite::Result<Digest> {
    let ids: String = r.get(5)?;
    Ok(Digest {
        id: r.get(0)?,
        period: col_parse(r, 1, Period::parse)?,
        period_start: col_parse(r, 2, parse_date)?,
        scope: col_parse(r, 3, DigestScope::parse)?,
        body: r.get(4)?,
        item_ids: serde_json::from_str(&ids).map_err(|e| conv_err(5, e.to_string()))?,
        provider: r.get(6)?,
        status: col_parse(r, 7, DigestStatus::parse)?,
        attempts: r.get(8)?,
        created_at: col_ts(r, 9)?,
    })
}

impl Store {
    pub fn save_digest_done(&self, d: &NewDigest, now: &DateTime<Utc>) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO digests(period, period_start, scope, body, item_ids, provider, status, created_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'done', ?7) \
             ON CONFLICT(period, period_start, scope) DO UPDATE SET \
               body = excluded.body, item_ids = excluded.item_ids, provider = excluded.provider, \
               status = 'done', created_at = excluded.created_at",
            params![
                d.period.as_str(),
                date_str(d.period_start),
                d.scope.key(),
                d.body,
                serde_json::to_string(&d.item_ids)?,
                d.provider,
                ts(now),
            ],
        )?;
        let id = self.conn.query_row(
            "SELECT id FROM digests WHERE period = ?1 AND period_start = ?2 AND scope = ?3",
            params![d.period.as_str(), date_str(d.period_start), d.scope.key()],
            |r| r.get(0),
        )?;
        Ok(id)
    }

    /// 生成の失敗を記録する。生成済みの行は変更しない。
    pub fn mark_digest_attempt_failed(
        &self,
        period: Period,
        start: NaiveDate,
        scope: &DigestScope,
        now: &DateTime<Utc>,
    ) -> Result<DigestStatus> {
        self.conn.execute(
            "INSERT INTO digests(period, period_start, scope, status, attempts, created_at) \
             VALUES (?1, ?2, ?3, 'pending', 1, ?4) \
             ON CONFLICT(period, period_start, scope) DO UPDATE SET \
               attempts = attempts + 1, \
               status = CASE WHEN attempts + 1 >= ?5 THEN 'failed' ELSE 'pending' END \
             WHERE digests.status != 'done'",
            params![period.as_str(), date_str(start), scope.key(), ts(now), MAX_ATTEMPTS],
        )?;
        let status = self.conn.query_row(
            "SELECT status FROM digests WHERE period = ?1 AND period_start = ?2 AND scope = ?3",
            params![period.as_str(), date_str(start), scope.key()],
            |r| col_parse(r, 0, DigestStatus::parse),
        )?;
        Ok(status)
    }

    pub fn get_digest(
        &self,
        period: Period,
        start: NaiveDate,
        scope: &DigestScope,
    ) -> Result<Option<Digest>> {
        Ok(self
            .conn
            .query_row(
                &format!(
                    "SELECT {DIGEST_COLS} FROM digests \
                     WHERE period = ?1 AND period_start = ?2 AND scope = ?3"
                ),
                params![period.as_str(), date_str(start), scope.key()],
                row_to_digest,
            )
            .optional()?)
    }

    pub fn list_digests(&self, period: Period, start: NaiveDate) -> Result<Vec<Digest>> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {DIGEST_COLS} FROM digests WHERE period = ?1 AND period_start = ?2 ORDER BY scope"
        ))?;
        let rows = stmt
            .query_map(params![period.as_str(), date_str(start)], row_to_digest)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn digests_in_range(
        &self,
        period: Period,
        scope: &DigestScope,
        from: NaiveDate,
        to_exclusive: NaiveDate,
    ) -> Result<Vec<Digest>> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {DIGEST_COLS} FROM digests \
             WHERE period = ?1 AND scope = ?2 AND status = 'done' \
               AND period_start >= ?3 AND period_start < ?4 \
             ORDER BY period_start"
        ))?;
        let rows = stmt
            .query_map(
                params![period.as_str(), scope.key(), date_str(from), date_str(to_exclusive)],
                row_to_digest,
            )?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn pending_digests(&self) -> Result<Vec<Digest>> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {DIGEST_COLS} FROM digests WHERE status = 'pending' ORDER BY period_start, scope"
        ))?;
        let rows = stmt
            .query_map([], row_to_digest)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Category;
    use crate::testutil::at;

    fn d(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    fn nd(period: Period, start: &str, scope: DigestScope, body: &str, ids: &[i64]) -> NewDigest {
        NewDigest {
            period,
            period_start: d(start),
            scope,
            body: body.to_string(),
            item_ids: ids.to_vec(),
            provider: "mock".to_string(),
        }
    }

    #[test]
    fn save_get_and_overwrite() {
        let store = Store::open_in_memory().unwrap();
        let scope = DigestScope::Category(Category::Github);
        let now = at("2026-10-06T00:00:00Z");
        store
            .save_digest_done(&nd(Period::Day, "2026-10-06", scope.clone(), "v1", &[1, 2]), &now)
            .unwrap();
        store
            .save_digest_done(&nd(Period::Day, "2026-10-06", scope.clone(), "v2", &[3]), &now)
            .unwrap();
        let got = store.get_digest(Period::Day, d("2026-10-06"), &scope).unwrap().unwrap();
        assert_eq!(got.body, "v2");
        assert_eq!(got.item_ids, vec![3]);
        assert_eq!(got.status, DigestStatus::Done);
        assert_eq!(got.provider, "mock");
        assert!(store.get_digest(Period::Week, d("2026-10-05"), &scope).unwrap().is_none());
        assert_eq!(store.list_digests(Period::Day, d("2026-10-06")).unwrap().len(), 1);
    }

    #[test]
    fn failed_attempts_go_pending_then_failed_and_success_recovers() {
        let store = Store::open_in_memory().unwrap();
        let scope = DigestScope::Topic(1);
        let now = at("2026-10-06T00:00:00Z");
        let day = d("2026-10-06");
        assert_eq!(
            store.mark_digest_attempt_failed(Period::Day, day, &scope, &now).unwrap(),
            DigestStatus::Pending
        );
        assert_eq!(store.pending_digests().unwrap().len(), 1);
        assert_eq!(
            store.mark_digest_attempt_failed(Period::Day, day, &scope, &now).unwrap(),
            DigestStatus::Pending
        );
        assert_eq!(
            store.mark_digest_attempt_failed(Period::Day, day, &scope, &now).unwrap(),
            DigestStatus::Failed
        );
        assert!(store.pending_digests().unwrap().is_empty());
        assert_eq!(store.get_digest(Period::Day, day, &scope).unwrap().unwrap().attempts, 3);

        store
            .save_digest_done(&nd(Period::Day, "2026-10-06", scope.clone(), "ok", &[]), &now)
            .unwrap();
        assert_eq!(
            store.get_digest(Period::Day, day, &scope).unwrap().unwrap().status,
            DigestStatus::Done
        );
    }

    #[test]
    fn failure_does_not_downgrade_done_digest() {
        let store = Store::open_in_memory().unwrap();
        let scope = DigestScope::Category(Category::Arxiv);
        let now = at("2026-10-06T00:00:00Z");
        store
            .save_digest_done(&nd(Period::Day, "2026-10-06", scope.clone(), "keep", &[]), &now)
            .unwrap();
        let st = store
            .mark_digest_attempt_failed(Period::Day, d("2026-10-06"), &scope, &now)
            .unwrap();
        assert_eq!(st, DigestStatus::Done);
        let got = store.get_digest(Period::Day, d("2026-10-06"), &scope).unwrap().unwrap();
        assert_eq!(got.body, "keep");
    }

    #[test]
    fn range_returns_done_only_in_order_for_scope() {
        let store = Store::open_in_memory().unwrap();
        let gh = DigestScope::Category(Category::Github);
        let ax = DigestScope::Category(Category::Arxiv);
        let now = at("2026-10-06T00:00:00Z");
        for (day, scope, body) in [
            ("2026-10-07", gh.clone(), "d7"),
            ("2026-10-05", gh.clone(), "d5"),
            ("2026-10-12", gh.clone(), "next week"),
            ("2026-10-06", ax.clone(), "other scope"),
        ] {
            store.save_digest_done(&nd(Period::Day, day, scope, body, &[]), &now).unwrap();
        }
        store
            .mark_digest_attempt_failed(Period::Day, d("2026-10-06"), &gh, &now)
            .unwrap();
        let got = store
            .digests_in_range(Period::Day, &gh, d("2026-10-05"), d("2026-10-12"))
            .unwrap();
        let bodies: Vec<&str> = got.iter().map(|g| g.body.as_str()).collect();
        assert_eq!(bodies, vec!["d5", "d7"]);
    }
}
