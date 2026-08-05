# MoMo Worker — Implementation Scratchpad / Handoff

> **Purpose:** Working state for the MoMo Worker build so another agent can continue without re-deriving anything.
> **Spec:** [`docs/spec/momo-worker.md`](./momo-worker.md) — read §0 (Code Audit) first; it is the load-bearing part.
> **Dispatch:** spec **§12** is the work plan (packages, model routing, waves). §9 below is now just a pointer into it.
> **Last updated:** 2026-08-05 · branch `dev` (0.9.1)
> **Build state:** `cargo check --all-targets` clean (warnings only) · `cargo test --bin momo-fetch` → **303 passed**

---

## 📊 Status at a glance

> **Keep this block current. It is the first thing anyone reads.**
> Update it at the end of every work package, together with §0 below.

**`19888b4`+WP-3 · dev · 2026-08-05 · rust 303 passed · web 22 passed · tsc clean**

| ส่วน | เสร็จ | เหลือ | |
|---|---:|---:|---|
| **Gateway** (G1–G13 + R1) | **14** | **0** | ██████████████████ **100%** ✅ |
| **Wave-0 bugs** (B0–B4) | 5 | 0 | ██████████████████ 100% |
| **Frontend** (F1–F29) | 4 | 25 | ██░░░░░░░░░░░░░░░░ 14% |
| **Desktop** (T1–T13) | 0 | 13 | ░░░░░░░░░░░░░░░░░░ 0% |
| **Phase 1** (gateway + frontend) | 18 | 25 | ███████░░░░░░░░░░░ 42% |
| **ทั้งโปรเจกต์** | 18 | 38 | ██████░░░░░░░░░░░░ 32% |

**✅ Gateway เสร็จครบ · frontend มี shell แล้ว.** วงจรครบรอบ: `npm run build` → `web/out/` → gateway เสิร์ฟที่ `/ui` → หน้าเว็บเรียก `/health` กลับมาที่ origin เดิมได้ ขึ้น `connected` พร้อมชื่อ model จริง ที่เหลือคือ chat panel, tool cards และ approval dialog

**Wave ถัดไป: WP-4** — F4 chat store · F6 chat panel · F7 markdown · F8 tool cards · **F9 approval dialog** · F10/F11 sessions · F29 409 handling

**⚠️ ค้างอยู่ 3 อย่าง**

| | เรื่อง | ผลกระทบ |
|---|---|---|
| 🔴 | **`security-review` ยังไม่รันบน WP-2 (G6)** — spec §12.7 ระบุเป็น blocking gate | G6 ยังไม่ควรถือว่า ship ได้ ที่เทสไปเป็นการเทสของผู้เขียนเอง |
| 🟡 | **cost parity (G10 acceptance) ยังพิสูจน์ไม่ได้** | ต้องใช้ paid model ถึงจะวัดได้ — free model ราคา 0 ทั้งสองฝั่ง เทียบแล้วไม่มีความหมาย |
| 🟡 | **`GET /v1/sessions` คืน `event_count: 0` ทุก session** | bug เดิมใน list projection (`/v1/sessions/{id}` ถูกต้อง) จะกวน F10 ถ้าจะโชว์จำนวน event |

**Human item เดียวที่เหลือ:** code-signing (T13 → T11) อยู่ ~สัปดาห์ 5 ไม่บล็อกอะไรตอนนี้ — build unsigned จาก T10 ใช้งานได้ปกติ

---

## 0. Wave progress log

Newest first. One entry per work package, added on completion.

### ✅ WP-3 — frontend foundation (F1, F2, F3, F5) · 2026-08-05

Wave 1, lane 3. First frontend code in the project — `web/` did not exist before this.

