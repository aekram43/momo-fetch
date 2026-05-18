//! Agent personalities — specialist agent definitions loaded from .harness/agents/
//!
//! Supports two formats:
//! - `.md` files: personality prompt only (uses default model/tools)
//! - `.yml` files: personality + model/provider/tools/capabilities config

pub mod orchestrator;

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Definition of a specialist agent, loaded from `.harness/agents/<name>.md` or `.yml`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentDef {
    /// Unique agent name (derived from filename).
    pub name: String,
    /// Human-readable description of what this agent does.
    pub description: Option<String>,
    /// The personality prompt content (loaded from the .md file).
    pub personality: String,
    /// Optional model override for this agent.
    pub model: Option<String>,
    /// Optional provider override for this agent.
    pub provider: Option<String>,
    /// Optional restricted tool set. None = all tools available.
    pub tools: Option<Vec<String>>,
    /// Capabilities tags (for orchestrator reference).
    pub capabilities: Vec<String>,
}

/// YAML config file schema for agent definitions.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AgentConfigFile {
    /// Human-readable description.
    pub description: Option<String>,
    /// Path to personality .md file (relative to .harness/agents/). Defaults to <name>.md.
    pub personality_file: Option<String>,
    /// Model override.
    pub model: Option<String>,
    /// Provider override.
    pub provider: Option<String>,
    /// Restricted tool set.
    pub tools: Option<Vec<String>>,
    /// Capability tags.
    pub capabilities: Option<Vec<String>>,
}

/// Registry of all agent personalities discovered in `.harness/agents/`.
#[derive(Debug, Clone)]
pub struct AgentRegistry {
    /// Path to the agents directory.
    #[allow(dead_code)]
    agents_dir: PathBuf,
    /// Loaded agent definitions keyed by name.
    agents: HashMap<String, AgentDef>,
}

impl AgentRegistry {
    /// Scan `.harness/agents/` for agent definitions.
    ///
    /// Returns an empty registry if the directory doesn't exist.
    pub fn new(project_path: &Path) -> anyhow::Result<Self> {
        let agents_dir = project_path.join(".harness").join("agents");

        if !agents_dir.exists() {
            return Ok(Self {
                agents_dir,
                agents: HashMap::new(),
            });
        }

        let mut agents = HashMap::new();

        // Phase 1: Load all .md files (base personalities)
        // Phase 2: Load all .yml/.yaml files (override/extend .md entries)
        // This ensures YAML always takes priority regardless of read_dir order.
        let entries: Vec<_> = std::fs::read_dir(&agents_dir)?.collect::<Result<_, _>>()?;

        // Phase 1: .md files
        for entry in &entries {
            let path = entry.path();
            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
            if ext == "md" {
                if let Some(name) = Self::stem_name(&path) {
                    let personality = std::fs::read_to_string(&path)?;
                    agents.insert(
                        name.clone(),
                        AgentDef {
                            name,
                            description: None,
                            personality,
                            model: None,
                            provider: None,
                            tools: None,
                            capabilities: Vec::new(),
                        },
                    );
                }
            }
        }

        // Phase 2: .yml/.yaml files (override .md entries)
        for entry in &entries {
            let path = entry.path();
            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
            if ext == "yml" || ext == "yaml" {
                if let Some(name) = Self::stem_name(&path) {
                    let content = std::fs::read_to_string(&path)?;
                    let config: AgentConfigFile =
                        serde_yaml::from_str(&content).unwrap_or_default();

                    // Load personality from referenced .md file or <name>.md
                    let personality_filename = config
                        .personality_file
                        .clone()
                        .unwrap_or_else(|| format!("{name}.md"));
                    let personality_path = agents_dir.join(&personality_filename);
                    let personality = if personality_path.exists() {
                        std::fs::read_to_string(&personality_path)?
                    } else {
                        // Use empty personality if .md file not found
                        String::new()
                    };

                    // YAML config takes priority: overwrite any .md-only entry
                    agents.insert(
                        name.clone(),
                        AgentDef {
                            name,
                            description: config.description,
                            personality,
                            model: config.model,
                            provider: config.provider,
                            tools: config.tools,
                            capabilities: config.capabilities.unwrap_or_default(),
                        },
                    );
                }
            }
        }

        Ok(Self {
            agents_dir,
            agents,
        })
    }

