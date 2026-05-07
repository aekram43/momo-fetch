pub mod file;
pub mod search;
pub mod shell;
pub mod web;
pub mod memory;

use std::sync::Arc;
use adk_tool::Tool;

use crate::sandbox::FilesystemSandbox;

/// Build the complete set of built-in tools for the agent.
///
/// Returns a vector of `Arc<dyn Tool>` ready for registration
/// with `LlmAgentBuilder::tool()`.
pub fn build_tool_registry(sandbox: Arc<FilesystemSandbox>) -> Vec<Arc<dyn Tool>> {
    // Set sandbox for all tool modules
    file::set_sandbox(sandbox.clone());
    shell::set_sandbox(sandbox.clone());
    search::set_sandbox(sandbox);

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
    ]
}
