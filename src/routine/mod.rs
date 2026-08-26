//! Routines — recurring work the lead, a specialist agent, or a standby worker
//! picks up on a schedule.
//!
//! A team answers "who is working on this right now". A routine answers "what
//! should happen again, without anyone remembering to ask". The nightly digest,
//! the hourly team health check, the Monday-morning backlog sweep: each is a
//! task template plus a trigger plus somebody to hand it to.
//!
//! # How a routine actually runs
//!
//! Dispatch has two shapes, and the difference is not cosmetic:
//!
//! - **A standby team worker** gets a mailbox message. The worker is already
//!   up and polling, so delivery is the whole job — the run is recorded as
//!   `delivered` and the worker owns the outcome from there. Claiming anything
//!   more would be a lie: nothing here can see whether the worker succeeded.
//! - **The lead or a named agent** gets a fresh headless `momo-fetch -p`
//!   process, spawned detached, with its own log and exit-code file. It runs
//!   *beside* whatever session the user is in rather than inside it.
//!
//! That second choice is the load-bearing one. The harness holds a single
//! session, a single runner and a single provider, and the gateway admits one
//! turn at a time (see `gateway::turn`); a routine that fired into that shared
//! object would interleave with whatever the user was typing. A separate
//! process has none of that coupling, and it gives the same exit-code ground
//! truth team workers already use.
//!
//! # Scheduling
//!
//! [`decide`] is pure: routine + runtime + clock + how many runs are already in
//! flight, in; a [`Decision`] and the next runtime, out. Every scheduling rule
//! this module has — catch-up, concurrency, queue depth — lives there and is
//! tested without a filesystem, a process or a tmux server.

pub mod cron;
pub mod view;
mod exec;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub use exec::{momo_binary, process_alive, read_exit_code};

/// Permission mode a routine's process runs under when the routine does not
/// say. `auto` for the same reason team workers default to it: a detached,
/// unattended process in `strict` stops at its first mutating tool and waits
/// for a confirmation nobody will ever type. Never `yolo` — an unattended
/// process is exactly the wrong place to remove the destructive-command guard.
pub const DEFAULT_ROUTINE_PERMISSION: &str = "auto";

/// How late a cron window may be and still count as "now" under
/// `CatchUp::Skip`.
///
/// The scheduler ticks in tens of seconds, so a window is routinely a few
/// seconds stale by the time anyone looks at it. Two minutes is comfortably
/// past any tick interval and comfortably short of "the laptop was shut".
pub const CATCH_UP_GRACE_SECS: i64 = 120;

/// Interval triggers below this are refused. A heartbeat that fires faster than
/// its own task can finish is a fork bomb with a cron face.
pub const MIN_INTERVAL_SECS: u64 = 30;

/// Windows a `queue` routine will hold before it starts dropping the oldest.
/// Unbounded, a routine whose runs never finish becomes an unbounded backlog
/// that all fires at once the moment it clears.
pub const MAX_QUEUE_DEPTH: usize = 20;

/// Run records kept on disk, newest last.
pub const MAX_RUN_HISTORY: usize = 200;

/// Missed windows walked through in one catch-up pass before the scheduler
/// gives up and resets the anchor to now.
const MAX_CATCH_UP_STEPS: usize = 5000;

// ─── Definitions ───────────────────────────────────────────────────

/// Who does the work when a routine fires.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Assignee {
    /// The project's default agent, in its own headless process.
    Lead,
    /// A specialist from `.harness/agents/<name>.md`, headless.
    Agent { name: String },
    /// A standby worker in the active team, reached through its mailbox.
    Worker { name: String },
}

impl Assignee {
    /// The `-a` argument for this assignee, if any.
    pub fn agent_name(&self) -> Option<&str> {
        match self {
            Self::Agent { name } => Some(name),
            _ => None,
        }
    }
}

impl std::fmt::Display for Assignee {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Lead => write!(f, "lead"),
            Self::Agent { name } => write!(f, "agent:{name}"),
            Self::Worker { name } => write!(f, "worker:{name}"),
        }
    }
}

