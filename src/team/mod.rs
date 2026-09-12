//! Agent teams — multi-process coordination (US-021).
//!
//! Implements a team system where a lead agent spawns worker agents as
//! separate processes, each in its own tmux pane (with optional git worktree).
//! Communication uses a file-based message queue (mailbox).

use std::collections::{BTreeMap, HashMap};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

// ─── Types ─────────────────────────────────────────────────────────

/// Status of an individual worker agent.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum WorkerStatus {
    /// Worker process has been spawned but hasn't reported ready yet.
    Starting,
    /// Worker is actively running a task.
    Running,
    /// Worker has completed its task successfully.
    Completed,
    /// Worker failed with an error.
    Failed(String),
    /// Process exited non-zero before it ever reported for duty — a bad
    /// agent name, a missing API key, a config the worker refused.
    FailedToStart(String),
    /// Process is gone and did not leave cleanly: non-zero exit after it had
    /// started working, or a tmux pane that disappeared under it.
    Crashed(String),
    /// A restart has been issued; the replacement pane has not reported yet.
    Restarting,
    /// Worker was stopped by the user.
    Stopped,
}

impl WorkerStatus {
    /// Whether this worker will do no more work without intervention.
    ///
    /// Drives "is the team finished" — a crashed worker ends the team's wait
    /// just as a completed one does, which is the whole reason the crash
    /// states exist.
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            Self::Completed | Self::Failed(_) | Self::FailedToStart(_) | Self::Crashed(_)
        )
    }
}

impl std::fmt::Display for WorkerStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Starting => write!(f, "starting"),
            Self::Running => write!(f, "running"),
            Self::Completed => write!(f, "completed"),
            Self::Failed(e) => write!(f, "failed: {e}"),
            Self::FailedToStart(e) => write!(f, "failed_to_start: {e}"),
            Self::Crashed(e) => write!(f, "crashed: {e}"),
            Self::Restarting => write!(f, "restarting"),
            Self::Stopped => write!(f, "stopped"),
        }
    }
}

/// Status of the overall team.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum TeamStatus {
    /// No team is active.
    Idle,
    /// Team is running (workers are active).
    Running,
    /// All workers have completed; awaiting merge.
    Completed,
    /// Team has been stopped.
    Stopped,
}

impl std::fmt::Display for TeamStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Idle => write!(f, "idle"),
            Self::Running => write!(f, "running"),
            Self::Completed => write!(f, "completed"),
            Self::Stopped => write!(f, "stopped"),
        }
    }
}

/// What a worker's process does once its opening task is done.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WorkerMode {
    /// Run the task, report, exit. What a worker has always been, and still
    /// the default — a team that only needs work fanned out once wants this.
    #[default]
    Oneshot,
    /// Stay up after the task, polling the mailbox for more work until told to
    /// shut down. For a worker the lead talks to repeatedly.
    Standby,
}

impl std::fmt::Display for WorkerMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Oneshot => write!(f, "oneshot"),
            Self::Standby => write!(f, "standby"),
        }
    }
}

impl std::str::FromStr for WorkerMode {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "oneshot" | "one-shot" => Ok(Self::Oneshot),
            "standby" => Ok(Self::Standby),
            _ => Err(format!(
                "Unknown worker mode '{s}'. Expected: oneshot or standby"
            )),
        }
    }
}

/// Definition of a single worker agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerDef {
    /// Unique name for this worker.
    pub name: String,
    /// Task description for the worker.
    pub task: String,
    /// Branch name for the worker (defaults to `team/<name>`).
    pub branch: Option<String>,
    /// Whether to use a git worktree (default: false).
    pub use_worktree: Option<bool>,
    /// Agent personality to load for this worker (from .harness/agents/<name>.md).
    #[serde(default)]
    pub agent: Option<String>,
    /// Permission mode the worker runs under: `strict`, `auto` or `yolo`.
    /// Defaults to [`DEFAULT_WORKER_PERMISSION`] — see the constant for why it
    /// is not `strict`.
    #[serde(default)]
    pub permission: Option<String>,
    /// `oneshot` (default) or `standby` — see [`WorkerMode`].
    #[serde(default)]
    pub mode: Option<String>,
}

/// A message in the mailbox queue.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MailboxMessage {
    /// Sender agent name.
    pub from: String,
    /// Recipient agent name ("lead" or a worker name).
    pub to: String,
    /// Message type.
    pub msg_type: String,
    /// Message body.
    pub body: String,
    /// Unix timestamp (millis).
    pub timestamp: u64,
}

// ─── Memory Sidecar Mailbox Protocol (Option C) ─────────────────────

/// Well-known mailbox identities for the memory sidecar protocol.
pub mod sidecar_protocol {
    /// The sidecar process identifies itself as "memory-sidecar".
    pub const SIDECAR_ID: &str = "memory-sidecar";
    /// The main process identifies itself as "main".
    pub const MAIN_ID: &str = "main";

    /// Message types for memory sidecar communication.
    pub mod msg_type {
        /// Search request: body contains the search query.
        pub const SEARCH_REQUEST: &str = "search_request";
        /// Search response: body contains JSON search results.
        pub const SEARCH_RESPONSE: &str = "search_response";
        /// Write request: body contains JSON TurnSummary.
        pub const WRITE_REQUEST: &str = "write_request";
        /// Write response: body contains the MemCell reference.
        pub const WRITE_RESPONSE: &str = "write_response";
        /// Health check / ready signal.
        pub const READY: &str = "ready";
        /// Shutdown request.
        pub const SHUTDOWN: &str = "shutdown";
    }
}

/// State for a single worker.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerState {
    pub name: String,
    pub task: String,
    /// Agent personality the worker was launched with, if any.
    #[serde(default)]
    pub agent: Option<String>,
    /// Permission mode the worker was launched with.
    #[serde(default = "default_worker_permission")]
    pub permission: String,
    /// Whether the worker exits after its task or stays up polling.
    #[serde(default)]
    pub mode: WorkerMode,
    pub branch: String,
    pub use_worktree: bool,
    pub status: WorkerStatus,
    /// Path to the worker's working directory (worktree or project root).
    pub work_dir: PathBuf,
    /// Tmux pane ID (if managed by tmux).
    pub pane_id: Option<String>,
    /// PID of the worker process.
    pub pid: Option<u32>,
    /// Result from the worker (set on completion).
    pub result: Option<String>,
    /// Timestamp (millis) of the last mailbox message received from this
    /// worker. `None` until the worker posts one — see `status()`.
    #[serde(default)]
    pub last_message_ts: Option<u64>,
}

/// Persistent team state, serialized to `.harness/team.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamState {
    /// Unique team session ID.
    pub id: String,
    /// Name of the config this team was started from, when it came from one.
    #[serde(default)]
    pub name: Option<String>,
    /// Overall team status.
    pub status: TeamStatus,
    /// Project path.
    pub project_path: PathBuf,
    /// Path to the mailbox directory.
    pub mailbox_path: PathBuf,
    /// Worker states keyed by name.
    pub workers: HashMap<String, WorkerState>,
    /// Team creation timestamp.
    pub created_at: u64,
    /// Team start timestamp.
    pub started_at: Option<u64>,
    /// Team completion timestamp.
    pub completed_at: Option<u64>,
}

// ─── Mailbox ───────────────────────────────────────────────────────

/// File-based message queue for inter-agent communication.
///
/// Messages are stored as JSON files in `<project>/.harness/mailbox/`.
/// Each message is a separate file named `<timestamp>_<from>_<to>.json`.
/// This provides atomic writes and simple polling.
pub struct Mailbox {
    path: PathBuf,
}

impl Mailbox {
    /// Create or open a mailbox at the given path.
    pub fn open(path: &Path) -> anyhow::Result<Self> {
        std::fs::create_dir_all(path)?;
        Ok(Self {
            path: path.to_path_buf(),
        })
    }

    /// Send a message to the mailbox.
    ///
    /// The filename carries a unique suffix because the timestamp is only
    /// milliseconds: a worker that reports a result and then reports ready —
    /// back to back, same sender, same recipient — produced the same name
    /// twice and the second message silently replaced the first. The result
    /// simply disappeared, and only sometimes, depending on which side of a
    /// millisecond the two sends landed.
    pub fn send(&self, msg: MailboxMessage) -> anyhow::Result<()> {
        let filename = format!(
            "{}_{}_{}_{}.json",
            msg.timestamp,
            msg.from,
            msg.to,
            &uuid::Uuid::new_v4().to_string()[..8]
        );
        let filepath = self.path.join(&filename);
        let tmp = filepath.with_extension("tmp");

        let content = serde_json::to_string_pretty(&msg)?;
        {
            let mut f = std::fs::File::create(&tmp)?;
            f.write_all(content.as_bytes())?;
            f.flush()?;
        }
        std::fs::rename(&tmp, &filepath)?;
        Ok(())
    }

    /// Receive all messages addressed to a specific agent.
    /// Removes messages from the mailbox after reading.
    pub fn receive(&self, recipient: &str) -> anyhow::Result<Vec<MailboxMessage>> {
        let mut messages = Vec::new();
        let entries = std::fs::read_dir(&self.path)?;

        for entry in entries {
            let entry = entry?;
            let path = entry.path();
            if path.extension().map(|e| e == "json").unwrap_or(false) {
                if let Ok(content) = std::fs::read_to_string(&path) {
                    if let Ok(msg) = serde_json::from_str::<MailboxMessage>(&content) {
                        if msg.to == recipient {
                            messages.push(msg);
                            // Remove after reading (consume)
                            let _ = std::fs::remove_file(&path);
                        }
                    }
                }
            }
        }

        // Sort by timestamp
        messages.sort_by_key(|m| m.timestamp);
        Ok(messages)
    }

