"use client";

import { useCallback, useEffect, useState } from "react";

import { Hint, PanelSection } from "@/components/shared/panel-section";
import {
  deleteSecret,
  getSecretStatus,
  isDesktop,
  setSecret,
  type SecretStatus,
} from "@/lib/desktop";
import { useToastStore } from "@/stores/toast-store";

/**
 * API keys, for people who would rather not edit a dotfile.
 *
 * Desktop only — it needs the shell to reach the OS keychain, and there is no
 * browser equivalent. In a browser it says so and points at `.env`, rather than
 * rendering nothing: as a section in a panel an absence was fine, but as a tab
 * in the settings dialog an empty pane is a dead end for the one person who
 * came looking for where to put a key.
 *
 * Two rules this component exists to honour:
 *
 * 1. **A key is write-only.** There is no command to read one back, so the UI
 *    shows "set" or "not set" and nothing else. A secret that cannot be read
 *    out of the app cannot be lifted by anything that reaches the app.
 * 2. **Say when a key overrides `.env`.** The shell injects stored keys into the
 *    gateway's environment at spawn, so they beat the workspace `.env`. Without
 *    saying so, "I have a key in .env and the app is using a different one" is
 *    unexplainable.
 */
export function ApiKeysPanel() {
  const [rows, setRows] = useState<SecretStatus[] | null>(null);
  const [editing, setEditing] = useState<string | null>(null);
  const [value, setValue] = useState("");
  const [busy, setBusy] = useState(false);
  const push = useToastStore((s) => s.push);

  const refresh = useCallback(async () => {
    const s = await getSecretStatus();
    if (s) setRows(s);
  }, []);

  useEffect(() => {
    if (!isDesktop()) return;
    let cancelled = false;
    void (async () => {
      const s = await getSecretStatus();
      if (!cancelled && s) setRows(s);
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  if (!isDesktop()) {
    return (
      <PanelSection title="API keys">
        <Hint>
          Keychain storage needs the desktop app — a browser has no way to reach
          the OS keychain. Set keys in the workspace{" "}
          <code className="font-mono">.env</code> instead, then restart the
          gateway.
        </Hint>
      </PanelSection>
    );
  }

  async function save(provider: string) {
    if (busy || !value.trim()) return;
    setBusy(true);
    const err = await setSecret(provider, value);
    setBusy(false);
    // Clear immediately either way — no reason to keep a key in component state.
    setValue("");
    setEditing(null);
    if (err) {
      push({ tone: "error", message: err });
      return;
    }
    push({ tone: "info", message: `Saved. Restarting with the new ${provider} key…` });
    void refresh();
  }

  async function remove(provider: string) {
    if (busy) return;
    setBusy(true);
    const err = await deleteSecret(provider);
    setBusy(false);
    if (err) push({ tone: "error", message: err });
    else void refresh();
  }

  return (
    <PanelSection title="API keys">
      {!rows ? (
        <Hint>Loading…</Hint>
      ) : (
        <ul className="space-y-1.5">
          {rows.map((r) => (
            <li key={r.provider}>
              <div className="flex items-center gap-1.5">
                <span
                  className={`size-1.5 shrink-0 rounded-full ${
                    r.configured ? "bg-consent" : "bg-faint"
                  }`}
                  aria-hidden
                />
                <span className="font-mono text-[11px] text-dim">{r.provider}</span>
                <span className="ml-auto flex items-center gap-1.5">
                  <span className="font-mono text-[10px] text-faint">
                    {r.configured ? "set" : "not set"}
                  </span>
                  <button
                    type="button"
                    disabled={busy}
                    onClick={() =>
                      setEditing(editing === r.provider ? null : r.provider)
                    }
                    className="font-mono text-[10px] text-dim transition-colors hover:text-ink disabled:opacity-40"
                  >
                    {r.configured ? "replace" : "add"}
                  </button>
                  {r.configured && (
                    <button
                      type="button"
                      disabled={busy}
                      onClick={() => void remove(r.provider)}
                      aria-label={`Remove the ${r.provider} key`}
                      className="font-mono text-[10px] text-faint transition-colors hover:text-halt disabled:opacity-40"
                    >
                      ×
                    </button>
                  )}
                </span>
              </div>

              {editing === r.provider && (
                <div className="mt-1 flex gap-1">
                  <input
                    type="password"
                    autoFocus
                    value={value}
                    onChange={(e) => setValue(e.target.value)}
                    onKeyDown={(e) => e.key === "Enter" && void save(r.provider)}
                    placeholder={r.env_var}
                    aria-label={`${r.provider} API key`}
                    className="min-w-0 flex-1 rounded border border-rule bg-void px-2 py-1 font-mono text-[11px] text-ink outline-none placeholder:text-faint focus:border-dim"
                  />
                  <button
                    type="button"
                    disabled={busy || !value.trim()}
                    onClick={() => void save(r.provider)}
                    className="shrink-0 rounded bg-signal px-2 py-1 font-mono text-[10px] font-semibold text-void disabled:opacity-40"
                  >
                    save
                  </button>
                </div>
              )}

              {r.configured && r.also_in_env_file && (
                <p className="mt-1 text-[10px] leading-relaxed text-signal">
                  Your <code className="font-mono">.env</code> also sets{" "}
                  <code className="font-mono">{r.env_var}</code>. The key stored
                  here is the one being used.
                </p>
              )}
            </li>
          ))}
        </ul>
      )}
      <p className="mt-2 text-[10px] leading-relaxed text-faint">
        Stored in the OS keychain. Saving restarts the agent so the new key takes
        effect. Keys cannot be read back out of the app.
      </p>
    </PanelSection>
  );
}
