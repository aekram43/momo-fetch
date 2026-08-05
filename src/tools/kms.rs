use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use adk_tool::{AdkError, tool};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::sandbox::FilesystemSandbox;

// ─── Process-global sandbox context ────────────────────────────
//
// Process-global rather than `thread_local!` — see the note in `file.rs`.

static SANDBOX_CTX: RwLock<Option<Arc<FilesystemSandbox>>> = RwLock::new(None);

/// Set the sandbox for the process (called when building the tool registry).
pub fn set_sandbox(sandbox: Arc<FilesystemSandbox>) {
    if let Ok(mut ctx) = SANDBOX_CTX.write() {
        *ctx = Some(sandbox);
    }
}

/// Get the sandbox. Returns an owned `Arc`, so no guard is held by the caller.
fn get_sandbox() -> Result<Arc<FilesystemSandbox>, AdkError> {
    SANDBOX_CTX
        .read()
        .ok()
        .and_then(|ctx| ctx.clone())
        .ok_or_else(|| AdkError::tool("kms tool sandbox not initialized"))
}

// ─── KmsRead ──────────────────────────────────────────────────

/// Arguments for reading a KMS page.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct KmsReadArgs {
    /// Knowledge base name (directory under .kms/)
    pub kb: String,
    /// Page name (file under .kms/<kb>/pages/, with or without .md extension)
    pub page: String,
}

/// Read a page from a knowledge base.
/// Returns the page content with metadata.
#[tool]
pub async fn kms_read(args: KmsReadArgs) -> Result<Value, AdkError> {
    let sandbox = get_sandbox()?;
    let root = sandbox.root();

    // Sanitize KB name (prevent path traversal)
    let kb_name = sanitize_name(&args.kb);
    if kb_name.is_empty() {
        return Err(AdkError::tool("kms_read: knowledge base name cannot be empty"));
    }

    // Build page filename
    let page_name = if args.page.ends_with(".md") {
        sanitize_name(&args.page.trim_end_matches(".md"))
    } else {
        sanitize_name(&args.page)
    };
    if page_name.is_empty() {
        return Err(AdkError::tool("kms_read: page name cannot be empty"));
    }

    let page_path = root
        .join(".kms")
        .join(&kb_name)
        .join("pages")
        .join(format!("{page_name}.md"));

    // Verify the resolved path is within the sandbox
    let canonical = safe_canonicalize(&page_path, root);
    if !canonical.starts_with(root.join(".kms")) {
        return Err(AdkError::tool("kms_read: path traversal blocked"));
    }

    if !page_path.exists() {
        return Err(AdkError::tool(format!(
            "kms_read: page '{page_name}' not found in knowledge base '{kb_name}'"
        )));
    }

    let content = fs::read_to_string(&page_path).map_err(|e| {
        AdkError::tool(format!("kms_read: failed to read page: {e}"))
    })?;

    Ok(json!({
        "kb": kb_name,
        "page": page_name,
        "path": format!(".kms/{kb_name}/pages/{page_name}.md"),
        "content": content
    }))
}

// ─── KmsSearch ────────────────────────────────────────────────

/// Arguments for searching KMS pages.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct KmsSearchArgs {
    /// Search query (plain text, case-insensitive)
    pub query: String,
    /// Optional knowledge base name to restrict search
    pub kb: Option<String>,
    /// Maximum number of results (default: 20)
    pub limit: Option<usize>,
}

