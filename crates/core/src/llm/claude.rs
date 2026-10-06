use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;

use async_trait::async_trait;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

use super::{LlmProvider, clean_output};
use crate::error::{Error, Result};

/// `claude -p` 1回の待ち時間の上限。応答しないプロセスで要約処理全体が止まるのを防ぐ。
pub const CLAUDE_TIMEOUT: Duration = Duration::from_secs(600);

pub struct ClaudeProvider {
    bin: PathBuf,
    timeout: Duration,
}

impl ClaudeProvider {
    pub fn new(bin: impl Into<PathBuf>) -> Self {
        Self::with_timeout(bin, CLAUDE_TIMEOUT)
    }

    pub fn with_timeout(bin: impl Into<PathBuf>, timeout: Duration) -> Self {
        Self {
            bin: bin.into(),
            timeout,
        }
    }
}

#[async_trait]
impl LlmProvider for ClaudeProvider {
    fn name(&self) -> String {
        "claude".to_string()
    }

    /// `claude -p` をツールなしで1回実行する。
    /// 作業ディレクトリを一時ディレクトリにし、`--setting-sources project` で
    /// 利用者のグローバル設定 (フックなど) が出力に混ざることを防ぐ。
    async fn complete(&self, system: &str, prompt: &str) -> Result<String> {
        let mut child = Command::new(&self.bin)
            .args([
                "-p",
                "--tools",
                "",
                "--output-format",
                "json",
                "--no-session-persistence",
                "--setting-sources",
                "project",
                "--system-prompt",
                system,
            ])
            .current_dir(std::env::temp_dir())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()?;
        // 時間切れでこの future を捨てると child も捨てられ、kill_on_drop で終了する。
        let run = async {
            if let Some(mut stdin) = child.stdin.take() {
                stdin.write_all(prompt.as_bytes()).await?;
                stdin.shutdown().await?;
            }
            child.wait_with_output().await
        };
        let out = tokio::time::timeout(self.timeout, run)
            .await
            .map_err(|_| Error::Llm("claude: timed out".to_string()))??;
        let value: serde_json::Value = serde_json::from_slice(&out.stdout).map_err(|e| {
            Error::Llm(format!(
                "claude: unparsable output ({e}): {}",
                String::from_utf8_lossy(&out.stderr).trim()
            ))
        })?;
        let result = value["result"].as_str().unwrap_or_default();
        if value["is_error"].as_bool().unwrap_or(false) || !out.status.success() {
            return Err(Error::Llm(format!("claude: {result}")));
        }
        clean_output(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    /// 引数と標準入力を記録し、指定のJSONを返す偽の claude コマンドを作る。
    fn fake_claude(dir: &std::path::Path, stdout_json: &str, exit_code: i32) -> PathBuf {
        let bin = dir.join("claude");
        let script = format!(
            "#!/bin/sh\nprintf '%s\\n' \"$@\" > \"{d}/args.txt\"\ncat > \"{d}/stdin.txt\"\nprintf '%s' '{out}'\nexit {code}\n",
            d = dir.display(),
            out = stdout_json,
            code = exit_code
        );
        std::fs::write(&bin, script).unwrap();
        std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
        bin
    }

    #[tokio::test]
    async fn passes_flags_and_prompt_and_returns_result() {
        let dir = tempfile::tempdir().unwrap();
        let bin = fake_claude(
            dir.path(),
            r#"{"is_error":false,"result":" 要約です。 "}"#,
            0,
        );
        let p = ClaudeProvider::new(bin);
        assert_eq!(p.name(), "claude");
        assert_eq!(
            p.complete("システム指示", "本文プロンプト").await.unwrap(),
            "要約です。"
        );

        let args = std::fs::read_to_string(dir.path().join("args.txt")).unwrap();
        let args: Vec<&str> = args.lines().collect();
        assert_eq!(
            args,
            vec![
                "-p",
                "--tools",
                "",
                "--output-format",
                "json",
                "--no-session-persistence",
                "--setting-sources",
                "project",
                "--system-prompt",
                "システム指示",
            ]
        );
        let stdin = std::fs::read_to_string(dir.path().join("stdin.txt")).unwrap();
        assert_eq!(stdin, "本文プロンプト");
    }

    #[tokio::test]
    async fn hung_process_times_out() {
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("claude");
        std::fs::write(&bin, "#!/bin/sh\ncat > /dev/null\nexec sleep 5\n").unwrap();
        std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
        let p = ClaudeProvider::with_timeout(bin, Duration::from_secs(1));
        let started = std::time::Instant::now();
        let err = p.complete("s", "p").await.unwrap_err();
        assert!(started.elapsed() < Duration::from_secs(4));
        assert!(
            matches!(&err, Error::Llm(m) if m == "claude: timed out"),
            "{err}"
        );
    }

    #[tokio::test]
    async fn error_flag_nonzero_exit_garbage_and_missing_binary_are_errors() {
        let dir = tempfile::tempdir().unwrap();
        let bin = fake_claude(
            dir.path(),
            r#"{"is_error":true,"result":"Not logged in"}"#,
            0,
        );
        let err = ClaudeProvider::new(bin)
            .complete("s", "p")
            .await
            .unwrap_err()
            .to_string();
        assert!(err.contains("Not logged in"), "{err}");

        let dir = tempfile::tempdir().unwrap();
        let bin = fake_claude(dir.path(), r#"{"is_error":false,"result":"x"}"#, 1);
        assert!(ClaudeProvider::new(bin).complete("s", "p").await.is_err());

        let dir = tempfile::tempdir().unwrap();
        let bin = fake_claude(dir.path(), "not json", 0);
        assert!(matches!(
            ClaudeProvider::new(bin).complete("s", "p").await,
            Err(Error::Llm(_))
        ));

        let dir = tempfile::tempdir().unwrap();
        let bin = fake_claude(dir.path(), r#"{"is_error":false,"result":"  "}"#, 0);
        assert!(matches!(
            ClaudeProvider::new(bin).complete("s", "p").await,
            Err(Error::Llm(_))
        ));

        let p = ClaudeProvider::new("/nonexistent/claude-binary");
        assert!(p.complete("s", "p").await.is_err());
    }
}
