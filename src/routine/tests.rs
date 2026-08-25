//! Scheduler tests.
//!
//! Nothing here starts a process, a team or a tmux server — the scheduling
//! rules are all in [`decide`], which is pure, and the store is a temp
//! directory. CI has no tmux and no model key; a test that needed either would
//! be a test nobody runs.

use super::*;

const T0: u64 = 1_767_225_600_000; // 2026-01-01T00:00:00Z
const HOUR: u64 = 3_600_000;
const MINUTE: u64 = 60_000;

fn hourly() -> Trigger {
    Trigger::Cron {
        expression: "0 * * * *".to_string(),
        timezone: "UTC".to_string(),
        catch_up: CatchUp::Skip,
    }
}

fn routine(trigger: Trigger) -> Routine {
    Routine {
        id: "rt-test".to_string(),
        name: "Nightly digest".to_string(),
        enabled: true,
        assignee: Assignee::Lead,
        trigger,
        concurrency: Concurrency::Queue,
        task: TaskTemplate {
            title: "Summarise the day".to_string(),
            priority: Priority::Medium,
            description: "Read the transcripts and write a digest.".to_string(),
        },
        permission: DEFAULT_ROUTINE_PERMISSION.to_string(),
        created_at: T0,
        updated_at: T0,
    }
}

// ─── decide ────────────────────────────────────────────────────────

#[test]
fn nothing_fires_before_the_window() {
    let r = routine(hourly());
    let (d, rt) = decide(&r, &RoutineRuntime::default(), T0 + 30 * MINUTE, 0);
    assert_eq!(d, Decision::Idle);
    assert_eq!(rt, RoutineRuntime::default());
}

#[test]
fn the_window_fires_once_and_advances_the_anchor() {
    let r = routine(hourly());
    let (d, rt) = decide(&r, &RoutineRuntime::default(), T0 + HOUR, 0);
    assert_eq!(d, Decision::Fire { scheduled: T0 + HOUR });
    assert_eq!(rt.last_fired_at, Some(T0 + HOUR));
    assert_eq!(rt.fired, 1);

    // Same instant, second tick: the anchor moved, so nothing is due again.
    let (d, _) = decide(&r, &rt, T0 + HOUR, 0);
    assert_eq!(d, Decision::Idle);
}

#[test]
fn a_disabled_routine_never_fires() {
    let mut r = routine(hourly());
    r.enabled = false;
    let (d, _) = decide(&r, &RoutineRuntime::default(), T0 + 10 * HOUR, 0);
    assert_eq!(d, Decision::Idle);
}

#[test]
fn manual_triggers_never_come_due() {
    let r = routine(Trigger::Manual);
    let (d, _) = decide(&r, &RoutineRuntime::default(), T0 + 10 * HOUR, 0);
    assert_eq!(d, Decision::Idle);
}

#[test]
fn an_interval_trigger_fires_once_the_interval_has_passed() {
    let r = routine(Trigger::Every { seconds: 300 });
    let (d, _) = decide(&r, &RoutineRuntime::default(), T0 + 4 * MINUTE, 0);
    assert_eq!(d, Decision::Idle);
    let (d, rt) = decide(&r, &RoutineRuntime::default(), T0 + 5 * MINUTE, 0);
    assert_eq!(d, Decision::Fire { scheduled: T0 + 5 * MINUTE });
    assert_eq!(rt.last_fired_at, Some(T0 + 5 * MINUTE));
}

#[test]
fn an_interval_that_elapsed_while_down_is_due_immediately() {
    // A heartbeat has no "the moment passed" reading — coming back after six
    // hours means the check is overdue, not cancelled.
    let r = routine(Trigger::Every { seconds: 300 });
    let (d, rt) = decide(&r, &RoutineRuntime::default(), T0 + 6 * HOUR, 0);
    assert_eq!(d, Decision::Fire { scheduled: T0 + 5 * MINUTE });
    assert_eq!(rt.skipped, 0);
}

#[test]
fn skip_missed_windows_lands_on_the_one_that_just_came_due() {
    let r = routine(hourly());
    let (d, rt) = decide(&r, &RoutineRuntime::default(), T0 + 5 * HOUR + MINUTE, 0);
    // 01:00–04:00 are gone; 05:00 is a minute old, which is inside the grace.
    assert_eq!(d, Decision::Fire { scheduled: T0 + 5 * HOUR });
    assert_eq!(rt.skipped, 4);
}