/// Full-text search across knowledge base pages.
/// Searches all pages in all KBs (or a specific KB) for the query string.
#[tool]
pub async fn kms_search(args: KmsSearchArgs) -> Result<Value, AdkError> {
    let sandbox = get_sandbox()?;
    let root = sandbox.root();
    let limit = args.limit.unwrap_or(20);
    let query_lower = args.query.to_lowercase();

    let kms_dir = root.join(".kms");
    if !kms_dir.exists() {
        return Ok(json!({
            "query": args.query,
            "results": [],
            "total": 0
        }));
    }

    let mut results: Vec<Value> = Vec::new();

    let kb_filter = args.kb.as_deref().map(sanitize_name);

    let kb_entries = fs::read_dir(&kms_dir).map_err(|e| {
        AdkError::tool(format!("kms_search: failed to read .kms directory: {e}"))
    })?;

    for kb_entry in kb_entries {
        let kb_entry = kb_entry.map_err(|e| {
            AdkError::tool(format!("kms_search: failed to read KB entry: {e}"))
        })?;

        if !kb_entry.file_type().map(|ft| ft.is_dir()).unwrap_or(false) {
            continue;
        }

        let kb_name = kb_entry
            .file_name()
            .to_string_lossy()
            .to_string();

        // Filter by KB name if specified
        if let Some(ref filter) = kb_filter {
            if &kb_name != filter {
                continue;
            }
        }

        let pages_dir = kb_entry.path().join("pages");
        if !pages_dir.exists() {
            continue;
        }

        let pages = fs::read_dir(&pages_dir).map_err(|e| {
            AdkError::tool(format!(
                "kms_search: failed to read pages in '{kb_name}': {e}"
            ))
        })?;

        for page_entry in pages {
            let page_entry = page_entry.map_err(|e| {
                AdkError::tool(format!("kms_search: failed to read page entry: {e}"))
            })?;

            let path = page_entry.path();
            if !path.extension().is_some_and(|ext| ext == "md") {
                continue;
            }

            let page_name = path
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_default();

            let content = match fs::read_to_string(&path) {
                Ok(c) => c,
                Err(_) => continue,
            };

            let content_lower = content.to_lowercase();
            if !content_lower.contains(&query_lower) {
                continue;
            }

            // Count matches and extract snippet
            let match_count = count_matches(&content_lower, &query_lower);
            let snippet = extract_snippet(&content, &args.query, 200);

            results.push(json!({
                "kb": kb_name,
                "page": page_name,
                "path": format!(".kms/{kb_name}/pages/{page_name}.md"),
                "match_count": match_count,
                "snippet": snippet
            }));

            if results.len() >= limit {
                break;
            }
        }

        if results.len() >= limit {
            break;
        }
    }

    let total = results.len();
    Ok(json!({
        "query": args.query,
        "results": results,
        "total": total
    }))
}

// ─── KmsWrite ─────────────────────────────────────────────────

/// Arguments for writing a KMS page.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct KmsWriteArgs {
    /// Knowledge base name (directory under .kms/)
    pub kb: String,
    /// Page name (file under .kms/<kb>/pages/, with or without .md extension)
    pub page: String,
    /// Content to write (markdown)
    pub content: String,
    /// Optional description for the index.md entry
    pub description: Option<String>,
}

/// Create or update a page in a knowledge base.
/// Creates the KB directory structure if it doesn't exist.
/// Updates the KB's index.md with the new page entry.
#[tool]
pub async fn kms_write(args: KmsWriteArgs) -> Result<Value, AdkError> {
    let sandbox = get_sandbox()?;
    let root = sandbox.root();

    // Sanitize names
    let kb_name = sanitize_name(&args.kb);
    if kb_name.is_empty() {
        return Err(AdkError::tool("kms_write: knowledge base name cannot be empty"));
    }

    let page_name = if args.page.ends_with(".md") {
        sanitize_name(&args.page.trim_end_matches(".md"))
    } else {
        sanitize_name(&args.page)
    };
    if page_name.is_empty() {
        return Err(AdkError::tool("kms_write: page name cannot be empty"));
    }

    let kb_dir = root.join(".kms").join(&kb_name);
    let pages_dir = kb_dir.join("pages");

    // Verify the resolved path is within the sandbox
    let canonical_kb = safe_canonicalize(&kb_dir, root);
    if !canonical_kb.starts_with(root.join(".kms")) {
        return Err(AdkError::tool("kms_write: path traversal blocked"));
    }

    // Create directory structure if needed
    fs::create_dir_all(&pages_dir).map_err(|e| {
        AdkError::tool(format!("kms_write: failed to create KB directory: {e}"))
    })?;

    let page_path = pages_dir.join(format!("{page_name}.md"));
    let is_update = page_path.exists();

    // Atomic write (temp + rename)
    let tmp_path = page_path.with_extension("tmp");
    fs::write(&tmp_path, &args.content).map_err(|e| {
        AdkError::tool(format!("kms_write: failed to write page: {e}"))
    })?;
    fs::rename(&tmp_path, &page_path).map_err(|e| {
        AdkError::tool(format!("kms_write: failed to save page: {e}"))
    })?;

    // Update index.md
    update_index(&kb_dir, &kb_name, &page_name, args.description.as_deref())?;

    let action = if is_update { "updated" } else { "created" };

    Ok(json!({
        "kb": kb_name,
        "page": page_name,
        "path": format!(".kms/{kb_name}/pages/{page_name}.md"),
        "action": action,
        "success": true
    }))
}

