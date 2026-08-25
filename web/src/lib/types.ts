/**
 * Types mirroring the gateway's Rust surface.
 *
 * Source of truth is `src/gateway/v2_types.rs` + `src/gateway/v2_handlers.rs`
 * and spec §5 / §7. Hand-maintained for now; spec §3.2 notes that deriving these
 * (ts-rs / schemars) in CI is the real fix, because a drifted event enum fails
 * *silently* — the parser just drops event names it doesn't know.
 *
 * That is why `sseEventNames` exists below and why the parser surfaces unknown
 * events instead of ignoring them.
 */

// ─── Error model (spec §8) ─────────────────────────────────────

export type ErrorCode =
  | "invalid_request"
  | "unauthorized"
  | "path_forbidden"
  | "not_found"
  | "turn_in_progress"
  | "stale_approval"
  | "rate_limited"
  | "internal"
  | "provider_unavailable"
  | "not_implemented";

export interface GatewayError {
  code: ErrorCode | string;
  message: string;
  details?: Record<string, unknown> | null;
}

/** Every `/v2` error body is `{ error: { code, message, details } }`. */
export interface GatewayErrorBody {
  error: GatewayError;
}

// ─── SSE stream events (spec §5) ───────────────────────────────

export interface RoleEvent {
  role: string;
  model: string;
  provider: string;
  session_id: string;
  agent: string | null;
  turn_id: string;
}

export interface TextEvent {
  content: string;
}

export interface ToolCallStartEvent {
  id: string;
  name: string;
  args: unknown;
}

export interface ToolCallResultEvent {
  id: string;
  name: string;
  status: "done" | "error";
  output_preview: string;
  truncated: boolean;
}

export interface ApprovalRequiredEvent {
  turn_id: string;
  call_id: string;
  name: string;
  args: unknown;
  destructive: boolean;
  /** One of: Destructive deletion / git / SQL / system. Null when not destructive. */
  category: string | null;
  /**
   * Always true today. Approving grants the tool for the rest of the *process*,
   * keyed by tool name — F9 is required to say so (spec C2, §9.4).
   */
  sticky: boolean;
  expires_at: string;
}

export interface ApprovalResolvedEvent {
  call_id: string;
  name?: string;
  approved: boolean;
  reason: "user" | "timeout" | "disconnect";
}

export interface UsageEvent {
  prompt_tokens: number;
  completion_tokens: number;
  total_tokens?: number;
  /**
   * Cumulative for the session, and emitted **once per leg** — a turn that went
   * through an approval emits several. Do not treat the last one as the turn
   * total.
   */
  cost_usd: number;
}

export interface ContextUsageEvent {
  used: number;
  total: number;
  percent: number;
}

/** One path a turn created, modified or deleted. */
export interface ArtifactChange {
  /** Relative to the project root. */
  path: string;
  change: "created" | "modified" | "deleted";
}

/**
 * Files the turn changed on disk.
 *
 * Emitted once, after the last leg — the gateway compares the project tree
 * before and after the turn, so this covers writes made through the shell as
 * well as through the file tools. Absent when nothing changed.
 */
export interface ArtifactsEvent {
  files: ArtifactChange[];
}

export interface DoneEvent {
  turn_id: string;
  stop_reason: "complete" | "error" | "interrupted";
}

/** Discriminated union over the `event:` field of each SSE frame. */
export type StreamEvent =
  | { type: "role"; data: RoleEvent }
  | { type: "text"; data: TextEvent }
  | { type: "tool_call_start"; data: ToolCallStartEvent }
  | { type: "tool_call_result"; data: ToolCallResultEvent }
  | { type: "approval_required"; data: ApprovalRequiredEvent }
  | { type: "approval_resolved"; data: ApprovalResolvedEvent }
  | { type: "usage"; data: UsageEvent }
  | { type: "context_usage"; data: ContextUsageEvent }
  | { type: "artifacts"; data: ArtifactsEvent }
  | { type: "error"; data: GatewayError }
  | { type: "done"; data: DoneEvent }
  /** Forward compatibility: surfaced, never silently dropped (spec §5). */
  | { type: "unknown"; name: string; raw: string };

