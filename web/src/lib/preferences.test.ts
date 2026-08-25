import { afterEach, describe, expect, it, vi } from "vitest";
import { readFileSync } from "node:fs";

import { PREFERENCES_KEY, defaultPreferences, loadPreferences } from "./preferences";

describe("preferences", () => {
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  /**
   * The theme is applied before first paint by an inline script in
   * `layout.tsx`, which cannot import from this module — it has to run before
   * the bundle loads. So the storage key is written out twice. If they ever
   * diverge the app silently ignores the stored theme and flashes dark on every
   * launch, which is the failure this whole feature exists to avoid.
   */
  it("the pre-paint script reads the same storage key this module writes", () => {
    const layout = readFileSync("src/app/layout.tsx", "utf8");
    expect(layout).toContain(`localStorage.getItem('${PREFERENCES_KEY}')`);
  });

  it("the pre-paint script understands the shipped default", () => {
    const layout = readFileSync("src/app/layout.tsx", "utf8");
    // Whatever the default is, the script must fall back to it when nothing is
    // stored — otherwise a first launch disagrees with the store.
    expect(layout).toContain(`|| '${defaultPreferences.theme}'`);
    expect(layout).toContain(`: '${defaultPreferences.theme}'`);
  });

  /**
   * A blob written by an older build is missing every key added since. Reading
   * it must fill those in rather than leave them `undefined` — an undefined
   * `squadOpen` renders the group collapsed, so someone who used the app
   * yesterday would open it today to a rail with half its contents gone.
   */
  it("an older stored blob gains the keys added since", () => {
    // These tests run in node, not jsdom — a two-method `localStorage` is all
    // `loadPreferences` touches, and it beats pulling in a DOM for one read.
    const stored = JSON.stringify({ theme: "light", sidebarOpen: false });
    vi.stubGlobal("window", {
      localStorage: { getItem: (k: string) => (k === PREFERENCES_KEY ? stored : null) },
    });

    const prefs = loadPreferences();
    expect(prefs.theme).toBe("light");
    expect(prefs.sidebarOpen).toBe(false);
    expect(prefs.squadOpen).toBe(defaultPreferences.squadOpen);
    expect(prefs.customizationOpen).toBe(defaultPreferences.customizationOpen);
  });
});
