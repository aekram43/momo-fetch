//! Recover the user's real `PATH` when the app is launched from the GUI.
//!
//! macOS hands a Finder/Dock-launched app the launchd default
//! `/usr/bin:/bin:/usr/sbin:/sbin` — the login shell is never involved, so
//! nothing installed by nvm, Homebrew, Volta, asdf or cargo is on it. The
//! gateway inherits that `PATH` and passes it on to every stdio MCP server it
//! spawns, which is why an `npx`-based server dies with
//! `command 'npx' not found` and F14 paints it red, while the same config works
//! when the harness is started from a terminal. It reads as a desktop-only MCP
//! bug; it is an environment bug one process earlier.
//!
//! The fix is the one editors have converged on: ask the login shell what
//! `PATH` it would have, once, and hand that to the gateway.

use std::ffi::OsString;
use std::sync::OnceLock;

#[cfg(unix)]
use std::io::Read;
#[cfg(unix)]
use std::process::{Command, Stdio};
#[cfg(unix)]
use std::sync::mpsc;
#[cfg(unix)]
use std::time::Duration;

/// Printed immediately before the shell's `PATH` so rc-file chatter — nvm
/// notices, oh-my-zsh banners, a motd — can never be mistaken for it.
#[cfg(unix)]
const MARKER: &str = "__MOMO_PATH__";

/// How long the login shell gets to answer. rc files can be slow (conda, nvm)
/// and can hang outright (a `read`, a prompt for a passphrase); this bounds
/// app startup either way, since a missed probe only costs us the recovery.
#[cfg(unix)]
const PROBE_TIMEOUT: Duration = Duration::from_secs(3);

/// The `PATH` to give the gateway, or `None` to inherit ours unchanged.
///
/// Resolved once per process: the probe runs a login shell, and the answer
/// cannot change while the app is running.
pub fn for_gateway() -> Option<OsString> {
    static RESOLVED: OnceLock<Option<OsString>> = OnceLock::new();
    RESOLVED.get_or_init(resolve).clone()
}

/// Windows has no login-shell `PATH` to recover — a GUI process there inherits
/// the same user/system `PATH` a console one does.
#[cfg(not(unix))]
fn resolve() -> Option<OsString> {
    None
}

#[cfg(unix)]
fn resolve() -> Option<OsString> {
    let current = std::env::var("PATH").unwrap_or_default();

    // Launched from a terminal (or by `cargo tauri dev`): the PATH we already
    // have *is* the user's, and spawning an interactive shell to confirm it
    // would put a few hundred milliseconds on every developer's startup.
    if !looks_inherited_from_launcher(&current) {
        return None;
    }

    let shell = std::env::var("SHELL").ok().filter(|s| !s.is_empty())?;
    let from_shell = probe(&shell)?;
    let merged = merge(&from_shell, &current);

    log::info!("recovered PATH from {shell}: {merged}");
    Some(OsString::from(merged))
}

/// Ask a login shell for its `PATH`.
///
/// `-ilc` — interactive *and* login — on purpose: users put their version
/// managers in `.zshrc`/`.bashrc` (interactive) at least as often as in
/// `.zprofile` (login), and a probe that reads only one of them misses half of
/// them.
#[cfg(unix)]
fn probe(shell: &str) -> Option<String> {
    let script = format!("printf '%s%s' '{MARKER}' \"$PATH\"");

    let mut child = match Command::new(shell)
        .args(["-ilc", &script])
        // No tty here. Without this an interactive shell can block reading
        // stdin instead of running the command and exiting.
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        // Interactive shells warn about job control on a non-tty. Not our
        // problem, and not worth putting in the log.
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(child) => child,
        Err(e) => {
            log::warn!("could not run {shell} to recover PATH: {e}");
            return None;
        }
    };

    // Read on a thread so a shell that never exits cannot wedge startup.
    let mut stdout = child.stdout.take()?;
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut buf = String::new();
        let _ = stdout.read_to_string(&mut buf);
        let _ = tx.send(buf);
    });

    let output = match rx.recv_timeout(PROBE_TIMEOUT) {
        Ok(output) => {
            let _ = child.wait();
            output
        }
        Err(_) => {
            log::warn!("{shell} did not report its PATH within {PROBE_TIMEOUT:?}");
            let _ = child.kill();
            let _ = child.wait();
            return None;
        }
    };

    let path = extract(&output);
    if path.is_none() {
        log::warn!("{shell} produced no usable PATH; keeping the inherited one");
    }
    path
}

/// Pull the `PATH` out of the probe's output, ignoring everything a shell
/// startup file may have printed before it.
#[cfg(unix)]
fn extract(output: &str) -> Option<String> {
    let after = output.rsplit_once(MARKER)?.1;
    // Nothing should follow — `printf` is the last thing the shell runs — but a
    // background job that prints after us would otherwise end up inside PATH.
    let path = after.split('\n').next().unwrap_or("").trim();
    (!path.is_empty()).then(|| path.to_string())
}

