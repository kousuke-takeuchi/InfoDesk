use infodesk_core::model::Topic;
use tauri::State;

use crate::error::AppResult;
use crate::state::{AppState, EVENT_DIGESTS};

pub fn list_topics_impl(state: &AppState) -> AppResult<Vec<Topic>> {
    state.with_store(|s| s.list_topics())
}

pub fn add_topic_impl(state: &AppState, name: &str, keywords: Vec<String>) -> AppResult<Topic> {
    if name.trim().is_empty() {
        return Err("topic name is empty".to_string().into());
    }
    let topic = state.with_store(|s| {
        let id = s.add_topic(name, &keywords)?;
        Ok(s.get_topic(id)?.expect("just inserted"))
    })?;
    state.notify(EVENT_DIGESTS);
    Ok(topic)
}

pub fn update_topic_impl(
    state: &AppState,
    id: i64,
    name: &str,
    keywords: Vec<String>,
) -> AppResult<()> {
    if name.trim().is_empty() {
        return Err("topic name is empty".to_string().into());
    }
    state.with_store(|s| s.update_topic(id, name, &keywords))?;
    state.notify(EVENT_DIGESTS);
    Ok(())
}

pub fn delete_topic_impl(state: &AppState, id: i64) -> AppResult<()> {
    state.with_store(|s| s.delete_topic(id))?;
    state.notify(EVENT_DIGESTS);
    Ok(())
}

#[tauri::command]
pub fn list_topics(state: State<'_, AppState>) -> AppResult<Vec<Topic>> {
    list_topics_impl(&state)
}

#[tauri::command]
pub fn add_topic(
    state: State<'_, AppState>,
    name: String,
    keywords: Vec<String>,
) -> AppResult<Topic> {
    add_topic_impl(&state, &name, keywords)
}

#[tauri::command]
pub fn update_topic(
    state: State<'_, AppState>,
    id: i64,
    name: String,
    keywords: Vec<String>,
) -> AppResult<()> {
    update_topic_impl(&state, id, &name, keywords)
}

#[tauri::command]
pub fn delete_topic(state: State<'_, AppState>, id: i64) -> AppResult<()> {
    delete_topic_impl(&state, id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::test_state;

    #[test]
    fn topic_crud_notifies_digests_changed() {
        let (state, events) = test_state();
        let t = add_topic_impl(&state, "認知症", vec!["認知症".into(), "dementia".into()]).unwrap();
        assert_eq!(t.keywords.len(), 2);
        update_topic_impl(&state, t.id, "認知症ケア", vec!["介護".into()]).unwrap();
        let all = list_topics_impl(&state).unwrap();
        assert_eq!(all[0].name, "認知症ケア");
        delete_topic_impl(&state, t.id).unwrap();
        assert!(list_topics_impl(&state).unwrap().is_empty());
        assert_eq!(
            events
                .lock()
                .unwrap()
                .iter()
                .filter(|e| *e == EVENT_DIGESTS)
                .count(),
            3
        );
        assert!(add_topic_impl(&state, "   ", vec![]).is_err());
    }
}