| Task | Landed | Notes |
|---|---|---|
| **F1** | `web/` scaffold | Next **16.3** + React 19.2 + Tailwind v4 + zustand, `output:'export'` → `web/out` |
| **F2** | `lib/types.ts`, `lib/api-client.ts` | full type surface + URL resolution + §8 error parsing |
| **F3** | `lib/sse-parser.ts` | `fetch` + `ReadableStream`, hand-rolled frames, **22 vitest cases** |
| **F5** | `components/layout/*` | 3-panel shell, collapsible, dark-first |

**Spec deviation:** §3.2 specifies Next 15; `create-next-app@latest` installs **16.3**. Kept 16 — it is the current line and nothing in the spec depends on 15. Note `web/AGENTS.md` (written by `next dev`) warns that Next 16 has breaking changes and to read `node_modules/next/dist/docs/` before writing config; that is how `basePath` below was confirmed.

**🔴 Two integration bugs that only appear when you actually serve the build through G9.** Both are the kind that a unit test never catches:

1. **Completely unstyled page.** Next emits **absolute** asset URLs, so a bundle mounted at `/ui` links `/_next/static/…` — not under `/ui`, so the browser 404s every stylesheet and script. Fixed with **`basePath: '/ui'`** in `next.config.ts`. This is easy to mis-diagnose as a Tailwind problem: fetching those same assets by hand *with* a `/ui` prefix returns 200, which is exactly what I did first and it told me nothing was wrong. **`basePath` must match G9's mount point**, and it is inlined at build time — changing one without the other silently breaks asset loading.

2. **"offline" on a page the gateway itself served.** `NEXT_PUBLIC_GATEWAY_URL` is baked in at build time, but the gateway binds an ephemeral port under `--gateway-port 0`, so the value is guaranteed wrong. `gatewayUrl()` now prefers `window.location.origin` when the path starts with `/ui`. Side benefit: those calls become same-origin, so the permissive CORS default stops mattering on the served-by-gateway path. This step is **not** in spec §3.2's resolution chain — it was added from testing and the spec should absorb it.

**Verified end to end:** `npm run build` → `web/out/` → gateway serves `/ui` with correct content types → SPA fallback works on `/ui/anything/deep` → header shows **`● connected`** and the live model name read back from the running harness. Screenshotted at 1440×900.

**Design direction** (F5, for whoever does F6–F9 and must stay consistent): *instrument panel, not terminal cosplay.* Two rules carry it —
- **Amber (`--color-signal`) is a state, never decoration.** It means the agent is working or wants something from you. If amber is on screen, the user has something to attend to. Do not use it for emphasis, links, or branding.
- **Monospace is machine speech** (tool names, paths, session ids, token counts); **sans is the interface talking to you.** They never swap.

The signature element is the **turn rail** — the 3px strip on the chat panel's leading edge. Idle it is dark; running, an amber segment travels; awaiting approval, the whole column pulses. It exists because the turn lifecycle is this product's core and is otherwise invisible. Status is never colour-alone: every dot is paired with a word.

Placeholders say what is missing and which task fills it (`F10`, `F14`, …) rather than faking content — a shell with mock messages hides exactly the integration problems above.

---

### ✅ G9 — static UI serving · 2026-08-05 · **Gateway is now complete**

`GET /ui/*` from `ServeDir`, with `ServeFile(index.html)` as the fallback so client-side routes survive a refresh. Path comes from `gateway.json` → `ui_dir`, defaulting to **`web/out`** (Next.js static export writes `out/`, not `dist/` — spec C12). Relative values resolve against the project root. Needed `tower-http`'s `fs` feature, now enabled.

**Auth-exempt, and for a stronger reason than `/health`:** a browser navigating to a page cannot attach an `Authorization` header, so an HTML shell behind a bearer token is unreachable by construction. Only the static bundle is exposed; the token still guards every `/v1` and `/v2` call the loaded app makes.

**Absent `web/out` is not an error.** The gateway is useful headless and the frontend may simply not be built yet, so the route is skipped and a one-line hint is logged. Verified both ways: without the directory `/ui/` → 404 and `/health` → 200; with it, `/ui/`, `/ui/index.html` and `/ui/_next/app.css` all → 200.

