use serde::{Deserialize, Serialize};

/// A raw experience memory cell.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemCell {
    pub id: String,
    pub timestamp: chrono::DateTime<chrono::Local>,
    pub project: String,
    pub topic: String,
    pub context: String,
    pub actions: Vec<ActionRecord>,
    pub outcome: String,
    pub keywords: Vec<String>,
}

/// Record of an action taken during a task.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionRecord {
    pub description: String,
    pub result: String,
}

/// An extracted event (fact) from a MemCell.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub id: String,
    pub memcell_ref: String,
    pub fact: String,
    pub significance: Significance,
    pub tags: Vec<String>,
}

/// A foresight (prediction) extracted from a MemCell.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Foresight {
    pub id: String,
    pub memcell_ref: String,
    pub prediction: String,
    pub confidence: f64,
    pub horizon: String,
    pub status: ForesightStatus,
}

/// An episode (narrative summary) generated from MemCells.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Episode {
    pub id: String,
    pub memcell_refs: Vec<String>,
    pub narrative: String,
    pub themes: Vec<String>,
    pub date: chrono::NaiveDate,
}

/// Significance level for events.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum Significance {
    Low,
    Medium,
    High,
}

/// Status of a foresight prediction.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum ForesightStatus {
    Pending,
    Confirmed,
    Disconfirmed,
    Expired,
}

/// Memory retrieval mode.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum RetrievalMode {
    GrepLlm,
    GraphWalk,
    TagFilter,
    Agentic,
}

impl std::str::FromStr for RetrievalMode {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "grep_llm" => Ok(Self::GrepLlm),
            "graph_walk" => Ok(Self::GraphWalk),
            "tag_filter" => Ok(Self::TagFilter),
            "agentic" => Ok(Self::Agentic),
            _ => Err(format!("Unknown retrieval mode: {s}")),
        }
    }
}

/// A memory search query.
#[derive(Debug, Clone)]
pub struct MemoryQuery {
    pub query: String,
    pub mode: RetrievalMode,
    pub levels: Option<Vec<String>>,
    pub project: Option<String>,
    pub tags: Option<Vec<String>>,
    pub limit: usize,
}

/// A memory search result.
#[derive(Debug, Clone)]
pub struct MemoryResult {
    pub ref_id: String,
    pub level: String,
    pub relevance_score: f64,
    pub snippet: String,
    pub path: std::path::PathBuf,
}

/// Vault statistics (stored in .vault-config.json).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VaultStats {
    pub total_memcells: u64,
    pub total_events: u64,
    pub total_foresights: u64,
    pub total_episodes: u64,
    #[serde(default)]
    pub pending_foresights: u64,
}

impl Default for VaultStats {
    fn default() -> Self {
        Self {
            total_memcells: 0,
            total_events: 0,
            total_foresights: 0,
            total_episodes: 0,
            pending_foresights: 0,
        }
    }
}

/// Vault operational settings (stored in .vault-config.json).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VaultSettings {
    #[serde(default = "default_consolidation_threshold")]
    pub consolidation_threshold: u32,
    #[serde(default = "default_profile_max_items")]
    pub profile_max_items: u32,
    #[serde(default = "default_profile_compact_threshold")]
    pub profile_compact_threshold: u32,
    #[serde(default = "default_profile_compact_ratio")]
    pub profile_compact_ratio: f64,
    #[serde(default = "default_archive_after_days")]
    pub archive_after_days: u32,
    #[serde(default = "default_retrieval_mode")]
    pub default_retrieval_mode: String,
    #[serde(default = "default_agentic_round1_limit")]
    pub agentic_round1_limit: u32,
    #[serde(default = "default_agentic_final_limit")]
    pub agentic_final_limit: u32,
    #[serde(default = "default_foresight_duration_days")]
    pub foresight_default_duration_days: u32,
    #[serde(default = "default_cluster_similarity_threshold")]
    pub cluster_similarity_threshold: f64,
    #[serde(default = "default_cluster_max_time_gap_days")]
    pub cluster_max_time_gap_days: u32,
}

impl Default for VaultSettings {
    fn default() -> Self {
        Self {
            consolidation_threshold: default_consolidation_threshold(),
            profile_max_items: default_profile_max_items(),
            profile_compact_threshold: default_profile_compact_threshold(),
            profile_compact_ratio: default_profile_compact_ratio(),
            archive_after_days: default_archive_after_days(),
            default_retrieval_mode: default_retrieval_mode(),
            agentic_round1_limit: default_agentic_round1_limit(),
            agentic_final_limit: default_agentic_final_limit(),
            foresight_default_duration_days: default_foresight_duration_days(),
            cluster_similarity_threshold: default_cluster_similarity_threshold(),
            cluster_max_time_gap_days: default_cluster_max_time_gap_days(),
        }
    }
}

fn default_consolidation_threshold() -> u32 { 5 }
fn default_profile_max_items() -> u32 { 25 }
fn default_profile_compact_threshold() -> u32 { 37 }
fn default_profile_compact_ratio() -> f64 { 0.7 }
fn default_archive_after_days() -> u32 { 90 }
fn default_retrieval_mode() -> String { "grep_llm".to_string() }
fn default_agentic_round1_limit() -> u32 { 20 }
fn default_agentic_final_limit() -> u32 { 20 }
fn default_foresight_duration_days() -> u32 { 7 }
fn default_cluster_similarity_threshold() -> f64 { 0.65 }
fn default_cluster_max_time_gap_days() -> u32 { 7 }

/// Vault ID counters for generating sequential note IDs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VaultCounters {
    #[serde(default)]
    pub event: u32,
    #[serde(default)]
    pub foresight: u32,
    #[serde(default)]
    pub episode: u32,
    #[serde(default)]
    pub cluster: u32,
    #[serde(default)]
    pub reflection: u32,
}

impl Default for VaultCounters {
    fn default() -> Self {
        Self {
            event: 0,
            foresight: 0,
            episode: 0,
            cluster: 0,
            reflection: 0,
        }
    }
}

/// Complete vault configuration stored in .vault-config.json.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VaultConfig {
    #[serde(default = "default_schema_version")]
    pub schema_version: String,
    #[serde(default)]
    pub vault_path: String,
    #[serde(default = "default_created_date")]
    pub created: String,
    #[serde(default)]
    pub stats: VaultStats,
    #[serde(default)]
    pub config: VaultSettings,
    #[serde(default)]
    pub counters: VaultCounters,
}

fn default_schema_version() -> String { "1.0.0".to_string() }
fn default_created_date() -> String {
    chrono::Local::now().format("%Y-%m-%d").to_string()
}

impl Default for VaultConfig {
    fn default() -> Self {
        Self {
            schema_version: default_schema_version(),
            vault_path: String::new(),
            created: default_created_date(),
            stats: VaultStats::default(),
            config: VaultSettings::default(),
            counters: VaultCounters::default(),
        }
    }
}

/// Result of a memcell extraction operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractionResult {
    pub memcell_ref: String,
    pub events_created: Vec<String>,
    pub foresights_created: Vec<String>,
    pub episode_id: Option<String>,
}
