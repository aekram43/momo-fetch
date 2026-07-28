//! Handlers for the `/v2` API — the rich surface consumed by the web UI.

use std::time::Duration;

use adk_rust::futures::StreamExt;
use adk_rust::{EventStream, Part};
use axum::{
    extract::State,
    response::{
        sse::{Event, KeepAlive, Sse},
        IntoResponse, Response,
    },
    Json,
};
use axum::http::StatusCode;
use tokio::sync::mpsc;

use super::turn::{ApprovalError, TurnLease};
use super::types::messages_to_prompt;
use super::v2_types::*;
use crate::gateway::GatewayState;
use crate::harness::Harness;

/// Max bytes of tool output echoed into `tool_call_result`.
const OUTPUT_PREVIEW_LIMIT: usize = 2048;
/// Buffer between the turn task and the SSE writer.
const EVENT_CHANNEL_CAPACITY: usize = 256;
/// How long to wait for the next event from the provider before giving up.
const STREAM_EVENT_TIMEOUT_SECS: u64 = 120;
/// Consecutive provider timeouts tolerated before the turn is abandoned.
const MAX_CONSECUTIVE_TIMEOUTS: u32 = 2;

// ── Guards ─────────────────────────────────────────────────────────────────

/// Refuse mutating endpoints while a turn holds the harness.
///
/// This is not just tidiness: every one of these calls rebuilds the runner
/// under a write lock, and a writer waiting on `RwLock` blocks all new readers.
/// Rejecting early means the streaming task never contends with one.
fn require_idle(state: &GatewayState) -> Result<(), Response> {
    match state.turns.active() {
        None => Ok(()),
        Some(active) => Err(v2_error(
            StatusCode::CONFLICT,
            "turn_in_progress",
            "A turn is already running. Interrupt it before changing configuration.",
            Some(serde_json::json!({
                "turn_id": active.turn_id,
                "active_session_id": active.session_id,
                "started_at": active.started_at.to_rfc3339(),
            })),
        )),
    }
}

// ── POST /v2/chat/stream ───────────────────────────────────────────────────

pub async fn v2_chat_stream(
    State(state): State<GatewayState>,
    Json(req): Json<V2ChatRequest>,
) -> Response {
    let prompt = messages_to_prompt(&req.messages);
    if prompt.trim().is_empty() {
        return v2_error(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "No user message found in `messages`.",
            None,
        );
    }

    // Claim the harness first so overrides below can't race another turn.
    let session_hint = req.session_id.clone().unwrap_or_default();
    let lease = match state.turns.try_begin(&session_hint) {
        Ok(lease) => lease,
        Err(active) => {
            return v2_error(
                StatusCode::CONFLICT,
                "turn_in_progress",
                "A turn is already running.",
                Some(serde_json::json!({
                    "turn_id": active.turn_id,
                    "active_session_id": active.session_id,
                })),
            );
        }
    };

    // Apply per-turn overrides while we hold the claim. Safe to take the write
    // lock here: no other turn exists, and mutating endpoints are 409'd.
    {
        let mut harness = state.harness.write().await;
        if let Some(session_id) = &req.session_id {
            if let Err(e) = harness.resume_session(session_id).await {
                return v2_error(
                    StatusCode::NOT_FOUND,
                    "not_found",
                    format!("Failed to resume session: {e}"),
                    None,
                );
            }
        }
        if let Some(agent) = &req.agent {
            if let Err(e) = harness.switch_agent(agent) {
                return v2_error(
                    StatusCode::BAD_REQUEST,
                    "invalid_request",
                    format!("{e}"),
                    None,
                );
            }
        }
        if let Some(model) = &req.model {
            if let Err(e) = harness.switch_model(model) {
                return v2_error(
                    StatusCode::BAD_REQUEST,
                    "invalid_request",
                    format!("{e}"),
                    None,
                );
            }
        }
    }

    let (tx, mut rx) = mpsc::channel::<V2StreamEvent>(EVENT_CHANNEL_CAPACITY);

    // The turn runs in its own task and owns the lease. Dropping the SSE body
    // drops `rx`, the next `send` fails, and the task unwinds — releasing the
    // lease. That is how a closed browser tab stops holding the harness.
    tokio::spawn(run_v2_turn(state.clone(), prompt, lease, tx));

    let sse = async_stream::stream! {
        while let Some(event) = rx.recv().await {
            yield Ok::<_, axum::Error>(
                Event::default().event(event.name()).data(event.data())
            );
        }
    };

    // Keep-alive matters here: an approval can park the stream for minutes and
    // intermediaries will drop an idle connection.
    Sse::new(sse).keep_alive(KeepAlive::default()).into_response()
}