impl std::str::FromStr for Assignee {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.trim();
        if s.eq_ignore_ascii_case("lead") {
            return Ok(Self::Lead);
        }
        match s.split_once(':') {
            Some((kind, name)) if !name.trim().is_empty() => {
                let name = name.trim().to_string();
                match kind.to_ascii_lowercase().as_str() {
                    "agent" => Ok(Self::Agent { name }),
                    "worker" => Ok(Self::Worker { name }),
                    other => Err(format!(
                        "Unknown assignee kind '{other}' — use lead, agent:<name> or worker:<name>."
                    )),
                }
            }
            _ => Err(format!(
                "Could not read assignee '{s}' — use lead, agent:<name> or worker:<name>."
            )),
        }
    }
}

/// What a missed cron window means.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CatchUp {
    /// The window passed; let it go and wait for the next one. The right
    /// default for anything whose value is tied to *when* it runs.
    #[default]
    Skip,
    /// Run once, late, then resume the schedule. For work that still needs
    /// doing whenever the machine comes back.
    RunOnce,
}

/// What to do when a routine comes due while its previous run is still going.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Concurrency {
    /// Hold the window and fire it when the lane clears.
    #[default]
    Queue,
    /// Drop the window.
    Skip,
    /// Fire anyway, alongside the run already going.
    Parallel,
}

impl std::fmt::Display for Concurrency {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Queue => "queue",
            Self::Skip => "skip",
            Self::Parallel => "parallel",
        })
    }
}

/// How urgent the created task is. Carried into the prompt, not enforced —
/// nothing here pre-empts a running agent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Priority {
    Low,
    #[default]
    Medium,
    High,
    Urgent,
}

impl std::fmt::Display for Priority {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::Urgent => "urgent",
        };
        f.write_str(s)
    }
}

impl std::str::FromStr for Priority {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "low" => Ok(Self::Low),
            "medium" | "normal" => Ok(Self::Medium),
            "high" => Ok(Self::High),
            "urgent" | "critical" => Ok(Self::Urgent),
            other => Err(format!(
                "Unknown priority '{other}' — use low, medium, high or urgent."
            )),
        }
    }
}

/// When a routine fires.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Trigger {
    /// A five-field cron expression in an IANA zone.
    Cron {
        expression: String,
        #[serde(default = "default_timezone")]
        timezone: String,
        #[serde(default)]
        catch_up: CatchUp,
    },
    /// A fixed interval since the last fire — the heartbeat shape.
    ///
    /// No catch-up setting: an interval that elapsed while the machine was down
    /// is due the moment it comes back, which is the only reading a heartbeat
    /// has.
    Every { seconds: u64 },
    /// Never fires on its own; `routine run` and the UI's Run now are the only
    /// way in.
    Manual,
}

fn default_timezone() -> String {
    "UTC".to_string()
}

impl Trigger {
    /// Reject expressions, zones and intervals that cannot schedule anything,
    /// at the point they are written rather than at the tick that needed them.
    pub fn validate(&self) -> Result<(), String> {
        match self {
            Self::Cron {
                expression,
                timezone,
                ..
            } => {
                cron::CronExpr::parse(expression)?;
                cron::parse_timezone(timezone)?;
                Ok(())
            }
            Self::Every { seconds } => {
                if *seconds < MIN_INTERVAL_SECS {
                    Err(format!(
                        "An interval trigger must be at least {MIN_INTERVAL_SECS}s (got {seconds}s)."
                    ))
                } else {
                    Ok(())
                }
            }
            Self::Manual => Ok(()),
        }
    }

    /// A short human description, for `routine list` and the UI row.
    pub fn summary(&self) -> String {
        match self {
            Self::Cron {
                expression,
                timezone,
                ..
            } => format!("cron {expression} ({timezone})"),
            Self::Every { seconds } => format!("every {}", format_duration(*seconds)),
            Self::Manual => "manual".to_string(),
        }
    }

    fn catch_up(&self) -> CatchUp {
        match self {
            Self::Cron { catch_up, .. } => *catch_up,
            // An elapsed interval is always run late rather than dropped.
            Self::Every { .. } => CatchUp::RunOnce,
            Self::Manual => CatchUp::Skip,
        }
    }

    /// The first firing strictly after `after_ms`, in epoch milliseconds.
    pub fn next_after(&self, after_ms: u64) -> Option<u64> {
        match self {
            Self::Cron {
                expression,
                timezone,
                ..
            } => {
                let expr = cron::CronExpr::parse(expression).ok()?;
                let tz = cron::parse_timezone(timezone).ok()?;
                let after = chrono::DateTime::from_timestamp_millis(after_ms as i64)?;
                expr.next_after(after, tz)
                    .map(|dt| dt.timestamp_millis() as u64)
            }
            Self::Every { seconds } => Some(after_ms + seconds * 1000),
            Self::Manual => None,
        }
    }
}

