use crate::config::HarnessConfig;
use crate::memory::vault::ObsidianVault;
use crate::providers::ProviderManager;
use crate::sandbox::FilesystemSandbox;
use crate::context::ContextBuilder;

/// Central orchestrator for the agent harness.
///
/// Owns the adk Runner, agent, vault, and all subsystems.
pub struct Harness {
    provider_mgr: ProviderManager,
    sandbox: FilesystemSandbox,
    context_builder: ContextBuilder,
    vault: ObsidianVault,
    config: HarnessConfig,
}

impl Harness {
    /// Build a new Harness from configuration.
    /// This is the main entry point after CLI argument parsing.
    pub async fn build(config: HarnessConfig) -> anyhow::Result<Self> {
        // Initialize memory vault
        let vault = ObsidianVault::open(&config.vault_path)?;

        // Initialize provider
        let provider_mgr = ProviderManager::from_env()?;

        // Initialize sandbox
        let sandbox = FilesystemSandbox::new(
            &config.project_path,
            config.permission_mode,
        )?;

        // Build context (AGENTS.md + KMS + memory)
        let context_builder = ContextBuilder::new(&config.project_path, &vault)?;

        Ok(Self {
            provider_mgr,
            sandbox,
            context_builder,
            vault,
            config,
        })
    }

    /// Get a reference to the provider manager.
    pub fn provider_mgr(&self) -> &ProviderManager {
        &self.provider_mgr
    }

    /// Get a mutable reference to the provider manager.
    pub fn provider_mgr_mut(&mut self) -> &mut ProviderManager {
        &mut self.provider_mgr
    }

    /// Get a reference to the sandbox.
    pub fn sandbox(&self) -> &FilesystemSandbox {
        &self.sandbox
    }

    /// Get a reference to the context builder.
    pub fn context_builder(&self) -> &ContextBuilder {
        &self.context_builder
    }

    /// Get a reference to the memory vault.
    pub fn vault(&self) -> &ObsidianVault {
        &self.vault
    }

    /// Get a reference to the config.
    pub fn config(&self) -> &HarnessConfig {
        &self.config
    }
}
