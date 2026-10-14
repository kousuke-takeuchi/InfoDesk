//! Storage behind cloud sync: a trait for the Drive `appDataFolder` and an in-memory fake.

use std::collections::BTreeMap;
use std::sync::Mutex;

use async_trait::async_trait;

#[derive(Clone, Debug, PartialEq)]
pub struct RemoteFile {
    pub id: String,
    pub name: String,
    pub modified_time: String,
    pub size: Option<u64>,
}

#[derive(Debug, thiserror::Error)]
pub enum DriveError {
    #[error("保存先に接続できません: {0}")]
    Retryable(String),
    #[error("Google の再ログインが必要です")]
    AuthExpired,
    #[error("NAS のユーザー名かパスワードが違います")]
    BadCredentials,
    /// 507 or 413 from the NAS.
    #[error("NAS の空き容量が足りません")]
    StorageFull,
    /// 403 from the NAS.
    #[error("NAS のフォルダに書き込む権限がありません")]
    Forbidden,
    /// A 3xx answer. Redirects are never followed.
    #[error("NAS の URL を確かめてください (転送されました)")]
    Redirected,
    #[error("保存先のエラー: {0}")]
    Other(String),
}

#[async_trait]
pub trait DriveStore: Send + Sync {
    async fn list(&self) -> Result<Vec<RemoteFile>, DriveError>;
    async fn download(&self, id: &str) -> Result<Vec<u8>, DriveError>;
    async fn create(
        &self,
        name: &str,
        bytes: Vec<u8>,
        mime: &str,
    ) -> Result<RemoteFile, DriveError>;
    async fn update(&self, id: &str, bytes: Vec<u8>, mime: &str) -> Result<RemoteFile, DriveError>;
    async fn delete(&self, id: &str) -> Result<(), DriveError>;
}

/// In-memory fake used by tests.
#[derive(Default)]
pub struct MemoryDrive {
    files: Mutex<BTreeMap<String, (RemoteFile, Vec<u8>)>>,
    pub fail_next: Mutex<Option<DriveError>>,
    /// When set, the next `create` or `update` of a file whose name starts with
    /// this prefix fails with a retryable error, and the prefix is cleared.
    pub fail_on_name: Mutex<Option<String>>,
    counter: Mutex<u64>,
}

impl MemoryDrive {
    pub fn new() -> Self {
        Self::default()
    }

    /// Names of all files, in id order.
    pub fn names(&self) -> Vec<String> {
        let files = self.files.lock().unwrap();
        files.values().map(|(f, _)| f.name.clone()).collect()
    }

    /// Upsert by name (another device's file).
    pub fn put(&self, name: &str, bytes: Vec<u8>) {
        let mut files = self.files.lock().unwrap();
        let existing = files
            .values()
            .find(|(f, _)| f.name == name)
            .map(|(f, _)| f.id.clone());
        let id = existing.unwrap_or_else(|| format!("id{:06}", self.tick()));
        let file = RemoteFile {
            id: id.clone(),
            name: name.to_string(),
            modified_time: self.pseudo_time(),
            size: Some(bytes.len() as u64),
        };
        files.insert(id, (file, bytes));
    }

    fn tick(&self) -> u64 {
        let mut c = self.counter.lock().unwrap();
        *c += 1;
        *c
    }

    /// Strictly increasing RFC3339 timestamp, one second per call.
    fn pseudo_time(&self) -> String {
        let n = self.tick() as i64;
        let t = chrono::DateTime::from_timestamp(1_700_000_000 + n, 0).unwrap();
        t.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string()
    }

    fn check_fail(&self) -> Result<(), DriveError> {
        match self.fail_next.lock().unwrap().take() {
            Some(e) => Err(e),
            None => Ok(()),
        }
    }