    /// Get an agent definition by name.
    pub fn get(&self, name: &str) -> Option<&AgentDef> {
        self.agents.get(name)
    }

    /// List all registered agent definitions.
    pub fn list(&self) -> Vec<&AgentDef> {
        let mut defs: Vec<_> = self.agents.values().collect();
        defs.sort_by(|a, b| a.name.cmp(&b.name));
        defs
    }

    /// Check if the registry has any agents.
    pub fn is_empty(&self) -> bool {
        self.agents.is_empty()
    }

    /// Number of registered agents.
    pub fn len(&self) -> usize {
        self.agents.len()
    }

    /// Check if an agent has orchestration capability.
    #[allow(dead_code)]
    pub fn is_orchestrator(&self, name: &str) -> bool {
        self.agents
            .get(name)
            .map(|def| def.capabilities.contains(&"orchestration".to_string()))
            .unwrap_or(false)
    }

    /// Get the agents directory path.
    #[allow(dead_code)]
    pub fn agents_dir(&self) -> &Path {
        &self.agents_dir
    }

    /// Extract stem name from a file path (e.g., "researcher.md" -> "researcher").
    fn stem_name(path: &Path) -> Option<String> {
        path.file_stem()
            .and_then(|s| s.to_str())
            .map(|s| s.to_string())
            .filter(|s| !s.is_empty())
    }

