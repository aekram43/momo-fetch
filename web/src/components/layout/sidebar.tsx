"use client";

/**
 * Left rail: sessions, agents, tools.
 *
 * Sections are labelled but not numbered — these are three parallel registers,
 * not a sequence, and numbering them would imply an order that does not exist.
 *
 * Every section here is an empty state until its task lands (F10 sessions, F12
 * agents, F14 tools). Each says what it will hold and how to fill it, rather
 * than rendering a spinner for something nobody has built yet.
 */
export function Sidebar() {
  return (
    <nav
      aria-label="Sessions, agents and tools"
      className="flex w-56 shrink-0 flex-col gap-5 overflow-y-auto border-r border-rule bg-panel px-3 py-4"
    >
      <Section title="Sessions" hint="F10">
        <Empty>No sessions yet. Start one to begin.</Empty>
      </Section>

      <Section title="Agents" hint="F12">
        <Empty>
          Personalities from <Path>.harness/agents/</Path>.
        </Empty>
      </Section>

      <Section title="Tools" hint="F14">
        <Empty>MCP server status appears here.</Empty>
      </Section>
    </nav>
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
        {/* Scaffold marker: which spec task fills this in. Removed as each lands. */}
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

function Path({ children }: { children: React.ReactNode }) {
  return <code className="font-mono text-[11px] text-dim">{children}</code>;
}
