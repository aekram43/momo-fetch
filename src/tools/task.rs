//! Sub-agent orchestration tool (US-016).
//!
//! Provides a `Task` tool that delegates subtasks to isolated sub-agents
//! using adk-rust's `SequentialAgent` and `ParallelAgent`.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use adk_rust::agent::{LlmAgentBuilder, ParallelAgent, SequentialAgent};
use adk_rust::prelude::{Agent, Content, EventStream, Part};
use adk_rust::runner::Runner;
use adk_tool::{AdkError, tool};
use adk_rust::futures::StreamExt;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::memory::vault::ObsidianVault;
use crate::providers::ProviderManager;
use crate::sandbox::FilesystemSandbox;

// ─── Thread-local context for sub-agent dependencies ──────────────

thread_local! {
    static TASK_CTX: std::cell::RefCell<Option<TaskContext>> = std::cell::RefCell::new(None);
}

/// Maximum recursion depth for sub-agents.
const MAX_RECURSION_DEPTH: u32 = 3;

/// Dependencies needed to spawn sub-agents.
pub struct TaskContext {
    pub provider_mgr: ProviderManager,
    pub sandbox: Arc<FilesystemSandbox>,
    pub vault: Arc<Mutex<ObsidianVault>>,
    pub system_prompt: String,
    /// Current recursion depth (0 = top-level, 1 = first sub-agent, etc.)
    pub depth: u32,
}

/// Set the task context for the current thread.
pub fn set_task_context(ctx: TaskContext) {
    TASK_CTX.with(|c| *c.borrow_mut() = Some(ctx));
}

/// Clear the task context for the current thread.
#[allow(dead_code)]
pub fn clear_task_context() {
    TASK_CTX.with(|c| *c.borrow_mut() = None);
}

fn get_task_context() -> Result<TaskContext, AdkError> {
    TASK_CTX.with(|c| {
        c.borrow()
            .as_ref()
            .map(|ctx| TaskContext {
                // Create a lightweight ProviderManager wrapping the same Arc<dyn Llm>.
                // Arc::clone is cheap — it shares the same model instance.
                provider_mgr: ProviderManager::from_current(
                    ctx.provider_mgr.current(),
                    ctx.provider_mgr.current_provider().to_string(),
                    ctx.provider_mgr.current_model_name().to_string(),
                ),
                sandbox: ctx.sandbox.clone(),
                vault: ctx.vault.clone(),
                system_prompt: ctx.system_prompt.clone(),
                depth: ctx.depth,
            })
            .ok_or_else(|| AdkError::tool("task context not initialized"))
    })
}

// ─── Argument types ───────────────────────────────────────────────

/// A single subtask definition.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct SubtaskDef {
    /// Short description of the subtask (used as agent name and instruction).
    pub description: String,
    /// Optional specific instruction for this subtask (defaults to description).
    pub instruction: Option<String>,
}

/// Arguments for the Task tool.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct TaskArgs {
    /// High-level description of the overall task.
    pub description: String,
    /// Execution mode: "sequential" (one after another) or "parallel" (all at once).
    pub mode: Option<String>,
    /// List of subtasks to delegate.
    pub subtasks: Vec<SubtaskDef>,
    /// Timeout per sub-agent in seconds (default: 300 = 5 minutes).
    pub timeout_secs: Option<u64>,
}

// ─── Task tool ────────────────────────────────────────────────────

