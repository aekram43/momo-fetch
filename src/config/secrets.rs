/// OS keychain secret management with .env fallback.
///
/// Uses `keyring-core` for secure storage on macOS (Keychain),
/// Linux (Secret Service/libsecret), and Windows (Credential Manager).
/// Falls back to environment variables for CI environments.

use std::fmt;

/// Known provider names and their corresponding env var names.
const KNOWN_PROVIDERS: &[(&str, &str)] = &[
    ("anthropic", "ANTHROPIC_API_KEY"),
    ("openai", "OPENAI_API_KEY"),
    ("deepseek", "DEEPSEEK_API_KEY"),
    ("groq", "GROQ_API_KEY"),
    ("openrouter", "OPENROUTER_API_KEY"),
    ("zai", "ZAI_API_KEY"),
    ("gemini", "GOOGLE_API_KEY"),
    ("serper", "SERPER_API_KEY"),
    ("ollama", ""), // Ollama has no API key
];

const SERVICE_NAME: &str = "momo-fetch";

/// Error type for secret operations.
#[derive(Debug)]
pub enum SecretError {
    NotFound { provider: String, env_var: String },
    KeyringError(String),
    NotSet { provider: String },
}

impl fmt::Display for SecretError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound { provider, env_var } => {
                write!(
                    f,
                    "Secret not found for '{provider}'. Set {env_var} or use /key set {provider}"
                )
            }
            Self::KeyringError(msg) => write!(f, "Keychain error: {msg}"),
            Self::NotSet { provider } => write!(f, "No secret set for '{provider}'"),
        }
    }
}

impl std::error::Error for SecretError {}

/// Manage secrets via OS keychain with .env fallback.
pub struct SecretStore;

impl SecretStore {
    /// Get a secret for a provider.
    ///
    /// Priority:
    /// 1. Environment variable (e.g., `ANTHROPIC_API_KEY`)
    /// 2. OS keychain
    pub fn get(provider: &str) -> Result<String, SecretError> {
        let env_var = env_var_for_provider(provider);

        // Check environment first (highest priority for runtime overrides)
        if let Ok(key) = std::env::var(&env_var) {
            return Ok(key);
        }

        // Try OS keychain
        let entry =
            keyring_core::Entry::new(SERVICE_NAME, provider).map_err(|e| {
                SecretError::KeyringError(format!("Failed to create keychain entry: {e}"))
            })?;

        match entry.get_password() {
            Ok(key) => Ok(key),
            Err(keyring_core::Error::NoEntry) => Err(SecretError::NotFound {
                provider: provider.to_string(),
                env_var,
            }),
            Err(e) => Err(SecretError::KeyringError(format!(
                "Failed to read from keychain: {e}"
            ))),
        }
    }

    /// Store a secret in the OS keychain.
    pub fn set(provider: &str, key: &str) -> Result<(), SecretError> {
        let entry =
            keyring_core::Entry::new(SERVICE_NAME, provider).map_err(|e| {
                SecretError::KeyringError(format!("Failed to create keychain entry: {e}"))
            })?;

        entry
            .set_password(key)
            .map_err(|e| SecretError::KeyringError(format!("Failed to store in keychain: {e}")))?;

        Ok(())
    }

    /// Delete a secret from the OS keychain.
    pub fn delete(provider: &str) -> Result<(), SecretError> {
        let entry =
            keyring_core::Entry::new(SERVICE_NAME, provider).map_err(|e| {
                SecretError::KeyringError(format!("Failed to create keychain entry: {e}"))
            })?;

        entry
            .delete_credential()
            .map_err(|e| match e {
                keyring_core::Error::NoEntry => SecretError::NotSet {
                    provider: provider.to_string(),
                },
                _ => SecretError::KeyringError(format!(
                    "Failed to delete from keychain: {e}"
                )),
            })?;

        Ok(())
    }

    /// List known providers that have secrets (keychain or env var).
    ///
    /// Returns `(provider_name, source)` where source is "keychain", "env", or "both".
    /// Keys are always masked for display.
    pub fn list() -> Vec<(String, String)> {
        let mut found = Vec::new();

        for &(provider, env_var) in KNOWN_PROVIDERS {
            if env_var.is_empty() {
                // Ollama: always available, no key needed
                found.push((provider.to_string(), "none".to_string()));
                continue;
            }

            let in_env = std::env::var(env_var).is_ok();
            let in_keychain = keyring_core::Entry::new(SERVICE_NAME, provider)
                .and_then(|e| e.get_password())
                .is_ok();

            if in_env && in_keychain {
                found.push((provider.to_string(), "both".to_string()));
            } else if in_env {
                found.push((provider.to_string(), "env".to_string()));
            } else if in_keychain {
                found.push((provider.to_string(), "keychain".to_string()));
            }
        }

        found
    }

    /// Get the env var name for a provider.
    pub fn env_var_for(provider: &str) -> String {
        env_var_for_provider(provider)
    }
}

/// Get the environment variable name for a provider.
fn env_var_for_provider(provider: &str) -> String {
    // First check known providers
    for &(name, env_var) in KNOWN_PROVIDERS {
        if name == provider && !env_var.is_empty() {
            return env_var.to_string();
        }
    }

    // For custom providers, generate from name
    format!("{}_API_KEY", provider.to_uppercase())
}

