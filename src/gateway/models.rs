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
//!
//! # Models the user adds
//!
//! Catalogues lag: a provider ships a model before its `/models` lists it, and
//! some endpoints (Z.ai's coding plan) never list everything they serve. So the
//! user can add names by hand. They live in the global settings file under
//! `custom_models`, keyed by provider, and are merged into every response —
//! cached or not, catalogue or no catalogue — so an added model is offered from
//! the moment it is saved.

use std::collections::HashMap;
use std::path::{Path as FsPath, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::{Map, Value};

use super::v2_types::v2_error;
use super::GatewayState;
use crate::config::secrets::SecretStore;

/// Catalogues change slowly; a request should not pay for a refetch each time.
const CACHE_TTL: Duration = Duration::from_secs(30 * 60);

/// Don't let a slow provider hold up the panel.
const FETCH_TIMEOUT: Duration = Duration::from_secs(10);

/// Longer than any real model id, short enough to keep junk out of settings.
const MAX_MODEL_NAME: usize = 200;

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

/// Put added models the catalogue does not list at the front.
///
/// Front, because the picker's closed list pads from the head: a model someone
/// went to the trouble of adding should not be hidden behind a search. One the
/// provider has since started listing stays where the provider put it.
fn with_custom(mut models: Vec<String>, custom: &[String]) -> Vec<String> {
    let missing: Vec<String> = custom
        .iter()
        .filter(|c| !models.contains(c))
        .cloned()
        .collect();
    models.splice(0..0, missing);
    models
}

pub async fn v2_provider_models(
    State(state): State<GatewayState>,
    Path(provider): Path<String>,
) -> Response {
    // Read on every request, never cached: an add must show on the next open.
    let custom = custom_models(&settings_path(), &provider);

    // Serve a warm cache without touching the network.
    if let Ok(c) = cache().lock() {
        if let Some(hit) = c.get(&provider) {
            if hit.at.elapsed() < CACHE_TTL {
                let models = with_custom(hit.models.clone(), &custom);
                return ok(&provider, models, &custom, true);
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
            ok(&provider, with_custom(models, &custom), &custom, false)
        }
        // Deliberately 200, not an error status: "we could not list them" is a
        // normal state the UI renders as a free-text field, not a failure that
        // should surface as a red toast.
        Err(reason) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "provider": provider,
                "models": with_custom(vec![current], &custom),
                "custom": custom,
                "available": false,
                "cached": false,
                "error": reason,
            })),
        )
            .into_response(),
    }
}

fn ok(provider: &str, models: Vec<String>, custom: &[String], cached: bool) -> Response {
    (
        StatusCode::OK,
        Json(serde_json::json!({
            "provider": provider,
            "models": models,
            "custom": custom,
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

// ─── Models the user adds ──────────────────────────────────────────

#[derive(serde::Deserialize)]
pub struct CustomModelRequest {
    pub model: String,
}

/// `POST /v2/providers/{provider}/models/custom` — offer a model the catalogue
/// does not list.
///
/// Only remembers the name; it does not switch to it. Whether the provider
/// actually serves it is found out by switching, the same as a typed name.
pub async fn v2_custom_model_add(
    Path(provider): Path<String>,
    Json(req): Json<CustomModelRequest>,
) -> Response {
    let model = match check_names(&provider, &req.model) {
        Ok(model) => model,
        Err(msg) => return v2_error(StatusCode::BAD_REQUEST, "invalid_request", msg, None),
    };
    let result = edit_custom_models(&settings_path(), &provider, |list| {
        if !list.contains(&model) {
            list.push(model.clone());
        }
    });
    custom_response(&provider, result)
}

/// `POST /v2/providers/{provider}/models/custom/remove` — stop offering one.
///
/// A POST rather than a DELETE on `…/custom/{model}`: OpenRouter ids contain
/// `/`, which does not survive as a path segment.
pub async fn v2_custom_model_remove(
    Path(provider): Path<String>,
    Json(req): Json<CustomModelRequest>,
) -> Response {
    let model = req.model.trim().to_string();
    let result = edit_custom_models(&settings_path(), &provider, |list| {
        list.retain(|m| *m != model);
    });
    custom_response(&provider, result)
}

fn custom_response(provider: &str, result: Result<Vec<String>, String>) -> Response {
    match result {
        Ok(list) => (
            StatusCode::OK,
            Json(serde_json::json!({ "provider": provider, "custom": list })),
        )
            .into_response(),
        Err(e) => v2_error(StatusCode::INTERNAL_SERVER_ERROR, "internal", e, None),
    }
}

/// The trimmed model name, or why it cannot be saved.
fn check_names(provider: &str, model: &str) -> Result<String, String> {
    if provider.is_empty()
        || !provider.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err(format!("'{provider}' is not a provider name."));
    }
    let model = model.trim();
    if model.is_empty() {
        return Err("Model name is empty.".into());
    }
    if model.len() > MAX_MODEL_NAME {
        return Err(format!("Model name is longer than {MAX_MODEL_NAME} characters."));
    }
    if model.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err("Model names have no spaces.".into());
    }
    Ok(model.to_string())
}

/// The global settings file — the same one `HarnessConfig` reads.
///
/// Global rather than per project: which models an account can reach does not
/// change with the folder it is opened in.
fn settings_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("momo-fetch")
        .join("settings.json")
}

/// The settings file as a JSON object; a missing or empty file is an empty one.
///
/// Read as a raw object, not `SettingsFile`, so a rewrite keeps every key this
/// build does not know about. Unparseable is an error, not a default: the
/// harness tolerates a broken file by ignoring it, but writing one back would
/// replace whatever the user had with a single `custom_models` key.
fn read_settings(path: &FsPath) -> Result<Map<String, Value>, String> {
    let content = match std::fs::read_to_string(path) {
        Ok(content) => content,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Map::new()),
        Err(e) => return Err(format!("Could not read {}: {e}", path.display())),
    };
    if content.trim().is_empty() {
        return Ok(Map::new());
    }
    match serde_json::from_str::<Value>(&content) {
        Ok(Value::Object(map)) => Ok(map),
        Ok(_) => Err(format!("{} is not a JSON object.", path.display())),
        Err(e) => Err(format!("{} is not valid JSON ({e}). Fix it first.", path.display())),
    }
}

fn list_of(settings: &Map<String, Value>, provider: &str) -> Vec<String> {
    settings
        .get("custom_models")
        .and_then(|all| all.get(provider))
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
        .unwrap_or_default()
}

fn custom_models(path: &FsPath, provider: &str) -> Vec<String> {
    read_settings(path).map(|s| list_of(&s, provider)).unwrap_or_default()
}

/// Change one provider's list and write the file back, returning the new list.
fn edit_custom_models(
    path: &FsPath,
    provider: &str,
    change: impl FnOnce(&mut Vec<String>),
) -> Result<Vec<String>, String> {
    // Two tabs adding at once must not each write back the file it read.
    static LOCK: Mutex<()> = Mutex::new(());
    let _guard = LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());

    let mut settings = read_settings(path)?;
    let mut list = list_of(&settings, provider);
    change(&mut list);

    let all = settings
        .entry("custom_models")
        .or_insert_with(|| Value::Object(Map::new()));
    let Some(all) = all.as_object_mut() else {
        return Err(format!("`custom_models` in {} is not an object.", path.display()));
    };
    // `shift_remove` keeps the file's key order; `remove` would swap the last
    // key into the gap.
    if list.is_empty() {
        all.shift_remove(provider);
    } else {
        all.insert(provider.to_string(), serde_json::json!(list));
    }
    if all.is_empty() {
        settings.shift_remove("custom_models");
    }

    write_atomic(path, &Value::Object(settings))
        .map_err(|e| format!("Could not write {}: {e}", path.display()))?;
    Ok(list)
}

