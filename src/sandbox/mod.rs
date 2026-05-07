use std::path::{Path, PathBuf};
use serde::{Deserialize, Serialize};

use adk_rust::ToolConfirmationPolicy;

/// Permission mode for tool execution.
///
/// Maps to adk-rust's `ToolConfirmationPolicy`:
/// - `Strict`  → `Always` (all mutating tools require confirmation)
/// - `Auto`    → `PerTool` (only destructive tools require confirmation)
/// - `Yolo`    → `Never`  (auto-approve everything)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PermissionMode {
    /// Prompt for all mutating operations
    Strict,
    /// Auto-approve non-destructive operations
    Auto,
    /// Auto-approve everything
    Yolo,
}

impl Default for PermissionMode {
    fn default() -> Self {
        Self::Strict
    }
}

impl std::fmt::Display for PermissionMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Strict => write!(f, "strict"),
            Self::Auto => write!(f, "auto"),
            Self::Yolo => write!(f, "yolo"),
        }
    }
}

impl std::str::FromStr for PermissionMode {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "strict" => Ok(Self::Strict),
            "auto" => Ok(Self::Auto),
            "yolo" => Ok(Self::Yolo),
            _ => Err(format!(
                "Unknown permission mode '{s}'. Expected: strict, auto, or yolo"
            )),
        }
    }
}

/// Result of destructive command detection.
#[derive(Debug, Clone)]
pub struct DestructiveCheck {
    pub is_destructive: bool,
    pub pattern: Option<String>,
    pub category: Option<String>,
}

/// Tools that are always classified as "mutating" for permission checks.
const MUTATING_TOOLS: &[&str] = &[
    "file_write",
    "file_edit",
    "shell_exec",
    "mem_write",
    "mem_extract",
];

/// Tools that are always classified as "read-only".
const READ_ONLY_TOOLS: &[&str] = &[
    "file_read",
    "grep",
    "glob",
    "web_search",
    "web_fetch",
    "mem_search",
    "mem_read",
];

/// Filesystem sandbox that scopes all operations to a project directory.
///
/// Provides:
/// - Path scoping (all file ops restricted to working directory)
/// - Path traversal protection (../../etc/passwd blocked)
/// - .agentignore file support (using `ignore` crate)
/// - Destructive command detection
/// - Permission mode integration with adk-rust's ToolConfirmationPolicy
pub struct FilesystemSandbox {
    root: PathBuf,
    permission_mode: PermissionMode,
    ignore_matcher: Option<ignore::gitignore::Gitignore>,
}

impl FilesystemSandbox {
    /// Create a new sandbox rooted at the given path with the specified permission mode.
    ///
    /// Loads .agentignore rules from the root directory if present.
    pub fn new(root: &Path, mode: PermissionMode) -> anyhow::Result<Self> {
        // Load .agentignore BEFORE canonicalizing the root,
        // since the file may only be accessible via the non-resolved path.
        let ignore_matcher = Self::load_agentignore(root)?;

        let root = root.canonicalize()?;

        Ok(Self {
            root,
            permission_mode: mode,
            ignore_matcher,
        })
    }

    /// Resolve a path and ensure it's within the sandbox root.
    ///
    /// Steps:
    /// 1. Clean the path to resolve `.` and `..`
    /// 2. Reject paths that escape upward via `..`
    /// 3. Join with root
    /// 4. Canonicalize to resolve symlinks
    /// 5. Verify the resolved path starts with root
    pub fn resolve_path(&self, relative: &str) -> anyhow::Result<PathBuf> {
        let cleaned = path_clean::PathClean::clean(&PathBuf::from(relative));

        // Early check: reject paths that escape upward via ..
        let cleaned_str = cleaned.to_string_lossy();
        if cleaned_str.starts_with("..") {
            anyhow::bail!(
                "Path traversal blocked: '{}' resolves outside sandbox",
                relative
            );
        }

        // Join with root
        let resolved = self.root.join(&cleaned);

        // For existing paths, canonicalize to resolve symlinks
        let canonical = if resolved.exists() {
            resolved.canonicalize()?
        } else if let Some(parent) = resolved.parent() {
            if parent.exists() {
                parent.canonicalize()?.join(
                    resolved.file_name().unwrap_or_default(),
                )
            } else {
                // Neither path nor parent exists — use the resolved path as-is
                resolved
            }
        } else {
            resolved
        };

        // Verify within root
        if !canonical.starts_with(&self.root) {
            anyhow::bail!(
                "Path traversal blocked: '{}' resolves outside sandbox",
                relative
            );
        }

        Ok(canonical)
    }

