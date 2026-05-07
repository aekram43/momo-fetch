use std::cell::RefCell;
use std::sync::{Arc, Mutex};

use adk_tool::{AdkError, tool};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::memory::types::ActionRecord;
use crate::memory::vault::ObsidianVault;

// ─── Thread-local vault context ──────────────────────────────────

thread_local! {
    static VAULT_CTX: RefCell<Option<Arc<Mutex<ObsidianVault>>>> = RefCell::new(None);
}

/// Set the vault for the current thread (called before tool execution).
pub fn set_vault(vault: Arc<Mutex<ObsidianVault>>) {
    VAULT_CTX.with(|ctx| *ctx.borrow_mut() = Some(vault));
}

/// Get the vault for the current thread.
fn get_vault() -> Result<Arc<Mutex<ObsidianVault>>, AdkError> {
    VAULT_CTX.with(|ctx| {
        ctx.borrow()
            .clone()
            .ok_or_else(|| AdkError::tool("memory tool vault not initialized"))
    })
}

/// Clear the vault for the current thread.
pub fn clear_vault() {
    VAULT_CTX.with(|ctx| *ctx.borrow_mut() = None);
}

// ─── Action Record Args ──────────────────────────────────────────

/// Record of an action taken during a task.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct ActionRecordArgs {
    /// What was done
    pub description: String,
    /// What happened as a result
    pub result: String,
}

// ─── MemWrite ────────────────────────────────────────────────────

/// Arguments for the mem_write tool.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct MemWriteArgs {
    /// Project name this memory belongs to
    pub project: String,
    /// Brief topic/title of this memory
    pub topic: String,
    /// Context: what was the user asking or what was happening
    pub context: String,
    /// Actions taken during this interaction
    pub actions: Vec<ActionRecordArgs>,
    /// Outcome or result of the interaction
    pub outcome: String,
    /// Keywords for categorization and retrieval
    pub keywords: Vec<String>,
}

/// Append a MemCell (raw experience) to the daily memory log.
///
/// Creates a structured memory entry with topic, context, actions, outcome,
/// and keywords. The entry is stored in the Obsidian wiki vault at
/// `1-memcells/YYYY/MM/YYYY-MM-DD.md`.
#[tool]
pub async fn mem_write(args: MemWriteArgs) -> Result<Value, AdkError> {
    let vault_arc = get_vault()?;
    let mut vault = vault_arc.lock().map_err(|e| {
        AdkError::tool(format!("mem_write: vault lock failed: {e}"))
    })?;

    let actions: Vec<ActionRecord> = args
        .actions
        .iter()
        .map(|a| ActionRecord {
            description: a.description.clone(),
            result: a.result.clone(),
        })
        .collect();

    let keywords: Vec<&str> = args.keywords.iter().map(|s| s.as_str()).collect();

    let memcell_ref = vault
        .write_memcell(
            &args.project,
            &args.topic,
            &args.context,
            &actions,
            &args.outcome,
            &keywords,
        )
        .map_err(|e| AdkError::tool(format!("mem_write: failed to write MemCell: {e}")))?;

    Ok(json!({
        "memcell_ref": memcell_ref,
        "status": "written",
        "total_memcells": vault.stats().total_memcells,
    }))
}

// ─── MemExtract ──────────────────────────────────────────────────

/// Arguments for the mem_extract tool.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct MemExtractArgs {
    /// MemCell reference to extract from (e.g., "2026-05-07#MemCell 001")
    pub memcell_ref: String,
    /// Project name
    pub project: String,
    /// Topic/title
    pub topic: String,
    /// Context description
    pub context: String,
    /// Actions taken
    pub actions: Vec<ActionRecordArgs>,
    /// Outcome/result
    pub outcome: String,
    /// Keywords for categorization
    pub keywords: Vec<String>,
}

