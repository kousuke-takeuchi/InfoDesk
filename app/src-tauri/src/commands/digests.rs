use chrono::NaiveDate;
use infodesk_core::model::{Category, Digest, DigestScope, Period};
use infodesk_core::period::period_start;
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
    state.with_store(|s| {
        scopes
            .into_iter()
            .map(|(scope, label)| {
                Ok(DigestView {
                    digest: s.get_digest(period, start, &scope)?,
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

/// ジョブを起動して即座に返す。進行はイベントで通知される。
#[tauri::command]
pub fn regenerate_digest(
    app: AppHandle,
    period: String,
    date: String,
    scope: DigestScope,
) -> AppResult<()> {
    let period = parse_period(&period)?;
    let date = parse_date(&date)?;
    tauri::async_runtime::spawn(async move {
        let state = app.state::<AppState>();
        if let Err(e) = jobs::run_digests(&state, period, date, &[scope], true).await {
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
}
