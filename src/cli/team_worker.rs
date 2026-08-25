//! Standby mode — `momo-fetch --team-worker <name> -p '<task>'`.
//!
//! A team worker has always been a one-shot: it ran the task in its pane and
//! the process ended. That is fine for fanning work out once, and useless for
//! a worker the lead wants to keep talking to — there was nothing left alive
//! to talk to, and no way to wake it.
//!
//! This keeps the process up. After the opening task it polls its inbox, runs
//! a turn per message, answers whoever asked, and touches a heartbeat file so
//! the lead can tell "thinking" from "wedged". It ends on a `shutdown`
//! message, or when the pane is killed.
//!
//! The mailbox is whatever `--mailbox` says — the lead's, not the one inside
//! the worker's own worktree.

use std::path::PathBuf;
use std::time::Duration;

use crate::harness::Harness;
use crate::team::{Mailbox, MailboxMessage};

/// How long to wait between inbox polls. Long enough not to spin a core on an
/// idle team, short enough that a message does not sit for a noticeable time.
const POLL_INTERVAL: Duration = Duration::from_secs(2);

/// Message types this loop understands. Anything else is treated as work.
mod msg_type {
    /// Sent to the lead when the worker is up and waiting.
    pub const READY: &str = "ready";
    /// Sent to the lead when a task is done. For a standby worker this means
    /// "this task is done", not "I am done".
    pub const COMPLETED: &str = "completed";
    /// Sent to the lead when a turn failed.
    pub const FAILED: &str = "failed";
    /// Received: stop polling and exit cleanly.
    pub const SHUTDOWN: &str = "shutdown";
}

/// Run as a standby worker until told to stop.
pub async fn run(
    harness: &Harness,
    worker_name: &str,
    initial_task: Option<&str>,
) -> anyhow::Result<()> {
    let mailbox_path = harness.mailbox_path();
    let mailbox = Mailbox::open(&mailbox_path)?;
    let heartbeat = heartbeat_path(harness, worker_name);

    eprintln!(
        "[{worker_name}] standby: mailbox {}",
        mailbox_path.display()
    );

    // The opening task, same as a one-shot would run it. Its reply goes to the
    // lead so `team status` has something to show before any message arrives.
    if let Some(task) = initial_task {
        match crate::cli::oneshot::run_turn(harness, task).await {
            Ok(reply) => send(&mailbox, worker_name, msg_type::COMPLETED, &reply),
            Err(e) => send(&mailbox, worker_name, msg_type::FAILED, &e.to_string()),
        }
    }

    send(
        &mailbox,
        worker_name,
        msg_type::READY,
        "Standing by for mailbox messages.",
    );
    touch_heartbeat(&heartbeat);

    loop {
        let messages = match mailbox.receive(worker_name) {
            Ok(m) => m,
            Err(e) => {
                // A transient read failure is not worth losing the worker
                // over — the next poll re-reads the same directory.
                eprintln!("[{worker_name}] mailbox read failed: {e}");
                Vec::new()
            }
        };

        for msg in messages {
            if msg.msg_type == msg_type::SHUTDOWN {
                eprintln!("[{worker_name}] shutdown received; exiting.");
                let _ = std::fs::remove_file(&heartbeat);
                return Ok(());
            }

            eprintln!(
                "[{worker_name}] task from {} ({})",
                msg.from, msg.msg_type
            );

            let prompt = format_task(&msg);
            match crate::cli::oneshot::run_turn(harness, &prompt).await {
                Ok(reply) => {
                    // Answer whoever asked; keep the lead informed either way,
                    // since the lead is what drives `team status`.
                    send(&mailbox, worker_name, msg_type::COMPLETED, &reply);
                    if msg.from != "lead" {
                        reply_to(&mailbox, worker_name, &msg.from, msg_type::COMPLETED, &reply);
                    }
                }
                Err(e) => {
                    send(&mailbox, worker_name, msg_type::FAILED, &e.to_string());
                }
            }
            touch_heartbeat(&heartbeat);
        }

        touch_heartbeat(&heartbeat);
        tokio::time::sleep(POLL_INTERVAL).await;
    }
}

/// Turn an inbox message into the turn's prompt.
///
/// The envelope is kept: a worker that knows who is asking, and what kind of
/// request this is, answers a lot better than one handed a bare body.
fn format_task(msg: &MailboxMessage) -> String {
    format!(
        "Message from '{}' (type: {}):\n\n{}",
        msg.from, msg.msg_type, msg.body
    )
}

fn send(mailbox: &Mailbox, from: &str, msg_type: &str, body: &str) {
    reply_to(mailbox, from, "lead", msg_type, body);
}

fn reply_to(mailbox: &Mailbox, from: &str, to: &str, msg_type: &str, body: &str) {
    let msg = MailboxMessage {
        from: from.to_string(),
        to: to.to_string(),
        msg_type: msg_type.to_string(),
        body: body.trim().to_string(),
        timestamp: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0),
    };

    if let Err(e) = mailbox.send(msg) {
        eprintln!("[{from}] failed to send {msg_type} to {to}: {e}");
    }
}

/// Where this worker reports that it is still alive.
///
/// Beside the lead's logs, not in the worker's own `.harness/` — a worktree
/// has its own, and the lead reads only one directory.
fn heartbeat_path(harness: &Harness, worker_name: &str) -> PathBuf {
    let harness_dir = harness
        .mailbox_path()
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| harness.config().project_path.join(".harness"));

    crate::team::worker_heartbeat_path(&harness_dir, worker_name)
}

/// Write the current time to the heartbeat file.
///
/// A file, not a message: a heartbeat that piled up in the mailbox would grow
/// without bound whenever the lead was not reading.
fn touch_heartbeat(path: &PathBuf) {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    let _ = std::fs::write(path, now.to_string());
}
