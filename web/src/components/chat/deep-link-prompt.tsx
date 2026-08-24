"use client";

import { useEffect, useState } from "react";

import {
  onDeepLinkOpenRequest,
  onDeepLinkRejected,
  openProject,
} from "@/lib/desktop";
import { useToastStore } from "@/stores/toast-store";

/**
 * **T9** — confirmation for a `momo://open` link.
 *
 * A deep link arrives from outside the app and asks to re-root the agent: the
 * sandbox, the memory vault and the session DB all move. The shell has checked
 * the path is real and free of traversal, but *validated is not authorised* —
 * any web page can fire one of these, so a human says yes.
 *
 * Deliberately a modal, and deliberately defaulting to Cancel: this is the same
 * shape as the tool-approval dialog because it is the same kind of decision.
 */
export function DeepLinkPrompt() {
  const [path, setPath] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const push = useToastStore((s) => s.push);

  useEffect(() => onDeepLinkOpenRequest(setPath), []);
  useEffect(
    () => onDeepLinkRejected((reason) => push({ tone: "warn", message: reason })),
    [push],
  );

  if (!path) return null;

  async function accept() {
    if (!path || busy) return;
    setBusy(true);
    const opened = await openProject(path);
    // The shell says why — a link can point at a directory that is not a
    // workspace, and "could not open" would hide the one useful sentence.
    if (!opened.ok) push({ tone: "error", message: opened.error });
    setBusy(false);
    setPath(null);
  }

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-void/80 p-4">
      <div
        role="alertdialog"
        aria-modal="true"
        aria-labelledby="deeplink-title"
        className="w-full max-w-lg rounded-lg border border-signal/40 bg-panel shadow-2xl"
      >
        <div className="border-b border-rule px-5 py-4">
          <h2 id="deeplink-title" className="text-sm font-semibold text-ink">
            Open this project?
          </h2>
          <p className="mt-0.5 text-xs text-dim">
            A link asked to point the agent at a different folder.
          </p>
        </div>

        <div className="px-5 py-4">
          <pre className="overflow-x-auto rounded border border-rule bg-void px-3 py-2 font-mono text-xs text-ink">
            {path}
          </pre>
          <p className="mt-3 text-xs leading-relaxed text-dim">
            This becomes the agent&apos;s sandbox — the files it can read and
            write. Sessions and memory move with it, and the agent restarts.
            Only continue if you recognise this folder and trust where the link
            came from.
          </p>
        </div>

        <div className="flex justify-end gap-2 border-t border-rule px-5 py-3">
          <button
            type="button"
            autoFocus
            disabled={busy}
            onClick={() => setPath(null)}
            className="rounded border border-rule px-3 py-1.5 text-xs font-medium text-ink transition-colors hover:border-dim disabled:opacity-50"
          >
            Cancel
          </button>
          <button
            type="button"
            disabled={busy}
            onClick={() => void accept()}
            className="rounded bg-signal px-3 py-1.5 text-xs font-semibold text-void transition-opacity hover:opacity-90 disabled:opacity-50"
          >
            Open this folder
          </button>
        </div>
      </div>
    </div>
  );
}