/// What each firing asks for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskTemplate {
    /// One line, the way a ticket title is one line.
    pub title: String,
    #[serde(default)]
    pub priority: Priority,
    /// What the assignee should actually do. This is the prompt.
    #[serde(default)]
    pub description: String,
}

/// A saved routine.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Routine {
    pub id: String,
    pub name: String,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    pub assignee: Assignee,
    pub trigger: Trigger,
    #[serde(default)]
    pub concurrency: Concurrency,
    pub task: TaskTemplate,
    /// Permission mode for the spawned process. Ignored for worker assignees —
    /// a standby worker already runs under the mode its team gave it.
    #[serde(default = "default_permission")]
    pub permission: String,
    #[serde(default)]
    pub created_at: u64,
    #[serde(default)]
    pub updated_at: u64,
}

fn default_enabled() -> bool {
    true
}

fn default_permission() -> String {
    DEFAULT_ROUTINE_PERMISSION.to_string()
}

impl Routine {
    /// Everything that must hold before this is written to disk.
    pub fn validate(&self) -> Result<(), String> {
        if self.name.trim().is_empty() {
            return Err("A routine needs a name.".to_string());
        }
        if self.task.title.trim().is_empty() {
            return Err("A routine needs a task title — it is what each run is called.".to_string());
        }
        if self.task.description.trim().is_empty() {
            return Err(
                "A routine needs a task description — it is the prompt the assignee receives."
                    .to_string(),
            );
        }
        if let Assignee::Agent { name } | Assignee::Worker { name } = &self.assignee {
            if name.trim().is_empty() {
                return Err("The assignee is missing a name.".to_string());
            }
        }
        self.trigger.validate()?;
        match self.permission.as_str() {
            "strict" | "auto" | "yolo" => {}
            other => {
                return Err(format!(
                    "Unknown permission mode '{other}' — use strict, auto or yolo."
                ));
            }
        }
        Ok(())
    }
}

// ─── Runs ──────────────────────────────────────────────────────────

/// Where one firing got to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum RunStatus {
    /// A process is up and has not written its exit code yet.
    Running,
    /// Posted to a standby worker's inbox. The worker owns it now — nothing
    /// here can see what it did with it.
    Delivered,
    Succeeded,
    Failed { reason: String },
    Skipped { reason: String },
}

impl RunStatus {
    pub fn is_active(&self) -> bool {
        matches!(self, Self::Running)
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Delivered => "delivered",
            Self::Succeeded => "succeeded",
            Self::Failed { .. } => "failed",
            Self::Skipped { .. } => "skipped",
        }
    }
}

/// One firing, from dispatch to outcome.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunRecord {
    pub id: String,
    pub routine_id: String,
    pub routine_name: String,
    pub assignee: Assignee,
    /// The window this run belongs to — not the same as `started_at` when the
    /// run was queued or caught up late.
    pub scheduled_at: u64,
    pub started_at: u64,
    #[serde(default)]
    pub finished_at: Option<u64>,
    pub status: RunStatus,
    #[serde(default)]
    pub pid: Option<u32>,
    #[serde(default)]
    pub exit_code: Option<i32>,
    #[serde(default)]
    pub log_path: Option<String>,
    /// `schedule` or `manual` — why this fired.
    #[serde(default)]
    pub source: String,
}

// ─── Scheduler bookkeeping ─────────────────────────────────────────

/// Per-routine scheduler state. Separate from the definition so editing a
/// routine in the UI never rewrites its history, and so a hand-edited routine
/// file cannot corrupt the schedule.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoutineRuntime {
    /// The window of the last fire — the anchor the next one is computed from.
    #[serde(default)]
    pub last_fired_at: Option<u64>,
    /// When the last run actually started (differs from the window when late).
    #[serde(default)]
    pub last_run_at: Option<u64>,
    /// Windows held back by `Concurrency::Queue`, oldest first.
    #[serde(default)]
    pub queue: Vec<u64>,
    #[serde(default)]
    pub fired: u64,
    #[serde(default)]
    pub skipped: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct SchedulerState {
    #[serde(default)]
    routines: BTreeMap<String, RoutineRuntime>,
}

