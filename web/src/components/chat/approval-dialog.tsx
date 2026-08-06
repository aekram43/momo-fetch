"use client";

import { useEffect, useRef, useState } from "react";

import { ApiError, approve, deny } from "@/lib/api-client";
import { useChatStore } from "@/stores/chat-store";
import { useUiStore } from "@/stores/ui-store";

/**
 * **F9 — the approval dialog. This is the product's security boundary.**
 *
 * Two things here are requirements, not design choices:
 *
 * 1. **Sticky scope must be stated.** Approving inserts the tool *name* into a
 *    process-wide `approved_tools` set and rebuilds the runner with that baked
 *    into `RunConfig` (spec C2). One "Approve" on `shell_exec` means the agent
 *    is never asked about `shell_exec` again for the life of the process.
 *    Silent stickiness on a shell tool is a security surprise, so the dialog
 *    says so in plain language rather than in a tooltip.
 *
 * 2. **Denial is not remembered.** `run_confirmation_turn(name, false)` records
 *    nothing, so the model can re-request immediately (spec Q10). The copy says
 *    that too — a user who thinks "Deny" is permanent will be surprised twice.
 *
 * It is a focus-trapped modal at every breakpoint (spec §6). It must never live
 * inside a collapsible panel, or a narrow-viewport user can strand a turn until
 * the 5-minute timeout fires.
 */
export function ApprovalDialog() {
  const pending = useChatStore((s) => s.pendingApproval);
  const onResolved = useChatStore((s) => s.onApprovalResolved);
  const [busy, setBusy] = useState(false);
  const denyRef = useRef<HTMLButtonElement>(null);
  const dialogRef = useRef<HTMLDivElement>(null);

  // Deny takes initial focus: the safe action should be the one a stray Enter
  // hits.
  useEffect(() => {
    if (pending) denyRef.current?.focus();
  }, [pending]);

  // Focus trap + Escape-to-deny.
  useEffect(() => {
    if (!pending) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        void resolve(false);
        return;
      }
      if (e.key !== "Tab") return;
      const focusable = dialogRef.current?.querySelectorAll<HTMLElement>(
        "button, [href], input, [tabindex]:not([tabindex='-1'])",
      );
      if (!focusable?.length) return;
      const first = focusable[0];
      const last = focusable[focusable.length - 1];
      if (e.shiftKey && document.activeElement === first) {
        e.preventDefault();
        last.focus();
      } else if (!e.shiftKey && document.activeElement === last) {
        e.preventDefault();
        first.focus();
      }
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [pending]);

  if (!pending) return null;

  async function resolve(approved: boolean) {
    if (!pending || busy) return;
    setBusy(true);
    const req = {
      turn_id: pending.turn_id,
      call_id: pending.call_id,
      tool_name: pending.name,
    };
    try {
      await (approved ? approve(req) : deny(req));
      // Approving adds a standing grant. Everything showing that set — the
      // status-bar badge, the settings dialog — has to hear about it now, not
      // at whatever poll comes next.
      if (approved) useUiStore.getState().bumpServerState();
      onResolved(approved);
    } catch (err) {
      // A stale approval means the turn already moved on — dismiss quietly
      // rather than showing an error for something the user cannot act on.
      if (err instanceof ApiError && err.isStaleApproval) {
        onResolved(false);
      } else {
        throw err;
      }
    } finally {
      setBusy(false);
    }
  }

  const args =
    typeof pending.args === "object" && pending.args !== null
      ? JSON.stringify(pending.args, null, 2)
      : String(pending.args ?? "");

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-void/80 p-4"
      role="presentation"
    >
      <div
        ref={dialogRef}
        role="alertdialog"
        aria-modal="true"
        aria-labelledby="approval-title"
        className="w-full max-w-lg rounded-lg border border-signal/40 bg-panel shadow-2xl"
      >
        <div className="flex items-start gap-3 border-b border-rule px-5 py-4">
          <span className="mt-0.5 size-2 shrink-0 rounded-full bg-signal" aria-hidden />
          <div className="min-w-0">
            <h2 id="approval-title" className="text-sm font-semibold text-ink">
              Run{" "}
              <code className="font-mono text-signal">{pending.name}</code>?
            </h2>
            <p className="mt-0.5 text-xs text-dim">
              The agent is waiting for your decision.
            </p>
          </div>
          <Countdown expiresAt={pending.expires_at} />
        </div>

        {pending.destructive && (
          <p className="border-b border-halt/30 bg-halt/10 px-5 py-2.5 text-xs text-ink">
            <span className="font-semibold text-halt">Destructive</span>
            {pending.category ? ` — ${pending.category}.` : "."} This cannot be
            undone.
          </p>
        )}

        <div className="px-5 py-4">
          <p className="mb-1.5 text-[11px] font-semibold uppercase tracking-[0.14em] text-dim">
            Arguments
          </p>
          <pre className="max-h-48 overflow-auto rounded border border-rule bg-void px-3 py-2 font-mono text-xs leading-relaxed text-ink">
            {args}
          </pre>
        </div>

        {/* The disclosure. Do not soften this copy — it is what the code does. */}
        {pending.sticky && (
          <p className="mx-5 mb-4 rounded border border-rule bg-raised px-3 py-2.5 text-xs leading-relaxed text-dim">
            Approving lets the agent run{" "}
            <code className="font-mono text-ink">{pending.name}</code> for the
            rest of this session without asking again. Denying applies only to
            this request — the agent can ask again.
          </p>
        )}

        <div className="flex justify-end gap-2 border-t border-rule px-5 py-3">
          <button
            ref={denyRef}
            type="button"
            disabled={busy}
            onClick={() => void resolve(false)}
            className="rounded border border-rule px-3 py-1.5 text-xs font-medium text-ink transition-colors hover:border-halt hover:text-halt disabled:opacity-50"
          >
            Deny
          </button>
          <button
            type="button"
            disabled={busy}
            onClick={() => void resolve(true)}
            className="rounded bg-consent px-3 py-1.5 text-xs font-semibold text-void transition-opacity hover:opacity-90 disabled:opacity-50"
          >
            Approve for this session
          </button>
        </div>
      </div>
    </div>
  );
}

/**
 * Time until the gateway auto-denies. The turn is not stuck forever, and saying
 * so removes the pressure to click something to make the dialog go away.
 */
function Countdown({ expiresAt }: { expiresAt: string }) {
  const [left, setLeft] = useState(() => secondsUntil(expiresAt));

  useEffect(() => {
    const id = setInterval(() => setLeft(secondsUntil(expiresAt)), 1000);
    return () => clearInterval(id);
  }, [expiresAt]);

  if (left <= 0) return null;
  const m = Math.floor(left / 60);
  const s = String(left % 60).padStart(2, "0");

  return (
    <span className="ml-auto shrink-0 font-mono text-xs text-dim" aria-live="off">
      {m}:{s}
    </span>
  );
}

function secondsUntil(iso: string): number {
  const ms = new Date(iso).getTime() - Date.now();
  return Number.isFinite(ms) ? Math.max(0, Math.floor(ms / 1000)) : 0;
}
