//! `GET /v2/activity` — everything working in the background, right now.
//!
//! The right panel answers "what is the agent doing", and until this existed it
//! could only answer for the turn in front of you. A team worker in a tmux pane
//! and a routine run in its own process are both work this session started and
//! neither showed up anywhere you were already looking.
//!
//! Like `/v2/routines`, this takes **no harness lock** — it reads `.harness/`
//! and nothing else. That matters more here than there: a panel that polls
//! every few seconds must never be able to queue up behind a streaming turn.

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;

use crate::routine::{RoutineService, view};
use crate::team::TeamService;

use super::GatewayState;

pub async fn v2_activity(State(state): State<GatewayState>) -> Response {
    let project = state.project_path.as_path();

    // A project with no team and no routines is the common case, and it must
    // answer with empty lists rather than an error — the panel renders "nothing
    // running", which is a real answer.
    let (team, team_configs) = match TeamService::new(project) {
        Ok(mut service) => {
            // Refresh liveness first: a worker whose pane died is the single
            // most useful thing this endpoint can tell anyone.
            service.status();
            // The configs come too, so an idle rail can say *what could run*
            // rather than only that nothing is. Starting one is still a
            // command — see the note on the Squad panel.
            let configs = service.list_team_configs().unwrap_or_default();
            (Some(crate::cli::team_cmd::team_payload(&service)), configs)
        }
        Err(_) => (None, Vec::new()),
    };

    let (runs, routines_armed) = match RoutineService::new(project) {
        Ok(mut service) => {
            // Settle finished runs here rather than waiting for the scheduler
            // tick, or a run that ended seconds ago reads "running" for half a
            // minute in a panel that refreshes every five.
            let _ = service.reconcile();
            let active: Vec<_> = service
                .runs(None, crate::routine::MAX_RUN_HISTORY)
                .into_iter()
                .filter(|r| r.status.is_active())
                .map(view::run_payload)
                .collect();
            let armed = service.list().iter().filter(|r| r.enabled).count();
            (active, armed)
        }
        Err(_) => (Vec::new(), 0),
    };

    let workers_running = team
        .as_ref()
        .and_then(|t| t.get("workers"))
        .and_then(|w| w.as_array())
        .map(|workers| {
            workers
                .iter()
                .filter(|w| w.get("status").and_then(|s| s.as_str()) == Some("running"))
                .count()
        })
        .unwrap_or(0);

    (
        StatusCode::OK,
        Json(json!({
            "team": team,
            "team_configs": team_configs,
            "runs": runs,
            "counts": {
                "workers_running": workers_running,
                "runs_active": runs.len(),
                "routines_armed": routines_armed,
            },
        })),
    )
        .into_response()
}
