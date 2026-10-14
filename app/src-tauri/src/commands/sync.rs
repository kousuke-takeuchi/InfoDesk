use tauri::{AppHandle, Manager, State};

use crate::error::AppResult;
use crate::state::AppState;
use crate::sync::{self, SyncStatus};

#[tauri::command]
pub fn sync_status(state: State<'_, AppState>) -> SyncStatus {
    sync::status_now(&state)
}

/// ブラウザーでサインインする。完了後、同期が有効なら1回投げる。
#[tauri::command]
pub async fn sync_login(app: AppHandle) -> AppResult<SyncStatus> {
    let email = sync::oauth::login(&app).await?;
    let state = app.state::<AppState>();
    sync::signed_in(&state, &email);
    sync::spawn_sync(&app);
    Ok(sync::status_now(&state))
}

#[tauri::command]
pub async fn sync_logout(app: AppHandle) -> AppResult<SyncStatus> {
    sync::oauth::logout(&app).await;
    let state = app.state::<AppState>();
    sync::signed_out(&state);
    Ok(sync::status_now(&state))
}

#[tauri::command]
pub fn sync_set_enabled(app: AppHandle, enabled: bool) -> AppResult<SyncStatus> {
    let state = app.state::<AppState>();
    sync::set_enabled(&state, enabled)?;
    if enabled {
        sync::spawn_sync(&app);
    }
    Ok(sync::status_now(&state))
}

#[tauri::command]
pub fn sync_set_device_name(state: State<'_, AppState>, name: String) -> AppResult<SyncStatus> {
    sync::set_device_name(&state, &name)?;
    Ok(sync::status_now(&state))
}

#[tauri::command]
pub fn sync_now(app: AppHandle) {
    sync::spawn_sync(&app);
}
