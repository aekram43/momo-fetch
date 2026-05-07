use std::sync::Arc;

use adk_tool::{AdkError, tool};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::sandbox::FilesystemSandbox;

// ─── Thread-local sandbox context ──────────────────────────────

thread_local! {
    static SEARCH_SANDBOX_CTX: std::cell::RefCell<Option<Arc<FilesystemSandbox>>> = std::cell::RefCell::new(None);
}

/// Set the sandbox for the current thread (called before tool execution).
pub fn set_sandbox(sandbox: Arc<FilesystemSandbox>) {
    SEARCH_SANDBOX_CTX.with(|ctx| *ctx.borrow_mut() = Some(sandbox));
}

/// Get the sandbox for the current thread.
fn get_sandbox() -> Result<Arc<FilesystemSandbox>, AdkError> {
    SEARCH_SANDBOX_CTX.with(|ctx| {
        ctx.borrow()
            .clone()
            .ok_or_else(|| AdkError::tool("search tool sandbox not initialized"))
    })
}

/// Clear the sandbox for the current thread.
pub fn clear_sandbox() {
    SEARCH_SANDBOX_CTX.with(|ctx| *ctx.borrow_mut() = None);
}

// ─── Grep ──────────────────────────────────────────────────────

/// Grep tool arguments.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct GrepArgs {
    /// Regex pattern to search for in file contents
    pub pattern: String,
    /// Directory or file to search in (relative to working directory)
    pub path: Option<String>,
    /// File glob filter (e.g., "*.rs", "*.ts")
    pub glob: Option<String>,
    /// Whether pattern is case insensitive (default: false)
    pub case_insensitive: Option<bool>,
}

/// Search file contents using regex pattern.
/// Returns matching lines with file path and line numbers.
#[tool]
pub async fn grep(args: GrepArgs) -> Result<Value, AdkError> {
    let sandbox = get_sandbox()?;

    // Resolve search path
    let search_path = if let Some(ref p) = args.path {
        sandbox.resolve_path(p).map_err(|e| {
            AdkError::tool(format!("grep: path resolution failed: {e}"))
        })?
    } else {
        sandbox.root().to_path_buf()
    };

    // Compile regex
    let pattern_flags = if args.case_insensitive.unwrap_or(false) {
        regex::RegexBuilder::new(&args.pattern).case_insensitive(true).build()
    } else {
        regex::Regex::new(&args.pattern)
    };
    let re = pattern_flags.map_err(|e| {
        AdkError::tool(format!("grep: invalid regex pattern '{}': {e}", args.pattern))
    })?;

    // Compile glob filter if provided
    let glob_matcher = args.glob.as_ref().and_then(|g| glob::Pattern::new(g).ok());

    let mut matches = Vec::new();
    let mut files_searched = 0usize;

    if search_path.is_file() {
        files_searched += 1;
        search_file(&search_path, &re, &glob_matcher, &mut matches, sandbox.root())?;
    } else if search_path.is_dir() {
        for entry in walkdir::WalkDir::new(&search_path)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            if entry.file_type().is_file() {
                files_searched += 1;
                search_file(entry.path(), &re, &glob_matcher, &mut matches, sandbox.root())?;
            }
        }
    }

    // Limit results to prevent overwhelming agent context
    let max_results = 200;
    let total_matches = matches.len();
    if matches.len() > max_results {
        matches.truncate(max_results);
    }

    Ok(json!({
        "matches": matches,
        "total_matches": total_matches,
        "files_searched": files_searched,
        "truncated": total_matches > max_results,
    }))
}

// ─── Glob ──────────────────────────────────────────────────────

/// Glob tool arguments.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct GlobArgs {
    /// Glob pattern to match files (e.g., "**/*.rs", "src/**/*.ts")
    pub pattern: String,
    /// Directory to search in (relative to working directory)
    pub path: Option<String>,
}

