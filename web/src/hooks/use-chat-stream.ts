"use client";

import { useCallback, useRef } from "react";

import { ApiError, interrupt, openChatStream } from "@/lib/api-client";
import { readStream } from "@/lib/sse-parser";
import { useChatStore } from "@/stores/chat-store";
import { useUiStore } from "@/stores/ui-store";

/**
 * Drives one turn: POST the stream, fan events into the chat store, and keep the
 * shell's turn phase in step.
 *
 * **Never retries.** A failed turn is left failed with a visible error. Silently
 * re-sending would double-bill and can re-run tools that already executed
 * (spec F21).
 */
export function useChatStream() {
  const abortRef = useRef<AbortController | null>(null);
  const setTurnPhase = useUiStore((s) => s.setTurnPhase);

  const send = useCallback(
    async (text: string) => {
      const store = useChatStore.getState();
      if (store.turnId) return; // a turn is already in flight

      store.startUserTurn(text);
      setTurnPhase("running");

      const controller = new AbortController();
      abortRef.current = controller;

      try {
        const res = await openChatStream(
          {
            messages: [{ role: "user", content: text }],
            session_id: store.sessionId ?? undefined,
          },
          controller.signal,
        );

        for await (const ev of readStream(res)) {
          const s = useChatStore.getState();
          switch (ev.type) {
            case "role":
              s.onRole(ev.data);
              break;
            case "text":
              s.onText(ev.data.content);
              break;
            case "tool_call_start":
              s.onToolStart(ev.data);
              break;
            case "tool_call_result":
              s.onToolResult(ev.data);
              break;
            case "approval_required":
              s.onApprovalRequired(ev.data);
              setTurnPhase("awaiting-approval");
              break;
            case "approval_resolved":
              s.onApprovalResolved(ev.data.approved);
              setTurnPhase("running");
              break;
            case "usage":
              s.onUsage(ev.data);
              break;
            case "context_usage":
              s.onContext(ev.data);
              break;
            case "error":
              s.onError(ev.data.message);
              break;
            case "done":
              s.onDone();
              break;
            case "unknown":
              // A gateway that gained an event must not fail silently here.
              console.warn("[sse] unknown event", ev.name, ev.raw);
              break;
          }
        }
      } catch (err) {
        const s = useChatStore.getState();
        if (err instanceof ApiError && err.isTurnConflict) {
          // F29: someone else (or another tab) holds the harness.
          s.setConflict(err.message);
        } else if ((err as Error)?.name === "AbortError") {
          // User-initiated; the interrupt path already reported it.
        } else {
          s.onError((err as Error).message ?? "Stream failed.");
        }
      } finally {
        useChatStore.getState().onDone();
        setTurnPhase("idle");
        abortRef.current = null;
      }
    },
    [setTurnPhase],
  );

  /**
   * Stop the current turn.
   *
   * Aborting the fetch only closes *our* end — the harness keeps generating and
   * keeps holding the turn lease. So tell the gateway first, then abort.
   */
  const stop = useCallback(async () => {
    const { turnId } = useChatStore.getState();
    try {
      await interrupt(turnId ?? undefined);
    } catch {
      // Already finished, or the gateway is gone. Abort locally regardless.
    }
    abortRef.current?.abort();
  }, []);

  return { send, stop };
}
