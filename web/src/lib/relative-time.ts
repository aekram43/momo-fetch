/**
 * "in 4h", "3m ago" — the only thing a schedule row has room for.
 *
 * Absolute timestamps are the truth and stay in the `title` attribute; a list
 * of routines is read to answer "what is about to happen", and an ISO string
 * makes the reader do the subtraction themselves.
 */

const MINUTE = 60_000;
const HOUR = 60 * MINUTE;
const DAY = 24 * HOUR;

/** Coarse distance between two instants — never more precise than it can be. */
export function formatDistance(ms: number): string {
  const abs = Math.abs(ms);
  if (abs < 45_000) return "now";
  if (abs < HOUR) return `${Math.round(abs / MINUTE)}m`;
  if (abs < DAY) return `${Math.round(abs / HOUR)}h`;
  return `${Math.round(abs / DAY)}d`;
}

/**
 * `null` in, `null` out: a routine with no next window has nothing to say, and
 * an em dash decided here would stop the caller choosing its own wording.
 */
export function formatRelative(iso: string | null, now = Date.now()): string | null {
  if (!iso) return null;
  const then = Date.parse(iso);
  if (Number.isNaN(then)) return null;
  const delta = then - now;
  if (Math.abs(delta) < 45_000) return "now";
  return delta > 0 ? `in ${formatDistance(delta)}` : `${formatDistance(delta)} ago`;
}
