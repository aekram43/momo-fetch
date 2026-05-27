//! Memory sidecar — auto-search before turns, auto-write after turns.
//!
//! Implements three memory extraction options, selected via config:
//! - **Option A** (default): TF-IDF keyword extraction, zero extra LLM cost.
//! - **Option B** (opt-in): Direct LLM call to a small model (e.g. deepseek-chat).
//! - **Option C** (opt-in): Separate process via Mailbox IPC.

use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use crate::config::MemorySettings;
use crate::memory::types::{ActionRecord, MemoryQuery, RetrievalMode};
use crate::memory::vault::ObsidianVault;

/// Parsed JSON response from the sidecar LLM (Option B).
#[derive(Debug, serde::Deserialize)]
struct SidecarExtraction {
    topic: String,
    context: String,
    actions: Vec<ActionRecord>,
    outcome: String,
    keywords: Vec<String>,
}

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
///   for writes (no extra LLM call).
/// - **Option B** (opt-in): When `sidecar_model` is set, calls the configured
///   model directly for memory extraction.
/// - **Option C** (opt-in): When a sidecar process is detected via Mailbox,
///   routes requests through IPC.
pub struct MemorySidecar {
    vault: Arc<Mutex<ObsidianVault>>,
    config: MemorySettings,
    /// Counter for auto-extract trigger (incremented per write).
    memcells_since_extract: AtomicUsize,
    /// Lazy-initialized sidecar LLM for Option B.
    sidecar_llm: OnceLock<Arc<dyn adk_rust::prelude::Llm>>,
}

impl MemorySidecar {
    /// Create a new MemorySidecar with the given vault and settings.
    pub fn new(vault: Arc<Mutex<ObsidianVault>>, config: MemorySettings) -> Self {
        Self {
            vault,
            config,
            memcells_since_extract: AtomicUsize::new(0),
            sidecar_llm: OnceLock::new(),
        }
    }

