pub mod file;
pub mod memory;
pub mod search;
pub mod shell;
pub mod task;
pub mod web;

use std::sync::{Arc, Mutex};

use adk_tool::Tool;

use crate::memory::vault::ObsidianVault;
use crate::sandbox::FilesystemSandbox;

/// Build the complete set of built-in tools for the agent.
///
/// Returns a vector of `Arc<dyn Tool>` ready for registration
/// with `LlmAgentBuilder::tool()`.
pub fn build_tool_registry(
    sandbox: Arc<FilesystemSandbox>,
    vault: Arc<Mutex<ObsidianVault>>,
) -> Vec<Arc<dyn Tool>> {
    // Set sandbox for all tool modules
    file::set_sandbox(sandbox.clone());
    shell::set_sandbox(sandbox.clone());
    search::set_sandbox(sandbox);

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
    ]
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
    search::set_sandbox(sandbox);

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
