"use client";

/**
 * A named group of panel sections that can be folded away.
 *
 * Used twice in the left rail — "Squad" (who does the work) and
 * "Customization" (what they have to work with). Between them they are the
 * tallest thing in the rail, so each gets one heading and one control instead
 * of six sections a user has to scroll past to reach the session list.
 *
 * The heading is a real `<button>` with `aria-expanded`, not a styled `div`:
 * folding away several panels is exactly the kind of control that must be
 * reachable by keyboard and announced as a disclosure.
 */
export function CollapsibleGroup({
  title,
  open,
  onToggle,
  children,
}: {
  title: string;
  open: boolean;
  onToggle: () => void;
  children: React.ReactNode;
}) {
  return (
    <section>
      <button
        type="button"
        onClick={onToggle}
        aria-expanded={open}
        className="mb-2 flex w-full items-center gap-1.5 border-b border-rule pb-1.5 text-[11px] font-semibold uppercase tracking-[0.14em] text-ink transition-colors hover:text-signal"
      >
        <span
          aria-hidden
          className={`font-mono text-[9px] text-faint transition-transform ${
            open ? "rotate-90" : ""
          }`}
        >
          ▸
        </span>
        {title}
      </button>
      {/* Unmounted, not hidden: the panels inside poll the gateway, and a
          collapsed group should stop asking rather than keep fetching for a
          view nobody is looking at. */}
      {/* Indented against a hairline so the sections read as *inside* the
          group. Without it the group heading looks like one more sibling, since
          every section heading in this rail is set the same way. */}
      {open && (
        <div className="flex flex-col gap-5 border-l border-rule pl-2.5">
          {children}
        </div>
      )}
    </section>
  );
}
