pub mod keychain;
pub mod oauth;

use std::sync::Arc;
use std::sync::OnceLock;
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
/// 通信できなかった失敗 (`DriveError::Retryable`) に出す文。詳細は画面に出さない。
pub const NETWORK_MESSAGE: &str = "ネットワークに接続できません。次の同期で再試行します。";

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
/// 外部コマンドを呼ぶので、結果は1回だけ調べて覚える。
pub fn default_device_name() -> String {
    static NAME: OnceLock<String> = OnceLock::new();
    NAME.get_or_init(|| {
        pick_device_name(
            command_output("scutil", &["--get", "ComputerName"])
                .filter(|s| !s.is_empty())
                .or_else(|| command_output("hostname", &[])),
        )
    })
    .clone()
}

fn parse_time(v: Option<String>) -> Option<DateTime<Utc>> {
    v.and_then(|v| DateTime::parse_from_rfc3339(&v).ok())
        .map(|d| d.with_timezone(&Utc))
}

/// 設定から読める項目を `SyncStatus` に反映する。`signed_in`、`syncing`、`error` は触らない。
pub fn refresh_status(state: &AppState) {
    let read = state.with_store(|s| {
        Ok((
            s.get_setting(SETTING_ENABLED)?,
            s.get_setting(oauth::SETTING_ACCOUNT_EMAIL)?,
            s.get_setting(SETTING_DEVICE_NAME)?,
            s.get_setting(SETTING_LAST_SYNC_AT)?,
        ))
    });
    let (enabled, email, name, last) = match read {
        Ok(v) => v,
        Err(e) => {
            let mut st = state.sync.lock().unwrap_or_else(|e| e.into_inner());
            st.available = oauth::oauth_config().is_some();
            st.error = Some(e.message);
            return;
        }
    };
    let name = pick_device_name(name).to_string();
    let name = if name == FALLBACK_DEVICE_NAME {
        default_device_name()
    } else {
        name
    };
    let mut st = state.sync.lock().unwrap_or_else(|e| e.into_inner());
    st.available = oauth::oauth_config().is_some();
    st.enabled = enabled.as_deref() == Some("true");
    st.account_email = email.filter(|e| !e.is_empty());
    st.device_name = name;
    st.last_sync_at = parse_time(last);
}

/// 起動時に1回呼ぶ。サインイン済みかは、保存したメールアドレスの有無で決める。
pub fn init_status(state: &AppState) {
    refresh_status(state);
    let mut st = state.sync.lock().unwrap_or_else(|e| e.into_inner());
    st.signed_in = st.available && st.account_email.is_some();
}

pub fn status_now(state: &AppState) -> SyncStatus {
    refresh_status(state);
    state.sync.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

fn update(state: &AppState, f: impl FnOnce(&mut SyncStatus)) {
    {
        let mut st = state.sync.lock().unwrap_or_else(|e| e.into_inner());
        f(&mut st);
    }
    state.notify(EVENT_SYNC);
}

/// 本物の Google ドライブクライアントを作る。キーチェーンを読むので、頻繁には呼ばない。
pub fn google_drive() -> AppResult<Arc<dyn DriveStore>> {
    Ok(Arc::new(GoogleDrive::new(Arc::new(oauth::token_source()?))))
}

/// 作ったクライアントを覚えておく。サインインやサインアウトで捨てる。
fn make_drive(state: &AppState, generation: u64) -> AppResult<Arc<dyn DriveStore>> {
    let mut cache = state.drive_cache.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(d) = cache.as_ref() {
        return Ok(d.clone());
    }
    let drive = (state.drive_factory)()?;
    // 作っている間にサインインし直されていたら、古い認証のものは覚えない。
    if state.sync_generation.load(Ordering::SeqCst) == generation {
        *cache = Some(drive.clone());
    }
    Ok(drive)
}

/// 認証が変わったので、覚えているクライアントと、実行中の同期の結果を捨てる。
fn reset_auth(state: &AppState) {
    state.sync_generation.fetch_add(1, Ordering::SeqCst);
    *state.drive_cache.lock().unwrap_or_else(|e| e.into_inner()) = None;
}

/// ログイン完了を反映する。エラー表示は消す。
pub fn signed_in(state: &AppState, email: &str) {
    reset_auth(state);
    update(state, |s| {
        s.signed_in = true;
        s.account_email = Some(email.to_string()).filter(|e| !e.is_empty());
        s.error = None;
    });
}

pub fn signed_out(state: &AppState) {
    reset_auth(state);
    update(state, |s| {
        s.signed_in = false;
        s.account_email = None;
        s.error = None;
    });
}

/// 有効にするときは、無効の間に残った未送信の操作を捨て、次の同期でスナップショットを作り直させる。
/// 印の状態は記事の `*_changed_at` が持っているので、そのスナップショットで届く。
pub fn set_enabled(state: &AppState, enabled: bool) -> AppResult<()> {
    state.with_store(|s| {
        if enabled {
            s.discard_pending_ops()?;
            s.set_setting(SETTING_LAST_COMPACTION_AT, "")?;
        }
        s.set_setting(SETTING_ENABLED, if enabled { "true" } else { "false" })
    })?;
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

/// 実行中の印。破棄すると `syncing` と実行中フラグを戻す (パニックでも戻る)。
struct RunGuard<'a>(&'a AppState);

impl<'a> RunGuard<'a> {
    fn begin(state: &'a AppState) -> Self {
        update(state, |s| s.syncing = true);
        Self(state)
    }
}

impl Drop for RunGuard<'_> {
    fn drop(&mut self) {
        // 先に状態を戻す。実行中フラグを先に戻すと、次の同期の `syncing` を上書きしうる。
        update(self.0, |s| s.syncing = false);
        self.0.sync_running.store(false, Ordering::SeqCst);
    }
}