/// Extract events, foresights, and episodes from a MemCell.
///
/// Processes a raw MemCell into structured knowledge:
/// - Events (atomic facts) → `2-events/fact-NNNN.md`
/// - Foresights (predictions) → `3-foresights/pred-NNNN.md`
/// - Episodes (narrative summaries) → `4-episodes/ep-NNNN.md`
#[tool]
pub async fn mem_extract(args: MemExtractArgs) -> Result<Value, AdkError> {
    let vault_arc = get_vault()?;
    let mut vault = vault_arc.lock().map_err(|e| {
        AdkError::tool(format!("mem_extract: vault lock failed: {e}"))
    })?;

    let actions: Vec<ActionRecord> = args
        .actions
        .iter()
        .map(|a| ActionRecord {
            description: a.description.clone(),
            result: a.result.clone(),
        })
        .collect();

    let keywords: Vec<&str> = args.keywords.iter().map(|s| s.as_str()).collect();

    let result = vault
        .extract_from_memcell(
            &args.memcell_ref,
            &args.project,
            &args.topic,
            &args.context,
            &actions,
            &args.outcome,
            &keywords,
        )
        .map_err(|e| AdkError::tool(format!("mem_extract: extraction failed: {e}")))?;

    Ok(json!({
        "memcell_ref": result.memcell_ref,
        "events_created": result.events_created,
        "foresights_created": result.foresights_created,
        "episode_id": result.episode_id,
        "total_events": vault.stats().total_events,
        "total_foresights": vault.stats().total_foresights,
        "total_episodes": vault.stats().total_episodes,
    }))
}

// ─── MemStats ────────────────────────────────────────────────────

/// Get memory vault statistics.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct MemStatsArgs {}

/// Returns statistics about the memory vault.
///
/// Shows total counts for all memory levels (MemCells, Events, Foresights, Episodes)
/// and the current vault configuration.
#[tool]
pub async fn mem_stats(_args: MemStatsArgs) -> Result<Value, AdkError> {
    let vault_arc = get_vault()?;
    let vault = vault_arc.lock().map_err(|e| {
        AdkError::tool(format!("mem_stats: vault lock failed: {e}"))
    })?;

    let stats = vault.stats();
    let counters = vault.counters();

    Ok(json!({
        "memcells": stats.total_memcells,
        "events": stats.total_events,
        "foresights": stats.total_foresights,
        "episodes": stats.total_episodes,
        "pending_foresights": stats.pending_foresights,
        "counters": {
            "next_event": counters.event + 1,
            "next_foresight": counters.foresight + 1,
            "next_episode": counters.episode + 1,
            "clusters": counters.cluster,
            "reflections": counters.reflection,
        },
    }))
}

// ─── MemRead ─────────────────────────────────────────────────────

/// Arguments for the mem_read tool.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct MemReadArgs {
    /// Note ID to read (e.g., "fact-0001", "pred-0001", "ep-0001")
    pub note_id: String,
}

/// Read a specific note from the memory vault by its ID.
///
/// Supports reading events (fact-NNNN), foresights (pred-NNNN),
/// episodes (ep-NNNN), and clusters (cluster-NNN).
#[tool]
pub async fn mem_read(args: MemReadArgs) -> Result<Value, AdkError> {
    let vault_arc = get_vault()?;
    let vault = vault_arc.lock().map_err(|e| {
        AdkError::tool(format!("mem_read: vault lock failed: {e}"))
    })?;

    match vault.read_note(&args.note_id) {
        Ok(Some((path, content))) => Ok(json!({
            "note_id": args.note_id,
            "path": path.to_string_lossy(),
            "content": content,
            "found": true,
        })),
        Ok(None) => Ok(json!({
            "note_id": args.note_id,
            "found": false,
            "error": format!("Note '{}' not found in vault", args.note_id),
        })),
        Err(e) => Err(AdkError::tool(format!(
            "mem_read: failed to read '{}': {e}",
            args.note_id
        ))),
    }
}

// ─── MemSearch ───────────────────────────────────────────────────

/// Arguments for the mem_search tool.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct MemSearchArgs {
    /// Natural language search query
    pub query: String,
    /// Retrieval mode: grep_llm (default), graph_walk, tag_filter, agentic
    pub mode: Option<String>,
    /// Filter by memory levels: memcell, event, foresight, episode, profile, cluster
    pub levels: Option<Vec<String>>,
    /// Filter by project name
    pub project: Option<String>,
    /// Filter by tags (used primarily with tag_filter mode)
    pub tags: Option<Vec<String>>,
    /// Maximum number of results (default: 20)
    pub limit: Option<usize>,
}

