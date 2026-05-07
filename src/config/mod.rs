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
    pub session_db_path: PathBuf,
    pub permission_mode: PermissionMode,
    pub provider: ProviderSettings,
    pub resume_session_id: Option<String>,
}

/// Settings file schema (both global and project-level).
///
/// Global: `~/.config/agent-harness/settings.json`
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
    /// 3. Global `~/.config/agent-harness/settings.json`
    /// 4. Defaults (lowest)
    pub fn from_cli_args(args: &CliArgs) -> anyhow::Result<Self> {
        let project_path = match &args.project {
            Some(p) => PathBuf::from(p),
            None => std::env::current_dir()?,
        };

        let config_dir = dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("agent-harness");

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
