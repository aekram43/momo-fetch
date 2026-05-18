use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use adk_rust::UsageMetadata;

/// Pricing data per provider/model (USD per 1M tokens).
/// Sources: Anthropic docs, OpenAI docs, Groq docs, DeepSeek docs, etc.
struct PricingEntry {
    prompt_per_million: f64,
    completion_per_million: f64,
}

/// A single cost record persisted to disk.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CostRecord {
    pub session_id: String,
    pub timestamp: DateTime<Utc>,
    pub provider: String,
    pub model: String,
    #[serde(default)]
    pub project: String,
    pub prompt_tokens: i32,
    pub completion_tokens: i32,
    pub total_tokens: i32,
    pub estimated_cost: f64,
}

/// Aggregated cost summary for display.
#[derive(Debug, Clone, Default)]
pub struct CostSummary {
    pub total_cost: f64,
    pub total_prompt_tokens: i64,
    pub total_completion_tokens: i64,
    pub total_tokens: i64,
    pub request_count: usize,
}

impl std::fmt::Display for CostSummary {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "${:.4} ({} in + {} out tokens, {} requests)",
            self.total_cost,
            self.total_prompt_tokens,
            self.total_completion_tokens,
            self.request_count,
        )
    }
}

/// Tracks token usage and cost across sessions.
///
/// Stores cost records in a JSON file alongside the session DB.
/// Accumulates usage from `Event.usage_metadata` during streaming.
pub struct CostTracker {
    records: Mutex<Vec<CostRecord>>,
    /// Current turn accumulators (reset each turn)
    turn_prompt_tokens: Mutex<i32>,
    turn_completion_tokens: Mutex<i32>,
    /// Running totals for the current session
    session_total_cost: Mutex<f64>,
    session_prompt_tokens: Mutex<i64>,
    session_completion_tokens: Mutex<i64>,
    session_request_count: Mutex<usize>,
    current_session_id: Mutex<String>,
    current_provider: Mutex<String>,
    current_model: Mutex<String>,
    current_project: Mutex<String>,
    cost_file_path: PathBuf,
    /// Budget limits
    max_cost_daily: Option<f64>,
    max_cost_session: Option<f64>,
    /// Daily totals (date_str -> cost)
    daily_totals: Mutex<HashMap<String, f64>>,
}

impl CostTracker {
    /// Create a new cost tracker backed by a JSON file.
    pub fn new(cost_file_path: PathBuf) -> Self {
        let records = Self::load_records(&cost_file_path);
        let daily_totals = Self::compute_daily_totals(&records);
        Self {
            records: Mutex::new(records),
            turn_prompt_tokens: Mutex::new(0),
            turn_completion_tokens: Mutex::new(0),
            session_total_cost: Mutex::new(0.0),
            session_prompt_tokens: Mutex::new(0),
            session_completion_tokens: Mutex::new(0),
            session_request_count: Mutex::new(0),
            current_session_id: Mutex::new(String::new()),
            current_provider: Mutex::new(String::new()),
            current_model: Mutex::new(String::new()),
            current_project: Mutex::new(String::new()),
            cost_file_path,
            max_cost_daily: None,
            max_cost_session: None,
            daily_totals: Mutex::new(daily_totals),
        }
    }

    /// Set budget limits.
    #[allow(dead_code)]
    pub fn set_budget_limits(&mut self, daily: Option<f64>, session: Option<f64>) {
        self.max_cost_daily = daily;
        self.max_cost_session = session;
    }

    /// Set the current session context (called at session start / switch).
    /// Preserves the existing project name.
    pub fn set_session_context(&self, session_id: &str, provider: &str, model: &str) {
        let project = {
            let guard = self.current_project.lock().unwrap();
            guard.clone()
        }; // lock released here
        self.set_session_context_with_project(session_id, provider, model, &project);
    }

