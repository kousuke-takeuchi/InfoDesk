//! アプリなしで NAS へ1回同期する。自宅の k3s で動く `infodesk tick` と `infodesk sync` が使う。
//! NAS へは LAN で直接つなぐ。パスワードは環境変数から読み、DB には置かない。

use std::sync::{Arc, Mutex};

use chrono::{DateTime, FixedOffset, Utc};

use super::drive::{DriveError, DriveStore};
use super::engine::{SyncError, SyncInput, SyncReport, compaction_due, run_sync};
use super::nas::*;
use super::webdav::{WebDavConfig, WebDavStore};
use crate::schedule::ScheduleConfig;
use crate::store::Store;

pub const NAS_PASSWORD_ENV: &str = "INFODESK_NAS_PASSWORD";
/// `sync.device_name` が空のときの書き手の名前。
pub const DEFAULT_SERVER_NAME: &str = "infodesk-server";

pub struct HeadlessEnv {
    pub nas_password: Option<String>,
    pub tz: FixedOffset,
}

impl std::fmt::Debug for HeadlessEnv {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HeadlessEnv")
            .field(
                "nas_password",
                &self.nas_password.as_ref().map(|_| "<redacted>"),
            )
            .field("tz", &self.tz)
            .finish()
    }
}

impl HeadlessEnv {
    pub fn from_env(tz: FixedOffset) -> Self {
        Self {
            nas_password: std::env::var(NAS_PASSWORD_ENV).ok(),
            tz,
        }
    }
}

pub type MakeDrive = dyn Fn(WebDavConfig) -> Result<Arc<dyn DriveStore>, DriveError> + Send + Sync;

/// 本物の WebDAV クライアント。
pub fn webdav(cfg: WebDavConfig) -> Result<Arc<dyn DriveStore>, DriveError> {
    Ok(Arc::new(WebDavStore::new(cfg)?))
}

#[derive(Debug)]
pub enum SyncOutcome {
    /// 同期が無効か、保存先が NAS ではない。
    Disabled,
    Synced(SyncReport),
}

struct Prepared {
    url: String,
    user: String,
    device_id: String,
    db_id: String,
    writer: String,
    last_compaction: Option<DateTime<Utc>>,
    schedule: ScheduleConfig,
}

fn lock(store: &Mutex<Store>) -> std::sync::MutexGuard<'_, Store> {
    store.lock().unwrap_or_else(|e| e.into_inner())
}

/// 設定を読む。無効なら `None`。
fn prepare(store: &Mutex<Store>) -> crate::Result<Option<Prepared>> {
    let s = lock(store);
    let text = |k: &str| -> crate::Result<String> {
        Ok(s.get_setting(k)?.unwrap_or_default().trim().to_string())
    };
    if text(SETTING_ENABLED)? != "true" || text(SETTING_BACKEND)? != BACKEND_NAS {
        return Ok(None);
    }
    // 保存先の切り替えの予約。ここでは保存先の控えを持たないので、消すだけでよい。
    if text(SETTING_RESET_PENDING)? == "true" {
        s.set_setting(SETTING_RESET_PENDING, "false")?;
    }
    let (device_id, db_id) = s.ensure_sync_ids()?;
    let writer = Some(text(SETTING_DEVICE_NAME)?)
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| DEFAULT_SERVER_NAME.to_string());
    let last_compaction = s
        .get_setting(SETTING_LAST_COMPACTION_AT)?
        .and_then(|v| DateTime::parse_from_rfc3339(&v).ok())
        .map(|d| d.with_timezone(&Utc));
    let schedule = ScheduleConfig::from_settings(|k| s.get_setting(k).ok().flatten());
    Ok(Some(Prepared {
        url: text(SETTING_NAS_URL)?,
        user: text(SETTING_NAS_USER)?,
        device_id,
        db_id,
        writer,
        last_compaction,
        schedule,
    }))
}