/// What [`decide`] concluded for one routine at one instant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    /// Nothing due.
    Idle,
    /// Fire now, for this window.
    Fire { scheduled: u64 },
    /// Due, but the lane is busy — held for later.
    Queue { scheduled: u64 },
    /// Due, and deliberately dropped.
    Skip { scheduled: u64, reason: String },
}

/// Decide what one routine should do right now.
///
/// Pure on purpose: no clock, no filesystem, no processes. `active` is how many
/// runs of this routine are still in flight. Returns the decision and the
/// runtime it implies — the caller persists the second only if it acted on the
/// first.
pub fn decide(
    routine: &Routine,
    runtime: &RoutineRuntime,
    now_ms: u64,
    active: usize,
) -> (Decision, RoutineRuntime) {
    let mut next = runtime.clone();

    if !routine.enabled {
        return (Decision::Idle, next);
    }

    // A held window comes first: firing a fresh one while an older one waits
    // would reorder the backlog, and the queue exists to preserve order.
    if routine.concurrency == Concurrency::Queue && active == 0 && !next.queue.is_empty() {
        let scheduled = next.queue.remove(0);
        next.fired += 1;
        next.last_run_at = Some(now_ms);
        return (Decision::Fire { scheduled }, next);
    }

    let anchor = runtime
        .last_fired_at
        .unwrap_or_else(|| routine.created_at.max(1));
    let Some(mut due) = routine.trigger.next_after(anchor) else {
        return (Decision::Idle, next);
    };

    if due > now_ms {
        return (Decision::Idle, next);
    }

    // Catch-up: walk the windows that passed while nothing was running.
    if routine.trigger.catch_up() == CatchUp::Skip {
        let grace_ms = (CATCH_UP_GRACE_SECS * 1000) as u64;
        let mut steps = 0;
        while now_ms.saturating_sub(due) > grace_ms {
            next.skipped += 1;
            next.last_fired_at = Some(due);
            steps += 1;
            match routine.trigger.next_after(due) {
                Some(d) if steps < MAX_CATCH_UP_STEPS => due = d,
                // Either the schedule ran out or the gap is absurd. Anchor on
                // now and let the next window be computed from a sane point.
                _ => {
                    next.last_fired_at = Some(now_ms);
                    return (Decision::Idle, next);
                }
            }
            if due > now_ms {
                return (Decision::Idle, next);
            }
        }
    }

    next.last_fired_at = Some(due);

    if active > 0 {
        return match routine.concurrency {
            Concurrency::Skip => {
                next.skipped += 1;
                (
                    Decision::Skip {
                        scheduled: due,
                        reason: "the previous run is still going".to_string(),
                    },
                    next,
                )
            }
            Concurrency::Queue => {
                if next.queue.len() >= MAX_QUEUE_DEPTH {
                    // Drop the oldest, not the newest: a stale window is worth
                    // less than the one that just came due.
                    next.queue.remove(0);
                    next.skipped += 1;
                }
                next.queue.push(due);
                (Decision::Queue { scheduled: due }, next)
            }
            Concurrency::Parallel => {
                next.fired += 1;
                next.last_run_at = Some(now_ms);
                (Decision::Fire { scheduled: due }, next)
            }
        };
    }

    next.fired += 1;
    next.last_run_at = Some(now_ms);
    (Decision::Fire { scheduled: due }, next)
}

/// The prompt one firing hands to its assignee.
///
/// Shaped as a briefing rather than a bare description because the receiver has
/// no conversation to read it in: a headless process starts cold, and a standby
/// worker gets it between two unrelated messages. Naming the routine, the
/// window and the priority is what makes the task legible on arrival.
pub fn build_prompt(routine: &Routine, scheduled_ms: u64) -> String {
    let when = format_timestamp(scheduled_ms);
    format!(
        "[Routine: {name}] {title}\n\
         Priority: {priority}\n\
         Scheduled: {when}\n\
         \n\
         {description}",
        name = routine.name.trim(),
        title = routine.task.title.trim(),
        priority = routine.task.priority,
        description = routine.task.description.trim(),
    )
}

// ─── Service ───────────────────────────────────────────────────────

/// What one [`RoutineService::tick`] did.
#[derive(Debug, Clone, Default)]
pub struct TickReport {
    pub now: u64,
    pub fired: Vec<RunRecord>,
    pub queued: Vec<(String, u64)>,
    pub skipped: Vec<(String, String)>,
    /// Runs that were in flight at the start of this tick and are not any more.
    pub finished: Vec<RunRecord>,
}