#[test]
fn skip_missed_windows_waits_when_even_the_last_one_is_stale() {
    let r = routine(hourly());
    let now = T0 + 5 * HOUR + 30 * MINUTE;
    let (d, rt) = decide(&r, &RoutineRuntime::default(), now, 0);
    assert_eq!(d, Decision::Idle);
    assert_eq!(rt.skipped, 5);
    assert_eq!(rt.last_fired_at, Some(T0 + 5 * HOUR));
}

#[test]
fn run_once_catches_up_on_the_first_missed_window() {
    let r = routine(Trigger::Cron {
        expression: "0 * * * *".to_string(),
        timezone: "UTC".to_string(),
        catch_up: CatchUp::RunOnce,
    });
    let (d, rt) = decide(&r, &RoutineRuntime::default(), T0 + 5 * HOUR + 30 * MINUTE, 0);
    assert_eq!(d, Decision::Fire { scheduled: T0 + HOUR });
    assert_eq!(rt.skipped, 0);
}

#[test]
fn skip_concurrency_drops_the_window_while_a_run_is_going() {
    let mut r = routine(hourly());
    r.concurrency = Concurrency::Skip;
    let (d, rt) = decide(&r, &RoutineRuntime::default(), T0 + HOUR, 1);
    assert!(matches!(d, Decision::Skip { scheduled, .. } if scheduled == T0 + HOUR));
    assert_eq!(rt.skipped, 1);
    assert_eq!(rt.fired, 0);
    // The anchor still moved: a skipped window is a window that happened.
    assert_eq!(rt.last_fired_at, Some(T0 + HOUR));
}

#[test]
fn parallel_concurrency_fires_alongside() {
    let mut r = routine(hourly());
    r.concurrency = Concurrency::Parallel;
    let (d, rt) = decide(&r, &RoutineRuntime::default(), T0 + HOUR, 3);
    assert_eq!(d, Decision::Fire { scheduled: T0 + HOUR });
    assert_eq!(rt.fired, 1);
}

#[test]
fn queue_holds_the_window_and_releases_it_in_order() {
    let r = routine(hourly());

    let (d, rt) = decide(&r, &RoutineRuntime::default(), T0 + HOUR, 1);
    assert_eq!(d, Decision::Queue { scheduled: T0 + HOUR });
    let (d, rt) = decide(&r, &rt, T0 + 2 * HOUR, 1);
    assert_eq!(d, Decision::Queue { scheduled: T0 + 2 * HOUR });
    assert_eq!(rt.queue, vec![T0 + HOUR, T0 + 2 * HOUR]);

    // The lane clears: the oldest held window goes first.
    let (d, rt) = decide(&r, &rt, T0 + 2 * HOUR + MINUTE, 0);
    assert_eq!(d, Decision::Fire { scheduled: T0 + HOUR });
    assert_eq!(rt.queue, vec![T0 + 2 * HOUR]);
}

#[test]
fn a_full_queue_drops_the_oldest_window_not_the_newest() {
    let r = routine(hourly());
    let mut rt = RoutineRuntime {
        queue: (0..MAX_QUEUE_DEPTH as u64).map(|i| T0 + i).collect(),
        last_fired_at: Some(T0),
        ..Default::default()
    };
    let before = rt.queue[1];
    let (d, next) = decide(&r, &rt, T0 + HOUR, 1);
    assert_eq!(d, Decision::Queue { scheduled: T0 + HOUR });
    assert_eq!(next.queue.len(), MAX_QUEUE_DEPTH);
    assert_eq!(next.queue[0], before);
    assert_eq!(*next.queue.last().unwrap(), T0 + HOUR);
    rt.queue.clear();
}

// ─── Values ────────────────────────────────────────────────────────

#[test]
fn assignees_round_trip_through_text() {
    for text in ["lead", "agent:planner", "worker:builder"] {
        let a: Assignee = text.parse().unwrap();
        assert_eq!(a.to_string(), text);
    }
    assert!("worker:".parse::<Assignee>().is_err());
    assert!("nobody".parse::<Assignee>().is_err());
    assert!("ghost:x".parse::<Assignee>().is_err());
}

#[test]
fn only_worker_assignees_skip_the_process_spawn() {
    assert!(Assignee::Lead.spawns_process());
    assert!(Assignee::Agent { name: "p".into() }.spawns_process());
    assert!(!Assignee::Worker { name: "b".into() }.spawns_process());
}

#[test]
fn priorities_round_trip() {
    for text in ["low", "medium", "high", "urgent"] {
        assert_eq!(text.parse::<Priority>().unwrap().to_string(), text);
    }
    assert!("whenever".parse::<Priority>().is_err());
}