/// Mask a secret key for safe display.
///
/// Shows first 4 and last 4 characters, masking the middle.
pub fn mask_key(key: &str) -> String {
    if key.len() <= 8 {
        return "*".repeat(key.len());
    }

    let start = &key[..4];
    let end = &key[key.len() - 4..];
    let masked_len = key.len() - 8;

    format!("{start}{}{end}", "*".repeat(masked_len))
}

/// Get the default model name for a given provider.
/// Used by /key set to provide context about what was configured.
pub fn default_model_for_provider(provider: &str) -> &'static str {
    match provider {
        "anthropic" => "claude-sonnet-4-20250514",
        "openai" => "gpt-4o",
        "deepseek" => "deepseek-chat",
        "groq" => "llama-3.3-70b-versatile",
        "ollama" => "llama3.2",
        "openrouter" => "anthropic/claude-sonnet-4",
        "zai" => "GLM-5",
        "gemini" => "gemini-2.0-flash",
        "serper" => "(search API, no model)",
        _ => "(unknown)",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_env_var_for_known_provider() {
        assert_eq!(env_var_for_provider("anthropic"), "ANTHROPIC_API_KEY");
        assert_eq!(env_var_for_provider("openai"), "OPENAI_API_KEY");
        assert_eq!(env_var_for_provider("deepseek"), "DEEPSEEK_API_KEY");
        assert_eq!(env_var_for_provider("groq"), "GROQ_API_KEY");
        assert_eq!(env_var_for_provider("openrouter"), "OPENROUTER_API_KEY");
        assert_eq!(env_var_for_provider("zai"), "ZAI_API_KEY");
        assert_eq!(env_var_for_provider("gemini"), "GOOGLE_API_KEY");
        assert_eq!(env_var_for_provider("serper"), "SERPER_API_KEY");
    }

    #[test]
    fn test_env_var_for_custom_provider() {
        assert_eq!(
            env_var_for_provider("myprovider"),
            "MYPROVIDER_API_KEY"
        );
        assert_eq!(
            env_var_for_provider("ZAI"),
            "ZAI_API_KEY"
        );
    }

    #[test]
    fn test_mask_key() {
        // sk-1234567890abcdef = 19 chars: 4 shown + 11 masked + 4 shown
        assert_eq!(mask_key("sk-1234567890abcdef"), "sk-1***********cdef");
        assert_eq!(mask_key("short"), "*****");
        assert_eq!(mask_key("12345678"), "********");
        assert_eq!(mask_key("a"), "*");
        assert_eq!(mask_key("ab"), "**");
        assert_eq!(mask_key("abcdefgh"), "********");
        // 9 chars: 4 shown + 1 masked + 4 shown
        // key[5..] = "fghi" (not "efgh")
        assert_eq!(mask_key("abcdefghi"), "abcd*fghi");
        // 10 chars: 4 shown + 2 masked + 4 shown
        // key[6..] = "ghij"
        assert_eq!(mask_key("abcdefghij"), "abcd**ghij");
    }

    #[test]
    fn test_default_model_for_provider() {
        assert_eq!(
            default_model_for_provider("anthropic"),
            "claude-sonnet-4-20250514"
        );
        assert_eq!(default_model_for_provider("openai"), "gpt-4o");
        assert_eq!(default_model_for_provider("ollama"), "llama3.2");
        assert!(default_model_for_provider("unknown").starts_with("(unknown"));
    }

    #[test]
    fn test_list_without_secrets() {
        // This test just verifies list() doesn't panic and returns a Vec.
        // We can't assert specific providers without knowing what's in
        // the test environment's keychain or env vars.
        let list = SecretStore::list();
        // Ollama should always be present (it has no key)
        assert!(list.iter().any(|(p, _)| p == "ollama"));
    }

    #[test]
    fn test_get_fallback_to_env() {
        // Use a provider name that definitely has no env var or keychain entry.
        let result = SecretStore::get("nonexistent_provider_test_xyz");
        assert!(result.is_err());
        // The error should mention the provider — either as NotFound or KeyringError
        let err_str = result.unwrap_err().to_string();
        assert!(
            err_str.contains("nonexistent_provider_test_xyz") || err_str.contains("Keychain"),
            "Unexpected error: {err_str}"
        );
    }

    #[tokio::test]
    async fn test_set_get_delete_roundtrip() {
        let provider = format!("__harness_test_{}", std::process::id());

        // Skip test if keychain is not available (e.g., CI, headless)
        if SecretStore::set(&provider, "test-key-12345").is_err() {
            println!("Skipping: OS keychain not available in this environment");
            return;
        }

        // Get (should find in keychain)
        let result = SecretStore::get(&provider).unwrap();
        assert_eq!(result, "test-key-12345");

        // Delete
        SecretStore::delete(&provider).unwrap();

        // Get after delete should fail
        let result = SecretStore::get(&provider);
        assert!(result.is_err());

        // Delete again should fail with NotSet
        let result = SecretStore::delete(&provider);
        assert!(matches!(result.unwrap_err(), SecretError::NotSet { .. }));
    }
}
