use std::path::{Path, PathBuf};

use crate::agent::AgentDef;
use crate::memory::vault::ObsidianVault;

/// Builds the system prompt from SOUL.md, AGENTS.md, CLAUDE.md, KMS, and memory context.
pub struct ContextBuilder {
    project_path: PathBuf,
    soul_md_content: Vec<(PathBuf, String)>,
    agents_md_content: Vec<(PathBuf, String)>,
    kms_toc: Option<String>,
    /// How to hand work to a team or a routine, and what this project has of
    /// each. See [`ContextBuilder::delegation_brief`].
    delegation: String,
}

impl ContextBuilder {
    /// Create a new context builder by walking up from project_path
    /// looking for SOUL.md, AGENTS.md, CLAUDE.md, and .harness/AGENTS.md files.
    pub fn new(project_path: &Path, _vault: &ObsidianVault) -> anyhow::Result<Self> {
        let mut soul_md_content = Vec::new();
        let mut agents_md_content = Vec::new();
        let mut current = project_path.to_path_buf();

        // Walk up from cwd looking for context files
        loop {
            // SOUL.md (agent personality/identity)
            let soul_candidate = current.join("SOUL.md");
            if soul_candidate.exists() {
                let content = std::fs::read_to_string(&soul_candidate)?;
                soul_md_content.push((soul_candidate, content));
            }

            // AGENTS.md / CLAUDE.md (project instructions)
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
                // Also check .harness/SOUL.md
                let soul_candidate = harness_dir.join("SOUL.md");
                if soul_candidate.exists() {
                    let content = std::fs::read_to_string(&soul_candidate)?;
                    soul_md_content.push((soul_candidate, content));
                }
            }

            if !current.pop() {
                break; // Reached filesystem root
            }
        }

        // Reverse so closer-to-cwd files come last (higher priority)
        soul_md_content.reverse();
        agents_md_content.reverse();

        // Load KMS TOC
        let kms_toc = Self::load_kms_toc(project_path)?;