    /// Peek at all messages for a recipient without removing them.
    pub fn peek(&self, recipient: &str) -> anyhow::Result<Vec<MailboxMessage>> {
        let mut messages = Vec::new();
        let entries = std::fs::read_dir(&self.path)?;

        for entry in entries {
            let entry = entry?;
            let path = entry.path();
            if path.extension().map(|e| e == "json").unwrap_or(false) {
                if let Ok(content) = std::fs::read_to_string(&path) {
                    if let Ok(msg) = serde_json::from_str::<MailboxMessage>(&content) {
                        if msg.to == recipient {
                            messages.push(msg);
                        }
                    }
                }
            }
        }

        messages.sort_by_key(|m| m.timestamp);
        Ok(messages)
    }

    /// Get count of unread messages for a recipient.
    pub fn unread_count(&self, recipient: &str) -> usize {
        self.peek(recipient)
            .map(|m| m.len())
            .unwrap_or(0)
    }

    /// Count queued messages per recipient in a single pass.
    ///
    /// Recipients with nothing queued are absent — seed the map with the
    /// names you care about if you need them reported as zero.
    pub fn unread_by_recipient(&self) -> anyhow::Result<BTreeMap<String, usize>> {
        let mut counts: BTreeMap<String, usize> = BTreeMap::new();

        for entry in std::fs::read_dir(&self.path)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().map(|e| e == "json").unwrap_or(false) {
                if let Ok(content) = std::fs::read_to_string(&path) {
                    if let Ok(msg) = serde_json::from_str::<MailboxMessage>(&content) {
                        *counts.entry(msg.to).or_insert(0) += 1;
                    }
                }
            }
        }

        Ok(counts)
    }

    /// Clear all messages from the mailbox.
    pub fn clear(&self) -> anyhow::Result<()> {
        let entries = std::fs::read_dir(&self.path)?;
        for entry in entries {
            let entry = entry?;
            if entry.path().extension().map(|e| e == "json").unwrap_or(false) {
                let _ = std::fs::remove_file(entry.path());
            }
        }
        Ok(())
    }
}

// ─── Tmux Manager ──────────────────────────────────────────────────

/// Manages tmux sessions and panes for worker agents.
pub struct TmuxManager;

