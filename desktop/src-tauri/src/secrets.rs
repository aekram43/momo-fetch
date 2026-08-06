//! API keys, stored in the OS keychain and handed to the gateway at spawn.
//!
//! # Why the shell owns this and the gateway never touches the keychain
//!
//! macOS attaches an ACL to every keychain item listing the binaries allowed to
//! read it. An item written by `momo-worker` and read by `momo-fetch` is a
//! cross-binary read: macOS prompts the user for permission, and in a spawned
//! child with no UI that prompt is at best confusing and at worst a hang.
//!
//! So exactly one binary — this one — reads and writes the keychain, and the
//! key reaches the gateway as an environment variable at spawn. That also makes
//! Windows and Linux behave identically instead of each having their own story.
//!
//! # Consequence for precedence
//!
//! The gateway resolves a secret as **env → keychain**, and `dotenvy` loads the
//! workspace `.env` into env without overriding what is already there. Since the
//! shell injects *before* the child starts, a key set in the UI wins over the
//! same key in the workspace `.env`.
//!
//! That is the opposite of the CLI, where `.env` wins because nothing injects.
//! It is the right default here — in a GUI the thing you typed into the app
//! should be the thing it uses — but it has to be *said*, so `status()` reports
//! which keys are also present in `.env` and the UI explains the override.
//!
//! # What is never exposed
//!
//! There is no command to read a key back. The UI needs to know *whether* one is
//! set, never what it is, and a stored secret that cannot be read out of the app
//! cannot be exfiltrated by anything that reaches the app.

use std::collections::HashMap;
use std::path::Path;

/// Providers the UI can configure, with the env var each maps to.
///
/// Mirrors `KNOWN_PROVIDERS` in `src/config/secrets.rs`. Ollama is absent
/// deliberately: it takes no key.
const PROVIDERS: &[(&str, &str)] = &[
    ("anthropic", "ANTHROPIC_API_KEY"),
    ("openai", "OPENAI_API_KEY"),
    ("deepseek", "DEEPSEEK_API_KEY"),
    ("groq", "GROQ_API_KEY"),
    ("openrouter", "OPENROUTER_API_KEY"),
    ("zai", "ZAI_API_KEY"),
    ("gemini", "GOOGLE_API_KEY"),
];

/// Must match `SERVICE_NAME` in `src/config/secrets.rs`, or the gateway would
/// look under a different service name than the shell writes to.
const SERVICE: &str = "momo-fetch";

#[derive(Debug, Clone, serde::Serialize)]
pub struct SecretStatus {
    pub provider: String,
    pub env_var: String,
    /// A key is stored for this provider.
    pub configured: bool,
    /// The workspace `.env` also defines this variable.
    ///
    /// Not an error — but the UI must explain which one wins, because "I changed
    /// my key and nothing happened" is otherwise unexplainable.
    pub also_in_env_file: bool,
}

pub fn env_var_for(provider: &str) -> Option<&'static str> {
    PROVIDERS
        .iter()
        .find(|(p, _)| *p == provider)
        .map(|(_, v)| *v)
}

/// Store a key. Overwrites any existing value for that provider.
pub fn set(provider: &str, key: &str) -> Result<(), String> {
    let _ = env_var_for(provider).ok_or_else(|| format!("Unknown provider '{provider}'."))?;
    if key.trim().is_empty() {
        return Err("The key is empty.".into());
    }
    keyring::Entry::new(SERVICE, provider)
        .map_err(|e| format!("Keychain unavailable: {e}"))?
        .set_password(key.trim())
        .map_err(|e| format!("Could not save to the keychain: {e}"))
}

/// Remove a stored key. Removing one that is not there is not an error — the
/// caller asked for it to be gone, and it is.
pub fn delete(provider: &str) -> Result<(), String> {
    let entry = keyring::Entry::new(SERVICE, provider)
        .map_err(|e| format!("Keychain unavailable: {e}"))?;
    match entry.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(format!("Could not remove from the keychain: {e}")),
    }
}

