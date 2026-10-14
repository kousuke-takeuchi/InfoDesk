pub mod keychain;
pub mod oauth;

use std::sync::Arc;
use std::sync::atomic::Ordering;

use chrono::{DateTime, Utc};
use infodesk_core::sync::drive::{DriveError, DriveStore};
use infodesk_core::sync::engine::{SyncError, SyncInput, compaction_due, run_sync};
use infodesk_core::sync::google::GoogleDrive;
use serde::Serialize;

use crate::error::AppResult;
use crate::state::{AppState, EVENT_ITEMS, EVENT_SYNC};

pub const SETTING_ENABLED: &str = "sync.enabled";
pub const SETTING_DEVICE_NAME: &str = "sync.device_name";
pub const SETTING_LAST_SYNC_AT: &str = "sync.last_sync_at";
pub const SETTING_LAST_COMPACTION_AT: &str = "sync.last_compaction_at";
pub const FALLBACK_DEVICE_NAME: &str = "この Mac";
pub const REAUTH_MESSAGE: &str = "Google の再ログインが必要です";

/// 同期の状態。収集やダイジェストの `last_error` とは別に持つ。
#[derive(Debug, Clone, Default, Serialize)]
pub struct SyncStatus {
    /// このビルドに OAuth の設定が入っているか。
    pub available: bool,
    pub signed_in: bool,
    pub account_email: Option<String>,
    pub enabled: bool,
    pub syncing: bool,
    pub last_sync_at: Option<DateTime<Utc>>,
    pub error: Option<String>,
    pub device_name: String,
}

/// ホスト名の候補から端末名を選ぶ。空なら既定の「この Mac」。
pub fn pick_device_name(candidate: Option<String>) -> String {
    candidate
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| FALLBACK_DEVICE_NAME.to_string())
}

fn command_output(program: &str, args: &[&str]) -> Option<String> {
    let out = std::process::Command::new(program)
        .args(args)
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// macOS のコンピューター名。取れなければホスト名、それも無理なら「この Mac」。
pub fn default_device_name() -> String {
    pick_device_name(
        command_output("scutil", &["--get", "ComputerName"])
            .filter(|s| !s.is_empty())
            .or_else(|| command_output("hostname", &[])),
    )
}

fn parse_time(v: Option<String>) -> Option<DateTime<Utc>> {
    v.and_then(|v| DateTime::parse_from_rfc3339(&v).ok())
        .map(|d| d.with_timezone(&Utc))
}

/// 設定から読める項目を `SyncStatus` に反映する。`signed_in`、`syncing`、`error` は触らない。
pub fn refresh_status(state: &AppState) {
    let (enabled, email, name, last) = state
        .with_store(|s| {
            Ok((
                s.get_setting(SETTING_ENABLED)?,
                s.get_setting(oauth::SETTING_ACCOUNT_EMAIL)?,
                s.get_setting(SETTING_DEVICE_NAME)?,
                s.get_setting(SETTING_LAST_SYNC_AT)?,
            ))
        })
        .unwrap_or_default();
    let name = pick_device_name(name).to_string();
    let name = if name == FALLBACK_DEVICE_NAME {
        default_device_name()
    } else {
        name
    };
    let mut st = state.sync.lock().expect("sync lock");
    st.available = oauth::oauth_config().is_some();
    st.enabled = enabled.as_deref() == Some("true");
    st.account_email = email.filter(|e| !e.is_empty());
    st.device_name = name;
    st.last_sync_at = parse_time(last);
}

/// 起動時に1回呼ぶ。サインイン済みかは、保存したメールアドレスの有無で決める。
pub fn init_status(state: &AppState) {
    refresh_status(state);
    let mut st = state.sync.lock().expect("sync lock");
    st.signed_in = st.available && st.account_email.is_some();
}

pub fn status_now(state: &AppState) -> SyncStatus {
    refresh_status(state);
    state.sync.lock().expect("sync lock").clone()
}

fn update(state: &AppState, f: impl FnOnce(&mut SyncStatus)) {
    {
        let mut st = state.sync.lock().unwrap_or_else(|e| e.into_inner());
        f(&mut st);
    }
    state.notify(EVENT_SYNC);
}

fn make_drive(state: &AppState) -> AppResult<Arc<dyn DriveStore>> {
    if let Some(d) = &state.drive_override {
        return Ok(d.clone());
    }
    Ok(Arc::new(GoogleDrive::new(Arc::new(oauth::token_source()?))))
}

/// ログイン完了を反映する。エラー表示は消す。
pub fn signed_in(state: &AppState, email: &str) {
    update(state, |s| {
        s.signed_in = true;
        s.account_email = Some(email.to_string()).filter(|e| !e.is_empty());
        s.error = None;
    });
}

pub fn signed_out(state: &AppState) {
    update(state, |s| {
        s.signed_in = false;
        s.account_email = None;
        s.error = None;
    });
}

pub fn set_enabled(state: &AppState, enabled: bool) -> AppResult<()> {
    state.with_store(|s| s.set_setting(SETTING_ENABLED, if enabled { "true" } else { "false" }))?;
    update(state, |s| {
        s.enabled = enabled;
        if !enabled {
            s.error = None;
        }
    });
    Ok(())
}

/// 端末名を保存する。空にすると既定 (ホスト名) に戻す。
pub fn set_device_name(state: &AppState, name: &str) -> AppResult<()> {
    let name = name.trim();
    state.with_store(|s| s.set_setting(SETTING_DEVICE_NAME, name))?;
    refresh_status(state);
    state.notify(EVENT_SYNC);
    Ok(())
}

/// 実行中フラグを戻す。
struct RunGuard<'a>(&'a AppState);

