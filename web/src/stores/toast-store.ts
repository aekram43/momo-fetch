import { create } from "zustand";

export type ToastTone = "error" | "warn" | "info";

export interface Toast {
  id: number;
  tone: ToastTone;
  message: string;
  /** Optional single action, e.g. "Retry" on an idempotent GET. */
  action?: { label: string; run: () => void };
}

interface ToastState {
  toasts: Toast[];
  push: (t: Omit<Toast, "id">) => void;
  dismiss: (id: number) => void;
}

let seq = 0;

export const useToastStore = create<ToastState>((set) => ({
  toasts: [],
  push: (t) =>
    set((s) => {
      const id = ++seq;
      // Cap the stack. A burst of failures should not bury the composer.
      const toasts = [...s.toasts, { ...t, id }].slice(-4);
      // Errors stay until dismissed — they usually need a decision. Anything
      // else self-clears.
      if (t.tone !== "error") {
        setTimeout(() => {
          set((cur) => ({ toasts: cur.toasts.filter((x) => x.id !== id) }));
        }, 5000);
      }
      return { toasts };
    }),
  dismiss: (id) =>
    set((s) => ({ toasts: s.toasts.filter((t) => t.id !== id) })),
}));

/**
 * Turn any thrown value into a toast.
 *
 * Errors carry the gateway's `code` (spec §8), so callers get a message written
 * for the situation rather than a generic "something went wrong".
 */
export function toastError(err: unknown, fallback = "Something failed."): void {
  const message =
    err && typeof err === "object" && "message" in err
      ? String((err as Error).message)
      : fallback;
  useToastStore.getState().push({ tone: "error", message });
}