impl TickReport {
    pub fn is_quiet(&self) -> bool {
        self.fired.is_empty()
            && self.queued.is_empty()
            && self.skipped.is_empty()
            && self.finished.is_empty()
    }
}

/// Routine definitions, their schedule state, and their run history, rooted at
/// one project's `.harness/routines/`.
pub struct RoutineService {
    project_path: PathBuf,
    dir: PathBuf,
    routines: BTreeMap<String, Routine>,
    state: SchedulerState,
    runs: Vec<RunRecord>,
}

impl RoutineService {
    /// Open (and create, if missing) the routine store for a project.
    pub fn new(project_path: &Path) -> anyhow::Result<Self> {
        let dir = project_path.join(".harness").join("routines");
        std::fs::create_dir_all(&dir)?;

        let mut service = Self {
            project_path: project_path.to_path_buf(),
            dir,
            routines: BTreeMap::new(),
            state: SchedulerState::default(),
            runs: Vec::new(),
        };
        service.reload()?;
        Ok(service)
    }

    /// Re-read every definition, the schedule state and the run log.
    ///
    /// A malformed definition is skipped rather than fatal: one bad file must
    /// not take the whole scheduler — and every other routine — down with it.
    pub fn reload(&mut self) -> anyhow::Result<()> {
        self.routines.clear();
        for entry in std::fs::read_dir(&self.dir)? {
            let path = entry?.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or_default();
            if stem == "state" || stem == "runs" {
                continue;
            }
            let Ok(content) = std::fs::read_to_string(&path) else {
                continue;
            };
            match serde_json::from_str::<Routine>(&content) {
                Ok(routine) => {
                    self.routines.insert(routine.id.clone(), routine);
                }
                Err(e) => {
                    tracing::warn!("Skipping unreadable routine {}: {e}", path.display());
                }
            }
        }

        self.state = read_json(&self.state_path()).unwrap_or_default();
        self.runs = read_json(&self.runs_path()).unwrap_or_default();
        Ok(())
    }

    fn state_path(&self) -> PathBuf {
        self.dir.join("state.json")
    }

    fn runs_path(&self) -> PathBuf {
        self.dir.join("runs.json")
    }

    /// Where a run's script, log and exit file live.
    pub fn run_dir(&self) -> PathBuf {
        self.dir.join("runs")
    }

    fn routine_path(&self, id: &str) -> PathBuf {
        self.dir.join(format!("{id}.json"))
    }

    /// Every routine, ordered by name so lists are stable between calls.
    pub fn list(&self) -> Vec<&Routine> {
        let mut out: Vec<&Routine> = self.routines.values().collect();
        out.sort_by(|a, b| a.name.cmp(&b.name).then(a.id.cmp(&b.id)));
        out
    }

    pub fn get(&self, id: &str) -> Option<&Routine> {
        self.routines.get(id)
    }

    /// Look up by id, falling back to an exact name match.
    ///
    /// Ids are generated, so anyone typing a routine by hand is typing its
    /// name; the CLI would be unusable otherwise.
    pub fn resolve(&self, id_or_name: &str) -> Option<&Routine> {
        if let Some(r) = self.routines.get(id_or_name) {
            return Some(r);
        }
        let mut hits = self
            .routines
            .values()
            .filter(|r| r.name.eq_ignore_ascii_case(id_or_name));
        let first = hits.next()?;
        // Two routines with the same name is a user error, not something to
        // resolve arbitrarily — an ambiguous pick would fire the wrong one.
        if hits.next().is_some() { None } else { Some(first) }
    }

    pub fn runtime(&self, id: &str) -> RoutineRuntime {
        self.state.routines.get(id).cloned().unwrap_or_default()
    }

    /// When this routine is next expected to fire, in epoch milliseconds.
    pub fn next_due(&self, routine: &Routine) -> Option<u64> {
        if !routine.enabled {
            return None;
        }
        let anchor = self
            .runtime(&routine.id)
            .last_fired_at
            .unwrap_or_else(|| routine.created_at.max(1));
        routine.trigger.next_after(anchor)
    }

