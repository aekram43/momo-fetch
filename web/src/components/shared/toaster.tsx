"use client";

import { useToastStore } from "@/stores/toast-store";

/**
 * F21 — surfaced failures.
 *
 * Errors are announced politely (`role="status"`, `aria-live="polite"`) rather
 * than assertively: a turn can fail while the user is mid-sentence in the
 * composer, and an assertive live region interrupts a screen reader on every
 * one.
 *
 * There is no automatic retry anywhere in this app. Re-sending a turn
 * double-bills and can re-run tools that already executed, so a retry action is
 * only ever offered for idempotent GETs, by the caller that knows it is safe.
 */
export function Toaster() {
  const toasts = useToastStore((s) => s.toasts);
  const dismiss = useToastStore((s) => s.dismiss);

  if (toasts.length === 0) return null;

  return (
    <div
      role="status"
      aria-live="polite"
      className="pointer-events-none fixed bottom-10 left-1/2 z-40 flex w-full max-w-md -translate-x-1/2 flex-col gap-1.5 px-4"
    >
      {toasts.map((t) => (
        <div
          key={t.id}
          className={`pointer-events-auto flex items-start gap-2 rounded border px-3 py-2 shadow-lg backdrop-blur ${
            t.tone === "error"
              ? "border-halt/40 bg-halt/15"
              : t.tone === "warn"
                ? "border-signal/40 bg-signal/15"
                : "border-rule bg-raised"
          }`}
        >
          <span
            className={`mt-1 size-1.5 shrink-0 rounded-full ${
              t.tone === "error"
                ? "bg-halt"
                : t.tone === "warn"
                  ? "bg-signal"
                  : "bg-dim"
            }`}
            aria-hidden
          />
          <p className="min-w-0 flex-1 text-xs leading-relaxed text-ink">
            {t.message}
          </p>
          {t.action && (
            <button
              type="button"
              onClick={() => {
                t.action?.run();
                dismiss(t.id);
              }}
              className="shrink-0 font-mono text-[11px] text-dim transition-colors hover:text-ink"
            >
              {t.action.label}
            </button>
          )}
          <button
            type="button"
            onClick={() => dismiss(t.id)}
            aria-label="Dismiss"
            className="shrink-0 font-mono text-[11px] text-faint transition-colors hover:text-ink"
          >
            ×
          </button>
        </div>
      ))}
    </div>
  );
}
