"use client";

import { useState } from "react";

import { Hint, PanelSection } from "@/components/shared/panel-section";
import { SkeletonRows } from "@/components/shared/skeleton";
import {
  ApiError,
  clearApprovedTools,
  getSettings,
  setPermissionMode,
} from "@/lib/api-client";
import { useGatewayResource } from "@/hooks/use-gateway-resource";
import { useUiStore } from "@/stores/ui-store";
import type { PermissionMode } from "@/lib/types";

const MODES: { value: PermissionMode; label: string; blurb: string }[] = [
  { value: "strict", label: "strict", blurb: "Asks before anything that changes your machine." },
  { value: "auto", label: "auto", blurb: "Asks only for destructive commands." },
  { value: "yolo", label: "yolo", blurb: "Runs everything without asking." },
];

/**
 * F17 — permission mode and sticky approvals.
 *
 * The approved-tools list is the user's only window into what they have already
 * granted. Approval is per tool *name* and lasts the life of the process
 * (spec C2), so without this panel there is no way to see or undo it.
 *
 * `yolo` gets a confirm step because it disables the confirmation boundary
 * entirely — spec §9.5.
 */
export function SettingsTab() {
  const { data, error, reload } = useGatewayResource(getSettings);
  const [busy, setBusy] = useState(false);
  const [conflict, setConflict] = useState(false);
  const [confirmYolo, setConfirmYolo] = useState(false);
  const soundEnabled = useUiStore((s) => s.soundEnabled);
  const toggleSound = useUiStore((s) => s.toggleSound);

  async function choose(mode: PermissionMode) {
    if (busy) return;
    if (mode === "yolo" && !confirmYolo) {
      setConfirmYolo(true);
      return;
    }
    setBusy(true);
    setConflict(false);
    try {
      await setPermissionMode(mode);
      reload();
    } catch (err) {
      if (err instanceof ApiError && err.isTurnConflict) setConflict(true);
    } finally {
      setBusy(false);
      setConfirmYolo(false);
    }
  }

  async function revoke() {
    if (busy) return;
    setBusy(true);
    try {
      await clearApprovedTools();
      reload();
    } finally {
      setBusy(false);
    }
  }

  return (
    <PanelSection title="Permissions">
      {error ? (
        <Hint>Could not load settings.</Hint>
      ) : !data ? (
        <SkeletonRows />
      ) : (
        <>
          <div className="mb-1.5 flex gap-1">
            {MODES.map((m) => (
              <button
                key={m.value}
                type="button"
                disabled={busy}
                onClick={() => void choose(m.value)}
                title={m.blurb}
                className={`flex-1 rounded border px-1.5 py-1 font-mono text-[10px] transition-colors disabled:opacity-50 ${
                  data.permission_mode === m.value
                    ? m.value === "yolo"
                      ? "border-halt bg-halt/15 text-halt"
                      : "border-signal/50 bg-signal/10 text-signal"
                    : "border-rule text-dim hover:text-ink"
                }`}
              >
                {m.label}
              </button>
            ))}
          </div>

          <p className="text-[10px] leading-relaxed text-faint">
            {MODES.find((m) => m.value === data.permission_mode)?.blurb}
          </p>

          {confirmYolo && (
            <div className="mt-2 rounded border border-halt/40 bg-halt/10 px-2 py-1.5">
              <p className="text-[11px] leading-relaxed text-ink">
                In <span className="font-mono">yolo</span> the agent runs shell
                commands and edits files with no confirmation. Continue?
              </p>
              <div className="mt-1.5 flex gap-1.5">
                <button
                  type="button"
                  onClick={() => setConfirmYolo(false)}
                  className="rounded border border-rule px-2 py-0.5 font-mono text-[10px] text-ink"
                >
                  cancel
                </button>
                <button
                  type="button"
                  onClick={() => void choose("yolo")}
                  className="rounded bg-halt px-2 py-0.5 font-mono text-[10px] font-semibold text-void"
                >
                  enable yolo
                </button>
              </div>
            </div>
          )}

          <div className="mt-3">
            <div className="mb-1 flex items-baseline justify-between">
              <span className="text-[10px] font-semibold uppercase tracking-[0.14em] text-dim">
                Approved tools
              </span>
              {data.approved_tools.length > 0 && (
                <button
                  type="button"
                  onClick={() => void revoke()}
                  disabled={busy}
                  className="font-mono text-[10px] text-dim hover:text-halt disabled:opacity-50"
                >
                  revoke all
                </button>
              )}
            </div>
            {data.approved_tools.length === 0 ? (
              <Hint>None granted. Every tool will ask.</Hint>
            ) : (
              <>
                <ul className="flex flex-wrap gap-1">
                  {data.approved_tools.map((t) => (
                    <li
                      key={t}
                      className="rounded border border-signal/40 bg-signal/10 px-1.5 py-0.5 font-mono text-[10px] text-signal"
                    >
                      {t}
                    </li>
                  ))}
                </ul>
                <p className="mt-1 text-[10px] leading-relaxed text-faint">
                  These run without asking for the rest of this session.
                </p>
              </>
            )}
          </div>
          {/* F27 — off by default; a cue only helps if the user asked for it. */}
          <label className="mt-3 flex cursor-pointer items-center gap-2 text-[11px] text-dim">
            <input
              type="checkbox"
              checked={soundEnabled}
              onChange={toggleSound}
              className="accent-signal"
            />
            Sound on approval, completion and errors
          </label>
        </>
      )}
      {conflict && (
        <p className="mt-1.5 text-[10px] leading-relaxed text-signal">
          A turn is running. Permission changes are refused until it finishes.
        </p>
      )}
    </PanelSection>
  );
}
