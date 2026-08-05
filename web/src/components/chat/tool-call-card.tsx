"use client";

import { useState } from "react";

import type { ToolCall, ToolCallStatus } from "@/stores/chat-store";

/**
 * One tool call (F8).
 *
 * Status is never colour alone — each state pairs a border colour with a glyph
 * and a word, because the amber/green distinction is exactly the one a
 * colour-blind user loses (spec §6).
 */
export function ToolCallCard({ call }: { call: ToolCall }) {
  const [open, setOpen] = useState(false);
  const s = STATES[call.status];

  const args =
    call.args == null
      ? null
      : typeof call.args === "object"
        ? JSON.stringify(call.args, null, 2)
        : String(call.args);

  return (
    <div className={`my-2 overflow-hidden rounded border-l-2 bg-raised/50 ${s.border}`}>
      <button
        type="button"
        onClick={() => setOpen((v) => !v)}
        aria-expanded={open}
        className="flex w-full items-center gap-2 px-3 py-2 text-left transition-colors hover:bg-raised"
      >
        <span className={`font-mono text-xs ${s.text}`} aria-hidden>
          {s.glyph}
        </span>
        {/* Tool name is machine speech. */}
        <code className="font-mono text-xs text-ink">{call.name}</code>
        <span className={`font-mono text-[10px] ${s.text}`}>{s.label}</span>
        <span className="ml-auto font-mono text-[10px] text-faint">
          {open ? "−" : "+"}
        </span>
      </button>

      {open && (
        <div className="space-y-2 border-t border-rule px-3 py-2">
          {args && (
            <Field label="Arguments">
              <pre className="max-h-40 overflow-auto font-mono text-[11px] leading-relaxed text-dim">
                {args}
              </pre>
            </Field>
          )}
          {call.preview !== null && (
            <Field label={call.status === "error" ? "Error" : "Result"}>
              <pre className="max-h-48 overflow-auto font-mono text-[11px] leading-relaxed text-dim">
                {call.preview}
              </pre>
              {call.truncated && (
                <p className="mt-1 text-[10px] text-faint">
                  Preview truncated at 2 KB. Open the file to see all of it.
                </p>
              )}
            </Field>
          )}
          {call.status === "unresolved" && (
            <p className="text-[11px] leading-relaxed text-faint">
              This call was superseded before it returned — usually because an
              approval restarted the turn. The result, if any, is on the call
              that followed it.
            </p>
          )}
        </div>
      )}
    </div>
  );
}

function Field({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div>
      <p className="mb-1 text-[10px] font-semibold uppercase tracking-[0.14em] text-faint">
        {label}
      </p>
      {children}
    </div>
  );
}

const STATES: Record<
  ToolCallStatus,
  { border: string; text: string; glyph: string; label: string }
> = {
  running: {
    border: "border-l-signal",
    text: "text-signal",
    glyph: "◐",
    label: "running",
  },
  awaiting: {
    border: "border-l-signal",
    text: "text-signal",
    glyph: "⏸",
    label: "needs approval",
  },
  done: {
    border: "border-l-consent",
    text: "text-consent",
    glyph: "✓",
    label: "done",
  },
  error: {
    border: "border-l-halt",
    text: "text-halt",
    glyph: "✕",
    label: "error",
  },
  unresolved: {
    border: "border-l-faint",
    text: "text-faint",
    glyph: "○",
    label: "superseded",
  },
};
