//! Memory sidecar — auto-search before turns, auto-write after turns.
//!
//! Implements Option A (callback-based, zero extra LLM cost for search)
//! and Option B (sub-agent with small model for extraction).
//! Option C (separate process via Mailbox) is in `src/team/mod.rs`.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use crate::config::MemorySettings;
use crate::memory::types::{ActionRecord, MemoryQuery, RetrievalMode};
use crate::memory::vault::ObsidianVault;

/// Summary of a completed conversational turn, used for auto-write.
#[derive(Debug, Clone)]
pub struct TurnSummary {
    /// The user's original message (truncated).
    pub user_message: String,
    /// Tool calls made during the turn (names + key args).
    pub tool_calls: Vec<String>,
    /// Preview of the agent's final response.
    pub response_preview: String,
    /// Project name for the memory entry.
    pub project: String,
}

/// Pre-turn search result for injection into user input.
#[derive(Debug, Clone)]
pub struct SearchResult {
    /// Formatted memory context to prepend to user input.
    pub context_block: String,
    /// Number of results found.
    pub count: usize,
}

/// The memory sidecar orchestrates auto-search and auto-write.
///
/// - **Option A** (default): grep-based search ($0), TF-IDF keyword extraction
///   for writes (no extra LLM call). Uses the main model's turn for extraction
///   quality when sidecar_model is null.
/// - **Option B** (opt-in): When `sidecar_model` is set, spawns a sub-agent
///   with a small model for memory extraction/write.
pub struct MemorySidecar {
    vault: Arc<Mutex<ObsidianVault>>,
    config: MemorySettings,
    /// Counter for auto-extract trigger (incremented per write).
    memcells_since_extract: AtomicUsize,
}

impl MemorySidecar {
    /// Create a new MemorySidecar with the given vault and settings.
    pub fn new(vault: Arc<Mutex<ObsidianVault>>, config: MemorySettings) -> Self {
        Self {
            vault,
            config,
            memcells_since_extract: AtomicUsize::new(0),
        }
    }

    /// Whether auto-search is enabled.
    pub fn auto_search_enabled(&self) -> bool {
        self.config.auto_search
    }

    /// Whether auto-write is enabled.
    pub fn auto_write_enabled(&self) -> bool {
        self.config.auto_write
    }

    /// Whether Option B (sub-agent) is configured.
    #[allow(dead_code)]
    pub fn has_sidecar_model(&self) -> bool {
        self.config.sidecar_model.is_some()
    }

    /// Get the sidecar model name (for Option B).
    pub fn sidecar_model(&self) -> Option<&str> {
        self.config.sidecar_model.as_deref()
    }

    /// Get the sidecar provider name (for Option B).
    #[allow(dead_code)]
    pub fn sidecar_provider(&self) -> Option<&str> {
        self.config.sidecar_provider.as_deref()
    }

    /// Get the memory settings.
    #[allow(dead_code)]
    pub fn settings(&self) -> &MemorySettings {
        &self.config
    }

    // ─── Pre-Turn: Auto-Search ─────────────────────────────────────

    /// Search the vault for memories relevant to the user's input.
    ///
    /// Uses grep-based search (zero LLM cost). Returns a formatted block
    /// of relevant memories to inject before the user's message.
    pub fn search_for_context(&self, user_input: &str) -> SearchResult {
        let vault = match self.vault.lock() {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!("Memory sidecar: vault lock failed during search: {e}");
                return SearchResult {
                    context_block: String::new(),
                    count: 0,
                };
            }
        };

        let mode = match self.config.search_mode.parse::<RetrievalMode>() {
            Ok(m) => m,
            Err(_) => RetrievalMode::GrepLlm,
        };

        let query = MemoryQuery {
            query: user_input.to_string(),
            mode,
            levels: None,
            project: None,
            tags: None,
            limit: self.config.max_results_per_turn,
        };