/// Emit an event; bail out of the turn if the client is gone.
macro_rules! emit {
    ($tx:expr, $state:expr, $lease:expr, $event:expr) => {
        if $tx.send($event).await.is_err() {
            // Receiver dropped — client disconnected. Stop generating so we
            // don't burn tokens on output nobody will read.
            let harness = $state.harness.read().await;
            harness.interrupt();
            harness.end_turn();
            tracing::debug!(turn_id = %$lease.turn_id(), "client disconnected mid-turn");
            return;
        }
    };
}

async fn run_v2_turn(
    state: GatewayState,
    prompt: String,
    lease: TurnLease,
    tx: mpsc::Sender<V2StreamEvent>,
) {
    let turn_id = lease.turn_id().to_string();

    // Opening frame: tells the client which session/model actually served the
    // turn, which may differ from what it asked for.
    {
        let harness = state.harness.read().await;
        let event = V2StreamEvent::Role(RolePayload {
            role: "assistant".to_string(),
            model: harness.provider_mgr().current_model_name().to_string(),
            provider: harness.provider_mgr().current_provider().to_string(),
            session_id: harness.current_session_id().to_string(),
            agent: harness.config().agent_name.clone(),
            turn_id: turn_id.clone(),
        });
        drop(harness);
        emit!(tx, state, lease, event);
    }

    let mut stop_reason = "complete";
    let mut next_leg = Some(LegStart::Initial(prompt));

    // One iteration per leg. A leg ends either with the turn finished or with a
    // tool confirmation, which adk surfaces by *ending the stream* — the next
    // leg is a fresh turn carrying the decision.
    while let Some(leg) = next_leg.take() {
        let stream_result = match leg {
            LegStart::Initial(prompt) => {
                let harness = state.harness.read().await;
                harness.begin_turn();
                harness.run_turn_enriched(&prompt).await.map(|(_, s)| s)
            }
            LegStart::Confirmation { tool_name, approved } => {
                // Write lock: `run_confirmation_turn` rebuilds the runner to
                // bake the decision into RunConfig. Taken only after the
                // approval wait has finished, never across it.
                let mut harness = state.harness.write().await;
                harness.begin_turn();
                harness.run_confirmation_turn(&tool_name, approved).await
            }
        };

        let mut stream = match stream_result {
            Ok(s) => s,
            Err(e) => {
                let event = V2StreamEvent::Error(ErrorPayload {
                    code: "internal".to_string(),
                    message: format!("Agent error: {e}"),
                });
                emit!(tx, state, lease, event);
                stop_reason = "error";
                break;
            }
        };

        let outcome = {
            // Held for the leg only. Readers (`GET /v2/*`) are unaffected and
            // writers are 409'd, so this cannot block anyone. Crucially it is
            // dropped before the approval wait below.
            let harness = state.harness.read().await;
            consume_leg(&harness, &mut stream, &tx).await
        };

        if outcome.client_gone {
            let harness = state.harness.read().await;
            harness.interrupt();
            harness.end_turn();
            return;
        }

        if let Some(message) = outcome.error {
            let event = V2StreamEvent::Error(ErrorPayload {
                code: "internal".to_string(),
                message,
            });
            emit!(tx, state, lease, event);
            stop_reason = "error";
            break;
        }

        if lease.is_interrupted() {
            stop_reason = "interrupted";
            break;
        }

        let Some(pending) = outcome.pending else {
            break;
        };

        // ── Approval wait: no lock held for its whole duration ──
        let (destructive, category) = classify(&state, &pending).await;
        let expires_at = chrono::Utc::now()
            + chrono::Duration::seconds(state.config.approval_timeout_secs as i64);

        let rx_decision = state.turns.register_approval(
            &turn_id,
            &pending.tool_name,
            pending.call_id.clone(),
        );

        let event = V2StreamEvent::ApprovalRequired(ApprovalRequiredPayload {
            turn_id: turn_id.clone(),
            call_id: pending.call_id.clone(),
            name: pending.tool_name.clone(),
            args: pending.args.clone(),
            destructive,
            category,
            // adk records the decision by tool name for the life of the
            // process. The dialog has to disclose that.
            sticky: true,
            expires_at: expires_at.to_rfc3339(),
        });
        emit!(tx, state, lease, event);

        let timeout = Duration::from_secs(state.config.approval_timeout_secs);
        let (approved, reason) = match tokio::time::timeout(timeout, rx_decision).await {
            Ok(Ok(decision)) => (decision, "user"),
            // Sender dropped: interrupt or turn teardown. Fail closed.
            Ok(Err(_)) => (false, "disconnect"),
            Err(_) => (false, "timeout"),
        };

        let event = V2StreamEvent::ApprovalResolved(ApprovalResolvedPayload {
            call_id: pending.call_id.clone(),
            name: pending.tool_name.clone(),
            approved,
            reason: reason.to_string(),
        });
        emit!(tx, state, lease, event);

        next_leg = Some(LegStart::Confirmation {
            tool_name: pending.tool_name,
            approved,
        });
    }

    // Close out cost accounting and report the final tallies.
    {
        let harness = state.harness.read().await;
        harness.end_turn();
        let usage = harness.context_usage();
        let event = V2StreamEvent::ContextUsage(ContextUsagePayload {
            used: usage.prompt_tokens,
            total: usage.context_window,
            percent: usage.percentage().map(|p| p * 100.0),
        });
        drop(harness);
        emit!(tx, state, lease, event);
    }

    if lease.is_interrupted() {
        stop_reason = "interrupted";
    }

    let event = V2StreamEvent::Done(DonePayload {
        turn_id,
        stop_reason: stop_reason.to_string(),
    });
    emit!(tx, state, lease, event);
}

