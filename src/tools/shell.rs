use std::sync::Arc;

use adk_tool::{AdkError, tool};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::sandbox::FilesystemSandbox;

// ─── Thread-local sandbox context ──────────────────────────────

thread_local! {
    static SHELL_SANDBOX_CTX: std::cell::RefCell<Option<Arc<FilesystemSandbox>>> = std::cell::RefCell::new(None);
}

/// Set the sandbox for the current thread (called before tool execution).
pub fn set_sandbox(sandbox: Arc<FilesystemSandbox>) {
    SHELL_SANDBOX_CTX.with(|ctx| *ctx.borrow_mut() = Some(sandbox));
}

/// Get the sandbox for the current thread.
fn get_sandbox() -> Result<Arc<FilesystemSandbox>, AdkError> {
    SHELL_SANDBOX_CTX.with(|ctx| {
        ctx.borrow()
            .clone()
            .ok_or_else(|| AdkError::tool("shell tool sandbox not initialized"))
    })
}

/// Clear the sandbox for the current thread.
pub fn clear_sandbox() {
    SHELL_SANDBOX_CTX.with(|ctx| *ctx.borrow_mut() = None);
}

// ─── ShellExec ─────────────────────────────────────────────────

/// Shell command execution arguments.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct ShellExecArgs {
    /// Shell command to execute
    pub command: String,
    /// Timeout in seconds (default: 120)
    pub timeout: Option<u64>,
}

/// Execute a shell command with timeout.
/// Destructive commands (rm -rf, git push --force, DROP TABLE, etc.)
/// are flagged with needs_approval and require user confirmation.
#[tool]
pub async fn shell_exec(args: ShellExecArgs) -> Result<Value, AdkError> {
    let sandbox = get_sandbox()?;
    let timeout_secs = args.timeout.unwrap_or(120);
    let timeout = std::time::Duration::from_secs(timeout_secs);

    // Destructive command detection
    let destructive = sandbox.check_destructive(&args.command);
    if destructive.is_destructive {
        return Ok(json!({
            "needs_approval": true,
            "reason": format!(
                "Destructive command detected: {}. Pattern matched: {}. \
                 This command requires explicit user approval before execution.",
                destructive.category.as_deref().unwrap_or("unknown"),
                destructive.pattern.as_deref().unwrap_or("unknown"),
            ),
            "command": args.command,
        }));
    }

    // Execute the command
    let output = tokio::time::timeout(timeout, async {
        tokio::process::Command::new("sh")
            .arg("-c")
            .arg(&args.command)
            .current_dir(sandbox.root())
            .output()
            .await
    })
    .await
    .map_err(|_| {
        AdkError::tool(format!(
            "shell_exec: command timed out after {timeout_secs}s"
        ))
    })?
    .map_err(|e| {
        AdkError::tool(format!("shell_exec: failed to execute command: {e}"))
    })?;

    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    let exit_code = output.status.code().unwrap_or(-1);

    // Truncate output if very large to avoid overwhelming the agent context
    let max_output_len = 50_000;
    let stdout_truncated = if stdout.len() > max_output_len {
        format!(
            "{}\n\n... [output truncated, {} bytes total] ...",
            &stdout[..max_output_len],
            stdout.len()
        )
    } else {
        stdout
    };

    Ok(json!({
        "stdout": stdout_truncated,
        "stderr": stderr,
        "exit_code": exit_code,
        "needs_approval": false,
    }))
}

