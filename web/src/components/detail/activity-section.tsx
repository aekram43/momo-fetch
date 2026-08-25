"use client";

import { Dot, Hint, PanelSection } from "@/components/shared/panel-section";
import { useGatewayResource } from "@/hooks/use-gateway-resource";
import { getActivity } from "@/lib/api-client";
import { formatRelative } from "@/lib/relative-time";
import type { RoutineRun, TeamWorker } from "@/lib/types";
import { useUiStore } from "@/stores/ui-store";

/**
 * Work happening outside this turn: team workers in their panes, and routine
 * runs in their own processes.
 *
 * The rest of this panel is about the turn in front of you. That was the whole
 * blind spot — a worker crashing in a detached tmux pane and a scheduled run
 * failing at 03:00 are both things this session started, and neither appeared
 * anywhere anyone was already looking.
 *
 * Polled rather than pushed, because none of it is caused by this tab: the
 * gateway fires routines on its own timer and tmux never reports to anyone.
 * Ten seconds matches the header's health poll — fast enough that a crash is
 * noticed, slow enough that the tmux calls behind it stay cheap.
 */
const POLL_MS = 10_000;

export function ActivitySection() {
  const { data, error } = useGatewayResource(getActivity, [], POLL_MS);
  const openRoutines = useUiStore((s) => s.openRoutines);

  const workers = data?.team?.workers ?? [];
  const runs = data?.runs ?? [];
  const backlog = data?.team?.mailbox.unread_by_recipient ?? {};

  const quiet = workers.length === 0 && runs.length === 0;

  return (
    <PanelSection
      title="Running elsewhere"
      action={
        data && data.counts.routines_armed > 0 ? (
          <button
            type="button"
            onClick={() => openRoutines(null)}
            className="font-mono text-[10px] font-normal text-faint transition-colors hover:text-ink"
            title="Scheduled routines"
          >
            {data.counts.routines_armed} armed
          </button>
        ) : undefined
      }
    >
      {error ? (
        <Hint>{error}</Hint>
      ) : quiet ? (
        <Hint>
          No team workers and no background runs. Anything the agent starts
          outside this turn shows up here.
        </Hint>
      ) : (
        <div className="space-y-2.5">
          {workers.length > 0 && (
            <ul className="space-y-0.5">
              {workers.map((w) => (
                <WorkerRow key={w.name} worker={w} unread={backlog[w.name] ?? 0} />
              ))}
            </ul>
          )}

          {runs.length > 0 && (
            <ul className="space-y-0.5">
              {runs.map((r) => (
                <RunRow key={r.id} run={r} onOpen={() => openRoutines(r.routine_id)} />
              ))}
            </ul>
          )}
        </div>
      )}
    </PanelSection>
  );
}

/**
 * The Rust `WorkerStatus` renders its failure variants as `"<state>: <reason>"`
 * — `failed_to_start: exited 1 …`. Matching the whole string would quietly
 * classify every failure as "unknown", which is the one state that must never
 * be wrong here, so only the part before the colon is the state.
 */
function workerState(status: string): string {
  return status.split(":", 1)[0].trim();
}

/**
 * Anything unrecognised reads as "off" rather than "fine". The state word next
 * to the dot is always the real answer — the dot is never alone.
 */
function workerTone(status: string): "ok" | "warn" | "bad" | "off" {
  switch (workerState(status)) {
    case "running":
    case "starting":
    case "restarting":
      return "warn";
    case "completed":
      return "ok";
    case "failed":
    case "crashed":
    case "failed_to_start":
      return "bad";
    default:
      return "off";
  }
}

function WorkerRow({ worker, unread }: { worker: TeamWorker; unread: number }) {
  const stale = worker.last_heartbeat
    ? formatRelative(worker.last_heartbeat)
    : null;

  return (
    <li title={worker.task}>
      <div className="flex items-baseline gap-1.5">
        <Dot tone={workerTone(worker.status)} />
        <span className="min-w-0 truncate font-mono text-[11px] text-dim">
          {worker.name}
        </span>
        {worker.agent && (
          <span className="shrink-0 font-mono text-[10px] text-faint">
            {worker.agent}
          </span>
        )}
        <span className="ml-auto shrink-0 font-mono text-[10px] text-faint">
          {workerState(worker.status)}
        </span>
      </div>
      {/* Failure reasons carry an absolute log path, which is exactly what you
          want to copy and exactly what must not take five lines of a 320px
          panel. Two lines here; the whole thing is in the tooltip. */}
      {worker.error && (
        <p
          title={worker.error}
          className="line-clamp-2 pl-3 text-[10px] leading-relaxed text-halt"
        >
          {worker.error}
        </p>
      )}
      {(unread > 0 || stale) && (
        <p className="pl-3 font-mono text-[10px] text-faint">
          {unread > 0 && `${unread} queued`}
          {unread > 0 && stale && " · "}
          {stale && `beat ${stale}`}
        </p>
      )}
    </li>
  );
}

function RunRow({ run, onOpen }: { run: RoutineRun; onOpen: () => void }) {
  return (
    <li>
      <button
        type="button"
        onClick={onOpen}
        className="flex w-full items-baseline gap-1.5 rounded px-1 py-0.5 text-left transition-colors hover:bg-raised"
        title={`${run.routine_name} → ${run.assignee}`}
      >
        <Dot tone="warn" />
        <span className="min-w-0 truncate font-mono text-[11px] text-dim">
          {run.routine_name}
        </span>
        <span className="ml-auto shrink-0 font-mono text-[10px] text-faint">
          {formatRelative(run.started_at) ?? "running"}
        </span>
      </button>
    </li>
  );
}
