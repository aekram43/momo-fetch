use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use adk_tool::{AdkError, tool};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::sandbox::FilesystemSandbox;

// ─── Thread-local sandbox context ──────────────────────────────

thread_local! {
    static SANDBOX_CTX: RefCell<Option<Arc<FilesystemSandbox>>> = RefCell::new(None);
}

/// Set the sandbox for the current thread (called before tool execution).
pub fn set_sandbox(sandbox: Arc<FilesystemSandbox>) {
    SANDBOX_CTX.with(|ctx| *ctx.borrow_mut() = Some(sandbox));
}

/// Get the sandbox for the current thread.
fn get_sandbox() -> Result<Arc<FilesystemSandbox>, AdkError> {
    SANDBOX_CTX.with(|ctx| {
        ctx.borrow()
            .clone()
            .ok_or_else(|| AdkError::tool("file tool sandbox not initialized"))
    })
}

/// Clear the sandbox for the current thread.
#[allow(dead_code)]
pub fn clear_sandbox() {
    SANDBOX_CTX.with(|ctx| *ctx.borrow_mut() = None);
}

// ─── FileRead ──────────────────────────────────────────────────

/// Maximum file size that can be read without a range parameter (256 KB).
const MAX_FILE_SIZE_NO_RANGE: usize = 256 * 1024;
/// Maximum file size that can be read even with a range parameter (2 MB).
const MAX_FILE_SIZE_ABSOLUTE: usize = 2 * 1024 * 1024;

/// Read file contents with optional line range.
/// Returns content with line numbers (cat -n format).
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct FileReadArgs {
    /// File path relative to working directory
    pub path: String,
    /// Optional line range, e.g., "1-50"
    pub range: Option<String>,
}

/// Reads a file and returns its content with line numbers.
/// Supports optional line range filtering (e.g., "1-50").
/// Returns an error if the file exceeds size limits (256 KB without range,
/// 2 MB with range) with actionable guidance.
#[tool]
pub async fn file_read(args: FileReadArgs) -> Result<Value, AdkError> {
    let sandbox = get_sandbox()?;
    let resolved = sandbox.resolve_path(&args.path).map_err(|e| {
        AdkError::tool(format!("file_read: path resolution failed: {e}"))
    })?;

    // Check file metadata before reading
    let metadata = tokio::fs::metadata(&resolved).await.map_err(|e| {
        AdkError::tool(format!("file_read: cannot access '{}': {e}", args.path))
    })?;
    let file_size = metadata.len() as usize;

    // Enforce size limits
    if args.range.is_none() && file_size > MAX_FILE_SIZE_NO_RANGE {
        // Estimate total lines for range suggestion
        let total_lines = estimate_line_count(&resolved).await;
        return Err(AdkError::tool(format!(
            "file_read: '{}' is {} KB — exceeds the 256 KB limit for full reads. \
             Use the \"range\" parameter to read specific sections, e.g.: \
             range=\"1-100\" (file has ~{} lines total). \
             Or read it in chunks: range=\"1-200\", range=\"201-400\", etc.",
            args.path,
            file_size / 1024,
            total_lines,
        )));
    }
    if file_size > MAX_FILE_SIZE_ABSOLUTE {
        return Err(AdkError::tool(format!(
            "file_read: '{}' is {} KB — exceeds the 2 MB absolute limit. \
             This file is too large for the context window. \
             Consider using grep to search for specific content, \
             or split the file into smaller parts first.",
            args.path,
            file_size / 1024,
        )));
    }

    let content = tokio::fs::read_to_string(&resolved).await.map_err(|e| {
        AdkError::tool(format!("file_read: failed to read '{}': {e}", args.path))
    })?;

    let total_lines = content.lines().count();
    let result = if let Some(ref range) = args.range {
        let (start, end) = parse_line_range(range, total_lines).map_err(|e| {
            AdkError::tool(format!("file_read: {e}"))
        })?;
        content
            .lines()
            .enumerate()
            .skip(start)
            .take(end - start)
            .map(|(i, line)| format!("{:>6}\t{}", i + 1, line))
            .collect::<Vec<_>>()
            .join("\n")
    } else {
        content
            .lines()
            .enumerate()
            .map(|(i, line)| format!("{:>6}\t{}", i + 1, line))
            .collect::<Vec<_>>()
            .join("\n")
    };

    // Include file size hint for files approaching the limit
    let size_note = if file_size > MAX_FILE_SIZE_NO_RANGE / 2 && args.range.is_some() {
        format!(" ({} KB read)", file_size / 1024)
    } else {
        String::new()
    };

    Ok(json!({
        "content": result,
        "path": args.path,
        "total_lines": total_lines,
        "size_note": size_note,
    }))
}

