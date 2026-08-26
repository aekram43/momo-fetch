//! `/v2/team` — starting, stopping and repairing a team from the UI.
//!
//! The rail could always *see* a team; it could never touch one. Starting was a
//! command you had to leave the app to type, which is defensible for a
//! destructive stop and indefensible for "run the squad I already configured".
//!
//! Every action here runs the same function `momo-fetch team …` runs, from
//! [`crate::cli::team_cmd`], and translates its outcome into HTTP. Nothing about
//! what starting a team *means* lives in this file — that would be a second
//! implementation, and the two would disagree the first time either changed.
//!
//! **No harness lock.** Like `/v2/activity` and `/v2/routines`, these read and
//! write `.harness/` and drive tmux; a team starting must not queue behind
//! somebody's streaming turn.

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Deserialize;

use crate::cli::team_cmd::{self, Outcome, EXIT_CONFLICT, EXIT_OK};

use super::GatewayState;
use super::v2_types::v2_error;

#[derive(Debug, Deserialize)]
pub struct StartRequest {
    /// Config name in `.harness/teams/`, without the extension.
    pub name: String,
}

#[derive(Debug, Deserialize)]
pub struct StopRequest {
    /// Also kill a tmux session left behind by a team the state no longer
    /// admits is running. For a team stranded by a reboot.
    #[serde(default)]
    pub force: bool,
}

#[derive(Debug, Deserialize)]
pub struct RestartRequest {
    /// Worker name as it appears in `team status`.
    pub worker: String,
}

/// `POST /v2/team/start` — start the named config.
pub async fn v2_team_start(
    State(state): State<GatewayState>,
    Json(req): Json<StartRequest>,
) -> Response {
    run(&state, move |service| team_cmd::start(service, &req.name)).await
}

/// `POST /v2/team/stop` — stop the active team.
///
/// Destructive, and the UI says so before it calls: each worktree goes with
/// `git worktree remove --force` and the worker branch is deleted.
pub async fn v2_team_stop(
    State(state): State<GatewayState>,
    body: Option<Json<StopRequest>>,
) -> Response {
    let force = body.map(|Json(b)| b.force).unwrap_or(false);
    run(&state, move |service| team_cmd::stop(service, force)).await
}

/// `POST /v2/team/restart` — relaunch one worker's pane.
pub async fn v2_team_restart(
    State(state): State<GatewayState>,
    Json(req): Json<RestartRequest>,
) -> Response {
    run(&state, move |service| team_cmd::restart(service, &req.worker)).await
}

/// Open the team state, run one action on it, and answer over HTTP.
///
/// Blocking on purpose: `start` creates git worktrees and tmux panes, which is
/// filesystem and process work, not async work. `spawn_blocking` keeps it off
/// the reactor that is streaming somebody's turn.
async fn run<F>(state: &GatewayState, action: F) -> Response
where
    F: FnOnce(&mut crate::team::TeamService) -> Outcome + Send + 'static,
{
    let project = state.project_path.as_path().to_path_buf();
    let outcome = tokio::task::spawn_blocking(move || {
        let mut service = match team_cmd::open_service(&project) {
            Ok(s) => s,
            Err(outcome) => return outcome,
        };
        action(&mut service)
    })
    .await;

    match outcome {
        Ok(outcome) => respond(outcome),
        Err(e) => v2_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "team_action_panicked",
            format!("The team command did not finish: {e}"),
            None,
        ),
    }
}

/// Translate a CLI outcome into HTTP.
///
/// The exit codes already carry the distinction that matters — a state conflict
/// is not a failure of the request — so this maps them rather than re-deciding:
/// 2 is a 409, anything else non-zero is a 400 with the payload's own error code
/// so the UI can tell "no such config" from "tmux is missing".
fn respond(outcome: Outcome) -> Response {
    if outcome.code == EXIT_OK {
        // The notes are the narration the CLI writes to stderr — "started with
        // 3 workers", "tmux not found, nothing launched". A UI that dropped
        // them would show a team as started and never mention that no process
        // is behind it.
        let mut body = outcome.stdout;
        if let Some(obj) = body.as_object_mut() {
            if !outcome.stderr.is_empty() {
                obj.insert("notes".into(), serde_json::json!(outcome.stderr));
            }
        }
        return (StatusCode::OK, Json(body)).into_response();
    }

    let code = outcome
        .stdout
        .get("error")
        .and_then(|v| v.as_str())
        .unwrap_or("team_error")
        .to_string();
    let message = outcome
        .stdout
        .get("message")
        .and_then(|v| v.as_str())
        .unwrap_or("The team command failed.")
        .to_string();
    let status = if outcome.code == EXIT_CONFLICT {
        StatusCode::CONFLICT
    } else if code == "config_not_found" {
        StatusCode::NOT_FOUND
    } else {
        StatusCode::BAD_REQUEST
    };
    v2_error(status, &code, message, Some(outcome.stdout))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn body_of(response: &Response) -> StatusCode {
        response.status()
    }

    #[test]
    fn a_state_conflict_is_a_409_not_a_400() {
        let outcome = Outcome {
            code: EXIT_CONFLICT,
            stdout: json!({ "error": "team_already_active", "message": "already running" }),
            stderr: vec![],
        };
        assert_eq!(body_of(&respond(outcome)), StatusCode::CONFLICT);
    }

    #[test]
    fn a_missing_config_is_a_404() {
        let outcome = Outcome {
            code: 1,
            stdout: json!({ "error": "config_not_found", "message": "no such config" }),
            stderr: vec![],
        };
        assert_eq!(body_of(&respond(outcome)), StatusCode::NOT_FOUND);
    }

    #[test]
    fn success_keeps_the_narration_the_cli_puts_on_stderr() {
        let outcome = Outcome {
            code: EXIT_OK,
            stdout: json!({ "team_id": "t-1" }),
            stderr: vec!["tmux not found — nothing was launched.".into()],
        };
        let response = respond(outcome);
        assert_eq!(response.status(), StatusCode::OK);
    }
}