**Traversal check.** `/ui/../.env`, `/ui/../../.env`, `/ui/%2e%2e/%2e%2e/.env` and `/ui/..%2f.env` all return 200 — but the body is `index.html`, not `.env`. `ServeDir` rejects the traversal and the SPA fallback answers. Confirmed by grepping the response for `API_KEY`/`sk-or-`: no match on any of them. **Do not "fix" that 200 into a 404 without re-checking the body** — it is the fallback behaving correctly, and the same 200 is what makes `/ui/settings/deep` work.

---

### ✅ WP-2 — G6 sandboxed file access · 2026-08-05

Wave 1, lane 2. Landed in `src/gateway/files.rs` (own module — this is the surface a security review should read first).

**🔴 The spec's security assumption was wrong, and it would have leaked the API key.**

Spec G6 says to "filter with `is_ignored` so `.gitignore`d files and secrets don't leak into the browser". `is_ignored` consults **`.agentignore` only** (`src/sandbox/mod.rs:170`). This repo has **no `.agentignore`**, so `ignore_matcher` is `None` and *every* in-root path reports as not-ignored. `.env` is gitignored but not agentignored — so as specified, `GET /v2/files?path=.env` would have returned `OPENROUTER_API_KEY` in plaintext. CORS is permissive by default, so any page the user merely visited could have fetched it.

**Fixed with four layers, in this order:**

| # | Layer | Catches |
|---|---|---|
| 1 | `resolve_path` | `..`, symlink escape, absolute paths (`Path::join` discards the root, so the trailing prefix re-check is what saves it) |
| 2 | `is_ignored` | `.agentignore` |
| 3 | `gitignored()` **new** | `.gitignore` — the layer the spec assumed layer 2 provided |
| 4 | `is_sensitive()` **new** | unconditional deny-list: `.env*`, `*.pem/key/p12/pfx/jks`, `id_rsa`, `.ssh`, `.git`, `.netrc`, `credentials`, `secrets.*` |

Layer 4 exists because layer 3 is only as good as the user's `.gitignore`, and a project that never gitignored its keys is exactly the project whose keys most need protecting. It is deliberately **not configurable**.

**Verified live — every one of these returns 403:** `.env` · `.env.local` · `../../etc/passwd` · `/etc/passwd` · `.git/config` · `../.env` · `src/../.env` · `.ssh/id_rsa` · `server.pem` · `target/debug/momo-fetch` (gitignored). The tree walk applies the same rules — `.` at depth 1 lists 18 entries with `.env`, `.git` and `target/` all absent.

Other acceptance criteria: a 3 MB file → capped at exactly 1 MiB with `truncated:true` and no OOM (the cap is applied *while reading*, via `File::take`, not after); a binary file → `binary:true` with `content:null` rather than lossy-decoded garbage; a directory via `/v2/files` → 400; a missing but permitted in-sandbox path → 404.

**403 vs 404 is load-bearing.** Everything denied returns 403, including paths that do not exist but would be denied. 404 is reserved for in-sandbox, permitted, genuinely absent. Splitting those would make the endpoint an oracle for mapping the filesystem.

**Still owed:** the `security-review` skill has *not* been run on this yet — spec §12.7 lists it as a blocking gate for WP-2. Everything above is my own testing. Run it before treating G6 as shippable.

---

### ✅ WP-1 — Gateway read endpoints (G4, G5, G8) · 2026-08-05

Wave 1, lane 1. All three landed, verified live against a running gateway.

| Task | Endpoint | Verified |
|---|---|---|
| **G4** | `GET /v2/mcp/servers` | 4 servers listed (3 http + 1 stdio) with transport and status |
| **G5** | `GET /v2/memory/search`, `/v2/memory/stats` | search hits a real vault (22 memcells); `limit=1000` → clamped to 100; empty `q` → 400 |
| **G8** | `GET /v2/sessions/{id}/messages` | 21 events → 5 tool calls, paired by call id, previews populated; missing session → 404 |

