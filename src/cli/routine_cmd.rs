//! Headless routine control — `momo-fetch routine …`.
//!
//! Same contract as `team`: exactly one JSON document on stdout, narration on
//! stderr, and the outcome in the exit code — so the agent in a session can
//! create, inspect and fire routines through `shell_exec` without a REPL.
//!
//! ```text
//! 0  success
//! 1  error (unknown routine, invalid schedule, dispatch failed)
//! 2  state conflict (a run of this routine is already going)
//! ```
//!
//! `routine tick` is the scheduler itself, factored out as a command. The
//! gateway calls the same [`RoutineService::tick`] on a timer; running it from
//! system cron or by hand does exactly what the gateway's loop does, which is
//! what makes the schedule debuggable.

use std::path::PathBuf;

use serde_json::{Value, json};

use crate::routine::{
    Assignee, CatchUp, Concurrency, Priority, Routine, RoutineService, TaskTemplate, Trigger,
    parse_duration, view,
};

pub const EXIT_OK: i32 = 0;
pub const EXIT_ERROR: i32 = 1;
pub const EXIT_CONFLICT: i32 = 2;

/// Repeated on every action so `--project` can follow it, the way
/// `momo-fetch routine list --project /repo` reads.
#[derive(clap::Args, Debug, Clone, Default)]
pub struct RoutineScope {
    /// Project directory holding `.harness/` (default: current directory)
    #[arg(long = "project", value_name = "DIR")]
    pub project: Option<String>,
}

/// The fields of a routine, as flags.
///
/// Shared by `add` and `update`: on `add` the missing pieces are defaults and
/// the required ones are checked, on `update` every unset flag means "leave it
/// alone". One struct so the two can never drift apart.
#[derive(clap::Args, Debug, Clone, Default)]
pub struct RoutineSpec {
    /// What this routine is called
    #[arg(long)]
    pub name: Option<String>,

    /// Who does the work: `lead`, `agent:<name>` or `worker:<name>`
    #[arg(long)]
    pub assignee: Option<String>,

    /// Five-field cron expression, e.g. "0 3 * * *"
    #[arg(long, value_name = "EXPR")]
    pub cron: Option<String>,

    /// IANA timezone the cron expression is read in (default: UTC)
    #[arg(long, value_name = "TZ")]
    pub timezone: Option<String>,

    /// What a missed cron window means: `skip` or `run_once`
    #[arg(long = "catch-up", value_name = "MODE")]
    pub catch_up: Option<String>,

    /// Fire on a fixed interval instead — the heartbeat shape. e.g. 5m, 2h
    #[arg(long, value_name = "INTERVAL", conflicts_with = "cron")]
    pub every: Option<String>,

    /// Never fire on a schedule; only `routine run` starts it
    #[arg(long, conflicts_with_all = ["cron", "every"])]
    pub manual: bool,

    /// What happens when it comes due mid-run: `queue`, `skip` or `parallel`
    #[arg(long)]
    pub concurrency: Option<String>,

    /// Title of the task each firing creates
    #[arg(long)]
    pub title: Option<String>,

    /// Task priority: low, medium, high, urgent
    #[arg(long)]
    pub priority: Option<String>,

    /// What the assignee should do each time this fires — the prompt
    #[arg(long)]
    pub description: Option<String>,

    /// Permission mode for the spawned run (default: auto)
    #[arg(long)]
    pub permission: Option<String>,

    /// Create it switched on (the default)
    #[arg(long, conflicts_with = "disabled")]
    pub enabled: bool,

    /// Create it switched off
    #[arg(long)]
    pub disabled: bool,

    /// Read the routine from a JSON file, or `-` for stdin. Flags win over it.
    #[arg(long, value_name = "PATH")]
    pub json: Option<String>,
}

#[derive(clap::Subcommand, Debug)]
pub enum RoutineAction {
    /// List every routine, with when each is next due
    List {
        #[command(flatten)]
        scope: RoutineScope,
    },

    /// Show one routine and its recent runs
    Show {
        /// Routine id or exact name
        routine: String,
        #[command(flatten)]
        scope: RoutineScope,
    },

