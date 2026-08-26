//! Handlers for the `/v2` API — the rich surface consumed by the web UI.

use std::time::{Duration, Instant};

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
/// How often a quiet stream wakes up to look around.
///
/// Not a deadline — just the resolution at which "nothing has arrived" becomes
/// a decision: report a running tool, or notice a provider that is gone.
const STREAM_TICK_SECS: u64 = 5;
/// Silence tolerated **with nothing running** before the turn is abandoned.
///
/// Only ever measured while no tool call is in flight. See `consume_leg`.
const PROVIDER_SILENCE_SECS: u64 = 240;
/// Gap between `tool_call_progress` events for a call that is still running.
const PROGRESS_EVERY_SECS: u64 = 15;

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

    // Name the session from its first prompt, before the turn starts, so the
    // sidebar stops being a list of hex ids. Only ever fills a blank — a title
    // someone typed is locked and never overwritten. Best-effort: a session
    // that cannot be labelled is cosmetic, and failing a turn over it would be
    // absurd.
    {
        let harness = state.harness.read().await;
        let session_id = harness.current_session_id().to_string();
        harness.session_mgr().auto_title(&session_id, &prompt).await;
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

    // Stamp the tree before anything runs. Whatever the turn writes — through
    // the file tools, through the shell, through a subprocess it started — is
    // the difference between this and the stamp taken after the last leg.
    let root = {
        let harness = state.harness.read().await;
        harness.sandbox().root().to_path_buf()
    };
    let before = stamp(&root).await;

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

    // Files, after cost: an interrupted or failed turn still wrote whatever it
    // wrote, and that is exactly when someone needs to see it.
    if let Some(before) = before {
        let after = stamp(&root).await;
        let files: Vec<ArtifactChange> = after
            .map(|after| crate::artifacts::changes(&before, &after, &root))
            .unwrap_or_default()
            .into_iter()
            .map(|c| ArtifactChange {
                path: c.path,
                change: c.kind.as_str().to_string(),
            })
            .collect();

        if !files.is_empty() {
            let event = V2StreamEvent::Artifacts(ArtifactsPayload { files });
            emit!(tx, state, lease, event);
        }
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

/// Stamp the tree off the async runtime.
///
/// A walk of a large project is milliseconds of *blocking* work; on a runtime
/// thread it stalls every other request the gateway is serving. `None` means
/// the stamp could not be taken, and the turn simply reports no files rather
/// than guessing at them.
async fn stamp(root: &std::path::Path) -> Option<crate::artifacts::Snapshot> {
    let root = root.to_path_buf();
    match tokio::task::spawn_blocking(move || crate::artifacts::Snapshot::take(&root)).await {
        Ok(snapshot) => Some(snapshot),
        Err(e) => {
            tracing::warn!("could not stamp the project tree for artifacts: {e}");
            None
        }
    }
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

/// What to do about a stream that has gone quiet.
#[derive(Debug, PartialEq, Eq)]
enum Quiet {
    /// Keep waiting, say nothing.
    Wait,
    /// Emit `tool_call_progress` for every call still in flight.
    Report,
    /// The provider is gone. Interrupt and end the leg with an error.
    Abandon,
}

/// The rule that used to be wrong, in one place, with no `Harness` in the way.
///
/// `has_in_flight` is the whole difference: silence with a tool running is that
/// tool working, and no amount of it means the provider died.
fn quiet_action(has_in_flight: bool, silence: Duration, since_progress: Duration) -> Quiet {
    if has_in_flight {
        return if since_progress.as_secs() >= PROGRESS_EVERY_SECS {
            Quiet::Report
        } else {
            Quiet::Wait
        };
    }
    if silence.as_secs() >= PROVIDER_SILENCE_SECS {
        Quiet::Abandon
    } else {
        Quiet::Wait
    }
}

/// A tool call this leg has started and not yet seen a response for.
struct InFlight {
    id: Option<String>,
    name: String,
    started: Instant,
}

/// Drain one event stream, translating adk events into V2 SSE events.
///
/// **A quiet stream is not a failure.** This used to abandon the turn after two
/// silent 120s windows, which was wrong in the one case that matters most: a
/// tool call is a black box to this stream, and `task(...)` runs an entire
/// sub-agent — minutes of real work, its own `timeout_secs` — inside a single
/// call that emits nothing here until it returns. The watchdog fired at 240s,
/// called `interrupt()`, and killed a sub-agent that was working perfectly, with
/// "No response from the provider" as the explanation. The REPL was fixed for
/// this; the gateway was not, so the web UI was the last place it could happen.
///
/// So silence is only fatal when *nothing is running*. While a call is in
/// flight, the wait is reported as progress instead and the turn keeps going —
/// the tool's own timeout bounds its work, and the stop button is the way out.
async fn consume_leg(
    harness: &Harness,
    stream: &mut EventStream,
    tx: &mpsc::Sender<V2StreamEvent>,
) -> LegOutcome {
    let mut outcome = LegOutcome::default();
    let mut in_flight: Vec<InFlight> = Vec::new();
    let mut last_event = Instant::now();
    let mut last_progress = Instant::now();

    macro_rules! send {
        ($event:expr) => {
            if tx.send($event).await.is_err() {
                outcome.client_gone = true;
                return outcome;
            }
        };
    }

    loop {
        let next =
            tokio::time::timeout(Duration::from_secs(STREAM_TICK_SECS), stream.next()).await;

        let event = match next {
            Err(_) => {
                let now = Instant::now();
                match quiet_action(
                    !in_flight.is_empty(),
                    now.duration_since(last_event),
                    now.duration_since(last_progress),
                ) {
                    Quiet::Wait => continue,
                    Quiet::Report => {
                        last_progress = now;
                        for call in &in_flight {
                            send!(V2StreamEvent::ToolCallProgress(ToolCallProgressPayload {
                                id: call.id.clone(),
                                name: call.name.clone(),
                                detail: progress_detail(&call.name),
                                elapsed_secs: now.duration_since(call.started).as_secs(),
                            }));
                        }
                        continue;
                    }
                    Quiet::Abandon => {
                        harness.interrupt();
                        outcome.error = Some(format!(
                            "No response from {} ({}) after {}s.",
                            harness.provider_mgr().current_provider(),
                            harness.provider_mgr().current_model_name(),
                            PROVIDER_SILENCE_SECS,
                        ));
                        return outcome;
                    }
                }
            }
            Ok(None) => return outcome,
            Ok(Some(Err(e))) => {
                outcome.error = Some(format!("Stream error: {e}"));
                return outcome;
            }
            Ok(Some(Ok(event))) => {
                last_event = Instant::now();
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
                        in_flight.push(InFlight {
                            id: id.clone(),
                            name: name.clone(),
                            started: Instant::now(),
                        });
                        // Give the call its full quiet window before the first
                        // progress line, rather than one left over from the
                        // call before it.
                        last_progress = Instant::now();
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
                        // Match on id where there is one; adk omits it for some
                        // providers, and then the name is all we have.
                        let done = in_flight.iter().position(|c| match (&c.id, id) {
                            (Some(a), Some(b)) => a == b,
                            _ => c.name == function_response.name,
                        });
                        if let Some(idx) = done {
                            in_flight.remove(idx);
                        }
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

/// What to say about a call that is still running.
///
/// Only `task` can answer: it keeps a live snapshot of every sub-agent run, and
/// the tool-call count in there is what separates work from a hang. Every other
/// tool is opaque from here — the elapsed time on the event is the whole story.
fn progress_detail(tool_name: &str) -> Option<String> {
    if tool_name != "task" {
        return None;
    }
    let runs = crate::tools::task::active_sub_agents();
    if runs.is_empty() {
        return None;
    }
    Some(
        runs.iter()
            .map(|r| r.label())
            .collect::<Vec<_>>()
            .join(" / "),
    )
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

// ─── G4: MCP server status ─────────────────────────────────────

/// Map a stdio server status to a human-readable failure reason.
///
/// Only genuine failures get an `error`. `stopped` and `disabled` are user
/// intent — surfacing them as errors would make F14 show a red state for a
/// server the user deliberately turned off.
fn stdio_status_error(status: &str) -> Option<String> {
    match status {
        "crashed" => Some("server process exited unexpectedly".to_string()),
        "failedtostart" => Some("server failed to start".to_string()),
        _ => None,
    }
}

/// Clamp a caller-supplied search limit into the supported range.
///
/// Clamped rather than rejected: a UI asking for more than we serve is not a
/// client error, and 400-ing it would be a worse experience than capping.
fn clamp_search_limit(requested: Option<usize>) -> usize {
    requested.unwrap_or(10).clamp(1, 100)
}

/// `GET /v2/mcp/servers` — status of every configured MCP server.
///
/// **On `tool_count`.** stdio servers all share one `McpServerManager` behind a
/// single `"mcp"` prefix, so there is no per-server attribution and the field is
/// `null` for them. Reporting `0` would be a lie the UI cannot distinguish from
/// "connected but exposes nothing" — F14 must render `null` as "—".
pub async fn v2_mcp_servers(State(state): State<GatewayState>) -> Response {
    let harness = state.harness.read().await;
    let mcp = harness.mcp_service();

    let statuses = mcp.all_statuses().await;
    let running = mcp.running_count().await;
    let http_connected = mcp.connected_http_ids();

    let mut servers: Vec<serde_json::Value> = Vec::new();

    // stdio servers — status comes from the manager.
    for id in mcp.configs().keys() {
        let status = statuses
            .get(id)
            .map(|s| format!("{s:?}").to_lowercase())
            .unwrap_or_else(|| "unknown".to_string());
        let error = stdio_status_error(&status);
        servers.push(serde_json::json!({
            "id": id,
            "status": status,
            "transport": "stdio",
            "tool_count": serde_json::Value::Null,
            "error": error,
        }));
    }

    // HTTP servers — the manager doesn't track these; connection state does.
    for (id, connected) in &http_connected {
        servers.push(serde_json::json!({
            "id": id,
            "status": if *connected { "running" } else { "stopped" },
            "transport": "http",
            "tool_count": serde_json::Value::Null,
            "error": if *connected {
                serde_json::Value::Null
            } else {
                serde_json::Value::String("not connected".into())
            },
        }));
    }

    servers.sort_by(|a, b| a["id"].as_str().unwrap_or("").cmp(b["id"].as_str().unwrap_or("")));

    // `running_count()` only knows about the stdio manager, so it under-reports
    // whenever HTTP servers are connected — the UI would show "1 running" next
    // to four green dots. Count what we actually report instead, and keep the
    // manager's number alongside it rather than silently discarding it.
    let running_total = servers
        .iter()
        .filter(|s| s["status"] == "running")
        .count();

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "servers": servers,
            "running": running_total,
            "running_stdio": running,
            "total_tools": serde_json::Value::Null,
        })),
    )
        .into_response()
}

// ─── G5: memory vault search ───────────────────────────────────

/// Query parameters for `GET /v2/memory/search`.
#[derive(serde::Deserialize)]
pub struct MemorySearchQuery {
    pub q: Option<String>,
    pub limit: Option<usize>,
}

/// `GET /v2/memory/search?q=&limit=` — search the memory vault.
///
/// Uses [`ObsidianVault::search`] rather than `MemorySidecar::search_for_context`,
/// which is enrichment-shaped (input → prompt injection) and applies its own
/// relevance gate — see spec C11.
///
/// **Locking.** The vault sits behind a `std::sync::Mutex`. Results are cloned
/// into owned values and the guard is dropped *before* the response is built, so
/// nothing is ever held across an `await` (spec §2.2).
pub async fn v2_memory_search(
    State(state): State<GatewayState>,
    axum::extract::Query(params): axum::extract::Query<MemorySearchQuery>,
) -> Response {
    let query = params.q.unwrap_or_default();
    let query = query.trim();
    if query.is_empty() {
        return v2_error(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "Query parameter 'q' is required and must not be empty.",
            None,
        );
    }
    let limit = clamp_search_limit(params.limit);

    let harness = state.harness.read().await;
    let vault = harness.vault().clone();
    drop(harness);

    let mem_query = crate::memory::types::MemoryQuery {
        query: query.to_string(),
        mode: crate::memory::types::RetrievalMode::GrepLlm,
        levels: None,
        project: None,
        tags: None,
        limit,
    };

    // Scoped so the std Mutex guard is released before we touch the response.
    let results = {
        let guard = match vault.lock() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        };
        match guard.search(&mem_query) {
            Ok(hits) => hits,
            Err(e) => {
                return v2_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal",
                    format!("memory search failed: {e}"),
                    None,
                );
            }
        }
    };

    let results: Vec<serde_json::Value> = results
        .into_iter()
        .map(|r| {
            serde_json::json!({
                "title": r.ref_id,
                "level": r.level,
                "path": r.path.to_string_lossy(),
                "score": r.relevance_score,
                "preview": r.snippet,
            })
        })
        .collect();

    (
        StatusCode::OK,
        Json(serde_json::json!({ "results": results, "count": results.len(), "limit": limit })),
    )
        .into_response()
}

/// `GET /v2/memory/stats` — vault counters and sidecar toggles.
pub async fn v2_memory_stats(State(state): State<GatewayState>) -> Response {
    let harness = state.harness.read().await;
    let vault = harness.vault().clone();
    let auto_search = harness.memory_sidecar().auto_search_enabled();
    let auto_write = harness.memory_sidecar().auto_write_enabled();
    drop(harness);

    // Same discipline as search: copy out, then drop the guard.
    let (stats, counters) = {
        let guard = match vault.lock() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        };
        (guard.stats().clone(), guard.counters().clone())
    };

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "total_memcells": stats.total_memcells,
            "total_events": stats.total_events,
            "total_foresights": stats.total_foresights,
            "total_episodes": stats.total_episodes,
            "pending_foresights": stats.pending_foresights,
            "total_clusters": stats.total_clusters,
            "total_reflections": stats.total_reflections,
            "profile_items": stats.profile_items,
            "counters": {
                "next_event": counters.event + 1,
                "next_foresight": counters.foresight + 1,
                "next_episode": counters.episode + 1,
            },
            "auto_search_enabled": auto_search,
            "auto_write_enabled": auto_write,
        })),
    )
        .into_response()
}

