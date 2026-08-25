//! `/v2/routines` — the scheduled-work surface.
//!
//! Handlers are deliberately thin. Every rule about what a routine may be, when
//! it is due and who it goes to lives in [`crate::routine`], and every document
//! these return is built by [`crate::routine::view`] — the same one the
//! `momo-fetch routine` CLI prints. A panel and an agent driving `shell_exec`
//! therefore cannot end up with different pictures of the same routine.
//!
//! **These never touch the harness.** They read and write `.harness/routines/`
//! and spawn detached processes; nothing here takes the harness lock, so the
//! scheduler cannot stall behind a running turn and a routine firing cannot
//! interleave with what the user is typing.

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Deserialize;
use serde_json::json;

use crate::routine::{
    Assignee, Concurrency, Routine, RoutineService, TaskTemplate, Trigger, view,
};

use super::GatewayState;
use super::v2_types::v2_error;

/// Create/update body.
///
/// Every field optional so `PATCH` can carry just the one that changed; `POST`
/// checks the required ones itself and says which is missing, rather than
/// rejecting the whole body with a serde message about a field name.
#[derive(Debug, Deserialize)]
pub struct RoutineUpsert {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub assignee: Option<Assignee>,
    #[serde(default)]
    pub trigger: Option<Trigger>,
    #[serde(default)]
    pub concurrency: Option<Concurrency>,
    #[serde(default)]
    pub task: Option<TaskTemplate>,
    #[serde(default)]
    pub permission: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct RunsQuery {
    #[serde(default = "default_run_limit")]
    pub limit: usize,
}

fn default_run_limit() -> usize {
    20
}

fn open(state: &GatewayState) -> Result<RoutineService, Response> {
    RoutineService::new(&state.project_path).map_err(|e| {
        v2_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "routine_store_unavailable",
            format!("Could not open the routine store: {e}"),
            None,
        )
    })
}

fn invalid(message: String) -> Response {
    v2_error(StatusCode::BAD_REQUEST, "invalid_routine", &message, None)
}

fn missing(id: &str) -> Response {
    v2_error(
        StatusCode::NOT_FOUND,
        "routine_not_found",
        format!("No routine '{id}'."),
        None,
    )
}

// ── GET /v2/routines ───────────────────────────────────────────────

pub async fn v2_routines(State(state): State<GatewayState>) -> Response {
    let service = match open(&state) {
        Ok(s) => s,
        Err(r) => return r,
    };
    let routines: Vec<_> = service
        .list()
        .into_iter()
        .map(|r| view::routine_payload(&service, r))
        .collect();
    (
        StatusCode::OK,
        Json(json!({ "routines": routines, "count": routines.len() })),
    )
        .into_response()
}

// ── GET /v2/routines/assignees ─────────────────────────────────────

/// Who a routine can be handed to in this project *right now*.
///
/// A live lookup rather than a static list: the agents come from
/// `.harness/agents/`, and the standby workers only exist while a team is up.
pub async fn v2_routine_assignees(State(state): State<GatewayState>) -> Response {
    (
        StatusCode::OK,
        Json(view::assignees_payload(&state.project_path)),
    )
        .into_response()
}

// ── POST /v2/routines ──────────────────────────────────────────────

pub async fn v2_routines_create(
    State(state): State<GatewayState>,
    Json(body): Json<RoutineUpsert>,
) -> Response {
    let mut service = match open(&state) {
        Ok(s) => s,
        Err(r) => return r,
    };

    let (Some(name), Some(trigger), Some(task)) = (body.name, body.trigger, body.task) else {
        return invalid(
            "A routine needs a name, a trigger and a task (title + description).".to_string(),
        );
    };

    let routine = Routine {
        id: String::new(),
        name,
        enabled: body.enabled.unwrap_or(true),
        assignee: body.assignee.unwrap_or(Assignee::Lead),
        trigger,
        concurrency: body.concurrency.unwrap_or_default(),
        task,
        permission: body
            .permission
            .unwrap_or_else(|| crate::routine::DEFAULT_ROUTINE_PERMISSION.to_string()),
        created_at: 0,
        updated_at: 0,
    };

    match service.create(routine) {
        Ok(created) => (
            StatusCode::CREATED,
            Json(view::routine_payload(&service, &created)),
        )
            .into_response(),
        Err(e) => invalid(e.to_string()),
    }
}

// ── GET /v2/routines/{id} ──────────────────────────────────────────

pub async fn v2_routine_get(
    State(state): State<GatewayState>,
    Path(id): Path<String>,
) -> Response {
    let service = match open(&state) {
        Ok(s) => s,
        Err(r) => return r,
    };
    let Some(routine) = service.get(&id) else {
        return missing(&id);
    };
    let mut payload = view::routine_payload(&service, routine);
    if let Some(obj) = payload.as_object_mut() {
        let runs: Vec<_> = service
            .runs(Some(&id), 20)
            .into_iter()
            .map(view::run_payload)
            .collect();
        obj.insert("runs".to_string(), json!(runs));
    }
    (StatusCode::OK, Json(payload)).into_response()
}