export const sseEventNames = [
  "role",
  "text",
  "tool_call_start",
  "tool_call_result",
  "approval_required",
  "approval_resolved",
  "usage",
  "context_usage",
  "artifacts",
  "error",
  "done",
] as const;

export type SseEventName = (typeof sseEventNames)[number];

// ─── Requests ──────────────────────────────────────────────────

export interface ChatMessage {
  role: "system" | "user" | "assistant";
  content: string;
}

export interface V2ChatRequest {
  session_id?: string;
  messages: ChatMessage[];
  agent?: string;
  model?: string;
}

export interface ApprovalRequest {
  turn_id: string;
  call_id: string;
  tool_name: string;
  approved: boolean;
}

// ─── Management surface (spec §7) ──────────────────────────────

export interface Agent {
  name: string;
  description: string | null;
  capabilities: string[];
  model: string | null;
  provider: string | null;
  is_current: boolean;
  is_orchestrator: boolean;
}

export interface ProviderModels {
  provider: string;
  models: string[];
  /** False when the catalogue could not be fetched — the UI offers free text. */
  available: boolean;
  cached: boolean;
  error: string | null;
}

export interface Provider {
  name: string;
  is_current: boolean;
  current_model: string | null;
  available: boolean;
  available_models?: string[];
}

export type PermissionMode = "strict" | "auto" | "yolo";

export interface Settings {
  permission_mode: PermissionMode;
  project_path: string;
  sandbox_root: string;
  approved_tools: string[];
  memory?: Record<string, unknown>;
}

export interface McpServer {
  id: string;
  status: string;
  transport: "stdio" | "http";
  /** Null means "unknown" — F14 must render "—", never "0" (spec G4). */
  tool_count: number | null;
  error: string | null;
}

export interface McpStatus {
  servers: McpServer[];
  running: number;
  running_stdio: number;
  total_tools: number | null;
}

export interface MemoryResult {
  title: string;
  level: string;
  path: string;
  score: number;
  preview: string;
}

export interface MemorySearchResponse {
  results: MemoryResult[];
  count: number;
  limit: number;
}

export interface MemoryStats {
  total_memcells: number;
  total_events: number;
  total_foresights: number;
  total_episodes: number;
  pending_foresights: number;
  auto_search_enabled: boolean;
  auto_write_enabled: boolean;
}

export interface FileContent {
  path: string;
  /** Null when `binary` is true. */
  content: string | null;
  size: number;
  truncated: boolean;
  binary: boolean;
}

export interface TreeEntry {
  name: string;
  path: string;
  is_dir: boolean;
  size: number | null;
}

export interface FileTree {
  path: string;
  depth: number;
  entries: TreeEntry[];
  truncated: boolean;
}

export interface SessionToolCall {
  id: string;
  name: string;
  args: unknown;
  /**
   * `unresolved` means the call never received a response — usually an approval
   * superseded it (the post-approval call gets a *different* id, spec §5).
   * Render it as abandoned, never as still-running.
   */
  status: "pending" | "done" | "unresolved";
  result_preview: string | null;
  truncated: boolean;
}

export interface SessionMessage {
  role: string;
  content: string;
  timestamp?: number;
  tool_calls: SessionToolCall[];
}

export interface SessionMessages {
  session_id: string;
  event_count: number;
  messages: SessionMessage[];
}

export interface SessionInfo {
  id: string;
  created_at: string;
  /**
   * Human-readable label, or `null` for an untitled session.
   *
   * The gateway names a session from its first prompt, and a title someone
   * types is locked against that. `null` is the honest answer for a session
   * that has never had a turn — the UI shows the id, which is what every
   * session showed before titles existed.
   */
  title: string | null;
  /**
   * `null` from the list endpoint — it is a metadata-only query that does not
   * load events, so no count is available. Render as unknown, never as 0.
   * `GET /v2/sessions/{id}/messages` has the real number.
   */
  event_count: number | null;
}

