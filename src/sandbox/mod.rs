use std::path::{Path, PathBuf};
use serde::{Deserialize, Serialize};

/// Permission mode for tool execution.
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

/// Result of destructive command detection.
#[derive(Debug, Clone)]
pub struct DestructiveCheck {
    pub is_destructive: bool,
    pub pattern: Option<String>,
    pub category: Option<String>,
}

/// Filesystem sandbox that scopes all operations to a project directory.
pub struct FilesystemSandbox {
    root: PathBuf,
    permission_mode: PermissionMode,
}

impl FilesystemSandbox {
    /// Create a new sandbox rooted at the given path.
    pub fn new(root: &Path, mode: PermissionMode) -> anyhow::Result<Self> {
        let root = root.canonicalize()?;
        Ok(Self {
            root,
            permission_mode: mode,
        })
    }

    /// Resolve a path and ensure it's within the sandbox root.
    pub fn resolve_path(&self, relative: &str) -> anyhow::Result<PathBuf> {
        let cleaned = path_clean::PathClean::clean(&PathBuf::from(relative));

        // Early check: reject paths that escape upward via ..
        // After cleaning, if the path starts with ../ it will escape the root.
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
                // (the early check above already rejected upward traversal)
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

    /// Detect destructive shell commands.
    pub fn check_destructive(&self, command: &str) -> DestructiveCheck {
        let patterns: &[(&str, &str)] = &[
            (r"(?i)\brm\s+(-[rfRF]+\s+)?/\s*$", "Destructive deletion"),
            (r"(?i)\brm\s+(-[rfRF]+\s+)?~\s*$", "Destructive deletion"),
            (r"(?i)git\s+push\s+.*--force", "Destructive git"),
            (r"(?i)git\s+reset\s+--hard", "Destructive git"),
            (r"(?i)DROP\s+TABLE", "Destructive SQL"),
            (r"(?i)DELETE\s+FROM\s+\w+\s*;", "Destructive SQL"),
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

    /// Get the sandbox root path.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Get the current permission mode.
    pub fn permission_mode(&self) -> PermissionMode {
        self.permission_mode
    }
}
