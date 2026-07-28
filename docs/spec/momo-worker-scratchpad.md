# MoMo Worker — Implementation Scratchpad / Handoff

> **Purpose:** Working state for the MoMo Worker build so another agent can continue without re-deriving anything.
> **Spec:** [`docs/spec/momo-worker.md`](./momo-worker.md) — read §0 (Code Audit) first; it is the load-bearing part.
> **Last updated:** 2026-07-28 · branch `dev` · base commit `7c4b704` (0.8.0)
> **Build state:** `cargo check --all-targets` clean · `cargo test --bin momo-fetch` → **297 passed** (11 new)

---

## 1. Context

The task is a web UI + desktop app for momo-fetch, built as: Next.js frontend → existing axum gateway → Rust harness. Two things happened in this session:

1. **The spec was audited against the code and rewritten.** The original draft assumed behaviour the harness does not have — the corrections are §0 of the spec, table C1–C12. Anyone continuing this work must read that table; several "obvious" implementations are wrong for this codebase.
2. **The Sprint-1 P0 gateway slab was implemented.** Rust only. No frontend code exists yet.

The gateway (`src/gateway/`) is untracked in git (`?? src/gateway/`) — it is uncommitted work-in-progress, not a published module. Nothing here has been committed.

---

## 2. What is DONE

All in Rust, all compiling, all covered by unit tests where testable without a live LLM.

| Task | What landed | Where |
|---|---|---|
| **G10** Cost lifecycle | `begin_turn` / `record_usage` / `end_turn` / `context_usage` as the single definition; REPL rerouted through them; v1 handlers wired too | `src/harness.rs:440,446,451,456`; `src/cli/repl.rs` |
| **G1** Rich SSE stream | `POST /v2/chat/stream` — typed events, part mapping, 4-phase approval stitch, disconnect cleanup, provider timeout | `src/gateway/v2_handlers.rs` |
| — Turn guard + approvals | RAII lease, `(turn_id, tool_name)` approval keying, stale `call_id` rejection, `deny_all` for interrupt | `src/gateway/turn.rs` (7 unit tests) |
| **G2** Agents | `GET /v2/agents`, `POST /v2/agents/switch`, `POST /v2/agents/default` | `v2_handlers.rs` |
| **G3** Providers | `GET /v2/providers`, `POST /v2/switch-model` · `switch-provider` · `switch`; added `ProviderManager::list_all()` so unconfigured providers report `available:false` instead of being omitted | `v2_handlers.rs`; `src/providers.rs:334,456` |
| **G7** Settings | `GET /v2/settings`, `POST /v2/settings/permission`, `DELETE /v2/settings/approved-tools` | `v2_handlers.rs` |
| **G13** Interrupt | `POST /v2/chat/interrupt` — marks interrupted, denies parked approvals, calls `Harness::interrupt()` | `v2_handlers.rs` |
| **G11** Bind/port flags | `--gateway-port` (0 = OS-assigned), `--gateway-bind`, `MOMO_GATEWAY_LISTENING <url>` stdout line | `src/cli/mod.rs`; `src/gateway/mod.rs` |
| **G12** Health | `/health` auth-exempt via router split; readiness payload with `turn_active`, `turn_id`, provider/model/session/permission | `src/gateway/handlers.rs`; `mod.rs` |
| — V2 types + error model | Event enum with `name()`/`data()`, uniform `{"error":{code,message,details}}` via `v2_error()` | `src/gateway/v2_types.rs` |

### Two bugs found and fixed en route (not in the spec's task list)

1. **`GatewayConfig` derived `Default`.** With no `.harness/gateway.json`, `unwrap_or_default()` produced `port: 0` (random OS port) and `cors_origins: []` (empty allow-list → every browser blocked). Derived `Default` zeroes fields; serde `#[serde(default = …)]` only applies when the file exists. Now a manual `impl Default` mirrors the serde defaults. `src/gateway/mod.rs`.
2. **v1 chat handlers had no turn guard.** They take the write lock to resume a session. Against an in-flight v2 turn that queues a writer on the `RwLock`, and because it is write-preferring, every subsequent reader blocks behind it. Both v1 handlers now take the same `TurnLease`. `src/gateway/handlers.rs`.