        Ok(Self {
            project_path: project_path.to_path_buf(),
            soul_md_content,
            agents_md_content,
            kms_toc,
            delegation: Self::delegation_brief(project_path),
        })
    }

    /// Build the system prompt by concatenating all context sources.
    pub fn system_prompt(&self) -> String {
        let mut parts = Vec::new();

        parts.push(
            "You are MOMO Fetch, an AI coding assistant. \
             You have access to tools for file operations, \
             shell execution, web search, and memory management. \
             Always prefer using dedicated tools over Bash commands. \
             Be concise. Do not add unnecessary comments or documentation \
             to code you didn't change."
                .into(),
        );

        parts.push(self.delegation.clone());

        // SOUL.md (agent personality/identity — highest behavioral priority)
        for (path, content) in &self.soul_md_content {
            parts.push(format!(
                "\n--- Soul from {} ---\n{}",
                path.display(),
                content
            ));
        }

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


    /// What this project can hand work to, and how.
    ///
    /// **Why this is in the system prompt at all.** Teams and routines are
    /// driven by `momo-fetch team …` / `momo-fetch routine …`, which print JSON
    /// on stdout precisely so an agent can drive them through `shell_exec` —
    /// but nothing ever told the agent they existed. Asked to "start the
    /// squad", it reached for `task(...)`, the only delegation it knew about,
    /// and reported the squad as started. Both front-ends had the commands; the
    /// model was the one left out.
    ///
    /// The names are read once, here, so the agent knows what exists without
    /// spending a tool call to find out. They go stale the moment someone adds
    /// a config, which is why the text says `list` is the live answer.
    fn delegation_brief(project_path: &Path) -> String {
        let harness = project_path.join(".harness");
        let teams = Self::config_names(&harness.join("teams"), &["json", "yml", "yaml"]);
        let routines = Self::routine_names(&harness.join("routines"));

        let mut brief = String::from(
            "\n--- Work outside this turn ---\n\
             This project can run work that outlives your turn. Drive it with `shell_exec`; \
             each command prints one JSON document on stdout.\n\
             \n\
             \x20 momo-fetch team list                 configs, and which one is active\n\
             \x20 momo-fetch team start <name>         start one — a tmux pane per worker\n\
             \x20 momo-fetch team status               workers, their state, mailbox backlog\n\
             \x20 momo-fetch team send <worker> <text> message a standby worker\n\
             \x20 momo-fetch team stop                 DESTRUCTIVE: removes worktrees, deletes branches\n\
             \x20 momo-fetch routine list|show|add|run|enable|disable\n",
        );

        brief.push_str(&format!(
            "\nTeams defined here: {}\nRoutines defined here: {}\n\
             (Read when this session started — `list` is the live answer.)\n",
            Self::name_list(&teams, "none — configs live in .harness/teams/"),
            Self::name_list(&routines, "none"),
        ));

        brief.push_str(
            "\nA team is not the `task` tool. `task(...)` runs sub-agents inside this turn \
             and is over when the turn is; a team is separate processes that keep working \
             after it. If you are asked to start a squad, a team or a worker, run the command \
             — do not substitute `task`, and do not report a team as started unless one of \
             these commands said so.\n",
        );

        brief
    }

    /// At most twelve names — enough to recognise what exists, not enough for a
    /// project with a hundred configs to crowd out the rest of the prompt.
    fn name_list(names: &[String], empty: &str) -> String {
        if names.is_empty() {
            return empty.to_string();
        }
        let shown: Vec<&str> = names.iter().take(12).map(String::as_str).collect();
        let mut out = shown.join(", ");
        if names.len() > shown.len() {
            out.push_str(&format!(", … ({} more)", names.len() - shown.len()));
        }
        out
    }

    /// File stems in `dir` with one of `extensions` — the name `team start`
    /// takes.
    fn config_names(dir: &Path, extensions: &[&str]) -> Vec<String> {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return Vec::new();
        };
        let mut names: Vec<String> = entries
            .flatten()
            .filter_map(|e| {
                let path = e.path();
                let ext = path.extension()?.to_str()?;
                extensions.contains(&ext).then(|| {
                    path.file_stem()
                        .and_then(|s| s.to_str())
                        .map(str::to_string)
                })?
            })
            .collect();
        names.sort();
        names.dedup();
        names
    }

    /// A routine is addressed by its `name`, not by the id its file is called,
    /// so the file stem would be the wrong thing to show.
    fn routine_names(dir: &Path) -> Vec<String> {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return Vec::new();
        };
        let mut names: Vec<String> = entries
            .flatten()
            .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
            .filter_map(|e| {
                let raw = std::fs::read_to_string(e.path()).ok()?;
                let value: serde_json::Value = serde_json::from_str(&raw).ok()?;
                value.get("name")?.as_str().map(str::to_string)
            })
            .collect();
        names.sort();
        names
    }

    /// Get the loaded context files (for logging).
    #[allow(dead_code)]
    pub fn loaded_files(&self) -> &[(PathBuf, String)] {
        &self.agents_md_content
    }

    /// Get the project path.
    pub fn project_path(&self) -> &Path {
        &self.project_path
    }

    /// Build the system prompt for a specific agent personality.
    ///
    /// Same as `system_prompt()` but:
    /// - Changes the base identity to reference the agent name
    /// - Appends the agent personality after SOUL.md
    pub fn system_prompt_for_agent(&self, agent_def: &AgentDef) -> String {
        let mut parts = Vec::new();

        // Agent-specific base identity
        let desc = agent_def
            .description
            .as_deref()
            .unwrap_or("specialist agent");
        parts.push(format!(
            "You are MOMO Fetch operating as **{}** — {}. \
             You have access to tools for file operations, \
             shell execution, web search, and memory management. \
             Always prefer using dedicated tools over Bash commands. \
             Be concise. Do not add unnecessary comments or documentation \
             to code you didn't change.",
            agent_def.name, desc
        ));

        parts.push(self.delegation.clone());

        // SOUL.md (agent personality/identity)
        for (path, content) in &self.soul_md_content {
            parts.push(format!(
                "\n--- Soul from {} ---\n{}",
                path.display(),
                content
            ));
        }

        // Agent personality override (between SOUL.md and AGENTS.md)
        if !agent_def.personality.is_empty() {
            parts.push(format!(
                "\n--- Agent Personality: {} ---\n{}",
                agent_def.name, agent_def.personality
            ));
        }

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

    /// Get relative paths of loaded context files for display.
    pub fn loaded_file_relative_paths(&self) -> Vec<String> {
        let cwd = std::env::current_dir().unwrap_or_default();
        let mut paths = Vec::new();

        for (p, _) in self.soul_md_content.iter().chain(self.agents_md_content.iter()) {
            let rel = p
                .strip_prefix(&cwd)
                .map(|rel| format!("./{}", rel.display()))
                .unwrap_or_else(|_| format!("{}", p.display()));
            paths.push(rel);
        }

        paths
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn the_delegation_brief_names_what_the_project_has() {
        let tmp = tempfile::tempdir().unwrap();
        let harness = tmp.path().join(".harness");
        fs::create_dir_all(harness.join("teams")).unwrap();
        fs::create_dir_all(harness.join("routines")).unwrap();
        fs::write(harness.join("teams/yolo-trade-squad.json"), "{}").unwrap();
        fs::write(harness.join("teams/notes.txt"), "not a config").unwrap();
        // Addressed by `name`, filed under its id — the file stem is the wrong
        // thing to put in front of the agent.
        fs::write(
            harness.join("routines/rt-abc123.json"),
            r#"{"id":"rt-abc123","name":"Nightly digest"}"#,
        )
        .unwrap();

        let brief = ContextBuilder::delegation_brief(tmp.path());
        assert!(brief.contains("yolo-trade-squad"), "{brief}");
        assert!(brief.contains("Nightly digest"), "{brief}");
        assert!(!brief.contains("notes"), "{brief}");
        assert!(!brief.contains("rt-abc123"), "{brief}");
        assert!(brief.contains("momo-fetch team start"), "{brief}");
        // The substitution that started all this.
        assert!(brief.contains("do not substitute `task`"), "{brief}");
    }

    #[test]
    fn a_project_with_no_teams_says_so_rather_than_listing_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        let brief = ContextBuilder::delegation_brief(tmp.path());
        assert!(brief.contains(".harness/teams/"), "{brief}");
        assert!(brief.contains("Routines defined here: none"), "{brief}");
    }

    #[test]
    fn test_discovers_agents_md_in_project_root() {
        let tmp = tempfile::tempdir().unwrap();
        let project = tmp.path().join("project");
        fs::create_dir_all(&project).unwrap();
        fs::write(project.join("AGENTS.md"), "# Project instructions\nUse Rust 2024 edition").unwrap();

        let vault_dir = tmp.path().join("vault");
        let vault = ObsidianVault::open(&vault_dir).unwrap();

        let ctx = ContextBuilder::new(&project, &vault).unwrap();
        assert_eq!(ctx.agents_md_content.len(), 1);
        assert!(ctx.agents_md_content[0].1.contains("Use Rust 2024 edition"));
    }

    #[test]
    fn test_discovers_claude_md() {
        let tmp = tempfile::tempdir().unwrap();
        let project = tmp.path().join("project");
        fs::create_dir_all(&project).unwrap();
        fs::write(project.join("CLAUDE.md"), "# Claude instructions").unwrap();

        let vault_dir = tmp.path().join("vault");
        let vault = ObsidianVault::open(&vault_dir).unwrap();

        let ctx = ContextBuilder::new(&project, &vault).unwrap();
        assert_eq!(ctx.agents_md_content.len(), 1);
        assert!(ctx.agents_md_content[0].1.contains("Claude instructions"));
    }

    #[test]
    fn test_discovers_harness_agents_md() {
        let tmp = tempfile::tempdir().unwrap();
        let project = tmp.path().join("project");
        let harness_dir = project.join(".harness");
        fs::create_dir_all(&harness_dir).unwrap();
        fs::write(harness_dir.join("AGENTS.md"), "# Harness-specific rules").unwrap();

        let vault_dir = tmp.path().join("vault");
        let vault = ObsidianVault::open(&vault_dir).unwrap();

        let ctx = ContextBuilder::new(&project, &vault).unwrap();
        assert_eq!(ctx.agents_md_content.len(), 1);
        assert!(ctx.agents_md_content[0].1.contains("Harness-specific rules"));
    }

    #[test]
    fn test_walks_up_directory_tree() {
        let tmp = tempfile::tempdir().unwrap();
        // Root has AGENTS.md
        fs::write(tmp.path().join("AGENTS.md"), "# Root instructions").unwrap();

        // Project subdir has CLAUDE.md
        let project = tmp.path().join("subdir").join("project");
        fs::create_dir_all(&project).unwrap();
        fs::write(project.join("CLAUDE.md"), "# Project instructions").unwrap();

        let vault_dir = tmp.path().join("vault");
        let vault = ObsidianVault::open(&vault_dir).unwrap();

        let ctx = ContextBuilder::new(&project, &vault).unwrap();
        // Should find both: root AGENTS.md and project CLAUDE.md
        assert!(ctx.agents_md_content.len() >= 2);

        // Closer files (project) should be last (higher priority)
        let last = ctx.agents_md_content.last().unwrap();
        assert!(last.1.contains("Project instructions"));
    }

    #[test]
    fn test_closer_files_override_further() {
        let tmp = tempfile::tempdir().unwrap();
        // Root has AGENTS.md with root content
        fs::write(tmp.path().join("AGENTS.md"), "# Root level").unwrap();

        // Project also has AGENTS.md with project content
        let project = tmp.path().join("deep").join("project");
        fs::create_dir_all(&project).unwrap();
        fs::write(project.join("AGENTS.md"), "# Project level").unwrap();

        let vault_dir = tmp.path().join("vault");
        let vault = ObsidianVault::open(&vault_dir).unwrap();

        let ctx = ContextBuilder::new(&project, &vault).unwrap();
        let prompt = ctx.system_prompt();

        // Both should appear, with project level content coming last
        assert!(prompt.contains("Root level"));
        assert!(prompt.contains("Project level"));

        // Project content appears after root content (higher priority)
        let root_pos = prompt.find("Root level").unwrap();
        let project_pos = prompt.find("Project level").unwrap();
        assert!(project_pos > root_pos, "Project (closer) should come after root (further)");
    }

    #[test]
    fn test_no_files_no_error() {
        let tmp = tempfile::tempdir().unwrap();
        let project = tmp.path().join("empty-project");
        fs::create_dir_all(&project).unwrap();

        let vault_dir = tmp.path().join("vault");
        let vault = ObsidianVault::open(&vault_dir).unwrap();

        let ctx = ContextBuilder::new(&project, &vault).unwrap();
        assert!(ctx.agents_md_content.is_empty());
        assert!(ctx.soul_md_content.is_empty());

        // System prompt should still work
        let prompt = ctx.system_prompt();
        assert!(prompt.contains("You are MOMO Fetch"));
    }

    #[test]
    fn test_system_prompt_contains_base_instruction() {
        let tmp = tempfile::tempdir().unwrap();
        let project = tmp.path().join("project");
        fs::create_dir_all(&project).unwrap();

        let vault_dir = tmp.path().join("vault");
        let vault = ObsidianVault::open(&vault_dir).unwrap();

        let ctx = ContextBuilder::new(&project, &vault).unwrap();
        let prompt = ctx.system_prompt();
        assert!(prompt.contains("You are MOMO Fetch"));
        assert!(prompt.contains("AI coding assistant"));
    }

    #[test]
    fn test_system_prompt_includes_context_files() {
        let tmp = tempfile::tempdir().unwrap();
        let project = tmp.path().join("project");
        fs::create_dir_all(&project).unwrap();
        fs::write(project.join("AGENTS.md"), "Always use tabs for indentation").unwrap();

        let vault_dir = tmp.path().join("vault");
        let vault = ObsidianVault::open(&vault_dir).unwrap();

        let ctx = ContextBuilder::new(&project, &vault).unwrap();
        let prompt = ctx.system_prompt();
        assert!(prompt.contains("Always use tabs for indentation"));
        assert!(prompt.contains("--- Context from"));
    }

    #[test]
    fn test_kms_toc_included_in_prompt() {
        let tmp = tempfile::tempdir().unwrap();
        let project = tmp.path().join("project");
        let kms_dir = project.join(".kms").join("conventions");
        fs::create_dir_all(kms_dir.join("pages")).unwrap();
        fs::write(kms_dir.join("index.md"), "# Conventions\n- Use Rust 2024").unwrap();

        let vault_dir = tmp.path().join("vault");
        let vault = ObsidianVault::open(&vault_dir).unwrap();

        let ctx = ContextBuilder::new(&project, &vault).unwrap();
        let prompt = ctx.system_prompt();
        assert!(prompt.contains("Project Knowledge Base"));
        assert!(prompt.contains("Use Rust 2024"));
    }

    #[test]
    fn test_discovers_soul_md() {
        let tmp = tempfile::tempdir().unwrap();
        let project = tmp.path().join("project");
        fs::create_dir_all(&project).unwrap();
        fs::write(project.join("SOUL.md"), "# My Soul\nBe friendly and concise").unwrap();

        let vault_dir = tmp.path().join("vault");
        let vault = ObsidianVault::open(&vault_dir).unwrap();

        let ctx = ContextBuilder::new(&project, &vault).unwrap();
        assert_eq!(ctx.soul_md_content.len(), 1);
        assert!(ctx.soul_md_content[0].1.contains("Be friendly and concise"));
    }

    #[test]
    fn test_soul_md_in_system_prompt_with_own_section() {
        let tmp = tempfile::tempdir().unwrap();
        let project = tmp.path().join("project");
        fs::create_dir_all(&project).unwrap();
        fs::write(project.join("SOUL.md"), "Speak in Thai by default").unwrap();
        fs::write(project.join("AGENTS.md"), "Use tabs for indentation").unwrap();

        let vault_dir = tmp.path().join("vault");
        let vault = ObsidianVault::open(&vault_dir).unwrap();

        let ctx = ContextBuilder::new(&project, &vault).unwrap();
        let prompt = ctx.system_prompt();

        // SOUL.md gets its own section header
        assert!(prompt.contains("--- Soul from"));
        assert!(prompt.contains("Speak in Thai by default"));

        // AGENTS.md gets context section header
        assert!(prompt.contains("--- Context from"));
        assert!(prompt.contains("Use tabs for indentation"));

        // SOUL.md should appear before AGENTS.md in the prompt
        let soul_pos = prompt.find("Speak in Thai").unwrap();
        let agents_pos = prompt.find("Use tabs").unwrap();
        assert!(soul_pos < agents_pos, "SOUL.md should come before AGENTS.md");
    }

    #[test]
    fn test_soul_md_in_loaded_file_paths() {
        let tmp = tempfile::tempdir().unwrap();
        let project = tmp.path().join("project");
        fs::create_dir_all(&project).unwrap();
        fs::write(project.join("SOUL.md"), "Be kind").unwrap();

        let vault_dir = tmp.path().join("vault");
        let vault = ObsidianVault::open(&vault_dir).unwrap();

        let ctx = ContextBuilder::new(&project, &vault).unwrap();
        let paths = ctx.loaded_file_relative_paths();
        assert!(paths.iter().any(|p| p.contains("SOUL.md")));
    }
}
