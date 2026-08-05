import { create } from "zustand";

import {
  defaultPreferences,
  loadPreferences,
  savePreferences,
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
  turnPhase: TurnPhase;
  mobileDrawer: MobileDrawer;
  /** True once stored preferences have been applied (F28). */
  hydrated: boolean;
  toggleSidebar: () => void;
  toggleDetail: () => void;
  openDrawer: (which: MobileDrawer) => void;
  toggleSound: () => void;
  setTurnPhase: (phase: TurnPhase) => void;
  hydrate: () => void;
}

/** Persist just the view state — see the note in `lib/preferences.ts`. */
function persist(s: Pick<UiState, "sidebarOpen" | "detailOpen" | "soundEnabled">) {
  savePreferences({
    sidebarOpen: s.sidebarOpen,
    detailOpen: s.detailOpen,
    soundEnabled: s.soundEnabled,
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
  // Opening one drawer closes the other — there is only room for one.
  openDrawer: (mobileDrawer) =>
    set((s) => ({
      mobileDrawer: s.mobileDrawer === mobileDrawer ? null : mobileDrawer,
    })),
  setTurnPhase: (turnPhase) => set({ turnPhase }),
  hydrate: () => {
    if (get().hydrated) return;
    set({ ...loadPreferences(), hydrated: true });
  },
}));
