"use client";

/** Shared chrome for the sidebar and detail-panel sections. */
export function PanelSection({
  title,
  action,
  children,
}: {
  title: string;
  action?: React.ReactNode;
  children: React.ReactNode;
}) {
  return (
    <section>
      <h2 className="mb-2 flex items-baseline justify-between text-[11px] font-semibold uppercase tracking-[0.14em] text-dim">
        {title}
        {action}
      </h2>
      {children}
    </section>
  );
}

export function Hint({ children }: { children: React.ReactNode }) {
  return <p className="text-xs leading-relaxed text-faint">{children}</p>;
}

/**
 * A status dot is never the only carrier of meaning — callers pair it with a
 * label (spec §6). This renders the dot alone; the label is the caller's job.
 */
export function Dot({ tone }: { tone: "ok" | "warn" | "bad" | "off" }) {
  const color = {
    ok: "bg-consent",
    warn: "bg-signal",
    bad: "bg-halt",
    off: "bg-faint",
  }[tone];
  return <span className={`size-1.5 shrink-0 rounded-full ${color}`} aria-hidden />;
}
