import { create } from "zustand";

import type {
  ApprovalRequiredEvent,
  ArtifactChange,
  ContextUsageEvent,
  RoleEvent,
  SessionMessage,
  ToolCallResultEvent,
  ToolCallStartEvent,
  UsageEvent,
} from "@/lib/types";

export type ToolCallStatus = "running" | "done" | "error" | "awaiting" | "unresolved";

export interface ToolCall {
  /** Gateway call id. Not stable across an approval — see `adoptAfterApproval`. */
  id: string;
  name: string;
  args: unknown;
  status: ToolCallStatus;
  preview: string | null;
  truncated: boolean;
}

export interface Message {
  id: string;
  role: "user" | "assistant";
  content: string;
  toolCalls: ToolCall[];
}

interface ChatState {
  messages: Message[];
  /** Null when no turn is in flight. */
  turnId: string | null;
  sessionId: string | null;
  model: string | null;
  pendingApproval: ApprovalRequiredEvent | null;
  /** Sum of the per-leg `usage` events for the current turn. */
  turnTokens: { prompt: number; completion: number };
  /**
   * Files the current turn changed on disk, from the gateway's before/after
   * comparison. Arrives once, at the end of the turn.
   */
  turnArtifacts: ArtifactChange[];
  sessionCost: number;
  context: ContextUsageEvent | null;
  /** Set when a turn is refused with 409, cleared on the next successful send. */
  conflict: string | null;
  error: string | null;

  startUserTurn: (text: string) => void;
  onRole: (e: RoleEvent) => void;
  onText: (chunk: string) => void;
  onToolStart: (e: ToolCallStartEvent) => void;
  onToolResult: (e: ToolCallResultEvent) => void;
  onApprovalRequired: (e: ApprovalRequiredEvent) => void;
  onApprovalResolved: (approved: boolean) => void;
  onUsage: (e: UsageEvent) => void;
  onContext: (e: ContextUsageEvent) => void;
  onArtifacts: (files: ArtifactChange[]) => void;
  onDone: () => void;
  onError: (message: string) => void;
  setConflict: (message: string | null) => void;
  loadHistory: (sessionId: string, messages: SessionMessage[]) => void;
  reset: () => void;
}

let seq = 0;
const nextId = () => `m${++seq}`;

/** Mutate the trailing assistant message, appending one if there isn't one. */
function withAssistant(
  messages: Message[],
  fn: (m: Message) => void,
): Message[] {
  const next = [...messages];
  let last = next[next.length - 1];
  if (!last || last.role !== "assistant") {
    last = { id: nextId(), role: "assistant", content: "", toolCalls: [] };
    next.push(last);
  } else {
    last = { ...last, toolCalls: [...last.toolCalls] };
    next[next.length - 1] = last;
  }
  fn(last);
  return next;
}