    /// Check if a path is ignored by .agentignore rules.
    ///
    /// Returns true if the path should be excluded from operations.
    /// Paths outside the sandbox root are always considered "ignored".
    pub fn is_ignored(&self, path: &Path) -> bool {
        // Canonicalize the path to resolve symlinks (macOS /var → /private/var etc.)
        let canonical = match path.canonicalize() {
            Ok(c) => c,
            Err(_) => {
                // If the path doesn't exist yet, try to resolve via parent
                if let Some(parent) = path.parent() {
                    if let Ok(canonical_parent) = parent.canonicalize() {
                        canonical_parent.join(path.file_name().unwrap_or_default())
                    } else {
                        // Can't resolve — treat as outside root
                        return true;
                    }
                } else {
                    return true;
                }
            }
        };

        // Paths outside root are always "ignored" (inaccessible)
        if !canonical.starts_with(&self.root) {
            return true;
        }

        if let Some(ref matcher) = self.ignore_matcher {
            // Get relative path from root for matching
            if let Ok(relative) = canonical.strip_prefix(&self.root) {
                return matcher.matched(relative, canonical.is_dir()).is_ignore();
            }
        }
        false
    }

    /// Check if a path is readable (not ignored, within sandbox).
    pub fn check_readable(&self, path: &Path) -> anyhow::Result<()> {
        if self.is_ignored(path) {
            anyhow::bail!("Path is ignored by .agentignore rules: {}", path.display());
        }
        Ok(())
    }

    /// Check if a path is writable (not ignored, within sandbox).
    pub fn check_writable(&self, path: &Path) -> anyhow::Result<()> {
        if self.is_ignored(path) {
            anyhow::bail!("Path is ignored by .agentignore rules: {}", path.display());
        }
        Ok(())
    }

    /// Detect destructive shell commands.
    ///
    /// Returns a `DestructiveCheck` indicating whether the command matches
    /// any known destructive patterns.
    pub fn check_destructive(&self, command: &str) -> DestructiveCheck {
        let patterns: &[(&str, &str)] = &[
            (r"(?i)\brm\s+(-[rfRF]+\s+)?/\s*$", "Destructive deletion"),
            (r"(?i)\brm\s+(-[rfRF]+\s+)?~\s*$", "Destructive deletion"),
            (r"(?i)git\s+push\s+.*--force", "Destructive git"),
            (r"(?i)git\s+reset\s+--hard", "Destructive git"),
            (r"(?i)DROP\s+TABLE", "Destructive SQL"),
            (r"(?i)DELETE\s+FROM\s+\w+", "Destructive SQL"),
            (r"(?i)\bmkfs\b", "Destructive system"),
            (r"(?i)\bdd\s+.*if=", "Destructive system"),
            (r"(?i)chmod\s+(-R\s+)?777\s+/", "Destructive system"),
        ];

        for (pattern, category) in patterns {
            if let Ok(re) = regex::Regex::new(pattern) {
                if re.is_match(command) {
                    return DestructiveCheck {
                        is_destructive: true,
                        pattern: Some(pattern.to_string()),
                        category: Some(category.to_string()),
                    };
                }
            }
        }

        DestructiveCheck {
            is_destructive: false,
            pattern: None,
            category: None,
        }
    }

