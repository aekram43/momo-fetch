"use client";

import { useEffect, useState } from "react";

import { Wordmark } from "@/components/shared/wordmark";
import { getCost, getHealth } from "@/lib/api-client";
import { useChatStore } from "@/stores/chat-store";
import { useUiStore } from "@/stores/ui-store";
import type { CostSummary, Health } from "@/lib/types";

/**
 * Top rail: what the gateway currently *is*, and whether we can reach it.
 *
 * Everything here is read from the live gateway rather than from local belief.
 * Spec §2.3: the harness holds one global session and one provider, so the UI
 * must display the gateway's actual state, not what this tab last asked for.
 */
export function Header({ onToggleSidebar, onToggleDetail }: {
  onToggleSidebar: () => void;
  onToggleDetail: () => void;
}) {
  const [health, setHealth] = useState<Health | null>(null);
  const [reachable, setReachable] = useState<boolean | null>(null);
  const [cost, setCost] = useState<CostSummary | null>(null);
  // Live figure from `usage` events; falls back to the polled total between turns.
  const liveCost = useChatStore((s) => s.sessionCost);
  const localTurn = useChatStore((s) => s.turnId);
  const serverStateNonce = useUiStore((s) => s.serverStateNonce);
  const openSettings = useUiStore((s) => s.openSettings);

  // F19 — poll /health every 10 s. Auth-exempt (G12), so this works before a
  // token is entered.
  useEffect(() => {
    let cancelled = false;
    const poll = async () => {
      try {
        const [h, c] = await Promise.all([getHealth(), getCost().catch(() => null)]);
        if (!cancelled) {
          setHealth(h);
          if (c) setCost(c);
          setReachable(true);
        }
      } catch {
        if (!cancelled) setReachable(false);
      }
    };
    void poll();
    const id = setInterval(() => void poll(), 10_000);
    return () => {
      cancelled = true;
      clearInterval(id);
    };
  }, [serverStateNonce]);

  // The gateway says a turn is running but this tab isn't the one driving it —
  // another tab, or the REPL. Say so rather than looking idle (spec F19).
  const foreignTurn = Boolean(health?.turn_active) && !localTurn;

  return (
    <header className="flex h-11 shrink-0 items-center gap-3 border-b border-rule bg-panel px-3">
      <PanelButton label="Toggle sessions panel" onClick={onToggleSidebar}>
        ▤
      </PanelButton>

      <Wordmark />

      <div className="mx-1 h-4 w-px bg-rule" />

      {/* Model and provider are machine facts — monospace. */}
      <span className="truncate font-mono text-xs text-dim">
        {health?.model ?? "—"}
      </span>

      <div className="ml-auto flex items-center gap-3">
        {foreignTurn && (
          <span
            className="font-mono text-xs text-signal"
            title="A turn is running that this tab did not start"
          >
            turn active elsewhere
          </span>
        )}
        <span
          className="font-mono text-xs text-dim"
          title={
            cost
              ? `${cost.total_tokens.toLocaleString()} tokens over ${cost.request_count} requests`
              : undefined
          }
        >
          ${(liveCost || cost?.total_cost || 0).toFixed(4)}
        </span>
        <ConnectionDot reachable={reachable} />
        <PanelButton label="Settings" onClick={() => openSettings()}>
          ⚙
        </PanelButton>
        <PanelButton label="Toggle detail panel" onClick={onToggleDetail}>
          ▥
        </PanelButton>
      </div>
    </header>
  );
}

function PanelButton({
  children,
  label,
  onClick,
}: {
  children: React.ReactNode;
  label: string;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      aria-label={label}
      onClick={onClick}
      className="rounded px-1.5 py-0.5 text-dim transition-colors hover:bg-raised hover:text-ink"
    >
      {children}
    </button>
  );
}

/**
 * Connection state.
 *
 * Never colour alone (spec §6): the dot is always paired with a word, because
 * green-vs-red is the exact distinction a colour-blind user loses.
 */
function ConnectionDot({ reachable }: { reachable: boolean | null }) {
  const [color, text] =
    reachable === null
      ? ["bg-faint", "connecting"]
      : reachable
        ? ["bg-consent", "connected"]
        : ["bg-halt", "offline"];

  return (
    <span className="flex items-center gap-1.5">
      <span className={`size-1.5 rounded-full ${color}`} aria-hidden />
      <span className="font-mono text-xs text-dim">{text}</span>
    </span>
  );
}
