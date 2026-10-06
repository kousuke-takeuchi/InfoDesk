use tauri::State;

use crate::state::{AppState, AppStatus};

#[tauri::command]
pub fn get_status(state: State<'_, AppState>) -> AppStatus {
    state.status()
}