    /// Determine if a tool call should require user confirmation
    /// based on the current permission mode.
    ///
    /// - Strict: all mutating tools require confirmation
    /// - Auto: only destructive tools require confirmation
    /// - Yolo: nothing requires confirmation
    pub fn requires_confirmation(&self, tool_name: &str, args: &serde_json::Value) -> bool {
        match self.permission_mode {
            PermissionMode::Yolo => false,
            PermissionMode::Auto => {
                // In auto mode, check for destructive commands in shell_exec
                if tool_name == "shell_exec" {
                    if let Some(cmd) = args.get("command").and_then(|v| v.as_str()) {
                        return self.check_destructive(cmd).is_destructive;
                    }
                }
                false
            }
            PermissionMode::Strict => {
                // In strict mode, all mutating tools require confirmation
                MUTATING_TOOLS.contains(&tool_name)
            }
        }
    }

    /// Convert our permission mode into adk-rust's `ToolConfirmationPolicy`.
    ///
    /// This is used when building the LlmAgent to configure HITL behavior.
    pub fn to_tool_confirmation_policy(&self) -> ToolConfirmationPolicy {
        match self.permission_mode {
            PermissionMode::Yolo => ToolConfirmationPolicy::Never,
            PermissionMode::Strict => ToolConfirmationPolicy::Always,
            PermissionMode::Auto => {
                // In auto mode, only require confirmation for mutating tools
                // (destructive check happens at runtime in the callback)
                let mut policy = ToolConfirmationPolicy::Never;
                for tool_name in MUTATING_TOOLS {
                    policy = policy.with_tool(*tool_name);
                }
                policy
            }
        }
    }

    /// Get the sandbox root path.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Get the current permission mode.
    pub fn permission_mode(&self) -> PermissionMode {
        self.permission_mode
    }

    /// Get the .agentignore matcher (for testing/inspection).
    pub fn ignore_matcher(&self) -> Option<&ignore::gitignore::Gitignore> {
        self.ignore_matcher.as_ref()
    }

    /// Load .agentignore rules from the sandbox root directory.
    ///
    /// Returns `None` if no .agentignore file exists or if it's empty.
    /// Uses the `ignore` crate's Gitignore builder for full .gitignore compatibility.
    fn load_agentignore(root: &Path) -> anyhow::Result<Option<ignore::gitignore::Gitignore>> {
        let agentignore_path = root.join(".agentignore");
        if !agentignore_path.exists() {
            return Ok(None);
        }

        let content = std::fs::read_to_string(&agentignore_path)?;
        if content.trim().is_empty() {
            return Ok(None);
        }

        let mut builder = ignore::gitignore::GitignoreBuilder::new(root);
        for line in content.lines() {
            let trimmed = line.trim();
            // Skip empty lines and comments
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            builder.add_line(None, trimmed)
                .map_err(|e| anyhow::anyhow!("Failed to parse .agentignore line '{}': {}", trimmed, e))?;
        }

        let matcher = builder.build()?;
        Ok(Some(matcher))
    }
}

