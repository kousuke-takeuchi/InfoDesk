use std::collections::HashMap;
use std::time::Instant;

use infodesk_core::llm::LlmProvider;
use infodesk_core::llm::claude::ClaudeProvider;
use infodesk_core::seed::DEFAULT_SETTINGS;
use serde::Serialize;
use tauri::State;

use crate::error::AppResult;
use crate::state::AppState;

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ProbeResult {
    pub ok: bool,
    pub message: String,
    pub millis: u64,
}

pub fn settings_map(state: &AppState) -> AppResult<HashMap<String, String>> {
    state.with_store(|store| {
        let mut map = HashMap::new();
        for (key, default) in DEFAULT_SETTINGS {
            map.insert(key.to_string(), store.get_setting_or(key, default)?);
        }
        Ok(map)
    })
}

/// 実行ファイルを指す設定。コマンド名 (`^[A-Za-z0-9_.-]+$`) か、存在する通常ファイルの
/// 絶対パスだけを受け付ける。
const EXECUTABLE_SETTINGS: &[&str] = &["claude.bin", "node.bin"];

fn is_bare_command(value: &str) -> bool {
    !value.is_empty()
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-'))
}

fn validate_executable(key: &str, value: &str) -> AppResult<()> {
    if is_bare_command(value) || (value.starts_with('/') && std::path::Path::new(value).is_file()) {
        return Ok(());
    }
    Err(format!(
        "invalid value for '{key}': use a command name or the absolute path of an existing file"
    )
    .into())
}

pub fn set_setting_value(state: &AppState, key: &str, value: &str) -> AppResult<()> {
    if !DEFAULT_SETTINGS.iter().any(|(k, _)| *k == key) {
        return Err(format!("unknown setting '{key}'").into());
    }
    if key == "app.launch_at_login" {
        return Err(
            "'app.launch_at_login' cannot be set directly; use set_launch_at_login"
                .to_string()
                .into(),
        );
    }
    if EXECUTABLE_SETTINGS.contains(&key) {
        validate_executable(key, value)?;
    }
    state.with_store(|store| store.set_setting(key, value))
}

/// `app.stay_resident` 設定。不正な値は true。
pub fn stay_resident(state: &AppState) -> bool {
    state
        .with_store(|s| s.get_setting_or("app.stay_resident", "true"))
        .map(|v| v != "false")
        .unwrap_or(true)
}

#[tauri::command]
pub fn set_launch_at_login(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    enabled: bool,
) -> AppResult<()> {
    use tauri_plugin_autostart::ManagerExt;
    let launcher = app.autolaunch();
    let result = if enabled {
        launcher.enable()
    } else {
        launcher.disable()
    };
    result.map_err(|e| e.to_string())?;
    state.with_store(|s| {
        s.set_setting(
            "app.launch_at_login",
            if enabled { "true" } else { "false" },
        )
    })
}

#[tauri::command]
pub fn show_main_window(app: tauri::AppHandle) {
    crate::tray::show_main_window(&app);
}

fn probe(ok: bool, message: impl Into<String>, started: Instant) -> ProbeResult {
    ProbeResult {
        ok,
        message: message.into(),
        millis: started.elapsed().as_millis() as u64,
    }
}

/// `GET {base}/api/tags` が返れば接続済みとみなす。
pub async fn probe_ollama(state: &AppState) -> ProbeResult {
    let started = Instant::now();
    let base =
        match state.with_store(|s| s.get_setting_or("ollama.base_url", "http://localhost:11434")) {
            Ok(b) => b,
            Err(e) => return probe(false, e.message, started),
        };
    let url = format!("{}/api/tags", base.trim_end_matches('/'));
    match state.http.get(&url).send().await {
        Ok(resp) if resp.status().is_success() => probe(true, format!("接続済み ({url})"), started),
        Ok(resp) => probe(false, format!("{url}: HTTP {}", resp.status()), started),
        Err(e) => probe(false, format!("{url}: {e}"), started),
    }
}

