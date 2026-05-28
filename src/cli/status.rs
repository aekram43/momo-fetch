//! Status event system for displaying background worker progress in the REPL.
//!
//! Workers (memory sidecar, task sub-agents, team workers) push `StatusEvent`s
//! into a shared channel. The REPL drains pending events and displays them
//! as inline log lines and/or a status footer.

use std::collections::HashMap;
use std::sync::Mutex;
use tokio::sync::mpsc;

/// Events emitted by background workers.
#[derive(Debug, Clone)]
pub enum StatusEvent {
    /// Worker has started a task.
    Started {
        worker: String,
        detail: String,
    },
    /// Worker is making progress.
    Progress {
        worker: String,
        detail: String,
    },
    /// Worker completed its task.
    Completed {
        worker: String,
        detail: String,
    },
    /// Worker failed.
    Failed {
        worker: String,
        error: String,
    },
}

impl StatusEvent {
    /// Which worker emitted this event.
    pub fn worker(&self) -> &str {
        match self {
            StatusEvent::Started { worker, .. }
            | StatusEvent::Progress { worker, .. }
            | StatusEvent::Completed { worker, .. }
            | StatusEvent::Failed { worker, .. } => worker,
        }
    }
}

/// Shared status channel for communicating background worker progress to the REPL.
pub struct StatusChannel {
    tx: mpsc::UnboundedSender<StatusEvent>,
    rx: Mutex<mpsc::UnboundedReceiver<StatusEvent>>,
}

impl StatusChannel {
    /// Create a new status channel.
    pub fn new() -> Self {
        let (tx, rx) = mpsc::unbounded_channel();
        Self {
            tx,
            rx: Mutex::new(rx),
        }
    }

    /// Get a sender that can be cloned and passed to workers.
    pub fn sender(&self) -> StatusSender {
        StatusSender {
            tx: self.tx.clone(),
        }
    }

    /// Drain all pending events (non-blocking). Returns events in order.
    pub fn drain_pending(&self) -> Vec<StatusEvent> {
        let mut events = Vec::new();
        if let Ok(mut rx) = self.rx.lock() {
            while let Ok(event) = rx.try_recv() {
                events.push(event);
            }
        }
        events
    }

    /// Check if any workers are still active (Started without matching Completed/Failed).
    /// Consumes events — call this only if you also display them via drain_pending.
    pub fn active_workers_from(events: &[StatusEvent]) -> Vec<(String, String)> {
        let mut active: HashMap<String, String> = HashMap::new();
        for event in events {
            match event {
                StatusEvent::Started { worker, detail } => {
                    active.insert(worker.clone(), detail.clone());
                }
                StatusEvent::Progress { worker, detail } => {
                    active.insert(worker.clone(), detail.clone());
                }
                StatusEvent::Completed { worker, .. } | StatusEvent::Failed { worker, .. } => {
                    active.remove(worker.as_str());
                }
            }
        }
        active.into_iter().collect()
    }
}

/// Sender handle for workers to emit status events.
#[derive(Clone)]
pub struct StatusSender {
    tx: mpsc::UnboundedSender<StatusEvent>,
}

impl StatusSender {
    /// Emit a status event (non-blocking, silently drops if channel is closed).
    pub fn emit(&self, event: StatusEvent) {
        let _ = self.tx.send(event);
    }

    /// Shorthand for Started event.
    pub fn started(&self, worker: &str, detail: &str) {
        self.emit(StatusEvent::Started {
            worker: worker.to_string(),
            detail: detail.to_string(),
        });
    }

    /// Shorthand for Progress event.
    pub fn progress(&self, worker: &str, detail: &str) {
        self.emit(StatusEvent::Progress {
            worker: worker.to_string(),
            detail: detail.to_string(),
        });
    }

    /// Shorthand for Completed event.
    pub fn completed(&self, worker: &str, detail: &str) {
        self.emit(StatusEvent::Completed {
            worker: worker.to_string(),
            detail: detail.to_string(),
        });
    }

    /// Shorthand for Failed event.
    pub fn failed(&self, worker: &str, error: &str) {
        self.emit(StatusEvent::Failed {
            worker: worker.to_string(),
            error: error.to_string(),
        });
    }
}

// ─── Display Formatting ──────────────────────────────────────────

/// Format a slice of StatusEvents as colored inline log lines.
/// Returns a string ready to print to stderr.
pub fn format_inline(events: &[StatusEvent]) -> String {
    let mut lines = Vec::new();
    for event in events {
        let line = match event {
            StatusEvent::Started { worker, detail } => format!(
                "  {} {}: {}",
                "\u{23f3}".dimmed(), // ⏳
                worker.cyan(),
                detail
            ),
            StatusEvent::Progress { worker, detail } => format!(
                "  {} {}: {}",
                "\u{23f3}".dimmed(), // ⏳
                worker.cyan(),
                detail
            ),
            StatusEvent::Completed { worker, detail } => format!(
                "  {} {}: {}",
                "\u{2713}".green(), // ✓
                worker.cyan(),
                detail
            ),
            StatusEvent::Failed { worker, error } => format!(
                "  {} {}: {}",
                "\u{2717}".red(), // ✗
                worker.cyan(),
                error.red()
            ),
        };
        lines.push(line);
    }
    lines.join("\n")
}

/// Format a status footer for active workers.
/// Returns None if no active workers.
pub fn format_footer(active: &[(String, String)]) -> Option<String> {
    if active.is_empty() {
        return None;
    }

    let items: Vec<String> = active
        .iter()
        .map(|(worker, detail)| {
            format!("{} {}", worker.cyan(), detail.dimmed())
        })
        .collect();

    let content = items.join(" │ ");
    Some(format!(
        "\n  {} {}",
        "\u{2500}".dimmed(), // ─
        content
    ))
}

use colored::Colorize;
