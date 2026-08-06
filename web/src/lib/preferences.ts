/**
 * F28 — local preferences.
 *
 * **Only view state lives here — never server state.** Panel visibility and the
 * sound toggle are this browser's business. The active session, agent, provider
 * and permission mode are *not*: the harness holds one global set of those
 * (spec §2.3, C4), so a remembered value is a belief that can be wrong the
 * moment another tab or the REPL changes it. The UI reads those from `/health`
 * and the `role` event instead — server state wins, per spec F28.
 *
 * Also never the bearer token: CORS is configurable and a token in
 * `localStorage` is readable by any script on the origin (spec §9.3).
 */

const KEY = "momo-worker.prefs.v1";

/** `auto` follows the OS; the other two pin it. */
export type ThemeChoice = "dark" | "light" | "auto";

export interface Preferences {
  sidebarOpen: boolean;
  detailOpen: boolean;
  soundEnabled: boolean;
  theme: ThemeChoice;
  /** The "Customization" group in the left rail. */
  customizationOpen: boolean;
}

export const defaultPreferences: Preferences = {
  sidebarOpen: true,
  detailOpen: true,
  // Off by default (spec F27) — an app that makes noise unasked is one the user
  // mutes at the OS level, losing the signal entirely.
  soundEnabled: false,
  // Follow the OS by default. Someone who has already told their machine they
  // want light has told us too; making them say it twice is the wrong default
  // even for an app whose home key is dark.
  theme: "auto",
  // Expanded by default. Collapsed would hide agents, tools, memory and files
  // behind a control nobody has been given a reason to click yet — a group is
  // for tidying away what you already know is there.
  customizationOpen: true,
};

/** Storage key, exported so the pre-paint script and this module cannot drift. */
export const PREFERENCES_KEY = KEY;

/** Resolve a choice to the theme actually applied right now. */
export function resolveTheme(choice: ThemeChoice): "dark" | "light" {
  if (choice !== "auto") return choice;
  if (typeof window === "undefined") return "dark";
  return window.matchMedia("(prefers-color-scheme: light)").matches
    ? "light"
    : "dark";
}

/** Stamp the resolved theme on `<html>`; the CSS keys off `data-theme`. */
export function applyTheme(choice: ThemeChoice): void {
  if (typeof document === "undefined") return;
  document.documentElement.dataset.theme = resolveTheme(choice);
}

export function loadPreferences(): Preferences {
  if (typeof window === "undefined") return defaultPreferences;
  try {
    const raw = window.localStorage.getItem(KEY);
    if (!raw) return defaultPreferences;
    const parsed = JSON.parse(raw) as Partial<Preferences>;
    // Merge rather than replace: a stored blob from an older build is missing
    // keys added since, and spreading defaults first keeps those sane.
    return { ...defaultPreferences, ...parsed };
  } catch {
    return defaultPreferences;
  }
}

export function savePreferences(prefs: Preferences): void {
  if (typeof window === "undefined") return;
  try {
    window.localStorage.setItem(KEY, JSON.stringify(prefs));
  } catch {
    // Private browsing, quota, or storage disabled. Preferences are a
    // convenience — never fail a render over them.
  }
}
