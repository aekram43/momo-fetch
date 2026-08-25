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

/** Which section of the settings dialog is showing. */
export type SettingsTab = "models" | "api-keys" | "permissions" | "appearance";

interface UiState {
  sidebarOpen: boolean;
  detailOpen: boolean;
  soundEnabled: boolean;
  theme: ThemeChoice;
  turnPhase: TurnPhase;
  mobileDrawer: MobileDrawer;
  /** The Customization group in the left rail (F28 view state). */
  customizationOpen: boolean;
  /**
   * The routines dialog, and which routine it should land on.
   *
   * A modal like settings, and for the same reason — but it carries a focus
   * id so the sidebar rows can deep-link to one routine instead of dropping
   * the user on an empty pane they then have to search.
   */
  routinesOpen: boolean;
  routinesFocusId: string | null;
  /**
   * The settings dialog.
   *
   * Configuration is a modal rather than a panel because it is a place you go,
   * finish, and leave. Left in a column it competes for width with the work,
   * and it is the part of the UI a user touches least.
   */
  settingsOpen: boolean;
  settingsTab: SettingsTab;
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
  toggleCustomization: () => void;
  openRoutines: (focusId?: string | null) => void;
  closeRoutines: () => void;
  openSettings: (tab?: SettingsTab) => void;
  closeSettings: () => void;
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
  s: Pick<
    UiState,
    "sidebarOpen" | "detailOpen" | "soundEnabled" | "theme" | "customizationOpen"
  >,
) {
  savePreferences({
    sidebarOpen: s.sidebarOpen,
    detailOpen: s.detailOpen,
    soundEnabled: s.soundEnabled,
    theme: s.theme,
    customizationOpen: s.customizationOpen,
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
  // Never open on load: a dialog the user did not ask for is in the way.
  routinesOpen: false,
  routinesFocusId: null,
  settingsOpen: false,
  settingsTab: "models",
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
  toggleCustomization: () => {
    set((s) => ({ customizationOpen: !s.customizationOpen }));
    persist(get());
  },
  // Opening with a tab is how the deep links work — "add a key" from the model
  // panel should land on API keys, not on whatever was open last time.
  openRoutines: (focusId = null) => set({ routinesOpen: true, routinesFocusId: focusId }),
  // The focus id is cleared on close so the next plain open starts neutral
  // rather than on whatever was last clicked.
  closeRoutines: () => set({ routinesOpen: false, routinesFocusId: null }),
  openSettings: (tab) =>
    set((s) => ({ settingsOpen: true, settingsTab: tab ?? s.settingsTab })),
  closeSettings: () => set({ settingsOpen: false }),
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
