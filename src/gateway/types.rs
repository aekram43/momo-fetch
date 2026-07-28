//! OpenAI-compatible request/response types.

use serde::{Deserialize, Serialize};

/// Chat message in OpenAI format.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

/// Chat completion request (OpenAI-compatible).
#[derive(Debug, Clone, Deserialize)]
pub struct ChatCompletionRequest {
    /// Model name (ignored — momo uses its own model, but kept for compatibility).
    #[serde(default = "default_model")]
    pub model: String,
    /// Messages in the conversation.
    pub messages: Vec<ChatMessage>,
    /// Session ID to continue a session. Omit for new session.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    /// Stream the response.
    #[serde(default)]
    pub stream: bool,
    /// Temperature (passed through, currently informational only).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    /// Max tokens (passed through, currently informational only).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u64>,
    /// Stop sequences.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop: Option<Vec<String>>,
    /// User identifier.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,
}

fn default_model() -> String {
    "momo-fetch".to_string()
}

/// Chat completion response (OpenAI-compatible).
#[derive(Debug, Clone, Serialize)]
pub struct ChatCompletionResponse {
    pub id: String,
    pub object: String,
    pub created: u64,
    pub model: String,
    pub choices: Vec<ChatChoice>,
    pub usage: Usage,
}

/// A single choice in the response.
#[derive(Debug, Clone, Serialize)]
pub struct ChatChoice {
    pub index: usize,
    pub message: ChatMessage,
    pub finish_reason: String,
}

/// Token usage stats.
#[derive(Debug, Clone, Serialize)]
pub struct Usage {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
}

/// SSE streaming chunk (OpenAI-compatible).
#[derive(Debug, Clone, Serialize)]
pub struct ChatCompletionChunk {
    pub id: String,
    pub object: String,
    pub created: u64,
    pub model: String,
    pub choices: Vec<StreamChoice>,
}

/// A streaming choice.
#[derive(Debug, Clone, Serialize)]
pub struct StreamChoice {
    pub index: usize,
    pub delta: StreamDelta,
    pub finish_reason: Option<String>,
}

/// Delta content for streaming.
#[derive(Debug, Clone, Serialize)]
pub struct StreamDelta {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
}

/// Session info returned by the sessions API.
#[derive(Debug, Clone, Serialize)]
pub struct SessionInfo {
    pub id: String,
    pub created_at: String,
    pub event_count: Option<usize>,
}

/// Model info.
#[derive(Debug, Clone, Serialize)]
pub struct ModelInfo {
    pub id: String,
    pub provider: String,
    pub current: bool,
}

/// Error response.
#[derive(Debug, Clone, Serialize)]
pub struct ErrorResponse {
    pub error: ErrorDetail,
}

/// Error detail.
#[derive(Debug, Clone, Serialize)]
pub struct ErrorDetail {
    pub message: String,
    pub r#type: String,
    pub code: Option<String>,
}

impl ErrorResponse {
    pub fn new(msg: &str, code: Option<&str>) -> serde_json::Value {
        serde_json::json!({
            "error": {
                "message": msg,
                "type": "invalid_request_error",
                "code": code
            }
        })
    }

    pub fn rate_limit() -> serde_json::Value {
        serde_json::json!({
            "error": {
                "message": "Rate limit exceeded",
                "type": "rate_limit_error",
                "code": "rate_limit_exceeded"
            }
        })
    }

    pub fn unauthorized() -> serde_json::Value {
        serde_json::json!({
            "error": {
                "message": "Invalid or missing API key",
                "type": "authentication_error",
                "code": "invalid_api_key"
            }
        })
    }
}

/// Convert a list of ChatMessages to a single prompt string for the harness.
/// Takes only the last user message (or system+user) to avoid re-sending
/// the full history (the harness session already has context).
pub fn messages_to_prompt(messages: &[ChatMessage]) -> String {
    // Use the last user message as the prompt
    for msg in messages.iter().rev() {
        if msg.role == "user" {
            return match &msg.content {
                serde_json::Value::String(s) => s.clone(),
                other => other.to_string(),
            };
        }
    }
    // Fallback: join all messages
    messages
        .iter()
        .map(|m| format!("{}: {}", m.role, m.content))
        .collect::<Vec<_>>()
        .join("\n")
}