    /// Create a routine
    ///
    /// Needs a name, a task title, a description, and one trigger
    /// (`--cron`, `--every` or `--manual`).
    Add {
        #[command(flatten)]
        spec: RoutineSpec,
        #[command(flatten)]
        scope: RoutineScope,
    },

    /// Change a routine. Unset flags are left alone.
    ///
    /// Changing the trigger re-anchors the schedule on now, so a new
    /// expression is never fired from a window it never had.
    Update {
        /// Routine id or exact name
        routine: String,
        #[command(flatten)]
        spec: RoutineSpec,
        #[command(flatten)]
        scope: RoutineScope,
    },

    /// Delete a routine. Its run history is kept.
    Rm {
        /// Routine id or exact name
        routine: String,
        #[command(flatten)]
        scope: RoutineScope,
    },

    /// Switch a routine on. Re-enabling anchors on now.
    Enable {
        /// Routine id or exact name
        routine: String,
        #[command(flatten)]
        scope: RoutineScope,
    },

    /// Switch a routine off without deleting it
    Disable {
        /// Routine id or exact name
        routine: String,
        #[command(flatten)]
        scope: RoutineScope,
    },

    /// Fire a routine now, outside its schedule
    ///
    /// Exits 2 with {"error":"run_in_progress"} when one is already going and
    /// the routine is not set to `parallel`.
    Run {
        /// Routine id or exact name
        routine: String,
        #[command(flatten)]
        scope: RoutineScope,
    },

    /// Advance the scheduler one step: settle finished runs, fire what is due
    ///
    /// The gateway does this on a timer. Run it from system cron, or by hand,
    /// when nothing is holding the gateway open.
    Tick {
        #[command(flatten)]
        scope: RoutineScope,
    },

    /// Recent runs, newest first
    Runs {
        /// Routine id or exact name. Omit for every routine.
        routine: Option<String>,
        /// How many to show (default 20)
        #[arg(long, default_value_t = 20)]
        limit: usize,
        #[command(flatten)]
        scope: RoutineScope,
    },

    /// Who a routine can be assigned to in this project right now
    Assignees {
        #[command(flatten)]
        scope: RoutineScope,
    },
}

impl RoutineAction {
    fn scope(&self) -> &RoutineScope {
        match self {
            Self::List { scope }
            | Self::Show { scope, .. }
            | Self::Add { scope, .. }
            | Self::Update { scope, .. }
            | Self::Rm { scope, .. }
            | Self::Enable { scope, .. }
            | Self::Disable { scope, .. }
            | Self::Run { scope, .. }
            | Self::Tick { scope }
            | Self::Runs { scope, .. }
            | Self::Assignees { scope } => scope,
        }
    }
}

/// One JSON document, whatever narration belongs on stderr, and the exit code.
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

    fn fail(error: &str, message: String) -> Self {
        Self::error(EXIT_ERROR, error, message, json!({}))
    }

    fn note(mut self, msg: impl Into<String>) -> Self {
        self.stderr.push(msg.into());
        self
    }
}

/// Run a `routine` subcommand, print its result, and return the exit code.
pub fn run(action: &RoutineAction, fallback_project: Option<&str>) -> i32 {
    let outcome = execute(action, fallback_project);

    for line in &outcome.stderr {
        eprintln!("{line}");
    }
    println!(
        "{}",
        serde_json::to_string_pretty(&outcome.stdout).unwrap_or_else(|e| {
            format!("{{\"error\":\"serialize_failed\",\"message\":\"{e}\"}}")
        })
    );

    outcome.code
}