impl TmuxManager {
    /// Check if tmux is available.
    pub fn is_available() -> bool {
        std::process::Command::new("which")
            .arg("tmux")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    /// Check whether a tmux session with this name exists.
    pub fn has_session(session_name: &str) -> bool {
        std::process::Command::new("tmux")
            .args(["has-session", "-t", session_name])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    /// Name of the tmux session hosting a team's workers.
    pub fn session_name(team_id: &str) -> String {
        format!("team-{team_id}")
    }

    /// Create a tmux session for the team.
    /// Returns the session name.
    pub fn create_session(team_id: &str) -> anyhow::Result<String> {
        let session_name = Self::session_name(team_id);

        if Self::has_session(&session_name) {
            return Ok(session_name);
        }

        // Create new detached session
        std::process::Command::new("tmux")
            .args([
                "new-session",
                "-d",
                "-s",
                &session_name,
                "-x",
                "200",
                "-y",
                "50",
            ])
            .output()?;

        Ok(session_name)
    }

    /// Create a new window in the session for a worker.
    /// Returns the pane ID.
    pub fn create_worker_pane(
        session_name: &str,
        worker_name: &str,
    ) -> anyhow::Result<String> {
        // Create a new window named after the worker
        let output = std::process::Command::new("tmux")
            .args([
                "new-window",
                "-t",
                session_name,
                "-n",
                worker_name,
                "-P",
                "-F",
                "#{pane_id}",
            ])
            .output()?;

        if !output.status.success() {
            return Err(anyhow::anyhow!(
                "Failed to create tmux pane for '{worker_name}': {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }

        let pane_id = String::from_utf8_lossy(&output.stdout).trim().to_string();
        Ok(pane_id)
    }

    /// Send keys (commands) to a tmux pane.
    pub fn send_keys(pane_id: &str, command: &str) -> anyhow::Result<()> {
        std::process::Command::new("tmux")
            .args(["send-keys", "-t", pane_id, command, "Enter"])
            .output()?;

        Ok(())
    }

    /// Whether a pane still exists anywhere in tmux.
    ///
    /// A worker's window outlives its process (the shell stays), so this only
    /// answers "was the window torn down", not "is the agent still running" —
    /// the exit file answers that.
    pub fn has_pane(pane_id: &str) -> bool {
        let output = std::process::Command::new("tmux")
            .args(["list-panes", "-a", "-F", "#{pane_id}"])
            .output();

        match output {
            Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout)
                .lines()
                .any(|line| line.trim() == pane_id),
            // No tmux, or no server running: nothing to distinguish a dead
            // pane from a machine without tmux, so claim nothing.
            _ => false,
        }
    }

    /// Kill a single pane (used when restarting one worker).
    pub fn kill_pane(pane_id: &str) -> anyhow::Result<()> {
        std::process::Command::new("tmux")
            .args(["kill-pane", "-t", pane_id])
            .output()?;
        Ok(())
    }

    /// Kill a tmux session.
    pub fn kill_session(session_name: &str) -> anyhow::Result<()> {
        std::process::Command::new("tmux")
            .args(["kill-session", "-t", session_name])
            .output()?;

        Ok(())
    }

    /// List panes in a session with their names.
    #[allow(dead_code)]
    pub fn list_panes(session_name: &str) -> anyhow::Result<Vec<(String, String)>> {
        let output = std::process::Command::new("tmux")
            .args([
                "list-panes",
                "-t",
                session_name,
                "-F",
                "#{pane_id} #{window_name}",
            ])
            .output()?;

        if !output.status.success() {
            return Ok(Vec::new());
        }

        let panes = String::from_utf8_lossy(&output.stdout)
            .lines()
            .filter_map(|line| {
                let parts: Vec<&str> = line.splitn(2, ' ').collect();
                if parts.len() == 2 {
                    Some((parts[0].to_string(), parts[1].to_string()))
                } else {
                    None
                }
            })
            .collect();

        Ok(panes)
    }
}

// ─── Git Worktree Manager ──────────────────────────────────────────

/// Manages git worktrees for worker isolation.
pub struct WorktreeManager;

impl WorktreeManager {
    /// Create a git worktree for a worker.
    /// Returns the path to the worktree.
    pub fn create(
        project_path: &Path,
        branch_name: &str,
        worktree_path: &Path,
    ) -> anyhow::Result<PathBuf> {
        // Create a new branch from HEAD
        let branch_output = std::process::Command::new("git")
            .args(["branch", branch_name])
            .current_dir(project_path)
            .output()?;

        if !branch_output.status.success() {
            let stderr = String::from_utf8_lossy(&branch_output.stderr);
            // Branch may already exist — that's okay if it does
            if !stderr.contains("already exists") {
                return Err(anyhow::anyhow!(
                    "Failed to create branch '{branch_name}': {stderr}"
                ));
            }
        }

        // Add worktree
        let output = std::process::Command::new("git")
            .args([
                "worktree",
                "add",
                worktree_path.to_str().unwrap_or(""),
                branch_name,
            ])
            .current_dir(project_path)
            .output()?;

        if !output.status.success() {
            return Err(anyhow::anyhow!(
                "Failed to create worktree at '{}': {}",
                worktree_path.display(),
                String::from_utf8_lossy(&output.stderr)
            ));
        }

        Ok(worktree_path.to_path_buf())
    }

    /// Remove a git worktree.
    pub fn remove(project_path: &Path, worktree_path: &Path) -> anyhow::Result<()> {
        let output = std::process::Command::new("git")
            .args([
                "worktree",
                "remove",
                "--force",
                worktree_path.to_str().unwrap_or(""),
            ])
            .current_dir(project_path)
            .output()?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            // Try pruning if remove fails
            let _ = std::process::Command::new("git")
                .args(["worktree", "prune"])
                .current_dir(project_path)
                .output();

            return Err(anyhow::anyhow!(
                "Failed to remove worktree '{}': {stderr}",
                worktree_path.display()
            ));
        }

        Ok(())
    }

    /// Merge a worker's branch back into the current branch.
    pub fn merge_branch(project_path: &Path, branch_name: &str) -> anyhow::Result<String> {
        let output = std::process::Command::new("git")
            .args(["merge", "--no-edit", branch_name])
            .current_dir(project_path)
            .output()?;

        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
        } else {
            // Abort the merge to leave the repo in a clean state
            let _ = std::process::Command::new("git")
                .args(["merge", "--abort"])
                .current_dir(project_path)
                .output();

            Err(anyhow::anyhow!(
                "Failed to merge branch '{branch_name}': {}",
                String::from_utf8_lossy(&output.stderr)
            ))
        }
    }

    /// Delete a branch.
    pub fn delete_branch(project_path: &Path, branch_name: &str) -> anyhow::Result<()> {
        let output = std::process::Command::new("git")
            .args(["branch", "-d", branch_name])
            .current_dir(project_path)
            .output()?;

        if !output.status.success() {
            // Force delete if regular delete fails (e.g., unmerged)
            let _ = std::process::Command::new("git")
                .args(["branch", "-D", branch_name])
                .current_dir(project_path)
                .output();
        }

        Ok(())
    }

    /// Check if the project is a git repo.
    pub fn is_git_repo(path: &Path) -> bool {
        std::process::Command::new("git")
            .args(["rev-parse", "--git-dir"])
            .current_dir(path)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }
}

// ─── Team Service ──────────────────────────────────────────────────

/// Extensions accepted for `.harness/teams/<name>.*`, in lookup order.
pub const TEAM_CONFIG_EXTENSIONS: [&str; 3] = ["json", "yml", "yaml"];

/// Permission mode a worker runs under when its config does not say.
///
/// **Not `strict`.** A worker is a headless one-shot in a detached tmux pane
/// with nobody watching it: under `strict` adk asks for confirmation on the
/// first mutating tool and the pane sits at "Tool confirmation required"
/// forever, because there is no one there to answer. `auto` lets the
/// non-destructive work through while `shell_exec`'s own destructive-pattern
/// check still refuses `rm -rf /`, `git push --force` and friends — a guard
/// that lives in the tool, not in the confirmation policy, so it holds no
/// matter what the pane is running. And **not `yolo`**, which would remove
/// that check too.
pub const DEFAULT_WORKER_PERMISSION: &str = "auto";

fn default_worker_permission() -> String {
    DEFAULT_WORKER_PERMISSION.to_string()
}

/// Central service for managing agent teams.
pub struct TeamService {
    /// Path to the project directory.
    project_path: PathBuf,
    /// Path to the team state file.
    state_path: PathBuf,
    /// Path to the worktrees directory.
    worktrees_path: PathBuf,
    /// Current team state (None if no team is active).
    state: Option<TeamState>,
}

impl TeamService {
    /// Create a new TeamService for the given project.
    pub fn new(project_path: &Path) -> anyhow::Result<Self> {
        let harness_dir = project_path.join(".harness");
        std::fs::create_dir_all(&harness_dir)?;

        let state_path = harness_dir.join("team.json");
        let worktrees_path = harness_dir.join("worktrees");
        std::fs::create_dir_all(&worktrees_path)?;

        // Try to load existing state
        let state = if state_path.exists() {
            let content = std::fs::read_to_string(&state_path)?;
            let state: TeamState = serde_json::from_str(&content)?;
            // Only restore if team was running (not completed/stopped)
            match state.status {
                TeamStatus::Running | TeamStatus::Completed => Some(state),
                _ => None,
            }
        } else {
            None
        };

        Ok(Self {
            project_path: project_path.to_path_buf(),
            state_path,
            worktrees_path,
            state,
        })
    }

    /// Get the current team state.
    pub fn state(&self) -> Option<&TeamState> {
        self.state.as_ref()
    }

    /// The project this service is bound to.
    pub fn project_path(&self) -> &Path {
        &self.project_path
    }

    /// Directory holding this project's team configs.
    pub fn teams_dir(&self) -> PathBuf {
        self.project_path.join(".harness").join("teams")
    }

    /// Resolve `<name>` to a config file in `.harness/teams/`, trying each
    /// supported extension in `TEAM_CONFIG_EXTENSIONS` order.
    pub fn team_config_path(&self, name: &str) -> Option<PathBuf> {
        let dir = self.teams_dir();
        TEAM_CONFIG_EXTENSIONS
            .iter()
            .map(|ext| dir.join(format!("{name}.{ext}")))
            .find(|p| p.exists())
    }

    /// Load a team config from `.harness/teams/<name>.{json,yml,yaml}`.
    ///
    /// All three extensions are accepted, and JSON parses through the YAML
    /// reader (YAML 1.2 is a superset of JSON) — one loader, so the REPL and
    /// the `team` subcommand can never disagree about what a config says.
    pub fn load_team_config(&self, name: &str) -> anyhow::Result<TeamConfig> {
        match self.team_config_path(name) {
            Some(path) => Self::parse_team_config(&path, name),
            None => Err(anyhow::anyhow!(
                "Team config '{}' not found in {} (tried {})",
                name,
                self.teams_dir().display(),
                TEAM_CONFIG_EXTENSIONS
                    .iter()
                    .map(|e| format!(".{e}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            )),
        }
    }

    /// List all available team configs in `.harness/teams/`.
    pub fn list_team_configs(&self) -> anyhow::Result<Vec<String>> {
        let teams_dir = self.teams_dir();
        if !teams_dir.exists() {
            return Ok(Vec::new());
        }

        let mut names = Vec::new();
        for entry in std::fs::read_dir(&teams_dir)? {
            let entry = entry?;
            let path = entry.path();
            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
            if TEAM_CONFIG_EXTENSIONS.contains(&ext) {
                if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                    names.push(stem.to_string());
                }
            }
        }
        names.sort();
        names.dedup();
        Ok(names)
    }

    /// Convert a TeamConfig into WorkerDefs.
    pub fn config_to_workers(config: &TeamConfig) -> Vec<WorkerDef> {
        config
            .workers
            .iter()
            .map(|w| WorkerDef {
                name: w.name.clone(),
                task: w.task.clone(),
                branch: w.branch.clone(),
                use_worktree: w.worktree,
                agent: w.agent.clone(),
                permission: w.permission.clone(),
                mode: w.mode.clone(),
            })
            .collect()
    }

    fn parse_team_config(path: &Path, name: &str) -> anyhow::Result<TeamConfig> {
        let content = std::fs::read_to_string(path)?;
        let config: TeamConfig = serde_yaml::from_str(&content).map_err(|e| {
            anyhow::anyhow!("Failed to parse team config '{}': {e}", name)
        })?;

        if config.workers.is_empty() {
            return Err(anyhow::anyhow!(
                "Team config '{}' has no workers defined.",
                name
            ));
        }

        if config.workers.len() > 8 {
            return Err(anyhow::anyhow!(
                "Team config '{}' has too many workers ({}). Maximum is 8.",
                name,
                config.workers.len()
            ));
        }

        Ok(config)
    }

    /// Start a new team with the given workers.
    ///
    /// `name` is the config the team came from, when it came from one. It is
    /// carried in `TeamState` so `status` can report which config is running
    /// without the caller having to remember — pass `None` for a team defined
    /// on the spot.
    pub fn start(
        &mut self,
        name: Option<String>,
        workers: Vec<WorkerDef>,
    ) -> anyhow::Result<String> {
        if self.state.is_some() {
            return Err(anyhow::anyhow!(
                "Team already active. Stop it first with /team stop."
            ));
        }

        if workers.is_empty() {
            return Err(anyhow::anyhow!(
                "No workers specified. Provide at least one worker."
            ));
        }

        if workers.len() > 8 {
            return Err(anyhow::anyhow!(
                "Too many workers ({}). Maximum is 8.",
                workers.len()
            ));
        }

        // Resolved before a tmux session or a worktree exists: a typo in one
        // worker's `permission` should not leave half a team behind. Parsing
        // through `PermissionMode` also means only the three canonical words
        // can ever reach the shell command below.
        let permissions = workers
            .iter()
            .map(|w| resolve_worker_permission(&w.name, w.permission.as_deref()))
            .collect::<anyhow::Result<Vec<_>>>()?;
        let modes = workers
            .iter()
            .map(|w| resolve_worker_mode(&w.name, w.mode.as_deref()))
            .collect::<anyhow::Result<Vec<_>>>()?;

        let team_id = format!(
            "team-{}",
            chrono::Local::now().format("%Y%m%d-%H%M%S")
        );
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)?
            .as_millis() as u64;

        let harness_dir = self.project_path.join(".harness");
        // The exit file is written by `echo $? > …` in the worker's shell line,
        // which will not create the directory for it.
        std::fs::create_dir_all(workers_dir(&harness_dir))?;
        let mailbox_path = harness_dir.join("mailbox");
        let mailbox = Mailbox::open(&mailbox_path)?;
        mailbox.clear()?;

        // Create tmux session (if available)
        let tmux_session = if TmuxManager::is_available() {
            Some(TmuxManager::create_session(&team_id)?)
        } else {
            None
        };

        // Set up workers
        let mut worker_states = HashMap::new();
        let mut worker_names = Vec::new();

        for ((worker_def, permission), mode) in
            workers.into_iter().zip(permissions).zip(modes)
        {
            let branch = worker_def
                .branch
                .clone()
                .unwrap_or_else(|| format!("team/{}", sanitize_branch_name(&worker_def.name)));

            let use_worktree = worker_def.use_worktree.unwrap_or(false);
            let work_dir = if use_worktree {
                let wt_path = self.worktrees_path.join(&worker_def.name);
                if WorktreeManager::is_git_repo(&self.project_path) {
                    WorktreeManager::create(
                        &self.project_path,
                        &branch,
                        &wt_path,
                    )?
                } else {
                    self.project_path.clone()
                }
            } else {
                self.project_path.clone()
            };

            let pane_id = if let Some(ref session) = tmux_session {
                match TmuxManager::create_worker_pane(session, &worker_def.name) {
                    Ok(id) => Some(id),
                    Err(e) => {
                        tracing::warn!(
                            "Failed to create tmux pane for '{}': {e}",
                            worker_def.name
                        );
                        None
                    }
                }
            } else {
                None
            };

            // Launch worker process in tmux pane
            if let Some(ref pid) = pane_id {
                let binary_path = std::env::current_exe()
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|_| "momo-fetch".into());

                // Evidence from a previous team, which would otherwise be
                // read as this one's: an old exit code makes a worker born
                // "crashed", and an old log makes its first failure look like
                // it already happened. The log is appended to from here on, so
                // a restart keeps the crash that prompted it.
                for stale in [
                    worker_exit_path(&harness_dir, &worker_def.name),
                    worker_log_path(&harness_dir, &worker_def.name),
                    worker_heartbeat_path(&harness_dir, &worker_def.name),
                ] {
                    let _ = std::fs::remove_file(stale);
                }

                let cmd = build_worker_command(&WorkerLaunch {
                    binary_path: &binary_path,
                    work_dir: &work_dir,
                    harness_dir: &harness_dir,
                    mailbox_path: &mailbox_path,
                    agent: worker_def.agent.as_deref(),
                    permission,
                    mode,
                    task: &worker_def.task,
                    name: &worker_def.name,
                });

                if let Err(e) = TmuxManager::send_keys(pid, &cmd) {
                    tracing::warn!(
                        "Failed to send command to tmux pane for '{}': {e}",
                        worker_def.name
                    );
                }
            }

            worker_states.insert(
                worker_def.name.clone(),
                WorkerState {
                    name: worker_def.name.clone(),
                    task: worker_def.task,
                    agent: worker_def.agent.clone(),
                    permission: permission.to_string(),
                    mode,
                    branch,
                    use_worktree,
                    status: WorkerStatus::Starting,
                    work_dir,
                    pane_id,
                    pid: None,
                    result: None,
                    last_message_ts: None,
                },
            );
            worker_names.push(worker_def.name);
        }

        let state = TeamState {
            id: team_id.clone(),
            name,
            status: TeamStatus::Running,
            project_path: self.project_path.clone(),
            mailbox_path,
            workers: worker_states,
            created_at: now,
            started_at: Some(now),
            completed_at: None,
        };

        self.save_state(&state)?;
        self.state = Some(state);

        Ok(team_id)
    }

    /// Get the current status of the team.
    pub fn status(&mut self) -> TeamStatus {
        if self.state.is_none() {
            return TeamStatus::Idle;
        }

        // Collect mailbox updates into a separate list before mutating state
        let mailbox_path = self.state.as_ref().unwrap().mailbox_path.clone();
        let messages: Vec<MailboxMessage> = Mailbox::open(&mailbox_path)
            .and_then(|m| m.receive("lead"))
            .unwrap_or_default();

        // Apply messages to worker states
        if !messages.is_empty() {
            for msg in &messages {
                if let Some(ref mut state) = self.state {
                    if let Some(worker) = state.workers.get_mut(&msg.from) {
                        worker.last_message_ts = Some(msg.timestamp);
                        match msg.msg_type.as_str() {
                            "ready" => {
                                worker.status = WorkerStatus::Running;
                            }
                            "progress" => {
                                worker.status = WorkerStatus::Running;
                            }
                            "completed" => {
                                // A standby worker finishing a task is not
                                // finished — it goes back to waiting. Only a
                                // one-shot's "completed" is the end of it.
                                worker.status = match worker.mode {
                                    WorkerMode::Standby => WorkerStatus::Running,
                                    WorkerMode::Oneshot => WorkerStatus::Completed,
                                };
                                worker.result = Some(msg.body.clone());
                            }
                            "failed" => {
                                worker.status = WorkerStatus::Failed(msg.body.clone());
                            }
                            _ => {}
                        }
                    }
                }
            }
            let _ = self.save_current_state();
        }

        // What the workers did not say for themselves: exit codes and panes.
        self.refresh_liveness();

        // Check if every worker is done for good. A standby worker never is —
        // it goes back to waiting after each task — so a team with one stays
        // Running until it is stopped.
        let should_complete = if let Some(ref state) = self.state {
            state.status == TeamStatus::Running
                && state.workers.values().all(|w| w.status.is_terminal())
        } else {
            false
        };

        if should_complete {
            if let Some(ref mut s) = self.state {
                s.status = TeamStatus::Completed;
                s.completed_at = Some(
                    SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .map(|d| d.as_millis() as u64)
                        .unwrap_or(0),
                );
            }
            let _ = self.save_current_state();
        }

        self.state.as_ref().map(|s| s.status.clone()).unwrap_or(TeamStatus::Idle)
    }

    /// Merge all completed workers' branches.
    /// Returns a summary of merge results.
    pub fn merge(&mut self) -> anyhow::Result<Vec<MergeResult>> {
        let state = self.state.as_mut().ok_or_else(|| {
            anyhow::anyhow!("No active team to merge.")
        })?;

        if !matches!(state.status, TeamStatus::Completed) {
            return Err(anyhow::anyhow!(
                "Team is not in completed state. Current: {}",
                state.status
            ));
        }

        let mut results = Vec::new();

        for (_, worker) in &mut state.workers {
            if matches!(worker.status, WorkerStatus::Completed) && worker.use_worktree {
                match WorktreeManager::merge_branch(&state.project_path, &worker.branch) {
                    Ok(msg) => {
                        // Clean up worktree
                        if worker.work_dir != state.project_path {
                            let _ = WorktreeManager::remove(
                                &state.project_path,
                                &worker.work_dir,
                            );
                        }
                        // Delete the feature branch
                        let _ = WorktreeManager::delete_branch(
                            &state.project_path,
                            &worker.branch,
                        );
                        results.push(MergeResult {
                            worker: worker.name.clone(),
                            branch: worker.branch.clone(),
                            success: true,
                            message: msg,
                        });
                    }
                    Err(e) => {
                        results.push(MergeResult {
                            worker: worker.name.clone(),
                            branch: worker.branch.clone(),
                            success: false,
                            message: e.to_string(),
                        });
                    }
                }
            }
        }

        // Update state
        state.status = TeamStatus::Stopped;
        let _ = self.save_current_state();

        Ok(results)
    }

    /// Stop the team (kill all workers, clean up).
    pub fn stop(&mut self) -> anyhow::Result<()> {
        let state = self.state.as_mut().ok_or_else(|| {
            anyhow::anyhow!("No active team to stop.")
        })?;

        // Kill tmux session
        let session_name = TmuxManager::session_name(&state.id);
        let _ = TmuxManager::kill_session(&session_name);

        // Clean up worktrees
        for (_, worker) in &mut state.workers {
            if worker.use_worktree && worker.work_dir != state.project_path {
                let _ = WorktreeManager::remove(&state.project_path, &worker.work_dir);
                let _ = WorktreeManager::delete_branch(&state.project_path, &worker.branch);
            }
            worker.status = WorkerStatus::Stopped;
        }

        // Clear mailbox
        if let Ok(mailbox) = Mailbox::open(&state.mailbox_path) {
            let _ = mailbox.clear();
        }

        state.status = TeamStatus::Stopped;
        self.save_current_state()?;

        // Clear state
        self.state = None;
        let _ = std::fs::remove_file(&self.state_path);

        Ok(())
    }

    /// Stop the team, forcing the tmux session down even when the recorded
    /// state has drifted from reality — a `team.json` left behind by a reboot,
    /// or a session that outlived the state file.
    ///
    /// Scoped to this project: the only session it touches is the one named
    /// after the team id recorded in `<project>/.harness/team.json`, whatever
    /// status that file carries. tmux session names carry no project, so a
    /// broader sweep would take down other projects' teams.
    pub fn stop_force(&mut self) -> anyhow::Result<ForceStopReport> {
        let session = self
            .recorded_team_id()
            .map(|id| TmuxManager::session_name(&id))
            .filter(|s| TmuxManager::has_session(s));

        let stopped_team = match self.state.as_ref().map(|s| s.id.clone()) {
            Some(id) => {
                self.stop()?;
                Some(id)
            }
            None => None,
        };

        let mut killed_sessions = Vec::new();
        if let Some(session) = session {
            if TmuxManager::has_session(&session) {
                TmuxManager::kill_session(&session)?;
            }
            killed_sessions.push(session);
        }

        // A state file the normal path leaves behind (status Stopped, or a
        // shape `TeamService::new` declined to restore) would otherwise keep
        // resurfacing.
        if self.state_path.exists() {
            std::fs::remove_file(&self.state_path)?;
        }

        Ok(ForceStopReport {
            stopped_team,
            killed_sessions,
        })
    }

    /// Team id as recorded on disk, regardless of the status it carries.
    ///
    /// Read as loose JSON on purpose: a state file this build cannot
    /// deserialize is exactly the case `stop_force` exists for.
    fn recorded_team_id(&self) -> Option<String> {
        let content = std::fs::read_to_string(&self.state_path).ok()?;
        let value: serde_json::Value = serde_json::from_str(&content).ok()?;
        value
            .get("id")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
    }

    /// Reconcile worker state with what the processes actually did.
    ///
    /// The mailbox only carries what a worker chose to say, and nothing in a
    /// default run says anything. This reads what it could not: the exit code
    /// its shell wrote when the process ended, and whether its tmux pane is
    /// still there. Without this, a worker that died in its first second sits
    /// at `starting` forever and the team never finishes.
    fn refresh_liveness(&mut self) {
        let harness_dir = self.project_path.join(".harness");
        let Some(state) = self.state.as_mut() else {
            return;
        };

        // Only trust a missing pane as evidence while the team's session is
        // still up. If the whole session is gone — or tmux is not installed —
        // "pane not found" says nothing about any individual worker.
        let session_alive = TmuxManager::has_session(&TmuxManager::session_name(&state.id));

        let mut changed = false;
        for worker in state.workers.values_mut() {
            if matches!(worker.status, WorkerStatus::Stopped) || worker.status.is_terminal() {
                continue;
            }
            if let Some(next) = observed_status(&harness_dir, worker, session_alive) {
                worker.status = next;
                changed = true;
            }
        }

        if changed {
            let _ = self.save_current_state();
        }
    }

    /// When a standby worker last touched its heartbeat file, in millis.
    ///
    /// `None` for a one-shot worker, or a standby one that has not reached its
    /// first poll.
    pub fn worker_heartbeat(&self, worker_name: &str) -> Option<u64> {
        let path = worker_heartbeat_path(&self.project_path.join(".harness"), worker_name);
        std::fs::read_to_string(path)
            .ok()?
            .trim()
            .parse::<u64>()
            .ok()
    }

    /// Put a message in a worker's inbox, from the lead.
    ///
    /// Only a standby worker reads its inbox; a one-shot worker has already
    /// exited by the time anyone could write to it.
    pub fn send_to_worker(
        &self,
        worker_name: &str,
        msg_type: &str,
        body: &str,
    ) -> anyhow::Result<()> {
        let state = self
            .state
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("No active team."))?;

        let worker = state
            .workers
            .get(worker_name)
            .ok_or_else(|| anyhow::anyhow!(
                "Unknown worker '{worker_name}'. This team has: {}",
                sorted_worker_names(state).join(", ")
            ))?;

        if worker.mode != WorkerMode::Standby {
            return Err(anyhow::anyhow!(
                "Worker '{worker_name}' runs in {} mode — it exits after its task and never \
                 reads its inbox. Give it \"mode\": \"standby\" in the team config.",
                worker.mode
            ));
        }

        let mailbox = Mailbox::open(&state.mailbox_path)?;
        mailbox.send(MailboxMessage {
            from: "lead".to_string(),
            to: worker_name.to_string(),
            msg_type: msg_type.to_string(),
            body: body.to_string(),
            timestamp: now_millis(),
        })
    }

    /// Relaunch one worker in a fresh pane, same task and settings.
    ///
    /// For a worker that crashed, or one whose pane was torn down. Requires
    /// tmux, since a pane is the only thing that ever runs a worker.
    pub fn restart(&mut self, worker_name: &str) -> anyhow::Result<()> {
        if !TmuxManager::is_available() {
            return Err(anyhow::anyhow!(
                "Restart needs tmux — a worker only ever runs inside a pane."
            ));
        }

        let harness_dir = self.project_path.join(".harness");
        let binary_path = std::env::current_exe()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|_| "momo-fetch".into());

        let state = self
            .state
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("No active team."))?;

        let known = sorted_worker_names(state);
        let mailbox_path = state.mailbox_path.clone();
        let session = TmuxManager::create_session(&state.id)?;

        let worker = state.workers.get_mut(worker_name).ok_or_else(|| {
            anyhow::anyhow!(
                "Unknown worker '{worker_name}'. This team has: {}",
                known.join(", ")
            )
        })?;

        if let Some(old_pane) = &worker.pane_id {
            let _ = TmuxManager::kill_pane(old_pane);
        }

        // Both are evidence from the previous life and would be read as this
        // one's.
        let _ = std::fs::remove_file(worker_exit_path(&harness_dir, worker_name));
        let _ = std::fs::remove_file(worker_heartbeat_path(&harness_dir, worker_name));

        let pane_id = TmuxManager::create_worker_pane(&session, worker_name)?;
        let permission = worker
            .permission
            .parse::<crate::sandbox::PermissionMode>()
            .unwrap_or(crate::sandbox::PermissionMode::Auto);

        let cmd = build_worker_command(&WorkerLaunch {
            binary_path: &binary_path,
            work_dir: &worker.work_dir,
            harness_dir: &harness_dir,
            mailbox_path: &mailbox_path,
            agent: worker.agent.as_deref(),
            permission,
            mode: worker.mode,
            task: &worker.task,
            name: worker_name,
        });
        TmuxManager::send_keys(&pane_id, &cmd)?;

        worker.pane_id = Some(pane_id);
        worker.status = WorkerStatus::Restarting;
        worker.result = None;
        worker.last_message_ts = None;

        // A team that had finished has work in it again — leaving it
        // `completed` would describe a team that is demonstrably running.
        if state.status == TeamStatus::Completed {
            state.status = TeamStatus::Running;
            state.completed_at = None;
        }

        self.save_current_state()
    }