        match vault.search(&query) {
            Ok(results) if results.is_empty() => SearchResult {
                context_block: String::new(),
                count: 0,
            },
            Ok(results) => {
                let count = results.len();
                let entries: Vec<String> = results
                    .iter()
                    .map(|r| {
                        format!(
                            "- {} (score: {:.2}): {}",
                            r.ref_id,
                            r.relevance_score,
                            truncate_str(&r.snippet, 120)
                        )
                    })
                    .collect();

                let context_block = format!(
                    "--- Relevant memories ---\n{}\n---\n",
                    entries.join("\n")
                );

                SearchResult {
                    context_block,
                    count,
                }
            }
            Err(e) => {
                tracing::warn!("Memory sidecar: search failed: {e}");
                SearchResult {
                    context_block: String::new(),
                    count: 0,
                }
            }
        }
    }

    /// Enrich user input with relevant memories (pre-turn injection).
    ///
    /// If auto_search is disabled or no results found, returns the original input.
    pub fn enrich_input(&self, user_input: &str) -> String {
        if !self.config.auto_search {
            return user_input.to_string();
        }

        let result = self.search_for_context(user_input);
        if result.count == 0 {
            return user_input.to_string();
        }

        format!(
            "{}\n{}",
            result.context_block, user_input
        )
    }

    // ─── Post-Turn: Auto-Write ─────────────────────────────────────

    /// Write a MemCell from the turn summary (Option A: TF-IDF keywords).
    ///
    /// Extracts keywords from the turn text using TF-IDF-like scoring,
    /// then writes a MemCell to the vault.
    pub fn write_turn_memory_option_a(&self, turn: &TurnSummary) -> anyhow::Result<String> {
        let keywords = self.extract_keywords_tfidf(turn);

        let project = if turn.project.is_empty() {
            "default".to_string()
        } else {
            turn.project.clone()
        };

        let topic = self.extract_topic(turn);
        let context = self.extract_context(turn);
        let actions: Vec<ActionRecord> = turn
            .tool_calls
            .iter()
            .map(|tc| ActionRecord {
                description: tc.clone(),
                result: "executed".to_string(),
            })
            .collect();
        let outcome = truncate_str(&turn.response_preview, 300).to_string();
        let kw_refs: Vec<&str> = keywords.iter().map(|s| s.as_str()).collect();

        let mut vault = self.vault.lock().map_err(|e| anyhow::anyhow!("vault lock: {e}"))?;
        let memcell_ref = vault.write_memcell(
            &project,
            &topic,
            &context,
            &actions,
            &outcome,
            &kw_refs,
        )?;

        tracing::debug!("Memory sidecar: wrote MemCell '{memcell_ref}'");

        // Check auto-extract threshold
        let prev = self.memcells_since_extract.fetch_add(1, Ordering::Relaxed);
        if prev + 1 >= self.config.extract_threshold {
            self.memcells_since_extract.store(0, Ordering::Relaxed);
            // Auto-extract is deferred — the agent can trigger it via mem_extract
            // tool, or it will happen on next consolidate cycle.
            tracing::info!(
                "Memory sidecar: {} MemCells written since last extract (threshold: {})",
                prev + 1,
                self.config.extract_threshold
            );
        }

        Ok(memcell_ref)
    }

    /// Build a sub-agent prompt for memory extraction (Option B).
    #[allow(dead_code)]
    ///
    /// Returns the prompt to send to the sidecar sub-agent, which will
    /// extract structured memory data from the turn summary.
    pub fn build_sidecar_prompt(&self, turn: &TurnSummary) -> String {
        format!(
            "Extract a memory entry from this conversation turn.\n\n\
             User message: {}\n\n\
             Tool calls: {}\n\n\
             Agent response: {}\n\n\
             Project: {}\n\n\
             Return a JSON object with these fields:\n\
             - topic: brief title (5-10 words)\n\
             - context: what was happening (1-2 sentences)\n\
             - actions: array of {{description, result}} objects\n\
             - outcome: what was achieved (1-2 sentences)\n\
             - keywords: array of 3-7 relevant keywords/tags",
            truncate_str(&turn.user_message, 200),
            turn.tool_calls.join(", "),
            truncate_str(&turn.response_preview, 300),
            turn.project,
        )
    }

    // ─── System Prompt Context ─────────────────────────────────────

    /// Build the memory system prompt addition.
    ///
    /// This is appended to the system prompt when auto_search is enabled,
    /// informing the agent about the automatic memory system.
    pub fn build_system_prompt_addition(&self) -> Option<String> {
        if !self.config.auto_search && !self.config.auto_write {
            return None;
        }

        let vault = match self.vault.lock() {
            Ok(v) => v,
            Err(_) => return None,
        };

        let stats = vault.stats();
        let total_memories = stats.total_memcells
            + stats.total_events
            + stats.total_foresights
            + stats.total_episodes;
        let session_count = stats.total_memcells; // Approximate

        Some(format!(
            "\n\n--- Memory System ---\n\
             You have access to a persistent memory vault with {total_memories} memories \
             across {session_count} sessions.\n\
             Relevant memories are automatically injected before each message \
             (marked with \"--- Relevant memories ---\").\n\
             You can also manually search with mem_search for deeper recall.\n\
             After significant work, a memory entry is automatically created — \
             you don't need to call mem_write unless you want to save something specific."
        ))
    }

    // ─── Keyword Extraction Helpers ─────────────────────────────────

    /// Extract keywords from turn text using TF-IDF-like scoring.
    fn extract_keywords_tfidf(&self, turn: &TurnSummary) -> Vec<String> {
        let full_text = format!(
            "{} {} {}",
            turn.user_message,
            turn.tool_calls.join(" "),
            turn.response_preview
        );

        let tokens = tokenize(&full_text);
        // Count term frequencies
        let mut tf: std::collections::HashMap<String, usize> =
            std::collections::HashMap::new();
        for token in &tokens {
            *tf.entry(token.clone()).or_insert(0) += 1;
        }

        // Stop words to filter out
        const STOP_WORDS: &[&str] = &[
            "the", "a", "an", "is", "are", "was", "were", "be", "been", "being",
            "have", "has", "had", "do", "does", "did", "will", "would", "could",
            "should", "may", "might", "can", "shall", "to", "of", "in", "for",
            "on", "with", "at", "by", "from", "as", "into", "about", "it", "its",
            "this", "that", "these", "those", "i", "you", "he", "she", "we", "they",
            "me", "him", "her", "us", "them", "my", "your", "his", "our", "their",
            "and", "or", "but", "not", "no", "so", "if", "then", "than", "too",
            "very", "just", "also", "how", "what", "which", "who", "when", "where",
            "why", "all", "each", "every", "both", "few", "more", "most", "other",
            "some", "such", "only", "own", "same", "here", "there", "now",
        ];

        // Score and sort: prefer longer words and higher frequency
        let mut scored: Vec<(String, f64)> = tf
            .into_iter()
            .filter(|(word, _)| word.len() > 2)
            .filter(|(word, _)| !STOP_WORDS.contains(&word.as_str()))
            .map(|(word, count)| {
                // Simple scoring: tf * length_bonus
                let length_bonus = (word.len() as f64).ln().max(1.0);
                let score = count as f64 * length_bonus;
                (word, score)
            })
            .collect();

        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        scored.truncate(7);

        scored.into_iter().map(|(w, _)| w).collect()
    }

    /// Extract a topic from the turn summary.
    fn extract_topic(&self, turn: &TurnSummary) -> String {
        // Use first meaningful line of user message, truncated
        let first_line = turn
            .user_message
            .lines()
            .next()
            .unwrap_or(&turn.user_message);
        truncate_str(first_line, 80).to_string()
    }

    /// Extract context from the turn summary.
    fn extract_context(&self, turn: &TurnSummary) -> String {
        let mut parts = Vec::new();

        if !turn.user_message.is_empty() {
            parts.push(format!(
                "User asked: {}",
                truncate_str(&turn.user_message, 150)
            ));
        }

        if !turn.tool_calls.is_empty() {
            parts.push(format!(
                "Tools used: {}",
                turn.tool_calls.join(", ")
            ));
        }

        parts.join(". ")
    }
}

