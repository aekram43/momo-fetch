"use client";

import { CollapsibleGroup } from "@/components/shared/collapsible-group";
import { AgentPicker } from "@/components/sidebar/agent-picker";
import { FilesTab } from "@/components/detail/files-tab";
import { MemoryTab } from "@/components/detail/memory-tab";
import { RoutinesSection } from "@/components/routines/routines-section";
import { SessionList } from "@/components/sidebar/session-list";
import { TeamSection } from "@/components/sidebar/team-section";
import { ToolStatus } from "@/components/sidebar/tool-status";
import { useUiStore } from "@/stores/ui-store";

/**
 * Left rail: your sessions, who is working, and what they have to work with.
 *
 * Two groups, because the registers answer two different questions and mixing
 * them made both harder to read:
 *
 * - **Squad** — *who is doing the work.* The agent this session is talking to,
 *   the team of workers running beside it, and the routines that hand out work
 *   when nobody is here. All three are actors with a schedule and a state.
 * - **Customization** — *what they have to work with.* Tools, memory, files.
 *   Inert capabilities; none of them does anything on its own.
 *
 * Agents used to sit with tools and files, which put "who is answering me" next
 * to "which MCP servers are up". Routines then made the mismatch obvious: a
 * schedule is not a capability, it is a member of the squad that happens to be
 * asleep.
 *
 * Sections within a group are labelled but not numbered — they are parallel
 * registers, not a sequence.
 */
export function Sidebar() {
  const squadOpen = useUiStore((s) => s.squadOpen);
  const toggleSquad = useUiStore((s) => s.toggleSquad);
  const customizationOpen = useUiStore((s) => s.customizationOpen);
  const toggleCustomization = useUiStore((s) => s.toggleCustomization);

  return (
    <nav
      aria-label="Sessions, squad and customization"
      className="flex w-72 shrink-0 flex-col gap-5 overflow-y-auto border-r border-rule bg-panel px-3 py-4"
    >
      <SessionList />
      <CollapsibleGroup title="Squad" open={squadOpen} onToggle={toggleSquad}>
        <AgentPicker />
        <TeamSection />
        <RoutinesSection />
      </CollapsibleGroup>
      <CollapsibleGroup
        title="Customization"
        open={customizationOpen}
        onToggle={toggleCustomization}
      >
        <ToolStatus />
        <MemoryTab />
        <FilesTab />
      </CollapsibleGroup>
    </nav>
  );
}