    // ── Persistence ────────────────────────────────────────────

    fn save_state(&self, state: &TeamState) -> anyhow::Result<()> {
        let content = serde_json::to_string_pretty(state)?;
        let tmp = self.state_path.with_extension("tmp");

        {
            let mut f = std::fs::File::create(&tmp)?;
            f.write_all(content.as_bytes())?;
            f.flush()?;
        }
        std::fs::rename(&tmp, &self.state_path)?;
        Ok(())
    }

    fn save_current_state(&self) -> anyhow::Result<()> {
        if let Some(ref state) = self.state {
            self.save_state(state)?;
        }
        Ok(())
    }
}

/// What a forced stop actually tore down.
#[derive(Debug, Clone)]
pub struct ForceStopReport {
    /// Id of the team that was stopped through the normal path, if one was
    /// active.
    pub stopped_team: Option<String>,
    /// tmux sessions that existed when the force stop began and are now gone.
    pub killed_sessions: Vec<String>,
}

/// Result of a merge operation.
#[derive(Debug, Clone)]
pub struct MergeResult {
    pub worker: String,
    pub branch: String,
    pub success: bool,
    pub message: String,
}

// ─── Team Config File ──────────────────────────────────────────────

/// Team configuration loaded from `.harness/teams/<name>.{json,yml,yaml}`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamConfig {
    /// Display name for the team.
    #[serde(default)]
    pub name: Option<String>,
    /// Worker definitions.
    pub workers: Vec<TeamWorkerConfig>,
}

