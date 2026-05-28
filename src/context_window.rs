//! Context window awareness: maps models to their token limits
//! and tracks usage from API-reported prompt_token_count.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use colored::Colorize;

/// Granular per-model context window sizes (in tokens).
///
/// Model name matching uses contains/starts_with to handle version suffixes
/// (e.g. "claude-sonnet-4" matches "claude-sonnet-4-20250514").
fn model_context_window(provider: &str, model: &str) -> Option<u64> {
    match provider {
        "anthropic" => {
            // All Claude 3/3.5/4 models share 200K context
            Some(200_000)
        }
        "openai" => {
            if model.starts_with("gpt-4.1") {
                Some(1_047_576)
            } else if model.starts_with("o3-pro") || model.starts_with("o3") {
                Some(200_000)
            } else if model.starts_with("o1-mini") {
                Some(128_000)
            } else if model.starts_with("o1") {
                Some(200_000)
            } else if model.starts_with("gpt-4o-mini") {
                Some(128_000)
            } else if model.starts_with("gpt-4o") {
                Some(128_000)
            } else if model.starts_with("gpt-4-turbo") {
                Some(128_000)
            } else if model.starts_with("gpt-4-32k") {
                Some(32_768)
            } else if model.starts_with("gpt-4") {
                Some(8_192)
            } else if model.starts_with("gpt-3.5-turbo") || model.starts_with("gpt-35-turbo") {
                Some(16_385)
            } else {
                Some(128_000) // default for unknown OpenAI models
            }
        }
        "deepseek" => {
            if model.contains("reasoner") || model.contains("r1") {
                Some(64_000)
            } else {
                Some(128_000) // deepseek-chat, deepseek-coder
            }
        }
        "groq" => {
            if model.contains("llama-3.3") || model.contains("llama-3.1") || model.contains("llama3.1") {
                Some(128_000)
            } else if model.contains("llama3.2") || model.contains("llama-3.2") {
                Some(128_000)
            } else if model.contains("llama3") || model.contains("llama-3") {
                Some(8_192)
            } else if model.contains("mixtral") {
                Some(32_000)
            } else if model.contains("gemma2") {
                Some(8_192)
            } else if model.contains("qwen") {
                Some(128_000)
            } else {
                Some(128_000)
            }
        }
        "ollama" => {
            if model.contains("llama3.1") || model.contains("llama-3.1") {
                Some(128_000)
            } else if model.contains("llama3.2") || model.contains("llama-3.2") {
                Some(128_000)
            } else if model.contains("llama3") || model.contains("llama-3") {
                Some(8_192)
            } else if model.contains("codellama") {
                Some(16_384)
            } else if model.contains("mistral") {
                Some(32_000)
            } else if model.contains("mixtral") {
                Some(32_000)
            } else if model.contains("qwen2.5") {
                Some(128_000)
            } else if model.contains("deepseek-r1") || model.contains("deepseek-coder-v2") {
                Some(128_000)
            } else if model.contains("gemma2") {
                Some(8_192)
            } else if model.contains("phi3") {
                Some(128_000)
            } else {
                Some(128_000)
            }
        }
        "openrouter" => {
            // OpenRouter uses "provider/model" format; dynamic fetch is primary.
            // Fallback for common routed models:
            if model.contains("anthropic/") {
                Some(200_000)
            } else if model.contains("openai/gpt-4.1") {
                Some(1_047_576)
            } else if model.contains("openai/o1-mini") {
                Some(128_000)
            } else if model.contains("openai/o1") || model.contains("openai/o3") {
                Some(200_000)
            } else if model.contains("openai/gpt-4o") {
                Some(128_000)
            } else if model.contains("deepseek/") {
                Some(128_000)
            } else if model.contains("meta-llama/") {
                Some(128_000)
            } else {
                Some(128_000)
            }
        }
        "zai" => {
            if model.contains("4.7") || model.contains("4-plus") {
                Some(1_000_000)
            } else {
                Some(128_000)
            }
        }
        "custom" => Some(128_000),
        _ => None,
    }
}

