# Gateway API Guide — Frontend Perspective

How the frontend talks to the gateway: HTTP requests, SSE streams, error handling, and wire types.

## Gateway URL Resolution

The frontend resolves the gateway URL in this order (from `web/src/lib/api-client.ts`):

1. **`window.__GATEWAY_URL__`** — Injected by Tauri at runtime (when the shell starts the gateway and picks an ephemeral port)
2. **Same origin** — If the page is served at `/ui`, use the current origin (the gateway is serving this bundle)
3. **`NEXT_PUBLIC_GATEWAY_URL`** — Build-time environment variable for dev against a separate gateway
4. **Default** — `http://localhost:3000`

All trailing slashes are stripped from resolved URLs.

### Why same-origin matters

When the gateway serves the UI at `/ui`, that same origin is the correct gateway to talk to (it is by definition the gateway that is serving this bundle). This path is not in the spec; it was added after testing showed that a build-time `NEXT_PUBLIC_GATEWAY_URL` is wrong when the gateway picks an ephemeral port.

## Authentication

The bearer token is held **in memory only**, never in `localStorage`.

**Why?** A token in `localStorage` is readable by any script on the origin, and
this token drives an API that can run shell commands (spec §9.3).

Note that the gateway's CORS default is the *opposite* of permissive: `cors_origins`
defaults to `[]`, and the gateway refuses to start with `["*"]` unless auth is
enabled — that combination would let any website you visit read your working tree
and run commands. Same-origin under `/ui` needs no grant; the desktop shell grants
its own webview origin explicitly with `--gateway-allow-origin`.

Set the token via `setAuthToken(token)` from `api-client.ts`. Every request includes it in the `Authorization: Bearer {token}` header.

## Streams: POST `/v2/chat/stream`

The main request-response cycle.

### Request Body

```typescript
interface V2ChatRequest {
  session_id?: string;      // Omit to create a new session
  messages: ChatMessage[];  // Array of role/content pairs
  agent?: string;           // Optional: named agent to use
  model?: string;           // Optional: override the current model
}

interface ChatMessage {
  role: "system" | "user" | "assistant";
  content: string;
}
```

### Response

A streaming response of SSE events. The frontend reads it with `fetch` + `ReadableStream` + hand-rolled parser (see below).

**Why not `EventSource`?** It is GET-only and cannot set headers. This request needs:
- POST method (stateful, carries a JSON body)
- JSON body (the message history and config)
- Authorization header (bearer token)

So the frontend uses `fetch()` with `signal` for abortion, and a hand-rolled frame parser that:
- Handles frames split across chunk boundaries
- Handles UTF-8 characters straddling boundaries
- Tolerates both `\r\n` and `\n` line endings
- Surfices unknown event types rather than silently dropping them

### Errors

A 409 (turn in progress) arrives as JSON, not as an SSE stream:

```json
{
  "error": {
    "code": "turn_in_progress",
    "message": "Another turn is running; mutations are refused until it ends"
  }
}
```

The `ApiError` class (from `api-client.ts`) carries the typed `code` so the UI can branch on `turn_in_progress`, `stale_approval`, etc.

## SSE Stream Events

The `/v2/chat/stream` response is a sequence of typed SSE frames. Each frame is a line-delimited event with fields `event:`, `data:`, and optional `id:`.

Frame format:
```
event: role
data: {"role":"assistant","model":"claude-3-5-sonnet","provider":"anthropic","session_id":"...","agent":null,"turn_id":"..."}

event: text
data: Hello

event: text
data:  there

event: artifacts
data: {"files":[{"path":"hello.txt","change":"created"}]}

event: done
data: {"turn_id":"...","stop_reason":"complete"}
```

The parser joins multi-line data with newlines (per the SSE spec) and yields typed `StreamEvent` objects.

### Event Types