impl Drop for RunGuard<'_> {
    fn drop(&mut self) {
        self.0.sync_running.store(false, Ordering::SeqCst);
    }
}

/// 同期を投げて待たない。
pub fn spawn_sync(app: &tauri::AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        use tauri::Manager;
        let _ = sync_once(&app.state::<AppState>()).await;
    });
}

/// 同期を1回行う。無効、未ログイン、実行中ならすぐ戻る。失敗は `SyncStatus.error` にだけ記録する。
pub async fn sync_once(state: &AppState) -> AppResult<()> {
    let enabled = state
        .with_store(|s| s.get_setting(SETTING_ENABLED))?
        .as_deref()
        == Some("true");
    if !enabled || !state.sync.lock().expect("sync lock").signed_in {
        return Ok(());
    }
    if state
        .sync_running
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return Ok(());
    }
    let _guard = RunGuard(state);
    update(state, |s| s.syncing = true);
    let result = run_once(state).await;
    update(state, |s| s.syncing = false);
    result
}

async fn run_once(state: &AppState) -> AppResult<()> {
    let drive = match make_drive(state) {
        Ok(d) => d,
        Err(e) => {
            update(state, |s| s.error = Some(e.message.clone()));
            return Ok(());
        }
    };
    let now = Utc::now();
    let (device_id, db_id, last_compaction, daily) = state.with_store(|s| {
        let (d, b) = s.ensure_sync_ids()?;
        Ok((
            d,
            b,
            s.get_setting(SETTING_LAST_COMPACTION_AT)?,
            s.get_setting("schedule.daily_time")?,
        ))
    })?;
    let daily_time = crate::scheduler::ScheduleConfig::from_settings(|k| {
        if k == "schedule.daily_time" {
            daily.clone()
        } else {
            None
        }
    })
    .daily_time;
    let compact = compaction_due(parse_time(last_compaction), now, state.tz, daily_time);
    let writer = state.sync.lock().expect("sync lock").device_name.clone();
    let input = SyncInput {
        device_id,
        db_id,
        writer,
        now,
        tz: state.tz,
        compact,
    };
    match run_sync(&state.store, drive.as_ref(), &input).await {
        Ok(report) => {
            let stamp = now.to_rfc3339();
            let _ = state.with_store(|s| {
                s.set_setting(SETTING_LAST_SYNC_AT, &stamp)?;
                if compact {
                    s.set_setting(SETTING_LAST_COMPACTION_AT, &stamp)?;
                }
                Ok(())
            });
            update(state, |s| {
                s.last_sync_at = Some(now);
                s.error = None;
            });
            if report.applied_ops > 0 {
                state.notify(EVENT_ITEMS);
            }
        }
        Err(SyncError::Drive(DriveError::AuthExpired)) => {
            let _ = state.with_store(|s| s.set_setting(oauth::SETTING_ACCOUNT_EMAIL, ""));
            update(state, |s| {
                s.signed_in = false;
                s.account_email = None;
                s.error = Some(REAUTH_MESSAGE.to_string());
            });
        }
        Err(e) => update(state, |s| s.error = Some(e.to_string())),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{EVENT_STATUS, test_state};
    use infodesk_core::sync::drive::MemoryDrive;

    fn ready(state: &mut AppState) -> Arc<MemoryDrive> {
        let drive = Arc::new(MemoryDrive::new());
        state.drive_override = Some(drive.clone());
        state
            .with_store(|s| s.set_setting(SETTING_ENABLED, "true"))
            .unwrap();
        state.sync.lock().unwrap().signed_in = true;
        drive
    }

    #[test]
    fn device_name_falls_back() {
        assert_eq!(pick_device_name(None), "この Mac");
        assert_eq!(pick_device_name(Some("  ".into())), "この Mac");
        assert_eq!(pick_device_name(Some(" Studio \n".into())), "Studio");
    }

    #[test]
    fn commands_update_status_and_settings() {
        let (state, events) = test_state();
        set_enabled(&state, true).unwrap();
        assert!(state.sync.lock().unwrap().enabled);
        assert_eq!(
            state
                .with_store(|s| s.get_setting(SETTING_ENABLED))
                .unwrap()
                .as_deref(),
            Some("true")
        );
        set_device_name(&state, "  Studio ").unwrap();
        assert_eq!(status_now(&state).device_name, "Studio");
        signed_in(&state, "a@b.c");
        assert_eq!(
            state.sync.lock().unwrap().account_email.as_deref(),
            Some("a@b.c")
        );
        signed_out(&state);
        assert!(!state.sync.lock().unwrap().signed_in);
        assert!(events.lock().unwrap().contains(&EVENT_SYNC.to_string()));
    }

    #[tokio::test]
    async fn successful_sync_records_time_and_clears_error() {
        let (mut state, events) = test_state();
        let drive = ready(&mut state);
        state.sync.lock().unwrap().error = Some("old".into());
        sync_once(&state).await.unwrap();
        let st = state.sync.lock().unwrap().clone();
        assert!(st.last_sync_at.is_some());
        assert_eq!(st.error, None);
        assert!(!st.syncing);
        assert!(drive.names().contains(&"manifest.json".to_string()));
        let stored = state
            .with_store(|s| s.get_setting(SETTING_LAST_SYNC_AT))
            .unwrap();
        assert!(stored.is_some());
        assert!(events.lock().unwrap().contains(&EVENT_SYNC.to_string()));
    }

    #[tokio::test]
    async fn sync_errors_do_not_touch_last_error() {
        let (mut state, events) = test_state();
        let drive = ready(&mut state);
        *drive.fail_next.lock().unwrap() = Some(DriveError::Other("boom".into()));
        state.update_status(|s| s.set_error(crate::state::ErrorOrigin::Job, "digest failed"));
        events.lock().unwrap().clear();
        sync_once(&state).await.unwrap();
        assert_eq!(state.status().last_error.as_deref(), Some("digest failed"));
        let err = state.sync.lock().unwrap().error.clone().unwrap();
        assert!(err.contains("boom"), "{err}");
        assert!(!events.lock().unwrap().contains(&EVENT_STATUS.to_string()));
    }

    #[tokio::test]
    async fn auth_expired_signs_out_and_says_so() {
        let (mut state, _) = test_state();
        let drive = ready(&mut state);
        state
            .with_store(|s| s.set_setting(oauth::SETTING_ACCOUNT_EMAIL, "a@b.c"))
            .unwrap();
        *drive.fail_next.lock().unwrap() = Some(DriveError::AuthExpired);
        sync_once(&state).await.unwrap();
        let st = state.sync.lock().unwrap().clone();
        assert!(!st.signed_in);
        assert_eq!(st.error.as_deref(), Some("Google の再ログインが必要です"));
        assert_eq!(state.status().last_error, None);
    }

    #[tokio::test]
    async fn sync_is_skipped_when_disabled_or_signed_out() {
        let (mut state, _) = test_state();
        let drive = ready(&mut state);
        *drive.fail_next.lock().unwrap() = Some(DriveError::Other("boom".into()));
        state
            .with_store(|s| s.set_setting(SETTING_ENABLED, "false"))
            .unwrap();
        sync_once(&state).await.unwrap();
        state
            .with_store(|s| s.set_setting(SETTING_ENABLED, "true"))
            .unwrap();
        state.sync.lock().unwrap().signed_in = false;
        sync_once(&state).await.unwrap();
        // どちらも Drive に触れていないので、仕込んだ失敗がそのまま残る。
        assert!(drive.fail_next.lock().unwrap().is_some());
        assert_eq!(state.sync.lock().unwrap().error, None);
        assert_eq!(state.sync.lock().unwrap().last_sync_at, None);
    }

    #[tokio::test]
    async fn a_running_sync_is_not_started_twice() {
        let (mut state, _) = test_state();
        let drive = ready(&mut state);
        *drive.fail_next.lock().unwrap() = Some(DriveError::Other("boom".into()));
        state.sync_running.store(true, Ordering::SeqCst);
        sync_once(&state).await.unwrap();
        assert!(drive.fail_next.lock().unwrap().is_some());
        assert!(
            state.sync_running.load(Ordering::SeqCst),
            "not ours to reset"
        );
        // 終わった後は、次の同期が走る。
        state.sync_running.store(false, Ordering::SeqCst);
        sync_once(&state).await.unwrap();
        assert!(drive.fail_next.lock().unwrap().is_none());
        assert!(!state.sync_running.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn imported_changes_emit_items_changed() {
        use infodesk_core::model::{ItemFilter, NewItem};
        use infodesk_core::sync::format::{Field, Op, OpsFile, ops_file_name};
        let (mut state, events) = test_state();
        let drive = ready(&mut state);
        let id = state
            .with_store(|s| {
                let src = s.list_sources()?.remove(0);
                let item = NewItem {
                    url: "https://e.com/1".into(),
                    title: "t".into(),
                    excerpt: String::new(),
                    author: None,
                    published_at: None,
                    score: None,
                    image_url: None,
                };
                s.upsert_items(&src, &[item], &Utc::now())?;
                Ok(s.query_items(&ItemFilter::default())?[0].id)
            })
            .unwrap();
        // 何も変わらない同期では記事の通知を出さない。
        sync_once(&state).await.unwrap();
        assert!(!events.lock().unwrap().contains(&EVENT_ITEMS.to_string()));
        let file = OpsFile {
            device: "phone".into(),
            ops: vec![Op {
                item_id: id,
                field: Field::Starred,
                value: true,
                at: "2999-01-01T00:00:00.000Z".into(),
                device: "phone".into(),
            }],
        };
        drive.put(
            &ops_file_name("phone", Utc::now(), 1),
            serde_json::to_vec(&file).unwrap(),
        );
        sync_once(&state).await.unwrap();
        assert!(events.lock().unwrap().contains(&EVENT_ITEMS.to_string()));
        assert!(
            state
                .with_store(|s| Ok(s.get_item(id)?.unwrap().starred))
                .unwrap()
        );
    }
}
