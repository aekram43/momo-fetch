"use client";

import { Hint, PanelSection } from "@/components/shared/panel-section";
import { ToolCallCard } from "@/components/chat/tool-call-card";
import { useChatStore } from "@/stores/chat-store";

/**
 * Right panel: what the agent is doing, right now.
 *
 * Only that. Memory, files and the settings sections used to live here too, and
 * they pushed the live turn — the one thing this product exists to show — into
 * a column shared with configuration. Memory and files moved to the left rail's
 * Customization group; models, keys, permissions and appearance moved into the
 * settings dialog.
 */
export function DetailPanel() {
  const messages = useChatStore((s) => s.messages);
  const tokens = useChatStore((s) => s.turnTokens);

  // Tool calls from the trailing assistant message = the current turn.
  const last = [...messages].reverse().find((m) => m.role === "assistant");
  const calls = last?.toolCalls ?? [];

  // Two sources, deliberately. The gateway compares the project tree before and
  // after the turn, which catches every file however it was written — a shell
  // heredoc, `sed -i`, a formatter — but only inside the sandbox root, and only
  // once the turn ends. Write-shaped tool calls fill both gaps: they appear as
  // the agent makes them, and they cover writes that land outside the project,
  // like `mem_write` into the vault.
  const written = useChatStore((s) => s.turnArtifacts);
  const fromCalls = calls
    .filter((c) => /write|edit|create/i.test(c.name))
    .map((c) => ({ key: c.id, path: pathOf(c.args) ?? c.name, change: null as string | null }));

  const seen = new Set(written.map((f) => f.path));
  const artifacts = [
    ...written.map((f) => ({ key: f.path, path: f.path, change: f.change })),
    // A path the diff already reported is the same file, better described.
    ...fromCalls.filter((c) => !seen.has(c.path)),
  ];

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
          <Hint>Files this turn changed show up here.</Hint>
        ) : (
          <>
            <ul className="space-y-0.5">
              {artifacts.map((a) => (
                <li key={a.key} className="flex items-baseline gap-1.5">
                  <span className="truncate font-mono text-[11px] text-dim">{a.path}</span>
                  {a.change && (
                    <span className="ml-auto shrink-0 font-mono text-[10px] text-faint">
                      {a.change === "created" ? "new" : a.change === "deleted" ? "del" : "mod"}
                    </span>
                  )}
                </li>
              ))}
            </ul>
          </>
        )}
      </PanelSection>

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