    fn check_fail_name(&self, name: &str) -> Result<(), DriveError> {
        let mut prefix = self.fail_on_name.lock().unwrap();
        if prefix.as_deref().is_some_and(|p| name.starts_with(p)) {
            *prefix = None;
            return Err(DriveError::Retryable(format!("injected failure: {name}")));
        }
        Ok(())
    }
}

#[async_trait]
impl DriveStore for MemoryDrive {
    async fn list(&self) -> Result<Vec<RemoteFile>, DriveError> {
        self.check_fail()?;
        let files = self.files.lock().unwrap();
        Ok(files.values().map(|(f, _)| f.clone()).collect())
    }

    async fn download(&self, id: &str) -> Result<Vec<u8>, DriveError> {
        self.check_fail()?;
        let files = self.files.lock().unwrap();
        files
            .get(id)
            .map(|(_, b)| b.clone())
            .ok_or_else(|| DriveError::Other(format!("not found: {id}")))
    }

    async fn create(
        &self,
        name: &str,
        bytes: Vec<u8>,
        _mime: &str,
    ) -> Result<RemoteFile, DriveError> {
        self.check_fail()?;
        self.check_fail_name(name)?;
        let id = format!("id{:06}", self.tick());
        let file = RemoteFile {
            id: id.clone(),
            name: name.to_string(),
            modified_time: self.pseudo_time(),
            size: Some(bytes.len() as u64),
        };
        self.files.lock().unwrap().insert(id, (file.clone(), bytes));
        Ok(file)
    }

    async fn update(
        &self,
        id: &str,
        bytes: Vec<u8>,
        _mime: &str,
    ) -> Result<RemoteFile, DriveError> {
        self.check_fail()?;
        let name = self
            .files
            .lock()
            .unwrap()
            .get(id)
            .map(|(f, _)| f.name.clone());
        if let Some(name) = name {
            self.check_fail_name(&name)?;
        }
        let modified_time = self.pseudo_time();
        let mut files = self.files.lock().unwrap();
        let entry = files
            .get_mut(id)
            .ok_or_else(|| DriveError::Other(format!("not found: {id}")))?;
        entry.0.modified_time = modified_time;
        entry.0.size = Some(bytes.len() as u64);
        entry.1 = bytes;
        Ok(entry.0.clone())
    }

    async fn delete(&self, id: &str) -> Result<(), DriveError> {
        self.check_fail()?;
        self.files.lock().unwrap().remove(id);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn fail_next_affects_only_the_next_call() {
        let d = MemoryDrive::new();
        *d.fail_next.lock().unwrap() = Some(DriveError::Retryable("x".into()));
        assert!(matches!(d.list().await, Err(DriveError::Retryable(_))));
        assert!(d.list().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn fail_on_name_hits_only_the_matching_write_once() {
        let d = MemoryDrive::new();
        let a = d.create("a.json", b"1".to_vec(), "x").await.unwrap();
        *d.fail_on_name.lock().unwrap() = Some("a.".into());
        assert!(d.create("b.json", b"1".to_vec(), "x").await.is_ok());
        assert!(matches!(
            d.update(&a.id, b"2".to_vec(), "x").await,
            Err(DriveError::Retryable(_))
        ));
        assert!(d.update(&a.id, b"2".to_vec(), "x").await.is_ok());
    }

    #[tokio::test]
    async fn create_update_download_delete_and_names() {
        let d = MemoryDrive::new();
        let f = d
            .create("a.json", b"1".to_vec(), "application/json")
            .await
            .unwrap();
        d.put("b.json", b"2".to_vec());
        assert_eq!(d.names(), vec!["a.json", "b.json"]);
        let u = d
            .update(&f.id, b"333".to_vec(), "application/json")
            .await
            .unwrap();
        assert_eq!(u.size, Some(3));
        assert!(u.modified_time > f.modified_time);
        assert_eq!(d.download(&f.id).await.unwrap(), b"333");
        d.delete(&f.id).await.unwrap();
        assert_eq!(d.names(), vec!["b.json"]);
    }
}