    /// Set the current session context with project name.
    pub fn set_session_context_with_project(
        &self,
        session_id: &str,
        provider: &str,
        model: &str,
        project: &str,
    ) {
        *self.current_session_id.lock().unwrap() = session_id.to_string();
        *self.current_provider.lock().unwrap() = provider.to_string();
        *self.current_model.lock().unwrap() = model.to_string();
        *self.current_project.lock().unwrap() = project.to_string();

        // Recompute session totals from records (release records lock before updating totals)
        let (total_cost, prompt_tokens, completion_tokens, count) = {
            let records = self.records.lock().unwrap();
            let sid = session_id.to_string();
            let mut total_cost = 0.0;
            let mut prompt_tokens: i64 = 0;
            let mut completion_tokens: i64 = 0;
            let mut count = 0;
            for r in records.iter().filter(|r| r.session_id == sid) {
                total_cost += r.estimated_cost;
                prompt_tokens += r.prompt_tokens as i64;
                completion_tokens += r.completion_tokens as i64;
                count += 1;
            }
            (total_cost, prompt_tokens, completion_tokens, count)
        }; // records lock released here

        *self.session_total_cost.lock().unwrap() = total_cost;
        *self.session_prompt_tokens.lock().unwrap() = prompt_tokens;
        *self.session_completion_tokens.lock().unwrap() = completion_tokens;
        *self.session_request_count.lock().unwrap() = count;
    }

    /// Reset turn accumulators (called at start of each turn).
    pub fn reset_turn(&self) {
        *self.turn_prompt_tokens.lock().unwrap() = 0;
        *self.turn_completion_tokens.lock().unwrap() = 0;
    }

    /// Accumulate usage from a stream event.
    /// Call this for each event that has usage_metadata.
    /// Returns the estimated cost if the event had usage data.
    pub fn record_event(&self, usage: &UsageMetadata) -> Option<f64> {
        // Only count non-partial, non-zero usage events to avoid double-counting
        // (streaming sends multiple partial events; the final one has the real totals)
        if usage.prompt_token_count == 0 && usage.candidates_token_count == 0 {
            return None;
        }

        // Accumulate for the turn
        let mut turn_prompt = self.turn_prompt_tokens.lock().unwrap();
        let mut turn_completion = self.turn_completion_tokens.lock().unwrap();
        *turn_prompt += usage.prompt_token_count;
        *turn_completion += usage.candidates_token_count;
        drop(turn_prompt);
        drop(turn_completion);

        // Estimate cost
        let provider = self.current_provider.lock().unwrap().clone();
        let model = self.current_model.lock().unwrap().clone();
        let cost = Self::estimate_cost(
            &provider,
            &model,
            usage.prompt_token_count,
            usage.candidates_token_count,
        );

        // Update session totals
        *self.session_total_cost.lock().unwrap() += cost;
        *self.session_prompt_tokens.lock().unwrap() += usage.prompt_token_count as i64;
        *self.session_completion_tokens.lock().unwrap() += usage.candidates_token_count as i64;

        Some(cost)
    }

    /// Finalize a turn: create a cost record and persist.
    /// Call this at the end of each REPL turn (after stream is consumed).
    pub fn finalize_turn(&self) {
        let turn_prompt = *self.turn_prompt_tokens.lock().unwrap();
        let turn_completion = *self.turn_completion_tokens.lock().unwrap();

        if turn_prompt == 0 && turn_completion == 0 {
            return; // No usage recorded this turn
        }

        let provider = self.current_provider.lock().unwrap().clone();
        let model = self.current_model.lock().unwrap().clone();
        let session_id = self.current_session_id.lock().unwrap().clone();
        let project = self.current_project.lock().unwrap().clone();
        let estimated_cost = Self::estimate_cost(&provider, &model, turn_prompt, turn_completion);
        let total_tokens = turn_prompt + turn_completion;

        // Increment request count
        *self.session_request_count.lock().unwrap() += 1;

        let record = CostRecord {
            session_id,
            timestamp: Utc::now(),
            provider,
            model,
            project,
            prompt_tokens: turn_prompt,
            completion_tokens: turn_completion,
            total_tokens,
            estimated_cost,
        };

        // Append to records
        {
            let mut records = self.records.lock().unwrap();
            records.push(record);
        }

        // Update daily total
        let date_str = Utc::now().format("%Y-%m-%d").to_string();
        {
            let mut daily = self.daily_totals.lock().unwrap();
            *daily.entry(date_str).or_insert(0.0) += estimated_cost;
        }

        // Persist to file
        self.save_records();

        // Reset turn accumulators
        self.reset_turn();
    }

    /// Get the current session cost summary.
    pub fn session_summary(&self) -> CostSummary {
        let total_cost = *self.session_total_cost.lock().unwrap();
        let prompt_tokens = *self.session_prompt_tokens.lock().unwrap();
        let completion_tokens = *self.session_completion_tokens.lock().unwrap();
        let request_count = *self.session_request_count.lock().unwrap();
        CostSummary {
            total_cost,
            total_prompt_tokens: prompt_tokens,
            total_completion_tokens: completion_tokens,
            total_tokens: prompt_tokens + completion_tokens,
            request_count,
        }
    }