fn execute(action: &RoutineAction, fallback_project: Option<&str>) -> Outcome {
    let project = match resolve_project(action.scope().project.as_deref().or(fallback_project)) {
        Ok(p) => p,
        Err(e) => return Outcome::fail("invalid_project", e.to_string()),
    };

    // `assignees` reads the agent directory and the team, not the routine
    // store — answering it must not depend on routines being set up yet.
    if let RoutineAction::Assignees { .. } = action {
        return Outcome::ok(view::assignees_payload(&project));
    }

    let mut service = match RoutineService::new(&project) {
        Ok(s) => s,
        Err(e) => {
            return Outcome::error(
                EXIT_ERROR,
                "state_error",
                format!("Could not open the routine store in {}: {e}", project.display()),
                json!({ "project_path": project.display().to_string() }),
            );
        }
    };

    match action {
        RoutineAction::List { .. } => list(&service),
        RoutineAction::Show { routine, .. } => show(&service, routine),
        RoutineAction::Add { spec, .. } => add(&mut service, spec),
        RoutineAction::Update { routine, spec, .. } => update(&mut service, routine, spec),
        RoutineAction::Rm { routine, .. } => remove(&mut service, routine),
        RoutineAction::Enable { routine, .. } => set_enabled(&mut service, routine, true),
        RoutineAction::Disable { routine, .. } => set_enabled(&mut service, routine, false),
        RoutineAction::Run { routine, .. } => run_now(&mut service, routine),
        RoutineAction::Tick { .. } => tick(&mut service),
        RoutineAction::Runs { routine, limit, .. } => runs(&service, routine.as_deref(), *limit),
        RoutineAction::Assignees { .. } => unreachable!("handled above"),
    }
}

/// Resolve `--project` to an absolute path — the service joins `.harness` onto
/// it, so a relative path would silently follow the caller's cwd.
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

fn list(service: &RoutineService) -> Outcome {
    let routines: Vec<Value> = service
        .list()
        .into_iter()
        .map(|r| view::routine_payload(service, r))
        .collect();
    let count = routines.len();
    Outcome::ok(json!({ "routines": routines, "count": count })).note(match count {
        0 => "No routines yet. Create one with: momo-fetch routine add --help".to_string(),
        1 => "1 routine.".to_string(),
        n => format!("{n} routines."),
    })
}

fn show(service: &RoutineService, key: &str) -> Outcome {
    let Some(routine) = service.resolve(key) else {
        return not_found(service, key);
    };
    let mut payload = view::routine_payload(service, routine);
    if let Some(obj) = payload.as_object_mut() {
        let runs: Vec<Value> = service
            .runs(Some(&routine.id), 10)
            .into_iter()
            .map(view::run_payload)
            .collect();
        obj.insert("runs".to_string(), json!(runs));
    }
    Outcome::ok(payload)
}

fn add(service: &mut RoutineService, spec: &RoutineSpec) -> Outcome {
    let base = match load_json_base(spec) {
        Ok(b) => b,
        Err(e) => return Outcome::fail("invalid_json", e),
    };
    let routine = match build_routine(spec, base, None) {
        Ok(r) => r,
        Err(e) => return Outcome::fail("invalid_routine", e),
    };

    match service.create(routine) {
        Ok(created) => {
            let payload = view::routine_payload(service, &created);
            let due = service
                .next_due(&created)
                .map(crate::routine::format_timestamp)
                .unwrap_or_else(|| "never (manual)".to_string());
            Outcome::ok(payload).note(format!(
                "Created '{}' ({}), assigned to {}. Next due: {due}.",
                created.name, created.id, created.assignee
            ))
        }
        Err(e) => Outcome::fail("create_failed", e.to_string()),
    }
}

fn update(service: &mut RoutineService, key: &str, spec: &RoutineSpec) -> Outcome {
    let Some(existing) = service.resolve(key).cloned() else {
        return not_found(service, key);
    };
    let base = match load_json_base(spec) {
        Ok(b) => b,
        Err(e) => return Outcome::fail("invalid_json", e),
    };
    let updated = match build_routine(spec, base, Some(existing.clone())) {
        Ok(r) => r,
        Err(e) => return Outcome::fail("invalid_routine", e),
    };

    match service.update(&existing.id, updated) {
        Ok(saved) => {
            let payload = view::routine_payload(service, &saved);
            Outcome::ok(payload).note(format!("Updated '{}' ({}).", saved.name, saved.id))
        }
        Err(e) => Outcome::fail("update_failed", e.to_string()),
    }
}

