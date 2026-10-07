use chrono::NaiveDate;
use infodesk_core::model::{Category, Digest, DigestScope, ItemFilter, Period};
use infodesk_core::period::{period_start, utc_range};
use serde::Serialize;
use tauri::{AppHandle, Manager, State};

use crate::error::AppResult;
use crate::jobs;
use crate::state::AppState;

#[derive(Debug, Clone, Serialize)]
pub struct DigestView {
    pub scope: DigestScope,
    pub label: String,
    pub digest: Option<Digest>,
    /// 取得日時がこの期間に入る記事の数。要約が無い理由 (記事なし / 未生成) を画面が区別する。
    pub item_count: i64,
}

fn parse_period(s: &str) -> AppResult<Period> {
    Period::parse(s).ok_or_else(|| format!("unknown period '{s}'").into())
}

fn parse_date(s: &str) -> AppResult<NaiveDate> {
    NaiveDate::parse_from_str(s, "%Y-%m-%d").map_err(|_| format!("invalid date '{s}'").into())
}

/// カテゴリー別 (topic_id なし) またはトピック別の要約を、生成済みかどうかに関わらず並べる。
pub fn digests_for(
    state: &AppState,
    period: &str,
    date: &str,
    topic_id: Option<i64>,
) -> AppResult<Vec<DigestView>> {
    let period = parse_period(period)?;
    let start = period_start(period, parse_date(date)?);
    let scopes: Vec<(DigestScope, String)> = match topic_id {
        Some(id) => {
            let topic = state
                .with_store(|s| s.get_topic(id))?
                .ok_or_else(|| format!("topic {id} not found"))?;
            vec![(DigestScope::Topic(id), topic.name)]
        }
        None => Category::ALL
            .into_iter()
            .map(|c| (DigestScope::Category(c), c.label_ja().to_string()))
            .collect(),
    };
    let (since, until) = utc_range(period, start, state.tz);
    state.with_store(|s| {
        scopes
            .into_iter()
            .map(|(scope, label)| {
                let mut filter = ItemFilter {
                    since: Some(since),
                    until: Some(until),
                    ..ItemFilter::default()
                };
                match &scope {
                    DigestScope::Category(c) => filter.category = Some(*c),
                    DigestScope::Topic(id) => filter.topic_id = Some(*id),
                }
                Ok(DigestView {
                    digest: s.get_digest(period, start, &scope)?,
                    item_count: s.count_items(&filter)?,
                    scope,
                    label,
                })
            })
            .collect()
    })
}

#[tauri::command]
pub fn get_digests(
    state: State<'_, AppState>,
    period: String,
    date: String,
    topic_id: Option<i64>,
) -> AppResult<Vec<DigestView>> {
    digests_for(&state, &period, &date, topic_id)
}

/// 再生成の引数を検証する。対象が空なら `None` (ジョブを起動しない)。
fn regenerate_request(
    period: &str,
    date: &str,
    scopes: Vec<DigestScope>,
) -> AppResult<Option<(Period, NaiveDate, Vec<DigestScope>)>> {
    let period = parse_period(period)?;
    let date = parse_date(date)?;
    Ok((!scopes.is_empty()).then_some((period, date, scopes)))
}

/// 指定した scope を1つのジョブで順に再生成する。ジョブを起動して即座に返す。
/// 進行はイベントで通知される。
#[tauri::command]
pub fn regenerate_digest(
    app: AppHandle,
    period: String,
    date: String,
    scopes: Vec<DigestScope>,
) -> AppResult<()> {
    let Some((period, date, scopes)) = regenerate_request(&period, &date, scopes)? else {
        return Ok(());
    };
    tauri::async_runtime::spawn(async move {
        let state = app.state::<AppState>();
        if let Err(e) = jobs::run_digests(&state, period, date, &scopes, true).await {
            state.update_status(|s| s.last_error = Some(e.message));
        }
    });
    Ok(())
}

#[tauri::command]
pub fn collect_now(app: AppHandle) -> AppResult<()> {
    // 収集中なら何もしない (重複起動はエラーではない)。
    if jobs::is_collecting(&app.state::<AppState>()) {
        return Ok(());
    }
    tauri::async_runtime::spawn(async move {
        let state = app.state::<AppState>();
        if let Err(e) = jobs::run_collect(&state).await
            && e.message != jobs::COLLECT_ALREADY_RUNNING
        {
            state.update_status(|s| s.last_error = Some(e.message));
        }
    });
    Ok(())
}

