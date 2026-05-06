use std::path::{Path, PathBuf};

use crate::memory::types::{MemoryQuery, MemoryResult, VaultConfig};

/// Obsidian-compatible memory vault engine.
///
/// Manages all vault I/O with vault_path, config, and counters.
pub struct ObsidianVault {
    vault_path: PathBuf,
    config: VaultConfig,
}

impl ObsidianVault {
    /// Open an existing vault or create a new one.
    pub fn open(path: &Path) -> anyhow::Result<Self> {
        let config_path = path.join(".vault-config.json");
        let config: VaultConfig = if config_path.exists() {
            let content = std::fs::read_to_string(&config_path)?;
            serde_json::from_str(&content)?
        } else {
            VaultConfig::default()
        };

        // Ensure directory structure exists
        for dir in &[
            "1-memcells",
            "2-events",
            "3-foresights",
            "4-episodes",
            "5-profile",
            "6-reflections/weekly",
            "6-reflections/monthly",
            "clusters",
            "templates",
        ] {
            std::fs::create_dir_all(path.join(dir))?;
        }

        Ok(Self {
            vault_path: path.to_path_buf(),
            config,
        })
    }

    /// Get the vault root path.
    pub fn path(&self) -> &Path {
        &self.vault_path
    }

    /// Get the vault configuration.
    pub fn config(&self) -> &VaultConfig {
        &self.config
    }

    /// Get a reference to the vault configuration (mutable).
    pub fn config_mut(&mut self) -> &mut VaultConfig {
        &mut self.config
    }

    /// Search using specified retrieval mode (placeholder).
    pub async fn search(&self, _query: &MemoryQuery) -> anyhow::Result<Vec<MemoryResult>> {
        // TODO: Implement retrieval modes in US-013
        Ok(vec![])
    }
}
