pub mod secrets;

use std::path::PathBuf;
use serde::{Deserialize, Serialize};

use std::str::FromStr;

use crate::cli::CliArgs;
use crate::sandbox::PermissionMode;

/// Main configuration for the harness.
#[derive(Debug, Clone)]
pub struct HarnessConfig {
    pub project_path: PathBuf,
    pub vault_path: PathBuf,
    #[allow(dead_code)]
    pub session_db_path: PathBuf,
    pub permission_mode: PermissionMode,
    #[allow(dead_code)]
    pub provider: ProviderSettings,
    pub resume_session_id: Option<String>,
    pub memory: MemorySettings,
    /// Agent personality name (from .harness/agents/<name>.md). None = default mode.
    pub agent_name: Option<String>,
}

/// Settings file schema (both global and project-level).
///
/// Global: `~/.config/momo-fetch/settings.json`
/// Project: `<project>/.harness/settings.json`
///
/// CLI flags override both; project overrides global.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SettingsFile {
    /// Default provider name
    pub default_provider: Option<String>,
    /// Default model name
    pub default_model: Option<String>,
    /// Permission mode: "strict" (default), "auto", "yolo"
    pub permission_mode: Option<String>,
    /// Memory auto-flow settings
    pub memory: Option<MemorySettings>,
}

/// Memory auto-flow configuration.
///
/// Controls whether the agent automatically searches memories before each turn
/// and writes memories after each turn.
///
/// ```json
/// {
///   "memory": {
///     "auto_search": true,
///     "auto_write": true,
///     "search_mode": "grep_llm",
///     "max_results_per_turn": 5,
///     "extract_threshold": 10,
///     "sidecar_model": null,
///     "sidecar_provider": null
///   }
/// }
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemorySettings {
    /// Auto-search vault before each turn (grep-based, $0 cost).
    #[serde(default = "default_true")]
    pub auto_search: bool,
    /// Auto-write MemCell after each turn.
    #[serde(default = "default_true")]
    pub auto_write: bool,
    /// Retrieval mode for auto-search: "grep_llm" (default) or "tag_filter".
    #[serde(default = "default_search_mode")]
    pub search_mode: String,
    /// Max memory results injected per turn.
    #[serde(default = "default_max_results")]
    pub max_results_per_turn: usize,
    /// MemCell threshold to trigger auto-extract (events/foresights).
    #[serde(default = "default_extract_threshold")]
    pub extract_threshold: usize,
    /// Sidecar model for memory extraction (Option B).
    /// null = use TF-IDF keywords, no extra LLM call (Option A).
    /// "deepseek-chat" = spawn sub-agent with this model.
    pub sidecar_model: Option<String>,
    /// Sidecar provider (used when sidecar_model is set).
    pub sidecar_provider: Option<String>,
    /// MemCell threshold to trigger auto-consolidate (clusters + profile).
    /// Set to 0 to disable auto-consolidate.
    #[serde(default = "default_consolidate_threshold")]
    pub consolidate_threshold: usize,
}

fn default_true() -> bool { true }
fn default_search_mode() -> String { "grep_llm".into() }
fn default_max_results() -> usize { 5 }
fn default_extract_threshold() -> usize { 10 }
fn default_consolidate_threshold() -> usize { 30 }

impl Default for MemorySettings {
    fn default() -> Self {
        Self {
            auto_search: true,
            auto_write: true,
            search_mode: default_search_mode(),
            max_results_per_turn: default_max_results(),
            extract_threshold: default_extract_threshold(),
            sidecar_model: None,
            sidecar_provider: None,
            consolidate_threshold: default_consolidate_threshold(),
        }
    }
}

/// Provider-related settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderSettings {
    pub default_provider: String,
    pub default_model: String,
}

impl Default for ProviderSettings {
    fn default() -> Self {
        Self {
            default_provider: "anthropic".into(),
            default_model: "claude-sonnet-4-20250514".into(),
        }
    }
}

impl HarnessConfig {
    /// Build config from CLI arguments.
    ///
    /// Settings are loaded in priority order:
    /// 1. CLI flags (highest)
    /// 2. Project-level `.harness/settings.json`
    /// 3. Global `~/.config/momo-fetch/settings.json`
    /// 4. Defaults (lowest)
    pub fn from_cli_args(args: &CliArgs) -> anyhow::Result<Self> {
        let project_path = match &args.project {
            Some(p) => PathBuf::from(p),
            None => std::env::current_dir()?,
        };

        let config_dir = dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("momo-fetch");

        // Load global settings
        let global_settings = Self::load_settings_file(&config_dir.join("settings.json"));

        // Load project settings (overrides global)
        let project_settings =
            Self::load_settings_file(&project_path.join(".harness/settings.json"));

        // Determine permission mode: CLI flag > project settings > global settings > default
        let permission_mode = if args.permission != "strict" {
            // CLI explicitly specified (strict is the default, so if it's different, it was set)
            PermissionMode::from_str(&args.permission).unwrap_or(PermissionMode::Strict)
        } else if let Some(ref mode) = project_settings.permission_mode {
            PermissionMode::from_str(mode).unwrap_or(PermissionMode::Strict)
        } else if let Some(ref mode) = global_settings.permission_mode {
            PermissionMode::from_str(mode).unwrap_or(PermissionMode::Strict)
        } else {
            PermissionMode::Strict
        };

        // Determine provider settings: project > global > defaults
        let provider = ProviderSettings {
            default_provider: project_settings
                .default_provider
                .or(global_settings.default_provider)
                .unwrap_or_else(|| "anthropic".into()),
            default_model: project_settings
                .default_model
                .or(global_settings.default_model)
                .unwrap_or_else(|| "claude-sonnet-4-20250514".into()),
        };

        Ok(Self {
            project_path: project_path.clone(),
            vault_path: project_path.join("memory-vault"),
            session_db_path: config_dir.join("sessions.db"),
            permission_mode,
            provider,
            resume_session_id: args.resume.clone(),
            memory: project_settings
                .memory
                .or(global_settings.memory)
                .unwrap_or_default(),
            agent_name: args.agent.clone(),
        })
    }

    /// Load a settings file, returning default if it doesn't exist or can't be parsed.
    fn load_settings_file(path: &PathBuf) -> SettingsFile {
        if path.exists() {
            let content = std::fs::read_to_string(path).unwrap_or_default();
            serde_json::from_str(&content).unwrap_or_default()
        } else {
            SettingsFile::default()
        }
    }
}
