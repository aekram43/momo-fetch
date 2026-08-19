//! Request/response types for the `/v2` gateway API.
//!
//! `/v1` stays OpenAI-compatible for external tooling. `/v2` is the rich
//! surface the web UI consumes: typed SSE events carrying tool calls and the
//! approval handshake, plus management endpoints.

use serde::{Deserialize, Serialize};

use super::types::ChatMessage;

// ── Chat ───────────────────────────────────────────────────────────────────

/// Body of `POST /v2/chat/stream`.
#[derive(Debug, Clone, Deserialize)]
pub struct V2ChatRequest {
    /// Resume this session before the turn. Omit to use the current session.
    ///
    /// Note: the harness holds a single `current_session_id`, so this switches
    /// global state — see the concurrency notes in `docs/spec/momo-worker.md`.
    #[serde(default)]
    pub session_id: Option<String>,
    /// Conversation messages (OpenAI-compatible). Only the last user message is
    /// used as the prompt — the harness session already holds the history.
    pub messages: Vec<ChatMessage>,
    /// Switch agent personality before this turn.
    #[serde(default)]
    pub agent: Option<String>,
    /// Override the model before this turn.
    #[serde(default)]
    pub model: Option<String>,
}

/// Body of `POST /v2/chat/approve` and `POST /v2/chat/deny`.
///
/// `call_id` and `turn_id` are echoed back from the `approval_required` event
/// so a stale click from a previous turn can be rejected rather than applied to
/// whatever happens to be pending now.
#[derive(Debug, Clone, Deserialize)]
pub struct ApprovalRequest {
    pub turn_id: String,
    #[serde(default)]
    pub call_id: Option<String>,
    pub tool_name: String,
    /// Present on the shared handler; `/approve` and `/deny` set it themselves.
    #[serde(default)]
    pub approved: Option<bool>,
}

/// Body of `POST /v2/chat/interrupt`.
#[derive(Debug, Clone, Deserialize)]
pub struct InterruptRequest {
    #[serde(default)]
    pub turn_id: Option<String>,
}

// ── SSE events ─────────────────────────────────────────────────────────────

/// A typed event on the `/v2/chat/stream` SSE channel.
///
/// The SSE `event:` field carries [`V2StreamEvent::name`]; the `data:` field is
/// the JSON of the inner payload. Clients must ignore unknown event names.
#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum V2StreamEvent {
    Role(RolePayload),
    Text(TextPayload),
    ToolCallStart(ToolCallStartPayload),
    ToolCallResult(ToolCallResultPayload),
    ApprovalRequired(ApprovalRequiredPayload),
    ApprovalResolved(ApprovalResolvedPayload),
    Usage(UsagePayload),
    ContextUsage(ContextUsagePayload),
    Artifacts(ArtifactsPayload),
    Error(ErrorPayload),
    Done(DonePayload),
}

impl V2StreamEvent {
    /// SSE `event:` name for this variant.
    pub fn name(&self) -> &'static str {
        match self {
            Self::Role(_) => "role",
            Self::Text(_) => "text",
            Self::ToolCallStart(_) => "tool_call_start",
            Self::ToolCallResult(_) => "tool_call_result",
            Self::ApprovalRequired(_) => "approval_required",
            Self::ApprovalResolved(_) => "approval_resolved",
            Self::Usage(_) => "usage",
            Self::ContextUsage(_) => "context_usage",
            Self::Artifacts(_) => "artifacts",
            Self::Error(_) => "error",
            Self::Done(_) => "done",
        }
    }

    /// JSON body for the SSE `data:` field.
    pub fn data(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|e| {
            // Serialising our own owned payloads cannot realistically fail, but
            // a silent drop here would look like a truncated stream.
            format!(r#"{{"code":"serialize_failed","message":"{e}"}}"#)
        })
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct RolePayload {
    pub role: String,
    pub model: String,
    pub provider: String,
    pub session_id: String,
    pub agent: Option<String>,
    pub turn_id: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct TextPayload {
    pub content: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ToolCallStartPayload {
    pub id: Option<String>,
    pub name: String,
    pub args: serde_json::Value,
}

#[derive(Debug, Clone, Serialize)]
pub struct ToolCallResultPayload {
    pub id: Option<String>,
    pub name: String,
    /// `"done"` or `"error"`.
    pub status: String,
    pub output_preview: String,
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ApprovalRequiredPayload {
    pub turn_id: String,
    pub call_id: Option<String>,
    pub name: String,
    pub args: serde_json::Value,
    /// Whether the sandbox flagged this as a destructive command.
    pub destructive: bool,
    /// Destructive category from the sandbox, e.g. "Destructive deletion".
    pub category: Option<String>,
    /// Always true today: adk records approval by tool *name* for the life of
    /// the process, so approving grants the tool for the rest of the session.
    /// The UI must say so.
    pub sticky: bool,
    pub expires_at: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ApprovalResolvedPayload {
    pub call_id: Option<String>,
    pub name: String,
    pub approved: bool,
    /// `"user"`, `"timeout"` or `"disconnect"`.
    pub reason: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct UsagePayload {
    pub prompt_tokens: i32,
    pub completion_tokens: i32,
    pub total_tokens: i32,
    pub cost_usd: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ContextUsagePayload {
    pub used: i64,
    pub total: Option<u64>,
    pub percent: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ErrorPayload {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ArtifactsPayload {
    pub files: Vec<ArtifactChange>,
}

/// One path the turn created, modified or deleted.
///
/// Found by comparing the sandbox tree before and after the turn, so it covers
/// writes the agent made through the shell as well as through the file tools —
/// see `crate::artifacts`.
#[derive(Debug, Clone, Serialize)]
pub struct ArtifactChange {
    /// Relative to the project root.
    pub path: String,
    /// `"created"`, `"modified"` or `"deleted"`.
    pub change: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct DonePayload {
    pub turn_id: String,
    /// `"complete"`, `"error"` or `"interrupted"`.
    pub stop_reason: String,
}

// ── Management payloads ────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct AgentSummary {
    pub name: String,
    pub description: Option<String>,
    pub capabilities: Vec<String>,
    pub model: Option<String>,
    pub provider: Option<String>,
    pub is_current: bool,
    pub is_orchestrator: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SwitchAgentRequest {
    pub name: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProviderSummary {
    pub name: String,
    pub is_current: bool,
    /// Model in use — only meaningful for the current provider.
    pub current_model: Option<String>,
    pub default_model: String,
    /// False when no API key is configured; the UI greys these out.
    pub available: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SwitchModelRequest {
    pub model: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SwitchProviderRequest {
    pub provider: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SwitchRequest {
    pub provider: String,
    pub model: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PermissionRequest {
    /// `"strict"`, `"auto"` or `"yolo"`.
    pub mode: String,
}

// ── Errors ─────────────────────────────────────────────────────────────────

/// Uniform `/v2` error body: `{"error":{"code","message","details"}}`.
///
/// `/v1` keeps its OpenAI-shaped errors; this is the shape the web UI switches
/// on, so every `/v2` failure path must produce it.
pub fn v2_error(
    status: axum::http::StatusCode,
    code: &str,
    message: impl Into<String>,
    details: Option<serde_json::Value>,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    let body = serde_json::json!({
        "error": {
            "code": code,
            "message": message.into(),
            "details": details,
        }
    });
    (status, axum::Json(body)).into_response()
}