/// A single worker in a team config file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamWorkerConfig {
    /// Worker name.
    pub name: String,
    /// Task description.
    pub task: String,
    /// Agent personality to load (from .harness/agents/).
    #[serde(default)]
    pub agent: Option<String>,
    /// Branch name (defaults to team/<name>).
    #[serde(default)]
    pub branch: Option<String>,
    /// Whether to use a git worktree.
    #[serde(default)]
    pub worktree: Option<bool>,
    /// Permission mode for this worker: `strict`, `auto` or `yolo`.
    /// Omitted means [`DEFAULT_WORKER_PERMISSION`].
    #[serde(default)]
    pub permission: Option<String>,
    /// `oneshot` (default) or `standby` — see [`WorkerMode`].
    #[serde(default)]
    pub mode: Option<String>,
}

// ─── Helpers ───────────────────────────────────────────────────────

/// The shell line a worker's tmux window runs.
///
/// `--permission` is never omitted. Left off, the worker takes the project's
/// `settings.json` or, failing that, `strict` — and a strict worker in a
/// detached pane stops at its first mutating tool waiting for an answer that
/// cannot arrive.
/// Everything the shell line for one worker depends on.
pub struct WorkerLaunch<'a> {
    /// Path to the momo-fetch binary the lead is running.
    pub binary_path: &'a str,
    /// Where the worker runs — its worktree, or the project root.
    pub work_dir: &'a Path,
    /// The **lead's** `.harness/`. Logs and exit files land here even when the
    /// worker lives in a worktree, so one directory answers "how is the team
    /// doing" instead of one per checkout.
    pub harness_dir: &'a Path,
    /// The lead's mailbox. A worker in a worktree would otherwise resolve
    /// `.harness/mailbox` inside its own checkout and talk to nobody.
    pub mailbox_path: &'a Path,
    pub agent: Option<&'a str>,
    pub permission: crate::sandbox::PermissionMode,
    pub mode: WorkerMode,
    pub task: &'a str,
    pub name: &'a str,
}

/// The shell line a worker's tmux window runs.
///
/// Three things are never left implicit:
///
/// - `--permission`, because the fallback is `strict` and a strict worker in a
///   detached pane stops at its first mutating tool forever.
/// - `--mailbox`, because a worktree has its own `.harness/`.
/// - the exit code, written to `worker-<name>.exit` the moment the process
///   ends. It is the only thing that can tell a finished worker from a dead
///   one: the tmux window survives either way.
fn build_worker_command(launch: &WorkerLaunch<'_>) -> String {
    let agent_flag = match launch.agent {
        Some(name) => format!(" -a '{}'", shell_quote(name)),
        None => String::new(),
    };

    let standby_flag = match launch.mode {
        WorkerMode::Standby => format!(" --team-worker '{}'", shell_quote(launch.name)),
        WorkerMode::Oneshot => String::new(),
    };

    // The braces keep `$?` as momo-fetch's status rather than tee's, and are
    // POSIX sh — tmux runs whatever login shell the user has.
    format!(
        "cd '{}' && {{ '{}'{}{} --permission {} --mailbox '{}' -p '{}'; echo $? > '{}'; }} 2>&1 | tee -a '{}'",
        shell_quote(&launch.work_dir.display().to_string()),
        shell_quote(launch.binary_path),
        agent_flag,
        standby_flag,
        launch.permission,
        shell_quote(&launch.mailbox_path.display().to_string()),
        shell_quote(launch.task),
        shell_quote(&worker_exit_path(launch.harness_dir, launch.name).display().to_string()),
        shell_quote(&worker_log_path(launch.harness_dir, launch.name).display().to_string()),
    )
}

