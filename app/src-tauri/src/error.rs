use serde::Serialize;

/// 画面に返すエラー。Tauri は `Err` を JSON にして `invoke` の reject に渡す。
#[derive(Debug, Clone, Serialize)]
pub struct AppError {
    pub message: String,
}

pub type AppResult<T> = Result<T, AppError>;

impl From<infodesk_core::Error> for AppError {
    fn from(e: infodesk_core::Error) -> Self {
        Self {
            message: e.to_string(),
        }
    }
}

impl From<String> for AppError {
    fn from(message: String) -> Self {
        Self { message }
    }
}

impl From<tauri::Error> for AppError {
    fn from(e: tauri::Error) -> Self {
        Self {
            message: e.to_string(),
        }
    }
}

impl std::fmt::Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_core_and_string_errors() {
        let e: AppError = infodesk_core::Error::Llm("down".into()).into();
        assert_eq!(e.message, "llm: down");
        let e: AppError = String::from("plain").into();
        assert_eq!(e.message, "plain");
        assert_eq!(serde_json::to_string(&e).unwrap(), r#"{"message":"plain"}"#);
    }
}
