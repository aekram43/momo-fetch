"use client";

import { useEffect, useRef, useState } from "react";

import { RoutineDetail } from "@/components/routines/routine-detail";
import { RoutineForm } from "@/components/routines/routine-form";
import { Dot } from "@/components/shared/panel-section";
import { useGatewayResource } from "@/hooks/use-gateway-resource";
import {
  ApiError,
  createRoutine,
  deleteRoutine,
  getAssignees,
  getRoutine,
  listRoutines,
  runRoutine,
  tickRoutines,
  updateRoutine,
} from "@/lib/api-client";
import { formatRelative } from "@/lib/relative-time";
import type { Routine, RoutineUpsert } from "@/lib/types";
import { useChatStore } from "@/stores/chat-store";
import { useToastStore } from "@/stores/toast-store";
import { useUiStore } from "@/stores/ui-store";

/**
 * Routines — recurring work, and what it has been doing.
 *
 * A modal for the same reason settings is one: you come here to set something
 * up or to check on it, then leave. It is wider than settings because the two
 * halves have to be visible together — a schedule list you cannot see the
 * outcomes beside is how a routine fails quietly for a week.
 *
 * The body is mounted only while the dialog is open, so every fetch is fresh on
 * open and no state has to be reset on close.
 */
export function RoutinesDialog() {
  const open = useUiStore((s) => s.routinesOpen);
  if (!open) return null;
  return <RoutinesDialogBody />;
}

