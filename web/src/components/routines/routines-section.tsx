"use client";

import { Dot, Hint, PanelSection } from "@/components/shared/panel-section";
import { useGatewayResource } from "@/hooks/use-gateway-resource";
import { listRoutines } from "@/lib/api-client";
import { formatRelative } from "@/lib/relative-time";
import { useUiStore } from "@/stores/ui-store";

/**
 * The rail's view of the schedule: what is armed, and what fires next.
 *
 * Rows are a summary and a way in, not a control surface — everything you can
 * *do* to a routine is one click away in the dialog. Putting run and delete
 * buttons in a 288px rail would mean hitting them by accident.
 */

/**
 * Polled, because almost nothing about a routine changes because of this tab.
 *
 * It used to fetch once on mount and then only when something in the UI
 * mutated gateway state. That missed the two ways routines actually change: the
 * **agent** creates and edits them through `shell_exec` — which this tab never
 * hears about — and the scheduler fires runs on the gateway's own timer, moving
 * `next_due`, `active_runs` and `last_run` with it. So a routine the agent had
 * just scheduled sat behind "Nothing scheduled" here while the right panel,
 * which does poll, already said `1 armed`. Ten seconds, matching that panel.
 */
const POLL_MS = 10_000;

export function RoutinesSection() {
  const openRoutines = useUiStore((s) => s.openRoutines);
  const { data, error } = useGatewayResource(listRoutines, [], POLL_MS);
  const routines = data?.routines ?? [];

  return (
    <PanelSection
      title="Routines"
      action={
        <button
          type="button"
          onClick={() => openRoutines(null)}
          className="font-mono text-[10px] font-normal text-dim transition-colors hover:text-ink"
        >
          manage
        </button>
      }
    >
      {error ? (
        <Hint>{error}</Hint>
      ) : routines.length === 0 ? (
        <Hint>
          Nothing scheduled. A routine hands a task to the agent — or to a standby
          worker — on a cron or a heartbeat.
        </Hint>
      ) : (
        <ul className="space-y-0.5">
          {routines.map((r) => (
            <li key={r.id}>
              <button
                type="button"
                onClick={() => openRoutines(r.id)}
                className="flex w-full items-baseline gap-1.5 rounded px-1 py-0.5 text-left transition-colors hover:bg-raised"
                title={`${r.trigger_summary} → ${r.assignee}`}
              >
                <Dot
                  tone={
                    !r.enabled
                      ? "off"
                      : r.last_run?.status === "failed"
                        ? "bad"
                        : r.active_runs > 0
                          ? "warn"
                          : "ok"
                  }
                />
                <span className="min-w-0 truncate font-mono text-[11px] text-dim">
                  {r.name}
                </span>
                <span className="ml-auto shrink-0 font-mono text-[10px] text-faint">
                  {!r.enabled
                    ? "off"
                    : r.active_runs > 0
                      ? "running"
                      : (formatRelative(r.next_due) ?? "manual")}
                </span>
              </button>
            </li>
          ))}
        </ul>
      )}
    </PanelSection>
  );
}
