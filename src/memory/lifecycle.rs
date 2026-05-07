use crate::memory::types::{ConsolidationResult, ExtractionResult, ForesightValidation, Reflection, ReflectionPeriod};
use crate::memory::vault::ObsidianVault;

/// Extract events, foresights, and episodes from MemCells.
///
/// This is a convenience wrapper around `ObsidianVault::extract_from_memcell()`.
pub fn extract_from_memcell(
    vault: &mut ObsidianVault,
    memcell_ref: &str,
    project: &str,
    topic: &str,
    context: &str,
    actions: &[crate::memory::types::ActionRecord],
    outcome: &str,
    keywords: &[&str],
) -> anyhow::Result<ExtractionResult> {
    vault.extract_from_memcell(memcell_ref, project, topic, context, actions, outcome, keywords)
}

/// Consolidate memories into clusters and update agent profile.
///
/// Detects clusters of related MemCells using keyword similarity (Jaccard)
/// and same-project grouping, then updates the agent profile with learned traits.
pub fn consolidate(vault: &mut ObsidianVault) -> anyhow::Result<ConsolidationResult> {
    vault.consolidate()
}

/// Generate a reflection (weekly/monthly) from memories.
///
/// Summarizes recent MemCells and extracts key themes for the given period.
pub fn reflect(vault: &mut ObsidianVault, period: &ReflectionPeriod) -> anyhow::Result<Reflection> {
    vault.reflect(period)
}

/// Validate pending foresight predictions.
///
/// Checks foresights that have passed their end_time and marks them as expired.
pub fn validate_foresights(vault: &mut ObsidianVault) -> anyhow::Result<Vec<ForesightValidation>> {
    vault.validate_foresights()
}
