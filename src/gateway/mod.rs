mod auth;
mod files;
mod handlers;
mod turn;
mod types;
mod v2_handlers;
mod v2_types;

use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;

use axum::{
    extract::State,
    middleware,
    routing::{get, post},
    Router,
};
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use tower_http::cors::CorsLayer;

use crate::config::HarnessConfig;
use crate::harness::Harness;

/// Gateway configuration loaded from `.harness/gateway.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewayConfig {
    /// Port to listen on (default: 3000).
    #[serde(default = "default_port")]
    pub port: u16,
    /// Allowed CORS origins. `"*"` for all.
    #[serde(default = "default_cors")]
    pub cors_origins: Vec<String>,
    /// Authentication configuration.
    #[serde(default)]
    pub auth: AuthConfig,
    /// How long a tool approval may stay pending before it is auto-denied.
    #[serde(default = "default_approval_timeout")]
    pub approval_timeout_secs: u64,
    /// Directory served at `/ui/*` (G9).
    ///
    /// Relative paths resolve against the project root. The default matches
    /// Next.js static export (`output: 'export'` writes `out/`, not `dist/` —
    /// spec C12). Configurable because in development the binary and the build
    /// output are not co-located.
    #[serde(default = "default_ui_dir")]
    pub ui_dir: String,
}

fn default_port() -> u16 {
    3000
}

/// No cross-origin access by default.
///
/// The bundled UI is served from the gateway's own origin at `/ui` (G9) and
/// resolves same-origin, so it needs no CORS grant. Anything else must opt in
/// explicitly — see the refusal in [`run`] for why `["*"]` is not a safe
/// default here.
fn default_cors() -> Vec<String> {
    Vec::new()
}

fn default_approval_timeout() -> u64 {
    300
}

fn default_ui_dir() -> String {
    "web/out".to_string()
}

// Derived `Default` would zero these fields, which is not the same as the serde
// defaults used when the file exists — a missing `.harness/gateway.json` would
// bind port 0 and build an empty CORS allow-list. Keep the two in step.
impl Default for GatewayConfig {
    fn default() -> Self {
        Self {
            port: default_port(),
            cors_origins: default_cors(),
            auth: AuthConfig::default(),
            approval_timeout_secs: default_approval_timeout(),
            ui_dir: default_ui_dir(),
        }
    }
}

/// Authentication configuration.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AuthConfig {
    /// Enable API key authentication.
    #[serde(default)]
    pub enabled: bool,
    /// API key definitions: key → metadata.
    #[serde(default)]
    pub keys: HashMap<String, ApiKeyMeta>,
}

/// Per-key metadata (rate limit, daily quota, description).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiKeyMeta {
    /// Human-readable name for this key.
    #[serde(default)]
    pub name: String,
    /// Max requests per minute (0 = unlimited).
    #[serde(default)]
    pub rate_limit: u32,
    /// Max requests per day (0 = unlimited).
    #[serde(default)]
    pub daily_quota: u32,
}

/// Shared gateway state passed to all handlers.
#[derive(Clone)]
pub struct GatewayState {
    pub harness: Arc<RwLock<Harness>>,
    pub config: GatewayConfig,
    pub rate_limiter: Arc<auth::RateLimiter>,
    /// Admits one turn at a time and brokers tool approvals.
    pub turns: turn::TurnRegistry,
}

/// Runtime overrides for where the gateway listens, from CLI flags.
///
/// Tauri needs a free port chosen at launch and must not rewrite the user's
/// `.harness/gateway.json` to get one.
#[derive(Debug, Clone, Default)]
pub struct BindOverrides {
    pub port: Option<u16>,
    pub bind: Option<IpAddr>,
}

/// Load gateway config from `.harness/gateway.json`.
pub fn load_gateway_config(project_path: &std::path::Path) -> GatewayConfig {
    let config_path = project_path.join(".harness/gateway.json");
    if config_path.exists() {
        let content = std::fs::read_to_string(&config_path).unwrap_or_default();
        serde_json::from_str(&content).unwrap_or_default()
    } else {
        GatewayConfig::default()
    }
}