Deadlock canary still green: `GET /v2/agents` → 200 in 0.36 ms.

**Two problems found and fixed while building — both would have shipped a lying UI:**

1. **`running` under-reported.** `McpService::running_count()` only knows about the stdio manager, so with 3 HTTP servers connected it returned `1` while the `servers` array showed 4 × `running` — F14 would have printed "1 running" beside four green dots. `running` is now derived from the reported array; the manager's figure is kept as `running_stdio` rather than discarded.

2. **`pending` tool calls that never resolve.** The pre-approval `FunctionCall` is abandoned when `run_confirmation_turn` starts a fresh turn, and the post-approval call gets a **different id** (see §5, "`call_id` does not survive an approval"). On replay that left a card marked `pending` forever — F11 would spin on it on every reload. Unpaired calls are now marked **`unresolved`**, which is the honest state for history: nothing is pending in a finished session. Confirmed on the real approval-test session — exactly one `unresolved`, the rest `done`.

**Also noticed, not fixed (out of WP-1 scope):** `GET /v1/sessions` reports `event_count: 0` for every session, while `GET /v1/sessions/{id}` on the same id correctly reports 21. Pre-existing bug in the list handler's projection, not in G8. Worth a task if F10 wants to show event counts in the session list.

**Notes for whoever does F14:** `tool_count` is `null` for *both* transports right now. The scratchpad §4.7 revision says HTTP servers are attributable in principle — they are, but `Toolset::tools()` needs an `Arc<dyn ReadonlyContext>` and is async, which is not worth plumbing into a status endpoint. The response shape is unchanged and the UI contract stands: render `null` as **"—", never "0"**.

---

## 1. Context

The task is a web UI + desktop app for momo-fetch, built as: Next.js frontend → existing axum gateway → Rust harness. Two things happened in the originating session:

1. **The spec was audited against the code and rewritten.** The original draft assumed behaviour the harness does not have — the corrections are §0 of the spec, table C1–C12. Anyone continuing this work must read that table; several "obvious" implementations are wrong for this codebase.
2. **The Sprint-1 P0 gateway slab was implemented.** Rust only. No frontend code exists yet.

**The gateway work is committed** as `546ef04` *"✨ feat(gateway): add V2 API with tool approval, turn guard and cost tracking"*, and the version has moved to **0.9.1**. Earlier revisions of this document described `src/gateway/` as untracked WIP — that is no longer true.

### Uncommitted work in the tree (not part of the gateway slab)

`git status` shows `M src/mcp/mod.rs` — an unrelated in-progress change that adds `PrefixedToolset` wrapping so MCP tools can't collide with built-in tool names. It matters here for one reason: **it partially invalidates §4.7 below**, which is a load-bearing constraint for G4. See that section.

Also modified and unrelated: `Cargo.toml`, `Cargo.lock`, `docs/user-guide.md`. Also untracked and unrelated: `.harness/skills/`, `.pi/`, `AGENTS.md`, `docs/skills/`, `src/momo-gateway/` (a shell script, not a Rust module).

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

> **Superseded ordering advice.** This section used to say: finish G4 → G5 → G6 → G8 first, *then* scaffold the frontend, because building the UI against a half-finished API means reworking `lib/types.ts` twice.
>
> **Spec §12.3 reverses this.** That reasoning was correct while the API surface was still moving; spec §5 and §7 have since frozen it, so `types.ts` is fully derivable today without a single endpoint existing. The new plan lands `501 not_implemented` route stubs first (R1), then runs gateway and frontend **in parallel** — which takes ~4 days off the critical path. See spec §12.3 for the contention map and §12.5 for the wave schedule.

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

### 4.7 MCP tool counts are attributable for HTTP servers, not stdio — *revised 2026-08-05*

**This changed under the uncommitted `src/mcp/mod.rs` edit (see §1).** The original claim — "no per-server attribution at all" — is now only half true, and G4 should be built against the new shape:

