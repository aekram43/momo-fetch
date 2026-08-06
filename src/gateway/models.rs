//! `GET /v2/providers/{provider}/models` — the model catalogue for one provider.
//!
//! # Queried, not hardcoded
//!
//! A baked-in list of model names is wrong the week after it is written: models
//! are added, renamed and retired constantly, and a stale list is worse than no
//! list because the UI offers something the API will reject. So each provider is
//! asked for its own catalogue. Most of them are OpenAI-compatible and answer
//! `GET {base}/models`; the three that are not get their own arm.
//!
//! # Failure is a first-class result
//!
//! A provider can be unreachable, rate-limited, or simply not offer a catalogue.
//! That is not an error page — the endpoint returns `available: false` with the
//! reason and an empty list, and the UI falls back to letting the user type a
//! model name. Never leave someone unable to switch models because a directory
//! lookup failed.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;

use super::v2_types::v2_error;
use super::GatewayState;
use crate::config::secrets::SecretStore;

/// Catalogues change slowly; a request should not pay for a refetch each time.
const CACHE_TTL: Duration = Duration::from_secs(30 * 60);

/// Don't let a slow provider hold up the panel.
const FETCH_TIMEOUT: Duration = Duration::from_secs(10);

struct Cached {
    models: Vec<String>,
    at: Instant,
}

fn cache() -> &'static Mutex<HashMap<String, Cached>> {
    static CACHE: OnceLock<Mutex<HashMap<String, Cached>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Base URL for the providers that speak the OpenAI `/models` shape.
///
/// Mirrors the URLs the clients are built with in `providers.rs`; `zai` reads the
/// same env override so a custom deployment is not left out.
fn openai_compatible_base(provider: &str) -> Option<String> {
    Some(match provider {
        "openai" => "https://api.openai.com/v1".to_string(),
        "deepseek" => "https://api.deepseek.com/v1".to_string(),
        "groq" => "https://api.groq.com/openai/v1".to_string(),
        "zai" => std::env::var("ZAI_URL")
            .or_else(|_| std::env::var("ZAI_LLM_URL"))
            .unwrap_or_else(|_| "https://api.z.ai/api/coding/paas/v4".to_string()),
        _ => return None,
    })
}

async fn fetch_models(provider: &str) -> Result<Vec<String>, String> {
    let client = reqwest::Client::builder()
        .timeout(FETCH_TIMEOUT)
        .build()
        .map_err(|e| e.to_string())?;

    match provider {
        // Ollama is local and needs no key; it also answers a different shape.
        "ollama" => {
            let host = std::env::var("OLLAMA_HOST")
                .unwrap_or_else(|_| "http://127.0.0.1:11434".to_string());
            let host = if host.starts_with("http") { host } else { format!("http://{host}") };
            let body: serde_json::Value = client
                .get(format!("{}/api/tags", host.trim_end_matches('/')))
                .send()
                .await
                .map_err(|_| "Ollama is not running.".to_string())?
                .json()
                .await
                .map_err(|e| e.to_string())?;
            Ok(collect(body["models"].as_array(), "name"))
        }

        "anthropic" => {
            let key = SecretStore::get(provider).map_err(|e| e.to_string())?;
            let body: serde_json::Value = client
                .get("https://api.anthropic.com/v1/models?limit=100")
                .header("x-api-key", key)
                // Required by the API; omitting it is a 400, not a default.
                .header("anthropic-version", "2023-06-01")
                .send()
                .await
                .map_err(|e| e.to_string())?
                .json()
                .await
                .map_err(|e| e.to_string())?;
            Ok(collect(body["data"].as_array(), "id"))
        }

        "gemini" => {
            let key = SecretStore::get(provider).map_err(|e| e.to_string())?;
            let body: serde_json::Value = client
                .get(format!(
                    "https://generativelanguage.googleapis.com/v1beta/models?key={key}"
                ))
                .send()
                .await
                .map_err(|e| e.to_string())?
                .json()
                .await
                .map_err(|e| e.to_string())?;
            // Gemini prefixes every name with `models/`; strip it so the value
            // matches what the switch endpoint expects.
            Ok(collect(body["models"].as_array(), "name")
                .into_iter()
                .map(|n| n.trim_start_matches("models/").to_string())
                .collect())
        }

        // OpenRouter has its own client already in the tree, used for context
        // windows — reuse it rather than hand-rolling the same request.
        "openrouter" => {
            let key = SecretStore::get(provider).map_err(|e| e.to_string())?;
            let config = adk_model::openrouter::OpenRouterConfig::new(&key, "unused");
            let c = adk_model::openrouter::OpenRouterClient::new(config)
                .map_err(|e| e.to_string())?;
            let models = c.list_models().await.map_err(|e| e.to_string())?;
            Ok(models.into_iter().map(|m| m.id).collect())
        }

        _ => {
            let base = openai_compatible_base(provider)
                .ok_or_else(|| format!("No model catalogue is known for '{provider}'."))?;
            let key = SecretStore::get(provider).map_err(|e| e.to_string())?;
            let body: serde_json::Value = client
                .get(format!("{}/models", base.trim_end_matches('/')))
                .bearer_auth(key)
                .send()
                .await
                .map_err(|e| e.to_string())?
                .json()
                .await
                .map_err(|e| e.to_string())?;
            Ok(collect(body["data"].as_array(), "id"))
        }
    }
}

