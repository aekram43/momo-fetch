use std::collections::HashMap;
use std::sync::Arc;

use adk_model::anthropic::{AnthropicClient, AnthropicConfig};
use adk_model::deepseek::DeepSeekClient;
use adk_model::groq::{GroqClient, GroqConfig};
use adk_model::ollama::{OllamaConfig, OllamaModel};
use adk_model::openai::{OpenAIClient, OpenAIConfig};
use adk_model::openai_compatible::{OpenAICompatible, OpenAICompatibleConfig};
use adk_model::openrouter::{OpenRouterClient, OpenRouterConfig};
use adk_rust::prelude::Llm;

use crate::context_window::ContextWindowCache;

/// Configuration for a custom (OpenAI-compatible) provider endpoint.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ProviderConfig {
    pub name: String,
    pub model: String,
    pub api_key_env: String,
    pub base_url: Option<String>,
    pub is_openai_compatible: bool,
}

/// Manages LLM providers and allows hot-swapping mid-session.
///
/// Holds a current `Arc<dyn Llm>` that can be swapped at runtime
/// via `/model` and `/provider` slash commands.
#[derive(Clone)]
pub struct ProviderManager {
    current: Arc<dyn Llm>,
    current_provider: String,
    current_model: String,
    custom_endpoints: HashMap<String, ProviderConfig>,
    /// Cache of dynamically fetched context window sizes from provider APIs.
    context_window_cache: Arc<ContextWindowCache>,
}

impl ProviderManager {
    /// Create ProviderManager from settings file with fallback to env.
    ///
    /// Priority order:
    /// 1. Settings file (default_provider + default_model)
    /// 2. Environment variables or OS keychain
    /// 3. Fallback → Ollama (localhost)
    pub fn from_settings_or_env(settings_provider: Option<&str>, settings_model: Option<&str>) -> anyhow::Result<Self> {
        // If settings specify both provider and model, use them
        if let (Some(provider), Some(model)) = (settings_provider, settings_model) {
            return Self::from_provider_and_model(provider, model);
        }

        // Otherwise, fall back to environment detection
        Self::from_env()
    }

    /// Create ProviderManager from explicit provider and model names.
    fn from_provider_and_model(provider: &str, model: &str) -> anyhow::Result<Self> {
        // Create a temporary ProviderManager with empty custom_endpoints
        let temp_manager = Self {
            current: std::sync::Arc::new(adk_model::anthropic::AnthropicClient::new(
                adk_model::anthropic::AnthropicConfig::new("temp_key", "temp_model")
            ).unwrap()),
            current_provider: "temp".to_string(),
            current_model: "temp".to_string(),
            custom_endpoints: HashMap::new(),
            context_window_cache: Arc::new(ContextWindowCache::new()),
        };

        let llm = temp_manager.create_model(provider, model)?;

        Ok(Self {
            current: llm,
            current_provider: provider.to_string(),
            current_model: model.to_string(),
            custom_endpoints: HashMap::new(),
            context_window_cache: Arc::new(ContextWindowCache::new()),
        })
    }

