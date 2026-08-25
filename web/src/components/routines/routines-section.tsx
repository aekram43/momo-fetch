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
export function RoutinesSection() {
  const openRoutines = useUiStore((s) => s.openRoutines);
  const { data, error } = useGatewayResource(listRoutines, []);
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