// ─── Helper functions ─────────────────────────────────────────

/// Sanitize a KB or page name: remove path separators and special chars.
fn sanitize_name(name: &str) -> String {
    name.chars()
        .filter(|c| {
            c.is_alphanumeric() || *c == '-' || *c == '_' || *c == ' '
        })
        .collect::<String>()
        .trim()
        .to_string()
}

/// Canonicalize a path, handling non-existent paths by canonicalizing the parent.
fn safe_canonicalize(path: &Path, root: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| {
        if let Some(parent) = path.parent() {
            parent
                .canonicalize()
                .map(|p| p.join(path.file_name().unwrap_or_default()))
                .unwrap_or_else(|_| root.join(path))
        } else {
            root.join(path)
        }
    })
}

/// Count occurrences of a substring (case-insensitive).
fn count_matches(text: &str, query: &str) -> usize {
    if query.is_empty() {
        return 0;
    }
    text.matches(query).count()
}

/// Extract a snippet around the first match of the query in the content.
fn extract_snippet(content: &str, query: &str, max_len: usize) -> String {
    if query.is_empty() || query.len() > content.len() {
        return content.chars().take(max_len).collect();
    }

    // Find first match (case-insensitive)
    let content_lower = content.to_lowercase();
    let query_lower = query.to_lowercase();

    if let Some(pos) = content_lower.find(&query_lower) {
        // Find word boundary start (up to 50 chars before match)
        let start = pos.saturating_sub(50);
        let start = content[..start + 1]
            .rfind(|c: char| c == '\n' || c == '.' || c == ' ')
            .map(|p| p + 1)
            .unwrap_or(start);

        // Find sentence end (up to max_len chars after start)
        let end = (start + max_len).min(content.len());
        let end = content[start..end]
            .find(|c: char| c == '\n' || c == '.')
            .map(|p| start + p + 1)
            .unwrap_or(end);

        let snippet = &content[start..end.min(content.len())];
        let prefix = if start > 0 { "..." } else { "" };
        let suffix = if end < content.len() { "..." } else { "" };

        format!("{prefix}{snippet}{suffix}")
    } else {
        content.chars().take(max_len).collect()
    }
}