    /// Auto-detect provider from environment variables or OS keychain.
    ///
    /// Priority order:
    /// 1. `ANTHROPIC_API_KEY` env or keychain → Anthropic Claude
    /// 2. `OPENAI_API_KEY` env or keychain → OpenAI GPT
    /// 3. `DEEPSEEK_API_KEY` env or keychain → DeepSeek
    /// 4. `GROQ_API_KEY` env or keychain → Groq
    /// 5. `OPENROUTER_API_KEY` env or keychain → OpenRouter
    /// 6. Fallback → Ollama (localhost)
    pub fn from_env() -> anyhow::Result<Self> {
        use crate::config::secrets::SecretStore;

        let (provider, model, llm): (String, String, Arc<dyn Llm>) =
            if let Ok(key) = SecretStore::get("anthropic") {
                let model = "claude-sonnet-4-20250514".to_string();
                let client = AnthropicClient::new(AnthropicConfig::new(&key, &model))?;
                ("anthropic".into(), model, Arc::new(client))
            } else if let Ok(key) = SecretStore::get("openai") {
                let model = "gpt-4o".to_string();
                let client = OpenAIClient::new(OpenAIConfig::new(&key, &model))?;
                ("openai".into(), model, Arc::new(client))
            } else if let Ok(key) = SecretStore::get("deepseek") {
                let model = "deepseek-chat".to_string();
                let client = DeepSeekClient::chat(&key)?;
                ("deepseek".into(), model, Arc::new(client))
            } else if let Ok(key) = SecretStore::get("groq") {
                let model = "llama-3.3-70b-versatile".to_string();
                let client = GroqClient::new(GroqConfig::new(&key, &model))?;
                ("groq".into(), model, Arc::new(client))
            } else if let Ok(key) = SecretStore::get("openrouter") {
                let model = "anthropic/claude-sonnet-4".to_string();
                let client = OpenRouterClient::new(OpenRouterConfig::new(&key, &model))?;
                ("openrouter".into(), model, Arc::new(client))
            } else if let Ok(key) = SecretStore::get("zai") {
                let model = std::env::var("ZAI_MODEL")
                    .or_else(|_| std::env::var("LLM_MODEL"))
                    .unwrap_or_else(|_| "GLM-5".to_string());
                let base_url = std::env::var("ZAI_URL")
                    .or_else(|_| std::env::var("ZAI_LLM_URL"))
                    .unwrap_or_else(|_| "https://api.z.ai/api/coding/paas/v4".to_string());
                let config = OpenAICompatibleConfig::new(&key, &model)
                    .with_base_url(&base_url)
                    .with_provider_name("zai");
                let client = OpenAICompatible::new(config)?;
                ("zai".into(), model, Arc::new(client))
            } else if let (Ok(url), Ok(key)) = (
                std::env::var("LLM_URL"),
                std::env::var("LLM_APIKEY"),
            ) {
                // Generic OpenAI-compatible endpoint via LLM_URL + LLM_APIKEY
                let model = std::env::var("LLM_MODEL")
                    .unwrap_or_else(|_| "default".to_string());
                let config = OpenAICompatibleConfig::new(&key, &model)
                    .with_base_url(&url)
                    .with_provider_name("custom");
                let client = OpenAICompatible::new(config)?;
                ("custom".into(), model, Arc::new(client))
            } else {
                // Default to ollama (no key needed)
                let model = "llama3.2".to_string();
                let client = OllamaModel::new(OllamaConfig::new(&model))?;
                ("ollama".into(), model, Arc::new(client))
            };

        Ok(Self {
            current: llm,
            current_provider: provider,
            current_model: model,
            custom_endpoints: HashMap::new(),
            context_window_cache: Arc::new(ContextWindowCache::new()),
        })
    }

    /// Switch to a different provider and/or model.
    ///
    /// Creates a new LLM instance from environment variables / custom config
    /// and replaces the current model. Returns an error if the provider is
    /// unknown or the required API key is missing.
    pub fn switch(&mut self, provider: &str, model: &str) -> anyhow::Result<()> {
        let new_llm = self.create_model(provider, model)?;
        self.current_provider = provider.to_string();
        self.current_model = model.to_string();
        self.current = new_llm;
        Ok(())
    }

    /// Switch only the model, keeping the current provider.
    pub fn switch_model(&mut self, model: &str) -> anyhow::Result<()> {
        let provider = self.current_provider.clone();
        self.switch(&provider, model)
    }

    /// Switch only the provider, using its default model.
    pub fn switch_provider(&mut self, provider: &str) -> anyhow::Result<()> {
        let model = default_model_for_provider(provider);
        self.switch(provider, &model)
    }

    /// Get the current LLM instance (for passing to agents).
    pub fn current(&self) -> Arc<dyn Llm> {
        self.current.clone()
    }

    /// Create a ProviderManager wrapping an existing LLM instance.
    ///
    /// Used by sub-agent spawning to share the same model without
    /// re-reading environment variables.
    pub fn from_current(
        llm: Arc<dyn Llm>,
        provider: String,
        model: String,
    ) -> Self {
        Self {
            current: llm,
            current_provider: provider,
            current_model: model,
            custom_endpoints: HashMap::new(),
            context_window_cache: Arc::new(ContextWindowCache::new()),
        }
    }

    /// Get current provider name.
    pub fn current_provider(&self) -> &str {
        &self.current_provider
    }

    /// Get current model name.
    pub fn current_model_name(&self) -> &str {
        &self.current_model
    }

    /// Get the context window cache.
    pub fn context_window_cache(&self) -> &Arc<ContextWindowCache> {
        &self.context_window_cache
    }

