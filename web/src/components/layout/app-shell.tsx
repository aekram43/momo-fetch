"use client";

import { useEffect } from "react";

import { ApprovalDialog } from "@/components/chat/approval-dialog";
import { ChatPanel } from "@/components/chat/chat-panel";
import { DetailPanel } from "@/components/detail/detail-panel";
import { Header } from "@/components/layout/header";
import { Sidebar } from "@/components/layout/sidebar";
import { Toaster } from "@/components/shared/toaster";
import { TurnRail } from "@/components/shared/turn-rail";
import { useShortcuts } from "@/hooks/use-shortcuts";
import { useTurnCues } from "@/hooks/use-turn-cues";
import { useChatStore } from "@/stores/chat-store";
import { useUiStore } from "@/stores/ui-store";

/**
 * Three-panel shell (spec §6).
 *
 * The page never scrolls; each region owns its own overflow. That matters more
 * than usual here — a streaming turn appends continuously, and a scrolling
 * document would drag the composer off screen while the user is trying to
 * interrupt.
 *
 * Responsive behaviour is deliberately conservative for now: panels collapse via
 * the header toggles, and below `lg` they stay closed. Full breakpoint work is
 * F23. The one hard rule from spec §6 already holds — the approval dialog (F9)
 * must be a focus-trapped modal at every width, never nested inside a panel that
 * can be collapsed, or a mobile user can strand a turn until it times out.
 */
export function AppShell() {
  const {
    sidebarOpen,
    detailOpen,
    turnPhase,
    mobileDrawer,
    toggleSidebar,
    toggleDetail,
    openDrawer,
  } = useUiStore();
  useShortcuts();
  useTurnCues();

  // F28 — apply stored panel/sound preferences after mount, never during module
  // evaluation, so the first client render matches the exported HTML.
  const hydrate = useUiStore((s) => s.hydrate);
  useEffect(() => {
    hydrate();
  }, [hydrate]);

  return (
    <div className="flex h-full flex-col">
      {/* Below the breakpoint the same buttons drive drawers instead of columns. */}
      <Header
        onToggleSidebar={() => {
          toggleSidebar();
          openDrawer("sessions");
        }}
        onToggleDetail={() => {
          toggleDetail();
          openDrawer("detail");
        }}
      />

      <div className="flex min-h-0 flex-1">
        {/* F23 — at lg+ this is a column. */}
        {sidebarOpen && (
          <div className="hidden lg:flex">
            <Sidebar />
          </div>
        )}

        <main className="flex min-w-0 flex-1">
          <TurnRail phase={turnPhase} />
          <ChatPanel />
        </main>

        {detailOpen && (
          <div className="hidden xl:flex">
            <DetailPanel />
          </div>
        )}

        {/* Below the breakpoint: one drawer at a time, over the chat, dismissible
            by tapping the scrim. Never two at once — that buries the composer. */}
        {mobileDrawer && (
          <>
            <button
              type="button"
              aria-label="Close panel"
              onClick={() => openDrawer(null)}
              className="fixed inset-0 z-20 bg-void/70 xl:hidden"
            />
            <div
              className={`fixed inset-y-0 z-30 flex xl:hidden ${
                mobileDrawer === "sessions" ? "left-0" : "right-0"
              } ${mobileDrawer === "sessions" ? "" : "lg:hidden"}`}
            >
              {mobileDrawer === "sessions" ? <Sidebar /> : <DetailPanel />}
            </div>
          </>
        )}
      </div>

      <StatusBar />

      {/* Modal at every breakpoint, outside every collapsible panel — a narrow
          viewport must never be able to strand a turn (spec §6). */}
      <ApprovalDialog />
      <Toaster />
    </div>
  );
}

/** Bottom rail: the three facts that change what a keystroke will do. */
function StatusBar() {
  const turnPhase = useUiStore((s) => s.turnPhase);
  const sessionId = useChatStore((s) => s.sessionId);
  const context = useChatStore((s) => s.context);
  const cost = useChatStore((s) => s.sessionCost);

  return (
    <footer className="flex h-7 shrink-0 items-center gap-4 border-t border-rule bg-panel px-3 font-mono text-[11px] text-faint">
      <span>session {sessionId ? sessionId.slice(0, 8) : "—"}</span>
      <span>
        context {context ? `${context.percent.toFixed(0)}%` : "—"}
      </span>
      <span>${cost.toFixed(4)}</span>
      <span className={`ml-auto ${turnPhase !== "idle" ? "text-signal" : ""}`}>
        {turnPhase === "awaiting-approval" ? "awaiting approval" : turnPhase}
      </span>
    </footer>
  );
}
