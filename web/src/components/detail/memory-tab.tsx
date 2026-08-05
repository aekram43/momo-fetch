"use client";

import { useEffect, useState } from "react";

import { Hint, PanelSection } from "@/components/shared/panel-section";
import { getMemoryStats, searchMemory } from "@/lib/api-client";
import { useGatewayResource } from "@/hooks/use-gateway-resource";
import type { MemoryResult } from "@/lib/types";

/** F15 — memory vault search, debounced at 250 ms per spec. */
export function MemoryTab() {
  const { data: stats } = useGatewayResource(getMemoryStats);
  const [query, setQuery] = useState("");
  // Results are stored *with* the query they answer. "Is a search in flight" is
  // then derived by comparing that to the current input, rather than tracked as
  // its own state — which also keeps every setState inside the async callback,
  // as React 19's set-state-in-effect rule requires.
  const [answered, setAnswered] = useState<{
    query: string;
    results: MemoryResult[];
  } | null>(null);

  const q = query.trim();
  const searching = q !== "" && answered?.query !== q;

  useEffect(() => {
    if (!q) return;
    let cancelled = false;
    const id = setTimeout(async () => {
      try {
        const res = await searchMemory(q, 10);
        if (!cancelled) setAnswered({ query: q, results: res.results });
      } catch {
        if (!cancelled) setAnswered({ query: q, results: [] });
      }
    }, 250);
    return () => {
      cancelled = true;
      clearTimeout(id);
    };
  }, [q]);

  const results = answered?.query === q ? answered.results : null;

  return (
    <PanelSection
      title="Memory"
      action={
        stats && (
          <span className="font-mono text-[10px] font-normal text-faint">
            {stats.total_memcells}
          </span>
        )
      }
    >
      <input
        value={query}
        onChange={(e) => setQuery(e.target.value)}
        placeholder="Search the vault…"
        aria-label="Search memory vault"
        className="mb-2 w-full rounded border border-rule bg-void px-2 py-1 text-xs text-ink outline-none placeholder:text-faint focus:border-dim"
      />

      {!q ? (
        <Hint>
          {stats
            ? `${stats.total_memcells} memcells · ${stats.total_events} events${
                stats.auto_search_enabled ? " · auto-search on" : ""
              }`
            : "Search past sessions and extracted knowledge."}
        </Hint>
      ) : searching ? (
        <Hint>Searching…</Hint>
      ) : !results?.length ? (
        <Hint>No matches for “{q}”.</Hint>
      ) : (
        <ul className="space-y-2">
          {results.map((r, i) => (
            <li key={`${r.path}-${i}`} className="rounded border border-rule bg-raised/40 px-2 py-1.5">
              <div className="flex items-baseline gap-1.5">
                <span className="truncate font-mono text-[11px] text-ink">{r.title}</span>
                <span className="ml-auto shrink-0 font-mono text-[10px] text-faint">
                  {r.score.toFixed(2)}
                </span>
              </div>
              <p className="mt-1 line-clamp-3 text-[11px] leading-relaxed text-dim">
                {r.preview}
              </p>
            </li>
          ))}
        </ul>
      )}
    </PanelSection>
  );
}