/// 同期を投げて待たない。失敗は `sync_once` が `SyncStatus.error` に記録済み。
pub fn spawn_sync(app: &tauri::AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        use tauri::Manager;
        if let Err(e) = sync_once(&app.state::<AppState>()).await {
            eprintln!("sync failed: {}", e.message);
        }
    });
}

fn record_error(state: &AppState, message: &str) {
    update(state, |s| s.error = Some(message.to_string()));
}

/// 同期を1回行う。無効、未ログインならすぐ戻る。実行中なら、終わったあとにもう1回走らせる依頼を残す
/// (何度頼まれても1回に束ねる)。失敗は `SyncStatus.error` にだけ記録する。
pub async fn sync_once(state: &AppState) -> AppResult<()> {
    loop {
        let enabled = match state.with_store(|s| s.get_setting(SETTING_ENABLED)) {
            Ok(v) => v.as_deref() == Some("true"),
            Err(e) => {
                record_error(state, &e.message);
                return Err(e);
            }
        };
        if !enabled
            || !state
                .sync
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .signed_in
        {
            return Ok(());
        }
        if state
            .sync_running
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            state.sync_rerun.store(true, Ordering::SeqCst);
            return Ok(());
        }
        let result = {
            let _guard = RunGuard::begin(state);
            run_once(state).await
        };
        if !state.sync_rerun.swap(false, Ordering::SeqCst) {
            return result;
        }
    }
}

