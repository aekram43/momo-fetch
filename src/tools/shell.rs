use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Shell exec tool arguments.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ShellExecArgs {
    /// Shell command to execute
    pub command: String,
    /// Timeout in seconds (default: 120)
    pub timeout: Option<u64>,
}

/// Execute a shell command.
/// Destructive commands require user approval (HITL).
pub async fn shell_exec(
    command: &str,
    timeout_secs: Option<u64>,
    working_dir: &std::path::Path,
) -> anyhow::Result<ShellExecResult> {
    let timeout = Duration::from_secs(timeout_secs.unwrap_or(120));

    let output = tokio::time::timeout(timeout, async {
        tokio::process::Command::new("sh")
            .arg("-c")
            .arg(command)
            .current_dir(working_dir)
            .output()
            .await
    })
    .await
    .map_err(|_| anyhow::anyhow!("Command timed out after {}s", timeout.as_secs()))?
    .map_err(|e| anyhow::anyhow!("Failed to execute command: {e}"))?;

    Ok(ShellExecResult {
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        exit_code: output.status.code().unwrap_or(-1),
    })
}

/// Result of a shell command execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShellExecResult {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
}
