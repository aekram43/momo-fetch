"use client";

import { useState } from "react";

import { Hint, PanelSection } from "@/components/shared/panel-section";
import { SkeletonRows } from "@/components/shared/skeleton";
import {
  ApiError,
  getAgents,
  resetToDefaultAgent,
  switchAgent,
} from "@/lib/api-client";
import { useGatewayResource } from "@/hooks/use-gateway-resource";

/**
 * F12 — agent personalities.
 *
 * Switching rebuilds the runner under a write lock, so the gateway 409s it while
 * a turn is active. That is surfaced inline rather than swallowed: the user
 * needs to know the click did nothing and why.
 */
export function AgentPicker() {
  const { data, error, reload } = useGatewayResource(getAgents);
  const [busy, setBusy] = useState(false);
  const [conflict, setConflict] = useState(false);

  async function choose(name: string | null) {
    if (busy) return;
    setBusy(true);
    setConflict(false);
    try {
      await (name === null ? resetToDefaultAgent() : switchAgent(name));
      reload();
    } catch (err) {
      if (err instanceof ApiError && err.isTurnConflict) setConflict(true);
    } finally {
      setBusy(false);
    }
  }

  return (
    <PanelSection title="Agents">
      {error ? (
        <Hint>Could not load agents.</Hint>
      ) : !data ? (
        <SkeletonRows />
      ) : data.agents.length === 0 ? (
        <Hint>
          None configured. Add one to{" "}
          <code className="font-mono text-[11px] text-dim">.harness/agents/</code>.
        </Hint>
      ) : (
        <ul className="-mx-1 space-y-px">
          <li>
            <Row
              active={data.current === null}
              onClick={() => void choose(null)}
              disabled={busy}
              label="default"
            />
          </li>
          {data.agents.map((a) => (
            <li key={a.name}>
              <Row
                active={a.is_current}
                onClick={() => void choose(a.name)}
                disabled={busy}
                label={a.name}
                // Capabilities as a title rather than a custom tooltip: it works
                // with keyboard focus and screen readers for free.
                title={
                  [a.description, a.capabilities.join(", "), a.model]
                    .filter(Boolean)
                    .join(" · ") || undefined
                }
                badge={a.is_orchestrator ? "orch" : undefined}
              />
            </li>
          ))}
        </ul>
      )}
      {conflict && (
        <p className="mt-1.5 text-[10px] leading-relaxed text-signal">
          A turn is running. Agent changes are refused until it finishes.
        </p>
      )}
    </PanelSection>
  );
}

function Row({
  active,
  onClick,
  disabled,
  label,
  title,
  badge,
}: {
  active: boolean;
  onClick: () => void;
  disabled: boolean;
  label: string;
  title?: string;
  badge?: string;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      disabled={disabled}
      title={title}
      className={`flex w-full items-center gap-1.5 rounded px-1.5 py-1 text-left font-mono text-[11px] transition-colors disabled:opacity-50 ${
        active ? "bg-raised text-signal" : "text-dim hover:bg-raised hover:text-ink"
      }`}
    >
      {active && <span aria-hidden>▸</span>}
      <span className="truncate">{label}</span>
      {badge && (
        <span className="ml-auto text-[9px] uppercase text-faint">{badge}</span>
      )}
    </button>
  );
}
