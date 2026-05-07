use std::sync::{Arc, Mutex};

use adk_rust::agent::LlmAgentBuilder;
use adk_rust::runner::Runner;
use adk_rust::{Content, EventStream, ToolConfirmationPolicy};

use crate::config::HarnessConfig;
use crate::context::ContextBuilder;
use crate::mcp::McpService;
use crate::memory::vault::ObsidianVault;
use crate::providers::ProviderManager;
use crate::sandbox::FilesystemSandbox;
use crate::session::SessionManager;

/// Central orchestrator for the agent harness.
///
/// Owns the adk Runner, agent, vault, and all subsystems.
pub struct Harness {
    provider_mgr: ProviderManager,
    sandbox: Arc<FilesystemSandbox>,
    context_builder: ContextBuilder,
    vault: Arc<Mutex<ObsidianVault>>,
    session_mgr: SessionManager,
    mcp_service: McpService,
    runner: Runner,
    current_session_id: String,
    config: HarnessConfig,
}

impl Harness {
    /// Build a new Harness from configuration.
    /// This is the main entry point after CLI argument parsing.
    pub async fn build(config: HarnessConfig) -> anyhow::Result<Self> {
        // Initialize memory vault
        let vault = Arc::new(Mutex::new(ObsidianVault::open(&config.vault_path)?));

        // Initialize provider
        let provider_mgr = ProviderManager::from_env()?;

        // Initialize sandbox
        let sandbox = Arc::new(FilesystemSandbox::new(
            &config.project_path,
            config.permission_mode,
        )?);

        // Build context (AGENTS.md + KMS + memory)
        let vault_guard = vault.lock().map_err(|e| anyhow::anyhow!("vault lock: {e}"))?;
        let context_builder = ContextBuilder::new(&config.project_path, &vault_guard)?;
        drop(vault_guard);

        // Initialize session service (SQLite)
        let session_mgr = SessionManager::new(&config.session_db_path).await?;

        // Create a new session (or resume if session_id is provided)
        let current_session_id = config.resume_session_id.clone().unwrap_or_default();

        let current_session_id = if current_session_id.is_empty() {
            let session = session_mgr.create_session(None).await?;
            session.id().to_string()
        } else {
            // Verify the session exists
            session_mgr.get_session(&current_session_id).await?;
            current_session_id
        };

        // Initialize MCP service
        let mcp_service = McpService::new(&config.project_path)?;

        // Start all configured MCP servers
        let start_results = mcp_service.start_all().await;
        for (id, result) in &start_results {
            if let Err(e) = result {
                tracing::warn!("MCP server '{id}' failed to start: {e}");
            }
        }
        if !start_results.is_empty() {
            let running = mcp_service.running_count().await;
            tracing::info!(
                "MCP: {}/{} servers started",
                running,
                start_results.len()
            );
        }

        // Start background health monitoring
        if mcp_service.has_servers() {
            mcp_service.start_monitoring();
        }

        // Build Runner with Agent
        let runner = Self::build_runner(
            &provider_mgr,
            &context_builder,
            &sandbox,
            &vault,
            session_mgr.service(),
            &mcp_service,
        )?;

        Ok(Self {
            provider_mgr,
            sandbox,
            context_builder,
            vault,
            session_mgr,
            mcp_service,
            runner,
            current_session_id,
            config,
        })
    }

    /// Build the adk Runner and LlmAgent.
    fn build_runner(
        provider_mgr: &ProviderManager,
        context_builder: &ContextBuilder,
        sandbox: &Arc<FilesystemSandbox>,
        vault: &Arc<Mutex<ObsidianVault>>,
        session_service: Arc<dyn adk_session::SessionService>,
        mcp_service: &McpService,
    ) -> anyhow::Result<Runner> {
        let tools = crate::tools::build_tool_registry(sandbox.clone(), vault.clone());

        let policy = sandbox.to_tool_confirmation_policy();

        let mut agent_builder = LlmAgentBuilder::new("agent-harness")
            .model(provider_mgr.current())
            .instruction(context_builder.system_prompt());

        // Set tool confirmation policy based on permission mode
        match policy {
            ToolConfirmationPolicy::Never => {}
            ToolConfirmationPolicy::Always => {
                agent_builder = agent_builder.tool_confirmation_policy(policy);
            }
            ToolConfirmationPolicy::PerTool(_) => {
                agent_builder = agent_builder.tool_confirmation_policy(policy);
            }
        }

        // Register built-in tools
        for tool in tools {
            agent_builder = agent_builder.tool(tool);
        }

        // Register MCP toolset (if any servers configured)
        if mcp_service.has_servers() {
            agent_builder = agent_builder.toolset(mcp_service.manager());
        }

        let agent = agent_builder.build()?;

        let runner = Runner::builder()
            .app_name("agent-harness")
            .agent(Arc::new(agent))
            .session_service(session_service)
            .build()?;

        Ok(runner)
    }

    /// Rebuild the Runner (e.g., after model/provider switch or MCP changes).
    pub fn rebuild_runner(&mut self) -> anyhow::Result<()> {
        self.runner = Self::build_runner(
            &self.provider_mgr,
            &self.context_builder,
            &self.sandbox,
            &self.vault,
            self.session_mgr.service(),
            &self.mcp_service,
        )?;
        Ok(())
    }

    /// Run a single conversational turn.
    /// Returns an EventStream for the REPL to consume.
    pub async fn run_turn(&self, input: &str) -> anyhow::Result<EventStream> {
        let content = Content::new("user").with_text(input);
        let stream = self
            .runner
            .run_str("default-user", &self.current_session_id, content)
            .await?;
        Ok(stream)
    }

    /// Interrupt current generation.
    pub fn interrupt(&self) -> bool {
        self.runner.interrupt(&self.current_session_id)
    }

    /// Switch model and rebuild runner.
    pub fn switch_model(&mut self, model: &str) -> anyhow::Result<()> {
        self.provider_mgr.switch_model(model)?;
        self.rebuild_runner()
    }

    /// Switch provider and rebuild runner.
    pub fn switch_provider(&mut self, provider: &str) -> anyhow::Result<()> {
        self.provider_mgr.switch_provider(provider)?;
        self.rebuild_runner()
    }

    /// Switch both provider and model, then rebuild runner.
    pub fn switch(&mut self, provider: &str, model: &str) -> anyhow::Result<()> {
        self.provider_mgr.switch(provider, model)?;
        self.rebuild_runner()
    }

    // ── Accessors ──────────────────────────────────────────────

    /// Get a reference to the provider manager.
    pub fn provider_mgr(&self) -> &ProviderManager {
        &self.provider_mgr
    }

    /// Get a mutable reference to the provider manager.
    /// Note: after mutating, call `rebuild_runner()` to update the agent.
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

    /// Get a reference to the memory vault (Arc<Mutex<>>).
    pub fn vault(&self) -> &Arc<Mutex<ObsidianVault>> {
        &self.vault
    }

    /// Get a reference to the session manager.
    pub fn session_mgr(&self) -> &SessionManager {
        &self.session_mgr
    }

    /// Get a reference to the MCP service.
    pub fn mcp_service(&self) -> &McpService {
        &self.mcp_service
    }

    /// Get a mutable reference to the MCP service.
    pub fn mcp_service_mut(&mut self) -> &mut McpService {
        &mut self.mcp_service
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