/// Where a worker's runtime files live: three per worker, in their own
/// directory rather than loose in `.harness/`.
pub fn workers_dir(harness_dir: &Path) -> PathBuf {
    harness_dir.join("workers")
}

/// Where a worker's process writes its exit code.
pub fn worker_exit_path(harness_dir: &Path, worker_name: &str) -> PathBuf {
    workers_dir(harness_dir).join(format!("worker-{worker_name}.exit"))
}

/// Where a worker's output is teed.
pub fn worker_log_path(harness_dir: &Path, worker_name: &str) -> PathBuf {
    workers_dir(harness_dir).join(format!("worker-{worker_name}.log"))
}

/// Where a standby worker touches down each poll, so the lead can tell
/// "thinking" from "wedged" without a message having been sent.
pub fn worker_heartbeat_path(harness_dir: &Path, worker_name: &str) -> PathBuf {
    workers_dir(harness_dir).join(format!("worker-{worker_name}.heartbeat"))
}

/// Escape a value for a single-quoted shell string.
fn shell_quote(value: &str) -> String {
    value.replace('\'', "'\\''")
}

/// Resolve a worker's configured permission mode, defaulting to
/// [`DEFAULT_WORKER_PERMISSION`] and rejecting anything that is not one of the
/// three known modes.
fn resolve_worker_permission(
    worker: &str,
    configured: Option<&str>,
) -> anyhow::Result<crate::sandbox::PermissionMode> {
    let raw = configured.unwrap_or(DEFAULT_WORKER_PERMISSION);
    raw.parse::<crate::sandbox::PermissionMode>().map_err(|e| {
        anyhow::anyhow!("Worker '{worker}': {e}")
    })
}

/// What the filesystem and tmux say about a worker, when it said nothing.
///
/// Returns `None` when there is no evidence either way — which is the normal
/// case for a worker that is simply still working.
fn observed_status(
    harness_dir: &Path,
    worker: &WorkerState,
    session_alive: bool,
) -> Option<WorkerStatus> {
    // The exit code is the process's own last word, so it wins.
    if let Some(code) = read_exit_code(harness_dir, &worker.name) {
        return Some(match code {
            0 => WorkerStatus::Completed,
            c if worker.last_message_ts.is_none() => WorkerStatus::FailedToStart(format!(
                "exited {c} without reporting — check {}",
                worker_log_path(harness_dir, &worker.name).display()
            )),
            c => WorkerStatus::Crashed(format!("exited {c}")),
        });
    }

    // No exit code written, yet the window it would have been written from is
    // gone: something took the pane down mid-run.
    match &worker.pane_id {
        Some(pane) if session_alive && !TmuxManager::has_pane(pane) => {
            Some(WorkerStatus::Crashed(format!("tmux pane {pane} is gone")))
        }
        _ => None,
    }
}

/// Read a worker's recorded exit code, if its process has ended.
///
/// An unparseable file means the shell is mid-write; treat it as "no answer
/// yet" rather than as a crash.
fn read_exit_code(harness_dir: &Path, worker_name: &str) -> Option<i32> {
    std::fs::read_to_string(worker_exit_path(harness_dir, worker_name))
        .ok()?
        .trim()
        .parse::<i32>()
        .ok()
}

/// Worker names of a team, sorted, for error messages that have to list them.
fn sorted_worker_names(state: &TeamState) -> Vec<String> {
    let mut names: Vec<String> = state.workers.keys().cloned().collect();
    names.sort();
    names
}

/// Milliseconds since the Unix epoch.
fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Resolve a worker's configured mode, defaulting to [`WorkerMode::Oneshot`]
/// so a config written before standby existed behaves exactly as it did.
fn resolve_worker_mode(worker: &str, configured: Option<&str>) -> anyhow::Result<WorkerMode> {
    match configured {
        None => Ok(WorkerMode::default()),
        Some(raw) => raw
            .parse::<WorkerMode>()
            .map_err(|e| anyhow::anyhow!("Worker '{worker}': {e}")),
    }
}

/// Sanitize a name for use as a git branch name.
fn sanitize_branch_name(name: &str) -> String {
    name.chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '-' })
        .collect::<String>()
        .trim_matches('-')
        .to_string()
}

/// Generate a unique ID.
#[allow(dead_code)]
fn generate_id() -> String {
    uuid::Uuid::new_v4().to_string()[..8].to_string()
}

