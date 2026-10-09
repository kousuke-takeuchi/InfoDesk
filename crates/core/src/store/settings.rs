use rusqlite::{OptionalExtension, params};

use super::Store;
use crate::error::Result;

impl Store {
    pub fn get_setting(&self, key: &str) -> Result<Option<String>> {
        Ok(self
            .conn
            .query_row(
                "SELECT value FROM settings WHERE key = ?1",
                params![key],
                |r| r.get(0),
            )
            .optional()?)
    }

    pub fn get_setting_or(&self, key: &str, default: &str) -> Result<String> {
        Ok(self
            .get_setting(key)?
            .unwrap_or_else(|| default.to_string()))
    }

    pub fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO settings(key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    /// 全ての設定をキー順に返す。
    pub fn list_settings(&self) -> Result<Vec<(String, String)>> {
        let mut stmt = self
            .conn
            .prepare("SELECT key, value FROM settings ORDER BY key")?;
        let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }
}

#[cfg(test)]
mod tests {
    use crate::store::Store;

    #[test]
    fn list_settings_returns_all_pairs_sorted_by_key() {
        let store = Store::open_in_memory().unwrap();
        store.set_setting("b.key", "2").unwrap();
        store.set_setting("a.key", "1").unwrap();
        assert_eq!(
            store.list_settings().unwrap(),
            vec![
                ("a.key".to_string(), "1".to_string()),
                ("b.key".to_string(), "2".to_string())
            ]
        );
    }
}