Also added, from spec §9: the gateway now **refuses to start** on a non-loopback bind when auth is disabled, rather than silently exposing shell execution to the network.

---

## 3. What is NOT done

**Gateway (Sprint 1–2 remainder):**
- **G4** `GET /v2/mcp/servers` — MCP status
- **G5** `GET /v2/memory/search`, `/v2/memory/stats`
- **G6** `GET /v2/files`, `/v2/files/tree` — sandboxed read
- **G8** `GET /v2/sessions/{id}/messages` — session messages with tool calls
- **G9** `GET /ui/*` — static serving

**Frontend:** nothing. F1–F29 all open. No `web/` directory exists.

**Phase 2 (Tauri):** nothing. T1–T13 all open.

Suggested order: finish **G4 → G5 → G6 → G8** so the gateway surface is complete and stable, *then* scaffold the frontend against it. Building the UI against a half-finished API means reworking `lib/types.ts` twice.

---

## 4. Hard-won knowledge — read before writing code

These cost real investigation time. Getting them wrong produces code that compiles and deadlocks.

### 4.1 The approval flow is not a pause/resume
adk **ends** the `EventStream` and sets `event.actions.tool_confirmation`. There is nothing to resume. The decision is applied by `Harness::run_confirmation_turn(tool_name, approved)` (`src/harness.rs:405`), which calls `rebuild_runner()` → needs `&mut self` → **write lock** → and starts a *brand new turn* whose user content is the literal string `"approved"` or `"denied"`.

The gateway hides this seam: one SSE response, four phases — read lock (stream leg) → **no lock** (human wait) → write lock (confirmation turn) → read lock (next leg). See `run_v2_turn` in `v2_handlers.rs`.

### 4.2 Approval is sticky, by tool NAME
`approved_tools: HashSet<String>` (`src/harness.rs:38`) is per-process and baked into `RunConfig::tool_confirmation_decisions`, which adk keys by tool name. **One "Approve" on `shell_exec` disarms confirmation for that tool for the rest of the process.** The `approval_required` event carries `sticky: true` and the UI is required to disclose this (spec F9). `GET /v2/settings` exposes the set; `DELETE /v2/settings/approved-tools` clears it.

Denial is **not** recorded — the model can re-request immediately. Spec Q10 tracks whether to add a `denied_tools` set.

### 4.3 Lock discipline is not optional
`tokio::sync::RwLock` is write-preferring: a waiting writer blocks new readers. Rules that are already implemented and must be preserved:
- Never hold any guard across the approval wait (up to 5 min).
- All mutating `/v2` endpoints call `require_idle()` and **409 before** taking the write lock — so a writer never queues behind a turn.
- The vault is behind a **`std::sync::Mutex`** (`src/harness.rs:28`). Never lock it across an `await` — copy results out or use `spawn_blocking`. This matters for **G5**.

### 4.4 Turn concurrency is process-wide
`Harness` has one `current_session_id`, one runner, one provider. There is no per-session isolation. `TurnRegistry` admits one turn at a time globally; a second returns 409 `turn_in_progress`. Do not "fix" this by making it per-session — the underlying state is shared.

### 4.5 Disconnect handling
The turn runs in a spawned task feeding an mpsc channel; the SSE body owns the receiver. Client disconnects → receiver dropped → next `send` fails → task interrupts the runner and returns → `TurnLease` drops → harness released. This is why the channel exists rather than inlining into `async_stream!`. It also lets helper fns emit events (a `stream!` macro body can only `yield` inline).

