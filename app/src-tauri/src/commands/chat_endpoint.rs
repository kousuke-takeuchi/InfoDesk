use tauri::{AppHandle, Manager, State};

use crate::sidecar::ChatEndpoint;
use crate::state::AppState;

#[tauri::command]
pub fn chat_endpoint(state: State<'_, AppState>) -> ChatEndpoint {
    state.chat.endpoint()
}

#[tauri::command]
pub fn restart_chat(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        app.state::<AppState>().chat.restart(&app).await;
    });
}