/// Update the index.md for a knowledge base with a page entry.
fn update_index(kb_dir: &Path, kb_name: &str, page_name: &str, description: Option<&str>) -> Result<(), AdkError> {
    // Ensure KB directory exists (create if needed)
    fs::create_dir_all(kb_dir).map_err(|e| {
        AdkError::tool(format!("kms_write: failed to create KB directory: {e}"))
    })?;

    let index_path = kb_dir.join("index.md");
    let entry_line = format!("- [{page_name}](pages/{page_name}.md)");

    let desc_part = description
        .map(|d| format!(" — {d}"))
        .unwrap_or_default();

    let new_entry = format!("{entry_line}{desc_part}");

    let new_content = if index_path.exists() {
        let existing = fs::read_to_string(&index_path).map_err(|e| {
            AdkError::tool(format!("kms_write: failed to read index.md: {e}"))
        })?;

        // Check if entry already exists
        if existing.contains(&entry_line) {
            // Update: replace existing entry
            let pattern = format!("- [{page_name}](pages/{page_name}.md)");
            if let Some(pos) = existing.find(&pattern) {
                // Find end of line
                let end = existing[pos..]
                    .find('\n')
                    .map(|p| pos + p + 1)
                    .unwrap_or(existing.len());
                format!("{}{}{}", &existing[..pos], new_entry, &existing[end..])
            } else {
                existing
            }
        } else {
            // Append new entry
            let separator = if existing.ends_with('\n') {
                ""
            } else {
                "\n"
            };
            format!("{existing}{separator}{new_entry}\n")
        }
    } else {
        // Create new index.md
        format!("# {kb_name}\n\n{new_entry}\n")
    };

    // Atomic write
    let tmp_path = index_path.with_extension("tmp");
    fs::write(&tmp_path, &new_content).map_err(|e| {
        AdkError::tool(format!("kms_write: failed to write index.md: {e}"))
    })?;
    fs::rename(&tmp_path, &index_path).map_err(|e| {
        AdkError::tool(format!("kms_write: failed to save index.md: {e}"))
    })?;

    Ok(())
}

/// List all knowledge bases in the project.
/// Returns (kb_name, page_count, has_index) for each KB.
pub fn list_knowledge_bases(project_path: &Path) -> Vec<KbInfo> {
    let kms_dir = project_path.join(".kms");
    if !kms_dir.exists() {
        return Vec::new();
    }

    let mut result = Vec::new();

    let entries = match fs::read_dir(&kms_dir) {
        Ok(e) => e,
        Err(_) => return Vec::new(),
    };

    for entry in entries.flatten() {
        if !entry.file_type().map(|ft| ft.is_dir()).unwrap_or(false) {
            continue;
        }

        let kb_name = entry.file_name().to_string_lossy().to_string();
        let pages_dir = entry.path().join("pages");
        let has_index = entry.path().join("index.md").exists();

        let page_count = if pages_dir.exists() {
            fs::read_dir(&pages_dir)
                .map(|entries| {
                    entries
                        .filter_map(|e| e.ok())
                        .filter(|e| {
                            e.path()
                                .extension()
                                .is_some_and(|ext| ext == "md")
                        })
                        .count()
                })
                .unwrap_or(0)
        } else {
            0
        };

        result.push(KbInfo {
            name: kb_name,
            page_count,
            has_index,
        });
    }

    result.sort_by(|a, b| a.name.cmp(&b.name));
    result
}

/// Information about a knowledge base.
pub struct KbInfo {
    pub name: String,
    pub page_count: usize,
    pub has_index: bool,
}