/// Legacy lookup: provider + model → context window size.
/// Uses the granular model-level mapping table.
#[allow(dead_code)]
pub fn context_window_size(provider: &str, model: &str) -> Option<u64> {
    model_context_window(provider, model)
}

// ── Dynamic cache ───────────────────────────────────────────────────

/// Cache TTL: 1 hour. Context window sizes rarely change.
const CACHE_TTL: Duration = Duration::from_secs(3600);

/// Cache for dynamically fetched context window sizes from provider APIs.
pub struct ContextWindowCache {
    entries: Mutex<HashMap<String, u64>>,
    fetched_at: Mutex<Option<Instant>>,
}

impl ContextWindowCache {
    pub fn new() -> Self {
        Self {
            entries: Mutex::new(HashMap::new()),
            fetched_at: Mutex::new(None),
        }
    }

    /// Check if the cache is stale (older than TTL or never populated).
    pub fn is_stale(&self) -> bool {
        let fetched_at = self.fetched_at.lock().unwrap();
        match *fetched_at {
            None => true,
            Some(when) => when.elapsed() > CACHE_TTL,
        }
    }

    /// Look up a model in the cache.
    pub fn get(&self, model: &str) -> Option<u64> {
        let entries = self.entries.lock().unwrap();
        entries.get(model).copied()
    }

    /// Populate the cache from a batch of model entries.
    pub fn populate(&self, models: Vec<(String, u64)>) {
        let mut entries = self.entries.lock().unwrap();
        for (name, ctx_len) in models {
            entries.insert(name, ctx_len);
        }
        let mut fetched_at = self.fetched_at.lock().unwrap();
        *fetched_at = Some(Instant::now());
    }

    /// Clear the cache (e.g. on provider switch).
    #[allow(dead_code)]
    pub fn invalidate(&self) {
        let mut entries = self.entries.lock().unwrap();
        entries.clear();
        let mut fetched_at = self.fetched_at.lock().unwrap();
        *fetched_at = None;
    }
}

// ── 3-layer resolution ──────────────────────────────────────────────

/// Resolve context window size with 3-layer priority:
/// 1. Settings override (`provider:model` > `model`)
/// 2. Dynamic API cache
/// 3. Model-level static mapping table
pub fn resolve_context_window_size(
    provider: &str,
    model: &str,
    overrides: Option<&HashMap<String, u64>>,
    cache: Option<&ContextWindowCache>,
) -> Option<u64> {
    // Layer 1: Settings override
    if let Some(ov) = overrides {
        let qualified = format!("{}:{}", provider, model);
        if let Some(&size) = ov.get(&qualified) {
            return Some(size);
        }
        if let Some(&size) = ov.get(model) {
            return Some(size);
        }
    }

    // Layer 2: Dynamic cache
    if let Some(c) = cache {
        if let Some(size) = c.get(model) {
            return Some(size);
        }
    }

    // Layer 3: Model-level static mapping (with provider fallback)
    model_context_window(provider, model)
}

/// Current context window usage snapshot.
pub struct ContextUsage {
    pub prompt_tokens: i64,
    pub context_window: Option<u64>,
}

impl ContextUsage {
    /// Create with full 3-layer resolution.
    pub fn new_resolved(
        prompt_tokens: i64,
        provider: &str,
        model: &str,
        overrides: Option<&HashMap<String, u64>>,
        cache: Option<&ContextWindowCache>,
    ) -> Self {
        Self {
            prompt_tokens,
            context_window: resolve_context_window_size(provider, model, overrides, cache),
        }
    }

    /// Legacy constructor (static mapping only, no cache or overrides).
    #[allow(dead_code)]
    pub fn new(prompt_tokens: i64, provider: &str, model: &str) -> Self {
        Self {
            prompt_tokens,
            context_window: context_window_size(provider, model),
        }
    }

    /// Percentage of context window used (0.0–1.0). None if window size unknown.
    pub fn percentage(&self) -> Option<f64> {
        self.context_window
            .map(|w| self.prompt_tokens as f64 / w as f64)
    }

    /// Warning level based on usage percentage.
    pub fn warning_level(&self) -> ContextWarning {
        match self.percentage() {
            Some(p) if p >= 0.95 => ContextWarning::Critical,
            Some(p) if p >= 0.80 => ContextWarning::Approaching,
            Some(_) => ContextWarning::None,
            None => ContextWarning::Unknown,
        }
    }