/// Delegate subtasks to isolated sub-agents.
///
/// Spawns child agents using adk-rust's SequentialAgent or ParallelAgent.
/// Each sub-agent gets its own tool registry (file, shell, search, web, memory tools).
/// Supports recursion up to 3 levels deep — sub-agents at depth < 3 can also
/// use the Task tool to spawn their own sub-agents. At depth 3, the Task tool
/// is excluded from the tool registry.
#[tool]
pub async fn task(args: TaskArgs) -> Result<Value, AdkError> {
    let ctx = get_task_context()?;

    // Check recursion depth limit
    if ctx.depth >= MAX_RECURSION_DEPTH {
        return Ok(json!({
            "error": format!(
                "Recursion limit reached ({} levels). Sub-agents cannot spawn further sub-agents.",
                MAX_RECURSION_DEPTH
            )
        }));
    }

    // Validate subtasks
    if args.subtasks.is_empty() {
        return Ok(json!({
            "error": "No subtasks provided. Specify at least one subtask."
        }));
    }

    if args.subtasks.len() > 10 {
        return Ok(json!({
            "error": format!(
                "Too many subtasks ({}). Maximum is 10.",
                args.subtasks.len()
            )
        }));
    }

    let timeout = Duration::from_secs(args.timeout_secs.unwrap_or(300));
    let mode = args.mode.as_deref().unwrap_or("sequential");
    let child_depth = ctx.depth + 1;

    // Build sub-agents
    let mut sub_agents: Vec<Arc<dyn Agent>> = Vec::new();
    for (i, subtask) in args.subtasks.iter().enumerate() {
        let agent_name = format!("subtask-{}-{}", i + 1, sanitize_name(&subtask.description));
        let instruction = subtask.instruction.as_deref().unwrap_or(&subtask.description);

        let sub_agent = build_sub_agent(
            &agent_name,
            instruction,
            &ctx,
            child_depth,
        ).map_err(|e| AdkError::tool(format!("failed to build sub-agent '{agent_name}': {e}")))?;

        sub_agents.push(Arc::new(sub_agent));
    }

    // Create workflow agent
    let task_name = format!("task-{}", sanitize_name(&args.description));
    let workflow_agent: Arc<dyn Agent> = match mode {
        "parallel" => Arc::new(
            ParallelAgent::new(task_name, sub_agents).with_shared_state(),
        ),
        _ => Arc::new(
            SequentialAgent::new(task_name, sub_agents),
        ),
    };

    // Execute with timeout using an in-memory session (sub-agents don't need persistence)
    let session_service = Arc::new(adk_session::InMemorySessionService::new());

    let runner = Runner::builder()
        .app_name("momo-fetch-task")
        .agent(workflow_agent)
        .session_service(session_service)
        .build()
        .map_err(|e| AdkError::tool(format!("failed to build sub-agent runner: {e}")))?;

    // Set task context for child execution with incremented depth.
    // Sub-agents that call the Task tool will see child_depth, and when
    // child_depth >= MAX_RECURSION_DEPTH the tool will return an error.
    let child_ctx = TaskContext {
        provider_mgr: ProviderManager::from_current(
            ctx.provider_mgr.current(),
            ctx.provider_mgr.current_provider().to_string(),
            ctx.provider_mgr.current_model_name().to_string(),
        ),
        sandbox: ctx.sandbox.clone(),
        vault: ctx.vault.clone(),
        system_prompt: ctx.system_prompt.clone(),
        depth: child_depth,
    };

    let content = Content::new("user").with_text(&args.description);

    let result = tokio::time::timeout(timeout, async {
        // Install child context for the duration of sub-agent execution
        set_task_context(child_ctx);

        let stream = runner
            .run_str("task-user", "task-session", content)
            .await
            .map_err(|e| AdkError::tool(format!("sub-agent execution failed: {e}")))?;

        let response = collect_final_response(stream).await;

        response
    })
    .await
    .map_err(|_| {
        AdkError::tool(format!(
            "Sub-agent execution timed out after {}s",
            timeout.as_secs()
        ))
    })??;

    Ok(result)
}

// ─── Helpers ──────────────────────────────────────────────────────

/// Build a sub-agent with its own tool set.
///
/// If `depth < MAX_RECURSION_DEPTH`, the sub-agent also gets the Task tool,
/// allowing it to spawn further sub-agents. Otherwise, it only gets
/// file, shell, search, web, and memory tools.
fn build_sub_agent(
    name: &str,
    instruction: &str,
    ctx: &TaskContext,
    depth: u32,
) -> anyhow::Result<adk_rust::agent::LlmAgent> {
    let tools = if depth < MAX_RECURSION_DEPTH {
        // Include Task tool for recursive sub-agent spawning
        crate::tools::build_tool_registry(
            ctx.sandbox.clone(),
            ctx.vault.clone(),
        )
    } else {
        // At max depth: no Task tool (leaf agent)
        crate::tools::build_sub_agent_tool_registry(
            ctx.sandbox.clone(),
            ctx.vault.clone(),
        )
    };

    let mut builder = LlmAgentBuilder::new(name)
        .model(ctx.provider_mgr.current())
        .instruction(instruction);

    for tool in tools {
        builder = builder.tool(tool);
    }

    // Limit iterations to prevent runaway sub-agents
    builder = builder.max_iterations(50);

    builder.build().map_err(Into::into)
}

/// Collect the final text response from an EventStream.
///
/// Iterates through all events, accumulating text content and counting
/// tool calls, until the stream ends or a final response is received.
async fn collect_final_response(mut stream: EventStream) -> Result<Value, AdkError> {
    let mut tool_call_count: usize = 0;
    let mut text_parts: Vec<String> = Vec::new();

    while let Some(event_result) = stream.next().await {
        match event_result {
            Ok(event) => {
                if let Some(content) = event.content() {
                    for part in &content.parts {
                        match part {
                            Part::FunctionCall { name, .. } => {
                                tool_call_count += 1;
                                tracing::debug!("sub-agent tool call: {name}");
                            }
                            Part::Text { text } => {
                                text_parts.push(text.clone());
                            }
                            _ => {}
                        }
                    }
                }

                if event.is_final_response() {
                    break;
                }
            }
            Err(e) => {
                return Err(AdkError::tool(format!(
                    "sub-agent stream error: {e}"
                )));
            }
        }
    }

    let combined_text = text_parts.join("");

    Ok(json!({
        "response": combined_text,
        "total_tool_calls": tool_call_count,
    }))
}

/// Sanitize a description for use as an agent name component.
fn sanitize_name(s: &str) -> String {
    s.chars()
        .take(30)
        .map(|c| if c.is_alphanumeric() || c == '-' { c } else { '-' })
        .collect::<String>()
        .trim_matches('-')
        .to_string()
}

