use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use adk_rust::agent::LlmAgentBuilder;
use adk_rust::runner::Runner;
use adk_rust::{Content, EventStream, ToolConfirmationPolicy};

use crate::config::HarnessConfig;
use crate::context::ContextBuilder;
use crate::cost::CostTracker;
use crate::mcp::McpService;
use crate::memory::sidecar::MemorySidecar;
use crate::memory::vault::ObsidianVault;
use crate::providers::ProviderManager;
use crate::sandbox::FilesystemSandbox;
use crate::session::SessionManager;
use crate::skill::SkillService;
use crate::team::TeamService;

/// Central orchestrator for the agent harness.
///
/// Owns the adk Runner, agent, vault, and all subsystems.
pub struct Harness {
    provider_mgr: ProviderManager,
    sandbox: Arc<FilesystemSandbox>,
    context_builder: ContextBuilder,
    vault: Arc<Mutex<ObsidianVault>>,
    memory_sidecar: MemorySidecar,
    session_mgr: SessionManager,
    mcp_service: McpService,
    skill_service: SkillService,
    team_service: TeamService,
    cost_tracker: CostTracker,
    runner: Runner,
    current_session_id: String,
    config: HarnessConfig,
}

impl Harness {
    /// Build a new Harness from configuration.
    /// This is the main entry point after CLI argument parsing.
    pub async fn build(config: HarnessConfig) -> anyhow::Result<Self> {
        // Ensure config directory exists
        let config_dir = dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("momo-fetch");
        let _ = std::fs::create_dir_all(&config_dir);

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

        // Initialize memory sidecar (auto-search + auto-write)
        let memory_sidecar = MemorySidecar::new(vault.clone(), config.memory.clone());
        tracing::info!(
            "Memory sidecar: auto_search={}, auto_write={}, sidecar_model={}",
            memory_sidecar.auto_search_enabled(),
            memory_sidecar.auto_write_enabled(),
            memory_sidecar.sidecar_model().unwrap_or("none"),
        );

        // Initialize session service (SQLite)
        let session_mgr = SessionManager::new(&config_dir.join("sessions.db")).await?;

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

        // Initialize skill service
        let skill_service = SkillService::new(&config.project_path)?;
        if skill_service.has_skills() {
            tracing::info!("Skills: {} loaded", skill_service.skill_count());
        }

        // Initialize team service
        let team_service = TeamService::new(&config.project_path)?;

        // Initialize cost tracker
        let cost_tracker = CostTracker::new(config_dir.join("cost.json"));

        // Build Runner with Agent
        let runner = Self::build_runner(
            &provider_mgr,
            &context_builder,
            &sandbox,
            &vault,
            &memory_sidecar,
            session_mgr.service(),
            &mcp_service,
            &skill_service,
        )?;

        // Initialize cost tracker session context
        let project_name = config
            .project_path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        cost_tracker.set_session_context_with_project(
            &current_session_id,
            &provider_mgr.current_provider().to_string(),
            provider_mgr.current_model_name(),
            &project_name,
        );

        Ok(Self {
            provider_mgr,
            sandbox,
            context_builder,
            vault,
            memory_sidecar,
            session_mgr,
            mcp_service,
            skill_service,
            team_service,
            cost_tracker,
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
        memory_sidecar: &MemorySidecar,
        session_service: Arc<dyn adk_session::SessionService>,
        mcp_service: &McpService,
        skill_service: &SkillService,
    ) -> anyhow::Result<Runner> {
        let tools = crate::tools::build_tool_registry(sandbox.clone(), vault.clone());

        let policy = sandbox.to_tool_confirmation_policy();

        // Build system prompt with skill context + memory context
        let mut system_prompt = context_builder.system_prompt().to_string();
        if let Some(skill_ctx) = skill_service.build_skill_context() {
            system_prompt.push_str("\n\n");
            system_prompt.push_str(&skill_ctx);
        }

        // Add memory system prompt if auto features are enabled
        if let Some(memory_ctx) = memory_sidecar.build_system_prompt_addition() {
            system_prompt.push_str(&memory_ctx);
        }

        // Set task context so the Task tool can spawn sub-agents
        crate::tools::task::set_task_context(crate::tools::task::TaskContext {
            provider_mgr: ProviderManager::from_current(
                provider_mgr.current(),
                provider_mgr.current_provider().to_string(),
                provider_mgr.current_model_name().to_string(),
            ),
            sandbox: sandbox.clone(),
            vault: vault.clone(),
            system_prompt: system_prompt.clone(),
            depth: 0,
        });

        let mut agent_builder = LlmAgentBuilder::new("momo-fetch")
            .model(provider_mgr.current())
            .instruction(&system_prompt);

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
            .app_name("momo-fetch")
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
            &self.memory_sidecar,
            self.session_mgr.service(),
            &self.mcp_service,
            &self.skill_service,
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

    /// Run a single conversational turn with memory enrichment.
    ///
    /// If auto_search is enabled, relevant memories are prepended to the input.
    /// Returns the enriched input string (for tracking) and the EventStream.
    pub async fn run_turn_enriched(&self, input: &str) -> anyhow::Result<(String, EventStream)> {
        let enriched = self.memory_sidecar.enrich_input(input);
        let content = Content::new("user").with_text(&enriched);
        let stream = self
            .runner
            .run_str("default-user", &self.current_session_id, content)
            .await?;
        Ok((enriched, stream))
    }

    /// Interrupt current generation.
    pub fn interrupt(&self) -> bool {
        self.runner.interrupt(&self.current_session_id)
    }

    /// Switch model and rebuild runner.
    pub fn switch_model(&mut self, model: &str) -> anyhow::Result<()> {
        self.provider_mgr.switch_model(model)?;
        self.rebuild_runner()?;
        self.cost_tracker
            .set_session_context(
                &self.current_session_id,
                &self.provider_mgr.current_provider().to_string(),
                self.provider_mgr.current_model_name(),
            );
        Ok(())
    }

    /// Switch provider and rebuild runner.
    pub fn switch_provider(&mut self, provider: &str) -> anyhow::Result<()> {
        self.provider_mgr.switch_provider(provider)?;
        self.rebuild_runner()?;
        self.cost_tracker
            .set_session_context(
                &self.current_session_id,
                &self.provider_mgr.current_provider().to_string(),
                self.provider_mgr.current_model_name(),
            );
        Ok(())
    }

    /// Switch both provider and model, then rebuild runner.
    pub fn switch(&mut self, provider: &str, model: &str) -> anyhow::Result<()> {
        self.provider_mgr.switch(provider, model)?;
        self.rebuild_runner()?;
        self.cost_tracker
            .set_session_context(
                &self.current_session_id,
                &self.provider_mgr.current_provider().to_string(),
                self.provider_mgr.current_model_name(),
            );
        Ok(())
    }

    /// Switch permission mode and rebuild runner.
    pub fn switch_permission(&mut self, mode: crate::sandbox::PermissionMode) -> anyhow::Result<()> {
        self.sandbox = Arc::new(FilesystemSandbox::new(
            &self.config.project_path,
            mode,
        )?);
        self.rebuild_runner()?;
        Ok(())
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

    /// Get a reference to the memory sidecar.
    pub fn memory_sidecar(&self) -> &MemorySidecar {
        &self.memory_sidecar
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

    /// Get a reference to the skill service.
    pub fn skill_service(&self) -> &SkillService {
        &self.skill_service
    }

    /// Get a mutable reference to the skill service.
    pub fn skill_service_mut(&mut self) -> &mut SkillService {
        &mut self.skill_service
    }

    /// Get a reference to the team service.
    pub fn team_service(&self) -> &TeamService {
        &self.team_service
    }

    /// Get a mutable reference to the team service.
    pub fn team_service_mut(&mut self) -> &mut TeamService {
        &mut self.team_service
    }

    /// Get a reference to the cost tracker.
    pub fn cost_tracker(&self) -> &CostTracker {
        &self.cost_tracker
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
        self.cost_tracker
            .set_session_context(
                &self.current_session_id,
                &self.provider_mgr.current_provider().to_string(),
                self.provider_mgr.current_model_name(),
            );
        Ok(self.current_session_id.clone())
    }

    /// Resume an existing session by ID.
    pub async fn resume_session(&mut self, session_id: &str) -> anyhow::Result<()> {
        // Verify session exists
        self.session_mgr.get_session(session_id).await?;
        self.current_session_id = session_id.to_string();
        self.cost_tracker
            .set_session_context(
                &self.current_session_id,
                &self.provider_mgr.current_provider().to_string(),
                self.provider_mgr.current_model_name(),
            );
        Ok(())
    }
}
