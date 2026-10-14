use tauri::{AppHandle, Manager, State};

use crate::error::{AppError, AppResult};
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
        sync::start_tailnet_if_needed(&state);
        sync::spawn_sync(&app);
    } else if sync::backend_is_nas(&state) {
        // 同期しないなら、NAS のための Tailscale も止める (別スレッドで)。
        drop(sync::stop_tailnet(&state));
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

/// 保存先 ("gdrive" か "nas") を切り替える。同じ保存先なら何もしない。
#[tauri::command]
pub fn sync_set_backend(app: AppHandle, backend: String) -> AppResult<SyncStatus> {
    let state = app.state::<AppState>();
    if sync::set_backend(&state, &backend)? {
        if backend == sync::BACKEND_NAS {
            sync::start_tailnet_if_needed(&state);
        } else {
            // Google ドライブには Tailscale を使わない。止めるのは別スレッドで。
            drop(sync::stop_tailnet(&state));
        }
        sync::spawn_sync(&app);
    }
    Ok(sync::status_now(&state))
}

/// NAS の URL、ユーザー名、パスワード (キーチェーン) を消す。
#[tauri::command]
pub fn sync_clear_nas(state: State<'_, AppState>) -> AppResult<SyncStatus> {
    sync::clear_nas(&state)?;
    Ok(sync::status_now(&state))
}

#[tauri::command]
pub fn sync_set_nas(
    app: AppHandle,
    url: String,
    user: String,
    password: Option<String>,
) -> AppResult<SyncStatus> {
    let state = app.state::<AppState>();
    sync::set_nas(&state, &url, &user, password.as_deref())?;
    sync::spawn_sync(&app);
    Ok(sync::status_now(&state))
}

/// 「Tailscale にログイン」が押された。ログイン画面を開くのはこの操作だけ。
/// 端末を始め、ログインが要るならブラウザーでログイン画面を開く。もうログイン待ちなら、
/// 同じログイン画面を開き直す (実行中の同期は止めない)。待たされうるので、別スレッドで行う。
#[tauri::command]
pub async fn tailnet_login(app: AppHandle) -> AppResult<SyncStatus> {
    let worker = app.clone();
    tauri::async_runtime::spawn_blocking(move || sync::login_tailnet(&worker.state::<AppState>()))
        .await
        .map_err(|e| AppError::from(format!("Tailscale の起動に失敗しました: {e}")))??;
    Ok(sync::status_now(&app.state::<AppState>()))
}

/// Tailscale からログアウトする。最長で 90 秒ほど待つので、別スレッドで行う。
#[tauri::command]
pub async fn tailnet_logout(app: AppHandle) -> AppResult<SyncStatus> {
    let tailnet = app.state::<AppState>().tailnet.clone();
    let result = tauri::async_runtime::spawn_blocking(move || tailnet.logout())
        .await
        .map_err(|e| AppError::from(format!("Tailscale のログアウトに失敗しました: {e}")))?;
    let state = app.state::<AppState>();
    sync::tailnet_changed(&state);
    result?;
    Ok(sync::status_now(&state))
}

/// ログアウトせずに、この Mac の Tailscale の状態を消す。起動が失敗し続けるときの手当て。
/// 状態ディレクトリの削除は待たされうるので、別スレッドで行う。
#[tauri::command]
pub async fn tailnet_forget(app: AppHandle) -> AppResult<SyncStatus> {
    let tailnet = app.state::<AppState>().tailnet.clone();
    let result = tauri::async_runtime::spawn_blocking(move || tailnet.forget())
        .await
        .map_err(|e| AppError::from(format!("Tailscale の状態を消せませんでした: {e}")))?;
    let state = app.state::<AppState>();
    sync::tailnet_changed(&state);
    result?;
    Ok(sync::status_now(&state))
}

/// NAS に一時ファイルを書き、読み、消して確かめる。結果は画面に出す文。
#[tauri::command]
pub async fn sync_test_nas(app: AppHandle) -> String {
    let state = app.state::<AppState>();
    sync::test_nas(&state).await
}