    /// Human-readable status like "12.4K/200K (6.2%)".
    pub fn format_status(&self) -> String {
        match self.context_window {
            Some(window) => {
                let pct = self.percentage().unwrap_or(0.0) * 100.0;
                format!(
                    "{}/{} ({:.1}%)",
                    format_tokens(self.prompt_tokens),
                    format_tokens(window as i64),
                    pct
                )
            }
            None => format!("{} (window unknown)", format_tokens(self.prompt_tokens)),
        }
    }

    /// Colored warning string if a threshold is crossed.
    pub fn format_warning(&self) -> Option<String> {
        match self.warning_level() {
            ContextWarning::Critical => {
                let _window = self.context_window.unwrap();
                Some(format!(
                    "{} Context CRITICAL: {} — responses may be truncated. Start a new session.",
                    "\u{26a0}".red().bold(),
                    self.format_status().red(),
                ))
            }
            ContextWarning::Approaching => Some(format!(
                "{} Context: {} — consider starting a new session",
                "\u{26a0}".yellow(),
                self.format_status().yellow(),
            )),
            ContextWarning::None | ContextWarning::Unknown => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContextWarning {
    None,
    Approaching,
    Critical,
    Unknown,
}

/// Format a token count as human-readable: "12.4K" or "1.2M".
fn format_tokens(count: i64) -> String {
    if count >= 1_000_000 {
        format!("{:.1}M", count as f64 / 1_000_000.0)
    } else if count >= 1_000 {
        format!("{:.1}K", count as f64 / 1_000.0)
    } else {
        count.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_anthropic_context_window() {
        assert_eq!(context_window_size("anthropic", "claude-sonnet-4-20250514"), Some(200_000));
        assert_eq!(context_window_size("anthropic", "claude-opus-4"), Some(200_000));
        assert_eq!(context_window_size("anthropic", "claude-3-5-sonnet-latest"), Some(200_000));
        assert_eq!(context_window_size("anthropic", "claude-3-haiku-20240307"), Some(200_000));
    }

    #[test]
    fn test_openai_context_window() {
        assert_eq!(context_window_size("openai", "gpt-4o"), Some(128_000));
        assert_eq!(context_window_size("openai", "gpt-4o-mini"), Some(128_000));
        assert_eq!(context_window_size("openai", "gpt-4.1"), Some(1_047_576));
        assert_eq!(context_window_size("openai", "gpt-4.1-mini"), Some(1_047_576));
        assert_eq!(context_window_size("openai", "o1-preview"), Some(200_000));
        assert_eq!(context_window_size("openai", "o1-mini"), Some(128_000));
        assert_eq!(context_window_size("openai", "o3-mini"), Some(200_000));
        assert_eq!(context_window_size("openai", "gpt-4-turbo"), Some(128_000));
        assert_eq!(context_window_size("openai", "gpt-4"), Some(8_192));
        assert_eq!(context_window_size("openai", "gpt-3.5-turbo"), Some(16_385));
    }

    #[test]
    fn test_deepseek_context_window() {
        assert_eq!(context_window_size("deepseek", "deepseek-chat"), Some(128_000));
        assert_eq!(context_window_size("deepseek", "deepseek-reasoner"), Some(64_000));
        assert_eq!(context_window_size("deepseek", "deepseek-r1"), Some(64_000));
    }

    #[test]
    fn test_zai_context_window() {
        assert_eq!(context_window_size("zai", "GLM-5"), Some(128_000));
        assert_eq!(context_window_size("zai", "GLM-5.1"), Some(128_000));
        assert_eq!(context_window_size("zai", "GLM-4.7"), Some(1_000_000));
        assert_eq!(context_window_size("zai", "GLM-4-plus"), Some(1_000_000));
    }

    #[test]
    fn test_unknown_provider() {
        assert_eq!(context_window_size("unknown", "model-x"), None);
    }

    #[test]
    fn test_resolve_with_overrides() {
        let mut overrides = HashMap::new();
        overrides.insert("claude-sonnet-4".into(), 100_000);
        overrides.insert("openai:gpt-4o".into(), 50_000);

        // Model-only override
        assert_eq!(
            resolve_context_window_size("anthropic", "claude-sonnet-4", Some(&overrides), None),
            Some(100_000)
        );
        // Qualified override (provider:model)
        assert_eq!(
            resolve_context_window_size("openai", "gpt-4o", Some(&overrides), None),
            Some(50_000)
        );
        // No override → falls through to static mapping
        assert_eq!(
            resolve_context_window_size("openai", "gpt-4o-mini", Some(&overrides), None),
            Some(128_000)
        );
    }

    #[test]
    fn test_resolve_override_beats_cache() {
        let mut overrides = HashMap::new();
        overrides.insert("model-a".into(), 50_000);

        let cache = ContextWindowCache::new();
        cache.populate(vec![("model-a".into(), 200_000)]);

        assert_eq!(
            resolve_context_window_size("provider", "model-a", Some(&overrides), Some(&cache)),
            Some(50_000) // override wins
        );
    }

    #[test]
    fn test_cache_populate_and_get() {
        let cache = ContextWindowCache::new();
        assert!(cache.get("test-model").is_none());

        cache.populate(vec![
            ("model-a".into(), 128_000),
            ("model-b".into(), 200_000),
        ]);

        assert_eq!(cache.get("model-a"), Some(128_000));
        assert_eq!(cache.get("model-b"), Some(200_000));
        assert!(cache.get("model-c").is_none());
    }

    #[test]
    fn test_cache_invalidate() {
        let cache = ContextWindowCache::new();
        assert!(cache.is_stale()); // never populated → stale
        cache.populate(vec![("model-a".into(), 128_000)]);
        assert!(!cache.is_stale()); // just populated → fresh
        cache.invalidate();
        assert!(cache.get("model-a").is_none());
        assert!(cache.is_stale()); // invalidated → stale
    }

    #[test]
    fn test_context_usage_new_resolved() {
        let overrides = HashMap::new();
        let cache = ContextWindowCache::new();
        let usage = ContextUsage::new_resolved(
            50_000, "anthropic", "claude-sonnet-4",
            Some(&overrides), Some(&cache),
        );
        assert!((usage.percentage().unwrap() - 0.25).abs() < 0.01);
    }

    #[test]
    fn test_context_usage_percentage() {
        let usage = ContextUsage::new(50_000, "anthropic", "claude-sonnet-4");
        assert!((usage.percentage().unwrap() - 0.25).abs() < 0.01);
    }

    #[test]
    fn test_warning_levels() {
        let low = ContextUsage::new(10_000, "anthropic", "claude-sonnet-4");
        assert_eq!(low.warning_level(), ContextWarning::None);

        let mid = ContextUsage::new(170_000, "anthropic", "claude-sonnet-4"); // 85%
        assert_eq!(mid.warning_level(), ContextWarning::Approaching);

        let high = ContextUsage::new(195_000, "anthropic", "claude-sonnet-4"); // 97.5%
        assert_eq!(high.warning_level(), ContextWarning::Critical);
    }

    #[test]
    fn test_warning_unknown_provider() {
        let usage = ContextUsage::new(10_000, "unknown", "model-x");
        assert_eq!(usage.warning_level(), ContextWarning::Unknown);
        assert!(usage.format_warning().is_none());
    }

    #[test]
    fn test_format_status() {
        let usage = ContextUsage::new(12_400, "anthropic", "claude-sonnet-4");
        let status = usage.format_status();
        assert!(status.contains("12.4K"));
        assert!(status.contains("200.0K"));
        assert!(status.contains("6.2%"));
    }

    #[test]
    fn test_format_status_unknown() {
        let usage = ContextUsage::new(5_000, "unknown", "model-x");
        let status = usage.format_status();
        assert!(status.contains("5.0K"));
        assert!(status.contains("unknown"));
    }

    #[test]
    fn test_format_tokens() {
        assert_eq!(format_tokens(500), "500");
        assert_eq!(format_tokens(12_345), "12.3K");
        assert_eq!(format_tokens(1_500_000), "1.5M");
    }
}