    /// Fetch context window sizes from provider model info APIs (fire-and-forget).
    ///
    /// Currently only OpenRouter's `list_models()` exposes `context_length`.
    /// Runs as a background task; never blocks the REPL.
    pub fn prefetch_context_windows(&self) {
        if self.current_provider != "openrouter" {
            return;
        }

        if !self.context_window_cache.is_stale() {
            return;
        }

        let cache = self.context_window_cache.clone();
        let api_key = crate::config::secrets::SecretStore::get("openrouter").ok();

        if let Some(key) = api_key {
            tokio::spawn(async move {
                let config = OpenRouterConfig::new(&key, "unused");
                let client = match OpenRouterClient::new(config) {
                    Ok(c) => c,
                    Err(_) => return,
                };
                match client.list_models().await {
                    Ok(models) => {
                        let entries: Vec<(String, u64)> = models
                            .into_iter()
                            .filter_map(|m| {
                                m.context_length.map(|cl| (m.id, cl as u64))
                            })
                            .collect();
                        if !entries.is_empty() {
                            tracing::info!(
                                "Context window cache: populated {} models from OpenRouter",
                                entries.len()
                            );
                            cache.populate(entries);
                        }
                    }
                    Err(e) => {
                        tracing::debug!("Context window cache: OpenRouter fetch failed: {}", e);
                    }
                }
            });
        }
    }

    /// List available providers based on configured API keys (env or keychain).
    pub fn list_available(&self) -> Vec<ProviderInfo> {
        use crate::config::secrets::SecretStore;

        let mut list = Vec::new();
        if SecretStore::get("anthropic").is_ok() {
            list.push(ProviderInfo {
                provider: "anthropic".into(),
                default_model: "claude-sonnet-4-20250514".into(),
                available: true,
            });
        }
        if SecretStore::get("openai").is_ok() {
            list.push(ProviderInfo {
                provider: "openai".into(),
                default_model: "gpt-4o".into(),
                available: true,
            });
        }
        if SecretStore::get("deepseek").is_ok() {
            list.push(ProviderInfo {
                provider: "deepseek".into(),
                default_model: "deepseek-chat".into(),
                available: true,
            });
        }
        if SecretStore::get("groq").is_ok() {
            list.push(ProviderInfo {
                provider: "groq".into(),
                default_model: "llama-3.3-70b-versatile".into(),
                available: true,
            });
        }
        if SecretStore::get("openrouter").is_ok() {
            list.push(ProviderInfo {
                provider: "openrouter".into(),
                default_model: "anthropic/claude-sonnet-4".into(),
                available: true,
            });
        }
        if SecretStore::get("zai").is_ok() {
            list.push(ProviderInfo {
                provider: "zai".into(),
                default_model: "GLM-5".into(),
                available: true,
            });
        }
        // Ollama always available (local)
        list.push(ProviderInfo {
            provider: "ollama".into(),
            default_model: "llama3.2".into(),
            available: true,
        });

        // Add custom endpoints
        for (name, config) in &self.custom_endpoints {
            list.push(ProviderInfo {
                provider: name.clone(),
                default_model: config.model.clone(),
                available: true,
            });
        }
        list
    }