fn remove(service: &mut RoutineService, key: &str) -> Outcome {
    let Some(routine) = service.resolve(key).cloned() else {
        return not_found(service, key);
    };
    match service.delete(&routine.id) {
        Ok(true) => Outcome::ok(json!({
            "status": "deleted",
            "id": routine.id,
            "name": routine.name,
        }))
        .note(format!("Deleted '{}'. Its run history is kept.", routine.name)),
        Ok(false) => not_found(service, key),
        Err(e) => Outcome::fail("delete_failed", e.to_string()),
    }
}

fn set_enabled(service: &mut RoutineService, key: &str, enabled: bool) -> Outcome {
    let Some(routine) = service.resolve(key).cloned() else {
        return not_found(service, key);
    };
    match service.set_enabled(&routine.id, enabled) {
        Ok(saved) => {
            let payload = view::routine_payload(service, &saved);
            Outcome::ok(payload).note(if enabled {
                format!(
                    "'{}' is on. Next due: {}.",
                    saved.name,
                    service
                        .next_due(&saved)
                        .map(crate::routine::format_timestamp)
                        .unwrap_or_else(|| "never (manual)".to_string())
                )
            } else {
                format!("'{}' is off. It keeps its definition and history.", saved.name)
            })
        }
        Err(e) => Outcome::fail("update_failed", e.to_string()),
    }
}

fn run_now(service: &mut RoutineService, key: &str) -> Outcome {
    let Some(routine) = service.resolve(key).cloned() else {
        return not_found(service, key);
    };

    // Settle anything that has already finished before calling it a conflict.
    let _ = service.reconcile();
    let active = service.active_runs(&routine.id);
    if active > 0 && routine.concurrency != Concurrency::Parallel {
        return Outcome::error(
            EXIT_CONFLICT,
            "run_in_progress",
            format!(
                "A run of '{}' is still going. Wait for it, or set --concurrency parallel.",
                routine.name
            ),
            json!({ "id": routine.id, "active_runs": active }),
        );
    }

    match service.run_now(&routine.id) {
        Ok(record) => {
            let note = format!(
                "Fired '{}' → {} ({}).",
                routine.name,
                routine.assignee,
                record.status.label()
            );
            Outcome::ok(view::run_payload(&record)).note(note)
        }
        Err(e) => Outcome::fail("run_failed", e.to_string()),
    }
}

fn tick(service: &mut RoutineService) -> Outcome {
    match service.tick(crate::routine::now_millis()) {
        Ok(report) => {
            let note = if report.is_quiet() {
                "Nothing due.".to_string()
            } else {
                format!(
                    "{} fired, {} finished, {} queued, {} skipped.",
                    report.fired.len(),
                    report.finished.len(),
                    report.queued.len(),
                    report.skipped.len()
                )
            };
            Outcome::ok(view::tick_payload(&report)).note(note)
        }
        Err(e) => Outcome::fail("tick_failed", e.to_string()),
    }
}

fn runs(service: &RoutineService, key: Option<&str>, limit: usize) -> Outcome {
    let id = match key {
        Some(k) => match service.resolve(k) {
            Some(r) => Some(r.id.clone()),
            None => return not_found(service, k),
        },
        None => None,
    };
    let records: Vec<Value> = service
        .runs(id.as_deref(), limit)
        .into_iter()
        .map(view::run_payload)
        .collect();
    let count = records.len();
    Outcome::ok(json!({ "runs": records, "count": count }))
}

fn not_found(service: &RoutineService, key: &str) -> Outcome {
    let known: Vec<String> = service
        .list()
        .into_iter()
        .map(|r| format!("{} ({})", r.name, r.id))
        .collect();
    Outcome::error(
        EXIT_ERROR,
        "routine_not_found",
        format!("No routine '{key}'."),
        json!({ "requested": key, "known": known }),
    )
}

// ─── Building a routine from flags ─────────────────────────────────

