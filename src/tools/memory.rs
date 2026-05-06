use serde::{Deserialize, Serialize};

use crate::memory::vault::ObsidianVault;

/// Memory tool arguments (placeholder — full implementation in US-012/US-013).
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct MemWriteArgs {
    pub project: String,
    pub topic: String,
    pub context: String,
    pub outcome: String,
    pub keywords: Vec<String>,
}

/// Memory search arguments.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct MemSearchArgs {
    pub query: String,
    pub mode: Option<String>,
    pub limit: Option<usize>,
}

/// Write a memory cell (placeholder — full implementation in US-012).
pub async fn mem_write(
    _vault: &ObsidianVault,
    _args: &MemWriteArgs,
) -> anyhow::Result<String> {
    // TODO: Implement in US-012
    Ok("mem_write: not yet implemented".into())
}

/// Search memories (placeholder — full implementation in US-013).
pub async fn mem_search(
    _vault: &ObsidianVault,
    _args: &MemSearchArgs,
) -> anyhow::Result<Vec<serde_json::Value>> {
    // TODO: Implement in US-013
    Ok(vec![])
}
