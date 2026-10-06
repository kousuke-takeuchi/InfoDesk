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

pub fn set_setting_value(state: &AppState, key: &str, value: &str) -> AppResult<()> {
    if !DEFAULT_SETTINGS.iter().any(|(k, _)| *k == key) {
        return Err(format!("unknown setting '{key}'").into());
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
    set_setting_value(
        &state,
        "app.launch_at_login",
        if enabled { "true" } else { "false" },
    )
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
        set_setting_value(&state, "claude.bin", "/nonexistent/claude").unwrap();
        let r = probe_claude(&state).await;
        assert!(!r.ok);
    }
}
