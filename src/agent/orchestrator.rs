//! Orchestrator tools — dynamic agent spawning and inter-agent messaging.
//!
//! Provides tools for the orchestrator agent to:
//! - `spawn_agent`: Dynamically spawn a specialist agent (inline or process mode)
//! - `send_message`: Send a message to a spawned agent via mailbox
//! - `receive_messages`: Receive messages from spawned agents via mailbox

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use adk_rust::agent::LlmAgentBuilder;
use adk_rust::prelude::{Agent, Content};
use adk_rust::runner::Runner;
use adk_tool::{AdkError, tool};
use adk_rust::futures::StreamExt;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::agent::AgentRegistry;
use crate::memory::vault::ObsidianVault;
use crate::providers::ProviderManager;
use crate::sandbox::FilesystemSandbox;
use crate::team::Mailbox;

// ─── Thread-local context for orchestrator dependencies ────────────

thread_local! {
    static ORCH_CTX: std::cell::RefCell<Option<OrchestratorContext>> = std::cell::RefCell::new(None);
}

/// Dependencies needed by the orchestrator tools.
pub struct OrchestratorContext {
    pub provider_mgr: ProviderManager,
    pub sandbox: Arc<FilesystemSandbox>,
    pub vault: Arc<Mutex<ObsidianVault>>,
    pub agent_registry: AgentRegistry,
    pub project_path: PathBuf,
    pub mailbox_path: PathBuf,
    /// Identity of this orchestrator (for mailbox addressing).
    pub identity: String,
}

/// Set the orchestrator context for the current thread.
pub fn set_orchestrator_context(ctx: OrchestratorContext) {
    ORCH_CTX.with(|c| *c.borrow_mut() = Some(ctx));
}

/// Clear the orchestrator context for the current thread.
pub fn clear_orchestrator_context() {
    ORCH_CTX.with(|c| *c.borrow_mut() = None);
}

fn get_orch_context() -> Result<OrchestratorContext, AdkError> {
    ORCH_CTX.with(|c| {
        c.borrow()
            .as_ref()
            .map(|ctx| OrchestratorContext {
                provider_mgr: ProviderManager::from_current(
                    ctx.provider_mgr.current(),
                    ctx.provider_mgr.current_provider().to_string(),
                    ctx.provider_mgr.current_model_name().to_string(),
                ),
                sandbox: ctx.sandbox.clone(),
                vault: ctx.vault.clone(),
                agent_registry: ctx.agent_registry.clone(),
                project_path: ctx.project_path.clone(),
                mailbox_path: ctx.mailbox_path.clone(),
                identity: ctx.identity.clone(),
            })
            .ok_or_else(|| AdkError::tool("orchestrator context not initialized"))
    })
}

// ─── spawn_agent tool ──────────────────────────────────────────────

/// Arguments for the spawn_agent tool.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct SpawnAgentArgs {
    /// Name of the agent personality to spawn (must exist in .harness/agents/).
    pub agent: String,
    /// Task description for the agent.
    pub task: String,
    /// Execution mode: "inline" (default, fast sub-agent) or "process" (separate OS process).
    pub mode: Option<String>,
    /// Timeout in seconds (default: 300). Only applies to inline mode.
    pub timeout_secs: Option<u64>,
}

/// Spawn a specialist agent to handle a task.
///
/// The orchestrator can dynamically spawn specialist agents based on the
/// task at hand. Two execution modes are available:
///
/// - "inline": Spawns an in-process sub-agent. Fast, synchronous, returns
///   the result immediately. Best for quick tasks.
///
/// - "process": Spawns a separate OS process via tmux. Asynchronous, returns
///   a worker ID. Best for long-running tasks. Use send_message/receive_messages
///   to coordinate.
#[tool]
pub async fn spawn_agent(args: SpawnAgentArgs) -> Result<Value, AdkError> {
    let ctx = get_orch_context()?;

    // Resolve agent personality
    let agent_def = ctx
        .agent_registry
        .get(&args.agent)
        .ok_or_else(|| {
            let available: Vec<_> = ctx.agent_registry.list().iter().map(|a| a.name.clone()).collect();
            AdkError::tool(format!(
                "Agent '{}' not found. Available: {}",
                args.agent,
                if available.is_empty() { "(none)".to_string() } else { available.join(", ") }
            ))
        })?
        .clone();

    let mode = args.mode.as_deref().unwrap_or("inline");
    let timeout = Duration::from_secs(args.timeout_secs.unwrap_or(300));

    match mode {
        "inline" => spawn_inline(&ctx, &agent_def, &args.task, timeout).await,
        "process" => spawn_process(&ctx, &agent_def, &args.task),
        _ => Ok(json!({
            "error": format!("Unknown mode '{}'. Use 'inline' or 'process'.", mode)
        })),
    }
}

