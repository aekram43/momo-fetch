"use client";

import type { TurnPhase } from "@/stores/ui-store";

/**
 * The signature element: a thin vertical strip on the chat panel's leading edge
 * that reports the turn lifecycle.
 *
 * The turn lifecycle is this product's actual core — one turn at a time,
 * process-wide, sometimes parked waiting for you — and it is otherwise
 * invisible. The rail makes it physical: dark when idle, a travelling amber
 * segment while the agent works, and a full pulsing column when it is blocked on
 * your decision.
 *
 * It carries no text, so it is paired with `aria-label` and a status role rather
 * than being left as decoration for screen readers.
 */
export function TurnRail({ phase }: { phase: TurnPhase }) {
  const label =
    phase === "awaiting-approval"
      ? "Waiting for your approval"
      : phase === "running"
        ? "Agent is working"
        : "Idle";

  return (
    <div
      role="status"
      aria-label={label}
      className="relative w-[3px] shrink-0 overflow-hidden bg-rule/40"
    >
      {phase === "awaiting-approval" && (
        <div className="awaiting absolute inset-0 bg-signal" />
      )}
      {phase === "running" && (
        <div className="absolute inset-x-0 h-1/3 animate-[travel_1.8s_ease-in-out_infinite] bg-signal" />
      )}
      <style>{`
        @keyframes travel {
          0%   { top: -33%; }
          100% { top: 100%; }
        }
      `}</style>
    </div>
  );
}
