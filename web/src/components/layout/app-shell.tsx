"use client";

import { DetailPanel } from "@/components/detail/detail-panel";
import { Header } from "@/components/layout/header";
import { Sidebar } from "@/components/layout/sidebar";
import { TurnRail } from "@/components/shared/turn-rail";
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
          <ChatPlaceholder />
        </main>

        {detailOpen && (
          <div className="hidden xl:flex">
            <DetailPanel />
          </div>
        )}
      </div>

      <StatusBar />
    </div>
  );
}

/**
 * Stands in for F6/F7. It states what is not built rather than faking a
 * conversation — a shell with mock messages hides exactly the integration
 * problems this scaffold exists to expose.
 */
function ChatPlaceholder() {
  return (
    <div className="flex min-w-0 flex-1 flex-col">
      <div className="flex flex-1 items-center justify-center px-6">
        <div className="max-w-sm text-center">
          <p className="font-mono text-xs uppercase tracking-[0.14em] text-faint">
            Shell only
          </p>
          <h1 className="mt-2 text-lg font-medium text-ink">
            The conversation lands here
          </h1>
          <p className="mt-2 text-sm leading-relaxed text-dim">
            The gateway is complete and streaming. What is missing is the chat
            panel, tool-call cards and the approval dialog — F6 through F9.
          </p>
        </div>
      </div>

      <div className="border-t border-rule px-4 py-3">
        <div className="flex items-center gap-2 rounded border border-rule bg-raised px-3 py-2">
          <span className="font-mono text-sm text-faint" aria-hidden>
            ›
          </span>
          <input
            disabled
            placeholder="Composer arrives with F6"
            aria-label="Message input (not yet implemented)"
            className="min-w-0 flex-1 bg-transparent text-sm text-ink outline-none placeholder:text-faint disabled:cursor-not-allowed"
          />
        </div>
      </div>
    </div>
  );
}

/** Bottom rail: the three facts that change what a keystroke will do. */
function StatusBar() {
  const turnPhase = useUiStore((s) => s.turnPhase);

  return (
    <footer className="flex h-7 shrink-0 items-center gap-4 border-t border-rule bg-panel px-3 font-mono text-[11px] text-faint">
      <span>permission —</span>
      <span>session —</span>
      <span>context —</span>
      <span className="ml-auto">
        {turnPhase === "idle" ? "idle" : turnPhase}
      </span>
    </footer>
  );
}
