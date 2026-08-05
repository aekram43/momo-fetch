"use client";

import { FilesTab } from "@/components/detail/files-tab";
import { MemoryTab } from "@/components/detail/memory-tab";
import { SettingsTab } from "@/components/detail/settings-tab";
import { Hint, PanelSection } from "@/components/shared/panel-section";
import { ToolCallCard } from "@/components/chat/tool-call-card";
import { useChatStore } from "@/stores/chat-store";

/**
 * Right panel: what the agent did this turn, and what it can reach.
 *
 * "This turn" is first and largest deliberately — it answers the question this
 * product exists to answer: what is the agent doing to my machine right now.
 */
export function DetailPanel() {
  const messages = useChatStore((s) => s.messages);
  const tokens = useChatStore((s) => s.turnTokens);

  // Tool calls from the trailing assistant message = the current turn.
  const last = [...messages].reverse().find((m) => m.role === "assistant");
  const calls = last?.toolCalls ?? [];

  // Artifacts are derived client-side from write-shaped tool calls; there is no
  // artifact API (spec §6). Turn-scoped, and the copy says so.
  const artifacts = calls.filter((c) => /write|edit|create/i.test(c.name));

  return (
    <aside
      aria-label="Turn details"
      className="flex w-80 shrink-0 flex-col gap-5 overflow-y-auto border-l border-rule bg-panel px-3 py-4"
    >
      <PanelSection
        title="This turn"
        action={
          (tokens.prompt > 0 || tokens.completion > 0) && (
            <span className="font-mono text-[10px] font-normal text-faint">
              {tokens.prompt}↑ {tokens.completion}↓
            </span>
          )
        }
      >
        {calls.length === 0 ? (
          <Hint>Tool calls appear here as the agent makes them.</Hint>
        ) : (
          <div className="-my-2">
            {calls.map((c) => (
              <ToolCallCard key={c.id} call={c} />
            ))}
          </div>
        )}
      </PanelSection>

      <PanelSection title="Artifacts">
        {artifacts.length === 0 ? (
          <Hint>Files written this turn show up here.</Hint>
        ) : (
          <>
            <ul className="space-y-0.5">
              {artifacts.map((a) => (
                <li key={a.id} className="truncate font-mono text-[11px] text-dim">
                  {pathOf(a.args) ?? a.name}
                </li>
              ))}
            </ul>
            <p className="mt-1 text-[10px] leading-relaxed text-faint">
              Derived from this turn&apos;s tool calls, not a filesystem diff.
            </p>
          </>
        )}
      </PanelSection>

      <MemoryTab />
      <FilesTab />
      <SettingsTab />
    </aside>
  );
}

function pathOf(args: unknown): string | null {
  if (args && typeof args === "object" && "path" in args) {
    const p = (args as { path?: unknown }).path;
    if (typeof p === "string") return p;
  }
  return null;
}
