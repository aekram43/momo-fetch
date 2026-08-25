"use client";

import { Dot, Hint, PanelSection } from "@/components/shared/panel-section";
import { useGatewayResource } from "@/hooks/use-gateway-resource";
import { getActivity } from "@/lib/api-client";
import type { TeamWorker } from "@/lib/types";

/**
 * The team: who is on it, and what each of them is doing.
 *
 * Read-only, deliberately. Starting a team asks for a config; stopping one
 * removes worktrees and deletes branches. Neither belongs behind a single click
 * in a 288px rail — the first would need a form, and the second is destructive
 * enough that the CLI makes you name it. So this shows the roster and points at
 * the command.
 *
 * Not the same question as the right panel's "Running elsewhere": that one is
 * *what is in flight this minute*, across teams and routines both. This is the
 * squad's standing composition — its name, its members, and the mail waiting
 * for each of them.
 */
const POLL_MS = 10_000;

export function TeamSection() {
  const { data, error } = useGatewayResource(getActivity, [], POLL_MS);

  const team = data?.team ?? null;
  const active = Boolean(team?.team_id);
  const workers = team?.workers ?? [];
  const backlog = team?.mailbox.unread_by_recipient ?? {};
  const configs = data?.team_configs ?? [];

  return (
    <PanelSection
      title="Team"
      action={
        active ? (
          <span className="font-mono text-[10px] font-normal text-faint">
            {workers.length} {workers.length === 1 ? "worker" : "workers"}
          </span>
        ) : undefined
      }
    >
      {error ? (
        <Hint>{error}</Hint>
      ) : !active ? (
        <IdleTeam configs={configs} />
      ) : (
        <>
          <p className="mb-1.5 truncate font-mono text-[11px] text-dim" title={team?.team_id ?? ""}>
            {team?.name ?? team?.team_id}
          </p>
          <ul className="space-y-0.5">
            {workers.map((w) => (
              <WorkerRow key={w.name} worker={w} unread={backlog[w.name] ?? 0} />
            ))}
          </ul>
          {(backlog.lead ?? 0) > 0 && (
            <p className="mt-1.5 font-mono text-[10px] text-signal">
              {backlog.lead} waiting for you
            </p>
          )}
        </>
      )}
    </PanelSection>
  );
}

function IdleTeam({ configs }: { configs: string[] }) {
  if (configs.length === 0) {
    return (
      <Hint>
        No team running. Define one in{" "}
        <code className="font-mono">.harness/teams/&lt;name&gt;.json</code>, then
        start it with <code className="font-mono">momo-fetch team start</code>.
      </Hint>
    );
  }
  return (
    <>
      <Hint>No team running. These are defined:</Hint>
      <ul className="mt-1 space-y-0.5">
        {configs.map((name) => (
          <li key={name} className="flex items-baseline gap-1.5">
            <Dot tone="off" />
            <span className="truncate font-mono text-[11px] text-dim">{name}</span>
          </li>
        ))}
      </ul>
      <p className="mt-1.5 text-[10px] leading-relaxed text-faint">
        Start one with{" "}
        <code className="font-mono">momo-fetch team start &lt;name&gt;</code>.
      </p>
    </>
  );
}

/**
 * `WorkerStatus` renders failures as `"<state>: <reason>"`, so only the part
 * before the colon is the state. Matching the whole string would file every
 * failure under "unknown" — the one answer that must never be wrong here.
 */
function workerState(status: string): string {
  return status.split(":", 1)[0].trim();
}

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
  return (
    <li>
      <div className="flex items-baseline gap-1.5" title={worker.task}>
        <Dot tone={workerTone(worker.status)} />
        <span className="min-w-0 truncate font-mono text-[11px] text-dim">
          {worker.name}
        </span>
        {/* A standby worker can be given more work; a one-shot one cannot, and
            that is the fact you need before you try to send it any. */}
        {worker.mode === "standby" && (
          <span className="shrink-0 font-mono text-[10px] text-faint">standby</span>
        )}
        <span className="ml-auto shrink-0 font-mono text-[10px] text-faint">
          {unread > 0 ? `${unread} queued` : workerState(worker.status)}
        </span>
      </div>
    </li>
  );
}
