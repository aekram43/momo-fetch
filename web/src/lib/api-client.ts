/**
 * HTTP client for the gateway.
 *
 * One place resolves the base URL and one place parses errors, so the rest of
 * the app never string-builds a gateway URL or reaches into an error body.
 */

import type {
  Agent,
  ApprovalRequest,
  AssigneeOption,
  CostSummary,
  FileContent,
  FileTree,
  GatewayError,
  Health,
  McpStatus,
  MemorySearchResponse,
  MemoryStats,
  PermissionMode,
  Provider,
  ProviderModels,
  Routine,
  RoutineRun,
  RoutineUpsert,
  SessionInfo,
  SessionMessages,
  Settings,
  TickReport,
  V2ChatRequest,
} from "./types";

declare global {
  interface Window {
    /** Injected by the Tauri shell at runtime (spec T3). */
    __GATEWAY_URL__?: string;
  }
}

/**
 * Resolution order:
 *   1. `window.__GATEWAY_URL__` — Tauri, injected at runtime (spec T3)
 *   2. same origin, when the page is being served by the gateway at `/ui`
 *   3. `NEXT_PUBLIC_GATEWAY_URL` — dev, `next dev` against a separate gateway
 *   4. `http://localhost:3000`
 *
 * The Tauri path *must* be runtime injection: `output: 'export'` inlines env
 * vars at build time, and the desktop shell picks a free port at launch.
 *
 * Step 2 is not in spec §3.2 and was added after testing G9 end to end. The
 * gateway binds an ephemeral port (`--gateway-port 0`), so a build-time
 * `NEXT_PUBLIC_GATEWAY_URL` is guaranteed wrong there — the UI loaded fine and
 * then reported "offline" against `localhost:3000`. When the gateway is serving
 * this bundle, it is by definition the right gateway to talk to, and its origin
 * is knowable at runtime. This also keeps those requests same-origin, so the
 * permissive CORS default stops mattering for the served-by-gateway path.
 */
export function gatewayUrl(): string {
  if (typeof window !== "undefined") {
    if (window.__GATEWAY_URL__) {
      return window.__GATEWAY_URL__.replace(/\/$/, "");
    }
    // Served under the gateway's own /ui mount → talk to that origin.
    if (window.location.pathname.startsWith("/ui")) {
      return window.location.origin;
    }
  }
  const fromEnv = process.env.NEXT_PUBLIC_GATEWAY_URL;
  if (fromEnv) return fromEnv.replace(/\/$/, "");
  return "http://localhost:3000";
}

/**
 * An error carrying the gateway's typed code, so callers can branch on
 * `turn_in_progress` / `path_forbidden` / … instead of matching message text.
 */
export class ApiError extends Error {
  readonly status: number;
  readonly code: string;
  readonly details?: Record<string, unknown> | null;

  constructor(status: number, error: GatewayError) {
    super(error.message);
    this.name = "ApiError";
    this.status = status;
    this.code = error.code;
    this.details = error.details;
  }

  /** A turn is running; mutations are refused until it ends (spec F29). */
  get isTurnConflict(): boolean {
    return this.code === "turn_in_progress";
  }

  /** The approval this refers to is no longer pending — dismiss silently. */
  get isStaleApproval(): boolean {
    return this.code === "stale_approval";
  }
}

/**
 * The bearer token, held **in memory only**.
 *
 * Deliberately not `localStorage`: CORS is permissive on the gateway by default,
 * so a token readable by any script on the origin is a token any hostile page
 * can lift and then drive shell execution with (spec §9.2, §9.3).
 */
let authToken: string | null = null;

export function setAuthToken(token: string | null): void {
  authToken = token;
}

export function hasAuthToken(): boolean {
  return authToken !== null;
}

function headers(extra?: HeadersInit): Headers {
  const h = new Headers(extra);
  if (authToken) h.set("Authorization", `Bearer ${authToken}`);
  return h;
}