/// `--json` gives a starting point; the flags are applied on top of it.
fn load_json_base(spec: &RoutineSpec) -> Result<Option<Routine>, String> {
    let Some(source) = &spec.json else {
        return Ok(None);
    };
    let text = if source == "-" {
        use std::io::Read;
        let mut buf = String::new();
        std::io::stdin()
            .read_to_string(&mut buf)
            .map_err(|e| format!("Could not read the routine from stdin: {e}"))?;
        buf
    } else {
        std::fs::read_to_string(source)
            .map_err(|e| format!("Could not read '{source}': {e}"))?
    };

    serde_json::from_str::<Routine>(&text)
        .map(Some)
        .map_err(|e| format!("That JSON is not a routine: {e}"))
}

/// Fold flags over a starting point.
///
/// `existing` is the routine being updated, if any. With neither it, nor
/// `--json`, every required field has to come from the flags — which is what
/// makes `add` refuse a half-specified routine instead of writing a routine
/// that can never fire.
fn build_routine(
    spec: &RoutineSpec,
    from_json: Option<Routine>,
    existing: Option<Routine>,
) -> Result<Routine, String> {
    let base = from_json.or_else(|| existing.clone());
    let is_update = existing.is_some();

    let name = spec
        .name
        .clone()
        .or_else(|| base.as_ref().map(|b| b.name.clone()))
        .ok_or("A routine needs --name.")?;

    let assignee = match &spec.assignee {
        Some(a) => a.parse::<Assignee>()?,
        None => base
            .as_ref()
            .map(|b| b.assignee.clone())
            .unwrap_or(Assignee::Lead),
    };

    let trigger = build_trigger(spec, base.as_ref().map(|b| &b.trigger), is_update)?;

    let concurrency = match &spec.concurrency {
        Some(c) => parse_concurrency(c)?,
        None => base.as_ref().map(|b| b.concurrency).unwrap_or_default(),
    };

    let title = spec
        .title
        .clone()
        .or_else(|| base.as_ref().map(|b| b.task.title.clone()))
        .ok_or("A routine needs --title: it is what each run is called.")?;

    let description = spec
        .description
        .clone()
        .or_else(|| base.as_ref().map(|b| b.task.description.clone()))
        .ok_or("A routine needs --description: it is the prompt the assignee receives.")?;

    let priority = match &spec.priority {
        Some(p) => p.parse::<Priority>()?,
        None => base.as_ref().map(|b| b.task.priority).unwrap_or_default(),
    };

    let permission = spec
        .permission
        .clone()
        .or_else(|| base.as_ref().map(|b| b.permission.clone()))
        .unwrap_or_else(|| crate::routine::DEFAULT_ROUTINE_PERMISSION.to_string());

    let enabled = if spec.disabled {
        false
    } else if spec.enabled {
        true
    } else {
        base.as_ref().map(|b| b.enabled).unwrap_or(true)
    };

    let routine = Routine {
        id: base.as_ref().map(|b| b.id.clone()).unwrap_or_default(),
        name,
        enabled,
        assignee,
        trigger,
        concurrency,
        task: TaskTemplate {
            title,
            priority,
            description,
        },
        permission,
        created_at: base.as_ref().map(|b| b.created_at).unwrap_or(0),
        updated_at: 0,
    };
    routine.validate()?;
    Ok(routine)
}

