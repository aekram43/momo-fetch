//! Spawning the process one routine firing runs in.
//!
//! Deliberately the same shape team workers use — a shell line, a log, and an
//! exit-code file written the instant the process ends — because the exit file
//! is the only thing that reliably distinguishes "finished" from "died". A pid
//! that is gone tells you the process left; it does not tell you whether it got
//! there on purpose.
//!
//! The run is detached rather than a child of whoever fired it. A tick from the
//! CLI lasts milliseconds and a gateway tick loop must never accumulate
//! unreaped children, so the script is backgrounded behind `nohup` and the
//! immediate `sh` is waited for and reaped straight away.

use std::path::{Path, PathBuf};
use std::process::Command;

pub struct SpawnRequest<'a> {
    /// Working directory for the run — the project root.
    pub project_path: &'a Path,
    /// The momo-fetch binary to re-invoke.
    pub binary: &'a Path,
    /// `-a <name>` for a specialist assignee; `None` runs the default agent.
    pub agent: Option<&'a str>,
    /// The routine this run belongs to, so the session it creates is filed as
    /// scheduled work rather than as somebody's chat.
    pub routine_name: &'a str,
    pub permission: &'a str,
    pub prompt: &'a str,
    /// Where the script, log and exit file go.
    pub run_dir: &'a Path,
    pub run_id: &'a str,
}

/// Only what the caller records. The exit file and the script are derived from
/// the run id whenever they are needed ([`exit_path`], [`script_path`]), so
/// carrying them here would be two sources for one path.
pub struct Spawned {
    pub pid: u32,
    pub log_path: PathBuf,
}

/// Start a run and return as soon as it is up.
pub fn spawn(req: SpawnRequest<'_>) -> anyhow::Result<Spawned> {
    std::fs::create_dir_all(req.run_dir)?;

    let script_path = script_path(req.run_dir, req.run_id);
    let log_path = log_path(req.run_dir, req.run_id);
    let exit_path = exit_path(req.run_dir, req.run_id);
    let pid_path = req.run_dir.join(format!("{}.pid", req.run_id));

    // A stale exit file from a reused id would settle the run before it began.
    let _ = std::fs::remove_file(&exit_path);
    let _ = std::fs::remove_file(&pid_path);

    std::fs::write(&script_path, build_script(&req, &exit_path))?;

    // The script is passed to `sh` by path, so nothing about the prompt is ever
    // nested inside a second layer of shell quoting.
    let launcher = format!(
        "nohup sh '{}' > '{}' 2>&1 & echo $! > '{}'",
        shell_quote(&script_path.display().to_string()),
        shell_quote(&log_path.display().to_string()),
        shell_quote(&pid_path.display().to_string()),
    );

    let status = Command::new("sh")
        .arg("-c")
        .arg(&launcher)
        .current_dir(req.project_path)
        .status()?;
    if !status.success() {
        anyhow::bail!("Could not start the run (shell exited {status}).");
    }

    let pid = std::fs::read_to_string(&pid_path)
        .ok()
        .and_then(|s| s.trim().parse::<u32>().ok())
        .ok_or_else(|| anyhow::anyhow!("The run started but reported no pid."))?;

    Ok(Spawned { pid, log_path })
}

/// The script one run executes.
///
/// `cd || exit 127` rather than a bare `cd`: a project directory that moved
/// would otherwise run the whole task in whatever directory the scheduler
/// happened to be in.
fn build_script(req: &SpawnRequest<'_>, exit_path: &Path) -> String {
    let agent_flag = match req.agent {
        Some(name) => format!(" -a '{}'", shell_quote(name)),
        None => String::new(),
    };
    let origin_flag = format!(" --origin 'routine:{}'", shell_quote(req.routine_name));
    format!(
        "#!/bin/sh\n\
         # momo-fetch routine run {run_id} — safe to delete.\n\
         cd '{project}' || exit 127\n\
         '{binary}'{agent_flag}{origin_flag} --permission {permission} -p '{prompt}'\n\
         echo $? > '{exit}'\n",
        run_id = req.run_id,
        project = shell_quote(&req.project_path.display().to_string()),
        binary = shell_quote(&req.binary.display().to_string()),
        agent_flag = agent_flag,
        origin_flag = origin_flag,
        permission = req.permission,
        prompt = shell_quote(req.prompt),
        exit = shell_quote(&exit_path.display().to_string()),
    )
}

pub fn script_path(run_dir: &Path, run_id: &str) -> PathBuf {
    run_dir.join(format!("{run_id}.sh"))
}

pub fn log_path(run_dir: &Path, run_id: &str) -> PathBuf {
    run_dir.join(format!("{run_id}.log"))
}

pub fn exit_path(run_dir: &Path, run_id: &str) -> PathBuf {
    run_dir.join(format!("{run_id}.exit"))
}

/// The exit code a finished run wrote, if it wrote one.
pub fn read_exit_code(path: &Path) -> Option<i32> {
    std::fs::read_to_string(path).ok()?.trim().parse().ok()
}

