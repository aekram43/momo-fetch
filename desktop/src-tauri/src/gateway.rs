//! **T2** — spawn and supervise the `momo-fetch --gateway` child process.
//!
//! This is the load-bearing part of the desktop shell. Everything the UI can do
//! goes through a gateway that this module started, and the failure modes are
//! genuinely platform-divergent — see [`Supervisor::shutdown`].

use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Output lines kept for the crash screen. Enough to see a failure and its
/// cause, not so many that a chatty gateway pins memory.
///
/// Both streams feed this. The harness prints startup failures — a missing API
/// key, an unreadable config — to **stdout**, so watching stderr alone leaves
/// the crash screen blank for exactly the errors a user can actually fix.
const OUTPUT_TAIL: usize = 50;

/// How long to wait for the listening line before giving up on the child.
const LISTEN_TIMEOUT: Duration = Duration::from_secs(30);

/// Grace period between asking a process to stop and killing it.
const SHUTDOWN_GRACE: Duration = Duration::from_secs(5);

#[derive(Debug, thiserror::Error)]
pub enum GatewayError {
    #[error("could not start the gateway: {0}")]
    Spawn(#[from] std::io::Error),
    #[error("the gateway did not report a listening address within {0:?}")]
    Timeout(Duration),
    #[error("the gateway exited before it was ready:\n{0}")]
    ExitedEarly(String),
}

pub struct Supervisor {
    child: Option<Child>,
    /// The URL parsed from the child's `MOMO_GATEWAY_LISTENING` line.
    pub url: String,
    output_tail: Arc<Mutex<Vec<String>>>,
}

/// Append to a bounded tail buffer.
fn record(tail: &Arc<Mutex<Vec<String>>>, line: String) {
    if let Ok(mut t) = tail.lock() {
        t.push(line);
        if t.len() > OUTPUT_TAIL {
            t.remove(0);
        }
    }
}

impl Supervisor {
    /// Start the gateway on an OS-assigned port and wait until it is listening.
    ///
    /// **Port 0, not a pre-picked port.** Choosing a free port in the parent and
    /// passing it down races: between the probe and the child's `bind`, anything
    /// else on the machine can take it. `--gateway-port 0` lets the kernel
    /// allocate, and the child reports what it got on a single machine-readable
    /// stdout line (G11). We parse that rather than guessing.
    pub fn start(binary: &PathBuf, project: &PathBuf) -> Result<Self, GatewayError> {
        let mut cmd = Command::new(binary);
        cmd.arg("--gateway")
            .arg("--gateway-port")
            .arg("0")
            .arg("--project")
            .arg(project)
            // Grant the webview's own origin. A Tauri webview is
            // `tauri://localhost` on macOS/iOS and `http://tauri.localhost` on
            // Windows/Android — both cross-origin to `http://127.0.0.1:<port>`,
            // so without this the app cannot call the gateway it just started
            // and every panel reports a load failure. Both are passed because
            // the flag is per-run and granting the other platform's origin
            // costs nothing.
            .arg("--gateway-allow-origin")
            .arg("tauri://localhost")
            .arg("--gateway-allow-origin")
            .arg("http://tauri.localhost")
            // A bundled macOS app is launched with cwd `/`, and the harness
            // creates `.harness/` relative to the working directory — so
            // without this the child dies instantly with
            // "Read-only file system (os error 30)". Always run it somewhere
            // writable, and never inherit the launcher's cwd.
            .current_dir(project)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        // On Unix, put the child in its own process group. Without this a
        // Ctrl-C in a terminal-launched app is delivered to the child too, and
        // it dies before our shutdown path runs.
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            unsafe {
                cmd.pre_exec(|| {
                    libc::setsid();
                    Ok(())
                });
            }
        }

        // On Windows, don't flash a console window for a GUI app.
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }

        let mut child = cmd.spawn()?;

