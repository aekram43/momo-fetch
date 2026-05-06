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
    // Set sandbox for the current thread so tools can access it
    file::set_sandbox(sandbox);

    vec![
        Arc::new(file::FileRead),
        Arc::new(file::FileWrite),
        Arc::new(file::FileEdit),
    ]
}
