"use client";

import { useEffect } from "react";

import { interrupt } from "@/lib/api-client";
import { useChatStore } from "@/stores/chat-store";
import { useUiStore } from "@/stores/ui-store";

/**
 * F20 — keyboard shortcuts.
 *
 * **Escape interrupts the turn** (G13). It is the one shortcut that reaches the
 * harness rather than the UI, so it is deliberately not swallowed by a
 * `preventDefault` elsewhere.
 *
 * Escape is *not* bound while the approval dialog is open — that dialog owns
 * Escape as "deny", and stealing it would make the safe action unreachable by
 * keyboard.
 */
export function useShortcuts() {
  const { toggleSidebar, toggleDetail, openSettings } = useUiStore();

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const mod = e.metaKey || e.ctrlKey;

      if (e.key === "Escape") {
        const { turnId, pendingApproval } = useChatStore.getState();
        if (pendingApproval) return; // the dialog handles it
        // The settings dialog also handles its own Escape, in the capture
        // phase, and stops it here — but check anyway rather than relying on
        // ordering for something that would otherwise stop the agent.
        if (useUiStore.getState().settingsOpen) return;
        if (turnId) {
          e.preventDefault();
          void interrupt(turnId);
        }
        return;
      }

      if (!mod) return;

      // Don't fight the browser or the composer for plain typing keys.
      switch (e.key.toLowerCase()) {
        case "b":
          e.preventDefault();
          toggleSidebar();
          break;
        case "j":
          e.preventDefault();
          toggleDetail();
          break;
        // Cmd/Ctrl+, is the settings shortcut on every platform that has one.
        case ",":
          e.preventDefault();
          openSettings();
          break;
        default:
          break;
      }
    };

    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [toggleSidebar, toggleDetail, openSettings]);
}
