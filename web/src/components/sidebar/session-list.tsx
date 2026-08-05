"use client";

import { useCallback, useEffect, useState } from "react";

import {
  createSession,
  deleteSession,
  getSessionMessages,
  listSessions,
} from "@/lib/api-client";
import type { SessionInfo } from "@/lib/types";
import { useChatStore } from "@/stores/chat-store";

/**
 * Sessions (F10) and history restore (F11).
 *
 * **Switching is global, not per-tab.** `Harness` holds one `current_session_id`
 * and `resume_session` mutates it process-wide (spec C4). Two tabs on different
 * sessions corrupt each other, so this list shows the gateway's actual session
 * as current — echoed back in every `role` event — rather than what this tab
 * last clicked.
 */
export function SessionList() {
  const [sessions, setSessions] = useState<SessionInfo[] | null>(null);
  const [busy, setBusy] = useState(false);
  const currentId = useChatStore((s) => s.sessionId);
  const loadHistory = useChatStore((s) => s.loadHistory);
  const reset = useChatStore((s) => s.reset);

  const refresh = useCallback(async () => {
    try {
      const { sessions } = await listSessions();
      setSessions(sessions);
    } catch {
      setSessions([]);
    }
  }, []);

  // Initial load. The state update happens in the promise callback, guarded by
  // `cancelled` so a unmount mid-flight doesn't set state on a dead component.
  useEffect(() => {
    let cancelled = false;
    const load = async () => {
      try {
        const { sessions } = await listSessions();
        if (!cancelled) setSessions(sessions);
      } catch {
        if (!cancelled) setSessions([]);
      }
    };
    void load();
    return () => {
      cancelled = true;
    };
  }, []);

  async function open(id: string) {
    if (busy) return;
    setBusy(true);
    try {
      const { messages } = await getSessionMessages(id);
      loadHistory(id, messages);
    } finally {
      setBusy(false);
    }
  }

  async function create() {
    if (busy) return;
    setBusy(true);
    try {
      const { session_id } = await createSession();
      reset();
      useChatStore.setState({ sessionId: session_id });
      await refresh();
    } finally {
      setBusy(false);
    }
  }

  async function remove(id: string) {
    if (busy) return;
    setBusy(true);
    try {
      await deleteSession(id);
      if (id === currentId) reset();
      await refresh();
    } finally {
      setBusy(false);
    }
  }

  return (
    <section>
      <h2 className="mb-2 flex items-baseline justify-between text-[11px] font-semibold uppercase tracking-[0.14em] text-dim">
        Sessions
        <button
          type="button"
          onClick={() => void create()}
          disabled={busy}
          className="font-mono text-[11px] font-normal text-dim transition-colors hover:text-signal disabled:opacity-40"
        >
          + new
        </button>
      </h2>

      {sessions === null ? (
        <p className="text-xs text-faint">Loading…</p>
      ) : sessions.length === 0 ? (
        <p className="text-xs leading-relaxed text-faint">
          No sessions yet. Start one to begin.
        </p>
      ) : (
        <ul className="-mx-1 max-h-56 space-y-px overflow-y-auto">
          {sessions.slice(0, 40).map((s) => {
            const active = s.id === currentId;
            return (
              <li key={s.id} className="group flex items-center">
                <button
                  type="button"
                  onClick={() => void open(s.id)}
                  className={`min-w-0 flex-1 truncate rounded px-1.5 py-1 text-left font-mono text-[11px] transition-colors ${
                    active
                      ? "bg-raised text-signal"
                      : "text-dim hover:bg-raised hover:text-ink"
                  }`}
                  title={s.id}
                >
                  {active && <span aria-hidden>▸ </span>}
                  {s.id.slice(0, 8)}
                </button>
                <button
                  type="button"
                  onClick={() => void remove(s.id)}
                  aria-label={`Delete session ${s.id.slice(0, 8)}`}
                  className="px-1 font-mono text-[11px] text-faint opacity-0 transition-opacity hover:text-halt group-hover:opacity-100 focus-visible:opacity-100"
                >
                  ×
                </button>
              </li>
            );
          })}
        </ul>
      )}

      {/* The single-harness constraint is a real behaviour, not a caveat to bury. */}
      <p className="mt-2 text-[10px] leading-relaxed text-faint">
        One session is active across every tab.
      </p>
    </section>
  );
}