export interface Health {
  status: string;
  version?: string;
  provider?: string;
  model?: string;
  session_id?: string;
  mcp_running?: number;
  turn_active: boolean;
  turn_id?: string | null;
}

export interface CostSummary {
  total_cost: number;
  total_prompt_tokens: number;
  total_completion_tokens: number;
  total_tokens: number;
  request_count: number;
}

// ─── Routines (src/routine/) ───────────────────────────────────

export type RoutineAssignee =
  | { kind: "lead" }
  | { kind: "agent"; name: string }
  | { kind: "worker"; name: string };

export type CatchUp = "skip" | "run_once";
export type Concurrency = "queue" | "skip" | "parallel";
export type TaskPriority = "low" | "medium" | "high" | "urgent";

export type RoutineTrigger =
  | { type: "cron"; expression: string; timezone: string; catch_up: CatchUp }
  | { type: "every"; seconds: number }
  | { type: "manual" };

export interface RoutineTask {
  title: string;
  priority: TaskPriority;
  description: string;
}

export type RunState =
  | "running"
  | "delivered"
  | "succeeded"
  | "failed"
  | "skipped";

export interface RoutineRun {
  id: string;
  routine_id: string;
  routine_name: string;
  assignee: string;
  status: RunState;
  reason: string | null;
  scheduled_at: string | null;
  started_at: string | null;
  finished_at: string | null;
  pid: number | null;
  exit_code: number | null;
  log_path: string | null;
  source: string;
}

/**
 * A routine as the gateway reports it: the definition plus the schedule state
 * that lives outside it (`next_due`, counters, what is in flight).
 */
export interface Routine {
  id: string;
  name: string;
  enabled: boolean;
  /** `lead`, `agent:<name>` or `worker:<name>` — the form's select value. */
  assignee: string;
  assignee_detail: RoutineAssignee;
  trigger: RoutineTrigger;
  trigger_summary: string;
  concurrency: Concurrency;
  task: RoutineTask;
  permission: PermissionMode;
  created_at: string | null;
  updated_at: string | null;
  next_due: string | null;
  last_fired_at: string | null;
  last_run_at: string | null;
  queued: number;
  fired: number;
  skipped: number;
  active_runs: number;
  last_run: RoutineRun | null;
  /** Only on the single-routine read. */
  runs?: RoutineRun[];
}

/** Body of create/update. Every field optional so a toggle can send just one. */
export interface RoutineUpsert {
  name?: string;
  enabled?: boolean;
  assignee?: RoutineAssignee;
  trigger?: RoutineTrigger;
  concurrency?: Concurrency;
  task?: RoutineTask;
  permission?: PermissionMode;
}

export interface AssigneeOption {
  value: string;
  kind: "lead" | "agent" | "worker";
  label: string;
  description?: string | null;
  status?: string;
  available: boolean;
}

export interface TickReport {
  status: string;
  now: string | null;
  fired: RoutineRun[];
  finished: RoutineRun[];
  queued: { routine_id: string; scheduled_at: string | null }[];
  skipped: { routine_id: string; reason: string }[];
}

// ─── Background activity (`/v2/activity`) ──────────────────────

/** A team worker, as `momo-fetch team status` reports it. */
export interface TeamWorker {
  name: string;
  agent: string | null;
  permission: string;
  mode: string;
  status: string;
  error: string | null;
  branch: string | null;
  pane_id: string | null;
  worktree_path: string | null;
  work_dir: string;
  task: string;
  result: string | null;
  last_heartbeat: string | null;
  last_message_ts: string | null;
}

export interface TeamSummary {
  team_id: string | null;
  name: string | null;
  status: string;
  workers: TeamWorker[];
  mailbox: { unread_by_recipient: Record<string, number> };
  started_at: string | null;
  project_path: string | null;
}

/**
 * What is working in the background right now — outside this turn, and outside
 * this process.
 */
export interface Activity {
  team: TeamSummary | null;
  /** Config names in `.harness/teams/` — what could be started. */
  team_configs: string[];
  /** Routine runs still in flight. */
  runs: RoutineRun[];
  counts: {
    workers_running: number;
    runs_active: number;
    routines_armed: number;
  };
}