    /// Get cost for today across all sessions.
    pub fn today_summary(&self) -> CostSummary {
        let date_str = Utc::now().format("%Y-%m-%d").to_string();
        self.date_range_summary(&date_str, &date_str)
    }

    /// Get cost for this week (last 7 days) across all sessions.
    pub fn week_summary(&self) -> CostSummary {
        let today = Utc::now().format("%Y-%m-%d").to_string();
        let week_ago = (Utc::now() - chrono::Duration::days(7)).format("%Y-%m-%d").to_string();
        self.date_range_summary(&week_ago, &today)
    }

    /// Get cost for a specific project across all sessions.
    pub fn project_summary(&self, project_name: &str) -> CostSummary {
        let records = self.records.lock().unwrap();
        let mut summary = CostSummary::default();
        for r in records.iter() {
            if r.project == project_name {
                summary.total_cost += r.estimated_cost;
                summary.total_prompt_tokens += r.prompt_tokens as i64;
                summary.total_completion_tokens += r.completion_tokens as i64;
                summary.request_count += 1;
            }
        }
        summary.total_tokens = summary.total_prompt_tokens + summary.total_completion_tokens;
        summary
    }

    /// Get the current project name.
    pub fn current_project(&self) -> String {
        self.current_project.lock().unwrap().clone()
    }

    /// List all unique project names from records.
    pub fn list_projects(&self) -> Vec<String> {
        let records = self.records.lock().unwrap();
        let mut projects: Vec<String> = records
            .iter()
            .filter(|r| !r.project.is_empty())
            .map(|r| r.project.clone())
            .collect();
        projects.sort();
        projects.dedup();
        projects
    }

    /// Get cost for a date range across all sessions.
    fn date_range_summary(&self, start_date: &str, end_date: &str) -> CostSummary {
        let records = self.records.lock().unwrap();
        let mut summary = CostSummary::default();
        for r in records.iter() {
            let record_date = r.timestamp.format("%Y-%m-%d").to_string();
            if record_date.as_str() >= start_date && record_date.as_str() <= end_date {
                summary.total_cost += r.estimated_cost;
                summary.total_prompt_tokens += r.prompt_tokens as i64;
                summary.total_completion_tokens += r.completion_tokens as i64;
                summary.request_count += 1;
            }
        }
        summary.total_tokens = summary.total_prompt_tokens + summary.total_completion_tokens;
        summary
    }

    /// Check if budget limits are exceeded.
    /// Returns (daily_exceeded, daily_limit, session_exceeded, session_limit).
    pub fn check_budget(&self) -> (bool, Option<f64>, bool, Option<f64>) {
        let daily_cost = self.today_summary().total_cost;
        let session_cost = *self.session_total_cost.lock().unwrap();

        let daily_exceeded = self
            .max_cost_daily
            .map(|limit| daily_cost >= limit)
            .unwrap_or(false);
        let session_exceeded = self
            .max_cost_session
            .map(|limit| session_cost >= limit)
            .unwrap_or(false);

        (
            daily_exceeded,
            self.max_cost_daily,
            session_exceeded,
            self.max_cost_session,
        )
    }

    /// Format a budget warning message if limits are approached or exceeded.
    pub fn budget_alert(&self) -> Option<String> {
        let (daily_exceeded, daily_limit, session_exceeded, session_limit) = self.check_budget();

        let mut warnings = Vec::new();
        if let Some(limit) = daily_limit {
            let daily_cost = self.today_summary().total_cost;
            let ratio = daily_cost / limit;
            if daily_exceeded {
                warnings.push(format!(
                    "Daily budget EXCEEDED: ${:.2} / ${:.2}",
                    daily_cost, limit
                ));
            } else if ratio > 0.8 {
                warnings.push(format!(
                    "Daily budget warning: ${:.2} / ${:.2} ({:.0}%)",
                    daily_cost,
                    limit,
                    ratio * 100.0
                ));
            }
        }
        if let Some(limit) = session_limit {
            let session_cost = *self.session_total_cost.lock().unwrap();
            let ratio = session_cost / limit;
            if session_exceeded {
                warnings.push(format!(
                    "Session budget EXCEEDED: ${:.2} / ${:.2}",
                    session_cost, limit
                ));
            } else if ratio > 0.8 {
                warnings.push(format!(
                    "Session budget warning: ${:.2} / ${:.2} ({:.0}%)",
                    session_cost,
                    limit,
                    ratio * 100.0
                ));
            }
        }

        if warnings.is_empty() {
            None
        } else {
            Some(warnings.join("\n"))
        }
    }