// ─── Tests ────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use adk_model::ollama::OllamaModel;
    use adk_model::ollama::OllamaConfig;

    #[test]
    fn test_sanitize_name() {
        assert_eq!(sanitize_name("hello world"), "hello-world");
        assert_eq!(sanitize_name("fix: auth bug #123"), "fix--auth-bug--123");
        assert_eq!(
            sanitize_name("a very long description that exceeds thirty characters"),
            "a-very-long-description-that-e"
        );
        assert_eq!(sanitize_name("---leading-dashes"), "leading-dashes");
        assert_eq!(sanitize_name(""), "");
        assert_eq!(sanitize_name("---"), "");
    }

    #[test]
    fn test_task_args_deserialization() {
        let args: TaskArgs = serde_json::from_str(r#"{
            "description": "Refactor the auth module",
            "mode": "sequential",
            "subtasks": [
                {"description": "Read current auth code"},
                {"description": "Identify issues", "instruction": "Find security issues in the auth code"}
            ],
            "timeout_secs": 120
        }"#).unwrap();

        assert_eq!(args.description, "Refactor the auth module");
        assert_eq!(args.mode.as_deref(), Some("sequential"));
        assert_eq!(args.subtasks.len(), 2);
        assert_eq!(args.timeout_secs, Some(120));
        assert!(args.subtasks[1].instruction.is_some());
    }

    #[test]
    fn test_task_args_minimal() {
        let args: TaskArgs = serde_json::from_str(r#"{
            "description": "Simple task",
            "subtasks": [{"description": "Do something"}]
        }"#).unwrap();

        assert_eq!(args.mode, None);
        assert_eq!(args.timeout_secs, None);
    }

    #[test]
    fn test_subtask_def() {
        let def: SubtaskDef = serde_json::from_str(
            r#"{"description": "test task"}"#,
        )
        .unwrap();
        assert_eq!(def.description, "test task");
        assert!(def.instruction.is_none());
    }

    #[test]
    fn test_task_args_empty_subtasks() {
        let args: TaskArgs = serde_json::from_str(r#"{
            "description": "empty",
            "subtasks": []
        }"#).unwrap();
        assert!(args.subtasks.is_empty());
    }

    #[test]
    fn test_task_args_parallel_mode() {
        let args: TaskArgs = serde_json::from_str(r#"{
            "description": "parallel work",
            "mode": "parallel",
            "subtasks": [
                {"description": "task a"},
                {"description": "task b"},
                {"description": "task c"}
            ]
        }"#).unwrap();
        assert_eq!(args.mode.as_deref(), Some("parallel"));
        assert_eq!(args.subtasks.len(), 3);
    }

    #[test]
    fn test_task_args_max_subtasks() {
        let subtasks: Vec<SubtaskDef> = (0..12)
            .map(|i| SubtaskDef {
                description: format!("task-{i}"),
                instruction: None,
            })
            .collect();
        let args = TaskArgs {
            description: "many tasks".into(),
            mode: None,
            subtasks,
            timeout_secs: None,
        };
        assert_eq!(args.subtasks.len(), 12);
    }

    #[test]
    fn test_max_recursion_depth() {
        assert_eq!(MAX_RECURSION_DEPTH, 3);
    }

    #[test]
    fn test_task_context_depth_zero() {
        let tmp = tempfile::tempdir().unwrap();
        let vault_dir = tmp.path().join("vault");
        std::fs::create_dir_all(&vault_dir).unwrap();

        let ctx = TaskContext {
            provider_mgr: ProviderManager::from_current(
                Arc::new(OllamaModel::new(OllamaConfig::new("test")).unwrap()),
                "ollama".into(),
                "test".into(),
            ),
            sandbox: Arc::new(FilesystemSandbox::new(
                tmp.path(),
                crate::sandbox::PermissionMode::Auto,
            ).unwrap()),
            vault: Arc::new(Mutex::new(
                ObsidianVault::open(&vault_dir).unwrap()
            )),
            system_prompt: "test".into(),
            depth: 0,
        };
        assert_eq!(ctx.depth, 0);
    }

    #[test]
    fn test_depth_preserved_in_get() {
        let tmp = tempfile::tempdir().unwrap();
        let vault_dir = tmp.path().join("vault");
        std::fs::create_dir_all(&vault_dir).unwrap();

        set_task_context(TaskContext {
            provider_mgr: ProviderManager::from_current(
                Arc::new(OllamaModel::new(OllamaConfig::new("test")).unwrap()),
                "ollama".into(),
                "test".into(),
            ),
            sandbox: Arc::new(FilesystemSandbox::new(
                tmp.path(),
                crate::sandbox::PermissionMode::Auto,
            ).unwrap()),
            vault: Arc::new(Mutex::new(
                ObsidianVault::open(&vault_dir).unwrap()
            )),
            system_prompt: "test".into(),
            depth: 2,
        });
        let ctx = get_task_context().unwrap();
        assert_eq!(ctx.depth, 2);
        clear_task_context();
    }
}
