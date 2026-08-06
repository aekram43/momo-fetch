import { describe, expect, it } from "vitest";
import { readFileSync } from "node:fs";

import { PREFERENCES_KEY, defaultPreferences } from "./preferences";

describe("preferences", () => {
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
});