/// Search the memory vault using multiple retrieval strategies.
///
/// Supports 4 retrieval modes:
/// - `grep_llm` (default): Keyword search with relevance scoring
/// - `graph_walk`: Follow [[wikilinks]] from a seed note ID
/// - `tag_filter`: Filter by YAML frontmatter tags
/// - `agentic`: Multi-round search with query expansion
///
/// Results include relevance_score (0-1) and content snippets.
#[tool]
pub async fn mem_search(args: MemSearchArgs) -> Result<Value, AdkError> {
    let vault_arc = get_vault()?;
    let vault = vault_arc.lock().map_err(|e| {
        AdkError::tool(format!("mem_search: vault lock failed: {e}"))
    })?;

    let mode_str = args.mode.as_deref().unwrap_or("grep_llm");
    let mode: crate::memory::types::RetrievalMode = mode_str
        .parse()
        .map_err(|e: String| AdkError::tool(format!("mem_search: {e}")))?;

    let query = crate::memory::types::MemoryQuery {
        query: args.query,
        mode,
        levels: args.levels,
        project: args.project,
        tags: args.tags,
        limit: args.limit.unwrap_or(20),
    };

    let results = vault
        .search(&query)
        .map_err(|e| AdkError::tool(format!("mem_search: search failed: {e}")))?;

    let json_results: Vec<Value> = results
        .iter()
        .map(|r| {
            json!({
                "ref_id": r.ref_id,
                "level": r.level,
                "relevance_score": (r.relevance_score * 100.0).round() / 100.0,
                "snippet": r.snippet,
            })
        })
        .collect();

    Ok(json!({
        "results": json_results,
        "total": json_results.len(),
        "mode": mode_str,
    }))
}

// ─── MemGraph ────────────────────────────────────────────────────

/// Arguments for the mem_graph tool.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct MemGraphArgs {
    /// Note ID to explore connections from (e.g., "fact-0001", "pred-0001")
    pub note_id: String,
}

/// Get connected notes (outgoing wikilinks + backlinks) for a vault note.
///
/// Traverses the wikilink graph starting from the specified note:
/// - Outgoing: notes linked FROM this note via [[wikilinks]]
/// - Backlinks: notes that link TO this note
///
/// Useful for exploring related context around a specific memory.
#[tool]
pub async fn mem_graph(args: MemGraphArgs) -> Result<Value, AdkError> {
    let vault_arc = get_vault()?;
    let vault = vault_arc.lock().map_err(|e| {
        AdkError::tool(format!("mem_graph: vault lock failed: {e}"))
    })?;

    let connections = vault
        .graph(&args.note_id)
        .map_err(|e| AdkError::tool(format!("mem_graph: graph traversal failed: {e}")))?;

    let json_connections: Vec<Value> = connections
        .iter()
        .map(|r| {
            json!({
                "ref_id": r.ref_id,
                "level": r.level,
                "relevance_score": (r.relevance_score * 100.0).round() / 100.0,
                "snippet": r.snippet,
            })
        })
        .collect();

    Ok(json!({
        "note_id": args.note_id,
        "connections": json_connections,
        "total": json_connections.len(),
    }))
}

// ─── MemProfile ──────────────────────────────────────────────────

/// Arguments for the mem_profile tool.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct MemProfileArgs {
    /// Which profile to read: "agent" (default) or "user"
    pub profile_type: Option<String>,
}

