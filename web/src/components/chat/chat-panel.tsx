"use client";

import { useEffect, useLayoutEffect, useRef, useState } from "react";

import { FileAttach } from "@/components/chat/file-attach";
import { MessageContent } from "@/components/chat/message-content";
import { ToolCallCard } from "@/components/chat/tool-call-card";
import { TypingIndicator } from "@/components/shared/skeleton";
import { useChatStream } from "@/hooks/use-chat-stream";
import { useChatStore } from "@/stores/chat-store";

export function ChatPanel() {
  const messages = useChatStore((s) => s.messages);
  const turnId = useChatStore((s) => s.turnId);
  const conflict = useChatStore((s) => s.conflict);
  const error = useChatStore((s) => s.error);
  const { send, stop } = useChatStream();

  const scrollRef = useRef<HTMLDivElement>(null);
  const pinnedRef = useRef(true);

  // Pinned-to-bottom heuristic: follow the stream only while the user is already
  // at the bottom. Yanking them back down mid-scroll while they read an earlier
  // tool result is the classic streaming-chat annoyance.
  useLayoutEffect(() => {
    const el = scrollRef.current;
    if (el && pinnedRef.current) el.scrollTop = el.scrollHeight;
  }, [messages]);

  function onScroll() {
    const el = scrollRef.current;
    if (!el) return;
    pinnedRef.current = el.scrollHeight - el.scrollTop - el.clientHeight < 48;
  }

  return (
    <div className="flex min-w-0 flex-1 flex-col">
      <div
        ref={scrollRef}
        onScroll={onScroll}
        className="flex-1 overflow-y-auto px-6 py-5"
      >
        {messages.length === 0 ? (
          <EmptyState />
        ) : (
          <div className="mx-auto max-w-3xl space-y-5">
            {messages.map((m) => (
              <article key={m.id}>
                <p className="mb-1.5 font-mono text-[10px] uppercase tracking-[0.14em] text-faint">
                  {m.role === "user" ? "you" : "agent"}
                </p>
                {m.role === "user" ? (
                  <p className="whitespace-pre-wrap text-sm leading-relaxed text-ink">
                    {m.content}
                  </p>
                ) : (
                  <>
                    {m.toolCalls.map((c) => (
                      <ToolCallCard key={c.id} call={c} />
                    ))}
                    {m.content && (
                      <MessageContent
                        content={m.content}
                        streaming={Boolean(turnId) && m.id === messages.at(-1)?.id}
                      />
                    )}
                  </>
                )}
              </article>
            ))}
            {turnId && !messages.at(-1)?.content && (
              <p className="flex items-center gap-2 font-mono text-xs text-signal">
                <TypingIndicator />
                working…
              </p>
            )}
          </div>
        )}
      </div>

      {error && <Banner tone="halt">{error}</Banner>}
      {conflict && <ConflictBanner onInterrupt={() => void stop()} />}

      <Composer busy={Boolean(turnId)} onSend={send} onStop={() => void stop()} />
    </div>
  );
}

function EmptyState() {
  return (
    <div className="flex h-full items-center justify-center">
      <div className="max-w-sm text-center">
        <p className="font-mono text-xs uppercase tracking-[0.14em] text-faint">
          Ready
        </p>
        <h1 className="mt-2 text-lg font-medium text-ink">
          Ask the agent to do something
        </h1>
        <p className="mt-2 text-sm leading-relaxed text-dim">
          It can read and write files, run shell commands and search memory. In
          strict mode it asks before anything that changes your machine.
        </p>
      </div>
    </div>
  );
}

/**
 * F29 — a turn is already running, process-wide (spec §2.3).
 *
 * Offers interrupt rather than retry. Re-sending would double-bill and could
 * re-run tools that already executed.
 */
function ConflictBanner({ onInterrupt }: { onInterrupt: () => void }) {
  return (
    <div className="flex items-center gap-3 border-t border-signal/30 bg-signal/10 px-4 py-2.5">
      <span className="size-1.5 shrink-0 rounded-full bg-signal" aria-hidden />
      <p className="text-xs text-ink">
        A turn is already running. The harness handles one at a time.
      </p>
      <button
        type="button"
        onClick={onInterrupt}
        className="ml-auto rounded border border-signal/50 px-2 py-1 font-mono text-[11px] text-signal transition-colors hover:bg-signal/15"
      >
        interrupt it
      </button>
    </div>
  );
}

function Banner({
  tone,
  children,
}: {
  tone: "halt";
  children: React.ReactNode;
}) {
  return (
    <div
      className={`border-t px-4 py-2.5 text-xs ${
        tone === "halt" ? "border-halt/30 bg-halt/10 text-ink" : ""
      }`}
    >
      {children}
    </div>
  );
}

function Composer({
  busy,
  onSend,
  onStop,
}: {
  busy: boolean;
  onSend: (text: string) => void;
  onStop: () => void;
}) {
  // Attached file contents are appended to the draft as fenced blocks (F25), so
  // the user can see and edit exactly what will be sent.
  const [value, setValue] = useState("");
  const ref = useRef<HTMLTextAreaElement>(null);

  // Grow with content, to a ceiling — a long paste should not push the send
  // affordance off screen.
  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    el.style.height = "auto";
    el.style.height = `${Math.min(el.scrollHeight, 200)}px`;
  }, [value]);

  function submit() {
    const text = value.trim();
    if (!text || busy) return;
    onSend(text);
    setValue("");
  }

  return (
    <div className="border-t border-rule px-4 py-3">
      <div className="mx-auto flex max-w-3xl items-end gap-2 rounded border border-rule bg-raised px-3 py-2 focus-within:border-dim">
        <span className="pb-1 font-mono text-sm text-faint" aria-hidden>
          ›
        </span>
        <FileAttach
          onAttach={(blocks) =>
            setValue((v) => (v ? `${v}\n\n${blocks}` : blocks))
          }
        />
        <textarea
          ref={ref}
          rows={1}
          value={value}
          onChange={(e) => setValue(e.target.value)}
          onKeyDown={(e) => {
            // Enter sends, Shift+Enter breaks the line.
            if (e.key === "Enter" && !e.shiftKey) {
              e.preventDefault();
              submit();
            }
          }}
          placeholder={busy ? "Agent is working…" : "Ask the agent…"}
          aria-label="Message"
          className="min-w-0 flex-1 resize-none bg-transparent text-sm leading-relaxed text-ink outline-none placeholder:text-faint"
        />
        {busy ? (
          <button
            type="button"
            onClick={onStop}
            className="shrink-0 rounded border border-signal/50 px-2 py-1 font-mono text-[11px] text-signal transition-colors hover:bg-signal/15"
          >
            stop
          </button>
        ) : (
          <button
            type="button"
            onClick={submit}
            disabled={!value.trim()}
            className="shrink-0 rounded bg-raised px-2 py-1 font-mono text-[11px] text-dim transition-colors hover:text-ink disabled:opacity-40"
          >
            send
          </button>
        )}
      </div>
    </div>
  );
}