// ─── G8: session messages with tool calls ──────────────────────

/// `GET /v2/sessions/{id}/messages` — session history including tool calls.
///
/// `GET /v1/sessions/{id}` already returns a `messages` array but drops every
/// non-text part (spec C5). This walks the same `session.events().all()` and
/// additionally maps `FunctionCall`/`FunctionResponse`, pairing them by call id
/// so the UI can rebuild tool-call cards on reload exactly as the live stream
/// rendered them.
pub async fn v2_session_messages(
    State(state): State<GatewayState>,
    axum::extract::Path(session_id): axum::extract::Path<String>,
) -> Response {
    let harness = state.harness.read().await;
    let session = match harness.session_mgr().get_session(&session_id).await {
        Ok(s) => s,
        Err(e) => {
            return v2_error(
                StatusCode::NOT_FOUND,
                "not_found",
                format!("Session not found: {e}"),
                None,
            );
        }
    };

    let events = session.events().all();

    // call_id → index into `messages` of the tool_call that is awaiting its
    // result. Responses can arrive in a later event than their call.
    let mut pending: std::collections::HashMap<String, (usize, usize)> =
        std::collections::HashMap::new();
    let mut messages: Vec<serde_json::Value> = Vec::new();

    for event in &events {
        let Some(content) = event.content() else { continue };

        let mut text = String::new();
        let mut tool_calls: Vec<serde_json::Value> = Vec::new();

        for part in &content.parts {
            match part {
                Part::Text { text: t } => text.push_str(t),
                Part::FunctionCall { name, args, id, .. } => {
                    let call_id = id.clone().unwrap_or_else(|| format!("call-{name}"));
                    tool_calls.push(serde_json::json!({
                        "id": call_id,
                        "name": name,
                        "args": args,
                        "status": "pending",
                        "result_preview": serde_json::Value::Null,
                        "truncated": false,
                    }));
                    pending.insert(call_id, (messages.len(), tool_calls.len() - 1));
                }
                Part::FunctionResponse { function_response, id } => {
                    let call_id = id
                        .clone()
                        .unwrap_or_else(|| format!("call-{}", function_response.name));
                    let (preview, truncated) = preview_of(&function_response.response);
                    // Attach to the originating call if we have seen it; a
                    // response with no matching call still gets surfaced rather
                    // than silently dropped.
                    if let Some((mi, ti)) = pending.remove(&call_id) {
                        if let Some(tc) = messages
                            .get_mut(mi)
                            .and_then(|m| m.get_mut("tool_calls"))
                            .and_then(|v| v.get_mut(ti))
                        {
                            tc["status"] = serde_json::json!("done");
                            tc["result_preview"] = serde_json::json!(preview);
                            tc["truncated"] = serde_json::json!(truncated);
                        }
                    } else {
                        tool_calls.push(serde_json::json!({
                            "id": call_id,
                            "name": function_response.name,
                            "args": serde_json::Value::Null,
                            "status": "done",
                            "result_preview": preview,
                            "truncated": truncated,
                        }));
                    }
                }
                _ => {}
            }
        }

        if text.is_empty() && tool_calls.is_empty() {
            continue;
        }

        messages.push(serde_json::json!({
            "role": content.role,
            "content": text,
            "timestamp": event.timestamp,
            "tool_calls": tool_calls,
        }));
    }

    // Anything still unpaired never received a `FunctionResponse`. On a *live*
    // stream that means "still running"; on a finished session being replayed it
    // does not — nothing is pending in history.
    //
    // The common cause is an approval: the pre-approval `FunctionCall` is
    // abandoned when `run_confirmation_turn` starts a fresh turn, and the
    // post-approval call carries a **different** id (spec §5). Leaving these as
    // `pending` would make F11 render a card that spins forever on every reload.
    for (_, (mi, ti)) in pending {
        if let Some(tc) = messages
            .get_mut(mi)
            .and_then(|m| m.get_mut("tool_calls"))
            .and_then(|v| v.get_mut(ti))
        {
            tc["status"] = serde_json::json!("unresolved");
        }
    }

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "session_id": session_id,
            "event_count": events.len(),
            "messages": messages,
        })),
    )
        .into_response()
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
    fn progress_detail_is_only_offered_when_a_tool_can_answer() {
        // shell_exec is opaque from here; the event still carries elapsed time.
        assert_eq!(progress_detail("shell_exec"), None);
        // And `task` says nothing when no sub-agent is actually running, rather
        // than inventing a line for a call that has not started one.
        assert_eq!(progress_detail("task"), None);
    }

    /// The regression this whole path exists for: a `task(...)` sub-agent given
    /// `timeout_secs=600` used to be killed at 240s by the watchdog, and told
    /// the user its provider had stopped responding.
    #[test]
    fn a_running_tool_is_never_read_as_a_dead_provider() {
        let ten_minutes = Duration::from_secs(600);
        assert_eq!(
            quiet_action(true, ten_minutes, Duration::from_secs(0)),
            Quiet::Wait
        );
        assert_eq!(
            quiet_action(true, ten_minutes, Duration::from_secs(PROGRESS_EVERY_SECS)),
            Quiet::Report
        );
    }

    #[test]
    fn a_silent_provider_with_nothing_running_still_ends_the_turn() {
        assert_eq!(
            quiet_action(
                false,
                Duration::from_secs(PROVIDER_SILENCE_SECS),
                Duration::from_secs(0)
            ),
            Quiet::Abandon
        );
        // One tick short of the budget is still just a slow model.
        assert_eq!(
            quiet_action(
                false,
                Duration::from_secs(PROVIDER_SILENCE_SECS - STREAM_TICK_SECS),
                Duration::from_secs(0)
            ),
            Quiet::Wait
        );
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

    // ─── G4 ────────────────────────────────────────────────────

    #[test]
    fn only_real_failures_report_an_mcp_error() {
        assert!(stdio_status_error("crashed").is_some());
        assert!(stdio_status_error("failedtostart").is_some());
        // User intent, not failure — F14 must not paint these red.
        assert!(stdio_status_error("stopped").is_none());
        assert!(stdio_status_error("disabled").is_none());
        assert!(stdio_status_error("running").is_none());
        assert!(stdio_status_error("restarting").is_none());
    }

    // ─── G5 ────────────────────────────────────────────────────

    #[test]
    fn search_limit_is_clamped_not_rejected() {
        assert_eq!(clamp_search_limit(None), 10);
        assert_eq!(clamp_search_limit(Some(3)), 3);
        // Spec G5 acceptance: `limit=1000` is clamped, not honoured.
        assert_eq!(clamp_search_limit(Some(1000)), 100);
        // Zero would produce an empty result set for a valid-looking request.
        assert_eq!(clamp_search_limit(Some(0)), 1);
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

/// `PATCH /v2/sessions/{id}` — rename a session.
///
/// A title typed here is locked: auto-titling only ever fills a blank, so the
/// next turn will not quietly replace a name someone chose.
pub async fn v2_session_rename(
    State(state): State<GatewayState>,
    axum::extract::Path(session_id): axum::extract::Path<String>,
    Json(req): Json<serde_json::Value>,
) -> Response {
    let Some(title) = req.get("title").and_then(|v| v.as_str()) else {
        return v2_error(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "Expected a JSON body with a `title` string.",
            None,
        );
    };

    let harness = state.harness.read().await;
    match harness.session_mgr().rename_session(&session_id, title).await {
        Ok(()) => {
            // Read it back rather than echoing the input: the stored title is
            // trimmed and clipped, and the caller should see what was kept.
            let stored = harness
                .session_mgr()
                .list_sessions()
                .await
                .ok()
                .and_then(|all| all.into_iter().find(|s| s.id == session_id))
                .and_then(|s| s.title);
            (
                StatusCode::OK,
                Json(serde_json::json!({ "id": session_id, "title": stored })),
            )
                .into_response()
        }
        Err(e) => v2_error(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            format!("{e}"),
            None,
        ),
    }
}
