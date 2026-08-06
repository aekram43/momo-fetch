"use client";

import { useState } from "react";

import { ModelPicker } from "@/components/sidebar/model-picker";
import { Hint, PanelSection } from "@/components/shared/panel-section";
import { SkeletonRows } from "@/components/shared/skeleton";
import { ApiError, getProviders, switchProvider } from "@/lib/api-client";
import { useUiStore } from "@/stores/ui-store";
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

/** The short suffix shown inside the option itself. */
function unavailableTag(name: string): string {
  return name === "ollama" ? "offline" : "no key";
}

/**
 * F13 — provider, then model.
 *
 * Two dependent choices, so two controls stacked in the order they are made:
 * pick the provider, then pick a model *from that provider*. The panel used to
 * list every provider as a row with the model picker nested under whichever one
 * was active, which put the two decisions at different levels and let the list
 * grow taller than the choice deserved.
 *
 * **The two controls are deliberately different.** Providers are a fixed handful
 * — a native `<select>` is the right control, and it gets keyboard behaviour and
 * screen-reader semantics for free. Models are several hundred, where a select
 * is a scroll bar with no way in, so that one is a filterable list. The control
 * follows the size of the set, not a wish for symmetry.
 *
 * **Unavailable providers stay in the list, disabled, labelled with why.** The
 * gateway returns them (`list_all`, spec G3) precisely so the UI can explain
 * itself — "where did Anthropic go?" is worse than "anthropic — no key".
 */
export function ModelSelector() {
  const { data, error, reload } = useGatewayResource(getProviders);
  const [busy, setBusy] = useState(false);
  const bump = useUiStore((s) => s.bumpServerState);
  const [conflict, setConflict] = useState(false);

  async function choose(name: string) {
    if (busy) return;
    setBusy(true);
    setConflict(false);
    try {
      await switchProvider(name);
      bump();
      reload();
    } catch (err) {
      if (err instanceof ApiError && err.isTurnConflict) setConflict(true);
    } finally {
      setBusy(false);
    }
  }

  const active = data?.providers.find((p) => p.is_current) ?? null;

  return (
    <PanelSection title="Models">
      {error ? (
        <Hint>Could not load providers.</Hint>
      ) : !data ? (
        <SkeletonRows />
      ) : (
        <>
          <label className="mb-0.5 block font-mono text-[9px] uppercase tracking-[0.14em] text-faint">
            provider
          </label>
          <div className="relative">
            <select
              value={active?.name ?? ""}
              disabled={busy}
              onChange={(e) => void choose(e.target.value)}
              aria-label="Provider"
              className="w-full appearance-none truncate rounded border border-rule bg-void py-1 pl-1.5 pr-5 font-mono text-[11px] text-ink outline-none focus-visible:border-signal disabled:opacity-50"
            >
              {data.providers.map((p) => (
                <option
                  key={p.name}
                  value={p.name}
                  disabled={!p.available}
                  title={p.available ? undefined : unavailableReason(p.name)}
                >
                  {p.available ? p.name : `${p.name} — ${unavailableTag(p.name)}`}
                </option>
              ))}
            </select>
            {/* The native arrow is suppressed above so the control matches the
                model trigger below; this replaces it. */}
            <span
              aria-hidden
              className="pointer-events-none absolute right-1.5 top-1/2 -translate-y-1/2 font-mono text-[10px] text-faint"
            >
              ▾
            </span>
          </div>

          <label className="mb-0.5 mt-1.5 block font-mono text-[9px] uppercase tracking-[0.14em] text-faint">
            model
          </label>
          {active ? (
            <ModelPicker provider={active.name} current={active.current_model} />
          ) : (
            <Hint>Choose a provider first.</Hint>
          )}
        </>
      )}
      {data?.providers.some((p) => !p.available && p.name !== "ollama") && (
        <p className="mt-1.5 text-[10px] leading-relaxed text-faint">
          Providers without a key are listed but not selectable. Add one in{" "}
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