- **HTTP servers: countable.** `http_toolsets` became `Vec<(String, Arc<dyn Toolset>)>` (was `Vec<Arc<dyn Toolset>>`), each wrapped in `PrefixedToolset::new(ts, server_id)`. The server id is retained, so a real `tool_count` is available per server.
- **stdio servers: still not countable.** All stdio servers go through one `McpServerManager` wrapped in a *single* `PrefixedToolset::new(manager, "mcp")` — one shared prefix, no per-server split. Still `null`.

**Caveat before relying on this:** `Toolset::tools()` takes `Arc<dyn ReadonlyContext>` and is async (`adk-tool-0.7.0/src/toolset/compose.rs:159`), so counting is not a free field read — it needs a context and an `.await`. If plumbing a context into the G4 handler proves ugly, report `null` for everything and keep the endpoint shape.

Either way the **response shape does not change** and neither does the UI contract: `tool_count` stays nullable, and F14 must render null as **"—", never "0"**. Decide per-server-count-vs-null as an implementation detail inside G4; do not let it change `/v2/mcp/servers`.

⚠️ If the `src/mcp/mod.rs` change is reverted or lands differently, re-check this section before building G4.

### 4.8 Memory search: use the right function
`MemorySidecar::search_for_context` is enrichment-shaped (input → prompt injection, with its own relevance gate). General search for **G5** is `ObsidianVault::search(&MemoryQuery)` (`src/memory/vault.rs:321`), plus `stats()` / `counters()`.

---

## 5. Environment blocker — live verification is incomplete

## 5. ~~Environment blocker~~ — RESOLVED 2026-08-05

> **This section previously said no LLM turn could complete. That is no longer true.**
> A working free model was found. **Everything in the "NOT verified" list below has now been verified end-to-end**, including the approval round-trip.

### The working configuration

```
provider: openrouter
model:    nvidia/nemotron-3-ultra-550b-a55b:free      ← free tier, tool-capable, 1M context
```

Verified: streaming text, **tool calling**, **the full approval handshake**, stickiness, interrupt-free completion. This model is the reference config for all remaining verification work.

### What was actually wrong (for the record)

| # | Finding | Status |
|---|---|---|
| 1 | Default `google/gemma-4-26b-a4b-it:free` → `model.invalid_input` | still broken — **it is a bogus slug**, and it is still the default in `.harness/settings.json` (both `default_model` and `memory.sidecar_model`) |
| 2 | OpenRouter account has no credits → `model.forbidden: Insufficient credits` | still true — reconfirmed against paid `deepseek/deepseek-chat-v3-0324` |
| 3 | OpenRouter retired the `:free` tier on the *obvious* fallbacks | true for `deepseek-chat-v3-0324:free`, `llama-3.3-70b-instruct:free`, `qwen-2.5-72b-instruct:free` — but **not universal**; nemotron's free tier is live |

The whole blocker was **a bad default model plus an unlucky choice of fallbacks**, not an account problem. Credits are still zero and are no longer needed.

### Where API keys actually come from — not the keychain

`SecretStore::get` (`src/config/secrets.rs:58`) tries the env var first, then the OS keychain. In practice **only the env-var path works**:

- `dotenvy::dotenv()` at `src/cli/mod.rs:68` loads **`.env`** at the repo root, which currently defines `OPENROUTER_API_KEY` and nothing else. That is the sole reason openrouter is the only available provider.
- **The keychain path is entirely non-functional.** `keyring-core` 1.0 requires a concrete store to be registered at startup, and nothing in the codebase ever calls `set_default_store` — so every `Entry::new` fails with *"No default store has been set"*. `SecretStore::set`/`delete` cannot work either.

**To add a provider, put its key in `.env`** (`ZAI_API_KEY`, `GROQ_API_KEY`, …). Do not expect `secrets set` to work until a keyring store is registered.

