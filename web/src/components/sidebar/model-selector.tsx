"use client";

import { useState } from "react";

import { Dot, Hint, PanelSection } from "@/components/shared/panel-section";
import { SkeletonRows } from "@/components/shared/skeleton";
import { ApiError, getProviders, switchProvider } from "@/lib/api-client";
import { useGatewayResource } from "@/hooks/use-gateway-resource";

/**
 * Why a provider can't be selected.
 *
 * Ollama needs no API key — it is unavailable when nothing is listening on its
 * port, which is a different problem with a different fix. Telling the user to
 * set an `OLLAMA_API_KEY` would send them looking for something that does not
 * exist.
 */
function unavailableReason(name: string): string {
  return name === "ollama"
    ? "Not running. Start Ollama, or set OLLAMA_HOST."
    : `No API key. Set ${name.toUpperCase()}_API_KEY in .env`;
}

/**
 * F13 — provider and model.
 *
 * **Unavailable providers are shown, greyed, with the reason.** The gateway
 * returns them (`list_all`, spec G3) precisely so the UI can explain *why* a
 * provider is not selectable instead of silently omitting it — "where did
 * Anthropic go?" is a worse experience than "Anthropic: no API key".
 */
export function ModelSelector() {
  const { data, error, reload } = useGatewayResource(getProviders);
  const [busy, setBusy] = useState(false);
  const [conflict, setConflict] = useState(false);

  async function choose(name: string) {
    if (busy) return;
    setBusy(true);
    setConflict(false);
    try {
      await switchProvider(name);
      reload();
    } catch (err) {
      if (err instanceof ApiError && err.isTurnConflict) setConflict(true);
    } finally {
      setBusy(false);
    }
  }

  return (
    <PanelSection title="Models">
      {error ? (
        <Hint>Could not load providers.</Hint>
      ) : !data ? (
        <SkeletonRows />
      ) : (
        <ul className="-mx-1 space-y-px">
          {data.providers.map((p) => (
            <li key={p.name}>
              <button
                type="button"
                disabled={!p.available || busy}
                onClick={() => void choose(p.name)}
                title={
                  p.available
                    ? (p.current_model ?? undefined)
                    : unavailableReason(p.name)
                }
                className={`flex w-full items-center gap-1.5 rounded px-1.5 py-1 text-left font-mono text-[11px] transition-colors ${
                  p.is_current
                    ? "bg-raised text-signal"
                    : p.available
                      ? "text-dim hover:bg-raised hover:text-ink"
                      : "cursor-not-allowed text-faint"
                }`}
              >
                <Dot tone={p.is_current ? "warn" : p.available ? "ok" : "off"} />
                <span className="truncate">{p.name}</span>
                {!p.available && (
                  <span className="ml-auto text-[9px] text-faint">
                    {p.name === "ollama" ? "offline" : "no key"}
                  </span>
                )}
              </button>
            </li>
          ))}
        </ul>
      )}
      {data?.providers.some((p) => !p.available && p.name !== "ollama") && (
        <p className="mt-1.5 text-[10px] leading-relaxed text-faint">
          Greyed providers need a key. Add one in{" "}
          <span className="text-dim">API keys</span>, or set it in{" "}
          <code className="font-mono">.env</code>.
        </p>
      )}
      {conflict && (
        <p className="mt-1.5 text-[10px] leading-relaxed text-signal">
          A turn is running. Model changes are refused until it finishes.
        </p>
      )}
    </PanelSection>
  );
}
