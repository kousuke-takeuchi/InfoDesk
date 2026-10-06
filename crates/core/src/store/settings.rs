use rusqlite::{params, OptionalExtension};

use super::Store;
use crate::error::Result;

impl Store {
    pub fn get_setting(&self, key: &str) -> Result<Option<String>> {
        Ok(self
            .conn
            .query_row("SELECT value FROM settings WHERE key = ?1", params![key], |r| {
                r.get(0)
            })
            .optional()?)
    }

    pub fn get_setting_or(&self, key: &str, default: &str) -> Result<String> {
        Ok(self.get_setting(key)?.unwrap_or_else(|| default.to_string()))
    }

    pub fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO settings(key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }
}