/// How the next leg of a turn is started.
enum LegStart {
    Initial(String),
    Confirmation { tool_name: String, approved: bool },
}

/// A tool call adk stopped on, waiting for the user.
struct PendingConfirmation {
    tool_name: String,
    call_id: Option<String>,
    args: serde_json::Value,
}

#[derive(Default)]
struct LegOutcome {
    pending: Option<PendingConfirmation>,
    error: Option<String>,
    client_gone: bool,
}

/// Drain one event stream, translating adk events into V2 SSE events.
async fn consume_leg(
    harness: &Harness,
    stream: &mut EventStream,
    tx: &mpsc::Sender<V2StreamEvent>,
) -> LegOutcome {
    let mut outcome = LegOutcome::default();
    let mut consecutive_timeouts = 0u32;

    macro_rules! send {
        ($event:expr) => {
            if tx.send($event).await.is_err() {
                outcome.client_gone = true;
                return outcome;
            }
        };
    }

    loop {
        let next = tokio::time::timeout(
            Duration::from_secs(STREAM_EVENT_TIMEOUT_SECS),
            stream.next(),
        )
        .await;

        let event = match next {
            Err(_) => {
                consecutive_timeouts += 1;
                if consecutive_timeouts >= MAX_CONSECUTIVE_TIMEOUTS {
                    harness.interrupt();
                    outcome.error = Some(format!(
                        "No response from {} ({}) after {}s.",
                        harness.provider_mgr().current_provider(),
                        harness.provider_mgr().current_model_name(),
                        STREAM_EVENT_TIMEOUT_SECS * MAX_CONSECUTIVE_TIMEOUTS as u64,
                    ));
                    return outcome;
                }
                continue;
            }
            Ok(None) => return outcome,
            Ok(Some(Err(e))) => {
                outcome.error = Some(format!("Stream error: {e}"));
                return outcome;
            }
            Ok(Some(Ok(event))) => {
                consecutive_timeouts = 0;
                event
            }
        };

        // Cost first: usage arrives on the same events as content, and a turn
        // that errors later still consumed tokens.
        if let Some(usage) = &event.llm_response.usage_metadata {
            let cost = harness.record_usage(usage);
            send!(V2StreamEvent::Usage(UsagePayload {
                prompt_tokens: usage.prompt_token_count,
                completion_tokens: usage.candidates_token_count,
                total_tokens: usage.total_token_count,
                cost_usd: cost,
            }));
        }

        // adk asks for confirmation by setting this and then ending the stream.
        if let Some(request) = &event.actions.tool_confirmation {
            outcome.pending = Some(PendingConfirmation {
                tool_name: request.tool_name.clone(),
                call_id: request.function_call_id.clone(),
                args: request.args.clone(),
            });
        }

        if let Some(content) = event.content() {
            for part in &content.parts {
                match part {
                    Part::Text { text } => {
                        send!(V2StreamEvent::Text(TextPayload {
                            content: text.clone(),
                        }));
                    }
                    Part::FunctionCall { name, args, id, .. } => {
                        send!(V2StreamEvent::ToolCallStart(ToolCallStartPayload {
                            id: id.clone(),
                            name: name.clone(),
                            args: args.clone(),
                        }));
                    }
                    Part::FunctionResponse {
                        function_response,
                        id,
                    } => {
                        let (preview, truncated) = preview_of(&function_response.response);
                        let status = if is_error_response(&function_response.response) {
                            "error"
                        } else {
                            "done"
                        };
                        send!(V2StreamEvent::ToolCallResult(ToolCallResultPayload {
                            id: id.clone(),
                            name: function_response.name.clone(),
                            status: status.to_string(),
                            output_preview: preview,
                            truncated,
                        }));
                    }
                    // Thinking traces and binary parts are not surfaced yet.
                    _ => {}
                }
            }
        }

        if let Some(err) = &event.llm_response.error_message {
            if !err.is_empty() {
                send!(V2StreamEvent::Error(ErrorPayload {
                    code: "provider_error".to_string(),
                    message: err.clone(),
                }));
            }
        }

        if event.is_final_response() {
            return outcome;
        }
    }
}