### z.ai / `glm-5-turbo` — untested, no key

`POST /v2/switch {"provider":"zai","model":"glm-5-turbo"}` → `503 provider_unavailable`, `Keychain error: … No default store has been set`. That is the missing-key path above, **not** a statement about the model. The switch was correctly rejected and harness state was left unchanged (the in-flight stream continued on the previous model). Add `ZAI_API_KEY` to `.env` to evaluate it.

### Bugs found while verifying — **all fixed 2026-08-05**

> Summary of the fixes, then the original findings for context. All verified live; `cargo test --bin momo-fetch` → **297 passed**.
>
> | ID | Fix | Verified |
> |---|---|---|
> | **B0** | `.harness/settings.json` → `nvidia/nemotron-3-ultra-550b-a55b:free` (both `default_model` and `memory.sidecar_model`) | gateway starts on a working model |
> | **B1** | `ollama_reachable()` — 150 ms TCP probe, honours `OLLAMA_HOST` (`src/providers.rs`) | `/v2/providers` → `ollama available:false` |
> | **B2** | `get_pricing` returns zero for any model slug ending `:free` (`src/cost.rs`) | `/v1/cost` → `total_cost: 0.0` over 30k tokens |
> | **B3** | all 5 tool contexts moved `thread_local!` → process-global `RwLock` | 0 occurrences of `not initialized` across a full approval turn |
> | **B4** | `SecretStore::get` maps a missing keyring store to `NotFound` (`src/config/secrets.rs`) | zai switch → *"Set ZAI_API_KEY or use /key set zai"* |
>
> **B3 turned out to be much bigger than the symptom suggested — read finding 3 below.**

### Original findings

1. **`ollama` availability is a false positive.** `src/providers.rs:343` — `available: *provider == "ollama" || SecretStore::get(provider).is_ok()`. Ollama is hard-coded available because it needs no key, with **no liveness probe**; `/v2/providers` reports `ollama available:true` while nothing listens on `:11434`. Misleads **F13**, which greys out unavailable providers *and explains why*. Filed as **B1** in spec §12.1.
2. **`:free` models accrue nonzero cost.** The nemotron `:free` model reported `cost_usd` climbing to `0.596` over 118k tokens, and `GET /v1/cost` agrees. The pricing table does not understand the `:free` suffix. This directly corrupts **G10**'s acceptance criterion and **F18**'s cost badge. Filed as **B2**.
3. **`shell tool sandbox not initialized`** — first seen as an intermittent failure on the turn following `run_confirmation_turn`, so it was initially filed as an approval-path bug. **That diagnosis was wrong, and the real one is worse.**

   **Root cause: thread affinity.** `file`, `shell`, `search`, `kms` and `memory` each stored their sandbox/vault in a **`thread_local!`**. `build_tool_registry` (`src/tools/mod.rs:37-40`) sets it on whichever thread builds the registry — but tools *execute* later on an arbitrary tokio worker thread, where the thread-local is unset. So **any** tool call could fail with `"… not initialized"` depending purely on which worker picked up the task. `rebuild_runner` only changed the timing enough to make it visible; `file_read`, `grep`, and every `mem_*` tool were equally exposed. This had nothing to do with approvals.

   **Fix:** all five contexts are now a process-global `RwLock<Option<Arc<…>>>`. That is the correct shape here — the harness has exactly one sandbox and one vault by construction (§4.4) — and `get_*` clones the `Arc` out, so no guard is held across an `await` (§4.3 still holds).

   **Watch out when writing tool tests:** the thread-locals were also providing test isolation, and 15 tests began clobbering each other the moment the context went global. Tests that install a sandbox or vault must now take `crate::tools::test_support::sandbox_guard()` and hold it for the body of the test.

**No credits were spent** — the working model is free tier, and every paid probe failed before billing. All model and permission switches were runtime-only (`POST /v2/switch-model`, `/v2/settings/permission`); **`.harness/settings.json` was not modified**, so its broken default model is still there (see finding 1).