| Event | Payload | Fired |
|-------|---------|-------|
| **role** | session_id, model, provider, agent, turn_id | Once per turn, first |
| **text** | content (string chunk) | Streaming, as tokens arrive |
| **tool_call_start** | id, name, args | Once per tool call, before execution |
| **tool_call_progress** | id, name, detail?, elapsed_secs | Every 15s while a call is still running. `detail` is present for `task` — the sub-agent's subtask, last tool and tool-call count |
| **tool_call_result** | id, name, status ("done" or "error"), output_preview, truncated | Once per tool call, after result |
| **approval_required** | turn_id, call_id, name, args, destructive, category, sticky, expires_at | When a tool needs approval |
| **approval_resolved** | call_id, approved, reason ("user", "timeout", "disconnect") | After approval decision |
| **usage** | prompt_tokens, completion_tokens, total_tokens?, cost_usd | Once per leg, cumulative cost |
| **context_usage** | used, total, percent | During long turns, as context fills |
| **artifacts** | files: [{ path, change: "created"/"modified"/"deleted" }] | At most once, after the last leg, before `done`. Omitted when nothing changed |
| **error** | code, message, details? | On error, may end turn |
| **done** | turn_id, stop_reason ("complete", "error", "interrupted") | At turn end, always last |
| **unknown** | name (event type), raw (data string) | Forward compat: unknown types are surfaced |

**Silence policy.** A turn is abandoned with an `error` event after 240s of provider silence — but that budget is only spent while **no tool call is in flight**. A tool is a black box to the model's stream (`task(...)` runs a whole sub-agent inside one call and emits nothing until it returns), so a running call is reported with `tool_call_progress` and waited on indefinitely. The tool's own timeout bounds its work; `/v2/chat/interrupt` is the way out.

Types are defined in `web/src/lib/types.ts` and must mirror `src/gateway/v2_types.rs` in the Rust harness. Type drift (new events added to gateway but not frontend) fails silently unless the frontend surfaces unknown events — which it does.

### Approval Lifecycle

1. Agent calls a tool that needs approval → `approval_required` event arrives with `call_id`
2. UI shows modal, user approves or denies
3. Frontend posts to `/v2/chat/approve` or `/v2/chat/deny` with `turn_id`, `call_id`, `tool_name`
4. Gateway responds with `approval_resolved` event
5. **Important:** The re-executed call gets a *different* `id` (new turn leg), so the old `call_id` is not stable across approval

The chat store handles this by dropping the awaiting card on resolve (not surfacing the before/after mismatch).

## Management Endpoints

All return JSON.

### Agents

```
GET  /v2/agents                        # List agents, current
POST /v2/agents/switch   body:{name}   # Switch to an agent
POST /v2/agents/default                # Reset to default
```

### Providers & Models

```
GET  /v2/providers                     # List providers
GET  /v2/providers/{provider}/models   # Get model catalogue (may be unavailable)
DELETE /v2/providers/{provider}/models # Clear cached catalogue
POST /v2/switch-model     body:{model} # Switch model
POST /v2/switch-provider  body:{provider}  # Switch provider
```

### Settings

```
GET  /v2/settings                      # Permission mode, approved_tools, paths
POST /v2/settings/permission  body:{mode}  # Set "strict", "auto", or "yolo"
DELETE /v2/settings/approved-tools     # Revoke all sticky approvals
```

### Background activity

```
GET  /v2/activity      # Team workers + routine runs in flight
```

```json
{
  "team": { "team_id": "...", "workers": [ … ] } | null,
  "team_configs": ["demo", "solo"],
  "runs": [ … ],
  "counts": { "workers_running": 0, "runs_active": 1, "routines_armed": 3 }
}
```

`team_configs` is what is defined in `.harness/teams/`, so an idle rail can say
*what could run* rather than only that nothing is — and, since the write side
below exists, run it.

### Team

```
POST /v2/team/start    body:{name}      # Start a config from .harness/teams/
POST /v2/team/stop     body:{force?}    # DESTRUCTIVE — see below
POST /v2/team/restart  body:{worker}    # Relaunch one worker's pane
```

