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

export interface Preferences {
  sidebarOpen: boolean;
  detailOpen: boolean;
  soundEnabled: boolean;
}

export const defaultPreferences: Preferences = {
  sidebarOpen: true,
  detailOpen: true,
  // Off by default (spec F27) — an app that makes noise unasked is one the user
  // mutes at the OS level, losing the signal entirely.
  soundEnabled: false,
};

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