### 4.6 adk API shapes (saves a registry dig)
Source: `~/.cargo/registry/src/*/adk-core-0.7.0/src/`
- `Part::Text { text }` · `Part::FunctionCall { name, args, id: Option<String>, thought_signature }` · `Part::FunctionResponse { function_response: FunctionResponseData, id: Option<String> }`; `FunctionResponseData { name, response: Value, .. }` — `types.rs:178+`
- `ToolConfirmationRequest { tool_name, function_call_id: Option<String>, args: Value }` — `context.rs:594`
- `UsageMetadata { prompt_token_count, candidates_token_count, total_token_count, .. }` (all `i32`) — `model.rs:148`
- `event.llm_response.usage_metadata` · `event.actions.tool_confirmation` · `event.llm_response.error_message` · `event.is_final_response()`

### 4.7 There is no per-server MCP tool count
`McpService::toolset()` returns one merged `Option<Arc<dyn Toolset>>` — no per-server attribution. **G4** must report `tool_count: null` per server plus a global total, or add attribution in `src/mcp/`. The UI must render null as "—", never "0".

### 4.8 Memory search: use the right function
`MemorySidecar::search_for_context` is enrichment-shaped (input → prompt injection, with its own relevance gate). General search for **G5** is `ObsidianVault::search(&MemoryQuery)` (`src/memory/vault.rs:321`), plus `stats()` / `counters()`.

---

## 5. Environment blocker — live verification is incomplete

**No LLM turn can complete on this machine.** Two independent causes:
1. The configured default model `google/gemma-4-26b-a4b-it:free` (`.harness/settings.json`) is rejected by OpenRouter: `model.invalid_input: Provider returned error`.
2. The OpenRouter account has no credits: `model.forbidden: Insufficient credits`. Only OpenRouter is configured; every other provider reports `available:false`.

Both are pre-existing environment issues, **not** bugs in this code — the untouched v1 endpoint fails identically on the same prompt. Nothing was spent verifying this; the model switch used during testing was runtime-only (`POST /v2/switch-model`) and `settings.json` was not modified.

### Verified live
Port-0 bind + listening line · `/health` · `/v2/agents` · `/v2/settings` · `/v2/providers` · model + permission switching · error model (400 bad permission mode, 409 `stale_approval`, 409 `turn_in_progress`) · SSE framing, event names and ordering (`role` → … → `context_usage` → `done`) · error path surfaces a typed `error` + `done` where v1 just stops mid-stream · **6 concurrent turns → 1 admitted, 5 rejected 409, lease released after**.

### NOT verified end-to-end (needs a working tool-capable model)
- `tool_call_start` / `tool_call_result` part mapping
- The approval handshake round-trip (`approval_required` → `POST /v2/chat/approve` → `approval_resolved` → follow-up leg)
- Cost parity between a REPL turn and a gateway turn (**G10** acceptance)

Unit tests cover the registry semantics, preview truncation on char boundaries, error detection, and event naming — but not the live wire path.

**To unblock:** add OpenRouter credits, or configure another provider key, then set a tool-capable model. Re-run the probes in spec §10.

---

## 6. Reproducing the test setup

```bash
# Start on an OS-assigned port; parse the URL from stdout
cargo run -- --gateway --gateway-port 0
# → MOMO_GATEWAY_LISTENING http://127.0.0.1:51686

U=http://127.0.0.1:51686
curl -s $U/health                  # no auth required (G12)
curl -s $U/v2/agents
curl -s $U/v2/providers
curl -s $U/v2/settings

# Error model
curl -s -X POST $U/v2/settings/permission -H 'content-type: application/json' -d '{"mode":"banana"}'   # 400
curl -s -X POST $U/v2/chat/approve -H 'content-type: application/json' \
     -d '{"turn_id":"t_nope","tool_name":"shell_exec","call_id":"c1"}'                                 # 409 stale_approval

# Turn guard: fire several concurrently, expect exactly one admitted
for i in 1 2 3 4 5 6; do (curl -sN -X POST $U/v2/chat/stream -H 'content-type: application/json' \
  -d '{"messages":[{"role":"user","content":"hi"}]}' > /tmp/race_$i.txt &); done

# Live turn (blocked until a working model is configured)
curl -N -X POST $U/v2/chat/stream -H 'content-type: application/json' \
  -d '{"messages":[{"role":"user","content":"read Cargo.toml"}]}'
```

