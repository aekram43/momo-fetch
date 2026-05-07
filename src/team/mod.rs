//! Agent teams — multi-process coordination (US-021).
//!
//! Implements a team system where a lead agent spawns worker agents as
//! separate processes, each in its own tmux pane (with optional git worktree).
//! Communication uses a file-based message queue (mailbox).

use std::collections::HashMap;
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
    /// Worker was stopped by the user.
    Stopped,
}

impl std::fmt::Display for WorkerStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Starting => write!(f, "starting"),
            Self::Running => write!(f, "running"),
            Self::Completed => write!(f, "completed"),
            Self::Failed(e) => write!(f, "failed: {e}"),
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

/// State for a single worker.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerState {
    pub name: String,
    pub task: String,
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
}

/// Persistent team state, serialized to `.harness/team.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamState {
    /// Unique team session ID.
    pub id: String,
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
    pub fn send(&self, msg: MailboxMessage) -> anyhow::Result<()> {
        let filename = format!(
            "{}_{}_{}.json",
            msg.timestamp, msg.from, msg.to
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

    /// Create a tmux session for the team.
    /// Returns the session name.
    pub fn create_session(team_id: &str) -> anyhow::Result<String> {
        let session_name = format!("team-{team_id}");

        // Check if session already exists
        let check = std::process::Command::new("tmux")
            .args(["has-session", "-t", &session_name])
            .output()?;

        if check.status.success() {
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

    /// Kill a tmux session.
    pub fn kill_session(session_name: &str) -> anyhow::Result<()> {
        std::process::Command::new("tmux")
            .args(["kill-session", "-t", session_name])
            .output()?;

        Ok(())
    }

    /// List panes in a session with their names.
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

    /// Start a new team with the given workers.
    pub fn start(&mut self, workers: Vec<WorkerDef>) -> anyhow::Result<String> {
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

        let team_id = format!(
            "team-{}",
            chrono::Local::now().format("%Y%m%d-%H%M%S")
        );
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)?
            .as_millis() as u64;

        let mailbox_path = self.project_path.join(".harness").join("mailbox");
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

        for worker_def in workers {
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
                    .unwrap_or_else(|_| "agent-harness".into());

                let work_dir_str = work_dir.display().to_string();
                let cmd = format!(
                    "cd {} && {} -p '{}' 2>&1 | tee .harness/worker-{}.log",
                    work_dir_str,
                    binary_path,
                    worker_def.task.replace('\'', "'\\''"),
                    worker_def.name,
                );

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
                    branch,
                    use_worktree,
                    status: WorkerStatus::Starting,
                    work_dir,
                    pane_id,
                    pid: None,
                    result: None,
                },
            );
            worker_names.push(worker_def.name);
        }

        let state = TeamState {
            id: team_id.clone(),
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
                        match msg.msg_type.as_str() {
                            "ready" => {
                                worker.status = WorkerStatus::Running;
                            }
                            "progress" => {
                                worker.status = WorkerStatus::Running;
                            }
                            "completed" => {
                                worker.status = WorkerStatus::Completed;
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

        // Check if all workers have completed
        let should_complete = if let Some(ref state) = self.state {
            state.status == TeamStatus::Running && state.workers.values().all(|w| {
                matches!(w.status, WorkerStatus::Completed | WorkerStatus::Failed(_))
            })
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
        let session_name = format!("team-{}", state.id);
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

/// Result of a merge operation.
#[derive(Debug, Clone)]
pub struct MergeResult {
    pub worker: String,
    pub branch: String,
    pub success: bool,
    pub message: String,
}

// ─── Helpers ───────────────────────────────────────────────────────

/// Sanitize a name for use as a git branch name.
fn sanitize_branch_name(name: &str) -> String {
    name.chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '-' })
        .collect::<String>()
        .trim_matches('-')
        .to_string()
}

/// Generate a unique ID.
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
    fn test_team_state_serialization() {
        let mut workers = HashMap::new();
        workers.insert(
            "worker-1".into(),
            WorkerState {
                name: "worker-1".into(),
                task: "Fix lint".into(),
                branch: "team/worker-1".into(),
                use_worktree: true,
                status: WorkerStatus::Running,
                work_dir: PathBuf::from("/tmp/worktree-1"),
                pane_id: Some("%0".into()),
                pid: None,
                result: None,
            },
        );

        let state = TeamState {
            id: "team-test".into(),
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
        assert_eq!(parsed.workers.len(), 1);
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
        let result = service.start(vec![]);
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
            })
            .collect();

        let result = service.start(workers);
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