    /// Create a routine, assigning an id and timestamps.
    pub fn create(&mut self, mut routine: Routine) -> anyhow::Result<Routine> {
        let now = now_millis();
        if routine.id.trim().is_empty() {
            routine.id = new_routine_id();
        }
        if self.routines.contains_key(&routine.id) {
            anyhow::bail!("A routine with id '{}' already exists.", routine.id);
        }
        routine.created_at = now;
        routine.updated_at = now;
        routine.validate().map_err(|e| anyhow::anyhow!(e))?;
        self.write_routine(&routine)?;
        self.routines.insert(routine.id.clone(), routine.clone());
        Ok(routine)
    }

    /// Replace a routine, keeping its id, creation time and schedule state.
    pub fn update(&mut self, id: &str, mut routine: Routine) -> anyhow::Result<Routine> {
        let existing = self
            .routines
            .get(id)
            .ok_or_else(|| anyhow::anyhow!("No routine '{id}'."))?;
        routine.id = existing.id.clone();
        routine.created_at = existing.created_at;
        routine.updated_at = now_millis();
        routine.validate().map_err(|e| anyhow::anyhow!(e))?;

        // A changed trigger invalidates the anchor: keeping the old one would
        // schedule the new expression from a window it never had.
        let trigger_changed = existing.trigger != routine.trigger;
        self.write_routine(&routine)?;
        self.routines.insert(routine.id.clone(), routine.clone());
        if trigger_changed {
            let mut rt = self.runtime(id);
            rt.last_fired_at = Some(now_millis());
            rt.queue.clear();
            self.state.routines.insert(id.to_string(), rt);
            self.write_state()?;
        }
        Ok(routine)
    }