/// Whether this `PATH` looks like one no shell ever touched.
///
/// The launchd default is exactly the four system directories; anything a
/// profile has run over picks up at least one entry outside them. `/usr/local`
/// is counted as system so a GUI-launched Linux build is recovered too.
#[cfg(unix)]
fn looks_inherited_from_launcher(path: &str) -> bool {
    const SYSTEM: [&str; 6] = [
        "/usr/bin",
        "/bin",
        "/usr/sbin",
        "/sbin",
        "/usr/local/bin",
        "/usr/local/sbin",
    ];

    if path.trim().is_empty() {
        return true;
    }

    std::env::split_paths(path).all(|entry| {
        let entry = entry.to_string_lossy();
        let entry = entry.trim_end_matches('/');
        SYSTEM.contains(&entry)
            || entry.starts_with("/System/")
            || entry.starts_with("/var/run/com.apple")
    })
}

/// Combine the shell's `PATH` with ours, shell first.
///
/// Order is the whole point: a user with nvm expects *their* node to win over
/// `/usr/bin/node`, and that is only true if the shell's entries come first.
/// Ours are kept on the tail rather than dropped — losing a system directory
/// would break far more than it fixed.
#[cfg(unix)]
fn merge(from_shell: &str, current: &str) -> String {
    let mut entries: Vec<String> = Vec::new();

    for source in [from_shell, current] {
        for entry in source.split(':') {
            let entry = entry.trim();
            if entry.is_empty() || entries.iter().any(|e| e == entry) {
                continue;
            }
            entries.push(entry.to_string());
        }
    }

    entries.join(":")
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn launchd_default_is_recognised() {
        const LAUNCHD_DEFAULT: &str = "/usr/bin:/bin:/usr/sbin:/sbin";
        assert!(looks_inherited_from_launcher(LAUNCHD_DEFAULT));
    }

    #[test]
    fn empty_path_is_recognised() {
        // Nothing to lose and everything to gain: probe.
        assert!(looks_inherited_from_launcher(""));
    }

    #[test]
    fn cryptex_and_system_entries_do_not_count_as_a_shell_touching_it() {
        // A stock macOS GUI launch carries these; they are not evidence of a
        // profile having run.
        assert!(looks_inherited_from_launcher(
            "/usr/bin:/bin:/System/Cryptexes/App/usr/bin:/var/run/com.apple.security.cryptexd/codex.system/bootstrap/usr/bin"
        ));
    }

    #[test]
    fn a_shell_path_is_left_alone() {
        // One entry outside the system set is enough — this PATH came from a
        // profile, and re-deriving it would only cost startup time.
        assert!(!looks_inherited_from_launcher(
            "/Users/x/.nvm/versions/node/v23.11.0/bin:/usr/bin:/bin"
        ));
        assert!(!looks_inherited_from_launcher("/opt/homebrew/bin:/usr/bin"));
    }

    #[test]
    fn the_marker_survives_a_chatty_rc_file() {
        let output = format!(
            "Now using node v23.11.0\n{MARKER}/Users/x/.nvm/versions/node/v23.11.0/bin:/usr/bin"
        );
        assert_eq!(
            extract(&output).as_deref(),
            Some("/Users/x/.nvm/versions/node/v23.11.0/bin:/usr/bin")
        );
    }

    #[test]
    fn output_after_the_path_is_not_swallowed() {
        let output = format!("{MARKER}/usr/bin:/bin\n[1]+ Done  something");
        assert_eq!(extract(&output).as_deref(), Some("/usr/bin:/bin"));
    }

    #[test]
    fn a_shell_that_said_nothing_useful_is_rejected() {
        assert_eq!(extract("bash: no job control in this shell"), None);
        assert_eq!(extract(&format!("{MARKER}   ")), None);
    }

    #[test]
    fn merge_puts_the_shell_first_and_keeps_ours() {
        let merged = merge(
            "/Users/x/.nvm/versions/node/v23.11.0/bin:/opt/homebrew/bin:/usr/bin:/bin",
            "/usr/bin:/bin:/usr/sbin:/sbin",
        );
        assert_eq!(
            merged,
            "/Users/x/.nvm/versions/node/v23.11.0/bin:/opt/homebrew/bin:/usr/bin:/bin:/usr/sbin:/sbin"
        );
    }

    #[test]
    fn merge_does_not_repeat_an_entry() {
        assert_eq!(merge("/a:/b:/a", "/b:/c"), "/a:/b:/c");
    }

    #[test]
    fn merge_drops_empty_segments() {
        // A trailing colon means "the current directory" to some tools. Never
        // pass that on to a process that spawns MCP servers.
        assert_eq!(merge("/a::/b:", "/c"), "/a:/b:/c");
    }
}