export const useChatStore = create<ChatState>((set) => ({
  messages: [],
  turnId: null,
  sessionId: null,
  model: null,
  pendingApproval: null,
  turnTokens: { prompt: 0, completion: 0 },
  turnArtifacts: [],
  sessionCost: 0,
  context: null,
  conflict: null,
  error: null,

  startUserTurn: (text) =>
    set((s) => ({
      messages: [
        ...s.messages,
        { id: nextId(), role: "user", content: text, toolCalls: [] },
      ],
      // Per-turn counters reset; session cost is cumulative and must not.
      turnTokens: { prompt: 0, completion: 0 },
      turnArtifacts: [],
      conflict: null,
      error: null,
    })),

  onRole: (e) =>
    set({
      turnId: e.turn_id,
      // The gateway's session is the truth, not what this tab believed
      // (spec §2.3 — the harness holds one global session).
      sessionId: e.session_id,
      model: e.model,
    }),

  onText: (chunk) =>
    set((s) => ({
      messages: withAssistant(s.messages, (m) => {
        m.content += chunk;
      }),
    })),

  onToolStart: (e) =>
    set((s) => ({
      messages: withAssistant(s.messages, (m) => {
        m.toolCalls.push({
          id: e.id,
          name: e.name,
          args: e.args,
          status: "running",
          preview: null,
          truncated: false,
        });
      }),
    })),

  onToolResult: (e) =>
    set((s) => ({
      messages: withAssistant(s.messages, (m) => {
        const call = m.toolCalls.find((c) => c.id === e.id);
        if (call) {
          call.status = e.status === "error" ? "error" : "done";
          call.preview = e.output_preview;
          call.truncated = e.truncated;
        } else {
          // A result with no matching start — surface it rather than drop it.
          m.toolCalls.push({
            id: e.id,
            name: e.name,
            args: null,
            status: e.status === "error" ? "error" : "done",
            preview: e.output_preview,
            truncated: e.truncated,
          });
        }
      }),
    })),

  onApprovalRequired: (e) =>
    set((s) => ({
      pendingApproval: e,
      messages: withAssistant(s.messages, (m) => {
        const call = m.toolCalls.find((c) => c.id === e.call_id);
        if (call) call.status = "awaiting";
      }),
    })),

  /**
   * Retire the parked card when a decision lands.
   *
   * **`call_id` does not survive an approval.** `run_confirmation_turn` starts a
   * brand-new turn, so the follow-up `tool_call_start` carries a *different* id
   * (spec §5, verified on the wire). Keying purely on id would leave the
   * pre-approval card spinning forever and render a second card beside it for
   * the same user-approved action.
   *
   * So on resolve we drop the awaiting card outright: if approved, the follow-up
   * leg emits a fresh start/result pair that tells the whole story; if denied,
   * there is nothing more to show.
   */
  onApprovalResolved: (approved) =>
    set((s) => ({
      pendingApproval: null,
      messages: withAssistant(s.messages, (m) => {
        const idx = m.toolCalls.findIndex((c) => c.status === "awaiting");
        if (idx === -1) return;
        if (approved) {
          m.toolCalls.splice(idx, 1);
        } else {
          m.toolCalls[idx] = {
            ...m.toolCalls[idx],
            status: "error",
            preview: "Denied. The agent may ask again.",
          };
        }
      }),
    })),

  // Emitted once per leg, and `cost_usd` is the running session total — so token
  // counts accumulate across legs while cost is a replacement, not a sum.
  onUsage: (e) =>
    set((s) => ({
      turnTokens: {
        prompt: s.turnTokens.prompt + e.prompt_tokens,
        completion: s.turnTokens.completion + e.completion_tokens,
      },
      sessionCost: e.cost_usd,
    })),

  onContext: (e) => set({ context: e }),

  onArtifacts: (files) => set({ turnArtifacts: files }),

  onDone: () => set({ turnId: null, pendingApproval: null }),

  onError: (message) => set({ error: message, turnId: null }),

  setConflict: (conflict) => set({ conflict }),

  /** Rebuild from G8 history when switching sessions (F11). */
  loadHistory: (sessionId, messages) =>
    set({
      sessionId,
      turnId: null,
      pendingApproval: null,
      error: null,
      conflict: null,
      // Changed files are observed live, never stored — a session loaded from
      // history has none, and showing the previous session's would be a lie.
      turnArtifacts: [],
      messages: messages
        // The harness injects retrieved memories as a synthetic user turn;
        // showing it would be confusing — it is not something the user typed.
        .filter((m) => !m.content.startsWith("--- Relevant memories ---"))
        .map((m) => ({
          id: nextId(),
          role: m.role === "user" ? "user" : "assistant",
          content: m.content,
          toolCalls: m.tool_calls.map((t) => ({
            id: t.id,
            name: t.name,
            args: t.args,
            status:
              t.status === "unresolved"
                ? ("unresolved" as const)
                : ("done" as const),
            preview: t.result_preview,
            truncated: t.truncated,
          })),
        })),
    }),

  reset: () =>
    set({
      messages: [],
      turnId: null,
      pendingApproval: null,
      turnTokens: { prompt: 0, completion: 0 },
      turnArtifacts: [],
      context: null,
      conflict: null,
      error: null,
    }),
}));
