use infodesk_core::sync::mode::is_reader;
use tauri::State;

use crate::state::{AppState, AppStatus};

#[tauri::command]
pub fn get_status(state: State<'_, AppState>) -> AppStatus {
    status_with_mode(&state)
}

/// 状態に、設定から読んだ読むだけのモードを添える。
pub fn status_with_mode(state: &AppState) -> AppStatus {
    let mut status = state.status();
    status.reader_mode = state.with_store(is_reader).unwrap_or(false);
    status
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_carries_the_reader_mode_from_the_settings() {
        let (state, _) = crate::state::test_state();
        assert!(!status_with_mode(&state).reader_mode);
        state.with_store(|s| s.switch_to_reader()).unwrap();
        assert!(status_with_mode(&state).reader_mode);
    }
}