/// Ask the sandbox whether a pending call looks destructive, for display.
async fn classify(state: &GatewayState, pending: &PendingConfirmation) -> (bool, Option<String>) {
    let command = pending
        .args
        .get("command")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let Some(command) = command else {
        return (false, None);
    };
    let harness = state.harness.read().await;
    let check = harness.sandbox().check_destructive(&command);
    (check.is_destructive, check.category)
}

/// Render a tool result for the UI, capped so a large file read doesn't ship
/// megabytes down the event stream.
fn preview_of(value: &serde_json::Value) -> (String, bool) {
    let rendered = match value {
        serde_json::Value::String(s) => s.clone(),
        other => other.to_string(),
    };
    if rendered.len() <= OUTPUT_PREVIEW_LIMIT {
        return (rendered, false);
    }
    let mut end = OUTPUT_PREVIEW_LIMIT;
    while end > 0 && !rendered.is_char_boundary(end) {
        end -= 1;
    }
    (rendered[..end].to_string(), true)
}

fn is_error_response(value: &serde_json::Value) -> bool {
    value
        .get("error")
        .is_some_and(|e| !e.is_null() && e.as_str() != Some(""))
}

// ── POST /v2/chat/approve · /v2/chat/deny ─────────────────────────────────

pub async fn v2_chat_approve(
    State(state): State<GatewayState>,
    Json(req): Json<ApprovalRequest>,
) -> Response {
    resolve_approval(state, req, true)
}

pub async fn v2_chat_deny(
    State(state): State<GatewayState>,
    Json(req): Json<ApprovalRequest>,
) -> Response {
    resolve_approval(state, req, false)
}

fn resolve_approval(state: GatewayState, req: ApprovalRequest, default: bool) -> Response {
    let approved = req.approved.unwrap_or(default);
    match state.turns.resolve(
        &req.turn_id,
        &req.tool_name,
        req.call_id.as_deref(),
        approved,
    ) {
        Ok(()) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "resolved": true,
                "approved": approved,
                "sticky": approved,
            })),
        )
            .into_response(),
        Err(ApprovalError::NotPending) => v2_error(
            StatusCode::CONFLICT,
            "stale_approval",
            "No approval is pending for that turn and tool.",
            None,
        ),
        Err(ApprovalError::StaleCallId) => v2_error(
            StatusCode::CONFLICT,
            "stale_approval",
            "That approval belongs to an earlier tool call.",
            None,
        ),
    }
}

// ── POST /v2/chat/interrupt ───────────────────────────────────────────────

