"use client";

import { useRef, useState } from "react";

import { useToastStore } from "@/stores/toast-store";

/** Refuse anything that would blow out the context window. */
const MAX_BYTES = 256 * 1024;
const MAX_FILES = 4;

/**
 * F25 — attach text files to a message.
 *
 * **Text only, read client-side, prepended as fenced context.** The file never
 * goes to the gateway as an upload — there is no upload endpoint and no
 * multimodal wiring (spec Q2), so this is the honest shape: the content becomes
 * part of the prompt, which is also why the size cap is not negotiable. A 5 MB
 * log pasted into a turn is an expensive way to hit the context limit.
 *
 * Binary files are rejected up front rather than sent as mojibake.
 */
export function FileAttach({
  onAttach,
}: {
  onAttach: (blocks: string) => void;
}) {
  const inputRef = useRef<HTMLInputElement>(null);
  const [busy, setBusy] = useState(false);
  const push = useToastStore((s) => s.push);

  async function handle(files: FileList | null) {
    if (!files?.length) return;
    setBusy(true);
    try {
      const chosen = Array.from(files).slice(0, MAX_FILES);
      if (files.length > MAX_FILES) {
        push({
          tone: "warn",
          message: `Attaching the first ${MAX_FILES} files. The rest were skipped.`,
        });
      }

      const blocks: string[] = [];
      for (const file of chosen) {
        if (file.size > MAX_BYTES) {
          push({
            tone: "warn",
            message: `${file.name} is ${(file.size / 1024).toFixed(0)} KB — over the ${MAX_BYTES / 1024} KB limit. Skipped.`,
          });
          continue;
        }
        const text = await file.text();
        // A NUL in the first 8 KB is the same binary heuristic the gateway uses
        // for G6, so both ends agree on what "text" means.
        if (text.slice(0, 8192).includes("\0")) {
          push({
            tone: "warn",
            message: `${file.name} looks binary. Text files only.`,
          });
          continue;
        }
        const ext = file.name.split(".").pop() ?? "";
        blocks.push(`\`\`\`${ext} ${file.name}\n${text}\n\`\`\``);
      }

      if (blocks.length) onAttach(blocks.join("\n\n"));
    } finally {
      setBusy(false);
      // Reset so re-picking the same file fires `change` again.
      if (inputRef.current) inputRef.current.value = "";
    }
  }

  return (
    <>
      <input
        ref={inputRef}
        type="file"
        multiple
        accept="text/*,.md,.json,.toml,.yaml,.yml,.rs,.ts,.tsx,.js,.jsx,.py,.go,.sh,.sql,.csv,.log"
        className="hidden"
        onChange={(e) => void handle(e.target.files)}
      />
      <button
        type="button"
        disabled={busy}
        onClick={() => inputRef.current?.click()}
        aria-label="Attach a text file"
        title="Attach a text file — its contents are added to your message"
        className="shrink-0 rounded px-1.5 py-1 font-mono text-[13px] text-faint transition-colors hover:text-ink disabled:opacity-40"
      >
        ⎘
      </button>
    </>
  );
}
