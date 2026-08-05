"use client";

import { AgentPicker } from "@/components/sidebar/agent-picker";
import { ModelSelector } from "@/components/sidebar/model-selector";
import { SessionList } from "@/components/sidebar/session-list";
import { ToolStatus } from "@/components/sidebar/tool-status";

/**
 * Left rail: sessions, agents, models, tools.
 *
 * Sections are labelled but not numbered — these are parallel registers, not a
 * sequence, and numbering would imply an order that does not exist.
 */
export function Sidebar() {
  return (
    <nav
      aria-label="Sessions, agents, models and tools"
      className="flex w-56 shrink-0 flex-col gap-5 overflow-y-auto border-r border-rule bg-panel px-3 py-4"
    >
      <SessionList />
      <AgentPicker />
      <ModelSelector />
      <ToolStatus />
    </nav>
  );
}
