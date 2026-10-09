use chrono::Utc;
use serde_json::Value;
use tauri::State;

use crate::error::AppResult;
use crate::state::AppState;
use infodesk_core::model::ChatThread;

pub fn create_impl(state: &AppState, title: &str) -> AppResult<ChatThread> {
    state.with_store(|s| s.create_chat_thread(title, &Utc::now()))
}
pub fn list_impl(state: &AppState) -> AppResult<Vec<ChatThread>> {
    state.with_store(|s| s.list_chat_threads())
}
pub fn save_impl(state: &AppState, thread_id: i64, messages: &[Value]) -> AppResult<()> {
    state.with_store(|s| s.save_chat_messages(thread_id, messages, &Utc::now()))
}

#[tauri::command]
pub fn list_chat_threads(state: State<'_, AppState>) -> AppResult<Vec<ChatThread>> {
    list_impl(&state)
}
#[tauri::command]
pub fn create_chat_thread(state: State<'_, AppState>, title: String) -> AppResult<ChatThread> {
    create_impl(&state, &title)
}
#[tauri::command]
pub fn rename_chat_thread(state: State<'_, AppState>, id: i64, title: String) -> AppResult<()> {
    state.with_store(|s| s.rename_chat_thread(id, &title))
}
#[tauri::command]
pub fn delete_chat_thread(state: State<'_, AppState>, id: i64) -> AppResult<()> {
    state.with_store(|s| s.delete_chat_thread(id))
}
#[tauri::command]
pub fn get_chat_messages(state: State<'_, AppState>, thread_id: i64) -> AppResult<Vec<Value>> {
    state.with_store(|s| s.get_chat_messages(thread_id))
}
#[tauri::command]
pub fn save_chat_messages(
    state: State<'_, AppState>,
    thread_id: i64,
    messages: Vec<Value>,
) -> AppResult<()> {
    save_impl(&state, thread_id, &messages)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::test_state;
    use serde_json::json;

    #[test]
    fn create_save_and_list_round_trip() {
        let (state, _events) = test_state();
        let t = create_impl(&state, "新しいスレッド").unwrap();
        save_impl(
            &state,
            t.id,
            &[json!({ "id": "m1", "role": "user", "parts": [] })],
        )
        .unwrap();
        let threads = list_impl(&state).unwrap();
        assert_eq!(threads.len(), 1);
        assert_eq!(threads[0].title, "新しいスレッド");
        let err = save_impl(&state, 999, &[]).unwrap_err();
        assert!(err.message.contains("chat thread 999 not found"));
    }

    #[test]
    fn thread_serializes_with_snake_case_fields() {
        let (state, _events) = test_state();
        let t = create_impl(&state, "x").unwrap();
        let v = serde_json::to_value(&t).unwrap();
        assert!(v.get("created_at").is_some() && v.get("updated_at").is_some());
    }
}