Note: `timeout` is not available on this macOS shell — use `curl --max-time`.

---

## 7. Artifacts

- [`docs/spec/momo-worker.md`](./momo-worker.md) — the spec. §0 audit table, §2.2 lock discipline, §2.4 corrected approval flow, §5 event protocol, §8 error model, §9 security, §10 testing.
- `src/gateway/turn.rs` — **new**, turn guard + approval broker (316 lines, 7 tests)
- `src/gateway/v2_handlers.rs` — **new**, all `/v2` handlers (871 lines, 4 tests)
- `src/gateway/v2_types.rs` — **new**, V2 request/event types + `v2_error()` (262 lines)
- `src/gateway/mod.rs` — modified: state, config, routes, router split, bind overrides
- `src/gateway/handlers.rs` — modified: turn lease on v1, cost wiring, readiness payload
- `src/harness.rs` — modified: turn lifecycle + approved-tools management
- `src/providers.rs` — modified: `list_all()` + `KNOWN_PROVIDERS`
- `src/cli/mod.rs`, `src/cli/repl.rs` — modified: flags; REPL routed through shared lifecycle

Nothing is committed. `git status` also shows unrelated pre-existing modifications (`Cargo.toml`, `Cargo.lock`, `docs/user-guide.md`, `src/main.rs`) that are **not** part of this work.

---

## 8. Suggested skills for the next agent

- **`code-review`** — before committing the gateway slab; the concurrency and lock-ordering logic is the risky part and deserves a second pass.
- **`security-review`** — mandatory before **G6** (sandboxed file read) ships. Path traversal, symlink escape, gitignored-secret leakage, and 404-vs-403 information disclosure are all in scope; hardening requirements are in spec G6 and §9.
- **`frontend-design`** or **`example-skills:frontend-design`** — when starting F1/F5, for the 3-panel layout and visual direction.
- **`example-skills:webapp-testing`** — Playwright driving of the approval dialog once the frontend exists.
- **`commit`** — the work is uncommitted; conventional commits when the user is ready.

Do **not** reach for `prd-generator` — the spec already exists and is reconciled against the code.

---

## 9. Next steps

1. **Unblock the provider** (credits or another key + a tool-capable model), then run the two unverified acceptance checks in §5. Do this first — it gates confidence in everything already written.
2. **G4** MCP status — smallest remaining gateway task; mind §4.7 (no per-server tool counts).
3. **G5** memory search — mind §4.8 (right function) and §4.3 (std Mutex, no await while held).
4. **G6** file read — the security-sensitive one; run `security-review` on it.
5. **G8** session messages with tool calls — extends the existing `/v1/sessions/{id}` walk to map `FunctionCall`/`FunctionResponse`, pairing by call id.
6. **Then** frontend F1–F3 (scaffold, API client, SSE parser). Note **C3**: `EventSource` cannot POST — use `fetch` + `ReadableStream`. The hand-rolled frame parser is the most likely source of silent breakage; unit-test it against split frames, `\r\n`, multi-line `data:`, comment keep-alives, and unknown event names.
7. **Decide spec Q9** (sticky-approval UX) before building F9. Recommendation on record: ship honest "approve for the session" copy now; per-call approval is a harness change, not a UI change.

---

## 10. Sensitive information

No secrets are included in this document. Specifically redacted/omitted:

- **API keys and values** — never read or printed; `SecretStore` was only ever queried for *presence* (`available: true/false`).
- **Provider credential inventory** — described only as "only OpenRouter is configured", with no key material, account identifier, or billing detail.
- **Session IDs and local absolute paths** seen during testing are ephemeral and have been omitted from the examples above.
