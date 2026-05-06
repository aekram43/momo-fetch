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

/// Vault configuration stored in .vault-config.json.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VaultConfig {
    pub version: String,
    pub total_memcells: u64,
    pub total_events: u64,
    pub total_foresights: u64,
    pub total_episodes: u64,
}

impl Default for VaultConfig {
    fn default() -> Self {
        Self {
            version: "1.0.0".into(),
            total_memcells: 0,
            total_events: 0,
            total_foresights: 0,
            total_episodes: 0,
        }
    }
}
