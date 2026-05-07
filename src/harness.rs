use crate::config::HarnessConfig;
use crate::memory::vault::ObsidianVault;
use crate::providers::ProviderManager;
use crate::sandbox::FilesystemSandbox;
use crate::context::ContextBuilder;
use crate::session::SessionManager;

/// Central orchestrator for the agent harness.
///
/// Owns the adk Runner, agent, vault, and all subsystems.
pub struct Harness {
    provider_mgr: ProviderManager,
    sandbox: FilesystemSandbox,
    context_builder: ContextBuilder,
    vault: ObsidianVault,
    session_mgr: SessionManager,
    current_session_id: String,
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

        // Initialize session service (SQLite)
        let session_mgr = SessionManager::new(&config.session_db_path).await?;

        // Create a new session (or resume if session_id is provided)
        let current_session_id = config
            .resume_session_id
            .clone()
            .unwrap_or_default();

        let current_session_id = if current_session_id.is_empty() {
            let session = session_mgr.create_session(None).await?;
            session.id().to_string()
        } else {
            // Verify the session exists
            session_mgr.get_session(&current_session_id).await?;
            current_session_id
        };

        Ok(Self {
            provider_mgr,
            sandbox,
            context_builder,
            vault,
            session_mgr,
            current_session_id,
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

    /// Get a reference to the session manager.
    pub fn session_mgr(&self) -> &SessionManager {
        &self.session_mgr
    }

    /// Get the current session ID.
    pub fn current_session_id(&self) -> &str {
        &self.current_session_id
    }

    /// Get a reference to the config.
    pub fn config(&self) -> &HarnessConfig {
        &self.config
    }

    /// Start a new session, replacing the current one.
    pub async fn new_session(&mut self) -> anyhow::Result<String> {
        let session = self.session_mgr.create_session(None).await?;
        self.current_session_id = session.id().to_string();
        Ok(self.current_session_id.clone())
    }

    /// Resume an existing session by ID.
    pub async fn resume_session(&mut self, session_id: &str) -> anyhow::Result<()> {
        // Verify session exists
        self.session_mgr.get_session(session_id).await?;
        self.current_session_id = session_id.to_string();
        Ok(())
    }
}