#[test]
fn intervals_parse_with_and_without_a_unit() {
    assert_eq!(parse_duration("90").unwrap(), 90);
    assert_eq!(parse_duration("5m").unwrap(), 300);
    assert_eq!(parse_duration("2h").unwrap(), 7200);
    assert_eq!(parse_duration("1d").unwrap(), 86_400);
    assert!(parse_duration("soon").is_err());
    assert!(parse_duration("5y").is_err());
    assert_eq!(format_duration(90), "1m 30s");
    assert_eq!(format_duration(3600), "1h");
}

#[test]
fn a_too_fast_interval_is_refused() {
    let err = Trigger::Every { seconds: 5 }.validate().unwrap_err();
    assert!(err.contains("at least"), "{err}");
    assert!(Trigger::Every { seconds: MIN_INTERVAL_SECS }.validate().is_ok());
}

#[test]
fn a_bad_cron_expression_is_refused_when_it_is_written() {
    let t = Trigger::Cron {
        expression: "not a cron".to_string(),
        timezone: "UTC".to_string(),
        catch_up: CatchUp::Skip,
    };
    assert!(t.validate().is_err());

    let t = Trigger::Cron {
        expression: "0 * * * *".to_string(),
        timezone: "Middle/Earth".to_string(),
        catch_up: CatchUp::Skip,
    };
    assert!(t.validate().is_err());
}

#[test]
fn validation_names_the_field_that_is_missing() {
    let mut r = routine(hourly());
    r.task.description = "  ".to_string();
    let err = r.validate().unwrap_err();
    assert!(err.contains("description"), "{err}");

    let mut r = routine(hourly());
    r.permission = "reckless".to_string();
    assert!(r.validate().unwrap_err().contains("permission"));
}

#[test]
fn the_prompt_names_the_routine_the_window_and_the_priority() {
    let mut r = routine(hourly());
    r.task.priority = Priority::High;
    let prompt = build_prompt(&r, T0 + HOUR);
    assert!(prompt.contains("[Routine: Nightly digest]"), "{prompt}");
    assert!(prompt.contains("Summarise the day"), "{prompt}");
    assert!(prompt.contains("Priority: high"), "{prompt}");
    assert!(prompt.contains("2026-01-01T01:00:00Z"), "{prompt}");
    assert!(prompt.contains("Read the transcripts"), "{prompt}");
}

// ─── Store ─────────────────────────────────────────────────────────

fn service() -> (tempfile::TempDir, RoutineService) {
    let tmp = tempfile::tempdir().unwrap();
    let svc = RoutineService::new(tmp.path()).unwrap();
    (tmp, svc)
}

#[test]
fn a_created_routine_survives_a_reload() {
    let (tmp, mut svc) = service();
    let mut r = routine(hourly());
    r.id.clear();
    let created = svc.create(r).unwrap();
    assert!(created.id.starts_with("rt-"));

    let reopened = RoutineService::new(tmp.path()).unwrap();
    let found = reopened.get(&created.id).expect("routine on disk");
    assert_eq!(found.name, "Nightly digest");
    assert_eq!(found.assignee, Assignee::Lead);
}

#[test]
fn an_invalid_routine_is_never_written() {
    let (tmp, mut svc) = service();
    let mut r = routine(hourly());
    r.id.clear();
    r.task.title = String::new();
    assert!(svc.create(r).is_err());
    assert_eq!(RoutineService::new(tmp.path()).unwrap().list().len(), 0);
}

#[test]
fn routines_are_addressable_by_name_as_well_as_id() {
    let (_tmp, mut svc) = service();
    let mut r = routine(hourly());
    r.id.clear();
    let created = svc.create(r).unwrap();
    assert_eq!(svc.resolve("Nightly digest").unwrap().id, created.id);
    assert_eq!(svc.resolve(&created.id).unwrap().id, created.id);
    assert!(svc.resolve("no such thing").is_none());
}

#[test]
fn an_ambiguous_name_resolves_to_nothing_rather_than_to_a_guess() {
    let (_tmp, mut svc) = service();
    for _ in 0..2 {
        let mut r = routine(hourly());
        r.id.clear();
        svc.create(r).unwrap();
    }
    assert!(svc.resolve("Nightly digest").is_none());
}

#[test]
fn changing_the_trigger_resets_the_anchor() {
    let (_tmp, mut svc) = service();
    let mut r = routine(hourly());
    r.id.clear();
    let created = svc.create(r).unwrap();

    let mut updated = created.clone();
    updated.trigger = Trigger::Every { seconds: 600 };
    svc.update(&created.id, updated).unwrap();

    let rt = svc.runtime(&created.id);
    assert!(rt.last_fired_at.is_some(), "a new trigger anchors on now");
    assert!(rt.queue.is_empty());
}

