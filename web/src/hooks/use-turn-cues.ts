"use client";

import { useEffect, useRef } from "react";

import { playCue } from "@/lib/sounds";
import { useChatStore } from "@/stores/chat-store";
import { useToastStore } from "@/stores/toast-store";
import { useUiStore } from "@/stores/ui-store";

/**
 * F27 — audio cues, and F21's stream-error toast.
 *
 * Watches the chat store for edges rather than being called from the stream
 * loop, so the cue logic stays out of the hot path and there is one place that
 * decides what is worth interrupting the user for.
 */
export function useTurnCues() {
  const soundEnabled = useUiStore((s) => s.soundEnabled);
  const push = useToastStore((s) => s.push);

  // Edge detection needs the previous value; refs avoid re-subscribing.
  const prevTurn = useRef<string | null>(null);
  const prevApproval = useRef<string | null>(null);
  const prevError = useRef<string | null>(null);

  useEffect(() => {
    return useChatStore.subscribe((state) => {
      // Approval opened — the one cue that means "you are blocking progress".
      const approvalId = state.pendingApproval?.call_id ?? null;
      if (approvalId && approvalId !== prevApproval.current) {
        playCue("approval", soundEnabled);
      }
      prevApproval.current = approvalId;

      // Turn finished (had one, now don't).
      if (prevTurn.current && !state.turnId) {
        playCue("done", soundEnabled);
      }
      prevTurn.current = state.turnId;

      // New error text — toast it and cue.
      if (state.error && state.error !== prevError.current) {
        push({ tone: "error", message: state.error });
        playCue("error", soundEnabled);
      }
      prevError.current = state.error;
    });
  }, [soundEnabled, push]);
}