    /// Estimate cost for a given provider/model and token counts.
    fn estimate_cost(provider: &str, model: &str, prompt_tokens: i32, completion_tokens: i32) -> f64 {
        let pricing = Self::get_pricing(provider, model);
        let prompt_cost = (prompt_tokens as f64 / 1_000_000.0) * pricing.prompt_per_million;
        let completion_cost = (completion_tokens as f64 / 1_000_000.0) * pricing.completion_per_million;
        prompt_cost + completion_cost
    }

    /// Get pricing for a provider/model combination.
    fn get_pricing(provider: &str, model: &str) -> PricingEntry {
        match provider {
            "anthropic" => {
                // Claude pricing (2025)
                if model.contains("opus") {
                    PricingEntry {
                        prompt_per_million: 15.0,
                        completion_per_million: 75.0,
                    }
                } else if model.contains("haiku") {
                    PricingEntry {
                        prompt_per_million: 0.80,
                        completion_per_million: 4.0,
                    }
                } else {
                    // Default: Claude Sonnet
                    PricingEntry {
                        prompt_per_million: 3.0,
                        completion_per_million: 15.0,
                    }
                }
            }
            "openai" => {
                if model.contains("gpt-4o") || model.contains("gpt-4o-mini") {
                    PricingEntry {
                        prompt_per_million: 2.5,
                        completion_per_million: 10.0,
                    }
                } else if model.contains("o1") || model.contains("o3") {
                    PricingEntry {
                        prompt_per_million: 15.0,
                        completion_per_million: 60.0,
                    }
                } else {
                    PricingEntry {
                        prompt_per_million: 5.0,
                        completion_per_million: 15.0,
                    }
                }
            }
            "deepseek" => {
                if model.contains("reasoner") {
                    PricingEntry {
                        prompt_per_million: 0.55,
                        completion_per_million: 2.19,
                    }
                } else {
                    PricingEntry {
                        prompt_per_million: 0.27,
                        completion_per_million: 1.10,
                    }
                }
            }
            "groq" => PricingEntry {
                prompt_per_million: 0.05,
                completion_per_million: 0.08,
            },
            "ollama" => PricingEntry {
                prompt_per_million: 0.0,
                completion_per_million: 0.0,
            },
            _ => {
                // Custom / unknown providers — generic estimate
                PricingEntry {
                    prompt_per_million: 5.0,
                    completion_per_million: 15.0,
                }
            }
        }
    }

    /// Load cost records from JSON file.
    fn load_records(path: &Path) -> Vec<CostRecord> {
        if !path.exists() {
            return Vec::new();
        }
        let content = std::fs::read_to_string(path).unwrap_or_default();
        serde_json::from_str(&content).unwrap_or_default()
    }

    /// Save cost records to JSON file (atomic write).
    fn save_records(&self) {
        let records = self.records.lock().unwrap();
        let content = serde_json::to_string_pretty(&*records).unwrap_or_default();
        drop(records);

        // Atomic write: temp + rename
        if let Some(parent) = self.cost_file_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let tmp = self.cost_file_path.with_extension("cost.tmp");
        if std::fs::write(&tmp, &content).is_ok() {
            let _ = std::fs::rename(&tmp, &self.cost_file_path);
        }
    }

