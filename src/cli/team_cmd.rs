//! Headless team control — `momo-fetch team …`.
//!
//! The REPL's `/team` is for a human at a prompt. This is the same lifecycle
//! for the agent *in* that session: it drives teams through `shell_exec`, so
//! every command prints one JSON document on stdout and nothing else, keeps
//! human-readable narration on stderr, and reports state conflicts through the
//! exit code rather than through prose.
//!
//! ```text
//! 0  success
//! 1  error (bad config, unreadable state, start failed)
//! 2  state conflict (a team is already active)
//! ```
//!
//! Both this and `/team` load configs through [`TeamService::load_team_config`]
//! so the two paths cannot disagree about what a config says.

use std::path::PathBuf;

use serde_json::{Value, json};

use crate::team::{TeamService, TeamState, TeamStatus, WorkerStatus};

pub const EXIT_OK: i32 = 0;
pub const EXIT_ERROR: i32 = 1;
pub const EXIT_CONFLICT: i32 = 2;

/// Project selection, repeated on every `team` action so the flag can follow
/// the action the way `momo-fetch team status --project /repo` reads.
#[derive(clap::Args, Debug, Clone, Default)]
pub struct TeamScope {
    /// Project directory holding `.harness/` (default: current directory)
    #[arg(long = "project", value_name = "DIR")]
    pub project: Option<String>,
}

#[derive(clap::Subcommand, Debug)]
pub enum TeamAction {
    /// Start a team from `.harness/teams/<name>.{json,yml,yaml}`
    ///
    /// Exits 2 with {"error":"team_already_active"} if a team is running.
    Start {
        /// Config name, without the extension
        name: String,
        #[command(flatten)]
        scope: TeamScope,
    },

    /// Stop the active team — kills the tmux session, removes worktrees
    ///
    /// Idempotent: exits 0 with {"status":"no_active_team"} when none is
    /// running. Destructive: each worktree goes with `git worktree remove
    /// --force` and the worker branch is deleted, so merge before stopping.
    Stop {
        #[command(flatten)]
        scope: TeamScope,
        /// Also kill the tmux session recorded in `.harness/team.json` even
        /// when the state no longer says the team is running, and drop the
        /// state file. For a team left stranded by a reboot.
        #[arg(long)]
        force: bool,
    },

    /// Print the active team, its workers and their mailbox backlog
    Status {
        #[command(flatten)]
        scope: TeamScope,
    },

    /// List the configs in `.harness/teams/` and which one is active
    List {
        #[command(flatten)]
        scope: TeamScope,
    },
}

impl TeamAction {
    fn scope(&self) -> &TeamScope {
        match self {
            Self::Start { scope, .. }
            | Self::Stop { scope, .. }
            | Self::Status { scope }
            | Self::List { scope } => scope,
        }
    }
}

/// What a `team` command produced: exactly one JSON document, whatever
/// narration belongs on stderr, and the process exit code.
struct Outcome {
    code: i32,
    stdout: Value,
    stderr: Vec<String>,
}

impl Outcome {
    fn ok(stdout: Value) -> Self {
        Self { code: EXIT_OK, stdout, stderr: Vec::new() }
    }

    fn error(code: i32, error: &str, message: String, extra: Value) -> Self {
        let mut stdout = json!({ "error": error, "message": message });
        if let (Some(obj), Some(extra)) = (stdout.as_object_mut(), extra.as_object()) {
            for (k, v) in extra {
                obj.insert(k.clone(), v.clone());
            }
        }
        Self { code, stdout, stderr: vec![message] }
    }

    fn note(mut self, msg: impl Into<String>) -> Self {
        self.stderr.push(msg.into());
        self
    }
}

/// Run a `team` subcommand, print its result, and return the process exit code.
///
/// `fallback_project` is the top-level `--project`, used when the action did
/// not carry one of its own.
pub fn run(action: &TeamAction, fallback_project: Option<&str>) -> i32 {
    let outcome = execute(action, fallback_project);

    for line in &outcome.stderr {
        eprintln!("{line}");
    }
    println!("{}", serde_json::to_string_pretty(&outcome.stdout).unwrap_or_else(|e| {
        format!("{{\"error\":\"serialize_failed\",\"message\":\"{e}\"}}")
    }));

    outcome.code
}