/// Start the gateway HTTP server.
pub async fn run(config: HarnessConfig, overrides: BindOverrides) -> anyhow::Result<()> {
    let project_path = config.project_path.clone();
    let gateway_config = load_gateway_config(&project_path);

    let bind_ip = overrides
        .bind
        .unwrap_or_else(|| IpAddr::from([127, 0, 0, 1]));
    let port = overrides.port.unwrap_or(gateway_config.port);

    // The gateway exposes shell execution and filesystem access. On loopback
    // that is the user's own machine; on any other interface it is remote code
    // execution for anyone who can reach the port. Refuse rather than warn.
    if !bind_ip.is_loopback() && !gateway_config.auth.enabled {
        anyhow::bail!(
            "Refusing to bind {bind_ip} without authentication.\n\
             The gateway can execute shell commands and read your filesystem.\n\
             Enable auth in .harness/gateway.json (auth.enabled = true, with keys) \
             or bind 127.0.0.1."
        );
    }

    let addr = SocketAddr::new(bind_ip, port);

    // Build the harness
    let harness = Harness::build(config).await?;
    let harness = Arc::new(RwLock::new(harness));

    // Build rate limiter from config
    let rate_limiter = Arc::new(auth::RateLimiter::new(
        gateway_config.auth.enabled,
        &gateway_config.auth.keys,
    ));

    let state = GatewayState {
        harness,
        config: gateway_config.clone(),
        rate_limiter,
        turns: turn::TurnRegistry::new(),
    };

    // `*` plus no auth is remote code execution from any page the user visits.
    //
    // `CorsLayer::permissive()` sends `Access-Control-Allow-Origin: *`, so a
    // hostile origin does not merely *send* requests — it can read the replies.
    // With auth disabled that grants any visited website `GET /v2/files` over
    // the whole working tree, and `POST /v2/settings/permission {"mode":"yolo"}`
    // followed by `POST /v2/chat/stream` to run shell commands. Loopback does
    // not help: the browser is already inside the trust boundary.
    //
    // Same fail-fast shape as the non-loopback bind check above.
    if gateway_config.cors_origins.iter().any(|o| o == "*") && !gateway_config.auth.enabled {
        anyhow::bail!(
            "Refusing to start with cors_origins = [\"*\"] while authentication is disabled.\n\
             That combination lets any website the user visits read files through \
             /v2/files and drive the agent through /v2/chat/stream.\n\
             Either list the origins you actually need in .harness/gateway.json, \
             or enable auth (auth.enabled = true, with keys).\n\
             The bundled UI at /ui is same-origin and needs no CORS entry."
        );
    }

    // Build CORS layer
    let cors: CorsLayer = if gateway_config.cors_origins.iter().any(|o| o == "*") {
        CorsLayer::permissive()
    } else if gateway_config.cors_origins.is_empty() {
        // Same-origin only: no CORS headers, so browsers refuse cross-origin
        // reads. The `/ui` bundle is unaffected.
        CorsLayer::new()
    } else {
        let origins: Vec<axum::http::HeaderValue> = gateway_config
            .cors_origins
            .iter()
            .filter_map(|o| o.parse().ok())
            .collect();
        use axum::http::{Method, HeaderValue};
        use tower_http::cors::{Any, AllowOrigin};
        CorsLayer::new()
            .allow_origin(AllowOrigin::list(origins))
            .allow_methods([Method::GET, Method::POST, Method::OPTIONS, Method::DELETE])
            .allow_headers([axum::http::header::CONTENT_TYPE, axum::http::header::AUTHORIZATION])
    };

    // Authenticated surface: everything that reads or drives the harness.
    let authed = Router::new()
        .route("/v1/chat/completions", post(handlers::chat_completions))
        .route("/v1/chat/completions/stream", post(handlers::chat_completions_stream))
        .route("/v1/sessions", get(handlers::list_sessions).post(handlers::create_session))
        .route("/v1/sessions/{session_id}", get(handlers::get_session).delete(handlers::delete_session))
        .route("/v1/sessions/{session_id}/compact", post(handlers::compact_session))
        .route("/v1/models", get(handlers::list_models))
        .route("/v1/cost", get(handlers::get_cost))
        .route("/v2/chat/stream", post(v2_handlers::v2_chat_stream))
        .route("/v2/chat/approve", post(v2_handlers::v2_chat_approve))
        .route("/v2/chat/deny", post(v2_handlers::v2_chat_deny))
        .route("/v2/chat/interrupt", post(v2_handlers::v2_chat_interrupt))
        .route("/v2/agents", get(v2_handlers::v2_agents))
        .route("/v2/agents/switch", post(v2_handlers::v2_agents_switch))
        .route("/v2/agents/default", post(v2_handlers::v2_agents_default))
        .route("/v2/providers", get(v2_handlers::v2_providers))
        .route("/v2/switch-model", post(v2_handlers::v2_switch_model))
        .route("/v2/switch-provider", post(v2_handlers::v2_switch_provider))
        .route("/v2/switch", post(v2_handlers::v2_switch))
        .route("/v2/settings", get(v2_handlers::v2_settings))
        .route("/v2/settings/permission", post(v2_handlers::v2_settings_permission))
        .route(
            "/v2/settings/approved-tools",
            axum::routing::delete(v2_handlers::v2_settings_clear_approvals),
        )
        // R1 — surface registered ahead of implementation so the remaining
        // gateway tasks don't all contend on this file, and so the frontend can
        // build against real HTTP. Each returns 501 in the standard error
        // shape. Swap the handler, not the route. See spec §12.3.
        .route("/v2/mcp/servers", get(v2_handlers::v2_mcp_servers))
        .route("/v2/memory/search", get(v2_handlers::v2_memory_search))
        .route("/v2/memory/stats", get(v2_handlers::v2_memory_stats))
        .route("/v2/files", get(files::v2_files))
        .route("/v2/files/tree", get(files::v2_files_tree))
        .route(
            "/v2/sessions/{session_id}/messages",
            get(v2_handlers::v2_session_messages),
        )
        .layer(middleware::from_fn_with_state(state.clone(), auth::auth_middleware));

    // `/health` stays unauthenticated: it is the readiness probe the desktop
    // shell polls before it has a key, and a container health check that 401s
    // is worse than useless. It exposes no session content.
    let mut public = Router::new().route("/health", get(handlers::health));

    // G9 — serve the built UI at /ui/*.
    //
    // Also auth-exempt, and for a harder reason than /health: a browser
    // navigating to a page cannot attach an Authorization header, so an HTML
    // shell behind a bearer token is unreachable by construction. The token
    // still guards every /v1 and /v2 call the loaded app makes.
    //
    // Only the static bundle is exposed here — no harness state — and G6's
    // deny-list is unaffected, since ServeDir is rooted at the build output
    // rather than the project.
    let ui_dir = {
        let configured = std::path::Path::new(&gateway_config.ui_dir);
        if configured.is_absolute() {
            configured.to_path_buf()
        } else {
            project_path.join(configured)
        }
    };
    if ui_dir.is_dir() {
        use tower_http::services::{ServeDir, ServeFile};
        // SPA fallback: client-side routes like /ui/settings have no file on
        // disk, so anything unmatched resolves to index.html and lets the
        // router take over. Without this, a refresh on any sub-route 404s.
        let index = ui_dir.join("index.html");
        let serve = ServeDir::new(&ui_dir).fallback(ServeFile::new(&index));
        public = public.nest_service("/ui", serve);
        tracing::info!("🖥️  Serving UI from {} at /ui", ui_dir.display());
    } else {
        // Not an error: the gateway is useful headless, and the frontend may
        // simply not be built yet. Say so once rather than 404ing silently.
        tracing::info!(
            "UI directory {} not found — /ui is disabled. \
             Build the frontend (npm run build in web/) or set ui_dir in \
             .harness/gateway.json.",
            ui_dir.display()
        );
    }

    let app = authed.merge(public).layer(cors).with_state(state);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    // With `--gateway-port 0` the OS picks the port, so report what we actually
    // got rather than what was asked for.
    let bound = listener.local_addr().unwrap_or(addr);
    let url = format!("http://{bound}");

    tracing::info!("🚀 Gateway listening on {url}");
    // Machine-readable first line: supervisors (Tauri) parse this instead of
    // guessing the port. Keep the format stable.
    println!("MOMO_GATEWAY_LISTENING {url}");
    println!("🚀 MOMO Gateway listening on {url}");
    println!("   Endpoints:");
    println!("     POST /v1/chat/completions           — Chat (OpenAI-compatible)");
    println!("     POST /v1/chat/completions/stream    — Chat (SSE streaming)");
    println!("     GET  /v1/sessions                    — List sessions");
    println!("     POST /v1/sessions                    — Create session");
    println!("     GET  /v1/sessions/:id                — Get session");
    println!("     DELETE /v1/sessions/:id             — Delete session");
    println!("     POST /v1/sessions/:id/compact       — Compact session");
    println!("     GET  /v1/models                      — List available models");
    println!("     GET  /v1/cost                        — Cost tracking");
    println!("     POST /v2/chat/stream                 — Chat (rich SSE: tools + approval)");
    println!("     POST /v2/chat/approve|deny           — Resolve a tool approval");
    println!("     POST /v2/chat/interrupt              — Interrupt the active turn");
    println!("     GET  /v2/agents                      — List agent personalities");
    println!("     POST /v2/agents/switch|default       — Switch agent");
    println!("     GET  /v2/providers                   — List providers + models");
    println!("     POST /v2/switch-model|switch-provider — Switch model/provider");
    println!("     GET  /health                         — Health check (no auth)");

    axum::serve(listener, app).await?;

    Ok(())
}


