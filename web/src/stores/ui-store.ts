import { create } from "zustand";

import {
  applyTheme,
  defaultPreferences,
  loadPreferences,
  savePreferences,
  type ThemeChoice,
} from "@/lib/preferences";

/**
 * Panel visibility, turn phase, and the sound toggle.
 *
 * `turnPhase` lives here rather than in the chat store because the shell itself
 * reacts to it — the turn rail and the status bar both read it, and neither owns
 * the conversation.
 */
export type TurnPhase = "idle" | "running" | "awaiting-approval";

/**
 * Which drawer is open on a narrow viewport.
 *
 * Panels are *columns* on desktop — two can be visible at once — but on mobile
 * they are drawers overlaying the chat, and two open at once buries the
 * composer entirely. So mobile gets its own single-slot state rather than
 * reusing the desktop booleans.
 */
export type MobileDrawer = "sessions" | "detail" | null;

interface UiState {
  sidebarOpen: boolean;
  detailOpen: boolean;
  soundEnabled: boolean;
  theme: ThemeChoice;
  turnPhase: TurnPhase;
  mobileDrawer: MobileDrawer;
  /**
   * Bumped whenever this tab changes something the gateway owns.
   *
   * Panels poll on their own schedule — the header every 10 s — which is fine
   * for drift but wrong right after a deliberate action: switch a model and the
   * header keeps showing the old one for up to ten seconds, which reads as the
   * switch having failed. Every reader of server state watches this and
   * refetches at once.
   */
  serverStateNonce: number;
  /** True once stored preferences have been applied (F28). */
  hydrated: boolean;
  toggleSidebar: () => void;
  toggleDetail: () => void;
  openDrawer: (which: MobileDrawer) => void;
  /** Call after any successful mutation of gateway state. */
  bumpServerState: () => void;
  toggleSound: () => void;
  setTheme: (theme: ThemeChoice) => void;
  setTurnPhase: (phase: TurnPhase) => void;
  hydrate: () => void;
}

/** Persist just the view state — see the note in `lib/preferences.ts`. */
function persist(
  s: Pick<UiState, "sidebarOpen" | "detailOpen" | "soundEnabled" | "theme">,
) {
  savePreferences({
    sidebarOpen: s.sidebarOpen,
    detailOpen: s.detailOpen,
    soundEnabled: s.soundEnabled,
    theme: s.theme,
  });
}

export const useUiStore = create<UiState>((set, get) => ({
  // Start from the defaults, not from localStorage: reading storage during
  // module evaluation would diverge from the server-rendered HTML and trip
  // hydration. `hydrate()` applies the stored values after mount.
  ...defaultPreferences,
  turnPhase: "idle",
  // Always starts closed: a drawer the user did not open should never be
  // covering the chat on load.
  mobileDrawer: null,
  serverStateNonce: 0,
  hydrated: false,

  toggleSidebar: () => {
    set((s) => ({ sidebarOpen: !s.sidebarOpen }));
    persist(get());
  },
  toggleDetail: () => {
    set((s) => ({ detailOpen: !s.detailOpen }));
    persist(get());
  },
  toggleSound: () => {
    set((s) => ({ soundEnabled: !s.soundEnabled }));
    persist(get());
  },
  setTheme: (theme) => {
    set({ theme });
    applyTheme(theme);
    persist(get());
  },
  // Opening one drawer closes the other — there is only room for one.
  openDrawer: (mobileDrawer) =>
    set((s) => ({
      mobileDrawer: s.mobileDrawer === mobileDrawer ? null : mobileDrawer,
    })),
  bumpServerState: () => set((s) => ({ serverStateNonce: s.serverStateNonce + 1 })),
  setTurnPhase: (turnPhase) => set({ turnPhase }),
  hydrate: () => {
    if (get().hydrated) return;
    const prefs = loadPreferences();
    set({ ...prefs, hydrated: true });
    // The pre-paint script in `layout.tsx` already stamped this; re-applying is
    // cheap and keeps the store the single source of truth afterwards.
    applyTheme(prefs.theme);

    // Follow the OS while the choice is `auto`. Without this, "auto" only means
    // "whatever the OS said at load" and a machine that switches at sunset
    // leaves the app on the wrong theme until reload.
    if (typeof window !== "undefined") {
      const mq = window.matchMedia("(prefers-color-scheme: light)");
      mq.addEventListener("change", () => {
        if (get().theme === "auto") applyTheme("auto");
      });
    }
  },
}));