pub async fn v2_chat_interrupt(
    State(state): State<GatewayState>,
    body: Option<Json<InterruptRequest>>,
) -> Response {
    let Some(active) = state.turns.active() else {
        return (
            StatusCode::OK,
            Json(serde_json::json!({ "interrupted": false, "reason": "no_active_turn" })),
        )
            .into_response();
    };

    if let Some(Json(req)) = body {
        if let Some(turn_id) = req.turn_id {
            if turn_id != active.turn_id {
                return v2_error(
                    StatusCode::CONFLICT,
                    "stale_turn",
                    "That turn is no longer active.",
                    Some(serde_json::json!({ "active_turn_id": active.turn_id })),
                );
            }
        }
    }

    state.turns.mark_interrupted(&active.turn_id);
    // A turn parked on an approval isn't inside the runner, so `interrupt()`
    // alone would leave it waiting out the timeout. Deny the pending request
    // too so it unwinds immediately.
    state.turns.deny_all(&active.turn_id);
    let harness = state.harness.read().await;
    let interrupted = harness.interrupt();

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "interrupted": true,
            "runner_interrupted": interrupted,
            "turn_id": active.turn_id,
        })),
    )
        .into_response()
}

// ── Agents ────────────────────────────────────────────────────────────────

pub async fn v2_agents(State(state): State<GatewayState>) -> Response {
    let harness = state.harness.read().await;
    let current = harness.config().agent_name.clone();
    let registry = harness.agent_registry();

    let agents: Vec<AgentSummary> = registry
        .list()
        .into_iter()
        .map(|def| AgentSummary {
            name: def.name.clone(),
            description: def.description.clone(),
            capabilities: def.capabilities.clone(),
            model: def.model.clone(),
            provider: def.provider.clone(),
            is_current: current.as_deref() == Some(def.name.as_str()),
            is_orchestrator: registry.is_orchestrator(&def.name),
        })
        .collect();

    (
        StatusCode::OK,
        Json(serde_json::json!({ "agents": agents, "current": current })),
    )
        .into_response()
}

pub async fn v2_agents_switch(
    State(state): State<GatewayState>,
    Json(req): Json<SwitchAgentRequest>,
) -> Response {
    if let Err(conflict) = require_idle(&state) {
        return conflict;
    }
    let mut harness = state.harness.write().await;
    match harness.switch_agent(&req.name) {
        Ok(()) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "switched_to": req.name,
                "provider": harness.provider_mgr().current_provider(),
                "model": harness.provider_mgr().current_model_name(),
            })),
        )
            .into_response(),
        Err(e) => v2_error(StatusCode::BAD_REQUEST, "invalid_request", format!("{e}"), None),
    }
}

pub async fn v2_agents_default(State(state): State<GatewayState>) -> Response {
    if let Err(conflict) = require_idle(&state) {
        return conflict;
    }
    let mut harness = state.harness.write().await;
    match harness.clear_agent() {
        Ok(()) => (
            StatusCode::OK,
            Json(serde_json::json!({ "switched_to": serde_json::Value::Null })),
        )
            .into_response(),
        Err(e) => v2_error(StatusCode::INTERNAL_SERVER_ERROR, "internal", format!("{e}"), None),
    }
}

// ── Providers & models ────────────────────────────────────────────────────

pub async fn v2_providers(State(state): State<GatewayState>) -> Response {
    let harness = state.harness.read().await;
    let current_provider = harness.provider_mgr().current_provider().to_string();
    let current_model = harness.provider_mgr().current_model_name().to_string();

    let providers: Vec<ProviderSummary> = harness
        .provider_mgr()
        .list_all()
        .into_iter()
        .map(|info| {
            let is_current = info.provider == current_provider;
            ProviderSummary {
                name: info.provider,
                is_current,
                current_model: is_current.then(|| current_model.clone()),
                default_model: info.default_model,
                available: info.available,
            }
        })
        .collect();

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "providers": providers,
            "current": { "provider": current_provider, "model": current_model },
        })),
    )
        .into_response()
}

pub async fn v2_switch_model(
    State(state): State<GatewayState>,
    Json(req): Json<SwitchModelRequest>,
) -> Response {
    if let Err(conflict) = require_idle(&state) {
        return conflict;
    }
    let mut harness = state.harness.write().await;
    match harness.switch_model(&req.model) {
        Ok(()) => current_selection(&harness),
        Err(e) => v2_error(StatusCode::BAD_REQUEST, "invalid_request", format!("{e}"), None),
    }
}

pub async fn v2_switch_provider(
    State(state): State<GatewayState>,
    Json(req): Json<SwitchProviderRequest>,
) -> Response {
    if let Err(conflict) = require_idle(&state) {
        return conflict;
    }
    let mut harness = state.harness.write().await;
    match harness.switch_provider(&req.provider) {
        Ok(()) => current_selection(&harness),
        Err(e) => v2_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "provider_unavailable",
            format!("{e}"),
            None,
        ),
    }
}