/// Estimate line count by reading the first 4 KB and extrapolating.
async fn estimate_line_count(path: &Path) -> usize {
    let sample_size = 4096usize;
    match tokio::fs::read(path).await {
        Ok(data) => {
            let sample = &data[..sample_size.min(data.len())];
            let lines_in_sample = sample.iter().filter(|&&b| b == b'\n').count().max(1);
            let ratio = lines_in_sample as f64 / sample.len().max(1) as f64;
            (data.len() as f64 * ratio) as usize
        }
        Err(_) => 0,
    }
}

// ─── FileWrite ─────────────────────────────────────────────────

/// Write content to a file. Creates parent directories if needed.
/// Overwrites existing content. Uses atomic write (temp file + rename).
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct FileWriteArgs {
    /// File path relative to working directory
    pub path: String,
    /// Content to write
    pub content: String,
}

/// Writes content to a file, creating parent directories if needed.
/// Uses atomic write (write to temp file, then rename).
#[tool]
pub async fn file_write(args: FileWriteArgs) -> Result<Value, AdkError> {
    let sandbox = get_sandbox()?;
    let resolved = sandbox.resolve_path(&args.path).map_err(|e| {
        AdkError::tool(format!("file_write: path resolution failed: {e}"))
    })?;

    // Create parent directories
    if let Some(parent) = resolved.parent() {
        tokio::fs::create_dir_all(parent).await.map_err(|e| {
            AdkError::tool(format!(
                "file_write: failed to create directories for '{}': {e}",
                args.path
            ))
        })?;
    }

    // Atomic write: temp file → rename
    let tmp_path = atomic_temp_path(&resolved);
    tokio::fs::write(&tmp_path, &args.content)
        .await
        .map_err(|e| {
            AdkError::tool(format!("file_write: failed to write '{}': {e}", args.path))
        })?;
    tokio::fs::rename(&tmp_path, &resolved)
        .await
        .map_err(|e| {
            AdkError::tool(format!("file_write: failed to rename temp file for '{}': {e}", args.path))
        })?;

    Ok(json!({
        "success": true,
        "path": args.path,
        "bytes_written": args.content.len(),
    }))
}

// ─── FileEdit ──────────────────────────────────────────────────

/// Replace a unique string in a file with a new string.
/// Fails if old_string is not found or has multiple matches.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct FileEditArgs {
    /// File path relative to working directory
    pub path: String,
    /// Exact string to find (must be unique in file)
    pub old_string: String,
    /// Replacement string
    pub new_string: String,
}

/// Replaces exactly one occurrence of old_string with new_string in a file.
/// The old_string must be unique in the file. Fails if not found or multiple matches.
#[tool]
pub async fn file_edit(args: FileEditArgs) -> Result<Value, AdkError> {
    let sandbox = get_sandbox()?;
    let resolved = sandbox.resolve_path(&args.path).map_err(|e| {
        AdkError::tool(format!("file_edit: path resolution failed: {e}"))
    })?;

    let content = tokio::fs::read_to_string(&resolved).await.map_err(|e| {
        AdkError::tool(format!("file_edit: failed to read '{}': {e}", args.path))
    })?;

    // Check uniqueness
    let matches = content.matches(&args.old_string).count();
    if matches == 0 {
        return Err(AdkError::tool(format!(
            "file_edit: old_string not found in '{}'",
            args.path
        )));
    }
    if matches > 1 {
        return Err(AdkError::tool(format!(
            "file_edit: old_string found {matches} times in '{}' — must be unique. \
             Add more surrounding context to make it unique.",
            args.path
        )));
    }

    let new_content = content.replacen(&args.old_string, &args.new_string, 1);

    // Atomic write
    let tmp_path = atomic_temp_path(&resolved);
    tokio::fs::write(&tmp_path, &new_content)
        .await
        .map_err(|e| AdkError::tool(format!("file_edit: write failed for '{}': {e}", args.path)))?;
    tokio::fs::rename(&tmp_path, &resolved)
        .await
        .map_err(|e| {
            AdkError::tool(format!("file_edit: rename failed for '{}': {e}", args.path))
        })?;

    Ok(json!({
        "success": true,
        "path": args.path,
        "replacements": 1,
    }))
}