/// Find files by glob pattern.
/// Returns list of matching file paths relative to working directory.
#[tool]
pub async fn glob(args: GlobArgs) -> Result<Value, AdkError> {
    let sandbox = get_sandbox()?;

    // Resolve search path
    let search_path = if let Some(ref p) = args.path {
        sandbox.resolve_path(p).map_err(|e| {
            AdkError::tool(format!("glob: path resolution failed: {e}"))
        })?
    } else {
        sandbox.root().to_path_buf()
    };

    let full_pattern = if args.pattern.starts_with('/') {
        args.pattern.clone()
    } else {
        format!("{}/{}", search_path.display(), args.pattern)
    };

    let mut results = Vec::new();
    for entry in glob::glob(&full_pattern)
        .map_err(|e| AdkError::tool(format!("glob: invalid pattern '{}': {e}", args.pattern)))?
        .filter_map(|e| e.ok())
    {
        // Only include files, not directories
        if entry.is_file() {
            // Make path relative to sandbox root
            let relative = entry
                .strip_prefix(sandbox.root())
                .unwrap_or(&entry);
            results.push(relative.display().to_string());
        }
    }

    let total = results.len();

    Ok(json!({
        "files": results,
        "total": total,
    }))
}

// ─── Helpers ───────────────────────────────────────────────────

/// A single grep match result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GrepMatch {
    pub path: String,
    pub line_number: usize,
    pub line: String,
}

fn search_file(
    path: &std::path::Path,
    re: &regex::Regex,
    glob_matcher: &Option<glob::Pattern>,
    matches: &mut Vec<GrepMatch>,
    root: &std::path::Path,
) -> Result<(), AdkError> {
    // Apply glob filter
    if let Some(pattern) = glob_matcher {
        let file_name = path.file_name().unwrap_or_default().to_string_lossy();
        if !pattern.matches(&file_name) {
            // Also try matching against the full relative path
            let relative = path.strip_prefix(root).unwrap_or(path);
            if !pattern.matches_path(relative) {
                return Ok(());
            }
        }
    }

    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return Ok(()), // Skip files we can't read (binary, permissions, etc.)
    };

    let relative_path = path
        .strip_prefix(root)
        .unwrap_or(path)
        .display()
        .to_string();

    for (line_num, line) in content.lines().enumerate() {
        if re.is_match(line) {
            matches.push(GrepMatch {
                path: relative_path.clone(),
                line_number: line_num + 1,
                line: line.to_string(),
            });
        }
    }

    Ok(())
}