// ─── Tests ────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn setup_kb(tmp: &TempDir) -> PathBuf {
        let kb_dir = tmp.path().join(".kms").join("test-kb");
        let pages_dir = kb_dir.join("pages");
        fs::create_dir_all(&pages_dir).unwrap();

        fs::write(
            pages_dir.join("getting-started.md"),
            "# Getting Started\n\nThis guide covers the basics of the project.\n\n## Setup\n\nRun `cargo build` to get started.",
        )
        .unwrap();

        fs::write(
            pages_dir.join("architecture.md"),
            "# Architecture\n\nThe system uses a modular design with clear separation of concerns.\n\n## Components\n\n- Agent orchestrator\n- Tool registry\n- Memory vault",
        )
        .unwrap();

        fs::write(
            kb_dir.join("index.md"),
            "# test-kb\n\n- [getting-started](pages/getting-started.md) — Quick start guide\n- [architecture](pages/architecture.md) — System design overview\n",
        )
        .unwrap();

        tmp.path().to_path_buf()
    }

    #[test]
    fn test_sanitize_name() {
        assert_eq!(sanitize_name("hello-world"), "hello-world");
        assert_eq!(sanitize_name("hello_world"), "hello_world");
        assert_eq!(sanitize_name("hello world"), "hello world");
        assert_eq!(sanitize_name("../etc/passwd"), "etcpasswd");
        assert_eq!(sanitize_name("test/../../bad"), "testbad");
        assert_eq!(sanitize_name(""), "");
    }

    #[test]
    fn test_extract_snippet() {
        let content = "This is a long document about Rust programming. It covers ownership, borrowing, and lifetimes. The memory model is unique.";
        let snippet = extract_snippet(content, "Rust programming", 50);
        assert!(snippet.contains("Rust programming"));
    }

    #[test]
    fn test_extract_snippet_no_match() {
        let content = "Some text without the query.";
        let snippet = extract_snippet(content, "nonexistent", 50);
        assert_eq!(snippet, "Some text without the query.");
    }

    #[test]
    fn test_count_matches() {
        assert_eq!(count_matches("hello world hello", "hello"), 2);
        assert_eq!(count_matches("hello world", "hello"), 1);
        assert_eq!(count_matches("test", "nonexistent"), 0);
        assert_eq!(count_matches("test", ""), 0);
    }

    #[test]
    fn test_update_index_new() {
        let tmp = TempDir::new().unwrap();
        let kb_dir = tmp.path().join("conventions");

        update_index(&kb_dir, "conventions", "coding-style", Some("Code style guide"))
            .unwrap();

        let index = fs::read_to_string(kb_dir.join("index.md")).unwrap();
        assert!(index.contains("# conventions"));
        assert!(index.contains("- [coding-style](pages/coding-style.md) — Code style guide"));
    }

    #[test]
    fn test_update_index_existing() {
        let tmp = TempDir::new().unwrap();
        let kb_dir = tmp.path().join("conventions");
        fs::create_dir_all(&kb_dir).unwrap();

        // Create existing index
        fs::write(
            kb_dir.join("index.md"),
            "# conventions\n\n- [coding-style](pages/coding-style.md) — Old description\n",
        )
        .unwrap();

        // Update existing entry
        update_index(&kb_dir, "conventions", "coding-style", Some("Updated guide"))
            .unwrap();

        let index = fs::read_to_string(kb_dir.join("index.md")).unwrap();
        assert!(index.contains("Updated guide"));
        assert!(!index.contains("Old description"));
    }

    #[test]
    fn test_update_index_append() {
        let tmp = TempDir::new().unwrap();
        let kb_dir = tmp.path().join("conventions");
        fs::create_dir_all(&kb_dir).unwrap();

        // Create existing index with one entry
        fs::write(
            kb_dir.join("index.md"),
            "# conventions\n\n- [existing](pages/existing.md)\n",
        )
        .unwrap();

        // Add new entry
        update_index(&kb_dir, "conventions", "new-page", None).unwrap();

        let index = fs::read_to_string(kb_dir.join("index.md")).unwrap();
        assert!(index.contains("[existing](pages/existing.md)"));
        assert!(index.contains("[new-page](pages/new-page.md)"));
    }

    #[tokio::test]
    async fn test_kms_read() {
        let tmp = TempDir::new().unwrap();
        let root = setup_kb(&tmp);

        let sandbox =
            FilesystemSandbox::new(&root, crate::sandbox::PermissionMode::Auto).unwrap();
        let _sandbox_guard = crate::tools::test_support::sandbox_guard();
        set_sandbox(Arc::new(sandbox));

        let result = kms_read(KmsReadArgs {
            kb: "test-kb".to_string(),
            page: "getting-started".to_string(),
        })
        .await
        .unwrap();

        assert_eq!(result["kb"], "test-kb");
        assert_eq!(result["page"], "getting-started");
        assert!(result["content"].as_str().unwrap().contains("Getting Started"));
        assert!(result["content"].as_str().unwrap().contains("cargo build"));
    }

    #[tokio::test]
    async fn test_kms_read_with_md_extension() {
        let tmp = TempDir::new().unwrap();
        let root = setup_kb(&tmp);

        let sandbox =
            FilesystemSandbox::new(&root, crate::sandbox::PermissionMode::Auto).unwrap();
        let _sandbox_guard = crate::tools::test_support::sandbox_guard();
        set_sandbox(Arc::new(sandbox));

        let result = kms_read(KmsReadArgs {
            kb: "test-kb".to_string(),
            page: "architecture.md".to_string(),
        })
        .await
        .unwrap();

        assert_eq!(result["page"], "architecture");
        assert!(result["content"].as_str().unwrap().contains("modular design"));
    }

    #[tokio::test]
    async fn test_kms_read_not_found() {
        let tmp = TempDir::new().unwrap();
        let root = setup_kb(&tmp);

        let sandbox =
            FilesystemSandbox::new(&root, crate::sandbox::PermissionMode::Auto).unwrap();
        let _sandbox_guard = crate::tools::test_support::sandbox_guard();
        set_sandbox(Arc::new(sandbox));

        let result = kms_read(KmsReadArgs {
            kb: "test-kb".to_string(),
            page: "nonexistent".to_string(),
        })
        .await;

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not found"));
    }

    #[tokio::test]
    async fn test_kms_search() {
        let tmp = TempDir::new().unwrap();
        let root = setup_kb(&tmp);

        let sandbox =
            FilesystemSandbox::new(&root, crate::sandbox::PermissionMode::Auto).unwrap();
        let _sandbox_guard = crate::tools::test_support::sandbox_guard();
        set_sandbox(Arc::new(sandbox));

        let result = kms_search(KmsSearchArgs {
            query: "cargo build".to_string(),
            kb: None,
            limit: Some(10),
        })
        .await
        .unwrap();

        assert_eq!(result["total"], 1);
        let results = result["results"].as_array().unwrap();
        assert_eq!(results[0]["page"], "getting-started");
    }

    #[tokio::test]
    async fn test_kms_search_filter_by_kb() {
        let tmp = TempDir::new().unwrap();
        let root = setup_kb(&tmp);

        // Create a second KB
        let kb2 = root.join(".kms").join("other-kb");
        let pages2 = kb2.join("pages");
        fs::create_dir_all(&pages2).unwrap();
        fs::write(pages2.join("setup.md"), "# Setup\nRun cargo build to compile.").unwrap();

        let sandbox =
            FilesystemSandbox::new(&root, crate::sandbox::PermissionMode::Auto).unwrap();
        let _sandbox_guard = crate::tools::test_support::sandbox_guard();
        set_sandbox(Arc::new(sandbox));

        let result = kms_search(KmsSearchArgs {
            query: "cargo".to_string(),
            kb: Some("other-kb".to_string()),
            limit: Some(10),
        })
        .await
        .unwrap();

        assert_eq!(result["total"], 1);
        let results = result["results"].as_array().unwrap();
        assert_eq!(results[0]["kb"], "other-kb");
    }

    #[tokio::test]
    async fn test_kms_search_no_kms_dir() {
        let tmp = TempDir::new().unwrap();

        let sandbox = FilesystemSandbox::new(
            tmp.path(),
            crate::sandbox::PermissionMode::Auto,
        )
        .unwrap();
        let _sandbox_guard = crate::tools::test_support::sandbox_guard();
        set_sandbox(Arc::new(sandbox));

        let result = kms_search(KmsSearchArgs {
            query: "test".to_string(),
            kb: None,
            limit: None,
        })
        .await
        .unwrap();

        assert_eq!(result["total"], 0);
        assert!(result["results"].as_array().unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_kms_write_create() {
        let tmp = TempDir::new().unwrap();
        let root = setup_kb(&tmp);

        let sandbox =
            FilesystemSandbox::new(&root, crate::sandbox::PermissionMode::Auto).unwrap();
        let _sandbox_guard = crate::tools::test_support::sandbox_guard();
        set_sandbox(Arc::new(sandbox));

        let result = kms_write(KmsWriteArgs {
            kb: "test-kb".to_string(),
            page: "new-page".to_string(),
            content: "# New Page\n\nSome content here.".to_string(),
            description: Some("A new page".to_string()),
        })
        .await
        .unwrap();

        assert_eq!(result["action"], "created");
        assert_eq!(result["page"], "new-page");
        assert!(result["success"].as_bool().unwrap());

        // Verify file exists
        let page_path = root.join(".kms/test-kb/pages/new-page.md");
        assert!(page_path.exists());
        let content = fs::read_to_string(&page_path).unwrap();
        assert_eq!(content, "# New Page\n\nSome content here.");

        // Verify index.md was updated
        let index = fs::read_to_string(root.join(".kms/test-kb/index.md")).unwrap();
        assert!(index.contains("[new-page](pages/new-page.md) — A new page"));
    }

    #[tokio::test]
    async fn test_kms_write_update() {
        let tmp = TempDir::new().unwrap();
        let root = setup_kb(&tmp);

        let sandbox =
            FilesystemSandbox::new(&root, crate::sandbox::PermissionMode::Auto).unwrap();
        let _sandbox_guard = crate::tools::test_support::sandbox_guard();
        set_sandbox(Arc::new(sandbox));

        let result = kms_write(KmsWriteArgs {
            kb: "test-kb".to_string(),
            page: "getting-started".to_string(),
            content: "# Getting Started (v2)\nUpdated content.".to_string(),
            description: None,
        })
        .await
        .unwrap();

        assert_eq!(result["action"], "updated");

        // Verify content was replaced
        let content = fs::read_to_string(root.join(".kms/test-kb/pages/getting-started.md"))
            .unwrap();
        assert!(content.contains("(v2)"));
        assert!(!content.contains("cargo build"));
    }

    #[tokio::test]
    async fn test_kms_write_creates_kb_dir() {
        let tmp = TempDir::new().unwrap();

        let sandbox = FilesystemSandbox::new(
            tmp.path(),
            crate::sandbox::PermissionMode::Auto,
        )
        .unwrap();
        let _sandbox_guard = crate::tools::test_support::sandbox_guard();
        set_sandbox(Arc::new(sandbox));

        let result = kms_write(KmsWriteArgs {
            kb: "brand-new-kb".to_string(),
            page: "intro".to_string(),
            content: "# Intro\n\nWelcome.".to_string(),
            description: Some("Introduction page".to_string()),
        })
        .await
        .unwrap();

        assert_eq!(result["action"], "created");

        // Verify KB directory structure was created
        assert!(tmp.path().join(".kms/brand-new-kb/pages/intro.md").exists());
        assert!(tmp.path().join(".kms/brand-new-kb/index.md").exists());

        // Verify index.md content
        let index = fs::read_to_string(tmp.path().join(".kms/brand-new-kb/index.md")).unwrap();
        assert!(index.contains("# brand-new-kb"));
        assert!(index.contains("[intro](pages/intro.md) — Introduction page"));
    }

    #[test]
    fn test_list_knowledge_bases() {
        let tmp = TempDir::new().unwrap();
        setup_kb(&tmp);

        // Create a second KB
        let kb2 = tmp.path().join(".kms").join("another-kb");
        fs::create_dir_all(kb2.join("pages")).unwrap();
        fs::write(kb2.join("pages/a.md"), "# A\n").unwrap();
        fs::write(kb2.join("pages/b.md"), "# B\n").unwrap();
        // No index.md for this KB

        let result = list_knowledge_bases(tmp.path());

        assert_eq!(result.len(), 2);
        assert_eq!(result[0].name, "another-kb");
        assert_eq!(result[0].page_count, 2);
        assert!(!result[0].has_index);
        assert_eq!(result[1].name, "test-kb");
        assert_eq!(result[1].page_count, 2);
        assert!(result[1].has_index);
    }

    #[test]
    fn test_list_knowledge_bases_empty() {
        let tmp = TempDir::new().unwrap();
        let result = list_knowledge_bases(tmp.path());
        assert!(result.is_empty());
    }
}
