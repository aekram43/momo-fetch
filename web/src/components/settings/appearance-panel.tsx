"use client";

import { PanelSection } from "@/components/shared/panel-section";
import { useUiStore } from "@/stores/ui-store";
import type { ThemeChoice } from "@/lib/preferences";

const THEMES: { value: ThemeChoice; label: string; blurb: string }[] = [
  { value: "dark", label: "dark", blurb: "Always dark." },
  { value: "light", label: "light", blurb: "Always light." },
  { value: "auto", label: "auto", blurb: "Follow the system setting." },
];

/**
 * Theme and sound — spec §6 / Q7, F27.
 *
 * Split out of the permissions panel, where both controls sat inside the branch
 * that renders only once `/v2/settings` has loaded. Nothing here touches the
 * gateway: if it is unreachable you could not change the theme, which is the
 * one setting that should always work.
 *
 * `auto` is the default. Someone who has already told their OS they want light
 * has told us too, and making them say it twice is the wrong default even for
 * an app whose home key is dark.
 */
export function AppearancePanel() {
  const theme = useUiStore((s) => s.theme);
  const setTheme = useUiStore((s) => s.setTheme);
  const soundEnabled = useUiStore((s) => s.soundEnabled);
  const toggleSound = useUiStore((s) => s.toggleSound);

  return (
    <PanelSection title="Appearance">
      <div role="radiogroup" aria-label="Theme" className="flex gap-1">
        {THEMES.map((t) => (
          <button
            key={t.value}
            type="button"
            role="radio"
            aria-checked={theme === t.value}
            onClick={() => setTheme(t.value)}
            title={t.blurb}
            className={`flex-1 rounded border px-1.5 py-1 font-mono text-[10px] transition-colors ${
              theme === t.value
                ? "border-signal/50 bg-signal/10 text-signal"
                : "border-rule text-dim hover:text-ink"
            }`}
          >
            {t.label}
          </button>
        ))}
      </div>
      <p className="mt-1 text-[10px] leading-relaxed text-faint">
        {THEMES.find((t) => t.value === theme)?.blurb}
      </p>

      {/* F27 — off by default; a cue only helps if the user asked for it. */}
      <label className="mt-3 flex cursor-pointer items-center gap-2 text-[11px] text-dim">
        <input
          type="checkbox"
          checked={soundEnabled}
          onChange={toggleSound}
          className="accent-signal"
        />
        Sound on approval, completion and errors
      </label>
    </PanelSection>
  );
}