// ─── Tests ─────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_shell_exec_basic() {
        let tmp = tempfile::tempdir().unwrap();
        let sandbox = Arc::new(
            FilesystemSandbox::new(tmp.path(), crate::sandbox::PermissionMode::Auto).unwrap(),
        );
        set_sandbox(sandbox.clone());

        let result = shell_exec(ShellExecArgs {
            command: "echo hello".into(),
            timeout: Some(10),
        })
        .await
        .unwrap();

        assert_eq!(result["exit_code"], 0);
        assert_eq!(result["needs_approval"], false);
        assert!(result["stdout"].as_str().unwrap().contains("hello"));

        clear_sandbox();
    }

    #[tokio::test]
    async fn test_shell_exec_with_stderr() {
        let tmp = tempfile::tempdir().unwrap();
        let sandbox = Arc::new(
            FilesystemSandbox::new(tmp.path(), crate::sandbox::PermissionMode::Auto).unwrap(),
        );
        set_sandbox(sandbox.clone());

        let result = shell_exec(ShellExecArgs {
            command: "echo error >&2".into(),
            timeout: Some(10),
        })
        .await
        .unwrap();

        assert_eq!(result["exit_code"], 0);
        assert!(result["stderr"].as_str().unwrap().contains("error"));

        clear_sandbox();
    }

    #[tokio::test]
    async fn test_shell_exec_exit_code() {
        let tmp = tempfile::tempdir().unwrap();
        let sandbox = Arc::new(
            FilesystemSandbox::new(tmp.path(), crate::sandbox::PermissionMode::Auto).unwrap(),
        );
        set_sandbox(sandbox.clone());

        let result = shell_exec(ShellExecArgs {
            command: "exit 42".into(),
            timeout: Some(10),
        })
        .await
        .unwrap();

        assert_eq!(result["exit_code"], 42);

        clear_sandbox();
    }

    #[tokio::test]
    async fn test_shell_exec_destructive_rm_rf() {
        let tmp = tempfile::tempdir().unwrap();
        let sandbox = Arc::new(
            FilesystemSandbox::new(tmp.path(), crate::sandbox::PermissionMode::Strict).unwrap(),
        );
        set_sandbox(sandbox.clone());

        let result = shell_exec(ShellExecArgs {
            command: "rm -rf /".into(),
            timeout: Some(10),
        })
        .await
        .unwrap();

        // Should flag as needs_approval, NOT actually execute
        assert_eq!(result["needs_approval"], true);
        assert!(result["reason"].as_str().unwrap().contains("Destructive"));

        clear_sandbox();
    }

    #[tokio::test]
    async fn test_shell_exec_destructive_git_force() {
        let tmp = tempfile::tempdir().unwrap();
        let sandbox = Arc::new(
            FilesystemSandbox::new(tmp.path(), crate::sandbox::PermissionMode::Strict).unwrap(),
        );
        set_sandbox(sandbox.clone());

        let result = shell_exec(ShellExecArgs {
            command: "git push --force origin main".into(),
            timeout: Some(10),
        })
        .await
        .unwrap();

        assert_eq!(result["needs_approval"], true);

        clear_sandbox();
    }

    #[tokio::test]
    async fn test_shell_exec_destructive_drop_table() {
        let tmp = tempfile::tempdir().unwrap();
        let sandbox = Arc::new(
            FilesystemSandbox::new(tmp.path(), crate::sandbox::PermissionMode::Strict).unwrap(),
        );
        set_sandbox(sandbox.clone());

        let result = shell_exec(ShellExecArgs {
            command: "DROP TABLE users;".into(),
            timeout: Some(10),
        })
        .await
        .unwrap();

        assert_eq!(result["needs_approval"], true);

        clear_sandbox();
    }

    #[tokio::test]
    async fn test_shell_exec_destructive_delete_from() {
        let tmp = tempfile::tempdir().unwrap();
        let sandbox = Arc::new(
            FilesystemSandbox::new(tmp.path(), crate::sandbox::PermissionMode::Strict).unwrap(),
        );
        set_sandbox(sandbox.clone());

        let result = shell_exec(ShellExecArgs {
            command: "DELETE FROM users;".into(),
            timeout: Some(10),
        })
        .await
        .unwrap();

        assert_eq!(result["needs_approval"], true);

        clear_sandbox();
    }

    #[tokio::test]
    async fn test_shell_exec_destructive_git_reset_hard() {
        let tmp = tempfile::tempdir().unwrap();
        let sandbox = Arc::new(
            FilesystemSandbox::new(tmp.path(), crate::sandbox::PermissionMode::Strict).unwrap(),
        );
        set_sandbox(sandbox.clone());

        let result = shell_exec(ShellExecArgs {
            command: "git reset --hard HEAD~1".into(),
            timeout: Some(10),
        })
        .await
        .unwrap();

        assert_eq!(result["needs_approval"], true);

        clear_sandbox();
    }

    #[tokio::test]
    async fn test_shell_exec_non_destructive() {
        let tmp = tempfile::tempdir().unwrap();
        let sandbox = Arc::new(
            FilesystemSandbox::new(tmp.path(), crate::sandbox::PermissionMode::Strict).unwrap(),
        );
        set_sandbox(sandbox.clone());

        // These should NOT be flagged as destructive
        for cmd in &["ls -la", "cargo build", "git status", "echo hello"] {
            let result = shell_exec(ShellExecArgs {
                command: cmd.to_string(),
                timeout: Some(10),
            })
            .await
            .unwrap();

            assert_eq!(
                result["needs_approval"], false,
                "Command '{cmd}' should not be flagged as destructive"
            );
        }

        clear_sandbox();
    }

    #[tokio::test]
    async fn test_shell_exec_working_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let sandbox = Arc::new(
            FilesystemSandbox::new(tmp.path(), crate::sandbox::PermissionMode::Auto).unwrap(),
        );
        set_sandbox(sandbox.clone());

        let result = shell_exec(ShellExecArgs {
            command: "pwd".into(),
            timeout: Some(10),
        })
        .await
        .unwrap();

        let stdout = result["stdout"].as_str().unwrap();
        // Should be in the sandbox root directory
        assert!(stdout.contains(&tmp.path().display().to_string()));

        clear_sandbox();
    }

    #[tokio::test]
    async fn test_shell_exec_timeout() {
        let tmp = tempfile::tempdir().unwrap();
        let sandbox = Arc::new(
            FilesystemSandbox::new(tmp.path(), crate::sandbox::PermissionMode::Auto).unwrap(),
        );
        set_sandbox(sandbox.clone());

        let result = shell_exec(ShellExecArgs {
            command: "sleep 10".into(),
            timeout: Some(1), // 1 second timeout
        })
        .await;

        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("timed out"), "Expected timeout error, got: {err}");

        clear_sandbox();
    }
}
