import { create } from "zustand";

/**
 * Panel visibility and turn phase.
 *
 * `turnPhase` lives here rather than in the chat store because the shell itself
 * reacts to it — the turn rail and the status bar both read it, and neither owns
 * the conversation.
 */
export type TurnPhase = "idle" | "running" | "awaiting-approval";

interface UiState {
  sidebarOpen: boolean;
  detailOpen: boolean;
  turnPhase: TurnPhase;
  toggleSidebar: () => void;
  toggleDetail: () => void;
  setTurnPhase: (phase: TurnPhase) => void;
}

export const useUiStore = create<UiState>((set) => ({
  sidebarOpen: true,
  detailOpen: true,
  turnPhase: "idle",
  toggleSidebar: () => set((s) => ({ sidebarOpen: !s.sidebarOpen })),
  toggleDetail: () => set((s) => ({ detailOpen: !s.detailOpen })),
  setTurnPhase: (turnPhase) => set({ turnPhase }),
}));