/// Read agent or user profile from the memory vault.
///
/// Returns the contents of the agent or user profile note from the vault.
/// The agent profile (`5-profile/agent-profile.md`) tracks learned preferences,
/// patterns, and behavioral traits. The user profile (`5-profile/user-profile.md`)
/// tracks user preferences, project context, and communication style.
#[tool]
pub async fn mem_profile(args: MemProfileArgs) -> Result<Value, AdkError> {
    let vault_arc = get_vault()?;
    let vault = vault_arc.lock().map_err(|e| {
        AdkError::tool(format!("mem_profile: vault lock failed: {e}"))
    })?;

    let profile_type = args.profile_type.as_deref().unwrap_or("agent");

    let result = match profile_type {
        "agent" => vault.read_profile(),
        "user" => vault.read_user_profile(),
        _ => Err(anyhow::anyhow!("Unknown profile type: {profile_type}. Use 'agent' or 'user'.")),
    };

    match result {
        Ok(Some((path, content))) => Ok(json!({
            "profile_type": profile_type,
            "path": path.to_string_lossy(),
            "content": content,
            "found": true,
        })),
        Ok(None) => Ok(json!({
            "profile_type": profile_type,
            "found": false,
            "message": format!("No {} profile found in vault", profile_type),
        })),
        Err(e) => Err(AdkError::tool(format!(
            "mem_profile: failed to read {profile_type} profile: {e}"
        ))),
    }
}

