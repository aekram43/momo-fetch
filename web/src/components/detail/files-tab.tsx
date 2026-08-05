"use client";

import { useState } from "react";

import { Hint, PanelSection } from "@/components/shared/panel-section";
import { isDesktop, openProject } from "@/lib/desktop";
import { SkeletonRows } from "@/components/shared/skeleton";
import { ApiError, readFile, readTree } from "@/lib/api-client";
import { useGatewayResource } from "@/hooks/use-gateway-resource";
import { useToastStore } from "@/stores/toast-store";
import type { FileContent } from "@/lib/types";

/**
 * F16 — sandbox file browser.
 *
 * `binary` and `truncated` are rendered explicitly rather than glossed: a user
 * looking at 1 MiB of a 5 MB log needs to know they are not seeing all of it,
 * and a binary file should say so instead of showing replacement characters.
 *
 * A 403 means "not accessible" and deliberately does not distinguish missing
 * from forbidden — the gateway refuses to be a filesystem oracle (G6), and the
 * UI must not invent that distinction back.
 */
export function FilesTab() {
  const [dir, setDir] = useState(".");
  const { data: tree, error } = useGatewayResource(() => readTree(dir, 1), [dir]);
  const [preview, setPreview] = useState<FileContent | null>(null);
  const [previewError, setPreviewError] = useState<string | null>(null);
  const [confirmRoot, setConfirmRoot] = useState(false);
  const push = useToastStore((s) => s.push);

  /**
   * T4 — re-root the whole harness at another directory.
   *
   * Behind a confirm because it moves the sandbox, the memory vault and the
   * session DB at once, and the gateway restarts under it. Desktop only: there
   * is no browser equivalent of choosing a folder on the host.
   */
  async function reroot() {
    const path = window.prompt("Project directory to open:");
    if (!path) return;
    const url = await openProject(path);
    if (url) {
      push({ tone: "info", message: "Reopening at the new project…" });
    } else {
      push({ tone: "error", message: "Could not open that directory." });
    }
    setConfirmRoot(false);
  }

  async function open(path: string) {
    setPreview(null);
    setPreviewError(null);
    try {
      setPreview(await readFile(path));
    } catch (err) {
      setPreviewError(
        err instanceof ApiError && err.status === 403
          ? "Not accessible."
          : "Could not read that file.",
      );
    }
  }

  const parent = dir === "." ? null : dir.split("/").slice(0, -1).join("/") || ".";

  return (
    <PanelSection
      title="Files"
      action={
        parent !== null && (
          <button
            type="button"
            onClick={() => setDir(parent)}
            className="font-mono text-[10px] font-normal text-dim hover:text-ink"
          >
            ↑ up
          </button>
        )
      }
    >
      <p className="mb-1.5 truncate font-mono text-[10px] text-faint">{dir}</p>

      {isDesktop() && (
        <div className="mb-2">
          {confirmRoot ? (
            <div className="rounded border border-signal/40 bg-signal/10 px-2 py-1.5">
              <p className="text-[11px] leading-relaxed text-ink">
                Opening another project moves the sandbox, memory vault and
                sessions, and restarts the agent. Continue?
              </p>
              <div className="mt-1.5 flex gap-1.5">
                <button
                  type="button"
                  onClick={() => setConfirmRoot(false)}
                  className="rounded border border-rule px-2 py-0.5 font-mono text-[10px] text-ink"
                >
                  cancel
                </button>
                <button
                  type="button"
                  onClick={() => void reroot()}
                  className="rounded bg-signal px-2 py-0.5 font-mono text-[10px] font-semibold text-void"
                >
                  choose folder
                </button>
              </div>
            </div>
          ) : (
            <button
              type="button"
              onClick={() => setConfirmRoot(true)}
              className="font-mono text-[10px] text-dim transition-colors hover:text-ink"
            >
              open another project…
            </button>
          )}
        </div>
      )}

      {error ? (
        <Hint>Could not list that directory.</Hint>
      ) : !tree ? (
        <SkeletonRows />
      ) : (
        <ul className="-mx-1 max-h-52 space-y-px overflow-y-auto">
          {tree.entries.map((e) => (
            <li key={e.path}>
              <button
                type="button"
                onClick={() => (e.is_dir ? setDir(e.path) : void open(e.path))}
                className="flex w-full items-center gap-1.5 rounded px-1.5 py-0.5 text-left font-mono text-[11px] text-dim transition-colors hover:bg-raised hover:text-ink"
              >
                <span className="text-faint" aria-hidden>
                  {e.is_dir ? "▸" : "·"}
                </span>
                <span className="truncate">{e.name}</span>
                {!e.is_dir && e.size !== null && (
                  <span className="ml-auto shrink-0 text-[10px] text-faint">
                    {formatSize(e.size)}
                  </span>
                )}
              </button>
            </li>
          ))}
        </ul>
      )}

      {tree?.truncated && (
        <p className="mt-1 text-[10px] text-faint">
          Listing capped at 1000 entries.
        </p>
      )}

      {previewError && <p className="mt-2 text-[11px] text-halt">{previewError}</p>}

      {preview && (
        <div className="mt-2 rounded border border-rule">
          <div className="flex items-baseline gap-2 border-b border-rule px-2 py-1">
            <span className="truncate font-mono text-[10px] text-ink">
              {preview.path}
            </span>
            <span className="ml-auto shrink-0 font-mono text-[10px] text-faint">
              {formatSize(preview.size)}
            </span>
          </div>
          {preview.binary ? (
            <p className="px-2 py-2 text-[11px] text-dim">
              Binary file — not shown.
            </p>
          ) : (
            <>
              <pre className="max-h-56 overflow-auto px-2 py-1.5 font-mono text-[11px] leading-relaxed text-dim">
                {preview.content}
              </pre>
              {preview.truncated && (
                <p className="border-t border-rule px-2 py-1 text-[10px] text-signal">
                  Showing the first 1 MiB of {formatSize(preview.size)}.
                </p>
              )}
            </>
          )}
        </div>
      )}
    </PanelSection>
  );
}

function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(0)} KB`;
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
}
