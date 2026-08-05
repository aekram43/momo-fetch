"use client";

/**
 * F22 — loading placeholders.
 *
 * Shaped like the rows they replace, so the panel does not reflow when data
 * arrives. `aria-hidden` because a screen reader gains nothing from three grey
 * bars; the surrounding region carries an accessible loading state instead.
 */
export function SkeletonRows({ rows = 3 }: { rows?: number }) {
  return (
    <div className="space-y-1.5" aria-hidden>
      {Array.from({ length: rows }, (_, i) => (
        <div
          key={i}
          className="h-3 animate-pulse rounded bg-raised"
          // Ragged widths read as content; identical bars read as a progress bar.
          style={{ width: `${90 - i * 12}%` }}
        />
      ))}
    </div>
  );
}

/** Three dots while the agent is composing its first token. */
export function TypingIndicator() {
  return (
    <span
      className="inline-flex items-center gap-1"
      role="status"
      aria-label="Agent is responding"
    >
      {[0, 1, 2].map((i) => (
        <span
          key={i}
          className="size-1 animate-pulse rounded-full bg-signal"
          style={{ animationDelay: `${i * 180}ms` }}
        />
      ))}
    </span>
  );
}