    /// Compute daily totals from records.
    fn compute_daily_totals(records: &[CostRecord]) -> HashMap<String, f64> {
        let mut totals = HashMap::new();
        for r in records {
            let date = r.timestamp.format("%Y-%m-%d").to_string();
            *totals.entry(date).or_insert(0.0) += r.estimated_cost;
        }
        totals
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn make_tracker() -> (TempDir, CostTracker) {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("cost.json");
        let tracker = CostTracker::new(path);
        (tmp, tracker)
    }

    fn make_usage(prompt: i32, completion: i32) -> UsageMetadata {
        UsageMetadata {
            prompt_token_count: prompt,
            candidates_token_count: completion,
            total_token_count: prompt + completion,
            ..Default::default()
        }
    }

    #[test]
    fn test_estimate_cost_anthropic() {
        // Sonnet: $3/M input, $15/M output
        let cost = CostTracker::estimate_cost("anthropic", "claude-sonnet-4-20250514", 1_000_000, 1_000_000);
        assert!((cost - 18.0).abs() < 0.01);
    }

    #[test]
    fn test_estimate_cost_anthropic_haiku() {
        let cost = CostTracker::estimate_cost("anthropic", "claude-haiku-4-5", 1_000_000, 1_000_000);
        assert!((cost - 4.8).abs() < 0.01);
    }

    #[test]
    fn test_estimate_cost_openai() {
        let cost = CostTracker::estimate_cost("openai", "gpt-4o", 1_000_000, 1_000_000);
        assert!((cost - 12.5).abs() < 0.01);
    }

    #[test]
    fn test_estimate_cost_deepseek() {
        let cost = CostTracker::estimate_cost("deepseek", "deepseek-chat", 1_000_000, 1_000_000);
        assert!((cost - 1.37).abs() < 0.01);
    }

    #[test]
    fn test_estimate_cost_groq() {
        let cost = CostTracker::estimate_cost("groq", "llama-3.1-70b", 1_000_000, 1_000_000);
        assert!((cost - 0.13).abs() < 0.01);
    }

    #[test]
    fn test_estimate_cost_ollama_free() {
        let cost = CostTracker::estimate_cost("ollama", "llama3.2", 1_000_000, 1_000_000);
        assert!((cost - 0.0).abs() < 0.01);
    }

    #[test]
    fn test_record_event_accumulates() {
        let (_tmp, tracker) = make_tracker();
        tracker.set_session_context("sess-1", "anthropic", "claude-sonnet-4-20250514");

        tracker.reset_turn();

        // Simulate streaming: partial events with small counts, then final with full
        let partial1 = make_usage(10, 5);
        let cost1 = tracker.record_event(&partial1).unwrap();

        // Partial events may report cumulative counts from the provider
        let final_event = make_usage(100, 50);
        let cost2 = tracker.record_event(&final_event).unwrap();

        assert!(cost1 > 0.0);
        assert!(cost2 > 0.0);

        // Session summary should include all recorded usage
        let summary = tracker.session_summary();
        assert!(summary.total_prompt_tokens > 0);
        assert!(summary.total_completion_tokens > 0);
        assert!(summary.total_cost > 0.0);
    }

    #[test]
    fn test_record_event_skips_zero() {
        let (_tmp, tracker) = make_tracker();
        let zero_usage = make_usage(0, 0);
        let result = tracker.record_event(&zero_usage);
        assert!(result.is_none());
    }

    #[test]
    fn test_finalize_turn_persists() {
        let (tmp, tracker) = make_tracker();
        tracker.set_session_context("sess-1", "anthropic", "claude-sonnet-4-20250514");

        tracker.reset_turn();
        let usage = make_usage(1000, 500);
        tracker.record_event(&usage);
        tracker.finalize_turn();

        // Reload and verify persistence
        let cost_file = tmp.path().join("cost.json");
        let records = CostTracker::load_records(&cost_file);
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].session_id, "sess-1");
        assert_eq!(records[0].provider, "anthropic");
        assert_eq!(records[0].prompt_tokens, 1000);
        assert_eq!(records[0].completion_tokens, 500);
        assert!(records[0].estimated_cost > 0.0);
    }

    #[test]
    fn test_session_summary() {
        let (tmp, tracker) = make_tracker();
        tracker.set_session_context("sess-1", "anthropic", "claude-sonnet-4-20250514");

        tracker.reset_turn();
        tracker.record_event(&make_usage(100, 50));
        tracker.finalize_turn();

        tracker.reset_turn();
        tracker.record_event(&make_usage(200, 100));
        tracker.finalize_turn();

        let summary = tracker.session_summary();
        assert_eq!(summary.total_prompt_tokens, 300);
        assert_eq!(summary.total_completion_tokens, 150);
        assert_eq!(summary.request_count, 2);
    }

    #[test]
    fn test_set_session_context_recomputes() {
        let (tmp, tracker) = make_tracker();

        // Add two records for different sessions with project
        tracker.set_session_context_with_project(
            "sess-a",
            "anthropic",
            "claude-sonnet-4-20250514",
            "project-a",
        );
        tracker.reset_turn();
        tracker.record_event(&make_usage(100, 50));
        tracker.finalize_turn();

        tracker.set_session_context_with_project(
            "sess-b",
            "anthropic",
            "claude-sonnet-4-20250514",
            "project-b",
        );
        tracker.reset_turn();
        tracker.record_event(&make_usage(200, 100));
        tracker.finalize_turn();

        // Switch back to sess-a
        tracker.set_session_context_with_project(
            "sess-a",
            "anthropic",
            "claude-sonnet-4-20250514",
            "project-a",
        );
        let summary = tracker.session_summary();
        assert_eq!(summary.total_prompt_tokens, 100);
        assert_eq!(summary.request_count, 1);
    }

    #[test]
    fn test_date_range_summary() {
        let (_tmp, tracker) = make_tracker();

        // Add records for two different dates
        tracker.set_session_context("sess-1", "anthropic", "claude-sonnet-4-20250514");
        tracker.reset_turn();
        tracker.record_event(&make_usage(1000, 500));
        tracker.finalize_turn();

        let today = Utc::now().format("%Y-%m-%d").to_string();
        let summary = tracker.today_summary();
        assert_eq!(summary.total_prompt_tokens, 1000);
        assert_eq!(summary.request_count, 1);

        // Query for a date range that excludes today
        let past = (Utc::now() - chrono::Duration::days(30)).format("%Y-%m-%d").to_string();
        let yesterday = (Utc::now() - chrono::Duration::days(1)).format("%Y-%m-%d").to_string();
        let empty = tracker.date_range_summary(&past, &yesterday);
        assert_eq!(empty.request_count, 0);
    }

    #[test]
    fn test_budget_check() {
        let (_tmp, mut tracker) = make_tracker();
        tracker.set_budget_limits(Some(1.0), Some(0.5));

        tracker.set_session_context("sess-1", "anthropic", "claude-sonnet-4-20250514");

        // No budget exceeded yet
        let (daily_ex, daily_lim, session_ex, session_lim) = tracker.check_budget();
        assert!(!daily_ex);
        assert_eq!(daily_lim, Some(1.0));
        assert!(!session_ex);
        assert_eq!(session_lim, Some(0.5));

        // Add some cost (Sonnet: $3/M in, $15/M out)
        // 100K prompt + 50K completion = $0.30 + $0.75 = $1.05
        tracker.reset_turn();
        tracker.record_event(&make_usage(100_000, 50_000));
        tracker.finalize_turn();

        let (daily_ex, _, session_ex, _) = tracker.check_budget();
        assert!(daily_ex); // $1.05 >= $1.0
        assert!(session_ex); // $1.05 >= $0.5
    }

    #[test]
    fn test_budget_alert() {
        let (_tmp, mut tracker) = make_tracker();
        tracker.set_budget_limits(Some(1.0), Some(0.5));
        tracker.set_session_context("sess-1", "anthropic", "claude-sonnet-4-20250514");

        // No alert when under budget
        assert!(tracker.budget_alert().is_none());

        // Add cost approaching limit
        tracker.reset_turn();
        tracker.record_event(&make_usage(80_000, 20_000));
        tracker.finalize_turn();
        // $0.24 + $0.30 = $0.54, ratio = 0.54/0.50 = 108% for session
        let alert = tracker.budget_alert().unwrap();
        assert!(alert.contains("Session budget EXCEEDED") || alert.contains("Session budget warning"));
    }

    #[test]
    fn test_persistence_round_trip() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("cost.json");

        // Write a record directly
        let record = CostRecord {
            session_id: "persist-sess".to_string(),
            timestamp: Utc::now(),
            provider: "openai".to_string(),
            model: "gpt-4o".to_string(),
            project: String::new(),
            prompt_tokens: 500,
            completion_tokens: 250,
            total_tokens: 750,
            estimated_cost: 0.00875, // $2.5/M * 500/1M + $10/M * 250/1M
        };
        std::fs::write(&path, serde_json::to_string_pretty(&vec![record]).unwrap()).unwrap();

        // Reload and verify
        let tracker = CostTracker::new(path);
        tracker.set_session_context_with_project(
            "persist-sess",
            "openai",
            "gpt-4o",
            "",
        );
        let summary = tracker.session_summary();
        assert_eq!(summary.total_prompt_tokens, 500);
        assert_eq!(summary.total_completion_tokens, 250);
        assert!(summary.total_cost > 0.0);
    }

    #[test]
    fn test_cost_summary_display() {
        let summary = CostSummary {
            total_cost: 0.0234,
            total_prompt_tokens: 1240,
            total_completion_tokens: 890,
            total_tokens: 2130,
            request_count: 3,
        };
        let display = format!("{summary}");
        assert!(display.contains("$0.0234"));
        assert!(display.contains("1240 in"));
        assert!(display.contains("890 out"));
        assert!(display.contains("3 requests"));
    }

    #[test]
    fn test_finalize_turn_no_usage() {
        let (_tmp, tracker) = make_tracker();
        tracker.set_session_context("sess-1", "anthropic", "claude-sonnet-4-20250514");
        tracker.reset_turn();
        // Don't record any events
        tracker.finalize_turn();
        // Should not have created any records
        let cost_file = tracker.cost_file_path.clone();
        let records = CostTracker::load_records(&cost_file);
        assert!(records.is_empty());
    }

    #[test]
    fn test_empty_file_load() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("nonexistent.json");
        let records = CostTracker::load_records(&path);
        assert!(records.is_empty());
    }

    #[test]
    fn test_project_summary() {
        let (_tmp, tracker) = make_tracker();
        tracker.set_session_context_with_project(
            "sess-1",
            "anthropic",
            "claude-sonnet-4-20250514",
            "my-project",
        );

        tracker.reset_turn();
        tracker.record_event(&make_usage(1000, 500));
        tracker.finalize_turn();

        // Switch to another project
        tracker.set_session_context_with_project(
            "sess-2",
            "anthropic",
            "claude-sonnet-4-20250514",
            "other-project",
        );
        tracker.reset_turn();
        tracker.record_event(&make_usage(500, 250));
        tracker.finalize_turn();

        // Check project summaries
        let proj1 = tracker.project_summary("my-project");
        assert_eq!(proj1.request_count, 1);
        assert!(proj1.total_cost > 0.0);

        let proj2 = tracker.project_summary("other-project");
        assert_eq!(proj2.request_count, 1);

        let unknown = tracker.project_summary("nonexistent");
        assert_eq!(unknown.request_count, 0);
        assert_eq!(unknown.total_cost, 0.0);
    }

    #[test]
    fn test_list_projects() {
        let (_tmp, tracker) = make_tracker();

        // No projects initially
        assert!(tracker.list_projects().is_empty());

        tracker.set_session_context_with_project(
            "sess-1",
            "anthropic",
            "claude-sonnet-4-20250514",
            "project-a",
        );
        tracker.reset_turn();
        tracker.record_event(&make_usage(100, 50));
        tracker.finalize_turn();

        tracker.set_session_context_with_project(
            "sess-2",
            "openai",
            "gpt-4o",
            "project-b",
        );
        tracker.reset_turn();
        tracker.record_event(&make_usage(200, 100));
        tracker.finalize_turn();

        let projects = tracker.list_projects();
        assert_eq!(projects.len(), 2);
        assert!(projects.contains(&"project-a".to_string()));
        assert!(projects.contains(&"project-b".to_string()));
    }

    #[test]
    fn test_set_session_context_preserves_project() {
        let (_tmp, tracker) = make_tracker();
        tracker.set_session_context_with_project(
            "sess-1",
            "anthropic",
            "claude-sonnet-4-20250514",
            "my-project",
        );
        assert_eq!(tracker.current_project(), "my-project");

        // switch_model preserves project
        tracker.set_session_context("sess-1", "openai", "gpt-4o");
        assert_eq!(tracker.current_project(), "my-project");
    }

    #[test]
    fn test_backward_compat_no_project_field() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("cost.json");

        // Write a cost record without the project field (old format)
        let old_format = r#"[{
            "session_id": "old-sess",
            "timestamp": "2026-05-07T00:00:00Z",
            "provider": "anthropic",
            "model": "claude-sonnet-4-20250514",
            "prompt_tokens": 1000,
            "completion_tokens": 500,
            "total_tokens": 1500,
            "estimated_cost": 0.0105
        }]"#;
        std::fs::write(&path, old_format).unwrap();

        // Load and verify it parses with default empty project
        let tracker = CostTracker::new(path);
        let records = tracker.records.lock().unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].project, "");
    }
}
