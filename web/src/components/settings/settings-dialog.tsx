"use client";

import { useEffect, useRef } from "react";

import { ApiKeysPanel } from "@/components/settings/api-keys-panel";
import { AppearancePanel } from "@/components/settings/appearance-panel";
import { ModelPanel } from "@/components/settings/model-panel";
import { PermissionsPanel } from "@/components/settings/permissions-panel";
import { useChatStore } from "@/stores/chat-store";
import { useUiStore, type SettingsTab } from "@/stores/ui-store";

const TABS: { id: SettingsTab; label: string; blurb: string }[] = [
  { id: "models", label: "Models", blurb: "Which provider and model answer." },
  { id: "api-keys", label: "API keys", blurb: "Stored in the OS keychain." },
  { id: "permissions", label: "Permissions", blurb: "What runs without asking." },
  { id: "appearance", label: "Appearance", blurb: "Theme and sound." },
];

/**
 * Settings — models, API keys, permissions, appearance.
 *
 * These four used to be sections in the two side panels, competing for width
 * with the work and pushing the things you actually watch during a turn below
 * the fold. They are a place you go, change one thing, and leave, so they are a
 * modal: opened from the header, Cmd+, or the native menu, and closed with
 * Escape.
 *
 * A tab rail rather than one long sheet because the four have nothing to do
 * with each other — nobody scrolls from a provider list to a theme switch, and
 * a rail lets other parts of the UI deep-link to the one that matters ("add a
 * key" from the model panel opens API keys).
 *
 * **Escape closes this, and must not also interrupt the turn.** The global
 * Escape binding reaches the harness (G13); a user dismissing a dialog is not
 * asking to stop the agent, so this handler runs first and stops propagation.
 * The approval dialog still outranks both — it owns Escape as "deny".
 */
export function SettingsDialog() {
  const open = useUiStore((s) => s.settingsOpen);
  const tab = useUiStore((s) => s.settingsTab);
  const openSettings = useUiStore((s) => s.openSettings);
  const close = useUiStore((s) => s.closeSettings);
  const pendingApproval = useChatStore((s) => s.pendingApproval);
  const dialogRef = useRef<HTMLDivElement>(null);
  const closeRef = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    if (open) closeRef.current?.focus();
  }, [open]);

  // Focus trap + Escape.
  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      // The approval dialog is on top and owns the keyboard while it is up.
      if (useChatStore.getState().pendingApproval) return;
      if (e.key === "Escape") {
        e.preventDefault();
        e.stopPropagation();
        close();
        return;
      }
      if (e.key !== "Tab") return;
      const focusable = dialogRef.current?.querySelectorAll<HTMLElement>(
        "button:not([disabled]), [href], input:not([disabled]), select:not([disabled]), [tabindex]:not([tabindex='-1'])",
      );
      if (!focusable?.length) return;
      const first = focusable[0];
      const last = focusable[focusable.length - 1];
      if (e.shiftKey && document.activeElement === first) {
        e.preventDefault();
        last.focus();
      } else if (!e.shiftKey && document.activeElement === last) {
        e.preventDefault();
        first.focus();
      }
    };
    // Capture phase: the global shortcut handler listens on `window`, and this
    // has to win before Escape becomes "interrupt the turn".
    document.addEventListener("keydown", onKey, true);
    return () => document.removeEventListener("keydown", onKey, true);
  }, [open, close]);

  if (!open) return null;

  return (
    <div
      className={`fixed inset-0 z-40 flex items-center justify-center bg-void/80 p-4 ${
        // An approval outranks settings: get out of its way rather than
        // stacking two modals.
        pendingApproval ? "pointer-events-none opacity-0" : ""
      }`}
      role="presentation"
    >
      <button
        type="button"
        aria-label="Close settings"
        onClick={close}
        className="absolute inset-0 cursor-default"
        tabIndex={-1}
      />
      <div
        ref={dialogRef}
        role="dialog"
        aria-modal="true"
        aria-labelledby="settings-title"
        className="relative flex h-[26rem] max-h-full w-full max-w-2xl overflow-hidden rounded-lg border border-rule bg-panel shadow-2xl"
      >
        <nav
          aria-label="Settings sections"
          className="flex w-40 shrink-0 flex-col gap-px border-r border-rule bg-void/40 p-2"
        >
          <h2
            id="settings-title"
            className="mb-1 px-1.5 py-1 text-[11px] font-semibold uppercase tracking-[0.14em] text-dim"
          >
            Settings
          </h2>
          {TABS.map((t) => (
            <button
              key={t.id}
              type="button"
              onClick={() => openSettings(t.id)}
              aria-current={tab === t.id ? "page" : undefined}
              title={t.blurb}
              className={`rounded px-1.5 py-1 text-left font-mono text-[11px] transition-colors ${
                tab === t.id
                  ? "bg-raised text-signal"
                  : "text-dim hover:bg-raised hover:text-ink"
              }`}
            >
              {t.label}
            </button>
          ))}
        </nav>

        <div className="min-w-0 flex-1 overflow-y-auto px-4 py-3">
          {tab === "models" && <ModelPanel />}
          {tab === "api-keys" && <ApiKeysPanel />}
          {tab === "permissions" && <PermissionsPanel />}
          {tab === "appearance" && <AppearancePanel />}
        </div>

        <button
          ref={closeRef}
          type="button"
          onClick={close}
          className="absolute right-2 top-2 rounded px-1.5 py-0.5 font-mono text-[11px] text-faint transition-colors hover:bg-raised hover:text-ink"
        >
          close
        </button>
      </div>
    </div>
  );
}