/// Pull one string field out of a JSON array, skipping entries that lack it.
fn collect(arr: Option<&Vec<serde_json::Value>>, field: &str) -> Vec<String> {
    let mut out: Vec<String> = arr
        .map(|a| {
            a.iter()
                .filter_map(|m| m[field].as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    out.sort();
    out.dedup();
    out
}

pub async fn v2_provider_models(
    State(state): State<GatewayState>,
    Path(provider): Path<String>,
) -> Response {
    // Serve a warm cache without touching the network.
    if let Ok(c) = cache().lock() {
        if let Some(hit) = c.get(&provider) {
            if hit.at.elapsed() < CACHE_TTL {
                return ok(&provider, hit.models.clone(), true);
            }
        }
    }

    // The current model is always offered, even when the catalogue is missing,
    // so the dropdown can show what is actually in use rather than an empty box.
    let current = {
        let harness = state.harness.read().await;
        harness.provider_mgr().current_model_name().to_string()
    };

    match fetch_models(&provider).await {
        Ok(mut models) => {
            // Front, not sorted in: providers return their catalogue in an
            // order that means something — OpenRouter leads with the popular
            // models — and the picker pads its short list from the head of it.
            // Re-sorting only in this branch would make that padding depend on
            // whether the current model happened to be listed.
            if !models.iter().any(|m| *m == current) {
                models.insert(0, current);
            }
            if let Ok(mut c) = cache().lock() {
                c.insert(
                    provider.clone(),
                    Cached { models: models.clone(), at: Instant::now() },
                );
            }
            ok(&provider, models, false)
        }
        // Deliberately 200, not an error status: "we could not list them" is a
        // normal state the UI renders as a free-text field, not a failure that
        // should surface as a red toast.
        Err(reason) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "provider": provider,
                "models": [current],
                "available": false,
                "cached": false,
                "error": reason,
            })),
        )
            .into_response(),
    }
}

fn ok(provider: &str, models: Vec<String>, cached: bool) -> Response {
    (
        StatusCode::OK,
        Json(serde_json::json!({
            "provider": provider,
            "models": models,
            "available": true,
            "cached": cached,
            "error": serde_json::Value::Null,
        })),
    )
        .into_response()
}

/// `DELETE /v2/providers/{provider}/models` — drop the cached catalogue.
///
/// For the refresh affordance in the UI: a user who just added a model to their
/// OpenRouter account should not wait out the TTL.
pub async fn v2_provider_models_refresh(Path(provider): Path<String>) -> Response {
    if let Ok(mut c) = cache().lock() {
        c.remove(&provider);
    } else {
        return v2_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal",
            "Could not clear the model cache.",
            None,
        );
    }
    (StatusCode::OK, Json(serde_json::json!({ "cleared": provider }))).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn openai_compatible_providers_have_a_base_url() {
        for p in ["openai", "deepseek", "groq", "zai"] {
            assert!(openai_compatible_base(p).is_some(), "{p} needs a base URL");
        }
        // These take their own arm in `fetch_models`; a base URL here would mean
        // they are about to be queried with the wrong shape.
        for p in ["anthropic", "gemini", "ollama", "openrouter"] {
            assert!(openai_compatible_base(p).is_none(), "{p} must not be treated as OpenAI-shaped");
        }
    }

    #[test]
    fn collect_pulls_the_named_field_and_sorts() {
        let v: serde_json::Value = serde_json::json!({
            "data": [{"id": "b"}, {"id": "a"}, {"no_id": 1}, {"id": "a"}]
        });
        assert_eq!(collect(v["data"].as_array(), "id"), vec!["a", "b"]);
    }

    #[test]
    fn collect_handles_a_missing_array() {
        let v: serde_json::Value = serde_json::json!({});
        assert!(collect(v["nope"].as_array(), "id").is_empty());
    }
}