fn execute(action: &TeamAction, fallback_project: Option<&str>) -> Outcome {
    let project = match resolve_project(action.scope().project.as_deref().or(fallback_project)) {
        Ok(p) => p,
        Err(e) => {
            return Outcome::error(EXIT_ERROR, "invalid_project", e.to_string(), json!({}));
        }
    };

    let mut service = match TeamService::new(&project) {
        Ok(s) => s,
        Err(e) => {
            return Outcome::error(
                EXIT_ERROR,
                "state_error",
                format!("Could not open team state in {}: {e}", project.display()),
                json!({ "project_path": project.display().to_string() }),
            );
        }
    };

    match action {
        TeamAction::Start { name, .. } => start(&mut service, name),
        TeamAction::Stop { force, .. } => stop(&mut service, *force),
        TeamAction::Status { .. } => status(&mut service),
        TeamAction::List { .. } => list(&mut service),
    }
}

/// Resolve `--project` to an absolute path — `TeamService` joins `.harness`
/// onto it, so a relative path would silently follow the caller's cwd.
fn resolve_project(project: Option<&str>) -> anyhow::Result<PathBuf> {
    let path = match project {
        Some(p) => PathBuf::from(p),
        None => return Ok(std::env::current_dir()?),
    };

    if !path.exists() {
        anyhow::bail!("Project directory '{}' does not exist", path.display());
    }
    if !path.is_dir() {
        anyhow::bail!("Project path '{}' is not a directory", path.display());
    }

    Ok(std::fs::canonicalize(&path)?)
}

// ─── Actions ───────────────────────────────────────────────────────

fn start(service: &mut TeamService, name: &str) -> Outcome {
    if let Some(state) = service.state() {
        return Outcome::error(
            EXIT_CONFLICT,
            "team_already_active",
            format!(
                "Team '{}' is already active. Stop it first: momo-fetch team stop",
                state.id
            ),
            json!({ "active": state.id, "active_name": state.name }),
        );
    }

    let config = match service.load_team_config(name) {
        Ok(c) => c,
        Err(e) => {
            return Outcome::error(
                EXIT_ERROR,
                "config_not_found",
                e.to_string(),
                json!({ "requested": name }),
            );
        }
    };

    let mut workers = TeamService::config_to_workers(&config);
    let mut notes = Vec::new();

    // Same two degradations the REPL applies, reported instead of drawn.
    if !crate::team::TmuxManager::is_available() {
        notes.push(
            "tmux not found — the team will be recorded but no worker process starts."
                .to_string(),
        );
    }
    if workers.iter().any(|w| w.use_worktree.unwrap_or(false))
        && !crate::team::WorktreeManager::is_git_repo(service.project_path())
    {
        notes.push("Not a git repository — worktrees disabled for every worker.".to_string());
        for w in &mut workers {
            w.use_worktree = None;
        }
    }

    let worker_count = workers.len();
    match service.start(Some(name.to_string()), workers) {
        Ok(team_id) => {
            let mut outcome = Outcome::ok(team_payload(service.state()));
            outcome = outcome.note(format!(
                "Team '{team_id}' started from config '{name}' with {worker_count} worker(s)."
            ));
            for note in notes {
                outcome = outcome.note(note);
            }
            outcome
        }
        Err(e) => Outcome::error(
            EXIT_ERROR,
            "start_failed",
            e.to_string(),
            json!({ "requested": name }),
        ),
    }
}

fn stop(service: &mut TeamService, force: bool) -> Outcome {
    if force {
        return match service.stop_force() {
            Ok(report) => {
                let stopped =
                    report.stopped_team.is_some() || !report.killed_sessions.is_empty();
                let note = if stopped {
                    "Forced stop: tmux session killed, worktrees removed, branches deleted."
                } else {
                    "No active team and no stale state; nothing to stop."
                };
                Outcome::ok(json!({
                    "status": if stopped { "stopped" } else { "no_active_team" },
                    "forced": true,
                    "team_id": report.stopped_team,
                    "killed_sessions": report.killed_sessions,
                }))
                .note(note)
            }
            Err(e) => Outcome::error(EXIT_ERROR, "stop_failed", e.to_string(), json!({})),
        };
    }

    let Some(team_id) = service.state().map(|s| s.id.clone()) else {
        // Idempotent on purpose: an agent that stops twice has not made a
        // mistake worth an error.
        return Outcome::ok(json!({ "status": "no_active_team", "forced": false }))
            .note("No active team; nothing to stop.");
    };

    // Asked before the stop, so `killed_sessions` reports what was actually
    // there rather than what the state file assumed.
    let session = crate::team::TmuxManager::session_name(&team_id);
    let killed: Vec<String> = if crate::team::TmuxManager::has_session(&session) {
        vec![session]
    } else {
        Vec::new()
    };

    match service.stop() {
        Ok(()) => Outcome::ok(json!({
            "status": "stopped",
            "forced": false,
            "team_id": team_id,
            "killed_sessions": killed,
        }))
        .note(format!(
            "Team '{team_id}' stopped: tmux session killed, worktrees removed, branches deleted."
        )),
        Err(e) => Outcome::error(
            EXIT_ERROR,
            "stop_failed",
            e.to_string(),
            json!({ "team_id": team_id }),
        ),
    }
}

