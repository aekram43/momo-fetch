use std::path::PathBuf;
use serde::{Deserialize, Serialize};

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
    pub fn from_cli_args(args: &CliArgs) -> anyhow::Result<Self> {
        let project_path = match &args.project {
            Some(p) => PathBuf::from(p),
            None => std::env::current_dir()?,
        };

        let config_dir = dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("agent-harness");

        let permission_mode = match args.permission.as_str() {
            "auto" => PermissionMode::Auto,
            "yolo" => PermissionMode::Yolo,
            _ => PermissionMode::Strict,
        };

        // Load settings file if exists
        let settings_path = config_dir.join("settings.json");
        let provider = if settings_path.exists() {
            let content = std::fs::read_to_string(&settings_path)?;
            serde_json::from_str(&content).unwrap_or_default()
        } else {
            ProviderSettings::default()
        };

        Ok(Self {
            project_path: project_path.clone(),
            vault_path: project_path.join("memory-vault"),
            session_db_path: config_dir.join("sessions.db"),
            permission_mode,
            provider,
        })
    }
}