// ─── Tests ─────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_grep_basic() {
        let tmp = tempfile::tempdir().unwrap();
        let sandbox = Arc::new(
            FilesystemSandbox::new(tmp.path(), crate::sandbox::PermissionMode::Auto).unwrap(),
        );
        set_sandbox(sandbox.clone());

        // Create test files
        tokio::fs::write(tmp.path().join("hello.txt"), "Hello world\nFoo bar\nHello Rust\n")
            .await
            .unwrap();
        tokio::fs::write(tmp.path().join("other.txt"), "No match here\n")
            .await
            .unwrap();

        let result = grep(GrepArgs {
            pattern: "Hello".into(),
            path: None,
            glob: None,
            case_insensitive: None,
        })
        .await
        .unwrap();

        assert_eq!(result["total_matches"], 2);
        let matches = result["matches"].as_array().unwrap();
        assert!(matches[0]["line"].as_str().unwrap().contains("Hello"));
    }

    #[tokio::test]
    async fn test_grep_with_path() {
        let tmp = tempfile::tempdir().unwrap();
        let sandbox = Arc::new(
            FilesystemSandbox::new(tmp.path(), crate::sandbox::PermissionMode::Auto).unwrap(),
        );
        set_sandbox(sandbox.clone());

        tokio::fs::create_dir_all(tmp.path().join("src")).await.unwrap();
        tokio::fs::write(tmp.path().join("src/main.rs"), "fn main() {}\nfn helper() {}\n")
            .await
            .unwrap();
        tokio::fs::write(tmp.path().join("other.txt"), "fn main() is here too\n")
            .await
            .unwrap();

        let result = grep(GrepArgs {
            pattern: "fn main".into(),
            path: Some("src".into()),
            glob: None,
            case_insensitive: None,
        })
        .await
        .unwrap();

        assert_eq!(result["total_matches"], 1);
    }

    #[tokio::test]
    async fn test_grep_with_glob() {
        let tmp = tempfile::tempdir().unwrap();
        let sandbox = Arc::new(
            FilesystemSandbox::new(tmp.path(), crate::sandbox::PermissionMode::Auto).unwrap(),
        );
        set_sandbox(sandbox.clone());

        tokio::fs::write(tmp.path().join("code.rs"), "pub fn hello() {}\n")
            .await
            .unwrap();
        tokio::fs::write(tmp.path().join("code.ts"), "function hello() {}\n")
            .await
            .unwrap();

        let result = grep(GrepArgs {
            pattern: "hello".into(),
            path: None,
            glob: Some("*.rs".into()),
            case_insensitive: None,
        })
        .await
        .unwrap();

        assert_eq!(result["total_matches"], 1);
        let matches = result["matches"].as_array().unwrap();
        assert!(matches[0]["path"].as_str().unwrap().contains("code.rs"));
    }

    #[tokio::test]
    async fn test_grep_case_insensitive() {
        let tmp = tempfile::tempdir().unwrap();
        let sandbox = Arc::new(
            FilesystemSandbox::new(tmp.path(), crate::sandbox::PermissionMode::Auto).unwrap(),
        );
        set_sandbox(sandbox.clone());

        tokio::fs::write(tmp.path().join("test.txt"), "Hello World\n")
            .await
            .unwrap();

        let result = grep(GrepArgs {
            pattern: "hello".into(),
            path: None,
            glob: None,
            case_insensitive: Some(true),
        })
        .await
        .unwrap();

        assert_eq!(result["total_matches"], 1);
    }

    #[tokio::test]
    async fn test_grep_no_matches() {
        let tmp = tempfile::tempdir().unwrap();
        let sandbox = Arc::new(
            FilesystemSandbox::new(tmp.path(), crate::sandbox::PermissionMode::Auto).unwrap(),
        );
        set_sandbox(sandbox.clone());

        tokio::fs::write(tmp.path().join("test.txt"), "Hello world\n")
            .await
            .unwrap();

        let result = grep(GrepArgs {
            pattern: "nonexistent_pattern".into(),
            path: None,
            glob: None,
            case_insensitive: None,
        })
        .await
        .unwrap();

        assert_eq!(result["total_matches"], 0);
    }

    #[tokio::test]
    async fn test_glob_basic() {
        let tmp = tempfile::tempdir().unwrap();
        let sandbox = Arc::new(
            FilesystemSandbox::new(tmp.path(), crate::sandbox::PermissionMode::Auto).unwrap(),
        );
        set_sandbox(sandbox.clone());

        tokio::fs::write(tmp.path().join("main.rs"), "fn main() {}")
            .await
            .unwrap();
        tokio::fs::write(tmp.path().join("lib.rs"), "pub fn lib() {}")
            .await
            .unwrap();
        tokio::fs::write(tmp.path().join("readme.md"), "# Hello")
            .await
            .unwrap();

        let result = glob(GlobArgs {
            pattern: "**/*.rs".into(),
            path: None,
        })
        .await
        .unwrap();

        let files = result["files"].as_array().unwrap();
        assert_eq!(files.len(), 2);
        assert_eq!(result["total"], 2);
    }

    #[tokio::test]
    async fn test_glob_with_path() {
        let tmp = tempfile::tempdir().unwrap();
        let sandbox = Arc::new(
            FilesystemSandbox::new(tmp.path(), crate::sandbox::PermissionMode::Auto).unwrap(),
        );
        set_sandbox(sandbox.clone());

        tokio::fs::create_dir_all(tmp.path().join("src")).await.unwrap();
        tokio::fs::write(tmp.path().join("src/main.rs"), "fn main() {}")
            .await
            .unwrap();
        tokio::fs::write(tmp.path().join("main.rs"), "other")
            .await
            .unwrap();

        let result = glob(GlobArgs {
            pattern: "*.rs".into(),
            path: Some("src".into()),
        })
        .await
        .unwrap();

        let files = result["files"].as_array().unwrap();
        assert_eq!(files.len(), 1);
        assert!(files[0].as_str().unwrap().contains("main.rs"));
    }

    #[tokio::test]
    async fn test_glob_no_matches() {
        let tmp = tempfile::tempdir().unwrap();
        let sandbox = Arc::new(
            FilesystemSandbox::new(tmp.path(), crate::sandbox::PermissionMode::Auto).unwrap(),
        );
        set_sandbox(sandbox.clone());

        tokio::fs::write(tmp.path().join("test.txt"), "hello")
            .await
            .unwrap();

        let result = glob(GlobArgs {
            pattern: "*.rs".into(),
            path: None,
        })
        .await
        .unwrap();

        assert_eq!(result["total"], 0);
    }
}