/// Write via a temp file and rename, so the harness never reads half a file.
fn write_atomic(path: &FsPath, value: &Value) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("json.tmp");
    let mut body = serde_json::to_string_pretty(value).map_err(std::io::Error::other)?;
    body.push('\n');
    std::fs::write(&tmp, body)?;
    std::fs::rename(&tmp, path)
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

    fn strings(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn added_models_the_catalogue_lacks_go_first() {
        let models = with_custom(strings(&["glm-4.5", "glm-4.6"]), &strings(&["glm-5.3", "glm-4.6"]));
        // glm-4.6 is already listed, so it keeps the provider's position.
        assert_eq!(models, strings(&["glm-5.3", "glm-4.5", "glm-4.6"]));
    }

    #[test]
    fn model_names_are_trimmed_and_checked() {
        assert_eq!(check_names("zai", "  glm-5.3 ").unwrap(), "glm-5.3");
        assert_eq!(check_names("openrouter", "anthropic/claude").unwrap(), "anthropic/claude");
        assert!(check_names("zai", "   ").is_err());
        assert!(check_names("zai", "glm 5").is_err());
        assert!(check_names("zai", &"x".repeat(MAX_MODEL_NAME + 1)).is_err());
        assert!(check_names("../etc", "glm-5.3").is_err());
    }

    #[test]
    fn adding_creates_the_file_and_does_not_duplicate() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("momo-fetch/settings.json");
        let add = |m: &str| {
            let m = m.to_string();
            edit_custom_models(&path, "zai", move |l| if !l.contains(&m) { l.push(m) }).unwrap()
        };
        add("glm-5.3");
        assert_eq!(add("glm-5.3"), strings(&["glm-5.3"]));
        assert_eq!(custom_models(&path, "zai"), strings(&["glm-5.3"]));
        assert!(custom_models(&path, "openai").is_empty());
    }

    #[test]
    fn editing_keeps_every_other_key_in_order() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        std::fs::write(
            &path,
            r#"{"// note": "keep me", "default_provider": "zai", "memory": {"auto_write": false}}"#,
        )
        .unwrap();

        edit_custom_models(&path, "zai", |l| l.push("glm-5.3".into())).unwrap();
        let raw = std::fs::read_to_string(&path).unwrap();
        let keys: Vec<String> = serde_json::from_str::<Map<String, Value>>(&raw)
            .unwrap()
            .keys()
            .cloned()
            .collect();
        assert_eq!(keys, strings(&["// note", "default_provider", "memory", "custom_models"]));

        // Removing the last one takes the whole key with it.
        edit_custom_models(&path, "zai", |l| l.clear()).unwrap();
        let after: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert!(after.get("custom_models").is_none());
        assert_eq!(after["memory"]["auto_write"], false);
    }

    #[test]
    fn a_broken_settings_file_is_never_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        std::fs::write(&path, "{ not json").unwrap();

        assert!(edit_custom_models(&path, "zai", |l| l.push("glm-5.3".into())).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{ not json");
        assert!(custom_models(&path, "zai").is_empty());
    }
}