/// Inline mode: spawn as in-process sub-agent (synchronous).
async fn spawn_inline(
    ctx: &OrchestratorContext,
    agent_def: &crate::agent::AgentDef,
    task: &str,
    timeout: Duration,
) -> Result<Value, AdkError> {
    // Build system prompt for the specialist
    let system_prompt = build_agent_prompt(agent_def);

    // Select model — agent override or current
    let model = if let Some(ref provider) = agent_def.provider {
        // TODO: support provider override when ProviderManager supports it
        ctx.provider_mgr.current()
    } else {
        ctx.provider_mgr.current()
    };

    // Build tool registry (restricted set for spawned agents — no task/orchestrator tools)
    let tools = crate::tools::build_sub_agent_tool_registry(
        ctx.sandbox.clone(),
        ctx.vault.clone(),
    );

    let mut agent_builder = LlmAgentBuilder::new(&format!("spawn-{}", agent_def.name))
        .model(model)
        .instruction(&system_prompt);

    for tool in tools {
        agent_builder = agent_builder.tool(tool);
    }

    let agent = agent_builder
        .build()
        .map_err(|e| AdkError::tool(format!("failed to build agent: {e}")))?;

    let session_service = Arc::new(adk_session::InMemorySessionService::new());
    let runner = Runner::builder()
        .app_name("momo-fetch-spawn")
        .agent(Arc::new(agent))
        .session_service(session_service)
        .build()
        .map_err(|e| AdkError::tool(format!("failed to build runner: {e}")))?;

    // Execute the task
    let content = Content::new("user").with_text(task);
    let stream = runner
        .run_str("default-user", "spawn-session", content)
        .await
        .map_err(|e| AdkError::tool(format!("failed to run agent: {e}")))?;

    // Collect response with timeout
    let result = tokio::time::timeout(timeout, collect_response(stream))
        .await
        .map_err(|_| AdkError::tool(format!("Agent '{}' timed out after {}s", agent_def.name, timeout.as_secs())))?
        .map_err(|e| AdkError::tool(format!("Agent '{}' failed: {e}", agent_def.name)))?;

    Ok(json!({
        "agent": agent_def.name,
        "mode": "inline",
        "result": result
    }))
}

/// Process mode: spawn as separate OS process (asynchronous).
fn spawn_process(
    ctx: &OrchestratorContext,
    agent_def: &crate::agent::AgentDef,
    task: &str,
) -> Result<Value, AdkError> {
    let worker_name = format!("spawn-{}", agent_def.name);

    // Get tmux session or create one
    let session_name = if crate::team::TmuxManager::is_available() {
        let session = crate::team::TmuxManager::create_session(&format!("orch-{}", ctx.identity))
            .map_err(|e| AdkError::tool(format!("tmux session failed: {e}")))?;
        Some(session)
    } else {
        None
    };

    let pane_id = if let Some(ref session) = session_name {
        crate::team::TmuxManager::create_worker_pane(session, &worker_name).ok()
    } else {
        None
    };

    // Launch worker process
    if let Some(ref pid) = pane_id {
        let binary_path = std::env::current_exe()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|_| "momo-fetch".into());

        let work_dir = ctx.project_path.display().to_string();
        let cmd = format!(
            "cd {} && {} -a '{}' -p '{}' 2>&1 | tee .harness/worker-{}.log",
            work_dir,
            binary_path,
            agent_def.name.replace('\'', "'\\''"),
            task.replace('\'', "'\\''"),
            worker_name,
        );

        crate::team::TmuxManager::send_keys(pid, &cmd)
            .map_err(|e| AdkError::tool(format!("failed to start worker: {e}")))?;
    }

    Ok(json!({
        "agent": agent_def.name,
        "mode": "process",
        "worker_id": worker_name,
        "status": "started",
        "message": "Use send_message/receive_messages to coordinate with this agent."
    }))
}

// ─── send_message tool ─────────────────────────────────────────────

/// Arguments for the send_message tool.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct SendMessageArgs {
    /// Recipient agent name (e.g., "spawn-researcher" or a team worker name).
    pub to: String,
    /// Message type (e.g., "instruction", "feedback", "question").
    pub msg_type: String,
    /// Message body.
    pub body: String,
}

/// Send a message to a spawned agent via the mailbox.
#[tool]
pub async fn send_message(args: SendMessageArgs) -> Result<Value, AdkError> {
    let ctx = get_orch_context()?;

    let mailbox = Mailbox::open(&ctx.mailbox_path)
        .map_err(|e| AdkError::tool(format!("mailbox open failed: {e}")))?;

    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);

    mailbox.send(crate::team::MailboxMessage {
        from: ctx.identity.clone(),
        to: args.to.clone(),
        msg_type: args.msg_type.clone(),
        body: args.body,
        timestamp: now_ms,
    })
    .map_err(|e| AdkError::tool(format!("mailbox send failed: {e}")))?;

    Ok(json!({
        "status": "sent",
        "to": args.to,
        "msg_type": args.msg_type
    }))
}