/// Whether a key is stored, without returning it.
fn is_configured(provider: &str) -> bool {
    keyring::Entry::new(SERVICE, provider)
        .and_then(|e| e.get_password())
        .is_ok()
}

/// Variable names defined in the workspace `.env`, if it has one.
///
/// A deliberately shallow parse: `KEY=` at the start of a line, skipping blanks
/// and comments. It is used only to *warn* about an overlap, so a missed exotic
/// line costs a warning, never correctness.
fn env_file_vars(workspace: &Path) -> Vec<String> {
    let Ok(content) = std::fs::read_to_string(workspace.join(".env")) else {
        return Vec::new();
    };
    content
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                return None;
            }
            line.split_once('=')
                .map(|(k, _)| k.trim().trim_start_matches("export ").trim().to_string())
        })
        .collect()
}

/// Per-provider status for the settings UI.
pub fn status(workspace: &Path) -> Vec<SecretStatus> {
    let in_file = env_file_vars(workspace);
    PROVIDERS
        .iter()
        .map(|(provider, env_var)| SecretStatus {
            provider: (*provider).to_string(),
            env_var: (*env_var).to_string(),
            configured: is_configured(provider),
            also_in_env_file: in_file.iter().any(|v| v == env_var),
        })
        .collect()
}

/// Stored keys as environment variables, for injection into the gateway child.
///
/// This is the only place a key leaves the keychain, and it goes straight into a
/// child process's environment — never across IPC, never to the webview.
pub fn env_overrides() -> HashMap<String, String> {
    let mut out = HashMap::new();
    for (provider, env_var) in PROVIDERS {
        if let Ok(key) = keyring::Entry::new(SERVICE, provider).and_then(|e| e.get_password()) {
            out.insert((*env_var).to_string(), key);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_names_map_to_the_gateway_env_vars() {
        // If these drift from src/config/secrets.rs the shell writes a key the
        // gateway will never look for.
        assert_eq!(env_var_for("openrouter"), Some("OPENROUTER_API_KEY"));
        assert_eq!(env_var_for("zai"), Some("ZAI_API_KEY"));
        assert_eq!(env_var_for("gemini"), Some("GOOGLE_API_KEY"));
        // Ollama takes no key and must not be offered.
        assert_eq!(env_var_for("ollama"), None);
        assert_eq!(env_var_for("nope"), None);
    }

    #[test]
    fn set_rejects_unknown_providers_and_blank_keys() {
        assert!(set("nope", "x").is_err());
        assert!(set("openrouter", "   ").is_err());
    }

    #[test]
    fn env_file_parsing_finds_declared_vars() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(
            tmp.path().join(".env"),
            "# a comment\n\nOPENROUTER_API_KEY=abc\nexport ZAI_API_KEY=def\nMALFORMED\n",
        )
        .unwrap();
        let vars = env_file_vars(tmp.path());
        assert!(vars.contains(&"OPENROUTER_API_KEY".to_string()));
        assert!(vars.contains(&"ZAI_API_KEY".to_string()));
        assert!(!vars.contains(&"MALFORMED".to_string()));
    }

    #[test]
    fn no_env_file_means_no_overlap_warnings() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(env_file_vars(tmp.path()).is_empty());
        assert!(status(tmp.path()).iter().all(|s| !s.also_in_env_file));
    }

    /// Round-trip against the real keychain.
    ///
    /// Uses a unique account each run so the item is always created *and* read
    /// by the same test binary — a fixed name would be read by a later build and
    /// trip macOS's cross-binary ACL prompt, which hangs a headless run.
    #[test]
    fn keychain_round_trips() {
        let account = format!("momo-test-{}", std::process::id());
        let entry = match keyring::Entry::new(SERVICE, &account) {
            Ok(e) => e,
            // No keychain on this machine (headless CI, no D-Bus). Not a failure
            // of this code.
            Err(_) => return,
        };
        if entry.set_password("value-under-test").is_err() {
            return;
        }
        assert_eq!(entry.get_password().unwrap(), "value-under-test");
        entry.delete_credential().unwrap();
        assert!(matches!(entry.get_password(), Err(keyring::Error::NoEntry)));
    }
}
