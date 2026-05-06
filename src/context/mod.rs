use std::path::{Path, PathBuf};

use crate::memory::vault::ObsidianVault;

/// Builds the system prompt from AGENTS.md, CLAUDE.md, KMS, and memory context.
pub struct ContextBuilder {
    project_path: PathBuf,
    agents_md_content: Vec<(PathBuf, String)>,
    kms_toc: Option<String>,
}

impl ContextBuilder {
    /// Create a new context builder by walking up from project_path
    /// looking for AGENTS.md, CLAUDE.md, and .harness/AGENTS.md files.
    pub fn new(project_path: &Path, _vault: &ObsidianVault) -> anyhow::Result<Self> {
        let mut agents_md_content = Vec::new();
        let mut current = project_path.to_path_buf();

        // Walk up from cwd looking for context files
        loop {
            for filename in &["AGENTS.md", "CLAUDE.md"] {
                let candidate = current.join(filename);
                if candidate.exists() {
                    let content = std::fs::read_to_string(&candidate)?;
                    agents_md_content.push((candidate, content));
                }
            }

            // Check .harness/AGENTS.md
            let harness_dir = current.join(".harness");
            if harness_dir.exists() {
                let candidate = harness_dir.join("AGENTS.md");
                if candidate.exists() {
                    let content = std::fs::read_to_string(&candidate)?;
                    agents_md_content.push((candidate, content));
                }
            }

            if !current.pop() {
                break; // Reached filesystem root
            }
        }

        // Reverse so closer-to-cwd files come last (higher priority)
        agents_md_content.reverse();

        // Load KMS TOC
        let kms_toc = Self::load_kms_toc(project_path)?;

        Ok(Self {
            project_path: project_path.to_path_buf(),
            agents_md_content,
            kms_toc,
        })
    }

    /// Build the system prompt by concatenating all context sources.
    pub fn system_prompt(&self) -> String {
        let mut parts = Vec::new();

        parts.push(
            "You are Agent Harness, an AI coding assistant. \
             You have access to tools for file operations, \
             shell execution, web search, and memory management. \
             Always prefer using dedicated tools over Bash commands. \
             Be concise. Do not add unnecessary comments or documentation \
             to code you didn't change."
                .into(),
        );

        // AGENTS.md / CLAUDE.md (closest = highest priority)
        for (path, content) in &self.agents_md_content {
            parts.push(format!(
                "\n--- Context from {} ---\n{}",
                path.display(),
                content
            ));
        }

        // KMS TOC
        if let Some(toc) = &self.kms_toc {
            parts.push(format!("\n--- Project Knowledge Base ---\n{}", toc));
        }

        parts.join("\n\n")
    }

    /// Get the loaded context files (for logging).
    pub fn loaded_files(&self) -> &[(PathBuf, String)] {
        &self.agents_md_content
    }

    fn load_kms_toc(project_path: &Path) -> anyhow::Result<Option<String>> {
        let kms_dir = project_path.join(".kms");
        if !kms_dir.exists() {
            return Ok(None);
        }

        let mut tocs = Vec::new();
        for entry in std::fs::read_dir(&kms_dir)? {
            let entry = entry?;
            let index = entry.path().join("index.md");
            if index.exists() {
                let content = std::fs::read_to_string(&index)?;
                tocs.push(format!("### {}\n{}", entry.file_name().display(), content));
            }
        }

        if tocs.is_empty() {
            Ok(None)
        } else {
            Ok(Some(tocs.join("\n\n")))
        }
    }
}