/// 手動の再試行。保留中に加え、上限まで失敗した要約も対象にする。
#[tauri::command]
pub fn retry_pending_digests(app: AppHandle) -> AppResult<()> {
    tauri::async_runtime::spawn(async move {
        let state = app.state::<AppState>();
        if let Err(e) = jobs::retry_all(&state).await {
            state.update_status(|s| s.last_error = Some(e.message));
        }
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::test_state;

    #[test]
    fn get_digests_lists_categories_or_one_topic() {
        let (state, _) = test_state();
        let views = digests_for(&state, "week", "2026-10-06", None).unwrap();
        assert_eq!(views.len(), 7);
        assert_eq!(views[0].label, "GitHub");
        assert!(views.iter().all(|v| v.digest.is_none()));

        let topic = state
            .with_store(|s| s.add_topic("介護", &["介護".to_string()]))
            .unwrap();
        let views = digests_for(&state, "day", "2026-10-06", Some(topic)).unwrap();
        assert_eq!(views.len(), 1);
        assert_eq!(views[0].label, "介護");
        assert_eq!(views[0].scope, DigestScope::Topic(topic));

        assert!(digests_for(&state, "year", "2026-10-06", None).is_err());
        assert!(digests_for(&state, "day", "not a date", None).is_err());
        assert!(digests_for(&state, "day", "2026-10-06", Some(999)).is_err());
    }

    #[test]
    fn get_digests_reports_item_count_for_the_period() {
        use infodesk_core::model::NewItem;
        let (state, _) = test_state();
        let fetched: chrono::DateTime<chrono::Utc> = "2026-10-06T03:00:00Z".parse().unwrap();
        let day = fetched.with_timezone(&state.tz).date_naive();
        let topic = state
            .with_store(|s| {
                let src = s
                    .list_sources()?
                    .into_iter()
                    .find(|x| x.category == Category::Medcare)
                    .unwrap();
                let items = ["認知症の新薬", "介護報酬", "Rust 2.0"]
                    .iter()
                    .enumerate()
                    .map(|(i, t)| NewItem {
                        url: format!("https://e.com/{i}"),
                        title: t.to_string(),
                        excerpt: String::new(),
                        author: None,
                        published_at: None,
                        score: None,
                    })
                    .collect::<Vec<_>>();
                s.upsert_items(&src, &items, &fetched)?;
                s.add_topic("介護", &["介護".to_string()])
            })
            .unwrap();
        let count = |views: &[DigestView], scope: DigestScope| {
            views.iter().find(|v| v.scope == scope).unwrap().item_count
        };
        let medcare = DigestScope::Category(Category::Medcare);
        let day_str = day.format("%Y-%m-%d").to_string();

        let views = digests_for(&state, "day", &day_str, None).unwrap();
        assert_eq!(count(&views, medcare.clone()), 3);
        assert_eq!(count(&views, DigestScope::Category(Category::Github)), 0);
        // 要約が無くても件数は出る。
        assert!(views.iter().all(|v| v.digest.is_none()));

        // 期間の途中の日付でも、その期間全体を数える。
        let views = digests_for(&state, "month", &day_str, None).unwrap();
        assert_eq!(count(&views, medcare.clone()), 3);

        // 取得日が期間外なら 0。
        let before = (day - chrono::Duration::days(1))
            .format("%Y-%m-%d")
            .to_string();
        let views = digests_for(&state, "day", &before, None).unwrap();
        assert_eq!(count(&views, medcare), 0);

        // トピックはキーワードに合う記事だけ。
        let views = digests_for(&state, "day", &day_str, Some(topic)).unwrap();
        assert_eq!(views[0].item_count, 1);
        let views = digests_for(&state, "day", &before, Some(topic)).unwrap();
        assert_eq!(views[0].item_count, 0);
    }

    #[test]
    fn regenerate_request_skips_empty_scopes_and_validates() {
        let scopes = vec![
            DigestScope::Category(Category::Github),
            DigestScope::Topic(2),
        ];
        let (period, date, got) = regenerate_request("week", "2026-10-06", scopes.clone())
            .unwrap()
            .unwrap();
        assert_eq!(period, Period::Week);
        assert_eq!(date, NaiveDate::from_ymd_opt(2026, 10, 6).unwrap());
        assert_eq!(got, scopes);
        // 対象が空ならジョブを起動しない。
        assert!(
            regenerate_request("day", "2026-10-06", vec![])
                .unwrap()
                .is_none()
        );
        assert!(regenerate_request("year", "2026-10-06", scopes.clone()).is_err());
        assert!(regenerate_request("day", "nope", scopes).is_err());
    }
}