// ─── receive_messages tool ─────────────────────────────────────────

/// Arguments for the receive_messages tool.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct ReceiveMessagesArgs {
    /// Filter by sender name (optional, receives from all if not specified).
    pub from: Option<String>,
}

/// Receive messages from spawned agents via the mailbox.
#[tool]
pub async fn receive_messages(args: ReceiveMessagesArgs) -> Result<Value, AdkError> {
    let ctx = get_orch_context()?;

    let mailbox = Mailbox::open(&ctx.mailbox_path)
        .map_err(|e| AdkError::tool(format!("mailbox open failed: {e}")))?;

    let mut messages = mailbox
        .receive(&ctx.identity)
        .map_err(|e| AdkError::tool(format!("mailbox receive failed: {e}")))?;

    // Filter by sender if specified
    if let Some(from) = &args.from {
        messages.retain(|m| &m.from == from);
    }

    let result: Vec<Value> = messages
        .iter()
        .map(|m| {
            json!({
                "from": m.from,
                "msg_type": m.msg_type,
                "body": m.body,
                "timestamp": m.timestamp,
            })
        })
        .collect();

    Ok(json!({
        "count": result.len(),
        "messages": result
    }))
}

// ─── Helpers ───────────────────────────────────────────────────────

/// Build a system prompt for a spawned agent.
fn build_agent_prompt(agent_def: &crate::agent::AgentDef) -> String {
    let desc = agent_def.description.as_deref().unwrap_or("specialist agent");
    let mut parts = vec![format!(
        "You are MOMO Fetch operating as **{}** — {}. \
         You have access to tools for file operations, \
         shell execution, web search, and memory management. \
         Always prefer using dedicated tools over Bash commands. \
         Be concise. Do not add unnecessary comments or documentation \
         to code you didn't change.",
        agent_def.name, desc
    )];

    if !agent_def.personality.is_empty() {
        parts.push(format!(
            "\n--- Agent Personality ---\n{}",
            agent_def.personality
        ));
    }

    parts.join("\n\n")
}

/// Collect text response from an event stream.
async fn collect_response(mut stream: adk_rust::EventStream) -> anyhow::Result<String> {
    let mut text_parts = Vec::new();

    while let Some(event_result) = stream.next().await {
        let event = event_result?;
        if let Some(content) = event.content() {
            for part in &content.parts {
                if let adk_rust::prelude::Part::Text { text } = part {
                    text_parts.push(text.clone());
                }
            }
        }

        if event.is_final_response() {
            break;
        }
    }

    Ok(text_parts.join(""))
}

// ─── Tests ─────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_agent_prompt_with_personality() {
        let def = crate::agent::AgentDef {
            name: "researcher".to_string(),
            description: Some("Research expert".to_string()),
            personality: "Always cite your sources.".to_string(),
            model: None,
            provider: None,
            tools: None,
            capabilities: vec![],
        };

        let prompt = build_agent_prompt(&def);
        assert!(prompt.contains("researcher"));
        assert!(prompt.contains("Research expert"));
        assert!(prompt.contains("Always cite your sources"));
    }

    #[test]
    fn test_build_agent_prompt_without_personality() {
        let def = crate::agent::AgentDef {
            name: "worker".to_string(),
            description: None,
            personality: String::new(),
            model: None,
            provider: None,
            tools: None,
            capabilities: vec![],
        };

        let prompt = build_agent_prompt(&def);
        assert!(prompt.contains("worker"));
        assert!(prompt.contains("specialist agent"));
        assert!(!prompt.contains("Agent Personality"));
    }

    #[test]
    fn test_spawn_agent_args_deserialization() {
        let args: SpawnAgentArgs = serde_json::from_str(r#"{
            "agent": "researcher",
            "task": "Find all TODO comments",
            "mode": "inline",
            "timeout_secs": 120
        }"#).unwrap();

        assert_eq!(args.agent, "researcher");
        assert_eq!(args.mode.as_deref(), Some("inline"));
        assert_eq!(args.timeout_secs, Some(120));
    }

    #[test]
    fn test_send_message_args_deserialization() {
        let args: SendMessageArgs = serde_json::from_str(r#"{
            "to": "spawn-researcher",
            "msg_type": "instruction",
            "body": "Focus on the auth module"
        }"#).unwrap();

        assert_eq!(args.to, "spawn-researcher");
        assert_eq!(args.msg_type, "instruction");
    }

    #[test]
    fn test_receive_messages_args_deserialization() {
        let args: ReceiveMessagesArgs = serde_json::from_str(r#"{
            "from": "spawn-coder"
        }"#).unwrap();

        assert_eq!(args.from.as_deref(), Some("spawn-coder"));

        let args_no_filter: ReceiveMessagesArgs = serde_json::from_str(r#"{}"#).unwrap();
        assert!(args_no_filter.from.is_none());
    }
}
