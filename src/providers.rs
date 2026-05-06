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
pub struct ProviderManager {
    current: Arc<dyn Llm>,
    current_provider: String,
    current_model: String,
    custom_endpoints: HashMap<String, ProviderConfig>,
}

impl ProviderManager {
    /// Auto-detect provider from environment variables.
    ///
    /// Priority order:
    /// 1. `ANTHROPIC_API_KEY` → Anthropic Claude
    /// 2. `OPENAI_API_KEY` → OpenAI GPT
    /// 3. `DEEPSEEK_API_KEY` → DeepSeek
    /// 4. `GROQ_API_KEY` → Groq
    /// 5. `OPENROUTER_API_KEY` → OpenRouter
    /// 6. Fallback → Ollama (localhost)
    pub fn from_env() -> anyhow::Result<Self> {
        let (provider, model, llm): (String, String, Arc<dyn Llm>) =
            if let Ok(key) = std::env::var("ANTHROPIC_API_KEY") {
                let model = "claude-sonnet-4-20250514".to_string();
                let client = AnthropicClient::new(AnthropicConfig::new(&key, &model))?;
                ("anthropic".into(), model, Arc::new(client))
            } else if let Ok(key) = std::env::var("OPENAI_API_KEY") {
                let model = "gpt-4o".to_string();
                let client = OpenAIClient::new(OpenAIConfig::new(&key, &model))?;
                ("openai".into(), model, Arc::new(client))
            } else if let Ok(key) = std::env::var("DEEPSEEK_API_KEY") {
                let model = "deepseek-chat".to_string();
                let client = DeepSeekClient::chat(&key)?;
                ("deepseek".into(), model, Arc::new(client))
            } else if let Ok(key) = std::env::var("GROQ_API_KEY") {
                let model = "llama-3.3-70b-versatile".to_string();
                let client = GroqClient::new(GroqConfig::new(&key, &model))?;
                ("groq".into(), model, Arc::new(client))
            } else if let Ok(key) = std::env::var("OPENROUTER_API_KEY") {
                let model = "anthropic/claude-sonnet-4".to_string();
                let client = OpenRouterClient::new(OpenRouterConfig::new(&key, &model))?;
                ("openrouter".into(), model, Arc::new(client))
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

    /// Get current provider name.
    pub fn current_provider(&self) -> &str {
        &self.current_provider
    }

    /// Get current model name.
    pub fn current_model_name(&self) -> &str {
        &self.current_model
    }

    /// List available providers based on configured API keys.
    pub fn list_available(&self) -> Vec<ProviderInfo> {
        let mut list = Vec::new();
        if std::env::var("ANTHROPIC_API_KEY").is_ok() {
            list.push(ProviderInfo {
                provider: "anthropic".into(),
                default_model: "claude-sonnet-4-20250514".into(),
                available: true,
            });
        }
        if std::env::var("OPENAI_API_KEY").is_ok() {
            list.push(ProviderInfo {
                provider: "openai".into(),
                default_model: "gpt-4o".into(),
                available: true,
            });
        }
        if std::env::var("DEEPSEEK_API_KEY").is_ok() {
            list.push(ProviderInfo {
                provider: "deepseek".into(),
                default_model: "deepseek-chat".into(),
                available: true,
            });
        }
        if std::env::var("GROQ_API_KEY").is_ok() {
            list.push(ProviderInfo {
                provider: "groq".into(),
                default_model: "llama-3.3-70b-versatile".into(),
                available: true,
            });
        }
        if std::env::var("OPENROUTER_API_KEY").is_ok() {
            list.push(ProviderInfo {
                provider: "openrouter".into(),
                default_model: "anthropic/claude-sonnet-4".into(),
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
    fn create_model(&self, provider: &str, model: &str) -> anyhow::Result<Arc<dyn Llm>> {
        match provider {
            "anthropic" => {
                let key = std::env::var("ANTHROPIC_API_KEY")
                    .map_err(|_| anyhow::anyhow!("ANTHROPIC_API_KEY not set"))?;
                let client = AnthropicClient::new(AnthropicConfig::new(&key, model))?;
                Ok(Arc::new(client))
            }
            "openai" => {
                let key = std::env::var("OPENAI_API_KEY")
                    .map_err(|_| anyhow::anyhow!("OPENAI_API_KEY not set"))?;
                let client = OpenAIClient::new(OpenAIConfig::new(&key, model))?;
                Ok(Arc::new(client))
            }
            "deepseek" => {
                let key = std::env::var("DEEPSEEK_API_KEY")
                    .map_err(|_| anyhow::anyhow!("DEEPSEEK_API_KEY not set"))?;
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
                let key = std::env::var("GROQ_API_KEY")
                    .map_err(|_| anyhow::anyhow!("GROQ_API_KEY not set"))?;
                let client = GroqClient::new(GroqConfig::new(&key, model))?;
                Ok(Arc::new(client))
            }
            "ollama" => {
                let client = OllamaModel::new(OllamaConfig::new(model))?;
                Ok(Arc::new(client))
            }
            "openrouter" => {
                let key = std::env::var("OPENROUTER_API_KEY")
                    .map_err(|_| anyhow::anyhow!("OPENROUTER_API_KEY not set"))?;
                let client = OpenRouterClient::new(OpenRouterConfig::new(&key, model))?;
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
                        "Unknown provider '{}'. Available: anthropic, openai, deepseek, groq, ollama, openrouter",
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
        assert_eq!(default_model_for_provider("unknown"), "unknown");
    }
}
