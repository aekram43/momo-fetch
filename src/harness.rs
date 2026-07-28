use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use adk_rust::agent::LlmAgentBuilder;
use adk_rust::runner::Runner;
use adk_rust::{Content, EventStream, RunConfig, ToolConfirmationDecision, ToolConfirmationPolicy};

use crate::agent::AgentRegistry;
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
    memory_sidecar: Arc<MemorySidecar>,
    status_channel: crate::cli::status::StatusChannel,
    session_mgr: SessionManager,
    mcp_service: McpService,
    skill_service: SkillService,
    team_service: TeamService,
    agent_registry: AgentRegistry,
    cost_tracker: CostTracker,
    /// Tool names that have been approved by the user during this session.
    approved_tools: Arc<Mutex<std::collections::HashSet<String>>>,
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

        // Initialize provider from settings or environment
        let provider_mgr = ProviderManager::from_settings_or_env(
            Some(&config.provider.default_provider),
            Some(&config.provider.default_model)
        )?;
        provider_mgr.prefetch_context_windows();

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
        let memory_sidecar = Arc::new(MemorySidecar::new(vault.clone(), config.memory.clone()));
        let status_channel = crate::cli::status::StatusChannel::new();
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
        let mut mcp_service = McpService::new(&config.project_path)?;

        // Start all configured stdio MCP servers
        let start_results = mcp_service.start_all().await;
        for (id, result) in &start_results {
            if let Err(e) = result {
                tracing::warn!("MCP server '{id}' failed to start: {e}");
            }
        }
        if !start_results.is_empty() {
            let running = mcp_service.running_count().await;
            tracing::info!(
                "MCP: {}/{} stdio servers started",
                running,
                start_results.len()
            );
        }

        // Connect to all HTTP MCP servers
        if mcp_service.has_http_servers() {
            let http_results = mcp_service.connect_http_servers().await;
            let connected = http_results.values().filter(|r| r.is_ok()).count();
            tracing::info!(
                "MCP: {}/{} HTTP servers connected",
                connected,
                http_results.len()
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

        // Load agent registry from .harness/agents/
        let agent_registry = AgentRegistry::new(&config.project_path)?;
        if !agent_registry.is_empty() {
            tracing::info!("Agent personalities: {} registered", agent_registry.len());
        }

        // Resolve agent personality if --agent flag was provided
        let agent_def = if let Some(ref name) = config.agent_name {
            Some(
                agent_registry
                    .get(name)
                    .ok_or_else(|| {
                        let available: Vec<_> =
                            agent_registry.list().iter().map(|a| a.name.clone()).collect();
                        anyhow::anyhow!(
                            "Agent '{}' not found. Available: {}",
                            name,
                            if available.is_empty() {
                                "(none)".to_string()
                            } else {
                                available.join(", ")
                            }
                        )
                    })?
                    .clone(),
            )
        } else {
            None
        };

        // Initialize cost tracker
        let cost_tracker = CostTracker::new(config_dir.join("cost.json"));

        // Build Runner with Agent (use agent-specific prompt if agent selected)
        let runner = Self::build_runner(
            &provider_mgr,
            &context_builder,
            &sandbox,
            &vault,
            &memory_sidecar,
            session_mgr.service(),
            &mcp_service,
            &skill_service,
            agent_def.as_ref(),
            &std::collections::HashSet::new(),
            &status_channel,
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
            status_channel,
            session_mgr,
            mcp_service,
            skill_service,
            team_service,
            agent_registry,
            cost_tracker,
            approved_tools: Arc::new(Mutex::new(std::collections::HashSet::new())),
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
        agent_def: Option<&crate::agent::AgentDef>,
        approved_tools: &std::collections::HashSet<String>,
        status_channel: &crate::cli::status::StatusChannel,
    ) -> anyhow::Result<Runner> {
        let tools = crate::tools::build_tool_registry_with_orchestrator(
            sandbox.clone(),
            vault.clone(),
            agent_def.map(|d| d.capabilities.contains(&"orchestration".to_string())).unwrap_or(false),
        );

        let policy = sandbox.to_tool_confirmation_policy();

        // Build system prompt — agent-specific if agent is selected, otherwise default
        let mut system_prompt = match agent_def {
            Some(def) => context_builder.system_prompt_for_agent(def),
            None => context_builder.system_prompt().to_string(),
        };
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

        // Set status sender so the Task tool can emit progress events
        crate::tools::task::set_status_sender(status_channel.sender());

        // Set orchestrator context if agent has orchestration capability
        if let Some(def) = agent_def {
            if def.capabilities.contains(&"orchestration".to_string()) {
                let project_path = context_builder.project_path().to_path_buf();
                crate::agent::orchestrator::set_orchestrator_context(
                    crate::agent::orchestrator::OrchestratorContext {
                        provider_mgr: ProviderManager::from_current(
                            provider_mgr.current(),
                            provider_mgr.current_provider().to_string(),
                            provider_mgr.current_model_name().to_string(),
                        ),
                        sandbox: sandbox.clone(),
                        vault: vault.clone(),
                        agent_registry: AgentRegistry::new(&project_path)
                            .unwrap_or_else(|_| AgentRegistry::empty(
                                project_path.join(".harness").join("agents")
                            )),
                        project_path: project_path.clone(),
                        mailbox_path: project_path.join(".harness").join("mailbox"),
                        identity: def.name.clone(),
                    },
                );
            }
        }

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

        // Register MCP toolset (stdio + HTTP merged)
        if let Some(toolset) = mcp_service.toolset() {
            agent_builder = agent_builder.toolset(toolset);
        }

        let agent = agent_builder.build()?;

        let mut run_config = RunConfig::default();
        for tool_name in approved_tools {
            run_config
                .tool_confirmation_decisions
                .insert(tool_name.clone(), ToolConfirmationDecision::Approve);
        }

        let mut runner_builder = Runner::builder()
            .app_name("momo-fetch")
            .agent(Arc::new(agent))
            .session_service(session_service);

        if !approved_tools.is_empty() {
            runner_builder = runner_builder.run_config(run_config);
        }

        let runner = runner_builder.build()?;

        Ok(runner)
    }

    /// Rebuild the Runner (e.g., after model/provider switch or MCP changes).
    pub fn rebuild_runner(&mut self) -> anyhow::Result<()> {
        let agent_def = self
            .config
            .agent_name
            .as_ref()
            .and_then(|name| self.agent_registry.get(name));
        let approved = self.approved_tools.lock().map(|g| g.clone()).unwrap_or_default();
        self.runner = Self::build_runner(
            &self.provider_mgr,
            &self.context_builder,
            &self.sandbox,
            &self.vault,
            &self.memory_sidecar,
            self.session_mgr.service(),
            &self.mcp_service,
            &self.skill_service,
            agent_def,
            &approved,
            &self.status_channel,
        )?;
        Ok(())
    }

    /// Run a single conversational turn.
    /// Returns an EventStream for the REPL to consume.
    #[allow(dead_code)]
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


    /// Run a follow-up turn after tool confirmation prompt.
    /// Records the approval decision and rebuilds the runner with the decision
    /// baked into RunConfig so adk-agent won't ask again for this tool.
    pub async fn run_confirmation_turn(
        &mut self,
        tool_name: &str,
        approved: bool,
    ) -> anyhow::Result<EventStream> {
        if approved {
            if let Ok(mut guard) = self.approved_tools.lock() {
                guard.insert(tool_name.to_string());
            }
            // Rebuild runner with the approved tool in RunConfig
            self.rebuild_runner()?;
        }
        let text = if approved { "approved" } else { "denied" };
        let content = Content::new("user").with_text(text);
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

    // ── Turn lifecycle (shared by the REPL and the gateway) ────
    //
    // Cost accounting used to live only in the REPL, which meant turns driven
    // through the gateway recorded nothing and `/v1/cost` under-reported. These
    // three calls are the single definition of the lifecycle; both front-ends
    // use them so they cannot drift.

    /// Reset per-turn accumulators. Call before every turn leg — including the
    /// follow-up leg after a tool confirmation, which is a separate LLM turn.
    pub fn begin_turn(&self) {
        self.cost_tracker.reset_turn();
    }

    /// Record usage metadata carried by a stream event.
    /// Returns the incremental cost in USD, if the event carried usage.
    pub fn record_usage(&self, usage: &adk_rust::UsageMetadata) -> Option<f64> {
        self.cost_tracker.record_event(usage)
    }

    /// Persist accumulated cost once the turn (all legs) is finished.
    pub fn end_turn(&self) {
        self.cost_tracker.finalize_turn();
    }

    /// Context-window usage for the most recently recorded prompt.
    pub fn context_usage(&self) -> crate::context_window::ContextUsage {
        let provider = self.provider_mgr.current_provider().to_string();
        let model = self.provider_mgr.current_model_name().to_string();
        crate::context_window::ContextUsage::new_resolved(
            self.cost_tracker.last_prompt_tokens() as i64,
            &provider,
            &model,
            Some(&self.config.context_window_overrides),
            Some(self.provider_mgr.context_window_cache().as_ref()),
        )
    }

    /// Tool names the user has approved for the lifetime of this process.
    ///
    /// Approval is sticky by tool *name* (see `run_confirmation_turn`), so this
    /// is the set of tools that will no longer prompt for confirmation.
    pub fn approved_tools(&self) -> Vec<String> {
        let mut names = self
            .approved_tools
            .lock()
            .map(|g| g.iter().cloned().collect::<Vec<_>>())
            .unwrap_or_default();
        names.sort();
        names
    }

    /// Revoke all sticky tool approvals and rebuild the runner so the next
    /// matching tool call prompts again.
    pub fn clear_approved_tools(&mut self) -> anyhow::Result<()> {
        if let Ok(mut guard) = self.approved_tools.lock() {
            guard.clear();
        }
        self.rebuild_runner()?;
        Ok(())
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
        self.provider_mgr.prefetch_context_windows();
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
        self.provider_mgr.prefetch_context_windows();
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
        self.provider_mgr.prefetch_context_windows();
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

    /// Switch to a specific agent personality mid-session.
    /// Rebuilds the runner with the agent's system prompt.
    pub fn switch_agent(&mut self, name: &str) -> anyhow::Result<()> {
        let agent_def = self
            .agent_registry
            .get(name)
            .ok_or_else(|| {
                let available: Vec<_> =
                    self.agent_registry.list().iter().map(|a| a.name.clone()).collect();
                anyhow::anyhow!(
                    "Agent '{}' not found. Available: {}",
                    name,
                    if available.is_empty() {
                        "(none)".to_string()
                    } else {
                        available.join(", ")
                    }
                )
            })?
            .clone();

        self.config.agent_name = Some(name.to_string());
        self.rebuild_runner_with_agent(Some(&agent_def))?;

        // Set orchestrator context if the new agent has orchestration capability
        if agent_def.capabilities.contains(&"orchestration".to_string()) {
            let project_path = self.config.project_path.clone();
            crate::agent::orchestrator::set_orchestrator_context(
                crate::agent::orchestrator::OrchestratorContext {
                    provider_mgr: ProviderManager::from_current(
                        self.provider_mgr.current(),
                        self.provider_mgr.current_provider().to_string(),
                        self.provider_mgr.current_model_name().to_string(),
                    ),
                    sandbox: self.sandbox.clone(),
                    vault: self.vault.clone(),
                    agent_registry: AgentRegistry::new(&project_path)
                        .unwrap_or_else(|_| AgentRegistry::empty(
                            project_path.join(".harness").join("agents")
                        )),
                    project_path: project_path.clone(),
                    mailbox_path: project_path.join(".harness").join("mailbox"),
                    identity: agent_def.name.clone(),
                },
            );
        } else {
            crate::agent::orchestrator::clear_orchestrator_context();
        }

        Ok(())
    }

    /// Clear the agent personality and switch back to default mode.
    /// Rebuilds the runner with the default system prompt.
    pub fn clear_agent(&mut self) -> anyhow::Result<()> {
        self.config.agent_name = None;
        self.rebuild_runner_with_agent(None)?;
        crate::agent::orchestrator::clear_orchestrator_context();
        Ok(())
    }

    /// Rebuild runner with an explicit agent definition.
    fn rebuild_runner_with_agent(
        &mut self,
        agent_def: Option<&crate::agent::AgentDef>,
    ) -> anyhow::Result<()> {
        let approved = self.approved_tools.lock().map(|g| g.clone()).unwrap_or_default();
        self.runner = Self::build_runner(
            &self.provider_mgr,
            &self.context_builder,
            &self.sandbox,
            &self.vault,
            &self.memory_sidecar,
            self.session_mgr.service(),
            &self.mcp_service,
            &self.skill_service,
            agent_def,
            &approved,
            &self.status_channel,
        )?;

        // Update task context with the new system prompt
        let new_prompt = match agent_def {
            Some(def) => self.context_builder.system_prompt_for_agent(def),
            None => self.context_builder.system_prompt().to_string(),
        };
        crate::tools::task::set_task_context(crate::tools::task::TaskContext {
            provider_mgr: ProviderManager::from_current(
                self.provider_mgr.current(),
                self.provider_mgr.current_provider().to_string(),
                self.provider_mgr.current_model_name().to_string(),
            ),
            sandbox: self.sandbox.clone(),
            vault: self.vault.clone(),
            system_prompt: new_prompt,
            depth: 0,
        });

        Ok(())
    }

    // ── Accessors ──────────────────────────────────────────────

    /// Get a reference to the provider manager.
    pub fn provider_mgr(&self) -> &ProviderManager {
        &self.provider_mgr
    }

    /// Get a mutable reference to the provider manager.
    /// Note: after mutating, call `rebuild_runner()` to update the agent.
    #[allow(dead_code)]
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

    /// Get a reference to the memory sidecar (Arc for sharing across threads).
    pub fn memory_sidecar(&self) -> &Arc<MemorySidecar> {
        &self.memory_sidecar
    }

    /// Get a reference to the status channel (for background worker events).
    pub fn status_channel(&self) -> &crate::cli::status::StatusChannel {
        &self.status_channel
    }

    /// Get the mailbox path for this project (used for Option C sidecar IPC).
    pub fn mailbox_path(&self) -> std::path::PathBuf {
        self.config.project_path.join(".harness").join("mailbox")
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

    /// Get a reference to the agent registry.
    pub fn agent_registry(&self) -> &AgentRegistry {
        &self.agent_registry
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
    #[allow(dead_code)]
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

    /// Clear all context by starting a fresh session (deleting the old one).
    pub async fn clear_session(&mut self) -> anyhow::Result<String> {
        let old_id = self.current_session_id.clone();
        let new_id = self.new_session().await?;
        // Best-effort delete of old session
        let _ = self.session_mgr.delete_session(&old_id).await;
        Ok(new_id)
    }

    /// Compact context: summarize the current session events into a single
    /// summary event in a new session.
    pub async fn compact_session(&mut self) -> anyhow::Result<(usize, String)> {
        let old_id = self.current_session_id.clone();

        // Extract text from current session events
        let session = self.session_mgr.get_session(&old_id).await?;
        let events = session.events().all();
        let event_count = events.len();

        let mut summary_parts: Vec<String> = Vec::new();
        for event in &events {
            if let Some(content) = event.content() {
                for part in &content.parts {
                    if let adk_rust::Part::Text { text } = part {
                        let truncated = if text.len() > 500 {
                            format!("{}...", &text[..500])
                        } else {
                            text.clone()
                        };
                        let role = &content.role;
                        summary_parts.push(format!("[{role}] {truncated}"));
                    }
                }
            }
        }

        let summary_text = if summary_parts.is_empty() {
            "Conversation was compacted but contained no text.".to_string()
        } else {
            format!(
                "Summary of previous conversation ({} events):\n{}",
                event_count,
                summary_parts.join("\n")
            )
        };

        // Create new session and append summary as a single event
        let new_id = self.new_session().await?;

        let summary_event = adk_rust::Event::new("compact");
        let mut summary_event = summary_event;
        summary_event.author = "system".to_string();
        summary_event.set_content(adk_rust::Content::new("system").with_text(&summary_text));

        self.session_mgr
            .service()
            .append_event(&new_id, summary_event)
            .await?;

        // Best-effort delete of old session
        let _ = self.session_mgr.delete_session(&old_id).await;

        Ok((event_count, new_id))
    }
}