/** Turn any non-2xx into an {@link ApiError}, whatever shape the body is in. */
async function raiseForStatus(res: Response): Promise<never> {
  let error: GatewayError = {
    code: "internal",
    message: `${res.status} ${res.statusText}`,
  };
  try {
    const body = await res.json();
    if (body?.error?.code) {
      error = body.error;
    } else if (typeof body?.error === "string") {
      // v1 handlers use a flatter shape than the §8 model.
      error = { code: "internal", message: body.error };
    }
  } catch {
    // Non-JSON body — keep the status-derived default.
  }
  throw new ApiError(res.status, error);
}

async function request<T>(path: string, init?: RequestInit): Promise<T> {
  const res = await fetch(`${gatewayUrl()}${path}`, {
    ...init,
    headers: headers(init?.headers),
  });
  if (!res.ok) await raiseForStatus(res);
  return res.status === 204 ? (undefined as T) : ((await res.json()) as T);
}

function post<T>(path: string, body?: unknown): Promise<T> {
  return request<T>(path, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
}

// ─── Chat ──────────────────────────────────────────────────────

/**
 * Open the V2 stream. Returns the raw `Response` — feed it to
 * `readStream()` from `sse-parser.ts`.
 *
 * `signal` should come from an `AbortController`; aborting is how F20's Escape
 * and component unmount stop a turn client-side. That only closes *this* end —
 * use `interrupt()` to stop the harness as well.
 *
 * **Never auto-retry this.** Re-sending a turn double-bills and can re-run tools
 * (spec F21).
 */
export async function openChatStream(
  req: V2ChatRequest,
  signal?: AbortSignal,
): Promise<Response> {
  const res = await fetch(`${gatewayUrl()}/v2/chat/stream`, {
    method: "POST",
    headers: headers({ "content-type": "application/json" }),
    body: JSON.stringify(req),
    signal,
  });
  // A 409 arrives as JSON, not as a stream.
  if (!res.ok) await raiseForStatus(res);
  return res;
}

export const approve = (req: Omit<ApprovalRequest, "approved">) =>
  post<{ resolved: boolean; approved: boolean; sticky: boolean }>(
    "/v2/chat/approve",
    { ...req, approved: true },
  );

export const deny = (req: Omit<ApprovalRequest, "approved">) =>
  post<{ resolved: boolean; approved: boolean }>("/v2/chat/deny", {
    ...req,
    approved: false,
  });

export const interrupt = (turnId?: string) =>
  post<{ interrupted: boolean }>("/v2/chat/interrupt", { turn_id: turnId });

// ─── Management ────────────────────────────────────────────────

export const getAgents = () =>
  request<{ agents: Agent[]; current: string | null }>("/v2/agents");

export const switchAgent = (name: string) =>
  post<{ switched_to: string }>("/v2/agents/switch", { name });

// Not `useDefaultAgent` — a name starting with "use" reads as a React hook to
// both eslint and human readers, and this is a plain HTTP call.
export const resetToDefaultAgent = () =>
  post<{ switched_to: null }>("/v2/agents/default");

export const getProviders = () =>
  request<{ providers: Provider[] }>("/v2/providers");

/**
 * The model catalogue for one provider, queried from the provider itself.
 *
 * Returns 200 even when the list could not be fetched — `available:false` with
 * a reason. A directory lookup failing must not stop someone switching models,
 * so the UI falls back to a text field.
 */
export const getProviderModels = (provider: string) =>
  request<ProviderModels>(
    `/v2/providers/${encodeURIComponent(provider)}/models`,
  );

/** Drop the cached catalogue so the next read refetches. */
export const refreshProviderModels = (provider: string) =>
  request<{ cleared: string }>(
    `/v2/providers/${encodeURIComponent(provider)}/models`,
    { method: "DELETE" },
  );

export const switchModel = (model: string) =>
  post<{ provider: string; model: string }>("/v2/switch-model", { model });

export const switchProvider = (provider: string) =>
  post<{ provider: string; model: string }>("/v2/switch-provider", { provider });

export const getSettings = () => request<Settings>("/v2/settings");

export const setPermissionMode = (mode: PermissionMode) =>
  post<{ mode: PermissionMode }>("/v2/settings/permission", { mode });

/** Revoke every sticky approval, so confirmation prompts come back (F17). */
export const clearApprovedTools = () =>
  request<{ approved_tools: string[] }>("/v2/settings/approved-tools", {
    method: "DELETE",
  });

export const getMcpServers = () => request<McpStatus>("/v2/mcp/servers");

export const searchMemory = (q: string, limit = 10) =>
  request<MemorySearchResponse>(
    `/v2/memory/search?q=${encodeURIComponent(q)}&limit=${limit}`,
  );

export const getMemoryStats = () => request<MemoryStats>("/v2/memory/stats");

export const readFile = (path: string) =>
  request<FileContent>(`/v2/files?path=${encodeURIComponent(path)}`);

export const readTree = (path = ".", depth = 1) =>
  request<FileTree>(
    `/v2/files/tree?path=${encodeURIComponent(path)}&depth=${depth}`,
  );

// ─── Sessions ──────────────────────────────────────────────────

export const listSessions = () =>
  request<{ sessions: SessionInfo[] }>("/v1/sessions");

export const createSession = () =>
  post<{ session_id: string }>("/v1/sessions");

/** Rename a session. The gateway trims and clips; it returns what it kept. */
export const renameSession = (id: string, title: string) =>
  request<{ id: string; title: string | null }>(
    `/v2/sessions/${encodeURIComponent(id)}`,
    {
      method: "PATCH",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ title }),
    },
  );

