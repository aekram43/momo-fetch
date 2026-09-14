"use client";

import { useEffect, useState } from "react";

import { getHealth } from "@/lib/api-client";
import { useUiStore } from "@/stores/ui-store";

/**
 * No provider can serve a turn: no API key is set, and no Ollama is running.
 *
 * The gateway starts anyway (refusing to start left nowhere to put the key), so
 * this is where the user finds out. A key for any one provider is enough: when
 * the configured provider has none, the gateway uses the first one that does.
 */
export function NoLlmBanner() {
  const [ready, setReady] = useState(true);
  const serverStateNonce = useUiStore((s) => s.serverStateNonce);
  const openSettings = useUiStore((s) => s.openSettings);

  useEffect(() => {
    let cancelled = false;
    getHealth()
      .then((h) => {
        // Absent on older gateways: assume ready rather than nag.
        if (!cancelled) setReady(h.llm_ready !== false);
      })
      .catch(() => undefined);
    return () => {
      cancelled = true;
    };
  }, [serverStateNonce]);

  if (ready) return null;

  return (
    <div className="flex items-center gap-3 border-t border-signal/30 bg-signal/10 px-4 py-2.5">
      <span className="size-1.5 shrink-0 rounded-full bg-signal" aria-hidden />
      <p className="text-xs text-ink">
        No LLM is set up. Add an API key for at least one provider.
      </p>
      <button
        type="button"
        onClick={() => openSettings("api-keys")}
        className="ml-auto rounded border border-signal/50 px-2 py-1 font-mono text-[11px] text-signal transition-colors hover:bg-signal/15"
      >
        add a key
      </button>
    </div>
  );
}
