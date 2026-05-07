pub mod file;
pub mod memory;
pub mod search;
pub mod shell;
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
    ]
}
