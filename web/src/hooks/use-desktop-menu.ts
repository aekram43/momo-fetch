"use client";

import { useEffect } from "react";

import { interrupt } from "@/lib/api-client";
import { onMenuAction } from "@/lib/desktop";
import { useChatStore } from "@/stores/chat-store";
import { useToastStore } from "@/stores/toast-store";
import { useUiStore } from "@/stores/ui-store";

/**
 * Routes native menu and tray actions (T6/T7) to the same handlers the
 * keyboard shortcuts use, so the two can never drift apart.
 *
 * A no-op outside the desktop shell — the event simply never fires.
 */
export function useDesktopMenu() {
  const toggleSidebar = useUiStore((s) => s.toggleSidebar);
  const toggleDetail = useUiStore((s) => s.toggleDetail);
  const openSettings = useUiStore((s) => s.openSettings);
  const push = useToastStore((s) => s.push);

  useEffect(() => {
    return onMenuAction((action) => {
      switch (action) {
        case "toggle-sidebar":
          toggleSidebar();
          break;
        case "toggle-detail":
          toggleDetail();
          break;
        case "settings":
          openSettings();
          break;
        case "interrupt": {
          const { turnId } = useChatStore.getState();
          if (turnId) void interrupt(turnId);
          break;
        }
        case "new-session":
          // The session list owns creation; ask it rather than duplicating the
          // create-then-reset dance here.
          window.dispatchEvent(new CustomEvent("momo:new-session"));
          break;
        case "open-project":
          // T4 is consequential — it re-roots the sandbox — so the dialog and
          // its confirmation live in the UI, not in the menu handler.
          push({
            tone: "info",
            message: "Open a project from Customization → Files to re-root the agent.",
          });
          break;
      }
    });
  }, [toggleSidebar, toggleDetail, openSettings, push]);
}