Each runs the same action `momo-fetch team …` runs, so the CLI, the agent
driving it through `shell_exec`, and the UI cannot end up with different ideas
of what starting a team means. Exit codes become statuses: a state conflict
(`team_already_active`) is **409**, a missing config **404**, anything else
**400** carrying the CLI's own error code. Success returns that action's JSON
plus `notes` — the narration the CLI writes to stderr, such as *"tmux not found
— the team will be recorded but no worker process starts"*. Do not drop it: a
team with no processes behind it otherwise looks exactly like a team.

`stop` removes every worktree with `git worktree remove --force` and deletes the
worker branches. It is idempotent (stopping nothing is a 200), and the UI
confirms before calling it.

What the agent is doing *outside* this turn: workers in their tmux panes,
routine runs in their own processes. It refreshes worker liveness and settles
finished runs on the way through, so a worker whose pane died reads `crashed`
here before anything else notices.

Poll it — nothing about it is caused by the tab asking, so there is no event to
subscribe to. The right panel uses 10 s. Like `/v2/routines`, it takes no
harness lock, which is what keeps a poll from queueing behind a streaming turn.

### Routines

```
GET    /v2/routines               # Every routine + schedule state
POST   /v2/routines               # Create one (201; 400 with invalid_routine)
GET    /v2/routines/assignees     # lead + agents + standby workers, right now
POST   /v2/routines/tick          # Advance the schedule now
GET    /v2/routines/{id}          # One routine, with its last 20 runs
PATCH  /v2/routines/{id}          # Partial — unset fields are left alone
DELETE /v2/routines/{id}
POST   /v2/routines/{id}/run      # Fire now (409 `run_in_progress`)
GET    /v2/routines/{id}/runs?limit=20
```

These are the only `/v2` endpoints that **never take the harness lock** — they
read `.harness/routines/` and spawn detached processes, so they keep answering
while a turn is streaming, and a routine firing cannot interleave with it. The
gateway also ticks the schedule itself every `routines.tick_seconds` (default
30, configurable in `.harness/gateway.json`).

`PATCH` being partial is what makes the enable toggle and the full form the same
request. Toggling `enabled` back on re-anchors the schedule on now, so a routine
that was off for a week does not come back and replay every window it missed.

### MCP Servers

```
GET  /v2/mcp/servers   # Server status, tool counts, errors
```

### Memory (Vault)

```
GET  /v2/memory/search?q=...&limit=10  # Search vault
GET  /v2/memory/stats                  # Total memories, foresights, etc.
```

### Files (Sandboxed)

```
GET  /v2/files?path=...                # Read a file
GET  /v2/files/tree?path=...&depth=1   # List directory contents
```

### Sessions

```
GET  /v1/sessions                      # List all sessions with metadata
POST /v1/sessions                      # Create a new session
DELETE /v1/sessions/{id}               # Delete a session
GET  /v2/sessions/{id}/messages        # Get session history (for F11)
```

Each listed session carries an `origin`: `"agent:<name>"`, `"worker:<name>"`,
`"routine:<name>"`, or `null` for a chat somebody typed. Every momo-fetch
process on the machine writes into one sessions table, so this list holds team
workers and scheduled runs beside the conversations; `null` covers both a chat
and a session older than origins, which is the same answer as far as a reader is
concerned. The sidebar marks each non-null kind with its own glyph — colour is
never the only carrier.

### Status

```
GET  /health                           # Gateway health, turn active status, MCP count
GET  /v1/cost                          # Total cost, tokens, request count for session
```

`/health` is auth-exempt; it can be polled before a token is entered.

## Mutations & Conflicts

### Turn Conflict (409)

Only one turn can run at a time (the harness holds a global session). If a mutation is attempted while a turn is running:

```json
{
  "error": {
    "code": "turn_in_progress",
    "message": "Another turn is running; mutations are refused until it ends"
  }
}
```

The UI catches this with `err instanceof ApiError && err.isTurnConflict` and displays a conflict badge (spec F29).

Mutations that trigger this:
- POST `/v2/chat/stream`
- POST `/v2/agents/switch`
- POST `/v2/switch-model`
- POST `/v2/switch-provider`
- POST `/v2/settings/permission`
- DELETE `/v1/sessions/{id}`

### Stale Approval