/// `claude -p` に短い指示を送る。利用者のCLI認証をそのまま使う。
pub async fn probe_claude(state: &AppState) -> ProbeResult {
    let started = Instant::now();
    let bin = match state.with_store(|s| s.get_setting_or("claude.bin", "claude")) {
        Ok(b) => b,
        Err(e) => return probe(false, e.message, started),
    };
    let provider = ClaudeProvider::new(bin);
    match provider.complete("Reply with exactly: OK", "ping").await {
        Ok(_) => probe(true, "接続済み", started),
        Err(e) => probe(false, e.to_string(), started),
    }
}

#[tauri::command]
pub fn get_settings(state: State<'_, AppState>) -> AppResult<HashMap<String, String>> {
    settings_map(&state)
}

#[tauri::command]
pub fn set_setting(state: State<'_, AppState>, key: String, value: String) -> AppResult<()> {
    set_setting_value(&state, &key, &value)
}

#[tauri::command]
pub async fn test_ollama(state: State<'_, AppState>) -> AppResult<ProbeResult> {
    Ok(probe_ollama(&state).await)
}

#[tauri::command]
pub async fn test_claude(state: State<'_, AppState>) -> AppResult<ProbeResult> {
    Ok(probe_claude(&state).await)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::test_state;

    #[test]
    fn settings_cover_every_default_key_and_reflect_updates() {
        let (state, _) = test_state();
        let all = settings_map(&state).unwrap();
        for (key, value) in DEFAULT_SETTINGS {
            assert_eq!(all.get(*key).map(String::as_str), Some(*value), "{key}");
        }
        set_setting_value(&state, "ollama.model", "m2").unwrap();
        assert_eq!(settings_map(&state).unwrap()["ollama.model"], "m2");
        assert!(set_setting_value(&state, "unknown.key", "x").is_err());
    }

    #[test]
    fn stay_resident_defaults_true_and_reads_setting() {
        let (state, _) = test_state();
        assert!(stay_resident(&state));
        set_setting_value(&state, "app.stay_resident", "false").unwrap();
        assert!(!stay_resident(&state));
        set_setting_value(&state, "app.stay_resident", "garbage").unwrap();
        assert!(stay_resident(&state), "不正な値は既定の true");
    }

    #[test]
    fn launch_at_login_is_not_a_plain_setting() {
        let (state, _) = test_state();
        let err = set_setting_value(&state, "app.launch_at_login", "true").unwrap_err();
        assert!(
            err.message.contains("set_launch_at_login"),
            "{}",
            err.message
        );
    }

    #[test]
    fn executable_settings_accept_bare_names_or_existing_absolute_files() {
        let (state, _) = test_state();
        let file = tempfile::NamedTempFile::new().unwrap();
        let abs = file.path().to_str().unwrap().to_string();
        let dir = file.path().parent().unwrap().to_str().unwrap().to_string();
        for key in ["claude.bin", "node.bin"] {
            for ok in ["claude", "node-22", "my_tool.v2", abs.as_str()] {
                set_setting_value(&state, key, ok).unwrap_or_else(|e| panic!("{key}={ok}: {e}"));
                assert_eq!(settings_map(&state).unwrap()[key], ok);
            }
            for bad in [
                "",
                "claude -p",
                "./claude",
                "bin/claude",
                "claude;rm -rf ~",
                "/nonexistent/claude",
                dir.as_str(),
            ] {
                assert!(
                    set_setting_value(&state, key, bad).is_err(),
                    "{key}={bad:?} should be rejected"
                );
            }
        }
    }

    #[tokio::test]
    async fn ollama_probe_reports_unreachable() {
        let (state, _) = test_state();
        set_setting_value(&state, "ollama.base_url", "http://127.0.0.1:1").unwrap();
        let r = probe_ollama(&state).await;
        assert!(!r.ok);
        assert!(r.message.contains("127.0.0.1:1"), "{}", r.message);
    }

    #[tokio::test]
    async fn claude_probe_reports_missing_binary() {
        let (state, _) = test_state();
        state
            .with_store(|s| s.set_setting("claude.bin", "/nonexistent/claude"))
            .unwrap();
        let r = probe_claude(&state).await;
        assert!(!r.ok);
    }
}
