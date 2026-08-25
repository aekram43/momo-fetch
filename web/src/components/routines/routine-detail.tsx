"use client";

import { Dot } from "@/components/shared/panel-section";
import { formatRelative } from "@/lib/relative-time";
import type { Routine, RoutineRun, RunState } from "@/lib/types";

/**
 * One routine, as it stands: when it next fires, what it last did, and the
 * three actions worth having in front of you (run, edit, delete).
 *
 * The run list is the point of this pane. A schedule you cannot see the
 * outcomes of is a schedule you have to trust, and a routine that has been
 * failing every night for a week looks exactly like a healthy one from its
 * definition alone.
 */
export function RoutineDetail({
  routine,
  runs,
  busy,
  onRun,
  onEdit,
  onToggle,
  onDelete,
}: {
  routine: Routine;
  runs: RoutineRun[];
  busy: boolean;
  onRun: () => void;
  onEdit: () => void;
  onToggle: () => void;
  onDelete: () => void;
}) {
  const due = formatRelative(routine.next_due);

  return (
    <div className="space-y-4">
      {/* The dialog's close button sits top-right; the id has to clear it. */}
      <div className="pr-12">
        <div className="flex items-baseline gap-2">
          <Dot tone={routine.enabled ? "ok" : "off"} />
          <h3 className="min-w-0 truncate text-sm font-medium text-ink">{routine.name}</h3>
          <span className="ml-auto shrink-0 font-mono text-[10px] text-faint">
            {routine.id}
          </span>
        </div>
        <p className="mt-1 font-mono text-[11px] text-dim">
          {routine.trigger_summary} → {routine.assignee}
        </p>
      </div>

      <dl className="grid grid-cols-2 gap-x-4 gap-y-1.5 font-mono text-[11px]">
        <Stat label="next">
          {routine.enabled ? (
            <span title={routine.next_due ?? undefined}>{due ?? "never"}</span>
          ) : (
            <span className="text-faint">off</span>
          )}
        </Stat>
        <Stat label="last run">
          <span title={routine.last_run_at ?? undefined}>
            {formatRelative(routine.last_run_at) ?? "never"}
          </span>
        </Stat>
        <Stat label="fired">{routine.fired}</Stat>
        <Stat label="skipped">{routine.skipped}</Stat>
        <Stat label="in flight">{routine.active_runs}</Stat>
        <Stat label="queued">{routine.queued}</Stat>
      </dl>

      <div className="rounded border border-rule bg-void/40 p-3">
        <p className="mb-1 text-[11px] font-semibold text-ink">{routine.task.title}</p>
        <p className="mb-2 font-mono text-[10px] uppercase tracking-[0.14em] text-faint">
          {routine.task.priority} · permission {routine.permission} ·{" "}
          {routine.concurrency}
        </p>
        <p className="whitespace-pre-wrap text-[11px] leading-relaxed text-dim">
          {routine.task.description}
        </p>
      </div>

      <div className="flex flex-wrap gap-2">
        <button
          type="button"
          onClick={onRun}
          disabled={busy}
          className="rounded bg-signal px-3 py-1 font-mono text-[11px] font-semibold text-void disabled:opacity-40"
        >
          Run now
        </button>
        <button
          type="button"
          onClick={onEdit}
          disabled={busy}
          className="rounded border border-rule px-3 py-1 font-mono text-[11px] text-dim transition-colors hover:text-ink disabled:opacity-40"
        >
          Edit
        </button>
        <button
          type="button"
          onClick={onToggle}
          disabled={busy}
          className="rounded border border-rule px-3 py-1 font-mono text-[11px] text-dim transition-colors hover:text-ink disabled:opacity-40"
        >
          {routine.enabled ? "Disable" : "Enable"}
        </button>
        <button
          type="button"
          onClick={onDelete}
          disabled={busy}
          className="ml-auto rounded px-2 py-1 font-mono text-[11px] text-faint transition-colors hover:text-halt disabled:opacity-40"
        >
          Delete
        </button>
      </div>

      <section>
        <h4 className="mb-1.5 text-[11px] font-semibold uppercase tracking-[0.14em] text-dim">
          Recent runs
        </h4>
        {runs.length === 0 ? (
          <p className="text-[11px] text-faint">
            Nothing has fired yet. Run now to see what it does.
          </p>
        ) : (
          <ul className="space-y-1">
            {runs.map((run) => (
              <RunRow key={run.id} run={run} />
            ))}
          </ul>
        )}
      </section>
    </div>
  );
}

function Stat({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className="flex items-baseline gap-2">
      <dt className="text-faint">{label}</dt>
      <dd className="ml-auto text-dim">{children}</dd>
    </div>
  );
}

/** A dot alone never carries the meaning — the state word is always next to it. */
function toneFor(state: RunState): "ok" | "warn" | "bad" | "off" {
  if (state === "succeeded") return "ok";
  if (state === "failed") return "bad";
  if (state === "running" || state === "delivered") return "warn";
  return "off";
}

function RunRow({ run }: { run: RoutineRun }) {
  const when = formatRelative(run.started_at) ?? "";
  return (
    <li>
      <div className="flex items-baseline gap-1.5">
        <Dot tone={toneFor(run.status)} />
        <span className="font-mono text-[11px] text-dim">{run.status}</span>
        {run.exit_code !== null && run.exit_code !== 0 && (
          <span className="font-mono text-[10px] text-faint">exit {run.exit_code}</span>
        )}
        <span className="ml-auto shrink-0 font-mono text-[10px] text-faint">
          {run.source === "manual" ? "manual · " : ""}
          <span title={run.started_at ?? undefined}>{when}</span>
        </span>
      </div>
      {run.reason && (
        <p className="pl-3 text-[10px] leading-relaxed text-faint">{run.reason}</p>
      )}
      {run.log_path && (
        <p className="truncate pl-3 font-mono text-[10px] text-faint" title={run.log_path}>
          {run.log_path}
        </p>
      )}
    </li>
  );
}
