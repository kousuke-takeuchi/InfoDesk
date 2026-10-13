use std::sync::Mutex;

pub const SERVICE: &str = "jp.infodesk.app.google";
pub const ACCOUNT: &str = "refresh_token";

/// 更新用トークンの置き場。本番はキーチェーン、テストはメモリ。
pub trait SecretStore: Send + Sync {
    fn get(&self) -> Result<Option<String>, String>;
    fn set(&self, value: &str) -> Result<(), String>;
    fn delete(&self) -> Result<(), String>;
}

#[derive(Default)]
pub struct MemorySecretStore(Mutex<Option<String>>);

impl SecretStore for MemorySecretStore {
    fn get(&self) -> Result<Option<String>, String> {
        Ok(self.0.lock().unwrap().clone())
    }
    fn set(&self, value: &str) -> Result<(), String> {
        *self.0.lock().unwrap() = Some(value.to_string());
        Ok(())
    }
    fn delete(&self) -> Result<(), String> {
        *self.0.lock().unwrap() = None;
        Ok(())
    }
}

/// macOS のキーチェーン (汎用パスワード)。
#[cfg(target_os = "macos")]
pub struct KeychainStore;

#[cfg(target_os = "macos")]
impl SecretStore for KeychainStore {
    fn get(&self) -> Result<Option<String>, String> {
        use security_framework::passwords::get_generic_password;
        match get_generic_password(SERVICE, ACCOUNT) {
            Ok(bytes) => String::from_utf8(bytes)
                .map(Some)
                .map_err(|e| e.to_string()),
            Err(e) if e.code() == ERR_SEC_ITEM_NOT_FOUND => Ok(None),
            Err(e) => Err(format!("keychain: {e}")),
        }
    }
    fn set(&self, value: &str) -> Result<(), String> {
        security_framework::passwords::set_generic_password(SERVICE, ACCOUNT, value.as_bytes())
            .map_err(|e| format!("keychain: {e}"))
    }
    fn delete(&self) -> Result<(), String> {
        match security_framework::passwords::delete_generic_password(SERVICE, ACCOUNT) {
            Ok(()) => Ok(()),
            Err(e) if e.code() == ERR_SEC_ITEM_NOT_FOUND => Ok(()),
            Err(e) => Err(format!("keychain: {e}")),
        }
    }
}

/// `errSecItemNotFound`。
#[cfg(target_os = "macos")]
const ERR_SEC_ITEM_NOT_FOUND: i32 = -25300;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_store_round_trips() {
        let s = MemorySecretStore::default();
        assert_eq!(s.get().unwrap(), None);
        s.set("a").unwrap();
        assert_eq!(s.get().unwrap().as_deref(), Some("a"));
        s.delete().unwrap();
        assert_eq!(s.get().unwrap(), None);
    }
}
