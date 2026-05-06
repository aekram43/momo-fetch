use std::collections::HashMap;

/// Configuration for a custom provider endpoint.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ProviderConfig {
    pub name: String,
    pub model: String,
    pub api_key_env: String,
    pub base_url: Option<String>,
    pub is_openai_compatible: bool,
}

/// Manages LLM providers and allows hot-swapping mid-session.
pub struct ProviderManager {
    current_provider: String,
    current_model: String,
    custom_endpoints: HashMap<String, ProviderConfig>,
}

impl ProviderManager {
    /// Auto-detect provider from environment variables.
    pub fn from_env() -> anyhow::Result<Self> {
        // Check env vars in priority order
        let (provider, model) = if std::env::var("ANTHROPIC_API_KEY").is_ok() {
            ("anthropic".into(), "claude-sonnet-4-20250514".into())
        } else if std::env::var("OPENAI_API_KEY").is_ok() {
            ("openai".into(), "gpt-4o".into())
        } else if std::env::var("DEEPSEEK_API_KEY").is_ok() {
            ("deepseek".into(), "deepseek-chat".into())
        } else if std::env::var("GROQ_API_KEY").is_ok() {
            ("groq".into(), "llama-3.3-70b-versatile".into())
        } else if std::env::var("OPENROUTER_API_KEY").is_ok() {
            ("openrouter".into(), "anthropic/claude-sonnet-4".into())
        } else {
            // Default to ollama (no key needed)
            ("ollama".into(), "llama3".into())
        };

        Ok(Self {
            current_provider: provider,
            current_model: model,
            custom_endpoints: HashMap::new(),
        })
    }

    /// Switch to a different provider/model.
    pub fn switch(&mut self, provider: &str, model: &str) {
        self.current_provider = provider.to_string();
        self.current_model = model.to_string();
    }

    /// Get current provider name.
    pub fn current_provider(&self) -> &str {
        &self.current_provider
    }

    /// Get current model name.
    pub fn current_model_name(&self) -> &str {
        &self.current_model
    }

    /// List available providers based on configured API keys.
    pub fn list_available(&self) -> Vec<(&str, &str)> {
        let mut list = Vec::new();
        if std::env::var("ANTHROPIC_API_KEY").is_ok() {
            list.push(("anthropic", "claude-sonnet-4-20250514"));
        }
        if std::env::var("OPENAI_API_KEY").is_ok() {
            list.push(("openai", "gpt-4o"));
        }
        if std::env::var("DEEPSEEK_API_KEY").is_ok() {
            list.push(("deepseek", "deepseek-chat"));
        }
        if std::env::var("GROQ_API_KEY").is_ok() {
            list.push(("groq", "llama-3.3-70b-versatile"));
        }
        if std::env::var("OPENROUTER_API_KEY").is_ok() {
            list.push(("openrouter", "anthropic/claude-sonnet-4"));
        }
        // Ollama always available (local)
        list.push(("ollama", "llama3"));

        // Add custom endpoints
        for (name, config) in &self.custom_endpoints {
            list.push((name.as_str(), &config.model));
        }
        list
    }
}