pub async fn v2_switch(
    State(state): State<GatewayState>,
    Json(req): Json<SwitchRequest>,
) -> Response {
    if let Err(conflict) = require_idle(&state) {
        return conflict;
    }
    let mut harness = state.harness.write().await;
    match harness.switch(&req.provider, &req.model) {
        Ok(()) => current_selection(&harness),
        Err(e) => v2_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "provider_unavailable",
            format!("{e}"),
            None,
        ),
    }
}

// ── Settings ──────────────────────────────────────────────────────────────

pub async fn v2_settings(State(state): State<GatewayState>) -> Response {
    let harness = state.harness.read().await;
    (
        StatusCode::OK,
        Json(serde_json::json!({
            "permission_mode": harness.sandbox().permission_mode().to_string(),
            "project_path": harness.config().project_path,
            "sandbox_root": harness.sandbox().root(),
            "agent": harness.config().agent_name,
            // Tools already granted for the life of this process. Approval is
            // sticky by tool name, so the user needs to be able to see this.
            "approved_tools": harness.approved_tools(),
            "memory": {
                "auto_search_enabled": harness.memory_sidecar().auto_search_enabled(),
            },
            "approval_timeout_secs": state.config.approval_timeout_secs,
        })),
    )
        .into_response()
}

pub async fn v2_settings_permission(
    State(state): State<GatewayState>,
    Json(req): Json<PermissionRequest>,
) -> Response {
    if let Err(conflict) = require_idle(&state) {
        return conflict;
    }
    let mode: crate::sandbox::PermissionMode = match req.mode.parse() {
        Ok(mode) => mode,
        Err(e) => {
            return v2_error(
                StatusCode::BAD_REQUEST,
                "invalid_request",
                format!("{e}"),
                Some(serde_json::json!({ "valid": ["strict", "auto", "yolo"] })),
            );
        }
    };

    let mut harness = state.harness.write().await;
    match harness.switch_permission(mode) {
        Ok(()) => (
            StatusCode::OK,
            Json(serde_json::json!({ "mode": mode.to_string() })),
        )
            .into_response(),
        Err(e) => v2_error(StatusCode::INTERNAL_SERVER_ERROR, "internal", format!("{e}"), None),
    }
}

/// Revoke every sticky tool approval, so confirmation prompts come back.
pub async fn v2_settings_clear_approvals(State(state): State<GatewayState>) -> Response {
    if let Err(conflict) = require_idle(&state) {
        return conflict;
    }
    let mut harness = state.harness.write().await;
    match harness.clear_approved_tools() {
        Ok(()) => (
            StatusCode::OK,
            Json(serde_json::json!({ "approved_tools": harness.approved_tools() })),
        )
            .into_response(),
        Err(e) => v2_error(StatusCode::INTERNAL_SERVER_ERROR, "internal", format!("{e}"), None),
    }
}

fn current_selection(harness: &Harness) -> Response {
    (
        StatusCode::OK,
        Json(serde_json::json!({
            "provider": harness.provider_mgr().current_provider(),
            "model": harness.provider_mgr().current_model_name(),
        })),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_is_capped_on_a_char_boundary() {
        let long = serde_json::Value::String("é".repeat(OUTPUT_PREVIEW_LIMIT));
        let (preview, truncated) = preview_of(&long);
        assert!(truncated);
        assert!(preview.len() <= OUTPUT_PREVIEW_LIMIT);
        // Would have panicked on a bad slice; this asserts we cut cleanly.
        assert!(preview.chars().all(|c| c == 'é'));
    }

    #[test]
    fn short_output_is_not_marked_truncated() {
        let (preview, truncated) = preview_of(&serde_json::json!("fn main() {}"));
        assert_eq!(preview, "fn main() {}");
        assert!(!truncated);
    }

    #[test]
    fn error_responses_are_detected() {
        assert!(is_error_response(&serde_json::json!({"error": "boom"})));
        assert!(!is_error_response(&serde_json::json!({"error": null})));
        assert!(!is_error_response(&serde_json::json!({"ok": true})));
    }

    #[test]
    fn event_names_match_the_protocol() {
        let done = V2StreamEvent::Done(DonePayload {
            turn_id: "t_1".into(),
            stop_reason: "complete".into(),
        });
        assert_eq!(done.name(), "done");
        assert_eq!(done.data(), r#"{"turn_id":"t_1","stop_reason":"complete"}"#);
    }
}