    pub fn set_enabled(&mut self, id: &str, enabled: bool) -> anyhow::Result<Routine> {
        let mut routine = self
            .routines
            .get(id)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("No routine '{id}'."))?;
        routine.enabled = enabled;
        routine.updated_at = now_millis();
        self.write_routine(&routine)?;
        // Re-enabling anchors on now, so a routine that was off for a week does
        // not come back and immediately fire a week-old window.
        if enabled {
            let mut rt = self.runtime(id);
            rt.last_fired_at = Some(now_millis());
            self.state.routines.insert(id.to_string(), rt);
            self.write_state()?;
        }
        self.routines.insert(routine.id.clone(), routine.clone());
        Ok(routine)
    }

    /// Delete a routine and its schedule state. Run history is kept — it is a
    /// record of what happened, and what happened did not stop being true.
    pub fn delete(&mut self, id: &str) -> anyhow::Result<bool> {
        if self.routines.remove(id).is_none() {
            return Ok(false);
        }
        let _ = std::fs::remove_file(self.routine_path(id));
        self.state.routines.remove(id);
        self.write_state()?;
        Ok(true)
    }

    /// Run history, newest first.
    pub fn runs(&self, routine_id: Option<&str>, limit: usize) -> Vec<&RunRecord> {
        self.runs
            .iter()
            .rev()
            .filter(|r| routine_id.is_none_or(|id| r.routine_id == id))
            .take(limit)
            .collect()
    }

    /// How many runs of this routine are still in flight.
    pub fn active_runs(&self, routine_id: &str) -> usize {
        self.runs
            .iter()
            .filter(|r| r.routine_id == routine_id && r.status.is_active())
            .count()
    }

    /// Settle every in-flight run against the filesystem.
    ///
    /// The exit-code file is ground truth, exactly as it is for team workers:
    /// a process that vanished without writing one died, and a pid that is gone
    /// with no exit file is the only way to tell that apart from "still going".
    pub fn reconcile(&mut self) -> anyhow::Result<Vec<RunRecord>> {
        let run_dir = self.run_dir();
        let mut finished = Vec::new();
        let now = now_millis();

        for record in self.runs.iter_mut().filter(|r| r.status.is_active()) {
            let exit_path = exec::exit_path(&run_dir, &record.id);
            if let Some(code) = read_exit_code(&exit_path) {
                record.exit_code = Some(code);
                record.finished_at = Some(now);
                record.status = if code == 0 {
                    RunStatus::Succeeded
                } else {
                    RunStatus::Failed {
                        reason: format!("exited {code}"),
                    }
                };
                finished.push(record.clone());
                continue;
            }
            if let Some(pid) = record.pid {
                if !process_alive(pid) {
                    record.finished_at = Some(now);
                    record.status = RunStatus::Failed {
                        reason: "the process ended without writing an exit code".to_string(),
                    };
                    finished.push(record.clone());
                }
            }
        }

        if !finished.is_empty() {
            self.write_runs()?;
        }
        Ok(finished)
    }

    /// Advance every routine by one scheduler step.
    pub fn tick(&mut self, now_ms: u64) -> anyhow::Result<TickReport> {
        let mut report = TickReport {
            now: now_ms,
            ..Default::default()
        };
        report.finished = self.reconcile()?;

        let ids: Vec<String> = self.routines.keys().cloned().collect();
        let mut state_dirty = false;

        for id in ids {
            let Some(routine) = self.routines.get(&id).cloned() else {
                continue;
            };
            let runtime = self.runtime(&id);
            let active = self.active_runs(&id);
            let (decision, next_runtime) = decide(&routine, &runtime, now_ms, active);

            if next_runtime != runtime {
                self.state.routines.insert(id.clone(), next_runtime);
                state_dirty = true;
            }

            match decision {
                Decision::Idle => {}
                Decision::Queue { scheduled } => report.queued.push((id.clone(), scheduled)),
                Decision::Skip { scheduled, reason } => {
                    let record = self.record_skip(&routine, scheduled, &reason)?;
                    report.skipped.push((id.clone(), reason));
                    let _ = record;
                }
                Decision::Fire { scheduled } => {
                    let record = self.dispatch(&routine, scheduled, "schedule")?;
                    report.fired.push(record);
                }
            }
        }

        if state_dirty {
            self.write_state()?;
        }
        Ok(report)
    }

    /// Fire a routine now, outside its schedule.
    ///
    /// Returns `Err` when the routine's own concurrency rule says no, so the
    /// caller can report a conflict rather than quietly doing nothing.
    pub fn run_now(&mut self, id: &str) -> anyhow::Result<RunRecord> {
        let routine = self
            .routines
            .get(id)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("No routine '{id}'."))?;
        self.reconcile()?;

        if self.active_runs(id) > 0 && routine.concurrency != Concurrency::Parallel {
            anyhow::bail!(
                "A run of '{}' is still going. Wait for it, or set concurrency to parallel.",
                routine.name
            );
        }

        let now = now_millis();
        let record = self.dispatch(&routine, now, "manual")?;

        let mut rt = self.runtime(id);
        rt.fired += 1;
        rt.last_run_at = Some(now);
        self.state.routines.insert(id.to_string(), rt);
        self.write_state()?;

        Ok(record)
    }

    /// Hand one firing to its assignee.
    fn dispatch(
        &mut self,
        routine: &Routine,
        scheduled: u64,
        source: &str,
    ) -> anyhow::Result<RunRecord> {
        let run_id = new_run_id();
        let prompt = build_prompt(routine, scheduled);
        let now = now_millis();

        let mut record = RunRecord {
            id: run_id.clone(),
            routine_id: routine.id.clone(),
            routine_name: routine.name.clone(),
            assignee: routine.assignee.clone(),
            scheduled_at: scheduled,
            started_at: now,
            finished_at: None,
            status: RunStatus::Running,
            pid: None,
            exit_code: None,
            log_path: None,
            source: source.to_string(),
        };

        match &routine.assignee {
            Assignee::Worker { name } => {
                // Delivery is the whole transaction here, so it finishes now.
                record.finished_at = Some(now);
                match self.post_to_worker(name, &prompt) {
                    Ok(()) => record.status = RunStatus::Delivered,
                    Err(e) => {
                        record.status = RunStatus::Failed {
                            reason: e.to_string(),
                        }
                    }
                }
            }
            assignee => {
                let run_dir = self.run_dir();
                let spawn = exec::spawn(exec::SpawnRequest {
                    project_path: &self.project_path,
                    binary: &momo_binary(),
                    agent: assignee.agent_name(),
                    routine_name: &routine.name,
                    permission: &routine.permission,
                    prompt: &prompt,
                    run_dir: &run_dir,
                    run_id: &run_id,
                });
                match spawn {
                    Ok(spawned) => {
                        record.pid = Some(spawned.pid);
                        record.log_path = Some(spawned.log_path.display().to_string());
                    }
                    Err(e) => {
                        record.finished_at = Some(now);
                        record.status = RunStatus::Failed {
                            reason: e.to_string(),
                        };
                    }
                }
            }
        }

        self.push_run(record.clone())?;
        Ok(record)
    }

    /// Post a task into a standby worker's inbox.
    ///
    /// Goes through `TeamService` rather than writing the file itself: that is
    /// where "is this worker standby, and does it exist" is decided, and two
    /// answers to that question would eventually disagree.
    fn post_to_worker(&self, worker: &str, prompt: &str) -> anyhow::Result<()> {
        let mut team = crate::team::TeamService::new(&self.project_path)?;
        if team.state().is_none() {
            anyhow::bail!("No team is running, so worker '{worker}' has no inbox to write to.");
        }
        // Refresh liveness first — queueing work for a crashed worker is worth
        // an error, not a message nobody will read.
        team.status();
        team.send_to_worker(worker, "task", prompt)
    }

    fn record_skip(
        &mut self,
        routine: &Routine,
        scheduled: u64,
        reason: &str,
    ) -> anyhow::Result<RunRecord> {
        let now = now_millis();
        let record = RunRecord {
            id: new_run_id(),
            routine_id: routine.id.clone(),
            routine_name: routine.name.clone(),
            assignee: routine.assignee.clone(),
            scheduled_at: scheduled,
            started_at: now,
            finished_at: Some(now),
            status: RunStatus::Skipped {
                reason: reason.to_string(),
            },
            pid: None,
            exit_code: None,
            log_path: None,
            source: "schedule".to_string(),
        };
        self.push_run(record.clone())?;
        Ok(record)
    }

    fn push_run(&mut self, record: RunRecord) -> anyhow::Result<()> {
        self.runs.push(record);
        if self.runs.len() > MAX_RUN_HISTORY {
            let excess = self.runs.len() - MAX_RUN_HISTORY;
            self.runs.drain(0..excess);
        }
        self.write_runs()
    }

    fn write_routine(&self, routine: &Routine) -> anyhow::Result<()> {
        write_json(&self.routine_path(&routine.id), routine)
    }

    fn write_state(&self) -> anyhow::Result<()> {
        write_json(&self.state_path(), &self.state)
    }

    fn write_runs(&self) -> anyhow::Result<()> {
        write_json(&self.runs_path(), &self.runs)
    }
}