fn status(service: &mut TeamService) -> Outcome {
    // Drains the mailbox into worker state, exactly as `/team status` does.
    service.status();
    Outcome::ok(team_payload(service.state()))
}

fn list(service: &mut TeamService) -> Outcome {
    let names = match service.list_team_configs() {
        Ok(n) => n,
        Err(e) => {
            return Outcome::error(
                EXIT_ERROR,
                "list_failed",
                format!("Could not read {}: {e}", service.teams_dir().display()),
                json!({}),
            );
        }
    };

    let teams: Vec<Value> = names
        .iter()
        .map(|name| {
            let path = service
                .team_config_path(name)
                .map(|p| p.display().to_string());
            match service.load_team_config(name) {
                Ok(config) => json!({
                    "name": name,
                    "path": path,
                    "display_name": config.name,
                    "workers": config.workers.iter().map(|w| json!({
                        "name": w.name,
                        "agent": w.agent,
                        "worktree": w.worktree.unwrap_or(false),
                        "permission": w.permission.clone()
                            .unwrap_or_else(|| crate::team::DEFAULT_WORKER_PERMISSION.to_string()),
                    })).collect::<Vec<_>>(),
                }),
                // A config that will not parse still belongs in the listing —
                // hiding it is how you end up debugging "my team vanished".
                Err(e) => json!({
                    "name": name,
                    "path": path,
                    "error": e.to_string(),
                }),
            }
        })
        .collect();

    let (active, active_name) = match service.state() {
        Some(s) => (Some(s.id.clone()), s.name.clone()),
        None => (None, None),
    };

    Outcome::ok(json!({
        "teams_dir": service.teams_dir().display().to_string(),
        "teams": teams,
        "active": active,
        "active_name": active_name,
    }))
}

// ─── JSON shaping ──────────────────────────────────────────────────

/// The `team status` document — also what `team start` returns, so a caller
/// gets the same shape whether it just started the team or asked about it.
fn team_payload(state: Option<&TeamState>) -> Value {
    let Some(state) = state else {
        return json!({
            "team_id": null,
            "name": null,
            "status": TeamStatus::Idle.to_string(),
            "workers": [],
            "mailbox": { "unread_by_recipient": {} },
            "started_at": null,
            "project_path": null,
        });
    };

    // Sorted: HashMap order is random, and a caller diffing two `status`
    // calls should see worker changes, not reshuffles.
    let mut workers: Vec<_> = state.workers.values().collect();
    workers.sort_by(|a, b| a.name.cmp(&b.name));

    let unread = unread_by_recipient(state);

    json!({
        "team_id": state.id,
        "name": state.name,
        "status": state.status.to_string(),
        "workers": workers.iter().map(|w| json!({
            "name": w.name,
            "agent": w.agent,
            "permission": w.permission,
            "pane_id": w.pane_id,
            "worktree_path": if w.use_worktree { Some(w.work_dir.display().to_string()) } else { None },
            "work_dir": w.work_dir.display().to_string(),
            "branch": w.branch,
            "status": w.status.to_string(),
            "error": match &w.status {
                WorkerStatus::Failed(e) => Some(e.clone()),
                _ => None,
            },
            "last_message_ts": w.last_message_ts.and_then(iso8601),
            "task": w.task,
            "result": w.result,
        })).collect::<Vec<_>>(),
        "mailbox": { "unread_by_recipient": unread },
        "started_at": state.started_at.and_then(iso8601),
        "completed_at": state.completed_at.and_then(iso8601),
        "project_path": state.project_path.display().to_string(),
        "mailbox_path": state.mailbox_path.display().to_string(),
    })
}

