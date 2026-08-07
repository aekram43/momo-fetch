//! Gateway HTTP handlers.

use std::pin::Pin;
use std::sync::Arc;

use adk_rust::futures::StreamExt;
use adk_rust::Part;
use axum::{
    extract::{Path, State},
    response::{sse::{Event, KeepAlive, Sse}, IntoResponse},
    Json,
};
use futures::Stream;
use tokio::sync::RwLock;

use super::types::*;
use crate::gateway::GatewayState;

/// Extract text from the last user message in a ChatCompletion request.
fn extract_prompt(req: &ChatCompletionRequest) -> String {
    messages_to_prompt(&req.messages)
}

/// POST /v1/chat/completions — Non-streaming chat.
pub async fn chat_completions(
    State(state): State<GatewayState>,
    Json(req): Json<ChatCompletionRequest>,
) -> impl IntoResponse {
    let prompt = extract_prompt(&req);

    // Turns are process-wide (one harness, one current session), so v1 has to
    // take the same lease as v2. Without it a v1 request would queue on the
    // write lock behind an in-flight v2 turn and stall the whole gateway.
    let _lease = match state.turns.try_begin(req.session_id.as_deref().unwrap_or_default()) {
        Ok(lease) => lease,
        Err(_) => {
            return (
                axum::http::StatusCode::CONFLICT,
                Json(ErrorResponse::new(
                    "A turn is already running.",
                    Some("turn_in_progress"),
                )),
            )
                .into_response();
        }
    };

    // If session_id provided, resume that session
    if let Some(ref session_id) = req.session_id {
        let mut harness = state.harness.write().await;
        if let Err(e) = harness.resume_session(session_id).await {
            return (
                axum::http::StatusCode::BAD_REQUEST,
                Json(ErrorResponse::new(&format!("Failed to resume session: {e}"), None)),
            )
                .into_response();
        }
    }

    let harness = state.harness.read().await;
    let session_id = harness.current_session_id().to_string();
    let model_name = harness.provider_mgr().current_model_name().to_string();
    let provider_name = harness.provider_mgr().current_provider().to_string();

    harness.begin_turn();
    match harness.run_turn_enriched(&prompt).await {
        Ok((_enriched, stream)) => {
            let (text, _tool_count, usage) = consume_stream(&harness, stream).await;
            harness.end_turn();

            // Post-turn memory write
            if harness.memory_sidecar().auto_write_enabled() {
                let project_name = harness
                    .config()
                    .project_path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default();
                let turn_summary = crate::memory::sidecar::TurnSummary {
                    user_message: prompt.clone(),
                    tool_calls: Vec::new(),
                    response_preview: text.clone(),
                    project: project_name,
                };
                let _ = harness.memory_sidecar().write_turn_memory(
                    &turn_summary,
                    harness.provider_mgr(),
                    Some(&harness.mailbox_path()),
                    None,
                );
            }

            let response = ChatCompletionResponse {
                id: format!("chatcmpl-{session_id}"),
                object: "chat.completion".to_string(),
                created: chrono::Utc::now().timestamp() as u64,
                model: format!("{provider_name}/{model_name}"),
                choices: vec![ChatChoice {
                    index: 0,
                    message: ChatMessage {
                        role: "assistant".to_string(),
                        content: serde_json::Value::String(text),
                        name: None,
                    },
                    finish_reason: "stop".to_string(),
                }],
                usage,
            };

            (axum::http::StatusCode::OK, Json(response)).into_response()
        }
        Err(e) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse::new(&format!("Agent error: {e}"), None)),
        )
            .into_response(),
    }
}

