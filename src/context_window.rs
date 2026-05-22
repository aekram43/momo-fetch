//! Context window awareness: maps models to their token limits
//! and tracks usage from API-reported prompt_token_count.

use colored::Colorize;

/// Known context window size (in tokens) for a given provider + model.
///
/// Uses the same match-on-provider-then-model pattern as CostTracker::get_pricing().
/// Returns `None` for unknown models so the system degrades gracefully.
pub fn context_window_size(provider: &str, model: &str) -> Option<u64> {
    match provider {
        "anthropic" => Some(200_000),
        "openai" => {
            if model.starts_with("o1") || model.starts_with("o3") {
                Some(200_000)
            } else {
                Some(128_000)
            }
        }
        "deepseek" => Some(128_000),
        "groq" => Some(128_000),
        "ollama" => Some(128_000),
        "openrouter" => Some(200_000),
        "zai" => Some(128_000),
        "custom" => Some(128_000),
        _ => None,
    }
}

/// Current context window usage snapshot.
pub struct ContextUsage {
    pub prompt_tokens: i64,
    pub context_window: Option<u64>,
}

impl ContextUsage {
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
        assert_eq!(context_window_size("anthropic", "anything"), Some(200_000));
    }

    #[test]
    fn test_openai_context_window() {
        assert_eq!(context_window_size("openai", "gpt-4o"), Some(128_000));
        assert_eq!(context_window_size("openai", "gpt-4o-mini"), Some(128_000));
        assert_eq!(context_window_size("openai", "o1-preview"), Some(200_000));
        assert_eq!(context_window_size("openai", "o3-mini"), Some(200_000));
    }

    #[test]
    fn test_zai_context_window() {
        assert_eq!(context_window_size("zai", "GLM-5"), Some(128_000));
    }

    #[test]
    fn test_unknown_provider() {
        assert_eq!(context_window_size("unknown", "model-x"), None);
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
    fn test_format_warning_approaching() {
        let usage = ContextUsage::new(170_000, "anthropic", "claude-sonnet-4");
        let warning = usage.format_warning().unwrap();
        assert!(warning.contains("Context:"));
    }

    #[test]
    fn test_format_warning_critical() {
        let usage = ContextUsage::new(195_000, "anthropic", "claude-sonnet-4");
        let warning = usage.format_warning().unwrap();
        assert!(warning.contains("CRITICAL"));
    }

    #[test]
    fn test_format_tokens() {
        assert_eq!(format_tokens(500), "500");
        assert_eq!(format_tokens(12_345), "12.3K");
        assert_eq!(format_tokens(1_500_000), "1.5M");
    }
}