/// Simple tokenization: lowercase, split on non-alphanumeric, filter short tokens.
fn tokenize(text: &str) -> Vec<String> {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|s| s.len() > 2)
        .map(|s| s.to_string())
        .collect()
}

/// Truncate a string to max_len characters, adding "..." if truncated.
fn truncate_str(s: &str, max_len: usize) -> &str {
    if s.len() <= max_len {
        s
    } else {
        // Find a safe boundary to avoid splitting multi-byte chars
        let mut end = max_len;
        while end > 0 && !s.is_char_boundary(end) {
            end -= 1;
        }
        &s[..end]
    }
}

// ─── Tests ────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn setup_sidecar() -> (tempfile::TempDir, MemorySidecar) {
        let tmp = tempfile::tempdir().unwrap();
        let vault = ObsidianVault::open(tmp.path()).unwrap();
        let config = MemorySettings::default();
        (
            tmp,
            MemorySidecar::new(
                Arc::new(Mutex::new(vault)),
                config,
            ),
        )
    }

    #[test]
    fn test_sidecar_default_settings() {
        let (_, sidecar) = setup_sidecar();
        assert!(sidecar.auto_search_enabled());
        assert!(sidecar.auto_write_enabled());
        assert!(!sidecar.has_sidecar_model());
    }

    #[test]
    fn test_sidecar_with_model() {
        let tmp = tempfile::tempdir().unwrap();
        let vault = ObsidianVault::open(tmp.path()).unwrap();
        let config = MemorySettings {
            sidecar_model: Some("deepseek-chat".into()),
            sidecar_provider: Some("deepseek".into()),
            ..MemorySettings::default()
        };
        let sidecar = MemorySidecar::new(Arc::new(Mutex::new(vault)), config);
        assert!(sidecar.has_sidecar_model());
        assert_eq!(sidecar.sidecar_model(), Some("deepseek-chat"));
        assert_eq!(sidecar.sidecar_provider(), Some("deepseek"));
    }

    #[test]
    fn test_enrich_input_no_results() {
        let (_, sidecar) = setup_sidecar();
        // Empty vault should return original input
        let result = sidecar.enrich_input("hello world");
        assert_eq!(result, "hello world");
    }

    #[test]
    fn test_enrich_input_disabled() {
        let tmp = tempfile::tempdir().unwrap();
        let vault = ObsidianVault::open(tmp.path()).unwrap();
        let config = MemorySettings {
            auto_search: false,
            ..MemorySettings::default()
        };
        let sidecar = MemorySidecar::new(Arc::new(Mutex::new(vault)), config);
        let result = sidecar.enrich_input("hello world");
        assert_eq!(result, "hello world");
    }

    #[test]
    fn test_search_for_context_empty_vault() {
        let (_, sidecar) = setup_sidecar();
        let result = sidecar.search_for_context("test query");
        assert_eq!(result.count, 0);
        assert!(result.context_block.is_empty());
    }

    #[test]
    fn test_write_turn_memory() {
        let (_, sidecar) = setup_sidecar();
        let turn = TurnSummary {
            user_message: "Fix the auth middleware JWT validation bug".into(),
            tool_calls: vec!["file_read(path=src/auth.rs)".into()],
            response_preview: "Fixed the JWT validation by adding proper base64 decoding".into(),
            project: "momo-fetch".into(),
        };

        let result = sidecar.write_turn_memory_option_a(&turn).unwrap();
        assert!(result.contains("MemCell"));
    }

    #[test]
    fn test_write_turn_memory_empty_project() {
        let (_, sidecar) = setup_sidecar();
        let turn = TurnSummary {
            user_message: "Hello".into(),
            tool_calls: vec![],
            response_preview: "Hi there!".into(),
            project: String::new(),
        };

        let result = sidecar.write_turn_memory_option_a(&turn).unwrap();
        assert!(result.contains("MemCell"));
    }

    #[test]
    fn test_build_sidecar_prompt() {
        let (_, sidecar) = setup_sidecar();
        let turn = TurnSummary {
            user_message: "Fix auth bug".into(),
            tool_calls: vec!["file_read".into()],
            response_preview: "Fixed!".into(),
            project: "test".into(),
        };

        let prompt = sidecar.build_sidecar_prompt(&turn);
        assert!(prompt.contains("Fix auth bug"));
        assert!(prompt.contains("file_read"));
        assert!(prompt.contains("JSON"));
        assert!(prompt.contains("keywords"));
    }

    #[test]
    fn test_build_system_prompt_addition() {
        let (_, sidecar) = setup_sidecar();
        let addition = sidecar.build_system_prompt_addition();
        assert!(addition.is_some());
        let text = addition.unwrap();
        assert!(text.contains("Memory System"));
        assert!(text.contains("mem_search"));
    }

    #[test]
    fn test_build_system_prompt_disabled() {
        let tmp = tempfile::tempdir().unwrap();
        let vault = ObsidianVault::open(tmp.path()).unwrap();
        let config = MemorySettings {
            auto_search: false,
            auto_write: false,
            ..MemorySettings::default()
        };
        let sidecar = MemorySidecar::new(Arc::new(Mutex::new(vault)), config);
        assert!(sidecar.build_system_prompt_addition().is_none());
    }

    #[test]
    fn test_extract_keywords_tfidf() {
        let (_, sidecar) = setup_sidecar();
        let turn = TurnSummary {
            user_message: "Fix the Rust async middleware for authentication".into(),
            tool_calls: vec!["file_read(path=src/auth.rs)".into()],
            response_preview: "Updated the Rust async auth middleware with proper error handling".into(),
            project: "test".into(),
        };

        let keywords = sidecar.extract_keywords_tfidf(&turn);
        assert!(!keywords.is_empty());
        assert!(keywords.len() <= 7);
        // Should contain domain-specific words
        assert!(keywords.iter().any(|k| k == "rust" || k == "async" || k == "auth" || k == "middleware"));
    }

    #[test]
    fn test_extract_keywords_filters_stopwords() {
        let (_, sidecar) = setup_sidecar();
        let turn = TurnSummary {
            user_message: "this is a test of the system".into(),
            tool_calls: vec![],
            response_preview: "the system is working".into(),
            project: "test".into(),
        };

        let keywords = sidecar.extract_keywords_tfidf(&turn);
        // Stop words should be filtered
        assert!(!keywords.iter().any(|k| k == "the" || k == "is" || k == "a"));
        assert!(keywords.iter().any(|k| k == "test" || k == "system"));
    }

    #[test]
    fn test_truncate_str() {
        assert_eq!(truncate_str("hello", 10), "hello");
        assert_eq!(truncate_str("hello world", 5), "hello");
    }

    #[test]
    fn test_tokenize() {
        let tokens = tokenize("Fix the Rust async middleware!");
        assert!(tokens.contains(&"rust".to_string()));
        assert!(tokens.contains(&"async".to_string()));
        assert!(tokens.contains(&"middleware".to_string()));
        assert!(tokens.contains(&"the".to_string())); // "the" is 3 chars, passes length filter
        // Stop word filtering is done in extract_keywords_tfidf, not in tokenize
    }
}