function RoutinesDialogBody() {
  const close = useUiStore((s) => s.closeRoutines);
  const focusId = useUiStore((s) => s.routinesFocusId);
  const bump = useUiStore((s) => s.bumpServerState);
  const pendingApproval = useChatStore((s) => s.pendingApproval);
  const push = useToastStore((s) => s.push);

  const [selectedId, setSelectedId] = useState<string | null>(focusId);
  const [mode, setMode] = useState<"detail" | "edit" | "new">("detail");
  const [busy, setBusy] = useState(false);
  const [formError, setFormError] = useState<string | null>(null);

  const list = useGatewayResource(listRoutines, []);
  const assignees = useGatewayResource(getAssignees, []);
  const detail = useGatewayResource<Routine | null>(
    () => (selectedId ? getRoutine(selectedId) : Promise.resolve(null)),
    [selectedId],
  );

  const dialogRef = useRef<HTMLDivElement>(null);
  const closeRef = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    closeRef.current?.focus();
  }, []);

  // Escape closes this and must not also interrupt the turn — same rule the
  // settings dialog follows, and the approval dialog still outranks both.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (useChatStore.getState().pendingApproval) return;
      if (e.key !== "Escape") return;
      e.preventDefault();
      e.stopPropagation();
      close();
    };
    document.addEventListener("keydown", onKey, true);
    return () => document.removeEventListener("keydown", onKey, true);
  }, [close]);

  const routines = list.data?.routines ?? [];
  const selected = detail.data ?? null;

  /** Run one mutation, report whatever it says, and refresh every reader. */
  async function act<T>(
    label: string,
    fn: () => Promise<T>,
    after?: (result: T) => void,
  ): Promise<void> {
    if (busy) return;
    setBusy(true);
    setFormError(null);
    try {
      const result = await fn();
      after?.(result);
      // Everything that reads gateway state refetches, so the list, the detail
      // and the sidebar row all move together.
      bump();
    } catch (err) {
      const message =
        err instanceof ApiError ? err.message : ((err as Error).message ?? "Request failed.");
      setFormError(message);
      push({ tone: "error", message: `${label}: ${message}` });
    } finally {
      setBusy(false);
    }
  }

  return (
    <div
      className={`fixed inset-0 z-40 flex items-center justify-center bg-void/80 p-4 ${
        pendingApproval ? "pointer-events-none opacity-0" : ""
      }`}
      role="presentation"
    >
      <button
        type="button"
        aria-label="Close routines"
        onClick={close}
        className="absolute inset-0 cursor-default"
        tabIndex={-1}
      />
      <div
        ref={dialogRef}
        role="dialog"
        aria-modal="true"
        aria-labelledby="routines-title"
        className="relative flex h-[34rem] max-h-full w-full max-w-4xl overflow-hidden rounded-lg border border-rule bg-panel shadow-2xl"
      >
        <nav
          aria-label="Routines"
          className="flex w-60 shrink-0 flex-col gap-px overflow-y-auto border-r border-rule bg-void/40 p-2"
        >
          <div className="mb-1 flex items-baseline gap-2 px-1.5 py-1">
            <h2
              id="routines-title"
              className="text-[11px] font-semibold uppercase tracking-[0.14em] text-dim"
            >
              Routines
            </h2>
            <button
              type="button"
              disabled={busy}
              onClick={() =>
                void act("Check schedule", tickRoutines, (report) =>
                  push({
                    tone: "info",
                    message:
                      report.fired.length > 0
                        ? `${report.fired.length} fired.`
                        : "Nothing was due.",
                  }),
                )
              }
              className="ml-auto font-mono text-[10px] text-faint transition-colors hover:text-ink disabled:opacity-40"
              title="Advance the schedule now instead of waiting for the next tick"
            >
              check
            </button>
          </div>

          <button
            type="button"
            onClick={() => {
              setSelectedId(null);
              setMode("new");
              setFormError(null);
            }}
            className={`mb-1 rounded px-1.5 py-1 text-left font-mono text-[11px] transition-colors ${
              mode === "new" ? "bg-raised text-signal" : "text-dim hover:bg-raised hover:text-ink"
            }`}
          >
            + New routine
          </button>

          {list.error && <p className="px-1.5 text-[10px] text-halt">{list.error}</p>}
          {!list.error && routines.length === 0 && (
            <p className="px-1.5 text-[10px] leading-relaxed text-faint">
              Nothing scheduled yet.
            </p>
          )}

          {routines.map((r) => (
            <button
              key={r.id}
              type="button"
              onClick={() => {
                setSelectedId(r.id);
                setMode("detail");
                setFormError(null);
              }}
              aria-current={selectedId === r.id ? "page" : undefined}
              className={`rounded px-1.5 py-1 text-left transition-colors ${
                selectedId === r.id && mode !== "new"
                  ? "bg-raised"
                  : "hover:bg-raised"
              }`}
            >
              <span className="flex items-baseline gap-1.5">
                <Dot tone={r.enabled ? "ok" : "off"} />
                <span
                  className={`min-w-0 truncate font-mono text-[11px] ${
                    selectedId === r.id && mode !== "new" ? "text-signal" : "text-dim"
                  }`}
                >
                  {r.name}
                </span>
                <span className="ml-auto shrink-0 font-mono text-[10px] text-faint">
                  {r.enabled ? (formatRelative(r.next_due) ?? "manual") : "off"}
                </span>
              </span>
            </button>
          ))}
        </nav>

        <div className="min-w-0 flex-1 overflow-y-auto px-4 py-3">
          {mode === "new" && (
            <RoutineForm
              key="new"
              routine={null}
              assignees={assignees.data?.assignees ?? []}
              busy={busy}
              error={formError}
              onCancel={() => setMode("detail")}
              onSubmit={(body: RoutineUpsert) =>
                void act("Create routine", () => createRoutine(body), (created) => {
                  setSelectedId(created.id);
                  setMode("detail");
                  push({ tone: "info", message: `'${created.name}' scheduled.` });
                })
              }
            />
          )}

          {mode === "edit" && selected && (
            <RoutineForm
              key={selected.id}
              routine={selected}
              assignees={assignees.data?.assignees ?? []}
              busy={busy}
              error={formError}
              onCancel={() => setMode("detail")}
              onSubmit={(body) =>
                void act("Save routine", () => updateRoutine(selected.id, body), () => {
                  setMode("detail");
                })
              }
            />
          )}

          {mode === "detail" && selected && (
            <RoutineDetail
              routine={selected}
              runs={selected.runs ?? []}
              busy={busy}
              onEdit={() => {
                setFormError(null);
                setMode("edit");
              }}
              onRun={() =>
                void act("Run routine", () => runRoutine(selected.id), (run) =>
                  push({
                    tone: "info",
                    message:
                      run.status === "delivered"
                        ? `Handed to ${run.assignee}.`
                        : `Started as ${run.assignee}.`,
                  }),
                )
              }
              onToggle={() =>
                void act("Update routine", () =>
                  updateRoutine(selected.id, { enabled: !selected.enabled }),
                )
              }
              onDelete={() =>
                void act("Delete routine", () => deleteRoutine(selected.id), () => {
                  setSelectedId(null);
                  push({ tone: "info", message: `'${selected.name}' deleted.` });
                })
              }
            />
          )}

          {mode === "detail" && !selected && (
            <div className="max-w-md space-y-2 py-6">
              <p className="text-sm text-ink">Pick a routine, or make one.</p>
              <p className="text-[11px] leading-relaxed text-dim">
                A routine is a task, a schedule, and somebody to hand it to — the
                default agent, a specialist from{" "}
                <code className="font-mono">.harness/agents/</code>, or a standby
                worker on the running team. Each firing starts its own run, beside
                whatever you are doing here.
              </p>
              {detail.error && <p className="text-[11px] text-halt">{detail.error}</p>}
            </div>
          )}
        </div>

        <button
          ref={closeRef}
          type="button"
          onClick={close}
          className="absolute right-2 top-2 rounded px-1.5 py-0.5 font-mono text-[11px] text-faint transition-colors hover:bg-raised hover:text-ink"
        >
          close
        </button>
      </div>
    </div>
  );
}