// ─── Helpers ───────────────────────────────────────────────────

/// Generate a temp file path for atomic writes.
fn atomic_temp_path(path: &Path) -> PathBuf {
    // Use a unique suffix to avoid collisions
    let tmp = path.to_path_buf();
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    let ext = path
        .extension()
        .map(|s| format!(".{}", s.to_string_lossy()))
        .unwrap_or_default();
    tmp.with_file_name(format!("{stem}.harness-tmp{ext}"))
}

/// Parse a line range string like "1-50" into (start, end) (0-indexed, inclusive).
fn parse_line_range(range: &str, total_lines: usize) -> Result<(usize, usize), String> {
    let parts: Vec<&str> = range.split('-').collect();
    if parts.len() != 2 {
        return Err(format!(
            "Invalid range format: '{range}'. Expected 'start-end' (e.g., '1-50')"
        ));
    }

    let start: usize = parts[0]
        .trim()
        .parse::<usize>()
        .map_err(|e| format!("Invalid range start: {e}"))?
        .saturating_sub(1);
    let end: usize = parts[1]
        .trim()
        .parse::<usize>()
        .map_err(|e| format!("Invalid range end: {e}"))?
        .min(total_lines);

    if start >= end {
        return Err(format!(
            "Invalid range: start ({}) >= end ({})",
            start + 1,
            end
        ));
    }

    Ok((start, end))
}

