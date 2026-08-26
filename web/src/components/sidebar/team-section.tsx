"use client";

import { useState } from "react";

import { Dot, Hint, PanelSection } from "@/components/shared/panel-section";
import { useGatewayResource } from "@/hooks/use-gateway-resource";
import {
  getActivity,
  restartWorker,
  startTeam,
  stopTeam,
} from "@/lib/api-client";
import type { TeamActionResult, TeamWorker } from "@/lib/types";
import { useUiStore } from "@/stores/ui-store";

/**
 * The team: who is on it, what each of them is doing, and the three buttons
 * that change any of it.
 *
 * It used to be read-only, on the reasoning that starting needs a config and
 * stopping destroys worktrees. Half of that held up. Stopping still asks before
 * it acts — it is the click that deletes branches — but "start the squad I
 * already wrote down" was never a dangerous operation, and making it the one
 * thing you had to leave the app to type meant the agent could not do it either
 * and quietly substituted something else.
 *
 * Every button calls the same action `momo-fetch team …` runs. What the CLI
 * prints on stderr — "tmux not found, nothing was launched" — is surfaced here
 * as a note rather than dropped, because a team with no processes behind it
 * otherwise looks exactly like a team.
 *
 * Not the same question as the right panel's "Running elsewhere": that one is
 * *what is in flight this minute*, across teams and routines both. This is the
 * squad's standing composition — its name, its members, and the mail waiting
 * for each of them.
 */
const POLL_MS = 10_000;

export function TeamSection() {
  const { data, error } = useGatewayResource(getActivity, [], POLL_MS);
  const bumpServerState = useUiStore((s) => s.bumpServerState);

  /** One action at a time, and the name of the one running, for its label. */
  const [busy, setBusy] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [failure, setFailure] = useState<string | null>(null);
  const [confirmStop, setConfirmStop] = useState(false);

  const team = data?.team ?? null;
  const active = Boolean(team?.team_id);
  const workers = team?.workers ?? [];
  const backlog = team?.mailbox.unread_by_recipient ?? {};
  const configs = data?.team_configs ?? [];

  /**
   * Run one team action and report it.
   *
   * `bumpServerState` rather than a local refetch: the same team is drawn in
   * the right panel too, and a start that only refreshed this rail would leave
   * the other one insisting nothing is running.
   */
  async function act(label: string, call: () => Promise<TeamActionResult>) {
    if (busy) return;
    setBusy(label);
    setNotice(null);
    setFailure(null);
    try {
      const result = await call();
      // The CLI's stderr narration. Usually empty; when it isn't, it is the
      // caveat that makes the difference between started and "recorded".
      setNotice(result.notes?.join(" ") ?? null);
      bumpServerState();
    } catch (e) {
      setFailure((e as Error).message ?? "The team command failed.");
    } finally {
      setBusy(null);
      setConfirmStop(false);
    }
  }

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
        <IdleTeam
          configs={configs}
          busy={busy}
          onStart={(name) => void act(name, () => startTeam(name))}
        />
      ) : (
        <>
          <p className="mb-1.5 truncate font-mono text-[11px] text-dim" title={team?.team_id ?? ""}>
            {team?.name ?? team?.team_id}
          </p>
          <ul className="space-y-0.5">
            {workers.map((w) => (
              <WorkerRow
                key={w.name}
                worker={w}
                unread={backlog[w.name] ?? 0}
                busy={busy}
                onRestart={() => void act(w.name, () => restartWorker(w.name))}
              />
            ))}
          </ul>
          {(backlog.lead ?? 0) > 0 && (
            <p className="mt-1.5 font-mono text-[10px] text-signal">
              {backlog.lead} waiting for you
            </p>
          )}

          {/* Two steps, and the second one says what it does. This click runs
              `git worktree remove --force` on every worker and deletes its
              branch — merge first, or the work is gone. */}
          {confirmStop ? (
            <div className="mt-2 rounded border border-halt/40 px-1.5 py-1">
              <p className="text-[10px] leading-relaxed text-halt">
                Removes every worktree and deletes each worker branch. Merge
                anything you want to keep first.
              </p>
              <div className="mt-1 flex gap-2">
                <button
                  type="button"
                  onClick={() => void act("stop", () => stopTeam())}
                  disabled={busy !== null}
                  className="font-mono text-[10px] text-halt transition-colors hover:underline disabled:opacity-40"
                >
                  {busy === "stop" ? "stopping…" : "stop the team"}
                </button>
                <button
                  type="button"
                  onClick={() => setConfirmStop(false)}
                  className="font-mono text-[10px] text-faint transition-colors hover:text-ink"
                >
                  cancel
                </button>
              </div>
            </div>
          ) : (
            <button
              type="button"
              onClick={() => setConfirmStop(true)}
              disabled={busy !== null}
              className="mt-2 font-mono text-[10px] text-faint transition-colors hover:text-halt disabled:opacity-40"
            >
              stop team
            </button>
          )}
        </>
      )}

      {notice && (
        <p className="mt-1.5 text-[10px] leading-relaxed text-signal">{notice}</p>
      )}
      {failure && (
        <p className="mt-1.5 text-[10px] leading-relaxed text-halt">{failure}</p>
      )}
    </PanelSection>
  );
}

function IdleTeam({
  configs,
  busy,
  onStart,
}: {
  configs: string[];
  busy: string | null;
  onStart: (name: string) => void;
}) {
  if (configs.length === 0) {
    return (
      <Hint>
        No team running. Define one in{" "}
        <code className="font-mono">.harness/teams/&lt;name&gt;.json</code>, then
        start it here or with{" "}
        <code className="font-mono">momo-fetch team start</code>.
      </Hint>
    );
  }
  return (
    <>
      <Hint>No team running. These are defined:</Hint>
      <ul className="mt-1 space-y-0.5">
        {configs.map((name) => (
          <li key={name} className="group flex items-baseline gap-1.5">
            <Dot tone="off" />
            <span className="min-w-0 truncate font-mono text-[11px] text-dim">
              {name}
            </span>
            <button
              type="button"
              onClick={() => onStart(name)}
              disabled={busy !== null}
              className="ml-auto shrink-0 font-mono text-[10px] text-faint opacity-0 transition-opacity hover:text-signal group-hover:opacity-100 focus-visible:opacity-100 disabled:opacity-40"
              aria-label={`Start team ${name}`}
            >
              {busy === name ? "starting…" : "start"}
            </button>
          </li>
        ))}
      </ul>
      <p className="mt-1.5 text-[10px] leading-relaxed text-faint">
        Each worker gets its own tmux pane, and a worktree when the config asks
        for one.
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

function WorkerRow({
  worker,
  unread,
  busy,
  onRestart,
}: {
  worker: TeamWorker;
  unread: number;
  busy: string | null;
  onRestart: () => void;
}) {
  const tone = workerTone(worker.status);

  return (
    <li>
      <div className="flex items-baseline gap-1.5" title={worker.task}>
        <Dot tone={tone} />
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
        {/* Offered only where it is the answer: a dead pane. Restarting a
            running worker would kill work in progress. */}
        {tone === "bad" && (
          <button
            type="button"
            onClick={onRestart}
            disabled={busy !== null}
            className="shrink-0 font-mono text-[10px] text-faint transition-colors hover:text-signal disabled:opacity-40"
            aria-label={`Restart worker ${worker.name}`}
          >
            {busy === worker.name ? "…" : "restart"}
          </button>
        )}
      </div>
    </li>
  );
}