        // Drain stderr on its own thread and keep a tail. This is not only for
        // the crash screen: a child whose pipe fills up blocks on write and
        // appears to hang, which looks exactly like a startup failure.
        let output_tail = Arc::new(Mutex::new(Vec::<String>::new()));
        if let Some(stderr) = child.stderr.take() {
            let tail = Arc::clone(&output_tail);
            std::thread::spawn(move || {
                for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                    record(&tail, line);
                }
            });
        }

        let url = Self::await_listening(&mut child, &output_tail)?;

        Ok(Self {
            child: Some(child),
            url,
            output_tail,
        })
    }

    /// Block until the child prints `MOMO_GATEWAY_LISTENING <url>`.
    fn await_listening(
        child: &mut Child,
        output_tail: &Arc<Mutex<Vec<String>>>,
    ) -> Result<String, GatewayError> {
        let stdout = child.stdout.take().expect("stdout was piped");
        let (tx, rx) = std::sync::mpsc::channel::<String>();
        let tail = Arc::clone(output_tail);

        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if let Some(url) = line.strip_prefix("MOMO_GATEWAY_LISTENING ") {
                    let _ = tx.send(url.trim().to_string());
                    // Keep draining afterwards so the child never blocks on a
                    // full stdout pipe.
                    continue;
                }
                // Recorded, not just logged: startup failures come out here.
                log::debug!("gateway: {line}");
                record(&tail, line);
            }
        });

        let deadline = Instant::now() + LISTEN_TIMEOUT;
        loop {
            // A child that died will never print the line; noticing that beats
            // waiting out the full timeout with a useless message.
            if let Ok(Some(status)) = child.try_wait() {
                // The reader threads may still be draining the last lines, and
                // the useful message is usually the final one. Give them a beat
                // before reporting, or the crash screen is blank.
                std::thread::sleep(Duration::from_millis(150));
                let tail = output_tail.lock().map(|t| t.join("\n")).unwrap_or_default();
                let detail = if tail.trim().is_empty() {
                    format!("exit status {status}")
                } else {
                    tail
                };
                return Err(GatewayError::ExitedEarly(detail));
            }

            match rx.recv_timeout(Duration::from_millis(200)) {
                Ok(url) => return Ok(url),
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                    if Instant::now() > deadline {
                        return Err(GatewayError::Timeout(LISTEN_TIMEOUT));
                    }
                }
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                    std::thread::sleep(Duration::from_millis(150));
                    let tail = output_tail.lock().map(|t| t.join("\n")).unwrap_or_default();
                    return Err(GatewayError::ExitedEarly(tail));
                }
            }
        }
    }

    /// The last lines the gateway wrote, for the crash screen.
    pub fn stderr_tail(&self) -> String {
        self.output_tail
            .lock()
            .map(|t| t.join("\n"))
            .unwrap_or_default()
    }

    /// Whether the child is still running.
    pub fn is_alive(&mut self) -> bool {
        match self.child.as_mut() {
            Some(c) => matches!(c.try_wait(), Ok(None)),
            None => false,
        }
    }

    /// Stop the gateway: ask nicely, then insist.
    ///
    /// **Windows has no SIGTERM.** `Child::kill` maps to `TerminateProcess`,
    /// which is an immediate hard kill — there is no polite variant to try
    /// first, so the graceful branch simply does not exist there and we go
    /// straight to the kill. On Unix we send `SIGTERM`, give the gateway
    /// [`SHUTDOWN_GRACE`] to flush cost records and shut down MCP servers, then
    /// `SIGKILL` whatever is left.
    ///
    /// Getting this wrong leaks a gateway on every quit — it holds the session
    /// DB and an ephemeral port, and the next launch fights it.
    pub fn shutdown(&mut self) {
        let Some(mut child) = self.child.take() else {
            return;
        };

        #[cfg(unix)]
        {
            // SIGTERM to the process *group* (see `setsid` above), so MCP
            // servers the gateway spawned go down with it rather than being
            // reparented to init and surviving.
            let pid = child.id() as i32;
            unsafe {
                libc::kill(-pid, libc::SIGTERM);
                libc::kill(pid, libc::SIGTERM);
            }

            let deadline = Instant::now() + SHUTDOWN_GRACE;
            while Instant::now() < deadline {
                if matches!(child.try_wait(), Ok(Some(_))) {
                    return;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            log::warn!("gateway ignored SIGTERM after {SHUTDOWN_GRACE:?}; killing");
        }

        let _ = child.kill();
        let _ = child.wait();
    }
}

impl Drop for Supervisor {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// Poll `/health` until the gateway answers.
///
/// The listening line means the socket is bound; it does not mean the harness
/// finished building (providers, MCP servers, the memory vault). `/health` is
/// auth-exempt by design (G12) precisely so this poll works before the shell has
/// a token.
pub fn await_ready(url: &str, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    let endpoint = format!("{url}/health");
    let mut backoff = Duration::from_millis(50);

    while Instant::now() < deadline {
        if let Ok(resp) = ureq::get(&endpoint).timeout(Duration::from_secs(2)).call() {
            if resp.status() == 200 {
                return true;
            }
        }
        std::thread::sleep(backoff);
        // Back off to 1s: a cold start compiles nothing but does open a SQLite
        // DB and may start MCP servers.
        backoff = (backoff * 2).min(Duration::from_secs(1));
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn listening_line_is_parsed_exactly() {
        // The format is a contract with the Rust side (G11). If this parse ever
        // needs loosening, fix the producer instead.
        let line = "MOMO_GATEWAY_LISTENING http://127.0.0.1:51686";
        assert_eq!(
            line.strip_prefix("MOMO_GATEWAY_LISTENING ").map(str::trim),
            Some("http://127.0.0.1:51686")
        );
    }

    #[test]
    fn unrelated_stdout_is_not_mistaken_for_the_listening_line() {
        for line in [
            "listening on http://127.0.0.1:3000",
            "MOMO_GATEWAY_LISTENING_EXTRA http://x",
            "",
        ] {
            assert!(line.strip_prefix("MOMO_GATEWAY_LISTENING ").is_none());
        }
    }
}