// ─── Tests ─────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_line_range() {
        assert_eq!(parse_line_range("1-50", 100).unwrap(), (0, 50));
        assert_eq!(parse_line_range("10-20", 100).unwrap(), (9, 20));
    }

    #[test]
    fn test_parse_line_range_invalid() {
        assert!(parse_line_range("50", 100).is_err());
        assert!(parse_line_range("abc-def", 100).is_err());
    }

    #[test]
    fn test_parse_line_range_bounds() {
        // End clamped to total_lines
        assert_eq!(parse_line_range("1-200", 100).unwrap(), (0, 100));
        // Start >= end should fail
        assert!(parse_line_range("50-10", 100).is_err());
    }

    #[tokio::test]
    async fn test_file_read_basic() {
        let tmp = tempfile::tempdir().unwrap();
        let sandbox = Arc::new(
            FilesystemSandbox::new(tmp.path(), crate::sandbox::PermissionMode::Auto).unwrap(),
        );
        set_sandbox(sandbox.clone());

        // Write a test file directly
        let file_path = tmp.path().join("test.txt");
        tokio::fs::write(&file_path, "line1\nline2\nline3\n")
            .await
            .unwrap();

        let result = file_read(FileReadArgs {
            path: "test.txt".into(),
            range: None,
        })
        .await
        .unwrap();

        let content = result["content"].as_str().unwrap();
        assert!(content.contains("line1"));
        assert!(content.contains("line2"));
        assert!(content.contains("line3"));
        assert_eq!(result["total_lines"], 3);

        clear_sandbox();
    }

    #[tokio::test]
    async fn test_file_read_with_range() {
        let tmp = tempfile::tempdir().unwrap();
        let sandbox = Arc::new(
            FilesystemSandbox::new(tmp.path(), crate::sandbox::PermissionMode::Auto).unwrap(),
        );
        set_sandbox(sandbox.clone());

        let file_path = tmp.path().join("test.txt");
        let content: Vec<String> = (1..=100).map(|i| format!("line {i}")).collect();
        tokio::fs::write(&file_path, content.join("\n"))
            .await
            .unwrap();

        let result = file_read(FileReadArgs {
            path: "test.txt".into(),
            range: Some("10-20".into()),
        })
        .await
        .unwrap();

        let text = result["content"].as_str().unwrap();
        assert!(text.contains("line 10"));
        assert!(text.contains("line 20"));
        assert!(!text.contains("line 21"));

        clear_sandbox();
    }

    #[tokio::test]
    async fn test_file_write_and_read() {
        let tmp = tempfile::tempdir().unwrap();
        let sandbox = Arc::new(
            FilesystemSandbox::new(tmp.path(), crate::sandbox::PermissionMode::Auto).unwrap(),
        );
        set_sandbox(sandbox.clone());

        // Write
        let result = file_write(FileWriteArgs {
            path: "subdir/hello.txt".into(),
            content: "Hello, world!".into(),
        })
        .await
        .unwrap();

        assert_eq!(result["success"], true);
        assert_eq!(result["bytes_written"], 13);

        // Verify file exists
        assert!(tmp.path().join("subdir/hello.txt").exists());

        // Read back
        let result = file_read(FileReadArgs {
            path: "subdir/hello.txt".into(),
            range: None,
        })
        .await
        .unwrap();

        let content = result["content"].as_str().unwrap();
        assert!(content.contains("Hello, world!"));

        clear_sandbox();
    }

    #[tokio::test]
    async fn test_file_edit_basic() {
        let tmp = tempfile::tempdir().unwrap();
        let sandbox = Arc::new(
            FilesystemSandbox::new(tmp.path(), crate::sandbox::PermissionMode::Auto).unwrap(),
        );
        set_sandbox(sandbox.clone());

        // Create initial file
        let file_path = tmp.path().join("test.txt");
        tokio::fs::write(&file_path, "Hello world\nGoodbye world\n")
            .await
            .unwrap();

        // Edit
        let result = file_edit(FileEditArgs {
            path: "test.txt".into(),
            old_string: "Hello".into(),
            new_string: "Hi".into(),
        })
        .await
        .unwrap();

        assert_eq!(result["success"], true);
        assert_eq!(result["replacements"], 1);

        // Verify
        let content = tokio::fs::read_to_string(&file_path).await.unwrap();
        assert_eq!(content, "Hi world\nGoodbye world\n");

        clear_sandbox();
    }

    #[tokio::test]
    async fn test_file_edit_not_found() {
        let tmp = tempfile::tempdir().unwrap();
        let sandbox = Arc::new(
            FilesystemSandbox::new(tmp.path(), crate::sandbox::PermissionMode::Auto).unwrap(),
        );
        set_sandbox(sandbox.clone());

        let file_path = tmp.path().join("test.txt");
        tokio::fs::write(&file_path, "Hello world\n").await.unwrap();

        let result = file_edit(FileEditArgs {
            path: "test.txt".into(),
            old_string: "nonexistent".into(),
            new_string: "replacement".into(),
        })
        .await;

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("not found"));

        clear_sandbox();
    }

    #[tokio::test]
    async fn test_file_edit_multiple_matches() {
        let tmp = tempfile::tempdir().unwrap();
        let sandbox = Arc::new(
            FilesystemSandbox::new(tmp.path(), crate::sandbox::PermissionMode::Auto).unwrap(),
        );
        set_sandbox(sandbox.clone());

        let file_path = tmp.path().join("test.txt");
        tokio::fs::write(&file_path, "common line 1\ncommon line 2\ncommon line 3\n")
            .await
            .unwrap();

        let result = file_edit(FileEditArgs {
            path: "test.txt".into(),
            old_string: "common".into(),
            new_string: "replaced".into(),
        })
        .await;

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("found 3 times"));

        clear_sandbox();
    }

    #[tokio::test]
    async fn test_file_read_path_traversal_blocked() {
        let tmp = tempfile::tempdir().unwrap();
        let sandbox = Arc::new(
            FilesystemSandbox::new(tmp.path(), crate::sandbox::PermissionMode::Strict).unwrap(),
        );

        // Sandbox should reject traversal directly
        assert!(sandbox.resolve_path("../../etc/passwd").is_err());
        assert!(sandbox.resolve_path("../../../etc/shadow").is_err());

        // Should also fail through the tool
        set_sandbox(sandbox.clone());

        let result = file_read(FileReadArgs {
            path: "../../etc/passwd".into(),
            range: None,
        })
        .await;

        assert!(result.is_err(), "Expected error for path traversal");
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("traversal") || msg.contains("outside") || msg.contains("blocked"),
            "Expected path traversal error, got: {msg}"
        );

        clear_sandbox();
    }
}