/// NAS へ1回同期する。エラーは利用者に見せる文 (秘密を含まない)。
pub async fn sync_nas(
    store: &Mutex<Store>,
    env: &HeadlessEnv,
    make_drive: &MakeDrive,
    now: DateTime<Utc>,
) -> Result<SyncOutcome, String> {
    let Some(p) = prepare(store).map_err(|e| e.to_string())? else {
        return Ok(SyncOutcome::Disabled);
    };
    let password = env.nas_password.clone().filter(|pw| !pw.is_empty());
    let (Some(password), false, false) = (password, p.url.is_empty(), p.user.is_empty()) else {
        return Err(NAS_INCOMPLETE_MESSAGE.to_string());
    };
    let url = lan_nas_url(&p.url).map_err(str::to_string)?;
    let drive = make_drive(WebDavConfig {
        url,
        user: p.user,
        password,
        proxy: None,
    })
    .map_err(|_| NAS_PREPARE_MESSAGE.to_string())?;
    let compact = p.last_compaction.is_none()
        || compaction_due(p.last_compaction, now, env.tz, p.schedule.daily_time);
    let input = SyncInput {
        device_id: p.device_id,
        db_id: p.db_id,
        writer: p.writer,
        now,
        tz: env.tz,
        compact,
    };
    let report = run_sync(store, drive.as_ref(), &input)
        .await
        .map_err(|e| match e {
            SyncError::Drive(DriveError::Retryable(_)) => NETWORK_MESSAGE.to_string(),
            other => other.to_string(),
        })?;
    let stamp = now.to_rfc3339();
    let s = lock(store);
    s.set_setting(SETTING_LAST_SYNC_AT, &stamp)
        .map_err(|e| e.to_string())?;
    if report.compacted {
        s.set_setting(SETTING_LAST_COMPACTION_AT, &stamp)
            .map_err(|e| e.to_string())?;
    }
    Ok(SyncOutcome::Synced(report))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sync::drive::MemoryDrive;
    use chrono::TimeZone;

    const NAS: &str = "http://192.168.0.151:5005/infodesk/";

    fn jst() -> FixedOffset {
        FixedOffset::east_opt(9 * 3600).unwrap()
    }

    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 11, 1, 3, 0, 0).unwrap()
    }

    fn nas_store(url: &str) -> Mutex<Store> {
        let s = Store::open_in_memory().unwrap();
        crate::seed::seed_defaults(&s).unwrap();
        s.set_setting(SETTING_ENABLED, "true").unwrap();
        s.set_setting(SETTING_BACKEND, BACKEND_NAS).unwrap();
        s.set_setting(SETTING_NAS_URL, url).unwrap();
        s.set_setting(SETTING_NAS_USER, "infodesk").unwrap();
        Mutex::new(s)
    }

    fn env(pw: Option<&str>) -> HeadlessEnv {
        HeadlessEnv {
            nas_password: pw.map(str::to_string),
            tz: jst(),
        }
    }

    /// 作ろうとした WebDAV の設定を記録し、メモリ上の保存先を返す。
    fn recording() -> (Arc<Mutex<Vec<WebDavConfig>>>, Box<MakeDrive>) {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let s = seen.clone();
        let make: Box<MakeDrive> = Box::new(move |cfg| {
            s.lock().unwrap().push(cfg);
            Ok(Arc::new(MemoryDrive::new()) as Arc<dyn DriveStore>)
        });
        (seen, make)
    }

    #[tokio::test]
    async fn off_or_google_does_nothing() {
        let store = nas_store(NAS);
        let (seen, make) = recording();
        store
            .lock()
            .unwrap()
            .set_setting(SETTING_ENABLED, "false")
            .unwrap();
        let got = sync_nas(&store, &env(Some("pw")), &*make, now()).await;
        assert!(matches!(got, Ok(SyncOutcome::Disabled)));
        store
            .lock()
            .unwrap()
            .set_setting(SETTING_ENABLED, "true")
            .unwrap();
        store
            .lock()
            .unwrap()
            .set_setting(SETTING_BACKEND, BACKEND_GDRIVE)
            .unwrap();
        let got = sync_nas(&store, &env(Some("pw")), &*make, now()).await;
        assert!(matches!(got, Ok(SyncOutcome::Disabled)));
        assert!(seen.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn missing_password_is_incomplete() {
        let store = nas_store(NAS);
        let (seen, make) = recording();
        for pw in [None, Some("")] {
            let got = sync_nas(&store, &env(pw), &*make, now()).await;
            assert_eq!(got.err().as_deref(), Some(NAS_INCOMPLETE_MESSAGE));
        }
        assert!(seen.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_name_from_the_mac_settings_sends_nothing() {
        // Review Focus 3: the copied Mac settings hold http://takeuchi-nas:5005/infodesk/
        let store = nas_store("http://takeuchi-nas:5005/infodesk/");
        let (seen, make) = recording();
        let got = sync_nas(&store, &env(Some("pw")), &*make, now()).await;
        assert_eq!(got.err().as_deref(), Some(NAS_LAN_URL_MESSAGE));
        assert!(seen.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_normal_pass_connects_directly_and_records_the_time() {
        let store = nas_store(NAS);
        let (seen, make) = recording();
        let got = sync_nas(&store, &env(Some("pw")), &*make, now()).await;
        assert!(matches!(got, Ok(SyncOutcome::Synced(_))), "{got:?}");
        let cfg = seen.lock().unwrap()[0].clone();
        assert_eq!(cfg.url, NAS);
        assert_eq!(cfg.user, "infodesk");
        assert_eq!(cfg.password, "pw");
        assert!(cfg.proxy.is_none());
        let s = store.lock().unwrap();
        assert_eq!(
            s.get_setting(SETTING_LAST_SYNC_AT).unwrap(),
            Some(now().to_rfc3339())
        );
        // 初回はまとめる (保存先にスナップショットがない)。
        assert_eq!(
            s.get_setting(SETTING_LAST_COMPACTION_AT).unwrap(),
            Some(now().to_rfc3339())
        );
    }

    #[test]
    fn debug_hides_the_password() {
        assert!(!format!("{:?}", env(Some("secret-pw"))).contains("secret-pw"));
    }
}