### Verified live
Port-0 bind + listening line · `/health` · `/v2/agents` · `/v2/settings` · `/v2/providers` · model + permission switching · error model (400 bad permission mode, 409 `stale_approval`, 409 `turn_in_progress`) · SSE framing, event names and ordering (`role` → … → `context_usage` → `done`) · error path surfaces a typed `error` + `done` where v1 just stops mid-stream · **6 concurrent turns → 1 admitted, 5 rejected 409, lease released after**.

### Verified live — 2026-08-05 session (the previously blocked items)

All on `nvidia/nemotron-3-ultra-550b-a55b:free`.

| Check | Result |
|---|---|
| `tool_call_start`/`tool_call_result` part mapping | ✅ `file_read` — `{id,name,args}` out, result paired on the **same `id`**, `output_preview` + `truncated` populated |
| **Approval handshake round-trip** | ✅ full 4-phase stitch, single unbroken SSE response — see transcript below |
| **Deadlock canary ([§2.2](#43-lock-discipline-is-not-optional))** | ✅ `GET /v2/agents` while an approval was parked → **200 in 0.4 ms**. The lock discipline is correct. |
| Sticky approval (**C2**) | ✅ after approving once, `GET /v2/settings` → `approved_tools:["shell_exec"]`, and a **second** `shell_exec` in strict mode produced **zero** `approval_required` events |
| Keep-alive comment frames | ✅ a bare `:` line appears mid-stream — **confirms the F3 parser must handle comment lines**, this is not theoretical |
| Event ordering guarantees | ✅ `role` first, `done` last, `tool_call_result` after its `tool_call_start`, `usage`/`context_usage` before `done` |
| Cost parity REPL vs gateway (**G10**) | ⚠️ **still open** — gateway side records correctly, but see bug **B2**: `:free` models are billed as paid, so the number itself is wrong on both sides |

Approval transcript (abridged, one response):

```
role → usage → tool_call_start{shell_exec} → text("Tool confirmation required…")
     → approval_required{sticky:true, destructive:false, expires_at:…}
     ⏸  [POST /v2/chat/approve → {"resolved":true,"approved":true,"sticky":true}]
     → approval_resolved{approved:true, reason:"user"}
     → :                                    ← keep-alive
     → usage → tool_call_start → tool_call_result{stdout:"hello-from-approval-test\n", exit_code:0}
     → text×5 → usage → context_usage → done{stop_reason:"complete"}
```

Note the follow-up leg emits a **new `call_id`** for the same logical call — the pre-approval `tool_call_start` and the post-approval one do **not** share an id, because it is genuinely a new turn (C1). **F8/F9 must correlate on tool *name* plus ordering, not on `call_id`**, or the UI will render two separate tool cards for one user-approved action.

Unit tests cover the registry semantics, preview truncation on char boundaries, error detection, and event naming; the wire path above is now covered by manual probe.

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

All of the below landed in **`546ef04`**.

- [`docs/spec/momo-worker.md`](./momo-worker.md) — the spec. §0 audit table, §2.2 lock discipline, §2.4 corrected approval flow, §5 event protocol, §8 error model, §9 security, §10 testing, **§12 execution plan**.
- `src/gateway/turn.rs` — **new**, turn guard + approval broker (7 tests)
- `src/gateway/v2_handlers.rs` — **new**, all `/v2` handlers (17 fns, 4 tests)
- `src/gateway/v2_types.rs` — **new**, V2 request/event types + `v2_error()`
- `src/gateway/mod.rs` — modified: state, config, routes (`:184-206`), router split (`:213`), bind overrides
- `src/gateway/handlers.rs` — modified: turn lease on v1, cost wiring, readiness payload
- `src/harness.rs` — modified: turn lifecycle (`:440-456`) + approved-tools management
- `src/providers.rs` — modified: `list_all()` (`:334,456`) + `KNOWN_PROVIDERS`
- `src/cli/mod.rs`, `src/cli/repl.rs` — modified: flags; REPL routed through shared lifecycle

**Routes verified present at `546ef04`:** `/v2/chat/{stream,approve,deny,interrupt}` · `/v2/agents{,/switch,/default}` · `/v2/providers` · `/v2/{switch-model,switch-provider,switch}` · `/v2/settings{,/permission}` · `DELETE /v2/settings/approved-tools` · `/health` on the auth-exempt public router.

For uncommitted, unrelated tree state see §1.

---

## 8. Suggested skills for the next agent

Spec §12.2 now assigns these per work package. Summary:

- **`code-review`** — gate on WP-1 (G4/G5/G8) and WP-7 (Tauri supervisor); the concurrency and lock-ordering logic is the risky part and deserves a second pass.
- **`security-review`** — **blocking** gate on WP-2 (**G6**, sandboxed file read). Path traversal, symlink escape, gitignored-secret leakage, and 404-vs-403 information disclosure are all in scope; hardening requirements are in spec G6 and §9.
- **`frontend-design`** or **`example-skills:frontend-design`** — opening move on WP-3 (F1/F5), for the 3-panel layout and visual direction.
- **`example-skills:webapp-testing`** — Playwright driving of the approval dialog; WP-4 exit gate.
- **`commit`** — conventional commit at each package boundary.

Do **not** reach for `prd-generator` — the spec already exists and is reconciled against the code.

---

## 9. Next steps

**The full plan is spec §12** — work packages, model routing, wave schedule, quality gates. Do not re-derive an ordering here; this section records only what is immediately actionable.

**Start here (spec WP-0, blocks everything):**

1. ⚠️ **Human action — unblock the provider.** Add OpenRouter credits or configure a second provider key, then set a tool-capable model. Nothing in §5's "NOT verified" list can be closed without it, and it gates confidence in the whole committed slab.
2. **R1 — route stubs.** Register `/v2/mcp/servers`, `/v2/memory/search`, `/v2/memory/stats`, `/v2/files`, `/v2/files/tree`, `/v2/sessions/{id}/messages` returning `501 not_implemented` in the spec §8 error shape. Independent of item 1, so it can start now — and it is what unlocks parallel dispatch (spec §12.3).
3. **G0 — run the two unverified acceptance checks** from §5 once item 1 lands: the approval round-trip, and REPL-vs-gateway cost parity.

**Then, per spec §12.5, three lanes run in parallel:** WP-1 (G4/G5/G8) · WP-2 (G6 — Opus + `security-review`) · WP-3 (F1/F2/F3/F5).

**Task-specific gotchas** — these stay here because they are what cost investigation time:

- **G4** — mind the **revised §4.7**: HTTP servers are now countable, stdio still are not. The response shape is unchanged either way.
- **G5** — mind §4.8 (`ObsidianVault::search`, not the sidecar) and §4.3 (`std::sync::Mutex`, no `await` while held).
- **G8** — extends the existing `/v1/sessions/{id}` walk to map `FunctionCall`/`FunctionResponse`, pairing by call id.
- **F3** — note **C3**: `EventSource` cannot POST, so use `fetch` + `ReadableStream`. The hand-rolled frame parser is the most likely source of silent breakage; unit-test it against split frames, `\r\n`, multi-line `data:`, comment keep-alives, and unknown event names.
- **F9** — spec **Q9 is now decided**: ship the honest "approve for the rest of this session" copy. Per-call approval is deferred as a harness change. No need to re-open it.

---

## 10. Sensitive information

No secrets are included in this document. Specifically redacted/omitted:

- **API keys and values** — never read or printed; `SecretStore` was only ever queried for *presence* (`available: true/false`).
- **Provider credential inventory** — described only as "only OpenRouter is configured", with no key material, account identifier, or billing detail.
- **Session IDs and local absolute paths** seen during testing are ephemeral and have been omitted from the examples above.