/// POST /v1/chat/completions/stream — SSE streaming chat.
pub async fn chat_completions_stream(
    State(state): State<GatewayState>,
    Json(req): Json<ChatCompletionRequest>,
) -> impl IntoResponse {
    let prompt = extract_prompt(&req);

    // Same process-wide turn lease as v2 — see `chat_completions`.
    let lease = match state.turns.try_begin(req.session_id.as_deref().unwrap_or_default()) {
        Ok(lease) => lease,
        Err(_) => {
            let err_event = Event::default().data(
                serde_json::json!({"error": {"message": "A turn is already running.",
                                             "code": "turn_in_progress"}})
                    .to_string(),
            );
            let stream: Pin<Box<dyn Stream<Item = Result<Event, axum::Error>> + Send>> =
                Box::pin(futures::stream::once(async move { Ok(err_event) }));
            return Sse::new(stream).keep_alive(KeepAlive::default());
        }
    };

    // If session_id provided, resume that session
    if let Some(ref session_id) = req.session_id {
        let mut harness = state.harness.write().await;
        if let Err(e) = harness.resume_session(session_id).await {
            let err_event = Event::default()
                .data(serde_json::json!({"error": format!("{e}")}).to_string());
            let stream: Pin<Box<dyn Stream<Item = Result<Event, axum::Error>> + Send>> =
                Box::pin(futures::stream::once(async move { Ok(err_event) }));
            return Sse::new(stream).keep_alive(KeepAlive::default());
        }
    }

    let harness = state.harness.read().await;
    let session_id = harness.current_session_id().to_string();
    let model_name = harness.provider_mgr().current_model_name().to_string();
    let provider_name = harness.provider_mgr().current_provider().to_string();
    let completion_id = format!("chatcmpl-{session_id}");

    harness.begin_turn();
    let stream_result = harness.run_turn_enriched(&prompt).await;
    drop(harness);

    let stream_state = state.clone();
    let event_stream: Pin<Box<dyn Stream<Item = Result<Event, axum::Error>> + Send>> =
        Box::pin(async_stream::stream! {
            // Owns the turn lease for the life of the response body: when the
            // client disconnects, the body is dropped and the turn is released.
            let _lease = lease;
            match stream_result {
                Ok((_enriched, mut event_stream)) => {
                    // Send initial role chunk
                    let init_chunk = serde_json::json!(ChatCompletionChunk {
                        id: completion_id.clone(),
                        object: "chat.completion.chunk".to_string(),
                        created: chrono::Utc::now().timestamp() as u64,
                        model: format!("{provider_name}/{model_name}"),
                        choices: vec![StreamChoice {
                            index: 0,
                            delta: StreamDelta {
                                role: Some("assistant".to_string()),
                                content: None,
                            },
                            finish_reason: None,
                        }],
                    });
                    yield Ok(Event::default().data(init_chunk.to_string()));

                    // Stream content chunks
                    loop {
                        match event_stream.next().await {
                            Some(Ok(event)) => {
                                if let Some(meta) = &event.llm_response.usage_metadata {
                                    stream_state.harness.read().await.record_usage(meta);
                                }
                                if let Some(content) = event.content() {
                                    for part in &content.parts {
                                        if let Part::Text { text } = part {
                                            let chunk = serde_json::json!(ChatCompletionChunk {
                                                id: completion_id.clone(),
                                                object: "chat.completion.chunk".to_string(),
                                                created: chrono::Utc::now().timestamp() as u64,
                                                model: format!("{provider_name}/{model_name}"),
                                                choices: vec![StreamChoice {
                                                    index: 0,
                                                    delta: StreamDelta {
                                                        role: None,
                                                        content: Some(text.clone()),
                                                    },
                                                    finish_reason: None,
                                                }],
                                            });
                                            yield Ok(Event::default().data(chunk.to_string()));
                                        }
                                    }
                                }

                                if event.is_final_response() {
                                    let done_chunk = serde_json::json!(ChatCompletionChunk {
                                        id: completion_id.clone(),
                                        object: "chat.completion.chunk".to_string(),
                                        created: chrono::Utc::now().timestamp() as u64,
                                        model: format!("{provider_name}/{model_name}"),
                                        choices: vec![StreamChoice {
                                            index: 0,
                                            delta: StreamDelta {
                                                role: None,
                                                content: None,
                                            },
                                            finish_reason: Some("stop".to_string()),
                                        }],
                                    });
                                    yield Ok(Event::default().data(done_chunk.to_string()));
                                    yield Ok(Event::default().data("[DONE]".to_string()));
                                    break;
                                }
                            }
                            Some(Err(e)) => {
                                let err = serde_json::json!({"error": format!("Stream error: {e}")});
                                yield Ok(Event::default().data(err.to_string()));
                                break;
                            }
                            None => break,
                        }
                    }
                }
                Err(e) => {
                    let err = serde_json::json!({"error": format!("Agent error: {e}")});
                    yield Ok(Event::default().data(err.to_string()));
                }
            }

            stream_state.harness.read().await.end_turn();
        });

    Sse::new(event_stream).keep_alive(KeepAlive::default())
}