/// Work out the trigger from the flags, keeping whatever the routine already
/// had when the flags say nothing about it.
///
/// `--timezone` and `--catch-up` on their own edit an existing cron trigger
/// rather than being ignored — moving a nightly job from UTC to Asia/Bangkok
/// should not require retyping its expression.
fn build_trigger(
    spec: &RoutineSpec,
    existing: Option<&Trigger>,
    is_update: bool,
) -> Result<Trigger, String> {
    if spec.manual {
        return Ok(Trigger::Manual);
    }

    if let Some(every) = &spec.every {
        let seconds = parse_duration(every)?;
        let trigger = Trigger::Every { seconds };
        trigger.validate()?;
        return Ok(trigger);
    }

    let existing_cron = match existing {
        Some(Trigger::Cron {
            expression,
            timezone,
            catch_up,
        }) => Some((expression.clone(), timezone.clone(), *catch_up)),
        _ => None,
    };

    if spec.cron.is_some() || spec.timezone.is_some() || spec.catch_up.is_some() {
        let expression = spec
            .cron
            .clone()
            .or_else(|| existing_cron.as_ref().map(|(e, _, _)| e.clone()))
            .ok_or("--timezone and --catch-up need a cron trigger — give --cron too.")?;
        let timezone = spec
            .timezone
            .clone()
            .or_else(|| existing_cron.as_ref().map(|(_, t, _)| t.clone()))
            .unwrap_or_else(|| "UTC".to_string());
        let catch_up = match &spec.catch_up {
            Some(c) => parse_catch_up(c)?,
            None => existing_cron.map(|(_, _, c)| c).unwrap_or_default(),
        };
        let trigger = Trigger::Cron {
            expression,
            timezone,
            catch_up,
        };
        trigger.validate()?;
        return Ok(trigger);
    }

    match existing {
        Some(t) => Ok(t.clone()),
        None if is_update => Err("This routine has no trigger — give --cron, --every or --manual.".to_string()),
        None => Err("A routine needs a trigger: --cron \"0 3 * * *\", --every 5m, or --manual.".to_string()),
    }
}

fn parse_concurrency(raw: &str) -> Result<Concurrency, String> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "queue" => Ok(Concurrency::Queue),
        "skip" => Ok(Concurrency::Skip),
        "parallel" => Ok(Concurrency::Parallel),
        other => Err(format!(
            "Unknown concurrency '{other}' — use queue, skip or parallel."
        )),
    }
}