// ─── Tests ─────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_worker_status_display() {
        assert_eq!(WorkerStatus::Starting.to_string(), "starting");
        assert_eq!(WorkerStatus::Running.to_string(), "running");
        assert_eq!(WorkerStatus::Completed.to_string(), "completed");
        assert_eq!(WorkerStatus::Stopped.to_string(), "stopped");
        assert_eq!(
            WorkerStatus::Failed("error msg".into()).to_string(),
            "failed: error msg"
        );
    }

    #[test]
    fn test_team_status_display() {
        assert_eq!(TeamStatus::Idle.to_string(), "idle");
        assert_eq!(TeamStatus::Running.to_string(), "running");
        assert_eq!(TeamStatus::Completed.to_string(), "completed");
        assert_eq!(TeamStatus::Stopped.to_string(), "stopped");
    }

    #[test]
    fn test_sanitize_branch_name() {
        assert_eq!(sanitize_branch_name("worker-1"), "worker-1");
        assert_eq!(sanitize_branch_name("my worker"), "my-worker");
        assert_eq!(sanitize_branch_name("fix: auth #123"), "fix--auth--123");
        assert_eq!(sanitize_branch_name("---leading"), "leading");
    }

    #[test]
    fn test_worker_permission_defaults_to_auto_not_strict() {
        use crate::sandbox::PermissionMode;

        // The whole point: a worker with nothing configured must not come up
        // strict, because a strict headless pane deadlocks on its first
        // mutating tool.
        assert_eq!(
            resolve_worker_permission("w", None).unwrap(),
            PermissionMode::Auto
        );
        assert_eq!(DEFAULT_WORKER_PERMISSION, "auto");

        assert_eq!(
            resolve_worker_permission("w", Some("yolo")).unwrap(),
            PermissionMode::Yolo
        );
        assert_eq!(
            resolve_worker_permission("w", Some("strict")).unwrap(),
            PermissionMode::Strict
        );

        let err = resolve_worker_permission("analyst", Some("relaxed"))
            .unwrap_err()
            .to_string();
        assert!(err.contains("analyst"), "{err}");
        assert!(err.contains("relaxed"), "{err}");
    }

    fn launch<'a>(
        agent: Option<&'a str>,
        permission: crate::sandbox::PermissionMode,
        mode: WorkerMode,
        task: &'a str,
        name: &'a str,
    ) -> WorkerLaunch<'a> {
        WorkerLaunch {
            binary_path: "/usr/local/bin/momo-fetch",
            work_dir: Path::new("/repo"),
            harness_dir: Path::new("/repo/.harness"),
            mailbox_path: Path::new("/repo/.harness/mailbox"),
            agent,
            permission,
            mode,
            task,
            name,
        }
    }

    #[test]
    fn test_worker_command_always_carries_permission_and_mailbox() {
        use crate::sandbox::PermissionMode;

        let cmd = build_worker_command(&launch(
            Some("yolo-validator"),
            PermissionMode::Auto,
            WorkerMode::Oneshot,
            "Backtest the strategy",
            "validator",
        ));

        assert!(cmd.contains("--permission auto"), "{cmd}");
        assert!(cmd.contains("--mailbox '/repo/.harness/mailbox'"), "{cmd}");
        assert!(cmd.contains("-a 'yolo-validator'"), "{cmd}");
        assert!(cmd.starts_with("cd '/repo' && { '/usr/local/bin/momo-fetch'"), "{cmd}");
        // Logs and exit codes land in the lead's .harness, not the worker's.
        assert!(cmd.contains("echo $? > '/repo/.harness/workers/worker-validator.exit'"), "{cmd}");
        assert!(cmd.ends_with("| tee -a '/repo/.harness/workers/worker-validator.log'"), "{cmd}");
        // A one-shot stays a one-shot.
        assert!(!cmd.contains("--team-worker"), "{cmd}");

        // No agent, and a task carrying a quote that must not break out of the
        // single-quoted argument.
        let cmd = build_worker_command(&launch(
            None,
            PermissionMode::Strict,
            WorkerMode::Oneshot,
            "don't stop",
            "solo",
        ));
        // Precise: `tee -a` also contains " -a ".
        assert!(!cmd.contains("momo-fetch' -a "), "{cmd}");
        assert!(cmd.contains("--permission strict"), "{cmd}");
        assert!(cmd.contains(r"-p 'don'\''t stop'"), "{cmd}");
    }

    #[test]
    fn test_standby_worker_command_carries_the_loop_flag() {
        use crate::sandbox::PermissionMode;

        let cmd = build_worker_command(&launch(
            None,
            PermissionMode::Auto,
            WorkerMode::Standby,
            "Stand by for trade requests",
            "executor",
        ));

        assert!(cmd.contains("--team-worker 'executor'"), "{cmd}");
        assert!(cmd.contains("-p 'Stand by for trade requests'"), "{cmd}");
    }

    #[test]
    fn test_worker_mode_parsing() {
        assert_eq!(resolve_worker_mode("w", None).unwrap(), WorkerMode::Oneshot);
        assert_eq!(
            resolve_worker_mode("w", Some("standby")).unwrap(),
            WorkerMode::Standby
        );
        assert_eq!(
            resolve_worker_mode("w", Some("oneshot")).unwrap(),
            WorkerMode::Oneshot
        );

        let err = resolve_worker_mode("analyst", Some("daemon"))
            .unwrap_err()
            .to_string();
        assert!(err.contains("analyst"), "{err}");
        assert!(err.contains("daemon"), "{err}");
    }

    #[test]
    fn test_observed_status_reads_the_exit_file() {
        let dir = tempfile::tempdir().unwrap();
        let harness_dir = dir.path();
        std::fs::create_dir_all(workers_dir(harness_dir)).unwrap();

        let mut worker = WorkerState {
            name: "w".into(),
            task: "t".into(),
            agent: None,
            permission: "auto".into(),
            mode: WorkerMode::Oneshot,
            branch: "team/w".into(),
            use_worktree: false,
            status: WorkerStatus::Starting,
            work_dir: harness_dir.to_path_buf(),
            pane_id: None,
            pid: None,
            result: None,
            last_message_ts: None,
        };

        // Still running: nothing on disk, nothing to say.
        assert!(observed_status(harness_dir, &worker, false).is_none());

        // Died before ever reporting — that is a failure to start, not a
        // crash, and the message points at the log.
        std::fs::write(worker_exit_path(harness_dir, "w"), "1\n").unwrap();
        match observed_status(harness_dir, &worker, false) {
            Some(WorkerStatus::FailedToStart(msg)) => {
                assert!(msg.contains("exited 1"), "{msg}");
                assert!(msg.contains("worker-w.log"), "{msg}");
            }
            other => panic!("expected FailedToStart, got {other:?}"),
        }

        // Same exit code, but it had reported in: that is a crash.
        worker.last_message_ts = Some(1);
        assert!(matches!(
            observed_status(harness_dir, &worker, false),
            Some(WorkerStatus::Crashed(_))
        ));

        // Clean exit is a completion whatever else happened.
        std::fs::write(worker_exit_path(harness_dir, "w"), "0").unwrap();
        assert_eq!(
            observed_status(harness_dir, &worker, false),
            Some(WorkerStatus::Completed)
        );

        // A half-written file is not evidence.
        std::fs::write(worker_exit_path(harness_dir, "w"), "").unwrap();
        assert!(observed_status(harness_dir, &worker, false).is_none());
    }

    #[test]
    fn test_worker_status_is_terminal() {
        assert!(WorkerStatus::Completed.is_terminal());
        assert!(WorkerStatus::Failed("e".into()).is_terminal());
        assert!(WorkerStatus::Crashed("e".into()).is_terminal());
        assert!(WorkerStatus::FailedToStart("e".into()).is_terminal());

        // A worker that can still do something is not terminal — Stopped
        // included, since the team is over by then anyway.
        assert!(!WorkerStatus::Starting.is_terminal());
        assert!(!WorkerStatus::Running.is_terminal());
        assert!(!WorkerStatus::Restarting.is_terminal());
        assert!(!WorkerStatus::Stopped.is_terminal());
    }

    #[test]
    fn test_config_to_workers_carries_permission() {
        let config: TeamConfig = serde_json::from_str(
            r#"{
              "workers": [
                {"name": "a", "task": "t", "permission": "yolo"},
                {"name": "b", "task": "t"}
              ]
            }"#,
        )
        .unwrap();

        let workers = TeamService::config_to_workers(&config);
        assert_eq!(workers[0].permission.as_deref(), Some("yolo"));
        assert_eq!(workers[1].permission, None);

        // Unset means the default, resolved at start time.
        assert_eq!(
            resolve_worker_permission(&workers[1].name, workers[1].permission.as_deref()).unwrap(),
            crate::sandbox::PermissionMode::Auto
        );
    }

    #[test]
    fn test_load_team_config_json() {
        let dir = tempfile::tempdir().unwrap();
        let teams = dir.path().join(".harness").join("teams");
        std::fs::create_dir_all(&teams).unwrap();
        std::fs::write(
            teams.join("squad.json"),
            r#"{
              "name": "squad",
              "description": "ignored extra key",
              "workers": [
                {"name": "analyst", "task": "gather evidence", "agent": "yolo-analyst"},
                {"name": "executor", "task": "trade", "worktree": true}
              ]
            }"#,
        )
        .unwrap();

        let service = TeamService::new(dir.path()).unwrap();
        let config = service.load_team_config("squad").unwrap();

        assert_eq!(config.name.as_deref(), Some("squad"));
        assert_eq!(config.workers.len(), 2);
        assert_eq!(config.workers[0].name, "analyst");
        assert_eq!(config.workers[0].agent.as_deref(), Some("yolo-analyst"));
        assert_eq!(config.workers[1].worktree, Some(true));
    }

    #[test]
    fn test_load_team_config_yml_and_yaml() {
        let dir = tempfile::tempdir().unwrap();
        let teams = dir.path().join(".harness").join("teams");
        std::fs::create_dir_all(&teams).unwrap();
        std::fs::write(
            teams.join("squad.yml"),
            "name: squad\nworkers:\n  - name: analyst\n    task: gather evidence\n",
        )
        .unwrap();
        std::fs::write(
            teams.join("other.yaml"),
            "workers:\n  - name: solo\n    task: do it all\n",
        )
        .unwrap();

        let service = TeamService::new(dir.path()).unwrap();

        let squad = service.load_team_config("squad").unwrap();
        assert_eq!(squad.workers[0].task, "gather evidence");

        let other = service.load_team_config("other").unwrap();
        assert_eq!(other.workers[0].name, "solo");
        assert!(other.name.is_none());
    }

    #[test]
    fn test_load_team_config_missing_names_the_extensions_tried() {
        let dir = tempfile::tempdir().unwrap();
        let service = TeamService::new(dir.path()).unwrap();

        let err = service.load_team_config("nope").unwrap_err().to_string();
        assert!(err.contains(".json"), "{err}");
        assert!(err.contains(".yml"), "{err}");
        assert!(err.contains(".yaml"), "{err}");
    }

    #[test]
    fn test_list_team_configs_covers_every_extension() {
        let dir = tempfile::tempdir().unwrap();
        let teams = dir.path().join(".harness").join("teams");
        std::fs::create_dir_all(&teams).unwrap();
        for file in ["a.json", "b.yml", "c.yaml", "notes.md"] {
            std::fs::write(teams.join(file), "workers: []").unwrap();
        }

        let service = TeamService::new(dir.path()).unwrap();
        assert_eq!(
            service.list_team_configs().unwrap(),
            vec!["a".to_string(), "b".to_string(), "c".to_string()]
        );
    }

    #[test]
    fn test_team_config_path_prefers_json() {
        let dir = tempfile::tempdir().unwrap();
        let teams = dir.path().join(".harness").join("teams");
        std::fs::create_dir_all(&teams).unwrap();
        std::fs::write(teams.join("squad.json"), "{\"workers\": []}").unwrap();
        std::fs::write(teams.join("squad.yml"), "workers: []").unwrap();

        let service = TeamService::new(dir.path()).unwrap();
        let path = service.team_config_path("squad").unwrap();
        assert_eq!(path.extension().unwrap(), "json");
    }

    #[test]
    fn test_mailbox_keeps_messages_sent_in_the_same_millisecond() {
        // A standby worker sends its result and its ready flag one after the
        // other. Sharing a filename cost it the result.
        let dir = tempfile::tempdir().unwrap();
        let mailbox = Mailbox::open(dir.path()).unwrap();

        for msg_type in ["completed", "ready"] {
            mailbox
                .send(MailboxMessage {
                    from: "keeper".into(),
                    to: "lead".into(),
                    msg_type: msg_type.into(),
                    body: format!("body of {msg_type}"),
                    timestamp: 1_700_000_000_000,
                })
                .unwrap();
        }

        let queued = mailbox.peek("lead").unwrap();
        assert_eq!(queued.len(), 2, "one message overwrote the other");
        let types: Vec<&str> = queued.iter().map(|m| m.msg_type.as_str()).collect();
        assert!(types.contains(&"completed"), "{types:?}");
        assert!(types.contains(&"ready"), "{types:?}");
    }

    #[test]
    fn test_mailbox_unread_by_recipient() {
        let dir = tempfile::tempdir().unwrap();
        let mailbox = Mailbox::open(dir.path()).unwrap();

        for (i, to) in ["executor", "executor", "lead"].iter().enumerate() {
            mailbox
                .send(MailboxMessage {
                    from: "lead".into(),
                    to: (*to).into(),
                    msg_type: "task".into(),
                    body: "go".into(),
                    timestamp: 1000 + i as u64,
                })
                .unwrap();
        }

        let counts = mailbox.unread_by_recipient().unwrap();
        assert_eq!(counts.get("executor"), Some(&2));
        assert_eq!(counts.get("lead"), Some(&1));
        assert_eq!(counts.get("analyst"), None);
    }

    #[test]
    fn test_worker_def_deserialization() {
        let def: WorkerDef = serde_json::from_str(r#"{
            "name": "worker-1",
            "task": "Fix all lint errors"
        }"#).unwrap();
        assert_eq!(def.name, "worker-1");
        assert_eq!(def.task, "Fix all lint errors");
        assert!(def.branch.is_none());
        assert!(def.use_worktree.is_none());
    }

    #[test]
    fn test_worker_def_with_branch() {
        let def: WorkerDef = serde_json::from_str(r#"{
            "name": "worker-2",
            "task": "Add tests",
            "branch": "feature/tests",
            "use_worktree": true
        }"#).unwrap();
        assert_eq!(def.branch.unwrap(), "feature/tests");
        assert!(def.use_worktree.unwrap());
    }

    #[test]
    fn test_mailbox_message_serialization() {
        let msg = MailboxMessage {
            from: "worker-1".into(),
            to: "lead".into(),
            msg_type: "completed".into(),
            body: "Done!".into(),
            timestamp: 1700000000000,
        };
        let json = serde_json::to_string(&msg).unwrap();
        let parsed: MailboxMessage = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.from, "worker-1");
        assert_eq!(parsed.to, "lead");
        assert_eq!(parsed.msg_type, "completed");
    }

    #[test]
    fn test_mailbox_send_receive() {
        let tmp = tempfile::tempdir().unwrap();
        let mailbox = Mailbox::open(tmp.path()).unwrap();

        let msg = MailboxMessage {
            from: "worker-1".into(),
            to: "lead".into(),
            msg_type: "completed".into(),
            body: "Task done!".into(),
            timestamp: 1700000000000,
        };

        mailbox.send(msg).unwrap();

        // Should be able to receive
        let messages = mailbox.receive("lead").unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].body, "Task done!");

        // Second receive should be empty (consumed)
        let messages2 = mailbox.receive("lead").unwrap();
        assert!(messages2.is_empty());
    }

    #[test]
    fn test_mailbox_peek() {
        let tmp = tempfile::tempdir().unwrap();
        let mailbox = Mailbox::open(tmp.path()).unwrap();

        let msg = MailboxMessage {
            from: "worker-1".into(),
            to: "lead".into(),
            msg_type: "progress".into(),
            body: "50% done".into(),
            timestamp: 1700000000000,
        };

        mailbox.send(msg).unwrap();

        // Peek should not consume
        let peeked = mailbox.peek("lead").unwrap();
        assert_eq!(peeked.len(), 1);

        let peeked2 = mailbox.peek("lead").unwrap();
        assert_eq!(peeked2.len(), 1);
    }

    #[test]
    fn test_mailbox_unread_count() {
        let tmp = tempfile::tempdir().unwrap();
        let mailbox = Mailbox::open(tmp.path()).unwrap();

        assert_eq!(mailbox.unread_count("lead"), 0);

        let msg = MailboxMessage {
            from: "worker-1".into(),
            to: "lead".into(),
            msg_type: "ready".into(),
            body: "Ready!".into(),
            timestamp: 1700000000000,
        };

        mailbox.send(msg).unwrap();
        assert_eq!(mailbox.unread_count("lead"), 1);
        assert_eq!(mailbox.unread_count("worker-1"), 0);
    }

    #[test]
    fn test_mailbox_multiple_messages() {
        let tmp = tempfile::tempdir().unwrap();
        let mailbox = Mailbox::open(tmp.path()).unwrap();

        for i in 0..5 {
            let msg = MailboxMessage {
                from: format!("worker-{i}"),
                to: "lead".into(),
                msg_type: "completed".into(),
                body: format!("Task {i} done"),
                timestamp: 1700000000000 + i as u64,
            };
            mailbox.send(msg).unwrap();
        }

        let messages = mailbox.receive("lead").unwrap();
        assert_eq!(messages.len(), 5);
        // Should be sorted by timestamp
        assert_eq!(messages[0].from, "worker-0");
        assert_eq!(messages[4].from, "worker-4");
    }

    #[test]
    fn test_mailbox_clear() {
        let tmp = tempfile::tempdir().unwrap();
        let mailbox = Mailbox::open(tmp.path()).unwrap();

        let msg = MailboxMessage {
            from: "worker-1".into(),
            to: "lead".into(),
            msg_type: "test".into(),
            body: "test".into(),
            timestamp: 1700000000000,
        };

        mailbox.send(msg).unwrap();
        assert_eq!(mailbox.unread_count("lead"), 1);

        mailbox.clear().unwrap();
        assert_eq!(mailbox.unread_count("lead"), 0);
    }

    #[test]
    fn test_team_state_reads_pre_agent_state_files() {
        // A `.harness/team.json` written before `name`, `agent` and
        // `last_message_ts` existed must still load — otherwise upgrading the
        // binary strands whatever team is running.
        let json = r#"{
          "id": "team-old",
          "status": "Running",
          "project_path": "/tmp/project",
          "mailbox_path": "/tmp/project/.harness/mailbox",
          "workers": {
            "w1": {
              "name": "w1",
              "task": "old task",
              "branch": "team/w1",
              "use_worktree": false,
              "status": "Starting",
              "work_dir": "/tmp/project",
              "pane_id": null,
              "pid": null,
              "result": null
            }
          },
          "created_at": 1700000000000,
          "started_at": 1700000000000,
          "completed_at": null
        }"#;

        let state: TeamState = serde_json::from_str(json).unwrap();
        assert_eq!(state.id, "team-old");
        assert!(state.name.is_none());
        assert!(state.workers["w1"].agent.is_none());
        assert!(state.workers["w1"].last_message_ts.is_none());
        assert_eq!(state.workers["w1"].permission, DEFAULT_WORKER_PERMISSION);
        // A state file from before standby existed is a team of one-shots.
        assert_eq!(state.workers["w1"].mode, WorkerMode::Oneshot);
    }

    #[test]
    fn test_team_state_serialization() {
        let mut workers = HashMap::new();
        workers.insert(
            "worker-1".into(),
            WorkerState {
                name: "worker-1".into(),
                task: "Fix lint".into(),
                agent: Some("reviewer".into()),
                permission: "auto".into(),
                mode: WorkerMode::Standby,
                branch: "team/worker-1".into(),
                use_worktree: true,
                status: WorkerStatus::Running,
                work_dir: PathBuf::from("/tmp/worktree-1"),
                pane_id: Some("%0".into()),
                pid: None,
                result: None,
                last_message_ts: Some(1700000000123),
            },
        );

        let state = TeamState {
            id: "team-test".into(),
            name: Some("squad".into()),
            status: TeamStatus::Running,
            project_path: PathBuf::from("/tmp/project"),
            mailbox_path: PathBuf::from("/tmp/project/.harness/mailbox"),
            workers,
            created_at: 1700000000000,
            started_at: Some(1700000000000),
            completed_at: None,
        };

        let json = serde_json::to_string_pretty(&state).unwrap();
        let parsed: TeamState = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.id, "team-test");
        assert_eq!(parsed.name.as_deref(), Some("squad"));
        assert_eq!(parsed.workers.len(), 1);
        let worker = &parsed.workers["worker-1"];
        assert_eq!(worker.agent.as_deref(), Some("reviewer"));
        assert_eq!(worker.last_message_ts, Some(1700000000123));
        assert!(matches!(parsed.status, TeamStatus::Running));
    }

    #[test]
    fn test_team_service_new_no_active_team() {
        let tmp = tempfile::tempdir().unwrap();
        let project_path = tmp.path();
        std::fs::create_dir_all(project_path.join(".harness")).unwrap();

        let service = TeamService::new(project_path).unwrap();
        assert!(service.state().is_none());
    }

    #[test]
    fn test_team_service_start_requires_workers() {
        let tmp = tempfile::tempdir().unwrap();
        let project_path = tmp.path();
        std::fs::create_dir_all(project_path.join(".harness")).unwrap();

        let mut service = TeamService::new(project_path).unwrap();
        let result = service.start(None, vec![]);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("No workers"));
    }

    #[test]
    fn test_team_service_start_max_workers() {
        let tmp = tempfile::tempdir().unwrap();
        let project_path = tmp.path();
        std::fs::create_dir_all(project_path.join(".harness")).unwrap();

        let mut service = TeamService::new(project_path).unwrap();

        let workers: Vec<WorkerDef> = (0..9)
            .map(|i| WorkerDef {
                name: format!("w-{i}"),
                task: format!("task {i}"),
                branch: None,
                use_worktree: Some(false),
                agent: None,
                permission: None,
                mode: None,
            })
            .collect();

        let result = service.start(None, workers);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Too many workers"));
    }

    #[test]
    fn test_team_service_stop_no_team() {
        let tmp = tempfile::tempdir().unwrap();
        let project_path = tmp.path();
        std::fs::create_dir_all(project_path.join(".harness")).unwrap();

        let mut service = TeamService::new(project_path).unwrap();
        let result = service.stop();
        assert!(result.is_err());
    }

    #[test]
    fn test_team_service_merge_no_team() {
        let tmp = tempfile::tempdir().unwrap();
        let project_path = tmp.path();
        std::fs::create_dir_all(project_path.join(".harness")).unwrap();

        let mut service = TeamService::new(project_path).unwrap();
        let result = service.merge();
        assert!(result.is_err());
    }

    #[test]
    fn test_generate_id() {
        let id = generate_id();
        assert_eq!(id.len(), 8);
    }

    #[test]
    fn test_worker_def_list() {
        let defs: Vec<WorkerDef> = serde_json::from_str(r#"[
            {"name": "w1", "task": "Fix bugs"},
            {"name": "w2", "task": "Add tests", "use_worktree": true},
            {"name": "w3", "task": "Update docs", "branch": "docs/update"}
        ]"#).unwrap();
        assert_eq!(defs.len(), 3);
        assert_eq!(defs[0].name, "w1");
        assert!(defs[1].use_worktree.unwrap());
        assert_eq!(defs[2].branch.as_ref().unwrap(), "docs/update");
    }
}