// ─── Tests ─────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_sandbox(tmp: &tempfile::TempDir, mode: PermissionMode) -> FilesystemSandbox {
        FilesystemSandbox::new(tmp.path(), mode).unwrap()
    }

    // ── Path scoping tests ──

    #[test]
    fn test_path_traversal_blocked() {
        let tmp = tempfile::tempdir().unwrap();
        let sandbox = make_sandbox(&tmp, PermissionMode::Strict);

        assert!(sandbox.resolve_path("../../etc/passwd").is_err());
        assert!(sandbox.resolve_path("../../../etc/shadow").is_err());
        assert!(sandbox.resolve_path("../hidden").is_err());
    }

    #[test]
    fn test_valid_path_resolved() {
        let tmp = tempfile::tempdir().unwrap();
        let sandbox = make_sandbox(&tmp, PermissionMode::Strict);

        let result = sandbox.resolve_path("src/main.rs");
        assert!(result.is_ok());
        // Must start with the sandbox root (which is canonicalized)
        assert!(result.unwrap().starts_with(sandbox.root()));
    }

    #[test]
    fn test_dot_path_resolved() {
        let tmp = tempfile::tempdir().unwrap();
        let sandbox = make_sandbox(&tmp, PermissionMode::Strict);

        let result = sandbox.resolve_path("./src/main.rs");
        assert!(result.is_ok());
    }

    #[test]
    fn test_absolute_path_resolved_inside_root() {
        let tmp = tempfile::tempdir().unwrap();
        let abs_path = tmp.path().join("src/main.rs");

        let sandbox = make_sandbox(&tmp, PermissionMode::Strict);

        // Create the file so canonicalize works
        std::fs::create_dir_all(tmp.path().join("src")).unwrap();
        std::fs::write(&abs_path, "test").unwrap();

        let result = sandbox.resolve_path(&abs_path.display().to_string());
        assert!(result.is_ok());
    }

    // ── Destructive command detection tests ──

    #[test]
    fn test_destructive_detection() {
        let tmp = tempfile::tempdir().unwrap();
        let sandbox = make_sandbox(&tmp, PermissionMode::Strict);

        let destructive = vec![
            "rm -rf /",
            "rm -rf ~",
            "git push --force origin main",
            "git push origin main --force",
            "git reset --hard HEAD~1",
            "DROP TABLE users",
            "DELETE FROM users WHERE id = 1;",
            "mkfs /dev/sda1",
            "dd if=/dev/zero of=/dev/sda",
            "chmod 777 /etc",
            "chmod -R 777 /var",
        ];

        for cmd in &destructive {
            let check = sandbox.check_destructive(cmd);
            assert!(check.is_destructive, "Should detect as destructive: {cmd}");
        }

        let safe = vec![
            "ls -la",
            "cargo build",
            "git status",
            "echo hello",
            "rm file.txt",
            "rm -rf ./build",
            "chmod 755 script.sh",
            "git push origin main",
        ];

        for cmd in &safe {
            let check = sandbox.check_destructive(cmd);
            assert!(!check.is_destructive, "Should NOT flag as destructive: {cmd}");
        }
    }

    // ── .agentignore tests ──

    #[test]
    fn test_agentignore_basic() {
        let tmp = tempfile::tempdir().unwrap();

        // Create .agentignore
        std::fs::write(
            tmp.path().join(".agentignore"),
            "*.log\nnode_modules/\ntarget/\n",
        )
        .unwrap();

        // Create some files/dirs
        std::fs::write(tmp.path().join("app.log"), "log").unwrap();
        std::fs::create_dir(tmp.path().join("src")).unwrap();
        std::fs::write(tmp.path().join("src/main.rs"), "fn main() {}").unwrap();
        std::fs::create_dir(tmp.path().join("node_modules")).unwrap();
        std::fs::create_dir(tmp.path().join("target")).unwrap();

        let sandbox = FilesystemSandbox::new(tmp.path(), PermissionMode::Auto).unwrap();

        assert!(sandbox.is_ignored(&tmp.path().join("app.log")));
        assert!(!sandbox.is_ignored(&tmp.path().join("src/main.rs")));
        assert!(sandbox.is_ignored(&tmp.path().join("node_modules")));
        assert!(sandbox.is_ignored(&tmp.path().join("target")));
    }

    #[test]
    fn test_agentignore_comments_and_empty() {
        let tmp = tempfile::tempdir().unwrap();

        // .agentignore with comments and empty lines
        std::fs::write(
            tmp.path().join(".agentignore"),
            "# This is a comment\n\n*.tmp\n# Another comment\n*.bak\n",
        )
        .unwrap();

        std::fs::write(tmp.path().join("file.tmp"), "temp").unwrap();
        std::fs::write(tmp.path().join("file.bak"), "backup").unwrap();
        std::fs::write(tmp.path().join("file.rs"), "code").unwrap();

        let sandbox = FilesystemSandbox::new(tmp.path(), PermissionMode::Auto).unwrap();

        assert!(sandbox.is_ignored(&tmp.path().join("file.tmp")));
        assert!(sandbox.is_ignored(&tmp.path().join("file.bak")));
        assert!(!sandbox.is_ignored(&tmp.path().join("file.rs")));
    }

    #[test]
    fn test_no_agentignore() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("test.txt"), "hello").unwrap();

        let sandbox = FilesystemSandbox::new(tmp.path(), PermissionMode::Auto).unwrap();

        // No .agentignore → nothing is ignored
        assert!(!sandbox.is_ignored(&tmp.path().join("test.txt")));
        assert!(sandbox.ignore_matcher.is_none());
    }

    #[test]
    fn test_agentignore_negation() {
        let tmp = tempfile::tempdir().unwrap();

        // Negation patterns: ignore *.log but keep important.log
        std::fs::write(
            tmp.path().join(".agentignore"),
            "*.log\n!important.log\n",
        )
        .unwrap();

        std::fs::write(tmp.path().join("debug.log"), "log").unwrap();
        std::fs::write(tmp.path().join("important.log"), "important").unwrap();

        let sandbox = FilesystemSandbox::new(tmp.path(), PermissionMode::Auto).unwrap();

        assert!(sandbox.is_ignored(&tmp.path().join("debug.log")));
        assert!(!sandbox.is_ignored(&tmp.path().join("important.log")));
    }

    #[test]
    fn test_check_readable_writable_respects_ignore() {
        let tmp = tempfile::tempdir().unwrap();

        std::fs::write(
            tmp.path().join(".agentignore"),
            "*.secret\n",
        )
        .unwrap();

        std::fs::write(tmp.path().join("data.secret"), "secret").unwrap();
        std::fs::write(tmp.path().join("data.txt"), "public").unwrap();

        let sandbox = FilesystemSandbox::new(tmp.path(), PermissionMode::Auto).unwrap();

        // .secret file is ignored → both readable and writable checks should fail
        assert!(sandbox.check_readable(&tmp.path().join("data.secret")).is_err());
        assert!(sandbox.check_writable(&tmp.path().join("data.secret")).is_err());

        // Normal file should pass both checks
        assert!(sandbox.check_readable(&tmp.path().join("data.txt")).is_ok());
        assert!(sandbox.check_writable(&tmp.path().join("data.txt")).is_ok());
    }

    // ── Permission mode tests ──

    #[test]
    fn test_permission_mode_display_and_from_str() {
        assert_eq!(PermissionMode::Strict.to_string(), "strict");
        assert_eq!(PermissionMode::Auto.to_string(), "auto");
        assert_eq!(PermissionMode::Yolo.to_string(), "yolo");

        assert_eq!("strict".parse::<PermissionMode>().unwrap(), PermissionMode::Strict);
        assert_eq!("auto".parse::<PermissionMode>().unwrap(), PermissionMode::Auto);
        assert_eq!("yolo".parse::<PermissionMode>().unwrap(), PermissionMode::Yolo);
        assert_eq!("STRICT".parse::<PermissionMode>().unwrap(), PermissionMode::Strict);
        assert!("unknown".parse::<PermissionMode>().is_err());
    }

    #[test]
    fn test_requires_confirmation_strict() {
        let tmp = tempfile::tempdir().unwrap();
        let sandbox = make_sandbox(&tmp, PermissionMode::Strict);

        // Mutating tools require confirmation
        assert!(sandbox.requires_confirmation("file_write", &serde_json::json!({})));
        assert!(sandbox.requires_confirmation("file_edit", &serde_json::json!({})));
        assert!(sandbox.requires_confirmation("shell_exec", &serde_json::json!({"command": "ls"})));

        // Read-only tools don't
        assert!(!sandbox.requires_confirmation("file_read", &serde_json::json!({})));
        assert!(!sandbox.requires_confirmation("grep", &serde_json::json!({})));
    }

    #[test]
    fn test_requires_confirmation_auto() {
        let tmp = tempfile::tempdir().unwrap();
        let sandbox = make_sandbox(&tmp, PermissionMode::Auto);

        // Non-destructive commands don't require confirmation in auto mode
        assert!(!sandbox.requires_confirmation("shell_exec", &serde_json::json!({"command": "ls"})));
        assert!(!sandbox.requires_confirmation("shell_exec", &serde_json::json!({"command": "cargo build"})));

        // Destructive commands still require confirmation
        assert!(sandbox.requires_confirmation("shell_exec", &serde_json::json!({"command": "rm -rf /"})));
        assert!(sandbox.requires_confirmation("shell_exec", &serde_json::json!({"command": "git push --force origin main"})));

        // Other mutating tools are handled by ToolConfirmationPolicy PerTool
    }

    #[test]
    fn test_requires_confirmation_yolo() {
        let tmp = tempfile::tempdir().unwrap();
        let sandbox = make_sandbox(&tmp, PermissionMode::Yolo);

        // Nothing requires confirmation in yolo mode
        assert!(!sandbox.requires_confirmation("file_write", &serde_json::json!({})));
        assert!(!sandbox.requires_confirmation("shell_exec", &serde_json::json!({"command": "rm -rf /"})));
        assert!(!sandbox.requires_confirmation("file_read", &serde_json::json!({})));
    }

    #[test]
    fn test_to_tool_confirmation_policy() {
        let tmp = tempfile::tempdir().unwrap();

        let yolo = make_sandbox(&tmp, PermissionMode::Yolo);
        assert_eq!(yolo.to_tool_confirmation_policy(), ToolConfirmationPolicy::Never);

        let strict = make_sandbox(&tmp, PermissionMode::Strict);
        assert_eq!(strict.to_tool_confirmation_policy(), ToolConfirmationPolicy::Always);

        let auto = make_sandbox(&tmp, PermissionMode::Auto);
        let auto_policy = auto.to_tool_confirmation_policy();
        // PerTool should require confirmation for mutating tools
        for tool in MUTATING_TOOLS {
            assert!(auto_policy.requires_confirmation(tool), "Auto mode should confirm {tool}");
        }
        // But not for read-only tools
        for tool in READ_ONLY_TOOLS {
            assert!(!auto_policy.requires_confirmation(tool), "Auto mode should NOT confirm {tool}");
        }
    }

    #[test]
    fn test_empty_agentignore_file() {
        let tmp = tempfile::tempdir().unwrap();

        // Empty .agentignore file → no ignore rules
        std::fs::write(tmp.path().join(".agentignore"), "").unwrap();

        let sandbox = FilesystemSandbox::new(tmp.path(), PermissionMode::Auto).unwrap();

        // Nothing should be ignored with empty .agentignore
        assert!(sandbox.ignore_matcher.is_none());
    }

    #[test]
    fn test_agentignore_wildcard_patterns() {
        let tmp = tempfile::tempdir().unwrap();

        std::fs::write(
            tmp.path().join(".agentignore"),
            "*.env\nsecrets/**\n.DS_Store\n",
        )
        .unwrap();

        // Create actual files for testing (is_ignored needs to canonicalize them)
        std::fs::write(tmp.path().join("production.env"), "key=val").unwrap();
        std::fs::create_dir_all(tmp.path().join("secrets")).unwrap();
        std::fs::write(tmp.path().join("secrets/db.pem"), "pem").unwrap();
        std::fs::write(tmp.path().join(".DS_Store"), "ds").unwrap();
        std::fs::create_dir_all(tmp.path().join("src")).unwrap();
        std::fs::write(tmp.path().join("src/main.rs"), "fn main() {}").unwrap();

        let sandbox = FilesystemSandbox::new(tmp.path(), PermissionMode::Auto).unwrap();

        assert!(sandbox.is_ignored(&tmp.path().join("production.env")));
        assert!(sandbox.is_ignored(&tmp.path().join("secrets/db.pem")));
        assert!(sandbox.is_ignored(&tmp.path().join(".DS_Store")));
        assert!(!sandbox.is_ignored(&tmp.path().join("src/main.rs")));
    }
}