// ── PATCH /v2/routines/{id} ────────────────────────────────────────

pub async fn v2_routine_update(
    State(state): State<GatewayState>,
    Path(id): Path<String>,
    Json(body): Json<RoutineUpsert>,
) -> Response {
    let mut service = match open(&state) {
        Ok(s) => s,
        Err(r) => return r,
    };
    let Some(existing) = service.get(&id).cloned() else {
        return missing(&id);
    };

    // Unset means "leave it alone": the enable toggle sends one field, the form
    // sends all of them, and both have to be the same request.
    let merged = Routine {
        name: body.name.unwrap_or_else(|| existing.name.clone()),
        enabled: body.enabled.unwrap_or(existing.enabled),
        assignee: body.assignee.unwrap_or_else(|| existing.assignee.clone()),
        trigger: body.trigger.unwrap_or_else(|| existing.trigger.clone()),
        concurrency: body.concurrency.unwrap_or(existing.concurrency),
        task: body.task.unwrap_or_else(|| existing.task.clone()),
        permission: body
            .permission
            .unwrap_or_else(|| existing.permission.clone()),
        ..existing.clone()
    };

    // `set_enabled` re-anchors the schedule, so an enable must go through it
    // rather than through a plain field write — otherwise switching a routine
    // back on replays every window it missed while it was off.
    let enable_changed = merged.enabled != existing.enabled;
    match service.update(&id, merged) {
        Ok(saved) => {
            let saved = if enable_changed {
                match service.set_enabled(&id, saved.enabled) {
                    Ok(s) => s,
                    Err(e) => return invalid(e.to_string()),
                }
            } else {
                saved
            };
            (
                StatusCode::OK,
                Json(view::routine_payload(&service, &saved)),
            )
                .into_response()
        }
        Err(e) => invalid(e.to_string()),
    }
}

// ── DELETE /v2/routines/{id} ───────────────────────────────────────

pub async fn v2_routine_delete(
    State(state): State<GatewayState>,
    Path(id): Path<String>,
) -> Response {
    let mut service = match open(&state) {
        Ok(s) => s,
        Err(r) => return r,
    };
    match service.delete(&id) {
        Ok(true) => (StatusCode::OK, Json(json!({ "deleted": true, "id": id }))).into_response(),
        Ok(false) => missing(&id),
        Err(e) => v2_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "delete_failed",
            e.to_string(),
            None,
        ),
    }
}

// ── POST /v2/routines/{id}/run ─────────────────────────────────────

pub async fn v2_routine_run(
    State(state): State<GatewayState>,
    Path(id): Path<String>,
) -> Response {
    let mut service = match open(&state) {
        Ok(s) => s,
        Err(r) => return r,
    };
    let Some(routine) = service.get(&id).cloned() else {
        return missing(&id);
    };

    let _ = service.reconcile();
    let active = service.active_runs(&id);
    if active > 0 && routine.concurrency != Concurrency::Parallel {
        // 409, the same status the turn registry uses for "something of yours
        // is already running" — the UI branches on the code, not the prose.
        return v2_error(
            StatusCode::CONFLICT,
            "run_in_progress",
            format!("A run of '{}' is still going.", routine.name),
            Some(json!({ "id": id, "active_runs": active })),
        );
    }

    match service.run_now(&id) {
        Ok(record) => (StatusCode::OK, Json(view::run_payload(&record))).into_response(),
        Err(e) => v2_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "run_failed",
            e.to_string(),
            None,
        ),
    }
}

// ── GET /v2/routines/{id}/runs ─────────────────────────────────────

pub async fn v2_routine_runs(
    State(state): State<GatewayState>,
    Path(id): Path<String>,
    Query(query): Query<RunsQuery>,
) -> Response {
    let service = match open(&state) {
        Ok(s) => s,
        Err(r) => return r,
    };
    if service.get(&id).is_none() {
        return missing(&id);
    }
    let runs: Vec<_> = service
        .runs(Some(&id), query.limit.clamp(1, crate::routine::MAX_RUN_HISTORY))
        .into_iter()
        .map(view::run_payload)
        .collect();
    (
        StatusCode::OK,
        Json(json!({ "runs": runs, "count": runs.len() })),
    )
        .into_response()
}

// ── POST /v2/routines/tick ─────────────────────────────────────────

/// Advance the scheduler now instead of waiting for the next tick.
///
/// The gateway already ticks on a timer; this exists so the UI's "check now"
/// and a test can force the step without a sleep.
pub async fn v2_routines_tick(State(state): State<GatewayState>) -> Response {
    let mut service = match open(&state) {
        Ok(s) => s,
        Err(r) => return r,
    };
    match service.tick(crate::routine::now_millis()) {
        Ok(report) => (StatusCode::OK, Json(view::tick_payload(&report))).into_response(),
        Err(e) => v2_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "tick_failed",
            e.to_string(),
            None,
        ),
    }
}