    /// Create a model instance for any supported provider.
    pub fn create_model(&self, provider: &str, model: &str) -> anyhow::Result<Arc<dyn Llm>> {
        use crate::config::secrets::SecretStore;

        match provider {
            "anthropic" => {
                let key = SecretStore::get("anthropic")
                    .map_err(|e| anyhow::anyhow!("{e}"))?;
                let client = AnthropicClient::new(AnthropicConfig::new(&key, model))?;
                Ok(Arc::new(client))
            }
            "openai" => {
                let key = SecretStore::get("openai")
                    .map_err(|e| anyhow::anyhow!("{e}"))?;
                let client = OpenAIClient::new(OpenAIConfig::new(&key, model))?;
                Ok(Arc::new(client))
            }
            "deepseek" => {
                let key = SecretStore::get("deepseek")
                    .map_err(|e| anyhow::anyhow!("{e}"))?;
                // Use chat() for deepseek-chat models, reasoner() for deepseek-reasoner
                if model.contains("reasoner") {
                    let client = DeepSeekClient::reasoner(&key)?;
                    Ok(Arc::new(client))
                } else {
                    let client = DeepSeekClient::chat(&key)?;
                    Ok(Arc::new(client))
                }
            }
            "groq" => {
                let key = SecretStore::get("groq")
                    .map_err(|e| anyhow::anyhow!("{e}"))?;
                let client = GroqClient::new(GroqConfig::new(&key, model))?;
                Ok(Arc::new(client))
            }
            "ollama" => {
                let client = OllamaModel::new(OllamaConfig::new(model))?;
                Ok(Arc::new(client))
            }
            "openrouter" => {
                let key = SecretStore::get("openrouter")
                    .map_err(|e| anyhow::anyhow!("{e}"))?;
                let client = OpenRouterClient::new(OpenRouterConfig::new(&key, model))?;
                Ok(Arc::new(client))
            }
            "zai" => {
                let key = SecretStore::get("zai")
                    .map_err(|e| anyhow::anyhow!("{e}"))?;
                let base_url = std::env::var("ZAI_LLM_URL")
                    .unwrap_or_else(|_| "https://api.z.ai/api/coding/paas/v4".to_string());
                let config = OpenAICompatibleConfig::new(&key, model)
                    .with_base_url(&base_url)
                    .with_provider_name("zai");
                let client = OpenAICompatible::new(config)?;
                Ok(Arc::new(client))
            }
            "custom" => {
                let url = std::env::var("LLM_URL")
                    .map_err(|_| anyhow::anyhow!("LLM_URL not set"))?;
                let key = std::env::var("LLM_APIKEY")
                    .map_err(|_| anyhow::anyhow!("LLM_APIKEY not set"))?;
                let config = OpenAICompatibleConfig::new(&key, model)
                    .with_base_url(&url)
                    .with_provider_name("custom");
                let client = OpenAICompatible::new(config)?;
                Ok(Arc::new(client))
            }
            _ => {
                // Check custom endpoints
                if let Some(config) = self.custom_endpoints.get(provider) {
                    let key = std::env::var(&config.api_key_env)
                        .map_err(|_| anyhow::anyhow!("{} not set", config.api_key_env))?;
                    let mut builder = OpenAICompatibleConfig::new(&key, model);
                    if let Some(url) = &config.base_url {
                        builder = builder.with_base_url(url);
                    }
                    let client = OpenAICompatible::new(builder.with_provider_name(provider))?;
                    Ok(Arc::new(client))
                } else {
                    Err(anyhow::anyhow!(
                        "Unknown provider '{}'. Available: anthropic, openai, deepseek, groq, ollama, openrouter, zai, custom (LLM_URL+LLM_APIKEY)",
                        provider
                    ))
                }
            }
        }
    }
}

/// Info about an available provider.
#[derive(Debug, Clone)]
pub struct ProviderInfo {
    pub provider: String,
    pub default_model: String,
    #[allow(dead_code)]
    pub available: bool,
}

/// Get the default model name for a given provider.
fn default_model_for_provider(provider: &str) -> String {
    match provider {
        "anthropic" => "claude-sonnet-4-20250514".into(),
        "openai" => "gpt-4o".into(),
        "deepseek" => "deepseek-chat".into(),
        "groq" => "llama-3.3-70b-versatile".into(),
        "ollama" => "llama3.2".into(),
        "openrouter" => "anthropic/claude-sonnet-4".into(),
        "zai" => "GLM-5".into(),
        "custom" => std::env::var("LLM_MODEL").unwrap_or_else(|_| "default".into()),
        _ => "unknown".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_model_for_provider() {
        assert_eq!(
            default_model_for_provider("anthropic"),
            "claude-sonnet-4-20250514"
        );
        assert_eq!(default_model_for_provider("openai"), "gpt-4o");
        assert_eq!(default_model_for_provider("deepseek"), "deepseek-chat");
        assert_eq!(
            default_model_for_provider("groq"),
            "llama-3.3-70b-versatile"
        );
        assert_eq!(default_model_for_provider("ollama"), "llama3.2");
        assert_eq!(
            default_model_for_provider("openrouter"),
            "anthropic/claude-sonnet-4"
        );
        assert_eq!(default_model_for_provider("zai"), "GLM-5");
        // custom provider reads from LLM_MODEL env var, defaults to "default"
        assert_eq!(default_model_for_provider("custom"), "default");
        assert_eq!(default_model_for_provider("unknown"), "unknown");
    }
}