#[test]
fn editing_the_task_leaves_the_schedule_alone() {
    let (_tmp, mut svc) = service();
    let mut r = routine(hourly());
    r.id.clear();
    let created = svc.create(r).unwrap();

    let mut updated = created.clone();
    updated.task.title = "Summarise the week".to_string();
    svc.update(&created.id, updated).unwrap();

    assert_eq!(svc.runtime(&created.id).last_fired_at, None);
    assert_eq!(svc.get(&created.id).unwrap().task.title, "Summarise the week");
}

#[test]
fn re_enabling_does_not_replay_the_time_it_was_off() {
    let (_tmp, mut svc) = service();
    let mut r = routine(hourly());
    r.id.clear();
    r.created_at = T0;
    let created = svc.create(r).unwrap();

    svc.set_enabled(&created.id, false).unwrap();
    svc.set_enabled(&created.id, true).unwrap();

    let rt = svc.runtime(&created.id);
    let anchor = rt.last_fired_at.expect("anchored on re-enable");
    assert!(anchor > T0, "the anchor moved forward to now");
}

#[test]
fn deleting_removes_the_definition_and_its_schedule_state() {
    let (tmp, mut svc) = service();
    let mut r = routine(hourly());
    r.id.clear();
    let created = svc.create(r).unwrap();

    assert!(svc.delete(&created.id).unwrap());
    assert!(!svc.delete(&created.id).unwrap());
    assert_eq!(RoutineService::new(tmp.path()).unwrap().list().len(), 0);
}

#[test]
fn a_manual_routine_with_no_team_records_the_failure_rather_than_panicking() {
    let (_tmp, mut svc) = service();
    let mut r = routine(Trigger::Manual);
    r.id.clear();
    r.assignee = Assignee::Worker { name: "builder".to_string() };
    let created = svc.create(r).unwrap();

    let record = svc.run_now(&created.id).unwrap();
    assert!(
        matches!(&record.status, RunStatus::Failed { reason } if reason.contains("No team")),
        "{:?}",
        record.status
    );
    assert_eq!(svc.runs(Some(&created.id), 10).len(), 1);
}

#[test]
fn one_bad_file_does_not_take_the_other_routines_down() {
    let (tmp, mut svc) = service();
    let mut r = routine(hourly());
    r.id.clear();
    svc.create(r).unwrap();
    std::fs::write(tmp.path().join(".harness/routines/broken.json"), "{ nope").unwrap();

    let reopened = RoutineService::new(tmp.path()).unwrap();
    assert_eq!(reopened.list().len(), 1);
}

#[test]
fn a_minimal_routine_file_still_loads() {
    // Everything optional left out — what a hand-written routine looks like.
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join(".harness/routines");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("rt-hand.json"),
        r#"{
            "id": "rt-hand",
            "name": "Health check",
            "assignee": { "kind": "lead" },
            "trigger": { "type": "every", "seconds": 300 },
            "task": { "title": "Check the team", "description": "momo-fetch team status" }
        }"#,
    )
    .unwrap();

    let svc = RoutineService::new(tmp.path()).unwrap();
    let r = svc.get("rt-hand").expect("loaded");
    assert!(r.enabled, "routines default to enabled");
    assert_eq!(r.concurrency, Concurrency::Queue);
    assert_eq!(r.permission, DEFAULT_ROUTINE_PERMISSION);
    assert_eq!(r.task.priority, Priority::Medium);
}

#[test]
fn run_history_is_capped() {
    let (_tmp, mut svc) = service();
    let r = routine(hourly());
    for i in 0..(MAX_RUN_HISTORY + 5) {
        svc.record_skip(&r, T0 + i as u64, "test").unwrap();
    }
    assert_eq!(svc.runs.len(), MAX_RUN_HISTORY);
    // Newest first, and the oldest windows are the ones that fell off.
    let newest = svc.runs(None, 1);
    assert_eq!(newest[0].scheduled_at, T0 + (MAX_RUN_HISTORY + 4) as u64);
}

#[test]
fn a_tick_with_nothing_due_is_quiet() {
    let (_tmp, mut svc) = service();
    let mut r = routine(hourly());
    r.id.clear();
    svc.create(r).unwrap();
    let report = svc.tick(now_millis()).unwrap();
    assert!(report.is_quiet(), "{report:?}");
}

#[test]
fn next_due_is_none_while_disabled() {
    let (_tmp, mut svc) = service();
    let mut r = routine(hourly());
    r.id.clear();
    let created = svc.create(r).unwrap();
    assert!(svc.next_due(svc.get(&created.id).unwrap()).is_some());
    let off = svc.set_enabled(&created.id, false).unwrap();
    assert!(svc.next_due(&off).is_none());
}
