"use client";

import { getSettings } from "@/lib/api-client";
import { useGatewayResource } from "@/hooks/use-gateway-resource";
import { useUiStore } from "@/stores/ui-store";

/**
 * What the agent can already do without asking — in the status bar, always.
 *
 * Approval is per tool *name* and lasts the life of the process (spec C2), so
 * one "Approve" on `shell_exec` means every later shell command runs silently.
 * That is a standing grant, and a standing grant the user cannot see is the
 * thing most likely to surprise them. Until now it was visible only inside the
 * settings dialog, which is exactly where nobody is looking while they work.
 *
 * **The status bar, not the detail panel.** The detail panel collapses with
 * Cmd+J and becomes an overlay below 1280px, so a permissions indicator living
 * there disappears in both cases — and it is worth least when it is hidden. The
 * status bar is present at every width and already carries the facts that
 * change what the next keystroke does.
 *
 * `yolo` outranks the count and says so persistently, which spec §9.5 requires
 * and nothing implemented until now: in yolo the approved list is irrelevant
 * because *nothing* asks.
 */
export function StandingPermissions() {
  const { data } = useGatewayResource(getSettings);
  const openSettings = useUiStore((s) => s.openSettings);

  if (!data) return null;

  const yolo = data.permission_mode === "yolo";
  const count = data.approved_tools.length;
  if (!yolo && count === 0) return null;

  return (
    <button
      type="button"
      onClick={() => openSettings("permissions")}
      title={
        yolo
          ? "yolo: every tool runs without asking. Click to change."
          : `Runs without asking: ${data.approved_tools.join(", ")}. Click to review or revoke.`
      }
      className={`flex items-center gap-1.5 rounded px-1.5 transition-colors hover:bg-raised ${
        yolo ? "text-halt" : "text-signal"
      }`}
    >
      <span
        className={`size-1.5 rounded-full ${yolo ? "bg-halt" : "bg-signal"}`}
        aria-hidden
      />
      {/* Never colour alone (spec §6) — the words carry it. */}
      {yolo ? "yolo · nothing asks" : `${count} auto-run`}
    </button>
  );
}