fn parse_catch_up(raw: &str) -> Result<CatchUp, String> {
    match raw.trim().to_ascii_lowercase().replace('-', "_").as_str() {
        "skip" => Ok(CatchUp::Skip),
        "run_once" | "once" => Ok(CatchUp::RunOnce),
        other => Err(format!(
            "Unknown catch-up mode '{other}' — use skip or run_once."
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec() -> RoutineSpec {
        RoutineSpec {
            name: Some("Nightly digest".into()),
            title: Some("Summarise the day".into()),
            description: Some("Read the transcripts and write it up.".into()),
            cron: Some("0 3 * * *".into()),
            ..Default::default()
        }
    }

    #[test]
    fn flags_alone_build_a_valid_routine() {
        let r = build_routine(&spec(), None, None).unwrap();
        assert_eq!(r.name, "Nightly digest");
        assert_eq!(r.assignee, Assignee::Lead);
        assert_eq!(r.concurrency, Concurrency::Queue);
        assert_eq!(r.permission, crate::routine::DEFAULT_ROUTINE_PERMISSION);
        assert!(r.enabled);
        assert!(matches!(r.trigger, Trigger::Cron { .. }));
    }

    #[test]
    fn an_unattended_run_never_defaults_to_yolo() {
        let r = build_routine(&spec(), None, None).unwrap();
        assert_eq!(r.permission, "auto");
    }

    #[test]
    fn a_routine_without_a_trigger_is_refused() {
        let mut s = spec();
        s.cron = None;
        let err = build_routine(&s, None, None).unwrap_err();
        assert!(err.contains("--cron"), "{err}");
    }

    #[test]
    fn a_routine_without_a_description_is_refused() {
        let mut s = spec();
        s.description = None;
        let err = build_routine(&s, None, None).unwrap_err();
        assert!(err.contains("--description"), "{err}");
    }

    #[test]
    fn interval_triggers_accept_human_units() {
        let mut s = spec();
        s.cron = None;
        s.every = Some("5m".into());
        let r = build_routine(&s, None, None).unwrap();
        assert_eq!(r.trigger, Trigger::Every { seconds: 300 });
    }

    #[test]
    fn a_too_fast_interval_is_refused_at_the_flag() {
        let mut s = spec();
        s.cron = None;
        s.every = Some("5s".into());
        assert!(build_routine(&s, None, None).is_err());
    }

    #[test]
    fn update_keeps_what_the_flags_do_not_mention() {
        let existing = build_routine(&spec(), None, None).unwrap();
        let only_title = RoutineSpec {
            title: Some("Summarise the week".into()),
            ..Default::default()
        };
        let updated = build_routine(&only_title, None, Some(existing.clone())).unwrap();
        assert_eq!(updated.task.title, "Summarise the week");
        assert_eq!(updated.trigger, existing.trigger);
        assert_eq!(updated.assignee, existing.assignee);
        assert_eq!(updated.task.description, existing.task.description);
    }

    #[test]
    fn timezone_alone_edits_an_existing_cron_trigger() {
        let existing = build_routine(&spec(), None, None).unwrap();
        let tz_only = RoutineSpec {
            timezone: Some("Asia/Bangkok".into()),
            ..Default::default()
        };
        let updated = build_routine(&tz_only, None, Some(existing)).unwrap();
        match updated.trigger {
            Trigger::Cron {
                expression,
                timezone,
                ..
            } => {
                assert_eq!(expression, "0 3 * * *");
                assert_eq!(timezone, "Asia/Bangkok");
            }
            other => panic!("expected a cron trigger, got {other:?}"),
        }
    }

    #[test]
    fn timezone_alone_on_a_new_routine_says_what_is_missing() {
        let tz_only = RoutineSpec {
            name: Some("x".into()),
            title: Some("t".into()),
            description: Some("d".into()),
            timezone: Some("UTC".into()),
            ..Default::default()
        };
        let err = build_routine(&tz_only, None, None).unwrap_err();
        assert!(err.contains("--cron"), "{err}");
    }

    #[test]
    fn assignees_parse_from_the_flag() {
        let mut s = spec();
        s.assignee = Some("worker:builder".into());
        let r = build_routine(&s, None, None).unwrap();
        assert_eq!(r.assignee, Assignee::Worker { name: "builder".into() });

        let mut s = spec();
        s.assignee = Some("nobody".into());
        assert!(build_routine(&s, None, None).is_err());
    }

    #[test]
    fn disabled_wins_over_the_default() {
        let mut s = spec();
        s.disabled = true;
        assert!(!build_routine(&s, None, None).unwrap().enabled);
    }

    #[test]
    fn manual_beats_a_stored_cron_trigger() {
        let existing = build_routine(&spec(), None, None).unwrap();
        let s = RoutineSpec { manual: true, ..Default::default() };
        assert_eq!(
            build_routine(&s, None, Some(existing)).unwrap().trigger,
            Trigger::Manual
        );
    }

    #[test]
    fn json_supplies_the_base_and_flags_win_over_it() {
        let json: Routine = serde_json::from_str(
            r#"{
                "id": "",
                "name": "From file",
                "assignee": { "kind": "agent", "name": "planner" },
                "trigger": { "type": "every", "seconds": 600 },
                "task": { "title": "Sweep", "description": "Look at the backlog." }
            }"#,
        )
        .unwrap();

        let override_name = RoutineSpec {
            name: Some("Renamed".into()),
            ..Default::default()
        };
        let r = build_routine(&override_name, Some(json), None).unwrap();
        assert_eq!(r.name, "Renamed");
        assert_eq!(r.assignee, Assignee::Agent { name: "planner".into() });
        assert_eq!(r.trigger, Trigger::Every { seconds: 600 });
    }

    #[test]
    fn catch_up_accepts_both_spellings() {
        assert_eq!(parse_catch_up("run-once").unwrap(), CatchUp::RunOnce);
        assert_eq!(parse_catch_up("run_once").unwrap(), CatchUp::RunOnce);
        assert_eq!(parse_catch_up("SKIP").unwrap(), CatchUp::Skip);
        assert!(parse_catch_up("later").is_err());
    }

    #[test]
    fn concurrency_names_the_valid_options_when_wrong() {
        let err = parse_concurrency("whatever").unwrap_err();
        assert!(err.contains("queue"), "{err}");
    }
}