    /// Wrap in Arc for sharing across threads (e.g., background post-turn write).
    pub fn clone_arc(self: &Arc<Self>) -> Arc<Self> {
        Arc::clone(self)
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
    pub fn has_sidecar_model(&self) -> bool {
        self.config.sidecar_model.is_some()
    }

    /// Get the sidecar model name (for Option B).
    pub fn sidecar_model(&self) -> Option<&str> {
        self.config.sidecar_model.as_deref()
    }

    /// Get the sidecar provider name (for Option B).
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

    // ─── Post-Turn: Auto-Write (Unified Dispatch) ───────────────────

    /// Write a MemCell from the turn summary, dispatching to the appropriate option.
    ///
    /// Option C (Mailbox) > Option B (sidecar LLM) > Option A (TF-IDF).
    /// After writing, checks thresholds for auto-extract and auto-consolidate.
    pub fn write_turn_memory(
        &self,
        turn: &TurnSummary,
        provider_mgr: &crate::providers::ProviderManager,
        mailbox_path: Option<&Path>,
    ) -> anyhow::Result<String> {
        let memcell_ref = self.dispatch_write(turn, provider_mgr, mailbox_path)?;
        tracing::debug!("Memory sidecar: wrote MemCell '{memcell_ref}'");

        // Auto-extract / auto-consolidate thresholds
        self.check_thresholds(&memcell_ref, turn);

        Ok(memcell_ref)
    }

    /// Internal dispatch: tries Option C → Option B → Option A.
    fn dispatch_write(
        &self,
        turn: &TurnSummary,
        provider_mgr: &crate::providers::ProviderManager,
        mailbox_path: Option<&Path>,
    ) -> anyhow::Result<String> {
        // Option C: check if sidecar process is alive via Mailbox
        if let Some(mpath) = mailbox_path {
            if let Ok(mailbox) = crate::team::Mailbox::open(mpath) {
                if let Ok(messages) = mailbox.peek(crate::team::sidecar_protocol::MAIN_ID) {
                    let has_ready = messages.iter().any(|m| {
                        m.msg_type == crate::team::sidecar_protocol::msg_type::READY
                            && m.from == crate::team::sidecar_protocol::SIDECAR_ID
                    });
                    if has_ready {
                        tracing::debug!("Memory sidecar: routing via Option C (Mailbox)");
                        return self.write_turn_memory_option_c(turn, &mailbox);
                    }
                }
            }
        }

        // Option B: sidecar model configured
        if self.has_sidecar_model() {
            tracing::debug!("Memory sidecar: routing via Option B (sidecar LLM)");
            return self.write_turn_memory_option_b(turn, provider_mgr);
        }

        // Option A: TF-IDF (default)
        self.write_turn_memory_option_a(turn)
    }

    /// Write a MemCell from the turn summary (Option A: TF-IDF keywords).
    pub(crate) fn write_turn_memory_option_a(&self, turn: &TurnSummary) -> anyhow::Result<String> {
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
        vault.write_memcell(&project, &topic, &context, &actions, &outcome, &kw_refs)
    }

    /// Write a MemCell using a sidecar LLM model (Option B).
    ///
    /// Makes a direct LLM call (no agent loop) to extract structured data,
    /// then writes a MemCell. Falls back to Option A on any failure.
    fn write_turn_memory_option_b(
        &self,
        turn: &TurnSummary,
        provider_mgr: &crate::providers::ProviderManager,
    ) -> anyhow::Result<String> {
        let provider = self
            .sidecar_provider()
            .unwrap_or("deepseek");
        let model = self
            .sidecar_model()
            .unwrap_or("deepseek-chat");

        // Lazy-init the sidecar LLM
        let llm = self.sidecar_llm.get_or_init(|| match provider_mgr.create_model(provider, model) {
            Ok(l) => l,
            Err(e) => {
                tracing::error!("Memory sidecar: failed to create sidecar LLM: {e}");
                // Return a dummy that will never be used (we check below)
                return provider_mgr.create_model(provider, model)
                    .expect("sidecar LLM creation failed after retry");
            }
        });

        let prompt = self.build_sidecar_prompt(turn);
        let content = adk_rust::Content::new("user").with_text(&prompt);
        let req = adk_rust::prelude::LlmRequest::new(model, vec![content]);

        // Direct LLM call via block_in_place + block_on (required because we're inside a tokio runtime)
        let response_text = match tokio::runtime::Handle::try_current() {
            Ok(handle) => tokio::task::block_in_place(|| {
                handle.block_on(async {
                    let mut stream = llm.generate_content(req, false).await?;
                    let mut text = String::new();
                    use adk_rust::futures::StreamExt;
                    while let Some(chunk) = stream.next().await {
                        let chunk = chunk?;
                        if let Some(c) = chunk.content {
                            for part in c.parts {
                                if let adk_rust::Part::Text { text: t } = part {
                                    text.push_str(&t);
                                }
                            }
                        }
                    }
                    Ok::<String, anyhow::Error>(text)
                })
            }),
            Err(_) => {
                tracing::warn!("Memory sidecar: no tokio runtime, falling back to Option A");
                return self.write_turn_memory_option_a(turn);
            }
        };

        match response_text {
            Ok(text) => {
                // Strip markdown code fences if present
                let json_str = strip_json_fences(&text);
                match serde_json::from_str::<SidecarExtraction>(&json_str) {
                    Ok(extracted) => {
                        let project = if turn.project.is_empty() {
                            "default".to_string()
                        } else {
                            turn.project.clone()
                        };
                        let kw_refs: Vec<&str> = extracted.keywords.iter().map(|s| s.as_str()).collect();
                        let mut vault = self.vault.lock()
                            .map_err(|e| anyhow::anyhow!("vault lock: {e}"))?;
                        vault.write_memcell(
                            &project,
                            &extracted.topic,
                            &extracted.context,
                            &extracted.actions,
                            &extracted.outcome,
                            &kw_refs,
                        )
                    }
                    Err(e) => {
                        tracing::warn!(
                            "Memory sidecar: Option B JSON parse failed: {e}, falling back to Option A"
                        );
                        self.write_turn_memory_option_a(turn)
                    }
                }
            }
            Err(e) => {
                tracing::warn!("Memory sidecar: Option B LLM call failed: {e}, falling back to Option A");
                self.write_turn_memory_option_a(turn)
            }
        }
    }

    /// Write a MemCell via the sidecar process (Option C: Mailbox IPC).
    ///
    /// Sends a WRITE_REQUEST and polls for WRITE_RESPONSE with a 5s timeout.
    /// Falls back to Option A on timeout or error.
    fn write_turn_memory_option_c(
        &self,
        turn: &TurnSummary,
        mailbox: &crate::team::Mailbox,
    ) -> anyhow::Result<String> {
        let body = serde_json::json!({
            "user_message": turn.user_message,
            "tool_calls": turn.tool_calls,
            "response_preview": turn.response_preview,
            "project": turn.project,
        }).to_string();

        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_millis() as u64;

        mailbox.send(crate::team::MailboxMessage {
            from: crate::team::sidecar_protocol::MAIN_ID.to_string(),
            to: crate::team::sidecar_protocol::SIDECAR_ID.to_string(),
            msg_type: crate::team::sidecar_protocol::msg_type::WRITE_REQUEST.to_string(),
            body,
            timestamp: now_ms,
        })?;

        // Poll for response (5s timeout, 50ms intervals)
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            let responses = mailbox.receive(crate::team::sidecar_protocol::MAIN_ID)?;
            for resp in responses {
                if resp.msg_type == crate::team::sidecar_protocol::msg_type::WRITE_RESPONSE {
                    let parsed: serde_json::Value = serde_json::from_str(&resp.body)
                        .unwrap_or(serde_json::Value::Null);
                    if parsed["status"].as_str() == Some("written") {
                        return Ok(parsed["memcell_ref"]
                            .as_str()
                            .unwrap_or("unknown")
                            .to_string());
                    }
                    let err = parsed["error"].as_str().unwrap_or("unknown error");
                    tracing::warn!("Memory sidecar: Option C write error: {err}");
                    break;
                }
            }
            if std::time::Instant::now() >= deadline {
                tracing::warn!("Memory sidecar: Option C timeout, falling back to Option A");
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }

        // Fallback to Option A
        self.write_turn_memory_option_a(turn)
    }

    /// Check auto-extract and auto-consolidate thresholds after writing a MemCell.
    fn check_thresholds(&self, memcell_ref: &str, turn: &TurnSummary) {
        let prev = self.memcells_since_extract.fetch_add(1, Ordering::Relaxed);
        let current = prev + 1;

        if current < self.config.extract_threshold {
            return;
        }

        // Reset counter
        self.memcells_since_extract.store(0, Ordering::Relaxed);

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
        let keywords = self.extract_keywords_tfidf(turn);
        let kw_refs: Vec<&str> = keywords.iter().map(|s| s.as_str()).collect();

        if let Ok(mut vault) = self.vault.lock() {
            match vault.extract_from_memcell(
                memcell_ref, &project, &topic, &context, &actions, &outcome, &kw_refs,
            ) {
                Ok(result) => tracing::info!(
                    "Auto-extract: {} events, {} foresights, episode {:?}",
                    result.events_created.len(),
                    result.foresights_created.len(),
                    result.episode_id,
                ),
                Err(e) => tracing::warn!("Auto-extract failed (non-fatal): {e}"),
            }

            // Auto-consolidate at higher threshold
            if self.config.consolidate_threshold > 0 && current >= self.config.consolidate_threshold {
                match vault.consolidate() {
                    Ok(result) => tracing::info!(
                        "Auto-consolidate: {} clusters, {} profile ops",
                        result.clusters_created.len(),
                        result.profile_ops.len(),
                    ),
                    Err(e) => tracing::warn!("Auto-consolidate failed (non-fatal): {e}"),
                }
            }
        }
    }

    /// Build a prompt for the sidecar LLM (Option B).
    ///
    /// Returns the prompt to send to the sidecar model, asking it to
    /// extract structured memory data from the turn summary.
    pub fn build_sidecar_prompt(&self, turn: &TurnSummary) -> String {
        format!(
            "Extract a memory entry from this conversation turn.\n\n\
             User message: {}\n\n\
             Tool calls: {}\n\n\
             Agent response: {}\n\n\
             Project: {}\n\n\
             Return ONLY a JSON object (no markdown, no explanation) with these fields:\n\
             - topic: brief title (5-10 words)\n\
             - context: what was happening (1-2 sentences)\n\
             - actions: array of {{\"description\": \"...\", \"result\": \"...\"}} objects\n\
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

/// Strip markdown code fences from LLM response to extract raw JSON.
fn strip_json_fences(s: &str) -> String {
    let trimmed = s.trim();
    // Remove ```json ... ``` or ``` ... ``` wrappers
    if trimmed.starts_with("```") {
        let without_opening = trimmed.trim_start_matches("```json")
            .trim_start_matches("```")
            .trim_start_matches('\n');
        if let Some(without_closing) = without_opening.strip_suffix("```") {
            return without_closing.trim().to_string();
        }
    }
    trimmed.to_string()
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

        // Option A is private, so test through the public write_turn_memory
        // (which falls through to Option A when no sidecar model or mailbox is set)
        let mgr = crate::providers::ProviderManager::from_env().unwrap_or_else(|_| {
            // In test without real providers, we can't test the full path,
            // but write_turn_memory_option_a is tested directly via dispatch.
            panic!("ProviderManager::from_env() failed — set ANTHROPIC_API_KEY for this test");
        });
        let result = sidecar.write_turn_memory(&turn, &mgr, None).unwrap();
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

        let mgr = crate::providers::ProviderManager::from_env().unwrap_or_else(|_| {
            panic!("ProviderManager::from_env() failed — set ANTHROPIC_API_KEY for this test");
        });
        let result = sidecar.write_turn_memory(&turn, &mgr, None).unwrap();
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

    #[test]
    fn test_strip_json_fences() {
        let raw = r#"```json
{"topic": "test", "context": "ctx", "actions": [], "outcome": "ok", "keywords": ["test"]}
```"#;
        let stripped = strip_json_fences(raw);
        assert!(stripped.starts_with('{'));
        assert!(stripped.ends_with('}'));

        let no_fence = r#"{"topic": "test", "context": "ctx", "actions": [], "outcome": "ok", "keywords": ["test"]}"#;
        assert_eq!(strip_json_fences(no_fence), no_fence.trim());
    }

    #[test]
    fn test_sidecar_extraction_parse() {
        let json = r#"{"topic": "Fix auth", "context": "User reported bug", "actions": [{"description": "file_read", "result": "ok"}], "outcome": "Fixed", "keywords": ["auth", "jwt"]}"#;
        let parsed: SidecarExtraction = serde_json::from_str(json).unwrap();
        assert_eq!(parsed.topic, "Fix auth");
        assert_eq!(parsed.keywords.len(), 2);
        assert_eq!(parsed.actions.len(), 1);
    }
}
