use crate::memory::types::{MemoryQuery, MemoryResult};
use crate::memory::vault::ObsidianVault;

/// Retrieve memories using grep + LLM ranking (placeholder).
pub async fn grep_llm(_vault: &ObsidianVault, _query: &MemoryQuery) -> anyhow::Result<Vec<MemoryResult>> {
    // TODO: Implement in US-013
    Ok(vec![])
}

/// Retrieve memories by walking wikilinks from a seed note (placeholder).
pub async fn graph_walk(_vault: &ObsidianVault, _query: &MemoryQuery) -> anyhow::Result<Vec<MemoryResult>> {
    // TODO: Implement in US-013
    Ok(vec![])
}

/// Retrieve memories by filtering on YAML frontmatter tags (placeholder).
pub async fn tag_filter(_vault: &ObsidianVault, _query: &MemoryQuery) -> anyhow::Result<Vec<MemoryResult>> {
    // TODO: Implement in US-013
    Ok(vec![])
}

/// Agentic retrieval: multi-round grep + LLM sufficiency check (placeholder).
pub async fn agentic(_vault: &ObsidianVault, _query: &MemoryQuery) -> anyhow::Result<Vec<MemoryResult>> {
    // TODO: Implement in US-013
    Ok(vec![])
}
