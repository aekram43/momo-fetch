"use client";

import { useCallback, useEffect, useState } from "react";

import {
  createSession,
  deleteSession,
  getSessionMessages,
  listSessions,
  renameSession,
} from "@/lib/api-client";
import type { SessionInfo } from "@/lib/types";
import { SkeletonRows } from "@/components/shared/skeleton";
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
  /** Id of the session being renamed, and the text in the field. */
  const [editing, setEditing] = useState<string | null>(null);
  const [draft, setDraft] = useState("");
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

  // The native menu (T7) asks for a new session rather than reimplementing the
  // create-then-reset dance.
  useEffect(() => {
    const onRequest = () => void create();
    window.addEventListener("momo:new-session", onRequest);
    return () => window.removeEventListener("momo:new-session", onRequest);
    // eslint-disable-next-line react-hooks/exhaustive-deps
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

  /**
   * Save the edited name.
   *
   * A blank field means "no change", not "clear the name": the gateway rejects
   * an empty title, and silently discarding someone's name because they
   * selected-all and tabbed away would be the wrong reading of a blank box.
   */
  async function commitRename(id: string) {
    const next = draft.trim();
    setEditing(null);
    const current = sessions?.find((s) => s.id === id)?.title ?? "";
    if (!next || next === current) return;
    try {
      await renameSession(id, next);
      await refresh();
    } catch {
      // The list is refetched either way, so a failure just leaves the old
      // name on screen — which is the truth.
      await refresh();
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
        <SkeletonRows />
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
                {editing === s.id ? (
                  <input
                    autoFocus
                    value={draft}
                    onChange={(e) => setDraft(e.target.value)}
                    onBlur={() => void commitRename(s.id)}
                    onKeyDown={(e) => {
                      if (e.key === "Enter") void commitRename(s.id);
                      // Escape must abandon the edit, not reach the harness as
                      // "interrupt the turn".
                      if (e.key === "Escape") {
                        e.stopPropagation();
                        setEditing(null);
                      }
                    }}
                    aria-label="Session name"
                    className="min-w-0 flex-1 rounded border border-signal/50 bg-void px-1.5 py-1 font-mono text-[11px] text-ink outline-none"
                  />
                ) : (
                  <button
                    type="button"
                    onClick={() => void open(s.id)}
                    onDoubleClick={() => {
                      setDraft(s.title ?? "");
                      setEditing(s.id);
                    }}
                    className={`min-w-0 flex-1 truncate rounded px-1.5 py-1 text-left transition-colors ${
                      s.title ? "text-[11px]" : "font-mono text-[11px]"
                    } ${
                      active
                        ? "bg-raised text-signal"
                        : "text-dim hover:bg-raised hover:text-ink"
                    }`}
                    // The id is what the gateway and the logs call it, so it
                    // stays reachable even once there is a name.
                    title={s.title ? `${s.title}\n${s.id}` : s.id}
                  >
                    {active && <span aria-hidden>▸ </span>}
                    {/* An untitled session shows its id — the same thing it
                        showed before titles existed. Never invent a name. */}
                    {s.title ?? s.id.slice(0, 8)}
                    {/* null means unknown, not empty — never render it as 0. */}
                    {s.event_count !== null && (
                      <span className="ml-1.5 text-faint">{s.event_count}</span>
                    )}
                  </button>
                )}
                <button
                  type="button"
                  onClick={() => {
                    setDraft(s.title ?? "");
                    setEditing(s.id);
                  }}
                  aria-label={`Rename session ${s.title ?? s.id.slice(0, 8)}`}
                  className="px-1 font-mono text-[10px] text-faint opacity-0 transition-opacity hover:text-ink group-hover:opacity-100 focus-visible:opacity-100"
                >
                  ✎
                </button>
                <button
                  type="button"
                  onClick={() => void remove(s.id)}
                  aria-label={`Delete session ${s.title ?? s.id.slice(0, 8)}`}
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