After the user approves or denies, if the turn has already moved on (the approval window closed), the response is:

```json
{
  "error": {
    "code": "stale_approval",
    "message": "This approval is no longer pending"
  }
}
```

The UI should dismiss silently (it is not actionable).

## Polling & Refresh

**Header polls every 10 seconds:**

- `/health` — Detects model/provider changes, turn activity elsewhere, MCP server count
- `/v1/cost` — Session total cost

**Standing permissions badge polls every 10 seconds:**

- `/v2/settings` — Approved tools list

**On mutation success:**

- Calling `useUiStore().bumpServerState()` immediately refetches all polled resources
- This keeps multiple tabs in sync without waiting for the next poll interval

**Refetch patterns:**

- `useGatewayResource()` hook — Wraps the fetcher, provides `reload()`, auto-refetches on `serverStateNonce` change
- Headers: Use `Promise.all([getHealth(), getCost()])` to poll in parallel

## Error Response Format

All `/v2` endpoints return errors in this format (spec §8):

```json
{
  "error": {
    "code": "one_of_the_error_codes",
    "message": "Human-readable message",
    "details": { "key": "value" }  // Optional context
  }
}
```

`/v1` endpoints may return a flatter shape (legacy):

```json
{
  "error": "message string"
}
```

The `raiseForStatus()` function in `api-client.ts` normalizes both, always producing an `ApiError` with `code`, `status`, and `message`.

## Wire Type Maintenance

**Source of truth:** `src/gateway/v2_types.rs` and `src/gateway/v2_handlers.rs` in the Rust harness.

**Frontend mirror:** `web/src/lib/types.ts` (hand-maintained).

Type drift is silent — the SSE parser just drops event names it doesn't know. To make it visible:

1. The `sseEventNames` array lists all known events
2. Unknown event types yield an `unknown` event, surfaced in the UI
3. Assertion in `preferences.test.ts` ensures theme key doesn't drift

**When the gateway adds an event:**

1. Add it to the enum in `src/gateway/v2_types.rs`
2. Add it to `sseEventNames` in `web/src/lib/types.ts`
3. Update the `StreamEvent` discriminated union
4. The parser will surface unknown events until the UI is updated to handle them

## Example: Sending a Turn

```typescript
// 1. Open the stream
const res = await openChatStream(
  {
    session_id: chatStore.sessionId ?? undefined,
    messages: [{ role: "user", content: text }]
  },
  controller.signal  // Can be aborted
);

// 2. Read events
for await (const ev of readStream(res)) {
  switch (ev.type) {
    case "role":
      chatStore.onRole(ev.data);
      break;
    case "text":
      chatStore.onText(ev.data.content);
      break;
    case "approval_required":
      chatStore.onApprovalRequired(ev.data);
      break;
    // ... etc
  }
}

// 3. Stop (if needed)
await interrupt(turnId);
controller.abort();
```

## Performance Notes

- **Streaming text** — Sent as separate `text` events; the UI appends without waiting for the full response
- **Token polling** — A `usage` event arrives once per leg (a leg is an approval round-trip). Multiple `usage` events can arrive in one turn; token counts are additive, cost is a replacement
- **Context pressure** — `context_usage` tells you how much of the model's context window is used; this is not a hard limit, but approaching 95% may cause the agent to summarize or truncate
- **Large tool outputs** — Truncated at the gateway; `truncated: true` signals that the output was cut off and the detail panel shows the full log
- **Artifacts cost a tree walk** — the gateway stamps the sandbox tree before the turn and after the last leg (`src/artifacts.rs`), skipping `.git`, `node_modules`, `target`, `.next` and anything `.gitignore`/`.agentignore` excludes. ~17 ms per stamp on a 378-file repo, off the async runtime; a tree over 50k files is not diffed at all and the event is simply omitted

## Testing

- Test fixtures are in `web/` (not shown here)
- The SSE parser is heavily tested in `web/src/lib/sse-parser.test.ts` to prevent silent event loss
- Type safety is enforced by TypeScript; runtime JSON parsing can still fail, surfaced as `unknown` events