/// The momo-fetch binary currently executing.
///
/// `current_exe` rather than a `PATH` lookup so a run uses the same build that
/// scheduled it — on a machine with an older `momo-fetch` on `PATH` those are
/// not the same program.
pub fn momo_binary() -> PathBuf {
    std::env::current_exe().unwrap_or_else(|_| PathBuf::from("momo-fetch"))
}

/// Whether a pid is still running.
///
/// `EPERM` counts as alive: the process exists, it just is not ours to signal.
/// Treating it as dead would mark a healthy run failed.
#[cfg(unix)]
pub fn process_alive(pid: u32) -> bool {
    // SAFETY: signal 0 performs error checking only — it delivers nothing.
    let rc = unsafe { libc::kill(pid as libc::pid_t, 0) };
    if rc == 0 {
        return true;
    }
    std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

/// Non-unix has no cheap liveness probe here, and "assume alive" is the safe
/// wrong answer: a run stays `running` until its exit file lands, rather than
/// being reported as a crash that never happened.
#[cfg(not(unix))]
pub fn process_alive(_pid: u32) -> bool {
    true
}

fn shell_quote(value: &str) -> String {
    value.replace('\'', "'\\''")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req<'a>(run_dir: &'a Path, prompt: &'a str, agent: Option<&'a str>) -> SpawnRequest<'a> {
        SpawnRequest {
            project_path: Path::new("/tmp/project"),
            binary: Path::new("/usr/local/bin/momo-fetch"),
            agent,
            routine_name: "Nightly digest",
            permission: "auto",
            prompt,
            run_dir,
            run_id: "run-abc",
        }
    }

    #[test]
    fn script_carries_permission_and_writes_an_exit_code() {
        let dir = Path::new("/tmp/runs");
        let script = build_script(&req(dir, "do the thing", None), &exit_path(dir, "run-abc"));
        assert!(script.contains("--permission auto"), "{script}");
        assert!(script.contains("-p 'do the thing'"), "{script}");
        assert!(script.contains("echo $? > '/tmp/runs/run-abc.exit'"), "{script}");
        // No agent flag when the assignee is the default agent.
        assert!(!script.contains(" -a "), "{script}");
    }

    #[test]
    fn the_run_says_which_routine_it_is_so_its_session_is_not_a_mystery_chat() {
        let dir = Path::new("/tmp/runs");
        let script = build_script(&req(dir, "do the thing", None), &exit_path(dir, "run-abc"));
        assert!(script.contains("--origin 'routine:Nightly digest'"), "{script}");
    }

    #[test]
    fn a_routine_name_with_a_quote_cannot_break_out_of_the_command() {
        let dir = Path::new("/tmp/runs");
        let mut request = req(dir, "x", None);
        request.routine_name = "Aek's sweep";
        let script = build_script(&request, &exit_path(dir, "run-abc"));
        assert!(script.contains(r"--origin 'routine:Aek'\''s sweep'"), "{script}");
    }

    #[test]
    fn a_specialist_assignee_gets_the_agent_flag() {
        let dir = Path::new("/tmp/runs");
        let script = build_script(
            &req(dir, "sweep the backlog", Some("planner")),
            &exit_path(dir, "run-abc"),
        );
        assert!(script.contains(" -a 'planner'"), "{script}");
    }

    #[test]
    fn quotes_in_a_prompt_cannot_break_out_of_the_command() {
        let dir = Path::new("/tmp/runs");
        let script = build_script(
            &req(dir, "it's fine; rm -rf /", None),
            &exit_path(dir, "run-abc"),
        );
        assert!(script.contains(r"-p 'it'\''s fine; rm -rf /'"), "{script}");
    }

    #[test]
    fn a_missing_project_directory_stops_the_run_rather_than_relocating_it() {
        let dir = Path::new("/tmp/runs");
        let script = build_script(&req(dir, "x", None), &exit_path(dir, "run-abc"));
        assert!(script.contains("|| exit 127"), "{script}");
    }

    /// End to end against `/bin/echo` — a real detached process, no tmux, no
    /// network, no model. It proves the pid comes back and the exit file lands.
    #[test]
    fn spawn_detaches_and_records_an_exit_code() {
        let tmp = tempfile::tempdir().unwrap();
        let run_dir = tmp.path().join("runs");
        let spawned = spawn(SpawnRequest {
            project_path: tmp.path(),
            binary: Path::new("/bin/echo"),
            agent: None,
            routine_name: "probe",
            permission: "auto",
            prompt: "hello",
            run_dir: &run_dir,
            run_id: "run-test",
        })
        .expect("spawn");

        assert!(spawned.pid > 0);
        let exit = exit_path(&run_dir, "run-test");
        for _ in 0..100 {
            if read_exit_code(&exit) == Some(0) {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        panic!(
            "no exit code after 5s; log: {}",
            std::fs::read_to_string(&spawned.log_path).unwrap_or_default()
        );
    }

    #[test]
    fn the_current_process_is_alive() {
        assert!(process_alive(std::process::id()));
    }
}