    /// Create an empty registry (for fallback when loading fails).
    pub fn empty(agents_dir: PathBuf) -> Self {
        Self {
            agents_dir,
            agents: HashMap::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_registry_when_no_agents_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let registry = AgentRegistry::new(tmp.path()).unwrap();
        assert!(registry.is_empty());
        assert_eq!(registry.len(), 0);
    }

    #[test]
    fn test_loads_md_agent() {
        let tmp = tempfile::tempdir().unwrap();
        let agents_dir = tmp.path().join(".harness").join("agents");
        std::fs::create_dir_all(&agents_dir).unwrap();
        std::fs::write(
            agents_dir.join("researcher.md"),
            "# Research Specialist\nYou are a research expert.",
        )
        .unwrap();

        let registry = AgentRegistry::new(tmp.path()).unwrap();
        assert_eq!(registry.len(), 1);

        let agent = registry.get("researcher").unwrap();
        assert_eq!(agent.name, "researcher");
        assert!(agent.personality.contains("research expert"));
        assert!(agent.model.is_none());
        assert!(agent.tools.is_none());
    }

    #[test]
    fn test_loads_yml_agent_with_personality() {
        let tmp = tempfile::tempdir().unwrap();
        let agents_dir = tmp.path().join(".harness").join("agents");
        std::fs::create_dir_all(&agents_dir).unwrap();

        std::fs::write(
            agents_dir.join("coder.yml"),
            r#"description: "Coding specialist"
model: deepseek-chat
provider: deepseek
tools: [file_read, file_write, bash]
capabilities: [coding, debugging]"#,
        )
        .unwrap();

        std::fs::write(
            agents_dir.join("coder.md"),
            "You are an expert coder. Write clean, efficient code.",
        )
        .unwrap();

        let registry = AgentRegistry::new(tmp.path()).unwrap();
        assert_eq!(registry.len(), 1);

        let agent = registry.get("coder").unwrap();
        assert_eq!(agent.name, "coder");
        assert_eq!(agent.description.as_deref(), Some("Coding specialist"));
        assert_eq!(agent.model.as_deref(), Some("deepseek-chat"));
        assert_eq!(agent.provider.as_deref(), Some("deepseek"));
        assert_eq!(agent.tools.as_ref().unwrap().len(), 3);
        assert!(agent.personality.contains("expert coder"));
    }

    #[test]
    fn test_yml_overrides_md_only_entry() {
        let tmp = tempfile::tempdir().unwrap();
        let agents_dir = tmp.path().join(".harness").join("agents");
        std::fs::create_dir_all(&agents_dir).unwrap();

        // Both .md and .yml exist for same name — yml takes priority
        std::fs::write(agents_dir.join("reviewer.md"), "Simple personality").unwrap();
        std::fs::write(
            agents_dir.join("reviewer.yml"),
            "description: \"Review expert\"\ncapabilities: [review]",
        )
        .unwrap();

        let registry = AgentRegistry::new(tmp.path()).unwrap();
        let agent = registry.get("reviewer").unwrap();
        assert_eq!(agent.description.as_deref(), Some("Review expert"));
        assert!(agent.capabilities.contains(&"review".to_string()));
    }

    #[test]
    fn test_yml_with_custom_personality_file() {
        let tmp = tempfile::tempdir().unwrap();
        let agents_dir = tmp.path().join(".harness").join("agents");
        std::fs::create_dir_all(&agents_dir).unwrap();

        std::fs::write(
            agents_dir.join("custom.yml"),
            "description: \"Custom agent\"\npersonality_file: shared-prompt.md",
        )
        .unwrap();
        std::fs::write(
            agents_dir.join("shared-prompt.md"),
            "Shared personality content",
        )
        .unwrap();

        let registry = AgentRegistry::new(tmp.path()).unwrap();
        let agent = registry.get("custom").unwrap();
        assert!(agent.personality.contains("Shared personality content"));
    }

    #[test]
    fn test_yml_without_md_gets_empty_personality() {
        let tmp = tempfile::tempdir().unwrap();
        let agents_dir = tmp.path().join(".harness").join("agents");
        std::fs::create_dir_all(&agents_dir).unwrap();

        std::fs::write(
            agents_dir.join("minimal.yml"),
            "description: \"No personality file\"",
        )
        .unwrap();

        let registry = AgentRegistry::new(tmp.path()).unwrap();
        let agent = registry.get("minimal").unwrap();
        assert_eq!(agent.personality, "");
    }

    #[test]
    fn test_multiple_agents() {
        let tmp = tempfile::tempdir().unwrap();
        let agents_dir = tmp.path().join(".harness").join("agents");
        std::fs::create_dir_all(&agents_dir).unwrap();

        std::fs::write(agents_dir.join("alpha.md"), "Alpha personality").unwrap();
        std::fs::write(agents_dir.join("beta.md"), "Beta personality").unwrap();
        std::fs::write(agents_dir.join("gamma.md"), "Gamma personality").unwrap();

        let registry = AgentRegistry::new(tmp.path()).unwrap();
        assert_eq!(registry.len(), 3);

        let list = registry.list();
        assert_eq!(list.len(), 3);
        // Should be sorted by name
        assert_eq!(list[0].name, "alpha");
        assert_eq!(list[1].name, "beta");
        assert_eq!(list[2].name, "gamma");
    }

    #[test]
    fn test_is_orchestrator() {
        let tmp = tempfile::tempdir().unwrap();
        let agents_dir = tmp.path().join(".harness").join("agents");
        std::fs::create_dir_all(&agents_dir).unwrap();

        std::fs::write(
            agents_dir.join("orchestrator.yml"),
            "capabilities: [orchestration, coordination]",
        )
        .unwrap();
        std::fs::write(agents_dir.join("worker.md"), "Worker personality").unwrap();

        let registry = AgentRegistry::new(tmp.path()).unwrap();
        assert!(registry.is_orchestrator("orchestrator"));
        assert!(!registry.is_orchestrator("worker"));
        assert!(!registry.is_orchestrator("nonexistent"));
    }

    #[test]
    fn test_ignores_non_agent_files() {
        let tmp = tempfile::tempdir().unwrap();
        let agents_dir = tmp.path().join(".harness").join("agents");
        std::fs::create_dir_all(&agents_dir).unwrap();

        std::fs::write(agents_dir.join("agent.md"), "Valid").unwrap();
        std::fs::write(agents_dir.join("notes.txt"), "Ignored").unwrap();
        std::fs::write(agents_dir.join(".hidden"), "Ignored").unwrap();

        let registry = AgentRegistry::new(tmp.path()).unwrap();
        assert_eq!(registry.len(), 1);
        assert!(registry.get("agent").is_some());
    }
}