async fn run_once(state: &AppState) -> AppResult<()> {
    let generation = state.sync_generation.load(Ordering::SeqCst);
    let stale = || state.sync_generation.load(Ordering::SeqCst) != generation;
    let drive = match make_drive(state, generation) {
        Ok(d) => d,
        Err(e) => {
            if !stale() {
                record_error(state, &e.message);
            }
            return Ok(());
        }
    };
    let now = Utc::now();
    let prepared = state.with_store(|s| {
        let (d, b) = s.ensure_sync_ids()?;
        Ok((
            d,
            b,
            s.get_setting(SETTING_LAST_COMPACTION_AT)?,
            s.get_setting("schedule.daily_time")?,
        ))
    });
    let (device_id, db_id, last_compaction, daily) = match prepared {
        Ok(v) => v,
        Err(e) => {
            record_error(state, &e.message);
            return Err(e);
        }
    };
    let daily_time = crate::scheduler::ScheduleConfig::from_settings(|k| {
        if k == "schedule.daily_time" {
            daily.clone()
        } else {
            None
        }
    })
    .daily_time;
    // まだ一度もまとめていなければ、時刻を待たずにまとめて、今の印を届ける。
    let last_compaction = parse_time(last_compaction);
    let compact =
        last_compaction.is_none() || compaction_due(last_compaction, now, state.tz, daily_time);
    let writer = state
        .sync
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .device_name
        .clone();
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
                if report.compacted {
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
        Err(err) => {
            // 取り込みのあとで失敗しても、取り込み済みの変更は画面に出す必要がある。
            state.notify(EVENT_ITEMS);
            {
                // 古さの確認と書き換えを同じロックの中で行い、間に入ったサインインを消さない。
                let mut st = state.sync.lock().unwrap_or_else(|e| e.into_inner());
                if stale() {
                    // 実行中にサインインし直された。古い認証の結果は捨てる。
                    return Ok(());
                }
                match err {
                    SyncError::Drive(DriveError::AuthExpired) => {
                        reset_auth(state);
                        let _ =
                            state.with_store(|s| s.set_setting(oauth::SETTING_ACCOUNT_EMAIL, ""));
                        st.signed_in = false;
                        st.account_email = None;
                        st.error = Some(REAUTH_MESSAGE.to_string());
                    }
                    SyncError::Drive(DriveError::Retryable(_)) => {
                        st.error = Some(NETWORK_MESSAGE.to_string());
                    }
                    other => st.error = Some(other.to_string()),
                }
            }
            state.notify(EVENT_SYNC);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{EVENT_STATUS, test_state};
    use infodesk_core::sync::drive::{MemoryDrive, RemoteFile};
    use std::sync::Mutex;
    use std::sync::atomic::AtomicUsize;

    /// 差し込んだ Drive を使うようにして、同期を有効・ログイン済みにする。作った回数を返す。
    fn use_drive(state: &mut AppState, drive: Arc<dyn DriveStore>) -> Arc<AtomicUsize> {
        let made = Arc::new(AtomicUsize::new(0));
        let counter = made.clone();
        state.drive_factory = Arc::new(move || {
            counter.fetch_add(1, Ordering::SeqCst);
            Ok(drive.clone())
        });
        state
            .with_store(|s| s.set_setting(SETTING_ENABLED, "true"))
            .unwrap();
        state.sync.lock().unwrap().signed_in = true;
        made
    }

    fn ready(state: &mut AppState) -> Arc<MemoryDrive> {
        let drive = Arc::new(MemoryDrive::new());
        use_drive(state, drive.clone());
        drive
    }

    /// `list` で1回譲る Drive。他の処理が割り込める。呼ばれた回数を数え、仕込んだ失敗を返す。
    #[derive(Default)]
    struct Slow {
        inner: MemoryDrive,
        lists: AtomicUsize,
        fail: Mutex<Option<DriveError>>,
    }

    #[async_trait::async_trait]
    impl DriveStore for Slow {
        async fn list(&self) -> Result<Vec<RemoteFile>, DriveError> {
            self.lists.fetch_add(1, Ordering::SeqCst);
            tokio::task::yield_now().await;
            if let Some(e) = self.fail.lock().unwrap().take() {
                return Err(e);
            }
            self.inner.list().await
        }
        async fn download(&self, id: &str) -> Result<Vec<u8>, DriveError> {
            self.inner.download(id).await
        }
        async fn create(&self, n: &str, b: Vec<u8>, m: &str) -> Result<RemoteFile, DriveError> {
            self.inner.create(n, b, m).await
        }
        async fn update(&self, id: &str, b: Vec<u8>, m: &str) -> Result<RemoteFile, DriveError> {
            self.inner.update(id, b, m).await
        }
        async fn delete(&self, id: &str) -> Result<(), DriveError> {
            self.inner.delete(id).await
        }
    }

    #[tokio::test]
    async fn the_drive_client_is_made_once_and_again_after_auth_changes() {
        let (mut state, _) = test_state();
        let drive = Arc::new(MemoryDrive::new());
        let made = use_drive(&mut state, drive.clone());
        sync_once(&state).await.unwrap();
        sync_once(&state).await.unwrap();
        assert_eq!(made.load(Ordering::SeqCst), 1);
        signed_out(&state);
        signed_in(&state, "a@b.c");
        sync_once(&state).await.unwrap();
        assert_eq!(made.load(Ordering::SeqCst), 2);
        *drive.fail_next.lock().unwrap() = Some(DriveError::AuthExpired);
        sync_once(&state).await.unwrap();
        signed_in(&state, "a@b.c");
        sync_once(&state).await.unwrap();
        assert_eq!(made.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn failure_after_import_still_emits_items_changed() {
        let (mut state, events) = test_state();
        let drive = ready(&mut state);
        phone_stars_an_item(&state, &drive);
        *drive.fail_on_name.lock().unwrap() = Some("meta".into());
        sync_once(&state).await.unwrap();
        assert!(state.sync.lock().unwrap().error.is_some());
        assert!(events.lock().unwrap().contains(&EVENT_ITEMS.to_string()));
    }

    #[tokio::test]
    async fn store_errors_reach_the_sync_error() {
        let (mut state, _) = test_state();
        ready(&mut state);
        let store = &state.store;
        let _ = std::thread::scope(|sc| {
            sc.spawn(|| {
                let _held = store.lock().unwrap();
                panic!("poison");
            })
            .join()
        });
        assert!(sync_once(&state).await.is_err());
        let err = state.sync.lock().unwrap().error.clone();
        assert!(err.unwrap().contains("poisoned"));
        let mut state2 = test_state().0;
        ready(&mut state2);
        let _ = std::thread::scope(|sc| {
            sc.spawn(|| {
                let _held = state2.store.lock().unwrap();
                panic!("poison");
            })
            .join()
        });
        refresh_status(&state2);
        assert!(state2.sync.lock().unwrap().error.is_some());
    }

    #[test]
    fn a_panic_does_not_leave_the_status_syncing() {
        let (state, events) = test_state();
        state.sync_running.store(true, Ordering::SeqCst);
        let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = RunGuard::begin(&state);
            assert!(state.sync.lock().unwrap().syncing);
            panic!("pass blew up");
        }));
        assert!(caught.is_err());
        assert!(!state.sync.lock().unwrap().syncing);
        assert!(!state.sync_running.load(Ordering::SeqCst));
        assert!(events.lock().unwrap().contains(&EVENT_SYNC.to_string()));
    }

    #[tokio::test]
    async fn requests_during_a_pass_queue_exactly_one_more() {
        let (mut state, _) = test_state();
        let drive = Arc::new(Slow::default());
        use_drive(&mut state, drive.clone());
        let (a, b, c) = tokio::join!(sync_once(&state), sync_once(&state), sync_once(&state));
        a.unwrap();
        b.unwrap();
        c.unwrap();
        assert_eq!(drive.lists.load(Ordering::SeqCst), 2);
        assert!(!state.sync_rerun.load(Ordering::SeqCst));
        assert!(!state.sync_running.load(Ordering::SeqCst));
        sync_once(&state).await.unwrap();
        assert_eq!(drive.lists.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn a_pass_that_outlives_a_sign_out_discards_its_auth_error() {
        let (mut state, _) = test_state();
        let drive = Arc::new(Slow::default());
        use_drive(&mut state, drive.clone());
        *drive.fail.lock().unwrap() = Some(DriveError::AuthExpired);
        let relogin = async {
            signed_out(&state);
            signed_in(&state, "new@b.c");
        };
        let (r, ()) = tokio::join!(sync_once(&state), relogin);
        r.unwrap();
        let st = state.sync.lock().unwrap().clone();
        assert!(st.signed_in, "the new sign-in survives");
        assert_eq!(st.error, None);
        assert_eq!(st.account_email.as_deref(), Some("new@b.c"));
    }

    #[test]
    fn device_name_is_looked_up_once() {
        assert_eq!(default_device_name(), default_device_name());
    }

    fn phone_stars_an_item(state: &AppState, drive: &MemoryDrive) -> i64 {
        use infodesk_core::model::{ItemFilter, NewItem};
        use infodesk_core::sync::format::{Field, Op, OpsFile, ops_file_name};
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
        id
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
    async fn network_errors_show_a_fixed_message_without_detail() {
        let (mut state, _) = test_state();
        let drive = ready(&mut state);
        *drive.fail_next.lock().unwrap() = Some(DriveError::Retryable(
            "error sending request: tcp connect".into(),
        ));
        sync_once(&state).await.unwrap();
        let err = state.sync.lock().unwrap().error.clone();
        assert_eq!(
            err.as_deref(),
            Some("ネットワークに接続できません。次の同期で再試行します。")
        );
    }

    /// 記事を1件足し、同期が無効の間に既読にする。
    fn read_while_sync_is_off(state: &AppState) -> i64 {
        use infodesk_core::model::{ItemFilter, NewItem};
        state
            .with_store(|s| {
                s.set_setting(SETTING_ENABLED, "false")?;
                let src = s.list_sources()?.remove(0);
                let item = NewItem {
                    url: "https://e.com/old".into(),
                    title: "t".into(),
                    excerpt: String::new(),
                    author: None,
                    published_at: None,
                    score: None,
                    image_url: None,
                };
                s.upsert_items(&src, &[item], &Utc::now())?;
                let id = s.query_items(&ItemFilter::default())?[0].id;
                s.set_read(id, true)?;
                Ok(id)
            })
            .unwrap()
    }

    #[tokio::test]
    async fn the_first_sync_publishes_marks_even_before_the_daily_time() {
        use infodesk_core::sync::format::{StateSnapshot, gunzip_json, gzip_json};
        let (mut state, _) = test_state();
        let drive = ready(&mut state);
        let id = read_while_sync_is_off(&state);
        state
            .with_store(|s| {
                s.set_setting(SETTING_ENABLED, "true")?;
                s.set_setting("schedule.daily_time", "23:59")
            })
            .unwrap();
        // Drive にはもう (空の) スナップショットがある。
        let empty = StateSnapshot {
            covers: Vec::new(),
            state: Default::default(),
        };
        drive.put("state-snapshot.json.gz", gzip_json(&empty).unwrap());
        sync_once(&state).await.unwrap();
        let f = drive
            .list()
            .await
            .unwrap()
            .into_iter()
            .find(|f| f.name == "state-snapshot.json.gz")
            .unwrap();
        let snap: StateSnapshot = gunzip_json(&drive.download(&f.id).await.unwrap()).unwrap();
        assert!(snap.state[&id].read.as_ref().expect("published").value);
        let last = state
            .with_store(|s| s.get_setting(SETTING_LAST_COMPACTION_AT))
            .unwrap();
        assert!(parse_time(last).is_some());
    }

    #[test]
    fn enabling_sync_drops_unsent_ops_and_asks_for_a_fresh_snapshot() {
        let (state, _) = test_state();
        let id = read_while_sync_is_off(&state);
        state
            .with_store(|s| {
                s.set_setting(SETTING_ENABLED, "true")?;
                s.set_starred(id, true)?;
                s.set_setting(SETTING_ENABLED, "false")?;
                s.set_setting(SETTING_LAST_COMPACTION_AT, &Utc::now().to_rfc3339())
            })
            .unwrap();
        let (device, _) = state.with_store(|s| s.ensure_sync_ids()).unwrap();
        assert_eq!(
            state
                .with_store(|s| s.pending_local_ops(&device))
                .unwrap()
                .len(),
            1
        );
        set_enabled(&state, true).unwrap();
        assert!(
            state
                .with_store(|s| s.pending_local_ops(&device))
                .unwrap()
                .is_empty()
        );
        let last = state
            .with_store(|s| s.get_setting(SETTING_LAST_COMPACTION_AT))
            .unwrap();
        assert_eq!(parse_time(last), None);
        // 無効にするときは何も消さない。
        state.with_store(|s| s.set_read(id, false)).unwrap();
        set_enabled(&state, false).unwrap();
        assert_eq!(
            state
                .with_store(|s| s.pending_local_ops(&device))
                .unwrap()
                .len(),
            1
        );
    }

    #[tokio::test]
    async fn imported_changes_emit_items_changed() {
        let (mut state, events) = test_state();
        let drive = ready(&mut state);
        // 何も変わらない同期では記事の通知を出さない。
        sync_once(&state).await.unwrap();
        assert!(!events.lock().unwrap().contains(&EVENT_ITEMS.to_string()));
        let id = phone_stars_an_item(&state, &drive);
        sync_once(&state).await.unwrap();
        assert!(events.lock().unwrap().contains(&EVENT_ITEMS.to_string()));
        assert!(
            state
                .with_store(|s| Ok(s.get_item(id)?.unwrap().starred))
                .unwrap()
        );
    }
}