// ─── Helpers ───────────────────────────────────────────────────────

fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Option<T> {
    let content = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&content).ok()
}

/// Write via a temp file and rename.
///
/// The scheduler and the UI read these while a tick is writing them; a partial
/// `runs.json` would come back as "no history at all" the next time anything
/// parsed it.
fn write_json<T: Serialize>(path: &Path, value: &T) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_string_pretty(value)?)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

pub fn now_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn new_routine_id() -> String {
    format!("rt-{}", &uuid::Uuid::new_v4().simple().to_string()[..8])
}

fn new_run_id() -> String {
    format!("run-{}", &uuid::Uuid::new_v4().simple().to_string()[..12])
}

/// RFC 3339 in UTC, or `-` for a zero timestamp.
pub fn format_timestamp(ms: u64) -> String {
    chrono::DateTime::from_timestamp_millis(ms as i64)
        .map(|dt| dt.to_rfc3339_opts(chrono::SecondsFormat::Secs, true))
        .unwrap_or_else(|| "-".to_string())
}

/// `90` → `1m 30s`, for trigger summaries.
pub fn format_duration(seconds: u64) -> String {
    let (h, m, s) = (seconds / 3600, (seconds % 3600) / 60, seconds % 60);
    let mut parts = Vec::new();
    if h > 0 {
        parts.push(format!("{h}h"));
    }
    if m > 0 {
        parts.push(format!("{m}m"));
    }
    if s > 0 || parts.is_empty() {
        parts.push(format!("{s}s"));
    }
    parts.join(" ")
}

/// Parse `30s`, `5m`, `2h`, or a bare number of seconds.
pub fn parse_duration(raw: &str) -> Result<u64, String> {
    let text = raw.trim().to_ascii_lowercase();
    if text.is_empty() {
        return Err("Empty interval.".to_string());
    }
    let (digits, unit) = match text.chars().last() {
        Some(c) if c.is_ascii_alphabetic() => (&text[..text.len() - 1], c),
        _ => (text.as_str(), 's'),
    };
    let value: u64 = digits
        .trim()
        .parse()
        .map_err(|_| format!("'{raw}' is not an interval — try 30s, 5m or 2h."))?;
    let secs = match unit {
        's' => value,
        'm' => value * 60,
        'h' => value * 3600,
        'd' => value * 86_400,
        other => return Err(format!("Unknown interval unit '{other}' — use s, m, h or d.")),
    };
    Ok(secs)
}

#[cfg(test)]
mod tests;