/// Queued messages per recipient, with every worker and `lead` present so a
/// caller can read a zero instead of having to treat "absent" as one.
fn unread_by_recipient(state: &TeamState) -> serde_json::Map<String, Value> {
    let mut counts: std::collections::BTreeMap<String, usize> = state
        .workers
        .keys()
        .map(|name| (name.clone(), 0))
        .collect();
    counts.insert("lead".to_string(), 0);

    if let Ok(mailbox) = crate::team::Mailbox::open(&state.mailbox_path) {
        if let Ok(actual) = mailbox.unread_by_recipient() {
            for (recipient, count) in actual {
                counts.insert(recipient, count);
            }
        }
    }

    counts
        .into_iter()
        .map(|(k, v)| (k, json!(v)))
        .collect()
}

/// Unix millis → ISO 8601 / RFC 3339 UTC, the one time format in this output.
fn iso8601(millis: u64) -> Option<Value> {
    chrono::DateTime::from_timestamp_millis(millis as i64)
        .map(|dt| json!(dt.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)))
}

// ─── Tests ─────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// A project with a team config and, optionally, a `team.json` claiming a
    /// team is running. Nothing here starts a process: the CLI's whole view of
    /// "is a team active" is that file, so a fabricated one exercises the
    /// conflict path without tmux — which CI does not have.
    fn project(config: Option<&str>, running: bool) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let harness = dir.path().join(".harness");
        std::fs::create_dir_all(harness.join("teams")).unwrap();

        if let Some(body) = config {
            std::fs::write(harness.join("teams").join("squad.json"), body).unwrap();
        }

        if running {
            std::fs::write(
                harness.join("team.json"),
                serde_json::to_string_pretty(&json!({
                    "id": "team-20260101-000000",
                    "name": "squad",
                    "status": "Running",
                    "project_path": dir.path(),
                    "mailbox_path": harness.join("mailbox"),
                    "workers": {
                        "analyst": {
                            "name": "analyst",
                            "task": "gather evidence",
                            "agent": "yolo-analyst",
                            "branch": "team/analyst",
                            "use_worktree": false,
                            "status": "Starting",
                            "work_dir": dir.path(),
                            "pane_id": "%1",
                            "pid": null,
                            "result": null,
                            "last_message_ts": null
                        }
                    },
                    "created_at": 1735689600000u64,
                    "started_at": 1735689600000u64,
                    "completed_at": null
                }))
                .unwrap(),
            )
            .unwrap();
        }

        dir
    }

    const SQUAD: &str = r#"{
      "name": "squad",
      "workers": [
        {"name": "analyst", "task": "gather evidence", "agent": "yolo-analyst"},
        {"name": "executor", "task": "trade", "agent": "yolo-executor"}
      ]
    }"#;

    fn scope(dir: &tempfile::TempDir) -> TeamScope {
        TeamScope {
            project: Some(dir.path().display().to_string()),
        }
    }

    #[test]
    fn status_is_idle_with_no_team() {
        let dir = project(Some(SQUAD), false);
        let out = execute(&TeamAction::Status { scope: scope(&dir) }, None);

        assert_eq!(out.code, EXIT_OK);
        assert_eq!(out.stdout["status"], "idle");
        assert!(out.stdout["team_id"].is_null());
        assert_eq!(out.stdout["workers"].as_array().unwrap().len(), 0);
        assert!(out.stdout["started_at"].is_null());
    }

    #[test]
    fn stop_without_a_team_is_idempotent() {
        let dir = project(Some(SQUAD), false);

        for _ in 0..2 {
            let out = execute(
                &TeamAction::Stop { scope: scope(&dir), force: false },
                None,
            );
            assert_eq!(out.code, EXIT_OK);
            assert_eq!(out.stdout["status"], "no_active_team");
            assert!(out.stdout.get("error").is_none());
        }
    }

    #[test]
    fn start_conflicts_with_an_active_team() {
        let dir = project(Some(SQUAD), true);
        let out = execute(
            &TeamAction::Start { name: "squad".into(), scope: scope(&dir) },
            None,
        );

        assert_eq!(out.code, EXIT_CONFLICT);
        assert_eq!(out.stdout["error"], "team_already_active");
        assert_eq!(out.stdout["active"], "team-20260101-000000");
    }

    #[test]
    fn start_with_an_unknown_config_is_a_plain_error() {
        let dir = project(Some(SQUAD), false);
        let out = execute(
            &TeamAction::Start { name: "ghost".into(), scope: scope(&dir) },
            None,
        );

        assert_eq!(out.code, EXIT_ERROR);
        assert_eq!(out.stdout["error"], "config_not_found");
        assert_eq!(out.stdout["requested"], "ghost");
    }

    #[test]
    fn status_reports_workers_and_mailbox_of_an_active_team() {
        let dir = project(Some(SQUAD), true);
        let mailbox = crate::team::Mailbox::open(&dir.path().join(".harness").join("mailbox"))
            .unwrap();
        mailbox
            .send(crate::team::MailboxMessage {
                from: "lead".into(),
                to: "analyst".into(),
                msg_type: "task".into(),
                body: "BTC 1h".into(),
                timestamp: 1735689601000,
            })
            .unwrap();

        let out = execute(&TeamAction::Status { scope: scope(&dir) }, None);

        assert_eq!(out.code, EXIT_OK);
        assert_eq!(out.stdout["status"], "running");
        assert_eq!(out.stdout["team_id"], "team-20260101-000000");
        assert_eq!(out.stdout["name"], "squad");
        assert_eq!(out.stdout["started_at"], "2025-01-01T00:00:00.000Z");

        let worker = &out.stdout["workers"][0];
        assert_eq!(worker["name"], "analyst");
        assert_eq!(worker["agent"], "yolo-analyst");
        assert_eq!(worker["pane_id"], "%1");
        assert_eq!(worker["branch"], "team/analyst");
        assert_eq!(worker["status"], "starting");
        assert!(worker["worktree_path"].is_null());
        assert!(worker["last_message_ts"].is_null());

        let unread = &out.stdout["mailbox"]["unread_by_recipient"];
        assert_eq!(unread["analyst"], 1);
        assert_eq!(unread["lead"], 0);
    }

    #[test]
    fn list_reports_configs_and_the_active_team() {
        let dir = project(Some(SQUAD), true);
        std::fs::write(
            dir.path().join(".harness").join("teams").join("solo.yml"),
            "workers:\n  - name: solo\n    task: everything\n",
        )
        .unwrap();

        let out = execute(&TeamAction::List { scope: scope(&dir) }, None);

        assert_eq!(out.code, EXIT_OK);
        let names: Vec<&str> = out.stdout["teams"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap())
            .collect();
        assert_eq!(names, vec!["solo", "squad"]);
        assert_eq!(out.stdout["teams"][1]["workers"].as_array().unwrap().len(), 2);
        assert_eq!(out.stdout["active"], "team-20260101-000000");
        assert_eq!(out.stdout["active_name"], "squad");
    }

    #[test]
    fn a_missing_project_directory_is_rejected() {
        let out = execute(
            &TeamAction::Status {
                scope: TeamScope { project: Some("/nope/not/here".into()) },
            },
            None,
        );

        assert_eq!(out.code, EXIT_ERROR);
        assert_eq!(out.stdout["error"], "invalid_project");
    }

    #[test]
    fn the_top_level_project_flag_is_the_fallback() {
        let dir = project(Some(SQUAD), true);
        let out = execute(
            &TeamAction::Status { scope: TeamScope::default() },
            Some(&dir.path().display().to_string()),
        );

        assert_eq!(out.code, EXIT_OK);
        assert_eq!(out.stdout["team_id"], "team-20260101-000000");
    }

    #[test]
    fn forced_stop_with_nothing_to_stop_says_so() {
        let dir = project(Some(SQUAD), false);
        let out = execute(&TeamAction::Stop { scope: scope(&dir), force: true }, None);

        assert_eq!(out.code, EXIT_OK);
        assert_eq!(out.stdout["status"], "no_active_team");
        assert!(out.stdout["team_id"].is_null());
        assert_eq!(out.stdout["killed_sessions"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn forced_stop_clears_a_stale_state_file() {
        // The reboot case: `team.json` says Running, tmux has nothing.
        let dir = project(Some(SQUAD), true);
        let state_file = dir.path().join(".harness").join("team.json");

        let out = execute(&TeamAction::Stop { scope: scope(&dir), force: true }, None);

        assert_eq!(out.code, EXIT_OK);
        assert_eq!(out.stdout["status"], "stopped");
        assert_eq!(out.stdout["forced"], true);
        assert_eq!(out.stdout["team_id"], "team-20260101-000000");
        assert!(!state_file.exists());
    }
}