export const deleteSession = (id: string) =>
  request<{ deleted: boolean }>(`/v1/sessions/${encodeURIComponent(id)}`, {
    method: "DELETE",
  });

export const getSessionMessages = (id: string) =>
  request<SessionMessages>(
    `/v2/sessions/${encodeURIComponent(id)}/messages`,
  );

// ─── Status ────────────────────────────────────────────────────

/** Auth-exempt (G12) — safe to poll before a token is entered. */
export const getHealth = () => request<Health>("/health");

export const getCost = () => request<CostSummary>("/v1/cost");

// ─── Routines ──────────────────────────────────────────────────

export const listRoutines = () =>
  request<{ routines: Routine[]; count: number }>("/v2/routines");

export const getRoutine = (id: string) =>
  request<Routine>(`/v2/routines/${encodeURIComponent(id)}`);

export const createRoutine = (body: RoutineUpsert) =>
  post<Routine>("/v2/routines", body);

/** Partial: unset fields are left as they are. */
export const updateRoutine = (id: string, body: RoutineUpsert) =>
  request<Routine>(`/v2/routines/${encodeURIComponent(id)}`, {
    method: "PATCH",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(body),
  });

export const deleteRoutine = (id: string) =>
  request<{ deleted: boolean; id: string }>(
    `/v2/routines/${encodeURIComponent(id)}`,
    { method: "DELETE" },
  );

/**
 * Fire a routine outside its schedule.
 *
 * Throws `ApiError` with code `run_in_progress` (409) when one is already
 * going and the routine is not set to `parallel`.
 */
export const runRoutine = (id: string) =>
  post<RoutineRun>(`/v2/routines/${encodeURIComponent(id)}/run`);

export const getRoutineRuns = (id: string, limit = 20) =>
  request<{ runs: RoutineRun[]; count: number }>(
    `/v2/routines/${encodeURIComponent(id)}/runs?limit=${limit}`,
  );

export const getAssignees = () =>
  request<{ assignees: AssigneeOption[] }>("/v2/routines/assignees");

/** Advance the schedule now rather than waiting for the gateway's timer. */
export const tickRoutines = () => post<TickReport>("/v2/routines/tick");