/// GET /v1/sessions — List sessions.
pub async fn list_sessions(
    State(state): State<GatewayState>,
) -> impl IntoResponse {
    let harness = state.harness.read().await;
    let sessions = match harness.session_mgr().list_sessions().await {
        Ok(s) => s,
        Err(e) => {
            return (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new(&format!("{e}"), None)),
            )
                .into_response();
        }
    };

    let infos: Vec<SessionInfo> = sessions
        .into_iter()
        .map(|s| SessionInfo {
            id: s.id,
            created_at: s.updated_at.to_string(),
            title: s.title,
            event_count: s.event_count,
        })
        .collect();

    (axum::http::StatusCode::OK, Json(serde_json::json!({ "sessions": infos }))).into_response()
}

/// POST /v1/sessions — Create a new session.
pub async fn create_session(
    State(state): State<GatewayState>,
) -> impl IntoResponse {
    let mut harness = state.harness.write().await;
    match harness.new_session().await {
        Ok(id) => (
            axum::http::StatusCode::CREATED,
            Json(serde_json::json!({ "session_id": id })),
        )
            .into_response(),
        Err(e) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse::new(&format!("{e}"), None)),
        )
            .into_response(),
    }
}

/// GET /v1/sessions/:id — Get session details.
pub async fn get_session(
    State(state): State<GatewayState>,
    Path(session_id): Path<String>,
) -> impl IntoResponse {
    let harness = state.harness.read().await;
    match harness.session_mgr().get_session(&session_id).await {
        Ok(session) => {
            let events = session.events().all();
            let mut messages: Vec<serde_json::Value> = Vec::new();
            for event in &events {
                if let Some(content) = event.content() {
                    let mut parts_text = Vec::new();
                    for part in &content.parts {
                        if let Part::Text { text } = part {
                            parts_text.push(text.clone());
                        }
                    }
                    if !parts_text.is_empty() {
                        messages.push(serde_json::json!({
                            "role": content.role,
                            "content": parts_text.join(""),
                        }));
                    }
                }
            }
            (
                axum::http::StatusCode::OK,
                Json(serde_json::json!({
                    "session_id": session_id,
                    "created_at": session.last_update_time(),
                    "event_count": events.len(),
                    "messages": messages,
                })),
            )
                .into_response()
        }
        Err(e) => (
            axum::http::StatusCode::NOT_FOUND,
            Json(ErrorResponse::new(&format!("Session not found: {e}"), None)),
        )
            .into_response(),
    }
}

/// DELETE /v1/sessions/:id — Delete a session.
pub async fn delete_session(
    State(state): State<GatewayState>,
    Path(session_id): Path<String>,
) -> impl IntoResponse {
    let harness = state.harness.read().await;
    match harness.session_mgr().delete_session(&session_id).await {
        Ok(()) => (
            axum::http::StatusCode::OK,
            Json(serde_json::json!({ "deleted": true })),
        )
            .into_response(),
        Err(e) => (
            axum::http::StatusCode::NOT_FOUND,
            Json(ErrorResponse::new(&format!("{e}"), None)),
        )
            .into_response(),
    }
}

