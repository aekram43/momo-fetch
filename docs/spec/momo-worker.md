# MoMo Worker — Web UI & Desktop App Spec

> **Status:** v3 — reconciled against code at `a218c19` (0.9.1), pushed to `origin/dev`.
> **Progress:** Sprint-1 gateway slab **landed** · **Wave 0 complete** (verification + R1 + B0–B4) · Waves 1–3 ready to start in parallel · frontend not started.
> **Where to look:** [**§12.1 Status Ledger**](#121-status-ledger) — what is done, what is open, who owns it. Start there.
> **Inspired by:** OpenWorker.com (3-panel layout, approval flow, artifacts)
> **Architecture:** Next.js frontend ↔ existing Gateway (axum) ↔ Harness (Rust)
> **Execution plan:** [§12](#12-execution-plan--model-assignment) is the dispatch document — read it before picking up work.
> **Handoff state:** [`momo-worker-scratchpad.md`](./momo-worker-scratchpad.md) — §4 "hard-won knowledge" is the load-bearing part.

---

## Table of Contents

0. [Code Audit — Corrections to v1](#0-code-audit--corrections-to-v1)
1. [Overview](#1-overview)
2. [Architecture](#2-architecture)
3. [Phase 1: Web UI (Next.js)](#3-phase-1-web-ui-nextjs)
   - 3.1 Gateway API Enhancements (Rust)
   - 3.2 Frontend Architecture
   - 3.3 Task Breakdown
4. [Phase 2: Tauri Desktop Wrap](#4-phase-2-tauri-desktop-wrap)
   - 4.1 Tauri Integration
   - 4.2 Task Breakdown
5. [SSE Event Protocol](#5-sse-event-protocol)
6. [UI Layout Specification](#6-ui-layout-specification)
7. [Gateway Endpoint Reference](#7-gateway-endpoint-reference)
8. [Error Model](#8-error-model)
9. [Security Considerations](#9-security-considerations)
10. [Testing Strategy](#10-testing-strategy)
11. [Open Questions](#11-open-questions)
12. [Execution Plan & Model Assignment](#12-execution-plan--model-assignment)
    - 12.1 Status Ledger
    - 12.2 Model Routing Rubric
    - 12.3 Contention Map — why naive parallelism breaks
    - 12.4 Work Packages
    - 12.5 Wave Schedule
    - 12.6 Handoff Contract
    - 12.7 Quality Gates

---

## 0. Code Audit — Corrections to v1

The v1 draft assumed behaviour the harness does not have. These are the load-bearing corrections; each is expanded in the section referenced.

| # | v1 assumption | Reality in code | Section |
|---|---------------|-----------------|---------|
| C1 | Approval pauses the stream; a `oneshot` keyed by `call_id` resumes it | adk ends the `EventStream` and surfaces `event.actions.tool_confirmation`. Resumption is a **new turn** via `Harness::run_confirmation_turn(tool_name, approved)` (`src/harness.rs:405`), which calls `rebuild_runner()` and therefore needs `&mut Harness` (**write** lock) | [2.4](#24-approval-flow-corrected), [G1](#task-g1-rich-sse-stream-v2chatstream) |
| C2 | Approval is per tool **call** | Approval is per tool **name** and **sticky for the process lifetime** — `approved_tools: HashSet<String>` (`src/harness.rs:38`). Approving `shell_exec` once means it is never asked again | [2.4](#24-approval-flow-corrected) |
| C3 | Frontend connects with native `EventSource` | `EventSource` is GET-only and cannot send a request body. `/v2/chat/stream` is a POST. Must use `fetch` + `ReadableStream`, or split into POST-then-GET | [3.2](#sse-transport-decision), [F3](#sprint-1-core-chat-week-1) |
| C4 | Sessions are independent per tab | `Harness` holds a single `current_session_id`; `resume_session` mutates global state. Two tabs on different sessions corrupt each other | [2.3](#23-concurrency-model-corrected) |
| C5 | `GET /v1/sessions/{id}` "returns raw events" | It already returns a `messages` array — text parts only (`src/gateway/handlers.rs:266`). G8 is an *extension* (add tool calls), not a new capability | [G8](#task-g8-session-messages-with-tool-calls) |
| C6 | `usage` / cost events can just be emitted | Cost tracking is wired **only in the REPL** (`reset_turn`/`record_event`/`finalize_turn` at `src/cli/repl.rs:415,522,537,644`). Gateway turns record nothing, so `GET /v1/cost` under-reports and the `usage` event has no source | [G10](#task-g10-cost--context-tracking-in-gateway-turns) |
| C7 | Tauri spawns `momo-fetch --gateway --port <dynamic>` | There is no `--port` flag. Port comes only from `.harness/gateway.json` (`src/gateway/mod.rs:78`) | [G11](#task-g11---gateway-port-cli-flag), [T2](#42-task-breakdown--phase-2) |
| C8 | Health check is unauthenticated | `auth_middleware` is layered over the whole router with no path exemption (`src/gateway/mod.rs:136`); `/health` requires the key when auth is enabled | [G12](#task-g12-health-exemption--readiness), [9](#9-security-considerations) |
| C9 | Interrupt/cancel exists in the UI plan | `Harness::interrupt()` exists (`src/harness.rs:427`) but no endpoint exposes it. Esc-to-cancel (F20) has nothing to call | [G13](#task-g13-interrupt-endpoint) |
| C10 | `risk` is `"low"\|"medium"\|"high"` (also written as `"destructive"` elsewhere) | Sandbox produces `DestructiveCheck { is_destructive, pattern, category }` and a binary `requires_confirmation` (`src/sandbox/mod.rs:225,264`). No 3-level scale exists | [5](#5-sse-event-protocol) |
| C11 | Memory search via `MemorySidecar::search_for_context` | That is enrichment-shaped (takes user input, returns `SearchResult` for prompt injection). General search is `ObsidianVault::search(&MemoryQuery)` (`src/memory/vault.rs:321`) behind a **`std::sync::Mutex`** — must not be locked across an `await` | [G5](#task-g5-memory-vault-search) |
| C12 | Static output served from `web/dist/` | Next.js static export writes to `web/out/`. v1 used both names | [G9](#task-g9-static-file-serving-optional) |

---

## 1. Overview

### Goal

Provide a graphical interface for momo-fetch so users who aren't comfortable with CLI can use the full power of the agent harness — chat, tool execution, memory vault, agent switching, MCP management, and more.

### Design Principles

- **Gateway-first:** All new features go through the existing axum gateway. Frontend is a pure consumer.
- **OpenAI-compatible base:** Keep existing `/v1/chat/completions` working. Add new `/v2/` endpoints for rich events alongside it.
- **Progressive disclosure:** Simple chat works immediately. Advanced features (agents, memory, MCP) are in sidebars/panels.
- **Real-time:** SSE streaming for text + tool calls + approval flow. No polling where possible.
- **Single-user by construction:** The harness is one mutable object with one current session. The UI must not pretend otherwise (see [2.3](#23-concurrency-model-corrected)).
- **Offline-capable in Phase 2:** Tauri wraps the web UI for desktop use without a browser.

### What We Already Have

Updated at `546ef04`. ✅ = shipped, ⚠️ = present but limited, ❌ = not built.

| Component | Status | Location |
|-----------|--------|----------|
| Gateway HTTP server | ✅ Running | `src/gateway/mod.rs` |
| Chat completions (non-stream) | ✅ | `POST /v1/chat/completions` |
| Chat completions (SSE stream) | ⚠️ Text only, by design — superseded by `/v2/chat/stream` | `src/gateway/handlers.rs:106` |
| Sessions CRUD | ✅ | `/v1/sessions/*` |
| Session messages | ⚠️ Text parts only — G8 extends | `src/gateway/handlers.rs:266` |
| Models list | ⚠️ Returns *current* model — catalogue is on `/v2/providers` | `GET /v1/models` |
| Cost tracking | ✅ **G10** — shared turn lifecycle, REPL + gateway both wired | `src/harness.rs:440-456`, `GET /v1/cost` |
| Auth + rate limit | ✅ router split; `/health` exempt | `src/gateway/auth.rs`, `mod.rs:213` |
| Health check + readiness | ✅ **G12** — auth-exempt, reports `turn_active` | `GET /health` |
| Rich SSE stream + approvals | ✅ **G1** — typed events, 4-phase approval stitch, disconnect cleanup | `src/gateway/v2_handlers.rs:58,160` |
| Turn guard (process-wide) | ✅ RAII lease, 409 `turn_in_progress`, 7 unit tests | `src/gateway/turn.rs` |
| Agent registry | ✅ **G2** | `GET/POST /v2/agents*` |
| Provider/model switching | ✅ **G3** — incl. `list_all()` so unconfigured report `available:false` | `/v2/providers`, `/v2/switch*`, `src/providers.rs:334,456` |
| Permission mode & settings | ✅ **G7** — incl. `DELETE /v2/settings/approved-tools` | `/v2/settings*` |
| Interrupt / cancel turn | ✅ **G13** | `POST /v2/chat/interrupt` |
| Bind/port flags | ✅ **G11** — `--gateway-port 0`, `MOMO_GATEWAY_LISTENING` line | `src/cli/mod.rs`, `src/gateway/mod.rs` |
| MCP server status | ❌ **G4** open — no gateway endpoint | `src/mcp/mod.rs:247` (`all_statuses`, async) |
| Memory vault search | ❌ **G5** open | `src/memory/vault.rs:321` |
| File read/preview | ❌ **G6** open — security-sensitive | `src/sandbox/mod.rs:123,205` |
| Static UI serving | ❌ **G9** open | — |
| Frontend (`web/`) | ❌ Nothing exists. F1–F29 open | — |
| Tauri shell | ❌ Nothing exists. T1–T13 open | — |
| Skills | ❌ Not in this spec (deferred) | `src/skill/` |

**Known gap carried forward:** no LLM turn completes on the current dev machine (no provider credits + a non-tool-capable default model). The approval round-trip and cost parity are therefore **spec-verified but not wire-verified** — see [§12.5 Wave 0](#125-wave-schedule).

---

## 2. Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                    Phase 2: Tauri Shell                     │
│  ┌───────────────────────────────────────────────────────┐  │
│  │              Phase 1: Next.js Web UI                   │  │
│  │  ┌────────┐ ┌──────────────┐ ┌────────────────────┐  │  │
│  │  │Sidebar │ │  Chat Panel  │ │  Detail Panel     │  │  │
│  │  │Sessions│ │  (SSE stream)│ │  Progress/Artifacts│  │  │
│  │  │Agents  │ │              │ │  Files/Approval    │  │  │
│  │  │Tools   │ │  Input box   │ │                    │  │  │
│  │  └───┬────┘ └──────┬───────┘ └────────┬───────────┘  │  │
│  │      └─────────────┼─────────────────┘               │  │
│  │                    │ fetch (POST + ReadableStream)    │  │
│  │                    ▼                                   │  │
│  │          HTTP Client (browser fetch)                  │  │
│  └────────────────────┬──────────────────────────────────┘  │
└───────────────────────┼──────────────────────────────────────┘
                        │
                        ▼
┌───────────────────────────────────────────────────────────┐
│              Gateway (axum) — `cargo run -- --gateway`      │
│  ┌──────────────────────────────────────────────────────┐  │
│  │  /v1/*  (existing, OpenAI-compatible)                │  │
│  │  /v2/*  (new, rich events + management)              │  │
│  │  TurnGuard (one active turn, process-wide)           │  │
│  │  ApprovalRegistry (oneshot per pending tool name)    │  │
│  └──────────────────────────────────────────────────────┘  │
│                         │                                  │
│                         ▼                                  │
│  ┌──────────────────────────────────────────────────────┐  │
│  │              Harness (Arc<RwLock<Harness>>)           │  │
│  │  Runner │ Provider │ Sandbox │ Vault │ MCP │ Skills   │  │
│  │  ── single `current_session_id` (global state) ──     │  │
│  └──────────────────────────────────────────────────────┘  │
└───────────────────────────────────────────────────────────┘
```

### 2.1 Key Architectural Decisions

1. **Single Harness, RwLock:** Gateway already wraps Harness in `Arc<RwLock<Harness>>`. Fine for single-user desktop use. Multi-user scaling is out of scope for Phase 1–2.

2. **`/v2/` prefix for new endpoints:** Existing `/v1/` stays OpenAI-compatible (for external tooling). New rich endpoints use `/v2/`.

3. **Two stream modes:**
   - `/v1/chat/completions/stream` — OpenAI-compatible SSE (text chunks only, existing, unchanged)
   - `/v2/chat/stream` — Rich SSE with tool calls, progress, approval (new)

4. **Frontend lives in `web/`** as a separate npm package. Not embedded in the Rust binary (except optional `ServeDir`, [G9](#task-g9-static-file-serving-optional)).

### 2.2 Lock discipline (mandatory)

`tokio::sync::RwLock` is write-preferring: a waiting writer blocks new readers. Combined with a human-in-the-loop pause, naive locking deadlocks the whole gateway.

Rules for every `/v2` handler:

- **Never hold a guard across a human-latency `await`.** The approval wait (up to 5 min) must happen with **no** guard held.
- **Never hold a guard across the lifetime of the SSE body.** Acquire → do work → drop, per phase. The existing v1 handler already drops its guard before the stream body runs; keep that shape.
- **Never hold the vault's `std::sync::Mutex` across an `await`** (`src/harness.rs:28`). Copy results out, or wrap in `spawn_blocking`.
- Read-lock phases: emitting events from an in-flight `EventStream`, all `GET /v2/*` endpoints.
- Write-lock phases: `resume_session`, `switch_*`, `switch_agent`/`clear_agent`, `switch_permission`, and `run_confirmation_turn` (all call `rebuild_runner`).

### 2.3 Concurrency model (corrected)

`Harness` is not per-session. It holds one `current_session_id`, one runner, and one provider. Consequences:

- **Turn concurrency is process-wide, not per-session.** A `TurnGuard` (`tokio::sync::Mutex<Option<TurnState>>` in `GatewayState`) admits one active turn at a time. Second concurrent request → **409 Conflict** with `{"error":{"code":"turn_in_progress","active_session_id":"…"}}`.
- **Session switching is global.** Tab A resuming session X while tab B streams session Y is a data-integrity bug, not a UX quirk. Mitigation for Phase 1: the 409 guard covers the streaming window; outside it, last-write-wins and the UI shows the gateway's actual `current_session_id` (echoed in the `role` event) rather than its local belief.
- **Approval state is global too** (`approved_tools` is per-process). See below.

### 2.4 Approval flow (corrected)

**How it actually works** (`src/cli/repl.rs:620-660`, `src/harness.rs:405-424`):

1. The runner's `EventStream` yields an event carrying `event.actions.tool_confirmation { tool_name, … }`.
2. The stream then **ends**. There is nothing to "resume".
3. The caller decides, then calls `harness.run_confirmation_turn(&tool_name, approved)`:
   - if approved → inserts `tool_name` into `approved_tools` and calls `rebuild_runner()` so `RunConfig` carries the decision (**needs `&mut self`**);
   - starts a **new** turn whose user content is the literal string `"approved"` or `"denied"`.
4. Because the decision is baked into `RunConfig` by tool **name**, that tool is never confirmed again for the life of the process.

**What the gateway does with this.** The client still sees one continuous SSE response — the stitching happens server-side:

```
Phase A (read lock)   stream events until EventStream ends
                      ↓ tool_confirmation seen?
                      emit `approval_required`, DROP the read guard
Phase B (no lock)     await oneshot from ApprovalRegistry, 5-min timeout
Phase C (write lock)  harness.run_confirmation_turn(tool_name, approved)
                      emit `approval_resolved`, drop write guard
Phase D (read lock)   stream the follow-up EventStream into the same response
                      (loop back to Phase A if it also ends in a confirmation)
```

**Registry keying.** Because the underlying decision is per tool name, the registry is keyed by `(turn_id, tool_name)`. `call_id` is still sent to the client for display/correlation, and the approve/deny request must echo both — the gateway rejects a mismatched `call_id` with 409 (stale click from a previous turn).

**Sticky-approval disclosure (required UX).** The approval dialog must state that approving grants the tool for the rest of the session, because that is what the code does. Silent stickiness on `shell_exec` is a security surprise. Two options for the dialog, decided in [Q9](#11-open-questions):

- "Approve `shell_exec` for the rest of this session" (honest, matches code), or
- add per-call approval to the harness (larger change, out of Phase 1 scope).

**Timeout:** no approval within 5 minutes → auto-deny. Emit `approval_resolved {approved:false, reason:"timeout"}`, run the deny turn, then continue. Timeout is configurable via `.harness/gateway.json` → `approval_timeout_secs`.

**Denial:** `run_confirmation_turn(name, false)` does **not** record anything; it just sends `"denied"` to the model. Denial is therefore *not* sticky — the model may ask again next turn. Reflect that in the dialog copy.

---

## 3. Phase 1: Web UI (Next.js)

### 3.1 Gateway API Enhancements (Rust Side)

Every task below lists explicit acceptance criteria. A task is not done until its criteria pass via the curl probes in [§10](#10-testing-strategy).

#### Task G1: Rich SSE Stream (`/v2/chat/stream`)

**Priority:** P0 (blocks everything else)

**Current state:** `chat_completions_stream` (`src/gateway/handlers.rs:106`) only forwards `Part::Text`. `Part::FunctionCall` and `Part::FunctionResponse` are silently dropped, and `event.actions.tool_confirmation` is ignored entirely — a strict-mode turn over the gateway today simply stops producing output with no explanation.

**Required changes:**

Add `POST /v2/chat/stream` emitting typed SSE events (`event:` + `data:`):

```
event: role
data: {"role":"assistant","model":"anthropic/claude-sonnet-4-20250514","provider":"anthropic","session_id":"abc123","agent":null,"turn_id":"t_01"}

event: text
data: {"content":"Let me look at that file..."}

event: tool_call_start
data: {"id":"call_001","name":"file_read","args":{"path":"src/main.rs"}}

event: tool_call_result
data: {"id":"call_001","name":"file_read","status":"done","output_preview":"fn main() {...}","truncated":true}

event: approval_required
data: {"turn_id":"t_01","call_id":"call_002","name":"shell_exec","args":{"command":"rm -rf /tmp/build"},"destructive":true,"category":"Destructive deletion","sticky":true,"expires_at":"2026-07-28T12:05:00Z"}

event: approval_resolved
data: {"call_id":"call_002","approved":true,"reason":"user"}

event: usage
data: {"prompt_tokens":1500,"completion_tokens":300,"cost_usd":0.0045}

event: context_usage
data: {"used":42000,"total":200000,"percent":21.0}

event: done
data: {"turn_id":"t_01","stop_reason":"complete"}
```

Implementation notes:

- **Part mapping:** `Part::Text` → `text`; `Part::FunctionCall{id,name,args}` → `tool_call_start`; `Part::FunctionResponse{id,name,response}` → `tool_call_result`. Preview is truncated to 2 KB with `truncated:true`; the UI fetches the full value via `/v2/files` when the tool was a file read.
- **Turn guard:** acquire the `TurnGuard` before the first lock. If already held → 409 (JSON body, not an SSE stream).
- **Approval:** per [2.4](#24-approval-flow-corrected). No guard held during the wait.
- **Client disconnect:** detect via the SSE sink closing; call `harness.interrupt()`, drop any pending approval oneshot as denied, and release the `TurnGuard`. Without this, one aborted browser tab wedges the gateway.
- **Keep-alive:** reuse `Sse::keep_alive(KeepAlive::default())` — needed to keep proxies from killing an idle 5-minute approval wait.
- **Errors mid-stream:** emit `event: error` then `event: done {"stop_reason":"error"}` and close. Never drop the connection silently.

**Files to modify:** `src/gateway/handlers.rs` (new `v2_chat_stream`), `src/gateway/types.rs` (V2 event types), `src/gateway/mod.rs` (routes + `GatewayState` additions).

**New state in `GatewayState`:**

```
turn_guard:  Arc<Mutex<Option<TurnState>>>        // { turn_id, session_id, started_at }
approvals:   Arc<Mutex<HashMap<ApprovalKey, oneshot::Sender<bool>>>>
             ApprovalKey = (turn_id: String, tool_name: String)
```

**New types in `types.rs`:**

```
V2ChatRequest {
  session_id: Option<String>,       // resume this session (write lock; global switch)
  messages:   Vec<ChatMessage>,     // OpenAI-compatible, same as v1
  agent:      Option<String>,       // switch agent for this turn
  model:      Option<String>,       // override model for this turn
}

V2StreamEvent (enum, serialized as SSE event type):
  Role            { role, model, provider, session_id, agent, turn_id }
  Text            { content }
  ToolCallStart   { id, name, args }
  ToolCallResult  { id, name, status: "done"|"error", output_preview, truncated }
  ApprovalRequired{ turn_id, call_id, name, args, destructive, category, sticky, expires_at }
  ApprovalResolved{ call_id, approved, reason: "user"|"timeout"|"disconnect" }
  Usage           { prompt_tokens, completion_tokens, cost_usd }
  ContextUsage    { used, total, percent }
  Error           { code, message }
  Done            { turn_id, stop_reason: "complete"|"error"|"interrupted" }

ApprovalRequest { turn_id: String, call_id: String, tool_name: String, approved: bool }
```

**Acceptance:**
- Strict mode + a `shell_exec` prompt produces `approval_required`, and the stream continues after `POST /v2/chat/approve`.
- A second concurrent POST returns 409 with `turn_in_progress`.
- Killing the client mid-turn frees the guard within 1 s (next request succeeds).
- `agent`/`model` overrides are applied and echoed in `role`.

---

#### Task G2: Agent Management Endpoints

**Priority:** P0

`AgentRegistry::list()` returns `Vec<&AgentDef>` with `name`, `description: Option<String>`, `capabilities: Vec<String>`, plus optional `model`/`provider`/`tools` overrides worth surfacing.

```
GET  /v2/agents          → { "agents": [{ "name","description","capabilities","model","provider","is_current","is_orchestrator" }], "current": "sales"|null }
POST /v2/agents/switch   → { "name": "sales" }  → { "switched_to": "sales", "model": "...", "provider": "..." }
POST /v2/agents/default  → { "switched_to": null }
```

Notes: `switch_agent`/`clear_agent` (`src/harness.rs:485,537`) take `&mut self` and rebuild the runner — **write lock**, and must be rejected with 409 while a turn is active (rebuilding under an in-flight runner is undefined). `is_orchestrator` comes from `AgentRegistry::is_orchestrator`.

**Acceptance:** switching to an agent with a `model` override changes the model reported by `GET /v1/models`; switching during an active turn returns 409.

---

#### Task G3: Provider & Model Switching

**Priority:** P0

```
GET  /v2/providers       → { "providers":[{"name","is_current","current_model","available_models":[...]}] }
POST /v2/switch-model    → { "model": "claude-sonnet-4-20250514" } → { "provider","model" }
POST /v2/switch-provider → { "provider": "anthropic" }             → { "provider","model" }
POST /v2/switch          → { "provider": "anthropic","model":"..." } → { "provider","model" }
```

**Implementation:** `ProviderManager::list_available()` (`src/providers.rs:264`) → `Vec<ProviderInfo>`; switching via `Harness::switch_model` / `switch_provider` / `switch` (`src/harness.rs:432-472`), each `&mut self` + `rebuild_runner` + cost-context reset.

Notes: **write lock**; 409 while a turn is active. `list_available` reflects configured credentials — a provider with no API key must be returned with `available:false` rather than omitted, so the UI can explain *why* it's not selectable. Model lists may come from `context_window_cache`; treat a cold cache as "unknown", not "empty" (call `prefetch_context_windows` at gateway start).

**Acceptance:** `GET /v2/providers` lists every configured provider with the current one flagged; a switch is reflected in the next `role` event.

---

#### Task G4: MCP Server Status

**Priority:** P1

```
GET /v2/mcp/servers → { "servers":[{"id","status","transport":"stdio"|"http","tool_count":null,"error":null}], "running": 2 }
```

**Implementation:** `McpService::all_statuses()` is **async** and returns `HashMap<String, ServerStatus>` (`src/mcp/mod.rs:247`); `running_count()` for the summary.

**Known limitation:** `McpService::toolset()` returns a single merged `Option<Arc<dyn Toolset>>` — there is no per-server tool attribution. Either report `tool_count: null` per server plus a global `total_tools`, or add per-server accounting in `src/mcp/`. Phase 1 takes the former; the UI (F14) must not render "0 tools" for unknown.

**Acceptance:** a deliberately broken stdio server appears with a non-running status and a populated `error`.

---

#### Task G5: Memory Vault Search

**Priority:** P1

```
GET /v2/memory/search?q=<query>&limit=10 → { "results":[{"title","path","score","preview"}] }
GET /v2/memory/stats                     → { "total_memcells","projects","auto_write_enabled","auto_search_enabled" }
```

**Implementation:** use `ObsidianVault::search(&MemoryQuery)` (`src/memory/vault.rs:321`) — **not** `MemorySidecar::search_for_context`, which is enrichment-shaped (input → prompt injection) and applies the sidecar's own relevance gate. Stats: `ObsidianVault::stats()` / `counters()`; toggles from `MemorySidecar::auto_search_enabled()`.

**Concurrency:** the vault sits behind `Arc<Mutex<…>>` — a **`std::sync::Mutex`** (`src/harness.rs:28`). Lock, collect results into owned values, unlock, then serialise. Do not `await` while holding it; if search proves slow, move the whole call into `tokio::task::spawn_blocking` with a cloned `Arc`.

**Validation:** `limit` clamped to 1..=100; empty `q` → 400.

**Acceptance:** searching a known memcell title returns it with `score > 0`; `limit=1000` is clamped, not honoured.

---

#### Task G6: File Read / Sandbox Preview

**Priority:** P1

```
GET /v2/files?path=<relative>          → { "path","content","size","truncated","binary":false } | 403 | 404
GET /v2/files/tree?path=<dir>&depth=2  → { "entries":[{"name","path","is_dir","size"}] }
```

**Implementation:** `FilesystemSandbox::resolve_path` (`src/sandbox/mod.rs:123`) then `check_readable` (`:205`). Filter with `is_ignored` (`:170`) so `.gitignore`d files and secrets don't leak into the browser. There is **no** existing tree walker — implement one bounded by `depth ≤ 3` and 1000 entries.

**Hardening (beyond v1's one-liner):**
- Reject absolute paths, `..` segments, and symlinks that escape root — `resolve_path` canonicalises; verify the result still has the sandbox root as prefix, and re-check after following symlinks.
- Cap response at 1 MB; set `truncated:true` beyond that.
- Detect binary (NUL in first 8 KB) and return `binary:true` with no content instead of mangled UTF-8.
- Always 403 (never 404) for out-of-sandbox paths, so the endpoint can't be used to probe the filesystem layout.

**Acceptance:** `path=../../etc/passwd` → 403; a `.gitignore`d file → 403; a 5 MB log → truncated, no OOM.

---

#### Task G7: Permission Mode & Settings

**Priority:** P2

```
GET  /v2/settings            → { "permission_mode":"strict"|"auto"|"yolo","project_path","sandbox_root","memory":{...},"approved_tools":[...] }
POST /v2/settings/permission → { "mode":"auto" } → { "mode":"auto" }
```

**Implementation:** `Harness::switch_permission(PermissionMode)` (`src/harness.rs:474`) — `&mut self`, rebuilds the runner → **write lock**, 409 during a turn. `PermissionMode` already has `Display`/`FromStr` (`src/sandbox/mod.rs:12-40`), so parse with `FromStr` and 400 on anything else.

Surface `approved_tools` — the user should be able to see which tools they've already granted for the process. A `DELETE /v2/settings/approved-tools` to clear the set is a small addition worth including (needs a new `Harness` method; the field is private).

**Acceptance:** switching to `yolo` makes a subsequent `shell_exec` turn run with no `approval_required` event.

---

#### Task G8: Session Messages With Tool Calls

**Priority:** P1

```
GET /v2/sessions/{id}/messages → { "messages":[{"role","content","timestamp","tool_calls":[{"id","name","args","result_preview"}]}] }
```

**Correction to v1:** `GET /v1/sessions/{id}` already returns a `messages` array; it just drops non-text parts (`src/gateway/handlers.rs:273-290`). This task walks the same `session.events().all()` but also maps `Part::FunctionCall`/`Part::FunctionResponse`, pairing them by call id so the UI can rebuild tool-call cards on reload.

**Acceptance:** reloading a session that used tools renders the same tool-call cards as the live stream did.

---

#### Task G9: Static File Serving (Optional)

**Priority:** P2

```
GET /ui/*  → serve files from web/out/    (Next.js static export directory)
```

Use `tower_http::services::ServeDir` with `.fallback(ServeFile::new("web/out/index.html"))` for client-side routes. Exempt `/ui/*` from `auth_middleware` — an HTML shell behind a bearer header cannot be loaded by a browser navigation. Path is configurable (`gateway.json` → `ui_dir`) since the binary and the build output aren't co-located in dev.

**Files to modify:** `src/gateway/mod.rs`, `Cargo.toml` (`tower-http` `fs` feature).

---

#### Task G10: Cost & Context Tracking in Gateway Turns

**Priority:** P0 (new — see [C6](#0-code-audit--corrections-to-v1))

Cost is currently recorded only by the REPL. Gateway turns leave `CostTracker` untouched, so `GET /v1/cost` under-reports, the header cost badge (F18) shows stale numbers, and `usage`/`context_usage` have nothing to emit.

Mirror the REPL's lifecycle inside `v2_chat_stream` (and `/v1` handlers for consistency):

```
reset_turn()                          before streaming            (src/cli/repl.rs:415)
record_event(usage) per event         when event carries usage    (src/cli/repl.rs:644)
finalize_turn()                       at end of turn              (src/cli/repl.rs:537)
reset_turn() again before the confirmation follow-up turn         (src/cli/repl.rs:522)
```

Then emit `usage` from `record_event`'s return and `context_usage` from `ContextWindowInfo::new(prompt_tokens, provider, model)` + `percentage()` (`src/context_window.rs:245,253`).

**Extract, don't duplicate:** the REPL and the gateway must not drift. Lift the loop into a shared helper (e.g. `src/harness.rs` or a new `src/turn.rs`) that both call.

**Acceptance:** a gateway turn increases `GET /v1/cost`'s session total by the same amount an equivalent REPL turn does.

---

#### Task G11: `--gateway-port` CLI Flag

**Priority:** P1 (new — blocks Phase 2 T2/T3)

Port is read only from `.harness/gateway.json` (`src/gateway/mod.rs:78-92`). Tauri needs a free port chosen at launch and must not rewrite the user's config file.

- Add `--gateway-port <u16>` to `src/cli/mod.rs` (next to the existing `--gateway` flag at line 50), overriding `GatewayConfig::port`.
- Add `--gateway-bind <addr>` defaulting to `127.0.0.1` (currently hard-coded, `mod.rs:92`). Keep loopback as the default; binding wider requires auth to be enabled (fail fast otherwise — see [§9](#9-security-considerations)).
- Print the resolved `http://addr:port` on a single, machine-parseable line so the Tauri supervisor can read it from stdout instead of guessing (`MOMO_GATEWAY_LISTENING http://127.0.0.1:53124`). Passing `--gateway-port 0` binds an OS-assigned port and reports it on that line.

**Acceptance:** `momo-fetch --gateway --gateway-port 0` prints the actual bound port and serves on it.

---

#### Task G12: Health Exemption & Readiness

**Priority:** P1 (new — see [C8](#0-code-audit--corrections-to-v1))

`auth_middleware` is layered over the entire router with no path exemption (`src/gateway/mod.rs:136`), so `/health` is 401 when auth is enabled — which breaks Tauri's readiness poll (T2) and any container health check.

- Exempt `/health` (and `/ui/*` per G9) from `auth_middleware`.
- Extend the payload to a genuine readiness signal: `{ "status":"ok","version","provider","model","session_id","mcp_running":2,"turn_active":false }`. `turn_active` lets the UI recover its state after a reload without probing.

**Acceptance:** with auth enabled, `curl /health` (no header) → 200; `curl /v2/agents` (no header) → 401.

---

#### Task G13: Interrupt Endpoint

**Priority:** P1 (new — see [C9](#0-code-audit--corrections-to-v1))

```
POST /v2/chat/interrupt → { "turn_id": "t_01" } → { "interrupted": true }
```

Calls `Harness::interrupt()` (`src/harness.rs:427`, delegates to `runner.interrupt(session_id)`) — read lock only. The active stream then emits `done {"stop_reason":"interrupted"}` and releases the `TurnGuard`. This is what F20's Escape key binds to.

**Acceptance:** Escape during a long turn stops output within ~1 s and the next turn starts cleanly.

---

### 3.2 Frontend Architecture

#### SSE transport decision

**v1 was wrong here.** `EventSource` cannot issue a POST or set an `Authorization` header, and `/v2/chat/stream` needs both a JSON body and (optionally) a bearer token. Options:

| Option | Verdict |
|--------|---------|
| **`fetch` + `ReadableStream` + custom SSE parser** | ✅ **Chosen.** Supports POST, headers, and `AbortController` for cancel. Costs: implement the framing parser (~60 lines) and manual reconnect. |
| POST `/v2/chat/turns` → `{turn_id}`, then `EventSource('/v2/chat/stream?turn_id=…')` | Works and gives free reconnect, but needs server-side turn buffering and a second endpoint. Revisit only if reconnect proves necessary. |
| WebSocket | Rejected — bidirectional framing for an approval click isn't worth abandoning SSE, and the gateway is already SSE-shaped. |

Parser requirements: handle `event:`/`data:`/`id:` lines, multi-line `data:`, `\r\n` and `\n`, comment lines (`:` keep-alives), and partial chunks split mid-frame. Reconnect is **not** automatic — on transport error, show a retry affordance rather than silently re-sending the turn (re-sending would double-bill and possibly re-run tools).

#### Tech Stack

| Layer | Choice | Rationale |
|-------|--------|-----------|
| Framework | Next.js 15 (App Router, React 19) | v1 said 14; 15 is the current line and required for React 19 + Tailwind v4 tooling |
| UI Components | shadcn/ui + Radix | Accessible, composable, dark mode built-in |
| Styling | Tailwind CSS v4 | shadcn/ui supports v4; use the CSS-first `@theme` config |
| State | Zustand | Lightweight, no boilerplate |
| SSE Client | `fetch` + `ReadableStream` + custom parser | See decision above — **not** `EventSource` |
| Markdown | react-markdown + remark-gfm | Render agent responses |
| Code Highlight | Shiki | Better grammar coverage than Prism; precompute the highlighter once |
| Icons | Lucide React | shadcn/ui default icon set |
| Build | Next.js static export (`output: 'export'`) | Tauri loads static files, no Node server needed |

**Static-export constraints** (call out before F1): `output: 'export'` disables Route Handlers, Middleware, ISR, and `next/image` optimisation. Everything dynamic must go to the gateway; set `images: { unoptimized: true }`. There is no server-side proxy, so **CORS on the gateway is load-bearing** for browser use.

#### Directory Structure

```
web/
├── package.json
├── next.config.ts
├── tsconfig.json
├── public/
│   └── favicon.svg
├── src/
│   ├── app/
│   │   ├── layout.tsx              # Root layout, dark mode, fonts
│   │   ├── page.tsx                # Main chat page
│   │   └── globals.css             # Tailwind v4 @theme tokens
│   ├── components/
│   │   ├── chat/
│   │   │   ├── chat-panel.tsx      # Main chat area (message list + input)
│   │   │   ├── message-bubble.tsx  # Single message (user/assistant/system)
│   │   │   ├── message-content.tsx # Markdown renderer for assistant messages
│   │   │   ├── chat-input.tsx      # Input box with send button + file drop
│   │   │   ├── tool-call-card.tsx  # Expandable tool call visualization
│   │   │   └── approval-dialog.tsx # Approve/Deny modal (states sticky scope)
│   │   ├── sidebar/
│   │   │   ├── session-list.tsx    # Session sidebar with create/delete/resume
│   │   │   ├── agent-picker.tsx    # Agent personality selector
│   │   │   ├── model-selector.tsx  # Provider + model dropdown
│   │   │   └── tool-status.tsx     # MCP server status indicators
│   │   ├── detail/
│   │   │   ├── detail-panel.tsx    # Right panel container (tabbed)
│   │   │   ├── progress-tab.tsx    # Step-by-step progress (tool calls this turn)
│   │   │   ├── artifacts-tab.tsx   # Files created/modified this turn
│   │   │   ├── files-tab.tsx       # File tree browser (sandbox)
│   │   │   └── memory-tab.tsx      # Memory vault search
│   │   ├── layout/
│   │   │   ├── app-shell.tsx       # 3-panel responsive layout
│   │   │   ├── sidebar.tsx         # Left sidebar container
│   │   │   └── header.tsx          # Top bar (connection status, cost)
│   │   └── shared/
│   │       ├── status-badge.tsx    # Colored dot + label
│   │       ├── code-block.tsx      # Code with copy button + language label
│   │       └── loading-spinner.tsx # Thinking indicator
│   ├── hooks/
│   │   ├── use-chat-stream.ts      # fetch stream + AbortController + state
│   │   ├── use-sessions.ts         # Session CRUD
│   │   ├── use-agents.ts           # Agent list + switch
│   │   ├── use-models.ts           # Provider/model switching
│   │   └── use-api.ts              # Base fetch wrapper with error handling
│   ├── stores/
│   │   ├── chat-store.ts           # Messages, streaming state, current turn
│   │   ├── session-store.ts        # Active session, session list
│   │   ├── config-store.ts         # Provider, model, agent, permission mode
│   │   └── ui-store.ts             # Panel visibility, sidebar collapse, theme
│   ├── lib/
│   │   ├── api-client.ts           # HTTP client; base URL resolution (see below)
│   │   ├── sse-parser.ts           # Frame parser for the V2 event stream
│   │   └── types.ts                # TS types generated/mirrored from Rust types
│   └── styles/
│       └── theme.ts                # Dark/light theme tokens
└── .env.local                      # NEXT_PUBLIC_GATEWAY_URL=http://localhost:3000
```

**Gateway URL resolution** (one place, `api-client.ts`): `window.__GATEWAY_URL__` (injected by Tauri, T3) → `process.env.NEXT_PUBLIC_GATEWAY_URL` → `http://localhost:3000`. Because `output: 'export'` inlines env vars at build time, the Tauri path *must* be the runtime injection, not the env var.

**Type parity:** `lib/types.ts` mirrors `src/gateway/types.rs`. Keep them in sync by deriving `ts-rs`/`schemars` output from the Rust types in CI rather than hand-copying; a drifted event enum fails silently at runtime (unknown `event:` names get dropped by the parser). Minimum bar: the parser logs and surfaces unknown event types instead of ignoring them.

---

### 3.3 Task Breakdown — Phase 1

#### Sprint 1: Core Chat (Week 1)

| # | Task | Type | Depends On | Description |
|---|------|------|------------|-------------|
| F1 | Project scaffold | Frontend | — | `npx create-next-app web/`, shadcn/ui, Tailwind v4, zustand. `output:'export'`, `images.unoptimized`. |
| F2 | API client + types | Frontend | F1 | `lib/api-client.ts` (URL resolution chain, bearer header, uniform error parsing), `lib/types.ts` mirroring `types.rs`. |
| F3 | SSE stream client | Frontend | F1 | `lib/sse-parser.ts` — **`fetch` + `ReadableStream`**, frame parser, `AbortController`, unknown-event logging. Not `EventSource`. |
| G1 | Rich SSE stream | Gateway | G10 | `/v2/chat/stream` with turn guard, part mapping, approval stitching, disconnect cleanup. |
| G10 | Cost/context tracking | Gateway | — | Shared turn lifecycle helper; wire `reset/record/finalize` into gateway turns. Prerequisite for G1's `usage` events. |
| G13 | Interrupt endpoint | Gateway | G1 | `POST /v2/chat/interrupt`. |
| G2 | Agent endpoints | Gateway | — | `GET /v2/agents`, `POST /v2/agents/switch`, `POST /v2/agents/default`. 409 during active turn. |
| G3 | Model/provider endpoints | Gateway | — | `GET /v2/providers`, `POST /v2/switch-model`, `/switch-provider`, `/switch`. |
| G12 | Health exemption + readiness | Gateway | — | Auth-exempt `/health`; richer readiness payload. |
| F4 | Chat store | Frontend | F2, F3 | Messages, streaming state, tool-call list, approval state, turn id. |
| F5 | App shell layout | Frontend | F1 | 3-panel layout, collapsible sidebars, dark mode. |
| F6 | Chat panel | Frontend | F3, F4, F5 | Message list with auto-scroll (pinned-to-bottom heuristic). Enter to send, Shift+Enter newline. |
| F7 | Message bubble + markdown | Frontend | F6 | Markdown with Shiki code blocks + copy button. |
| F8 | Tool call cards | Frontend | F3, F4, F6 | Expandable cards: name, args (collapsed), result preview, status (running/done/error/awaiting). |
| F9 | Approval dialog | Frontend | G1, F3, F8 | Modal on `approval_required`. **Must state sticky scope** and show `category` when destructive. Countdown to `expires_at`. Approve/Deny → `POST /v2/chat/approve`\|`/deny`. |
| G8 | Session messages | Gateway | — | `GET /v2/sessions/{id}/messages` including tool calls. |
| F10 | Session list | Frontend | F2 | List/create/switch/delete. Warn that switching is global (single-harness). |
| F11 | Session persistence | Frontend | F8, F10 | Load messages on switch; append streamed messages; restore tool-call cards. |
| F29 | Turn conflict handling | Frontend | F4, G1 | Handle 409 `turn_in_progress`: disable send, show "a turn is running", offer interrupt (G13). |

#### Sprint 2: Management Panels (Week 2)

| # | Task | Type | Depends On | Description |
|---|------|------|------------|-------------|
| F12 | Agent picker | Frontend | G2, F2 | Sidebar list from `GET /v2/agents`; switch; highlight current; show capabilities as tooltips. |
| F13 | Model selector | Frontend | G3, F2 | Provider → model dropdown. Grey out providers with `available:false` and say why. |
| G4 | MCP status endpoint | Gateway | — | `GET /v2/mcp/servers`. |
| F14 | Tool status panel | Frontend | G4, F2 | Status dots + error tooltip. Render `tool_count:null` as "—", never "0". |
| G5 | Memory search endpoint | Gateway | — | `GET /v2/memory/search`, `/v2/memory/stats`. |
| F15 | Memory tab | Frontend | G5, F2 | Debounced search (250 ms), results with path + preview, stats summary. |
| G6 | File read endpoints | Gateway | — | `GET /v2/files`, `/v2/files/tree`. Sandbox-enforced, ignore-filtered. |
| F16 | Files tab | Frontend | G6, F2 | Lazy-loading tree; click to preview; binary/truncated states rendered explicitly. |
| G7 | Settings endpoint | Gateway | — | `GET /v2/settings`, `POST /v2/settings/permission`, optional clear-approved-tools. |
| F17 | Settings panel | Frontend | G7, F2 | Permission mode (strict/auto/yolo) with a plain-language warning on `yolo`; approved-tools list with clear button. |
| F18 | Cost display | Frontend | F2, G10 | Header badge from `GET /v1/cost` + live `usage` events. Click for breakdown. |
| F19 | Connection status | Frontend | F2, G12 | Poll `/health` (10 s). Green/red dot; degraded state when `turn_active` is true but no local stream. |
| F20 | Keyboard shortcuts | Frontend | F5, G13 | Ctrl+N new session, Ctrl+K palette, Ctrl+B sidebar, **Esc → interrupt (G13)**. |

#### Sprint 3: Polish & Hardening (Week 3)

| # | Task | Type | Depends On | Description |
|---|------|------|------------|-------------|
| F21 | Error handling | Frontend | All | Toasts from the uniform error model (§8). Retry only on idempotent GETs — never auto-retry a turn. |
| F22 | Loading states | Frontend | All | Skeletons for session/agent lists; typing indicator. |
| F23 | Responsive layout | Frontend | F5 | Breakpoints per §6. Approval dialog must remain reachable on mobile. |
| F24 | Code block improvements | Frontend | F7 | Language detection, line numbers toggle, copy, "open in editor" (Tauri protocol handler). |
| F25 | File attach | Frontend | F6, G6 | Text files only in Phase 1: read client-side, prepend as fenced context, enforce a size cap. No multimodal. |
| F26 | Empty states | Frontend | All | New-session welcome, empty session list, "no agents configured" with a pointer to `.harness/agents/`. |
| F27 | Notification sounds | Frontend | F8 | Optional audio on turn complete / approval required / error. Off by default. |
| F28 | Local storage persistence | Frontend | F10, F12, F13 | Last session, agent, panel visibility, sidebar width. Reconcile against `/health` on load — server state wins. |
| G9 | Static file serving | Gateway | F1 | `GET /ui/*` from `web/out/`, auth-exempt, SPA fallback. |

---

## 4. Phase 2: Tauri Desktop Wrap

### 4.1 Tauri Integration

Wrap the Next.js static export in a Tauri v2 shell. The gateway runs as a child process managed by Tauri; the web view loads the exported static files.

```
momo-worker/  (Tauri project root)
├── src-tauri/
│   ├── Cargo.toml
│   ├── tauri.conf.json
│   ├── src/
│   │   ├── main.rs          # Tauri entry, launch gateway child process
│   │   ├── gateway.rs       # Spawn/supervise momo-fetch --gateway
│   │   └── commands.rs      # Tauri commands (file dialogs, native menus)
│   └── icons/
├── web/out/                 # Next.js static export (copied at build time)
└── package.json
```

#### Gateway Process Management

- Tauri spawns `momo-fetch --gateway --gateway-port 0` (**requires [G11](#task-g11---gateway-port-cli-flag)**) and reads the resolved URL from the `MOMO_GATEWAY_LISTENING` stdout line — more reliable than pre-picking a port, which races.
- Readiness: poll `GET /health` (**requires [G12](#task-g12-health-exemption--readiness)** if auth is enabled) with backoff, 30 s ceiling, before showing the UI.
- Frontend gets the URL via `window.__GATEWAY_URL__` injected with `WebviewWindow::eval`/init script (build-time env vars won't work with static export).
- Shutdown: SIGTERM, then SIGKILL after 5 s. On Windows there is no SIGTERM — use a job object or `taskkill`, otherwise the gateway leaks on quit.
- Crash: supervise and show an error screen with restart + last 50 lines of stderr.
- **Single instance:** the harness is process-global state; two app instances on the same project fight over `.harness/` and the session DB. Use `tauri-plugin-single-instance` and focus the existing window instead of spawning a second gateway.

#### Tauri-Specific Features

| Feature | Description |
|---------|-------------|
| System tray | Minimize to tray. Tray menu: New Session, Show Window, Quit. |
| Native menus | File (New, Open Project), Edit, View (Toggle Panels), Help. |
| File dialogs | "Open Project" → native folder picker → restart gateway with `--project`. |
| Deep links | `momo://open?path=/foo/bar`. **Validate the path** — a deep link is untrusted input that would otherwise re-root the sandbox anywhere on disk. Prompt the user to confirm the target directory. |
| Auto-update | Tauri updater plugin. Requires the signing keys from T12. |
| Window state | Remember size, position, maximized state. |

### 4.2 Task Breakdown — Phase 2

| # | Task | Type | Depends On | Description |
|---|------|------|------------|-------------|
| T1 | Tauri project scaffold | Desktop | Phase 1 complete | `npm create tauri-app`, v2 config, `web/out/` as frontendDist. Set the CSP to allow `connect-src http://127.0.0.1:*`. |
| T2 | Gateway process supervisor | Desktop | T1, G11, G12 | Spawn, parse the listening line, readiness poll, graceful shutdown (incl. Windows), crash recovery with stderr tail. |
| T3 | Runtime URL injection | Desktop | T2 | Inject `window.__GATEWAY_URL__` via init script before the app bundle runs. |
| T4 | Open Project dialog | Desktop | T1, T2 | Native folder picker → restart gateway with `--project <path>`; confirm-on-change since it re-roots the sandbox. |
| T5 | Single-instance guard | Desktop | T1 | `tauri-plugin-single-instance`; focus existing window. Prevents duplicate gateways over one `.harness/`. |
| T6 | System tray | Desktop | T1 | Tray icon, minimize-to-tray, context menu. |
| T7 | Native menus | Desktop | T1 | Menu bar wired to the same actions as F20's shortcuts. |
| T8 | Window state persistence | Desktop | T1, F5 | `tauri-plugin-window-state`. |
| T9 | Deep link protocol | Desktop | T1, T4 | Register `momo://`; validate and confirm paths. |
| T10 | Build pipeline | Desktop | T1 | CI: `next build` → copy `out/` → `tauri build` for macOS (.dmg), Windows (.msi), Linux (.deb/.AppImage). Bundle the `momo-fetch` binary as a Tauri sidecar per target triple. |
| T11 | Auto-updater | Desktop | T10, T13 | `tauri-plugin-updater`; check on startup. |
| T12 | App icon & branding | Desktop | T1 | .icns / .ico / .png, app name "MoMo Worker". |
| T13 | Code signing | Desktop | T10 | Apple notarization, Windows signing, updater signing key. Blocks T11. |

---

## 5. SSE Event Protocol

### V2 Stream Event Types

Every frame is `event: <type>` + `data: <json>`. Clients **must** ignore unknown event types without erroring (forward compatibility) but should log them.

| Event Type | Payload |
|---|---|
| `role` | `{ role, model, provider, session_id, agent, turn_id }` |
| `text` | `{ content }` |
| `tool_call_start` | `{ id, name, args }` |
| `tool_call_result` | `{ id, name, status: "done"\|"error", output_preview, truncated }` |
| `approval_required` | `{ turn_id, call_id, name, args, destructive, category, sticky, expires_at }` |
| `approval_resolved` | `{ call_id, approved, reason: "user"\|"timeout"\|"disconnect" }` |
| `usage` | `{ prompt_tokens, completion_tokens, cost_usd }` |
| `context_usage` | `{ used, total, percent }` |
| `error` | `{ code, message }` |
| `done` | `{ turn_id, stop_reason: "complete"\|"error"\|"interrupted" }` |

**On `destructive`/`category` (replaces v1's `risk`):** v1 specified `risk: "low"|"medium"|"high"` in one place and `"destructive"` in another. Neither exists in code. `FilesystemSandbox::check_destructive` (`src/sandbox/mod.rs:225`) returns `{ is_destructive, pattern, category }` where `category` is one of *Destructive deletion / Destructive git / Destructive SQL / Destructive system*. The event carries those directly. Confirmation itself is binary — `requires_confirmation` (`:264`) returns `bool` gated on `PermissionMode` and the `MUTATING_TOOLS` list. A synthetic risk scale would be a lie the UI can't back up.

**Ordering guarantees:** `role` is always first and `done` always last. `tool_call_result` always follows its `tool_call_start`. `approval_required` for a call always precedes that call's `tool_call_result`. `usage`/`context_usage` arrive before `done`. Nothing else is ordered.

**⚠️ `call_id` does not survive an approval** *(observed 2026-08-05, not a bug — a consequence of C1)*. The pre-approval `tool_call_start` and the post-approval one carry **different `id`s**, because the follow-up is genuinely a new turn with a new function call, not a resumption:

```
tool_call_start   {"id":"call-ab7c9a43…","name":"shell_exec"}   ← leg 1
approval_required {"call_id":"call-ab7c9a43…"}
approval_resolved {"call_id":"call-ab7c9a43…"}
tool_call_start   {"id":"call-b05ba433…","name":"shell_exec"}   ← leg 2, NEW id
tool_call_result  {"id":"call-b05ba433…","status":"done"}
```

**Consequence for F8/F9:** a UI that keys tool-call cards purely on `id` renders **two cards for one user-approved action** — one stuck forever in "awaiting approval", one that completes. Correlate the post-approval call to the pending card by **tool name + ordering** within the turn, and retire the pre-approval card when `approval_resolved{approved:true}` arrives. `usage` also arrives once per leg, so per-turn cost is the **sum** of the `usage` events, not the last one.

Also confirmed on the wire: the stream carries bare `:` **keep-alive comment frames** mid-turn. The F3 parser must skip them — this is observed behaviour, not a hypothetical.

### Sequence: Normal Turn (No Approval)

```
Client                          Gateway                         Harness
  │                                │                                │
  │─── POST /v2/chat/stream ──────>│                                │
  │    { messages: [...] }         │── acquire TurnGuard            │
  │                                │── read lock ──────────────────>│
  │                                │── run_turn_enriched() ────────>│
  │<── event: role ────────────────│                                │
  │<── event: text ────────────────│    (streaming)                 │
  │<── event: tool_call_start ─────│    file_read("src/main.rs")    │
  │<── event: tool_call_result ────│    ✓ done                      │
  │<── event: text ────────────────│    (more text)                 │
  │<── event: usage ───────────────│  (from record_event, G10)      │
  │<── event: context_usage ───────│                                │
  │<── event: done ────────────────│── finalize_turn(), release     │
```

### Sequence: Turn With Approval (strict mode) — corrected

```
Client                          Gateway                         Harness
  │                                │                                │
  │─── POST /v2/chat/stream ──────>│── TurnGuard, read lock ───────>│
  │<── event: role ────────────────│                                │
  │<── event: text ────────────────│                                │
  │<── event: tool_call_start ─────│  shell_exec("rm -rf /tmp/x")   │
  │                                │  EventStream ENDS with         │
  │                                │  actions.tool_confirmation     │
  │<── event: approval_required ───│  ⚠ DROP READ LOCK, then wait   │
  │                                │  (oneshot, 5-min timeout,      │
  │                                │   NO lock held)                │
  │   [User clicks Approve]        │                                │
  │─── POST /v2/chat/approve ─────>│  resolve oneshot               │
  │    { turn_id, call_id,         │                                │
  │      tool_name, approved }     │                                │
  │<── event: approval_resolved ───│── WRITE lock ─────────────────>│
  │                                │   run_confirmation_turn(       │
  │                                │     tool_name, true)           │
  │                                │   → approved_tools.insert()    │
  │                                │   → rebuild_runner()           │
  │                                │   → NEW turn, input "approved" │
  │                                │── drop write, take read ──────>│
  │<── event: tool_call_result ────│    ✓ done                      │
  │<── event: text ────────────────│    "Deleted successfully."     │
  │<── event: usage ───────────────│  (reset_turn ran before the    │
  │<── event: done ────────────────│   follow-up turn — G10)        │
```

Note the two things v1 got wrong and this diagram makes explicit: the stream **ends and is restarted** server-side rather than paused, and the approval is recorded against the **tool name**, permanently for the process.

---

## 6. UI Layout Specification

### Desktop Layout (>1024px)

```
┌─────────────────────────────────────────────────────────────────────┐
│ ◉ MoMo Worker          anthropic/claude-sonnet   💰 $0.04   🟢 Connected│
├──────────┬──────────────────────────────────┬────────────────────────┤
│ Sessions │         CHAT PANEL               │  Progress & Details   │
│──────────│                                  │────────────────────────│
│ 💬 Sess 1│  ┌──────────────────────────┐   │  ◉ Steps (this turn)  │
│ 💬 Sess 2│  │ 🤖 Assistant            │   │  ✓ Read src/main.rs   │
│ 💬 Sess 3│  │ Let me analyze the code │   │  ✓ Search for tests   │
│          │  │ ...                     │   │  ⏳ Write test file    │
│──────────│  │ 📎 file_read           │   │                        │
│ Agents   │  │   path: src/main.rs    │   │────────────────────────│
│ 🤖 Def   │  │   → 142 lines         │   │  📄 Artifacts          │
│ 🤖 Sales │  │ 📎 file_write          │   │  ├─ test_main.rs  NEW  │
│          │  │   path: tests/...      │   │  └─ summary.md   MOD   │
│──────────│  │   → Created            │   │                        │
│ Models   │  │                         │   │────────────────────────│
│ Anthropic│  │ 👤 You                  │   │  🔍 Memory              │
│  └ Sonnet│  │ Run the tests please   │   │  [Search memories... ]  │
│  └ Opus  │  │                         │   │                        │
│──────────│  └──────────────────────────┘   │────────────────────────│
│ Tools    │                                  │  📁 Files              │
│ 🟢 mcp-1 │  ┌──────────────────────────┐   │  ├─ src/               │
│ 🟢 mcp-2 │  │ Type a message...   ↵  │   │  ├─ tests/             │
│ 🟡 mcp-3 │  └──────────────────────────┘   │  └─ Cargo.toml        │
├──────────┴──────────────────────────────────┴────────────────────────┤
│ /help │ Permission: strict │ Session: abc123 │ Context: 42%          │
└─────────────────────────────────────────────────────────────────────┘
```

**Artifacts tab derivation:** there is no artifact-tracking API. The tab is derived client-side from `tool_call_start` events for write-shaped tools (`file_write`, `file_edit`, …) observed during the turn. It is therefore turn-scoped and lost on reload unless reconstructed from G8's tool-call history. Say so in the UI rather than implying a filesystem diff.

**Progress tab:** same source — the ordered `tool_call_start`/`tool_call_result` pairs for the current turn.

### Panel Responsiveness

| Width | Layout |
|-------|--------|
| >1280px | 3-panel: sidebar (240px) + chat (flex) + detail (320px) |
| 1024–1280px | 3-panel: sidebar (200px) + chat (flex) + detail (280px) |
| 768–1024px | 2-panel: sidebar (collapsible) + chat. Detail panel as overlay. |
| <768px | 1-column: chat only. Sidebar + detail as drawers/sheets. |

At every breakpoint the approval dialog is a focus-trapped modal — it must never be inside a collapsed panel, or a mobile user can strand a turn until it times out.

### Color Scheme

Dark mode default (matches terminal aesthetic). Light mode available; follow `prefers-color-scheme` on first load.

- Background: `zinc-950` / `white`
- Sidebar: `zinc-900` / `zinc-50`
- User bubble: `blue-600`
- Assistant bubble: `zinc-800` / `zinc-100`
- Tool call card: `zinc-800/50`, left border by status (blue=running, green=done, red=error, amber=awaiting approval)
- Approval dialog: amber accent, Approve (green) / Deny (red)

**Accessibility:** status is never conveyed by colour alone — every status dot and card border pairs with an icon or text label (colour-blind users, and the MCP amber/green distinction in particular). Target WCAG AA contrast in both themes.

---

## 7. Gateway Endpoint Reference

### New V2 Endpoints (Phase 1)

| Method | Path | Description | Lock | Auth |
|--------|------|-------------|------|------|
| POST | `/v2/chat/stream` | Rich SSE chat stream | read → (none) → write → read | Bearer (if enabled) |
| POST | `/v2/chat/approve` | Approve pending tool call | none (resolves oneshot) | Bearer |
| POST | `/v2/chat/deny` | Deny pending tool call | none | Bearer |
| POST | `/v2/chat/interrupt` | Interrupt active turn (G13) | read | Bearer |
| GET | `/v2/agents` | List available agents | read | Bearer |
| POST | `/v2/agents/switch` | Switch agent personality | **write** | Bearer |
| POST | `/v2/agents/default` | Reset to default agent | **write** | Bearer |
| GET | `/v2/providers` | List providers + models | read | Bearer |
| POST | `/v2/switch-model` | Switch model | **write** | Bearer |
| POST | `/v2/switch-provider` | Switch provider | **write** | Bearer |
| POST | `/v2/switch` | Switch provider + model | **write** | Bearer |
| GET | `/v2/mcp/servers` | MCP server status | read | Bearer |
| GET | `/v2/memory/search` | Search memory vault | read (+ blocking mutex) | Bearer |
| GET | `/v2/memory/stats` | Memory vault statistics | read | Bearer |
| GET | `/v2/files` | Read file from sandbox | read | Bearer |
| GET | `/v2/files/tree` | List sandbox directory | read | Bearer |
| GET | `/v2/sessions/{id}/messages` | Session messages incl. tool calls | read | Bearer |
| GET | `/v2/settings` | Get current settings | read | Bearer |
| POST | `/v2/settings/permission` | Change permission mode | **write** | Bearer |
| DELETE | `/v2/settings/approved-tools` | Clear sticky approvals | **write** | Bearer |

All **write**-lock endpoints return **409** while a turn is active.

### Existing V1 Endpoints (unchanged)

| Method | Path | Description |
|--------|------|-------------|
| POST | `/v1/chat/completions` | OpenAI-compatible chat (non-stream) |
| POST | `/v1/chat/completions/stream` | OpenAI-compatible chat (SSE, text only) |
| GET | `/v1/sessions` | List sessions |
| POST | `/v1/sessions` | Create session |
| GET | `/v1/sessions/{id}` | Get session details (text messages) |
| DELETE | `/v1/sessions/{id}` | Delete session |
| POST | `/v1/sessions/{id}/compact` | Compact session |
| GET | `/v1/models` | Current model |
| GET | `/v1/cost` | Cost tracking (accurate only after G10) |
| GET | `/health` | Health check (auth-exempt after G12) |

---

## 8. Error Model

v1 had none, so the frontend had nothing to render against. All `/v2` errors use one shape (extend the existing `ErrorResponse` in `types.rs`):

```json
{ "error": { "code": "turn_in_progress", "message": "A turn is already running.", "details": { "active_session_id": "abc123" } } }
```

| HTTP | `code` | Meaning | UI response |
|------|--------|---------|-------------|
| 400 | `invalid_request` | Bad params (empty `q`, bad permission mode) | Inline field error |
| 401 | `unauthorized` | Missing/invalid bearer | Prompt for key; stop polling |
| 403 | `path_forbidden` | Outside sandbox or gitignored | "Not accessible" — do not reveal whether it exists |
| 404 | `not_found` | Session/file missing | Empty state |
| 409 | `turn_in_progress` | Turn active; mutation refused | Disable controls, offer interrupt |
| 409 | `stale_approval` | `call_id`/`turn_id` no longer pending | Dismiss dialog silently |
| 429 | `rate_limited` | `RateLimiter` rejected | Back off, show retry-after |
| 500 | `internal` | Unexpected | Toast + surface `message` |
| 503 | `provider_unavailable` | Provider/API key failure | Point at model selector |

Mid-stream failures use `event: error` with the same `{code, message}` body, followed by `done`.

---

## 9. Security Considerations

Absent from v1 and non-optional, since this UI exposes shell execution and filesystem access over HTTP.

1. **Bind loopback only.** The gateway hard-codes `127.0.0.1` today (`src/gateway/mod.rs:92`); G11 keeps that as the default. Binding to `0.0.0.0` must require `auth.enabled = true` — refuse to start otherwise. An unauthenticated gateway on a LAN is remote code execution by design.
2. **CORS is permissive by default** (`cors_origins: ["*"]` → `CorsLayer::permissive()`, `mod.rs:111`). Combined with a browser UI, any page the user visits can drive the gateway. For Phase 1 dev this is tolerable on loopback; document that production/shared use must set explicit origins. Note that `permissive` + bearer auth is still a mitigation only if the token isn't stored where a hostile origin can read it — keep it in memory, not `localStorage`.
3. **Bearer token storage.** In Tauri, keep the token in the Rust side and inject per-request; in the browser, in-memory only, re-entered on reload.
4. **Approval is the security boundary.** Everything in [2.4](#24-approval-flow-corrected) matters: sticky-by-tool-name approval means one "Approve" on `shell_exec` disarms confirmation for the rest of the process. The dialog must say so, and G7 must let the user inspect and clear `approved_tools`.
5. **`yolo` mode** (`ToolConfirmationPolicy::Never`) executes everything unattended. Behind a confirm-once dialog in F17, and shown persistently in the status bar while active.
6. **File endpoints** are the classic traversal target — see G6's hardening list. Symlinks that resolve outside root, `.gitignore`d secrets, and 404-vs-403 information leaks are all in scope.
7. **Deep links** (`momo://open?path=…`) re-root the sandbox from untrusted input. Validate and confirm.
8. **Rate limiting exists** (`src/gateway/auth.rs`) but is keyed per API key; with auth disabled there is no limit. Fine on loopback, dangerous otherwise.

---

## 10. Testing Strategy

Also absent from v1. Each gateway task ships with the probe that proves it.

**Rust unit/integration.** Use `axum::Router` with `tower::ServiceExt::oneshot` against an in-memory harness (`SessionManager::new_in_memory`, `src/session.rs:45`; `McpService::new_empty`, `src/mcp/mod.rs:193`) so tests need no provider credentials. Cover:
- turn guard: second concurrent stream → 409;
- approval registry: stale `call_id` → 409; timeout path fires the deny turn;
- sandbox: `..`, absolute paths, symlink escape, gitignored file → 403;
- permission mode parsing round-trip;
- error-shape conformance for every `/v2` route.

**Lock-discipline test.** A regression test that starts a turn which requires approval, then issues `GET /v2/agents` while the approval is pending, and asserts it returns promptly. This is the deadlock canary for [2.2](#22-lock-discipline-mandatory) — it would fail against v1's design.

**Cost parity test (G10).** Same prompt through REPL and gateway → identical `CostSummary` deltas.

**Manual probes.**

```bash
cargo run -- --gateway --gateway-port 0        # prints MOMO_GATEWAY_LISTENING <url>

curl -N -X POST $URL/v2/chat/stream \
  -H 'content-type: application/json' \
  -d '{"messages":[{"role":"user","content":"list files in src"}]}'

curl -X POST $URL/v2/chat/approve \
  -H 'content-type: application/json' \
  -d '{"turn_id":"t_01","call_id":"call_002","tool_name":"shell_exec","approved":true}'

curl "$URL/v2/files?path=../../etc/passwd"     # expect 403 path_forbidden
curl "$URL/v2/memory/search?q=momo&limit=5"
curl $URL/health                               # expect 200 without a bearer (G12)
```

**Frontend.** Vitest for `sse-parser` (split frames, `\r\n`, multi-line `data:`, comments, unknown events) — that parser is hand-rolled and is the single most likely source of silent breakage. Playwright for the approval flow against a mock SSE server.

---

## 11. Open Questions

Resolved or narrowed by the code audit:

1. **Multi-session concurrency** — *Resolved: not possible as designed.* `Harness` has one `current_session_id` and one runner; concurrency needs per-session harness instances, a much larger refactor. Phase 1 is single-turn, single-session, enforced by the `TurnGuard`.
2. **File upload in chat** — *Proposal stands:* text files only, read client-side, sent as context (F25). Multimodal needs provider-level support that isn't wired.
3. **Web UI deployment** — *Proposal stands:* G9 (`ServeDir`) for Tauri and simple deployment; `next dev` for development. Note G9 requires an auth exemption.
4. **Real-time session sync between tabs** — *Revised:* v1's "each tab is independent" is unsafe (see C4). Tabs share one global harness. Phase 1: the 409 guard prevents concurrent turns; the UI displays the gateway's actual `current_session_id`. Proper multi-tab support is out of scope.
5. **Memory vault write from UI** — Search/read only in Phase 1.
6. **MCP server management from UI** — View status only in Phase 1, though `add_server`/`remove_server`/`start_server`/`stop_server` all exist (`src/mcp/mod.rs:217-247`) and would be cheap to expose later.
7. **Theming** — Dark + light, system preference auto-detect. No custom accent colours in Phase 1.
8. **Tauri vs Electron** — *Confirmed:* Tauri v2 (smaller binary, Rust-native, better security posture).

Still genuinely open:

9. **Sticky approval semantics.** — *Decided (v3): ship the honest "approve for the rest of this session" dialog in Phase 1.* Per-call approval is deferred as a follow-up; it is a harness change (`RunConfig` handling, `approved_tools` keyed by call), not a UI change, and gating F9 on it would block the whole of WP-4. Binding requirement on F9 in [§12.4](#124-work-packages).
10. **Denial is not remembered.** `run_confirmation_turn(name, false)` records nothing, so the model can re-request the same tool immediately and the user gets prompted repeatedly. Add a `denied_tools` set, or leave it? Recommendation: add it alongside per-call approval in the same follow-up.
11. **Per-server MCP tool counts.** Report `null`, or add attribution in `src/mcp/`? Phase 1 reports `null` (G4).
12. **Turn history after reconnect.** With `fetch`-based streaming there is no automatic resume. If the connection drops mid-turn, the turn keeps running server-side but the UI loses the tail. Options: buffer the last N events per turn and add `GET /v2/chat/replay?turn_id=`, or accept the loss and refetch via G8 when the turn ends. Recommendation: accept for Phase 1, revisit if it bites.

---

## 12. Execution Plan & Model Assignment

This section is the dispatch document. §1–§11 say *what* to build; this says *who builds what, in what order, and what proves it done*.

### 12.1 Status Ledger

Single source of truth for task state. Update this table, not the individual task sections.

**As of `a218c19` · 2026-08-05 · pushed to `origin/dev` · `cargo test` 297 passed**

| | Done | Open | Progress |
|---|---|---|---|
| **Gateway** (G1–G13 + R1) | 12 | 5 | ██████████████░░░░ 71% |
| **Wave-0 bugs** (B0–B4) | 5 | 0 | ██████████████████ 100% |
| **Frontend** (F1–F29) | 0 | 29 | ░░░░░░░░░░░░░░░░░░ 0% |
| **Desktop** (T1–T13) | 0 | 13 | ░░░░░░░░░░░░░░░░░░ 0% |

**Phase 1 is roughly one-third done.** All of it is backend: the gateway's hard parts — streaming, approvals, concurrency, cost — are built and now wire-verified. Not one line of UI exists yet, and the frontend is the larger half of the remaining work.

#### ✅ Done

| Task | Landed in | Notes |
|---|---|---|
| G1 rich SSE stream | `546ef04` | 4-phase approval stitch — **wire-verified 2026-08-05** |
| G2 agents | `546ef04` | |
| G3 providers/models | `546ef04` | incl. `list_all()` so unconfigured providers report `available:false` |
| G7 settings | `546ef04` | incl. `DELETE /v2/settings/approved-tools` |
| G10 cost/context | `546ef04` | shared lifecycle, `src/harness.rs:440-456` |
| G11 port/bind flags | `546ef04` | `--gateway-port 0` + `MOMO_GATEWAY_LISTENING` |
| G12 health exemption | `546ef04` | auth-exempt `/health` with readiness payload |
| G13 interrupt | `546ef04` | |
| — turn guard | `546ef04` | `src/gateway/turn.rs`, 7 tests |
| **G0** wire verification | `57fb7a4` | approval round-trip · tool mapping · stickiness · **deadlock canary 200 in 0.4 ms**. Ran on `nvidia/nemotron-3-ultra-550b-a55b:free` |
| **R1** route pre-registration | `57fb7a4` | 6 routes → `501 not_implemented` in the §8 shape. **This is what unblocks parallel dispatch** |
| **B0** broken default model | `57fb7a4` | `.harness/settings.json` pointed at a slug that does not exist |
| **B1** ollama false positive | `57fb7a4` | `ollama_reachable()` TCP probe, 150 ms, honours `OLLAMA_HOST` |
| **B2** `:free` billed as paid | `57fb7a4` | `get_pricing` short-circuits the `:free` suffix. `/v1/cost` → `0.0` over 30k tokens (was `$0.596`) |
| **B3** sandbox "not initialized" | `57fb7a4` | thread-affinity race — see note below |
| **B4** keyring never registered | `57fb7a4` | missing store now degrades to `NotFound`, so the API says *"Set ZAI_API_KEY"* |
| — MCP tool-name prefixing | `9d71ddc` | pre-existing work; gives HTTP servers per-server attribution (affects G4) |

#### ⬜ Open

| Task | Owner model | Package | Notes |
|---|---|---|---|
| **G4** MCP status | Sonnet 5 | **WP-1** | `tool_count` nullable — see the §4.7 revision in the scratchpad |
| **G5** memory search | Sonnet 5 | **WP-1** | `std::sync::Mutex` — no `await` while held |
| **G8** session messages | Sonnet 5 | **WP-1** | extend the existing walk, pair by call id |
| **G6** file read/tree | **Opus 5** | **WP-2** | security boundary; `security-review` is a blocking gate |
| **G9** static serving | Haiku 4.5 | **WP-6** | needs `web/out/` to exist first |
| **F1–F3** scaffold, client, SSE parser | Sonnet 5 (+Haiku for types) | **WP-3** | codes to §5/§7, not to what's implemented |
| **F4–F11, F29** core chat | Sonnet 5 | **WP-4** | F9 approval dialog is the risky one |
| **F12–F20** management panels | Sonnet 5 | **WP-5** | |
| **F21–F28** polish | Haiku 4.5 | **WP-6** | |
| **T1–T5** Tauri core | **Opus 5** | **WP-7** | process supervision, cross-platform kill |
| **T6–T9, T12** shell features | Sonnet 5 | **WP-8** | |
| **T10, T11, T13** ship pipeline | Sonnet 5 + human | **WP-9** | T13 signing needs human credentials |
| **Cost parity** (G10 acceptance) | Sonnet 5 | **WP-1** | the one Wave-0 check still open — was gated on B2, now unblocked. Needs a paid model to be meaningful |

> **B3 was misdiagnosed as an approval-path bug. It was a latent thread-affinity race affecting every sandboxed tool call.**
>
> `file`, `shell`, `search`, `kms` and `memory` each held their sandbox/vault in a **`thread_local!`**. `build_tool_registry` sets it on whichever thread builds the registry, but tools *execute* later on an arbitrary tokio worker — so a tool call succeeded or failed with `"… not initialized"` depending on which worker picked up the task. It surfaced during the approval test only because `rebuild_runner` reshuffled the timing; it was never approval-specific, and `file_read`/`grep`/`mem_*` were equally exposed.
>
> Fixed by moving all five contexts to a process-global `RwLock<Option<Arc<…>>>`, which is correct here because the harness has exactly one sandbox and one vault by construction ([§2.3](#23-concurrency-model-corrected)). `get_*` clones the `Arc` out, so no guard is ever held across an `await`.
>
> The tool tests had been relying on thread-locals for isolation and began clobbering one another once the context went global; they are now serialised through `tools::test_support::sandbox_guard()`. Suite green at **297 passed**.

### 12.2 Model Routing Rubric

Route on **cost of being wrong**, not on task size. A 30-line handler that decides whether a path escapes the sandbox is Opus work; a 400-line React panel that renders a list is not.

| Model | Give it | Why | In this plan |
|---|---|---|---|
| **Opus 5** | Security boundaries, concurrency/lock ordering, process supervision, cross-cutting refactors, final review of the above | Failure modes are silent and expensive: a traversal bug ships an exfiltration primitive; a lock-order bug deadlocks the gateway under a rare interleaving that tests miss | WP-2 (G6), WP-7 (T1–T5), review gates on WP-1/WP-4 |
| **Sonnet 5** | Implementation against a settled contract — endpoints whose payload §7 already fixes, React components whose behaviour §6 already fixes, test suites | The spec removes the design risk; what remains is throughput, and Sonnet has the context budget to hold a module plus its tests | WP-0, WP-1, WP-3, WP-4, WP-5, WP-8, WP-9 |
| **Haiku 4.5** | Mechanical transforms with a checkable output: route stubs, TS types mirrored from Rust structs, empty/loading states, icon and copy passes, doc table updates | No judgment required, and every result is diff-checkable against a source of truth | R1, types in WP-3, WP-6 |

**Escalation rule.** Any agent that hits one of these stops and hands back up rather than guessing:
- a lock is needed across an `await`, or the phase ordering in [§2.4](#24-approval-flow-corrected) doesn't fit → **Opus**
- a `/v2` payload must differ from [§7](#7-gateway-endpoint-reference) → **spec change first**, not a local deviation
- a sandbox check can't be expressed with `resolve_path` + `check_readable` + `is_ignored` → **Opus**

**Skills to attach**, per the scratchpad's §8:
- `security-review` — **mandatory gate** on WP-2 before merge.
- `code-review` — gate on WP-1 and WP-7.
- `frontend-design` — opening move on WP-3/WP-4 for the 3-panel visual direction.
- `example-skills:webapp-testing` — Playwright on the approval dialog, WP-4 exit.
- `commit` — conventional commits at each package boundary.
- **Not** `prd-generator` — the spec exists and is reconciled.

### 12.3 Contention Map — why naive parallelism breaks

Two files are written by nearly every open package. Dispatching the packages in parallel without handling these produces merge conflicts on every landing:

| Hot file | Contended by | Mitigation |
|---|---|---|
| `src/gateway/mod.rs` (routes + `GatewayState`) | G4, G5, G6, G8, G9 | **R1 lands first**: register all five routes pointing at stub handlers that return `501 not_implemented` in the [§8](#8-error-model) error shape. Feature packages then only touch their own handler file. |
| `web/src/lib/types.ts` | F2 and every F-task after it | **F2 lands the complete type surface up front** — it is fully derivable from [§5](#5-sse-event-protocol) and [§7](#7-gateway-endpoint-reference) without any endpoint existing. Later packages import, never extend. |

This is what makes the frontend safe to start before the gateway is finished — a reversal of the scratchpad's "finish G4–G8 first" advice. That advice was sound when the API surface was still moving; §5 and §7 have since frozen it. **The spec is the contract, and the frontend codes against the spec.** Stub routes returning `501` let WP-3/WP-4 build and run end-to-end against real HTTP.

### 12.4 Work Packages

Each package is one agent's assignment: self-contained, one branch, one review, one merge.

---

**WP-0 · Unblock & pre-register** — Sonnet 5 (G0) + Haiku 4.5 (R1) · *serial, blocks everything*

✅ **The environment blocker is gone — no human action, no credits needed.** The working reference config is `openrouter` + **`nvidia/nemotron-3-ultra-550b-a55b:free`** (free tier, tool-capable, 1M context). The prior blocker was a bogus default model slug, not an account problem.

- **G0 — done.** Verified live 2026-08-05: tool-call part mapping, the **full approval round-trip**, sticky-approval behaviour, and the [§2.2](#22-lock-discipline-mandatory) deadlock canary (`GET /v2/agents` returned **200 in 0.4 ms** while an approval was parked). Transcript in scratchpad §5. Cost parity is the one check still open, and it is gated on **B2**, not on the environment.
- **R1 — now the actual Wave-0 gate.** Add `/v2/mcp/servers`, `/v2/memory/search`, `/v2/memory/stats`, `/v2/files`, `/v2/files/tree`, `/v2/sessions/{id}/messages` as `501 not_implemented` stubs in the §8 error shape.
- **Bug fixes batched here**, all found during verification and all cheap:
  - **B0** — `.harness/settings.json` still defaults to the broken `google/gemma-4-26b-a4b-it:free` for both `default_model` and `memory.sidecar_model`. Point them at the working model. *(Left unmodified deliberately; it is the user's config.)*
  - **B1** — `src/providers.rs:343` hard-codes `available:true` for `ollama` with no liveness probe, so `/v2/providers` reports it selectable when nothing listens on `:11434`. Misleads F13. Probe it, or split *configured* from *reachable*.
  - **B2** — `:free` models are billed as paid (`GET /v1/cost` reached `$0.596` on a free model). **Fix before claiming G10 acceptance**, and before F18 shows a user a number.
  - **B3** — Opus: the first tool call after an approval turn returns `tool.internal: shell tool sandbox not initialized`; `rebuild_runner()` appears not to re-establish the shell tool's global sandbox. In the G1 approval path, so it surfaces as a spurious error card.
  - **B4** — `keyring-core` never has `set_default_store` called, so the entire OS-keychain path fails; `.env` is the only working secret source. Either register a store or drop the keychain claim from the docs.
- **Exit:** cost parity passes once B2 lands; `curl $URL/v2/files` returns a well-formed 501; B0/B1/B3/B4 fixed or explicitly deferred.

---

**WP-1 · Gateway read endpoints** — Sonnet 5 · *parallel after R1* · gate: `code-review`

G4 (MCP status) · G5 (memory search + stats) · G8 (session messages with tool calls).

Three independent handler files, one branch. Constraints that are not negotiable:
- G4 reports `tool_count: null` per server plus a global total — there is no per-server attribution (`McpService::toolset()` merges). `all_statuses()` is **async**.
- G5 uses `ObsidianVault::search(&MemoryQuery)`, **not** `MemorySidecar::search_for_context`. The vault is behind a `std::sync::Mutex` — collect owned results, drop the guard, *then* serialise. Never `await` while holding it.
- G8 walks the same `session.events().all()` as `/v1/sessions/{id}` and additionally maps `Part::FunctionCall`/`Part::FunctionResponse`, paired by call id.

**Exit:** each task's §3.1 acceptance criterion passes via its §10 curl probe; the [§2.2](#22-lock-discipline-mandatory) deadlock canary test still passes.

---

**WP-2 · Sandboxed file access** — **Opus 5** · *parallel after R1* · gate: **`security-review` (blocking)**

G6 only. Small in lines, largest in blast radius: this endpoint turns the gateway into a file server, and every classic mistake is reachable from a browser.

Ship with the full G6 hardening list — absolute-path and `..` rejection, post-canonicalisation prefix re-check, symlink escape after resolution, `is_ignored` filtering so gitignored secrets don't leak, 1 MB cap with `truncated:true`, NUL-sniff binary detection, and **403 for everything out of sandbox, never 404** so the endpoint can't map the filesystem.

**Exit:** `path=../../etc/passwd` → 403; a gitignored file → 403; a symlink to `/etc` → 403; a 5 MB log → truncated with no OOM; `security-review` returns no unresolved findings.

---

**WP-3 · Frontend foundation** — Sonnet 5, types by Haiku 4.5 · *parallel after R1* · gate: Vitest

F1 (scaffold) · F2 (API client + types) · F3 (SSE parser) · F5 (app shell).

- F1: `create-next-app` into `web/`, Next 15 / React 19 / Tailwind v4 / shadcn/ui / zustand, `output:'export'`, `images.unoptimized`.
- F2: URL resolution chain `window.__GATEWAY_URL__ → NEXT_PUBLIC_GATEWAY_URL → http://localhost:3000`; uniform §8 error parsing; **the complete `types.ts` surface** per §12.3.
- F3: `fetch` + `ReadableStream` + hand-rolled frame parser. **Not `EventSource`** (C3).
- F5: 3-panel shell per §6 — open with the `frontend-design` skill.

**The parser is the single highest-risk artifact in the frontend.** It is hand-rolled, and its failure mode is silent event loss. Vitest coverage is a merge requirement, not a nice-to-have: frames split mid-chunk, `\r\n` and `\n`, multi-line `data:`, `:` keep-alive comments, and unknown `event:` names (logged and surfaced, never silently dropped).

**Exit:** `npm run build` produces `web/out/`; parser suite green; the shell renders against a live gateway with real `role`/`text`/`done` events.

---

**WP-4 · Core chat** — Sonnet 5 · *after WP-3* · gate: `code-review` + Playwright

F4 (chat store) · F6 (chat panel) · F7 (markdown + Shiki) · F8 (tool-call cards) · F9 (approval dialog) · F10/F11 (sessions) · F29 (409 handling).

**F9 carries a security requirement, not just a UX one.** Approval is sticky by tool *name* for the life of the process (C2) — approving `shell_exec` once means it is never asked again. The dialog must say so in plain language. **Q9 is hereby decided: ship the honest "Approve `shell_exec` for the rest of this session" copy now**; per-call approval is a harness change and is deferred (see [§11 Q9/Q10](#11-open-questions)). Also render `category` when `destructive`, and count down to `expires_at`.

F29 must handle 409 `turn_in_progress` by disabling send and offering interrupt — never by auto-retrying. Re-sending a turn double-bills and can re-run tools.

**Exit:** Playwright drives approve *and* deny against a mock SSE server; an interrupted turn leaves the UI able to start a fresh one.

---

**WP-5 · Management panels** — Sonnet 5 · *after WP-3; needs WP-1 + WP-2 merged*

F12 (agents) · F13 (models) · F14 (MCP status) · F15 (memory) · F16 (files) · F17 (settings) · F18 (cost) · F19 (connection) · F20 (shortcuts).

Two renderings are contractual, not cosmetic: F14 shows `tool_count: null` as **"—", never "0"**; F13 greys out `available:false` providers **and says why**. F17 must expose the `approved_tools` list with a clear button — that is the user's only visibility into what they've granted.

---

**WP-6 · Polish & serve** — Haiku 4.5 · *after WP-5*

F21–F28 (errors, loading, responsive, code blocks, file attach, empty states, sounds, local storage) · G9 (`ServeDir` on `/ui/*`, auth-exempt, SPA fallback → `web/out/`).

Per §6, the approval dialog stays a focus-trapped modal at every breakpoint — never inside a collapsible panel, or a mobile user strands a turn until it times out.

---

**WP-7 · Tauri core** — **Opus 5** · *after Phase 1*

T1 (scaffold) · T2 (gateway supervisor) · T3 (URL injection) · T4 (open project) · T5 (single instance).

Opus because T2 is process supervision with genuinely platform-divergent failure modes: **Windows has no SIGTERM** — a job object or `taskkill`, or the gateway leaks on every quit. T5 is a correctness requirement, not a convenience: two instances over one `.harness/` fight over global harness state and the session DB.

Spawn `--gateway-port 0`, parse `MOMO_GATEWAY_LISTENING` from stdout (do not pre-pick a port — it races), poll `/health` with backoff to a 30 s ceiling, inject `window.__GATEWAY_URL__` via init script (build-time env vars cannot work under static export).

---

**WP-8 · Desktop shell features** — Sonnet 5 · *after WP-7*

T6 (tray) · T7 (menus) · T8 (window state) · T9 (deep links) · T12 (icons/branding).

T9 handles untrusted input: `momo://open?path=…` re-roots the sandbox. Validate and require explicit user confirmation of the target directory.

---

**WP-9 · Ship pipeline** — Sonnet 5 + human · *after WP-8*

T10 (CI build, three targets, `momo-fetch` as a per-triple sidecar) · T13 (Apple notarization, Windows signing, updater key) · T11 (updater, blocked on T13).

**Human action required:** T13 needs developer credentials and cannot be delegated to any model.

### 12.5 Wave Schedule

Estimates are engineering-days per package assuming one agent per lane.

```
Wave 0  ── ✅ COMPLETE 2026-08-05 ──────────────────────────────────────────
  WP-0  G0 wire verification             ✅ approval round-trip + canary pass
        R1 route stubs                   ✅ 6 × 501 registered
        B0-B4 bug batch                  ✅ all fixed & verified live
                                         ⇒ Waves 1-3 can start in parallel now

Wave 1  ── 3 lanes in parallel ─────────────────────────────────────── ~4d
  WP-1  G4 + G5 + G8          (Sonnet)  ──> code-review
  WP-2  G6                    (Opus)    ──> security-review  [BLOCKING GATE]
  WP-3  F1 F2 F3 F5           (Sonnet + Haiku)  ──> vitest

Wave 2  ── 1 lane, depends on WP-3 ─────────────────────────────────── ~5d
  WP-4  F4 F6-F11 F29         (Sonnet)  ──> code-review + playwright

Wave 3  ── 1 lane, needs WP-1 + WP-2 merged ────────────────────────── ~4d
  WP-5  F12-F20               (Sonnet)

Wave 4  ── polish ──────────────────────────────────────────────────── ~3d
  WP-6  F21-F28 + G9          (Haiku)
                                        ══ Phase 1 complete ══

Wave 5  ── Phase 2 ─────────────────────────────────────────────────── ~4d
  WP-7  T1-T5                 (Opus)

Wave 6  ── 2 lanes ─────────────────────────────────────────────────── ~4d
  WP-8  T6-T9 T12             (Sonnet)
  WP-9  T10 T13 T11           (Sonnet + HUMAN for signing credentials)
```

Critical path: **WP-0 → WP-3 → WP-4 → WP-5 → WP-6 → WP-7**. WP-1 and WP-2 are off the critical path and can absorb schedule slip; WP-3 cannot — everything downstream of it stalls.

**Only one item still needs a human, and it is not urgent.**

| | Blocks | When it bites | Status |
|---|---|---|---|
| ~~Provider credits~~ | ~~Everything~~ | — | ✅ **Resolved 2026-08-05.** Not a credit problem — a bad default model slug. `nvidia/nemotron-3-ultra-550b-a55b:free` works on the free tier, tool calling included. No spend required. |
| **Code-signing credentials** (WP-9) | T13 → T11 (auto-updater) only | **~Week 5** | Needs an Apple Developer account and a Windows cert — procurement, not engineering. |

Code signing is a **lead-time item, not a blocker**. Unsigned builds still produce working `.dmg`/`.msi`/`.deb` artifacts from T10; only the auto-updater (T11) truly requires T13. Start the certificate paperwork early because issuance takes days, but do **not** treat it as blocking Phase 1, WP-7, or WP-8.

### 12.6 Handoff Contract

Every package hands off with the same five things. A package that skips these forces the next agent to re-derive context — which is the failure mode this document exists to prevent.

1. **Ledger updated** — [§12.1](#121-status-ledger) state flipped, nowhere else.
2. **Acceptance evidence** — the actual §10 probe output, not a claim that it passed.
3. **Deviations from spec** — anything built differently than §3/§5/§7 says, with the reason. If the spec was wrong, **fix the spec in the same PR**; a spec that drifts from the code is worse than no spec, because it is trusted.
4. **New hard-won knowledge** — anything that cost real investigation goes into scratchpad §4. That section is the highest-value artifact in this project.
5. **Conventional commit** at the package boundary (`commit` skill).

### 12.7 Quality Gates

| Gate | Applies to | Blocking? | What it catches |
|---|---|---|---|
| `cargo check --all-targets` clean | every Rust package | yes | — |
| Deadlock canary ([§10](#10-testing-strategy)) | WP-1, WP-2 | yes | a `/v2` GET taken while an approval is parked must return promptly — the [§2.2](#22-lock-discipline-mandatory) regression test |
| `security-review` | **WP-2** | **yes** | traversal, symlink escape, gitignored-secret leak, 404-vs-403 disclosure |
| `code-review` | WP-1, WP-4, WP-7 | yes | lock ordering, process supervision, approval-state handling |
| Vitest on `sse-parser` | WP-3 | yes | silent event loss on split frames — the frontend's top risk |
| Playwright approval flow | WP-4 | yes | the approve/deny round-trip, incl. sticky-scope disclosure |
| Cost parity (REPL vs gateway) | WP-0 | yes | G10's acceptance criterion, still unproven on the wire |

---

## Appendix: Task Dependency Graph

```
Phase 1 Sprint 1:

  G10 (cost/context) ──> G1 (rich SSE) ──┬──> G13 (interrupt)
                                         │
  F1 (scaffold) ──┬──> F2 (api client) ──┼──> F4 (chat store) ──> F6 (chat panel) ──> F7 (markdown)
                  │                      │                        │
                  ├──> F3 (sse client) ──┘                        ├──> F8 (tool cards) ──> F9 (approval)
                  │                                               │
                  └──> F5 (app shell) ───────────────────────────┘
                                                                  │
  G1 ────────────────────────────────────────> F29 (409 handling)─┘
  G12 (health) ──────────────────────────────> F19 (connection status)
  G2 (agents) ───────────────────────────────> F12 (agent picker)
  G3 (models) ───────────────────────────────> F13 (model selector)
  F2 ────────────────────────────────────────> F10 (session list) ──> F11 (persist)
  G8 (session msgs) ─────────────────────────> F11

Phase 1 Sprint 2:
  G4 (mcp status) ──> F14 (tool status)
  G5 (memory) ─────> F15 (memory tab)
  G6 (files) ──────> F16 (files tab) ──> F25 (file attach)
  G7 (settings) ───> F17 (settings panel)
  G10 ─────────────> F18 (cost display)
  G13 ─────────────> F20 (shortcuts: Esc = interrupt)

Phase 1 Sprint 3:
  All ──────────────> F21-F28 (polish)
  G9 (static) ──────> (ready for Phase 2)

Phase 2:
  Phase 1 + G11 (--gateway-port) + G12 (health) ──> T1 ──> T2 (supervisor) ──> T3 (url inject)
                                                     │
                                                     ├──> T4 (open project) ──> T9 (deep links)
                                                     ├──> T5 (single instance)
                                                     ├──> T6 (tray)
                                                     ├──> T7 (menus)
                                                     ├──> T8 (window state)
                                                     └──> T10 (build) ──> T13 (signing) ──> T11 (updater)
                                                          T12 (icons)
```

### Package-level view (dispatch order — see [§12.5](#125-wave-schedule))

The task graph above is the *logical* dependency order. This is the order work is actually handed out; `R1` (route stubs) and a complete `types.ts` are what cut the two edges that would otherwise force gateway and frontend to run in series.

```
        ┌─ R1 stubs (Haiku) ─┬──> WP-1  G4 G5 G8      (Sonnet) ─┐
WP-0 ───┤                    ├──> WP-2  G6            (Opus)   ─┤
        └─ G0 verify (Sonnet)└──> WP-3  F1 F2 F3 F5   (Sonnet) ─┼──> WP-5  F12-F20 (Sonnet)
           ⚠ human: credits            │                        │         │
                                       └──> WP-4  F4 F6-F11 F29 ┘         │
                                                  (Sonnet)                │
                                                                          ▼
                                                            WP-6  F21-F28 + G9 (Haiku)
                                                                          │
                                                                          ▼
                                                            WP-7  T1-T5   (Opus)
                                                                          │
                                                              ┌───────────┴───────────┐
                                                              ▼                       ▼
                                                WP-8  T6-T9 T12 (Sonnet)   WP-9  T10 T13 T11
                                                                             (Sonnet + ⚠ human: signing)
```
