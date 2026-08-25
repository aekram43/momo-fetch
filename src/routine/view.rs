//! JSON shapes for routines, shared by `momo-fetch routine …` and `/v2/routines`.
//!
//! One place builds these documents so the CLI an agent drives through
//! `shell_exec` and the panel a human clicks in can never describe the same
//! routine differently.

use std::path::Path;

use serde_json::{Value, json};

use super::{Routine, RoutineService, RunRecord, RunStatus, TickReport, format_timestamp};

/// Everything the UI and the CLI show about one routine, including the
/// schedule state that lives outside the definition.
pub fn routine_payload(service: &RoutineService, routine: &Routine) -> Value {
    let runtime = service.runtime(&routine.id);
    let last_run = service
        .runs(Some(&routine.id), 1)
        .first()
        .map(|r| run_payload(r));

    json!({
        "id": routine.id,
        "name": routine.name,
        "enabled": routine.enabled,
        "assignee": routine.assignee.to_string(),
        "assignee_detail": routine.assignee,
        "trigger": routine.trigger,
        "trigger_summary": routine.trigger.summary(),
        "concurrency": routine.concurrency,
        "task": routine.task,
        "permission": routine.permission,
        "created_at": iso(routine.created_at),
        "updated_at": iso(routine.updated_at),
        "next_due": service.next_due(routine).map(format_timestamp),
        "last_fired_at": runtime.last_fired_at.map(format_timestamp),
        "last_run_at": runtime.last_run_at.map(format_timestamp),
        "queued": runtime.queue.len(),
        "fired": runtime.fired,
        "skipped": runtime.skipped,
        "active_runs": service.active_runs(&routine.id),
        "last_run": last_run,
    })
}

/// One firing.
pub fn run_payload(run: &RunRecord) -> Value {
    // The reason is lifted out of the status so a caller can read it without
    // knowing how the enum is tagged.
    let reason = match &run.status {
        RunStatus::Failed { reason } | RunStatus::Skipped { reason } => Some(reason.clone()),
        _ => None,
    };

    json!({
        "id": run.id,
        "routine_id": run.routine_id,
        "routine_name": run.routine_name,
        "assignee": run.assignee.to_string(),
        "status": run.status.label(),
        "reason": reason,
        "scheduled_at": iso(run.scheduled_at),
        "started_at": iso(run.started_at),
        "finished_at": run.finished_at.map(format_timestamp),
        "pid": run.pid,
        "exit_code": run.exit_code,
        "log_path": run.log_path,
        "source": run.source,
    })
}

/// What one scheduler step did.
pub fn tick_payload(report: &TickReport) -> Value {
    json!({
        "status": "ticked",
        "now": iso(report.now),
        "fired": report.fired.iter().map(run_payload).collect::<Vec<_>>(),
        "finished": report.finished.iter().map(run_payload).collect::<Vec<_>>(),
        "queued": report.queued.iter().map(|(id, at)| json!({
            "routine_id": id,
            "scheduled_at": iso(*at),
        })).collect::<Vec<_>>(),
        "skipped": report.skipped.iter().map(|(id, reason)| json!({
            "routine_id": id,
            "reason": reason,
        })).collect::<Vec<_>>(),
    })
}

/// Who a routine can be assigned to in this project, right now.
///
/// Workers appear only while a team is running and only if they are standby —
/// a one-shot worker has already exited by the time anything could post to it,
/// so offering it would be offering a dead letter box.
pub fn assignees_payload(project_path: &Path) -> Value {
    let mut out = vec![json!({
        "value": "lead",
        "kind": "lead",
        "label": "lead (default agent)",
        "available": true,
    })];

    if let Ok(registry) = crate::agent::AgentRegistry::new(project_path) {
        for def in registry.list() {
            out.push(json!({
                "value": format!("agent:{}", def.name),
                "kind": "agent",
                "label": def.name,
                "description": def.description,
                "available": true,
            }));
        }
    }

    if let Ok(mut team) = crate::team::TeamService::new(project_path) {
        team.status();
        if let Some(state) = team.state() {
            let mut names: Vec<&String> = state.workers.keys().collect();
            names.sort();
            for name in names {
                let worker = &state.workers[name];
                if worker.mode != crate::team::WorkerMode::Standby {
                    continue;
                }
                out.push(json!({
                    "value": format!("worker:{name}"),
                    "kind": "worker",
                    "label": format!("{name} (standby)"),
                    "status": worker.status.to_string(),
                    "available": !worker.status.is_terminal(),
                }));
            }
        }
    }

    json!({ "assignees": out })
}

fn iso(ms: u64) -> Value {
    if ms == 0 {
        Value::Null
    } else {
        Value::String(format_timestamp(ms))
    }
}
