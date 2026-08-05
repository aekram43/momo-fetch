pub mod file;
pub mod kms;
pub mod memory;
pub mod search;
pub mod shell;
pub mod task;
pub mod web;

use std::sync::{Arc, Mutex};

use adk_tool::Tool;

use crate::memory::vault::ObsidianVault;
use crate::sandbox::FilesystemSandbox;

#[cfg(test)]
pub(crate) mod test_support {
    use std::sync::{Mutex, MutexGuard, OnceLock};

    static SANDBOX_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

    /// Serialise tests that install the process-global sandbox or vault.
    ///
    /// The sandbox/vault contexts in `file`, `shell`, `search`, `kms` and
    /// `memory` are process-global (they must be — tools execute on arbitrary
    /// tokio worker threads, so a thread-local is unset by the time the tool
    /// runs). Tests each point that global at their own temp directory, so
    /// without this guard they clobber one another under the default parallel
    /// test runner.
    ///
    /// Hold the returned guard for the body of the test. Poisoning is ignored:
    /// one failing test should not cascade into every other test erroring.
    pub fn sandbox_guard() -> MutexGuard<'static, ()> {
        SANDBOX_LOCK
            .get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

/// Build the complete set of built-in tools for the agent.
///
/// Returns a vector of `Arc<dyn Tool>` ready for registration
/// with `LlmAgentBuilder::tool()`.
///
/// If `is_orchestrator` is true, also includes orchestrator tools
/// (spawn_agent, send_message, receive_messages).
pub fn build_tool_registry(
    sandbox: Arc<FilesystemSandbox>,
    vault: Arc<Mutex<ObsidianVault>>,
) -> Vec<Arc<dyn Tool>> {
    build_tool_registry_with_orchestrator(sandbox, vault, false)
}

/// Build the tool registry with optional orchestrator tools.
pub fn build_tool_registry_with_orchestrator(
    sandbox: Arc<FilesystemSandbox>,
    vault: Arc<Mutex<ObsidianVault>>,
    is_orchestrator: bool,
) -> Vec<Arc<dyn Tool>> {
    // Set sandbox for all tool modules
    file::set_sandbox(sandbox.clone());
    shell::set_sandbox(sandbox.clone());
    search::set_sandbox(sandbox.clone());
    kms::set_sandbox(sandbox.clone());

    // Set vault for memory tools
    memory::set_vault(vault);

    let mut tools = vec![
        // File tools
        Arc::new(file::FileRead) as Arc<dyn Tool>,
        Arc::new(file::FileWrite),
        Arc::new(file::FileEdit),
        // Shell tool
        Arc::new(shell::ShellExec),
        // Search tools
        Arc::new(search::Grep),
        Arc::new(search::Glob),
        // Web tools
        Arc::new(web::WebSearch),
        Arc::new(web::WebFetch),
        // KMS tools
        Arc::new(kms::KmsRead),
        Arc::new(kms::KmsSearch),
        Arc::new(kms::KmsWrite),
        // Memory tools
        Arc::new(memory::MemWrite),
        Arc::new(memory::MemExtract),
        Arc::new(memory::MemStats),
        Arc::new(memory::MemRead),
        Arc::new(memory::MemSearch),
        Arc::new(memory::MemGraph),
        Arc::new(memory::MemProfile),
        Arc::new(memory::MemConsolidate),
        Arc::new(memory::MemValidateForesights),
        Arc::new(memory::MemReflect),
        // Sub-agent orchestration
        Arc::new(task::Task),
    ];

    // Orchestrator tools (spawn_agent, send_message, receive_messages)
    if is_orchestrator {
        tools.push(Arc::new(crate::agent::orchestrator::SpawnAgent));
        tools.push(Arc::new(crate::agent::orchestrator::SendMessage));
        tools.push(Arc::new(crate::agent::orchestrator::ReceiveMessages));
    }

    tools
}

/// Build a restricted tool set for sub-agents (no Task tool).
///
/// Sub-agents get file, shell, search, web, and memory tools but NOT
/// the Task tool itself, preventing recursive sub-agent spawning.
pub fn build_sub_agent_tool_registry(
    sandbox: Arc<FilesystemSandbox>,
    vault: Arc<Mutex<ObsidianVault>>,
) -> Vec<Arc<dyn Tool>> {
    // Set sandbox for sub-agent tools (may already be set — safe to re-set)
    file::set_sandbox(sandbox.clone());
    shell::set_sandbox(sandbox.clone());
    search::set_sandbox(sandbox.clone());
    kms::set_sandbox(sandbox.clone());

    // Set vault for memory tools
    memory::set_vault(vault);

    vec![
        // File tools
        Arc::new(file::FileRead),
        Arc::new(file::FileWrite),
        Arc::new(file::FileEdit),
        // Shell tool
        Arc::new(shell::ShellExec),
        // Search tools
        Arc::new(search::Grep),
        Arc::new(search::Glob),
        // Web tools
        Arc::new(web::WebSearch),
        Arc::new(web::WebFetch),
        // KMS tools
        Arc::new(kms::KmsRead),
        Arc::new(kms::KmsSearch),
        Arc::new(kms::KmsWrite),
        // Memory tools
        Arc::new(memory::MemWrite),
        Arc::new(memory::MemExtract),
        Arc::new(memory::MemStats),
        Arc::new(memory::MemRead),
        Arc::new(memory::MemSearch),
        Arc::new(memory::MemGraph),
        Arc::new(memory::MemProfile),
        Arc::new(memory::MemConsolidate),
        Arc::new(memory::MemValidateForesights),
        Arc::new(memory::MemReflect),
        // NOTE: No Task tool — prevents recursive sub-agent spawning
    ]
}
