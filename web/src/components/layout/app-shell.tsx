"use client";

import { ApprovalDialog } from "@/components/chat/approval-dialog";
import { ChatPanel } from "@/components/chat/chat-panel";
import { DetailPanel } from "@/components/detail/detail-panel";
import { Header } from "@/components/layout/header";
import { Sidebar } from "@/components/layout/sidebar";
import { TurnRail } from "@/components/shared/turn-rail";
import { useShortcuts } from "@/hooks/use-shortcuts";
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
  const { sidebarOpen, detailOpen, turnPhase, toggleSidebar, toggleDetail } =
    useUiStore();
  useShortcuts();

  return (
    <div className="flex h-full flex-col">
      <Header onToggleSidebar={toggleSidebar} onToggleDetail={toggleDetail} />

      <div className="flex min-h-0 flex-1">
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
      </div>

      <StatusBar />

      {/* Modal at every breakpoint, outside every collapsible panel — a narrow
          viewport must never be able to strand a turn (spec §6). */}
      <ApprovalDialog />
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
