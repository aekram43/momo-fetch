"use client";

/**
 * Right panel: what the agent did this turn, and what it can reach.
 *
 * "This turn" is deliberately the first and largest section. It is the answer to
 * the question this product exists to answer — what is the agent doing to my
 * machine right now.
 *
 * Note on Artifacts (spec §6): there is no artifact-tracking API. The tab is
 * derived client-side from write-shaped tool calls seen during the turn, so it
 * is turn-scoped and lost on reload unless rebuilt from G8. The copy says so
 * rather than implying a filesystem diff.
 */
export function DetailPanel() {
  return (
    <aside
      aria-label="Turn details"
      className="flex w-80 shrink-0 flex-col gap-5 overflow-y-auto border-l border-rule bg-panel px-3 py-4"
    >
      <Section title="This turn" hint="F8">
        <Empty>Tool calls appear here as the agent makes them.</Empty>
      </Section>

      <Section title="Artifacts" hint="F8">
        <Empty>
          Files written this turn. Turn-scoped — reconstructed from history on
          reload.
        </Empty>
      </Section>

      <Section title="Memory" hint="F15">
        <Empty>Search the vault.</Empty>
      </Section>

      <Section title="Files" hint="F16">
        <Empty>Browse the sandbox.</Empty>
      </Section>
    </aside>
  );
}

function Section({
  title,
  hint,
  children,
}: {
  title: string;
  hint: string;
  children: React.ReactNode;
}) {
  return (
    <section>
      <h2 className="mb-2 flex items-baseline justify-between text-[11px] font-semibold uppercase tracking-[0.14em] text-dim">
        {title}
        <span className="font-mono text-[10px] font-normal text-faint">
          {hint}
        </span>
      </h2>
      {children}
    </section>
  );
}

function Empty({ children }: { children: React.ReactNode }) {
  return <p className="text-xs leading-relaxed text-faint">{children}</p>;
}
