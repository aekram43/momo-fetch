"use client";

import { CollapsibleGroup } from "@/components/shared/collapsible-group";
import { AgentPicker } from "@/components/sidebar/agent-picker";
import { FilesTab } from "@/components/detail/files-tab";
import { MemoryTab } from "@/components/detail/memory-tab";
import { RoutinesSection } from "@/components/routines/routines-section";
import { SessionList } from "@/components/sidebar/session-list";
import { ToolStatus } from "@/components/sidebar/tool-status";
import { useUiStore } from "@/stores/ui-store";

/**
 * Left rail: your sessions, and everything the agent can draw on.
 *
 * The registers under "Customization" — agents, tools, routines, memory, files —
 * answer one question between them: *what does this agent have to work with?*
 * Routines belong with them: a schedule is standing instructions, which is a
 * capability the agent has whether or not anyone is at the keyboard.
 * They were split across both side panels, so the answer was in two places and
 * neither was complete. Now they are one group behind one control, which leaves
 * the right panel free to do its own job: what the agent is doing right now.
 *
 * Sections are labelled but not numbered — these are parallel registers, not a
 * sequence, and numbering would imply an order that does not exist.
 */
export function Sidebar() {
  const customizationOpen = useUiStore((s) => s.customizationOpen);
  const toggleCustomization = useUiStore((s) => s.toggleCustomization);

  return (
    <nav
      aria-label="Sessions and customization"
      className="flex w-72 shrink-0 flex-col gap-5 overflow-y-auto border-r border-rule bg-panel px-3 py-4"
    >
      <SessionList />
      <CollapsibleGroup
        title="Customization"
        open={customizationOpen}
        onToggle={toggleCustomization}
      >
        <AgentPicker />
        <ToolStatus />
        <RoutinesSection />
        <MemoryTab />
        <FilesTab />
      </CollapsibleGroup>
    </nav>
  );
}