// ─── Tests ───────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn setup_vault() -> (tempfile::TempDir, Arc<Mutex<ObsidianVault>>) {
        let tmp = tempfile::tempdir().unwrap();
        let vault = ObsidianVault::open(tmp.path()).unwrap();
        (tmp, Arc::new(Mutex::new(vault)))
    }

    #[tokio::test]
    async fn test_mem_write_tool() {
        let (_tmpdir, vault) = setup_vault();
        set_vault(vault.clone());

        let result = mem_write(MemWriteArgs {
            project: "test-project".into(),
            topic: "Test Topic".into(),
            context: "Testing the mem_write tool".into(),
            actions: vec![ActionRecordArgs {
                description: "ran test".into(),
                result: "passed".into(),
            }],
            outcome: "Tool works correctly".into(),
            keywords: vec!["test".into(), "tool".into()],
        })
        .await
        .unwrap();

        assert_eq!(result["status"], "written");
        assert!(result["memcell_ref"].as_str().unwrap().contains("MemCell 001"));
        assert_eq!(result["total_memcells"], 1);

        clear_vault();
    }

    #[tokio::test]
    async fn test_mem_extract_tool() {
        let (_tmpdir, vault) = setup_vault();
        set_vault(vault.clone());

        let result = mem_extract(MemExtractArgs {
            memcell_ref: "2026-05-07#MemCell 001".into(),
            project: "test-project".into(),
            topic: "Architecture Decision".into(),
            context: "Chose Obsidian wiki for vault".into(),
            actions: vec![ActionRecordArgs {
                description: "researched options".into(),
                result: "found good match".into(),
            }],
            outcome: "Selected".into(),
            keywords: vec!["architecture".into(), "decision".into()],
        })
        .await
        .unwrap();

        assert!(result["events_created"].as_array().unwrap().len() > 0);
        // "architecture" and "decision" are predictive keywords
        assert!(result["foresights_created"].as_array().unwrap().len() > 0);
        assert!(result["episode_id"].as_str().is_some());

        clear_vault();
    }

    #[tokio::test]
    async fn test_mem_stats_tool() {
        let (_tmpdir, vault) = setup_vault();
        set_vault(vault.clone());

        let result = mem_stats(MemStatsArgs {}).await.unwrap();

        assert_eq!(result["memcells"], 0);
        assert_eq!(result["events"], 0);

        clear_vault();
    }

    #[tokio::test]
    async fn test_mem_read_tool() {
        let (_tmpdir, vault) = setup_vault();
        set_vault(vault.clone());

        // Create a note via extraction
        mem_extract(MemExtractArgs {
            memcell_ref: "2026-05-07#MemCell 001".into(),
            project: "test".into(),
            topic: "Test".into(),
            context: "ctx".into(),
            actions: vec![],
            outcome: "ok".into(),
            keywords: vec!["test".into()],
        })
        .await
        .unwrap();

        // Read the event back
        let vault_guard = vault.lock().unwrap();
        let counters = vault_guard.counters();
        let event_id = format!("fact-{:04}", counters.event);
        drop(vault_guard);

        let result = mem_read(MemReadArgs {
            note_id: event_id.clone(),
        })
        .await
        .unwrap();

        assert_eq!(result["found"], true);
        assert!(result["content"].as_str().unwrap().contains(&event_id));

        // Read non-existent note
        let result2 = mem_read(MemReadArgs {
            note_id: "fact-9999".into(),
        })
        .await
        .unwrap();

        assert_eq!(result2["found"], false);

        clear_vault();
    }

    #[tokio::test]
    async fn test_mem_search_tool_grep_llm() {
        let (_tmpdir, vault) = setup_vault();
        set_vault(vault.clone());

        // Create some data
        mem_extract(MemExtractArgs {
            memcell_ref: "2026-05-07#MemCell 001".into(),
            project: "test".into(),
            topic: "Rust async patterns".into(),
            context: "Learning about tokio async runtime".into(),
            actions: vec![],
            outcome: "Async works well".into(),
            keywords: vec!["rust".into(), "async".into(), "tokio".into()],
        })
        .await
        .unwrap();

        // Search for it
        let result = mem_search(MemSearchArgs {
            query: "rust async tokio".into(),
            mode: Some("grep_llm".into()),
            levels: None,
            project: None,
            tags: None,
            limit: Some(10),
        })
        .await
        .unwrap();

        assert!(result["total"].as_u64().unwrap() > 0);
        assert_eq!(result["mode"], "grep_llm");

        let results = result["results"].as_array().unwrap();
        assert!(!results.is_empty());
        assert!(results[0]["ref_id"].is_string());
        assert!(results[0]["relevance_score"].as_f64().unwrap() > 0.0);

        clear_vault();
    }

    #[tokio::test]
    async fn test_mem_search_tool_tag_filter() {
        let (_tmpdir, vault) = setup_vault();
        set_vault(vault.clone());

        // Create data with tags
        mem_extract(MemExtractArgs {
            memcell_ref: "2026-05-07#MemCell 001".into(),
            project: "test".into(),
            topic: "Test Topic".into(),
            context: "ctx".into(),
            actions: vec![],
            outcome: "ok".into(),
            keywords: vec!["rust".into(), "async".into()],
        })
        .await
        .unwrap();

        let result = mem_search(MemSearchArgs {
            query: String::new(),
            mode: Some("tag_filter".into()),
            levels: None,
            project: None,
            tags: Some(vec!["rust".into()]),
            limit: Some(10),
        })
        .await
        .unwrap();

        assert!(result["total"].as_u64().unwrap() > 0);
        assert_eq!(result["mode"], "tag_filter");

        clear_vault();
    }

    #[tokio::test]
    async fn test_mem_graph_tool() {
        let (_tmpdir, vault) = setup_vault();
        set_vault(vault.clone());

        // Create notes with wikilinks
        mem_extract(MemExtractArgs {
            memcell_ref: "2026-05-07#MemCell 001".into(),
            project: "test".into(),
            topic: "Graph test".into(),
            context: "Testing graph traversal".into(),
            actions: vec![],
            outcome: "ok".into(),
            keywords: vec!["graph".into()],
        })
        .await
        .unwrap();

        // Get the event ID
        let vault_guard = vault.lock().unwrap();
        let event_id = format!("fact-{:04}", vault_guard.counters().event);
        drop(vault_guard);

        let result = mem_graph(MemGraphArgs {
            note_id: event_id.clone(),
        })
        .await
        .unwrap();

        assert_eq!(result["note_id"], event_id);
        // connections may be empty if no backlinks exist yet
        assert!(result["total"].as_u64().is_some());

        clear_vault();
    }

    #[tokio::test]
    async fn test_mem_profile_tool_missing() {
        let (_tmpdir, vault) = setup_vault();
        set_vault(vault.clone());

        // No profile created yet
        let result = mem_profile(MemProfileArgs {
            profile_type: Some("agent".into()),
        })
        .await
        .unwrap();

        assert_eq!(result["found"], false);
        assert_eq!(result["profile_type"], "agent");

        clear_vault();
    }

    #[tokio::test]
    async fn test_mem_profile_tool_default_agent() {
        let (_tmpdir, vault) = setup_vault();
        set_vault(vault.clone());

        // Default should be agent profile
        let result = mem_profile(MemProfileArgs {
            profile_type: None,
        })
        .await
        .unwrap();

        assert_eq!(result["profile_type"], "agent");
        assert_eq!(result["found"], false);

        clear_vault();
    }
}
