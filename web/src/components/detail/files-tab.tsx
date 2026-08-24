"use client";

import { useState } from "react";

import { Hint, PanelSection } from "@/components/shared/panel-section";
import {
  createWorkspace,
  isDesktop,
  openProject,
  pickDirectory,
  recentWorkspaces,
} from "@/lib/desktop";
import { SkeletonRows } from "@/components/shared/skeleton";
import { ApiError, getSettings, readFile, readTree } from "@/lib/api-client";
import { projectName } from "@/lib/project-name";
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
  const { data: tree, error } = useGatewayResource(
    () => readTree(dir, 1),
    [dir],
  );
  // Only for the label. The tree API addresses the root as ".", which is the
  // right key and a useless name — settings is what knows where the project is.
  const { data: settings } = useGatewayResource(getSettings);
  const [preview, setPreview] = useState<FileContent | null>(null);
  const [previewError, setPreviewError] = useState<string | null>(null);
  // null = neither action started. Opening warns first; creating needs a
  // parent directory and a name before it can do anything.
  const [rootAction, setRootAction] = useState<"open" | "create" | null>(null);
  const [newParent, setNewParent] = useState<string | null>(null);
  const [newName, setNewName] = useState("");
  const [busy, setBusy] = useState(false);
  const [recent, setRecent] = useState<string[]>([]);
  const push = useToastStore((s) => s.push);

  /** Loaded when the switch panel opens, not on mount: it costs an IPC call
   *  and nobody needs the list until they are looking at it. */
  function beginOpen() {
    setRootAction("open");
    if (isDesktop()) void recentWorkspaces().then(setRecent);
  }

  /** Re-root at a path the user already trusts — a recent one, or a picked one. */
  async function openAt(path: string) {
    setBusy(true);
    const opened = await openProject(path);
    setBusy(false);
    if (opened.ok) {
      push({ tone: "info", message: "Reopening at the new workspace…" });
    } else {
      push({ tone: "error", message: opened.error });
    }
    setRootAction(null);
  }

  /**
   * T4 — re-root the whole harness at another workspace.
   *
   * Behind a confirm because it moves the sandbox, the memory vault and the
   * session DB at once, and the gateway restarts under it. Desktop only: there
   * is no browser equivalent of choosing a folder on the host.
   *
   * The shell refuses a directory that is not already a workspace, and its
   * refusal is shown verbatim — it names the missing file and points at the
   * other button, which is more than this component could work out.
   */
  async function chooseWorkspace() {
    // The button says "choose folder", so it has to open one.
    //
    // It used to call `window.prompt` — which asks you to *type* an absolute
    // path, not choose a folder, and which does nothing at all here. wry's
    // `WKUIDelegate` implements only `runOpenPanelWithParameters`, media
    // capture and new-window handling; it has no
    // `runJavaScriptTextInputPanelWithPrompt`, and WebKit silently returns null
    // for a prompt the delegate does not handle. So on macOS the button was
    // dead: click it, nothing appears, nothing happens, no error.
    const path = await pick();
    if (!path) return;
    await openAt(path);
  }

  /** Create a workspace under the chosen directory, then open it. */
  async function create() {
    if (!newParent) return;

    setBusy(true);
    const created = await createWorkspace(newParent, newName);
    setBusy(false);
    if (created.ok) {
      push({ tone: "info", message: `Created ${newName}. Opening…` });
      setRootAction(null);
      setNewParent(null);
      setNewName("");
    } else {
      // Kept open with the name still typed: every failure here — a name with
      // a slash, a folder that is already a workspace — is one edit away from
      // working.
      push({ tone: "error", message: created.error });
    }
  }

  /** The native folder picker, with its own failure reported. */
  async function pick(): Promise<string | null> {
    try {
      return await pickDirectory("Choose a project directory");
    } catch {
      push({ tone: "error", message: "Could not open the folder picker." });
      return null;
    }
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

  const parent =
    dir === "." ? null : dir.split("/").slice(0, -1).join("/") || ".";
  const location = dir === "." ? projectName(settings?.project_path) : dir;

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
      {/* The root shows the project's folder name; anywhere else, the path you
          navigated to. Rendered only once there is something real to print —
          a bare "." was the line this replaced. */}
      {location && (
        <p className="mb-1.5 truncate font-mono text-[10px] text-faint">
          {location}
        </p>
      )}

      {isDesktop() && (
        <div className="mb-2">
          {rootAction === null && (
            <div className="flex gap-2">
              <button
                type="button"
                onClick={beginOpen}
                className="font-mono text-[10px] text-dim transition-colors hover:text-ink"
              >
                choose workspace…
              </button>
              <button
                type="button"
                onClick={() => setRootAction("create")}
                className="font-mono text-[10px] text-dim transition-colors hover:text-ink"
              >
                create workspace…
              </button>
            </div>
          )}

          {rootAction === "open" && (
            <div className="rounded border border-signal/40 bg-signal/10 px-2 py-1.5">
              <p className="text-[11px] leading-relaxed text-ink">
                Opening another workspace moves the sandbox, memory vault and
                sessions, and restarts the agent. Continue?
              </p>
              {recent.length > 0 && (
                <>
                  <p className="mt-1.5 font-mono text-[10px] text-faint">
                    recent
                  </p>
                  <ul className="space-y-px">
                    {/* Three: enough to get back to what you were doing, short
                      enough not to push the folder picker out of sight. */}
                    {recent.slice(0, 3).map((path) => (
                      <li key={path}>
                        <button
                          type="button"
                          disabled={busy}
                          onClick={() => void openAt(path)}
                          title={path}
                          className="w-full truncate rounded px-1 py-0.5 text-left font-mono text-[10px] text-dim transition-colors hover:bg-raised hover:text-ink disabled:opacity-60"
                        >
                          {basename(path)}
                          <span className="ml-1.5 text-faint">
                            {parentOf(path)}
                          </span>
                        </button>
                      </li>
                    ))}
                  </ul>
                </>
              )}

              <div className="mt-1.5 flex gap-1.5">
                <button
                  type="button"
                  onClick={() => setRootAction(null)}
                  className="rounded border border-rule px-2 py-0.5 font-mono text-[10px] text-ink"
                >
                  cancel
                </button>
                <button
                  type="button"
                  disabled={busy}
                  onClick={() => void chooseWorkspace()}
                  className="rounded bg-signal px-2 py-0.5 font-mono text-[10px] font-semibold text-void disabled:opacity-60"
                >
                  choose folder
                </button>
              </div>
            </div>
          )}

          {rootAction === "create" && (
            <div className="rounded border border-signal/40 bg-signal/10 px-2 py-1.5">
              <p className="text-[11px] leading-relaxed text-ink">
                Creates a folder with a fresh settings file and memory vault,
                then opens it. The current model carries over; API keys are not
                copied.
              </p>

              <button
                type="button"
                onClick={() => void pick().then((p) => p && setNewParent(p))}
                className="mt-1.5 block w-full truncate rounded border border-rule px-2 py-0.5 text-left font-mono text-[10px] text-ink"
              >
                {newParent ?? "choose where…"}
              </button>

              <input
                value={newName}
                onChange={(e) => setNewName(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === "Enter" && newParent && newName.trim())
                    void create();
                }}
                placeholder="workspace name"
                aria-label="Workspace name"
                className="mt-1 w-full rounded border border-rule bg-panel px-2 py-0.5 font-mono text-[10px] text-ink placeholder:text-faint"
              />

              <div className="mt-1.5 flex gap-1.5">
                <button
                  type="button"
                  onClick={() => {
                    setRootAction(null);
                    setNewParent(null);
                    setNewName("");
                  }}
                  className="rounded border border-rule px-2 py-0.5 font-mono text-[10px] text-ink"
                >
                  cancel
                </button>
                <button
                  type="button"
                  // Both halves are needed, and the button says which is missing
                  // by staying out of reach until they are there.
                  disabled={busy || !newParent || !newName.trim()}
                  onClick={() => void create()}
                  className="rounded bg-signal px-2 py-0.5 font-mono text-[10px] font-semibold text-void disabled:opacity-60"
                >
                  create
                </button>
              </div>
            </div>
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

      {previewError && (
        <p className="mt-2 text-[11px] text-halt">{previewError}</p>
      )}

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

/** The workspace's own name — what the user calls it. */
function basename(path: string): string {
  return projectName(path) ?? path;
}

/** Where it lives, dimmed beside the name: two workspaces can share a name. */
function parentOf(path: string): string {
  const trimmed = path.replace(/[\\/]+$/, "");
  const cut = Math.max(trimmed.lastIndexOf("/"), trimmed.lastIndexOf("\\"));
  return cut > 0 ? trimmed.slice(0, cut) : "";
}

function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(0)} KB`;
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
}