/// POST /v1/sessions/:id/compact — Compact a session.
pub async fn compact_session(
    State(state): State<GatewayState>,
    Path(session_id): Path<String>,
) -> impl IntoResponse {
    let mut harness = state.harness.write().await;
    if let Err(e) = harness.resume_session(&session_id).await {
        return (
            axum::http::StatusCode::BAD_REQUEST,
            Json(ErrorResponse::new(&format!("{e}"), None)),
        )
            .into_response();
    }
    match harness.compact_session().await {
        Ok((count, new_id)) => (
            axum::http::StatusCode::OK,
            Json(serde_json::json!({
                "compacted_events": count,
                "new_session_id": new_id,
            })),
        )
            .into_response(),
        Err(e) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse::new(&format!("{e}"), None)),
        )
            .into_response(),
    }
}

/// GET /v1/models — List available models.
pub async fn list_models(
    State(state): State<GatewayState>,
) -> impl IntoResponse {
    let harness = state.harness.read().await;
    let current_provider = harness.provider_mgr().current_provider().to_string();
    let current_model = harness.provider_mgr().current_model_name().to_string();

    let models = vec![ModelInfo {
        id: format!("{current_provider}/{current_model}"),
        provider: current_provider,
        current: true,
    }];

    (axum::http::StatusCode::OK, Json(serde_json::json!({ "data": models }))).into_response()
}

/// GET /v1/cost — Get cost tracking info.
pub async fn get_cost(
    State(state): State<GatewayState>,
) -> impl IntoResponse {
    let harness = state.harness.read().await;
    let cost = harness.cost_tracker().session_summary();
    let body = serde_json::json!({
        "total_cost": cost.total_cost,
        "total_prompt_tokens": cost.total_prompt_tokens,
        "total_completion_tokens": cost.total_completion_tokens,
        "total_tokens": cost.total_tokens,
        "request_count": cost.request_count,
    });
    (axum::http::StatusCode::OK, Json(body)).into_response()
}

/// GET /health — Health/readiness check.
///
/// Unauthenticated by design (see the router split in `mod.rs`): this is the
/// probe a desktop shell polls before the UI loads. `turn_active` lets a client
/// that reloaded mid-turn recover its state without guessing.
pub async fn health(State(state): State<GatewayState>) -> impl IntoResponse {
    let harness = state.harness.read().await;
    let active = state.turns.active();

    (
        axum::http::StatusCode::OK,
        Json(serde_json::json!({
            "status": "ok",
            "version": env!("CARGO_PKG_VERSION"),
            "provider": harness.provider_mgr().current_provider(),
            "model": harness.provider_mgr().current_model_name(),
            "session_id": harness.current_session_id(),
            "agent": harness.config().agent_name,
            "permission_mode": harness.sandbox().permission_mode().to_string(),
            "turn_active": active.is_some(),
            "turn_id": active.map(|t| t.turn_id),
        })),
    )
        .into_response()
}

/// Consume an EventStream and return (text, tool_call_count, usage).
///
/// Records usage against the cost tracker as it goes — previously gateway turns
/// recorded nothing, so `/v1/cost` only ever reflected REPL activity.
async fn consume_stream(
    harness: &crate::harness::Harness,
    mut stream: adk_rust::EventStream,
) -> (String, usize, Usage) {
    let mut text = String::new();
    let mut tool_count = 0;
    let mut usage = Usage {
        prompt_tokens: 0,
        completion_tokens: 0,
        total_tokens: 0,
    };

    loop {
        match stream.next().await {
            Some(Ok(event)) => {
                if let Some(meta) = &event.llm_response.usage_metadata {
                    harness.record_usage(meta);
                    usage.prompt_tokens = meta.prompt_token_count.max(0) as u64;
                    usage.completion_tokens = meta.candidates_token_count.max(0) as u64;
                    usage.total_tokens = meta.total_token_count.max(0) as u64;
                }
                if let Some(content) = event.content() {
                    for part in &content.parts {
                        match part {
                            Part::Text { text: t } => text.push_str(t),
                            Part::FunctionCall { .. } => tool_count += 1,
                            _ => {}
                        }
                    }
                }
                if event.is_final_response() {
                    break;
                }
            }
            Some(Err(_)) => break,
            None => break,
        }
    }

    (text, tool_count, usage)
}
