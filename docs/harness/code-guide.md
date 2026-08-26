# MOMO Fetch — Code Guideline

This document explains how the codebase works, step by step, from startup to tool execution.

---

## 1. Architecture Overview

```
main.rs
  |
  v
cli/mod.rs (clap arg parsing)
  |
  +---> oneshot.rs    (momo-fetch -p "prompt")
  +---> repl.rs       (interactive REPL, default)
  +---> team_cmd.rs   (momo-fetch team ... — headless team control, JSON out)
  +---> routine_cmd.rs (momo-fetch routine ... — scheduled work, JSON out)
  +---> team_worker.rs (momo-fetch --team-worker <name> — standby worker loop)
          |
          v
        harness.rs     (central orchestrator)
          |
          +---> providers.rs    (LLM provider management)
          +---> agent/          (agent personalities + orchestrator tools)
          +---> sandbox/        (filesystem isolation)
          +---> context/        (system prompt builder)
          +---> session.rs      (SQLite session persistence)
          +---> transcript.rs   (reassembles streamed replies into the session)
          +---> memory/         (Obsidian vault engine)
          +---> mcp/            (MCP server management)
          +---> skill/          (skill system)
          +---> team/           (multi-agent coordination)
          +---> cost.rs         (token cost tracking)
          +---> tools/          (agent tool implementations)
```

`routine/` sits deliberately outside that tree: it reads `.harness/routines/`
and spawns fresh `momo-fetch` processes, and touches the harness at no point.
That is what lets a routine fire while a turn is running.

**Key principle:** `harness.rs` owns everything. The CLI layer calls Harness methods. Tools use thread-local context to access the sandbox and vault.

---

## 2. Startup Flow

### Step 1: Entry Point (`src/main.rs`)

1. Initializes tracing via `tracing_subscriber` (controlled by `HARNESS_LOG` env var, defaults to `warn`)
2. Parses CLI args via `clap::Parser`
3. Delegates to `cli::run(args)`

### Step 2: CLI Dispatch (`src/cli/mod.rs`)

1. Loads `.env` file (if present) via `dotenvy::dotenv()`
2. Builds `HarnessConfig` from CLI args (project path, permission mode, vault path, resume session ID)
3. Calls `Harness::build(config)` — the main initialization
4. Applies CLI overrides (`--provider`, `--model`)
5. Branches:
   - `-p "prompt"` → `oneshot::run()` — single prompt, stdout/stderr output, exit
   - No `-p` → `repl::run()` — interactive REPL loop

`team` and `routine` subcommands short-circuit **before** step 2: they touch
only `.harness/` on disk and must not need a provider or an API key. Both flush
stdout and `std::process::exit(code)` themselves, because stdout is their
contract and `exit` skips `Drop`.

### Step 3: Harness Initialization (`src/harness.rs` — `Harness::build()`)

This is the central construction sequence. Order matters:

1. **Config directory** — ensures `~/.config/momo-fetch/` exists
2. **Memory vault** — `ObsidianVault::open(vault_path)` creates the directory structure if needed
3. **Provider manager** — `ProviderManager::from_env()` auto-detects LLM provider from env vars / OS keychain
4. **Filesystem sandbox** — `FilesystemSandbox::new(project_path, permission_mode)` canonicalizes root, loads `.agentignore`
5. **Context builder** — `ContextBuilder::new(project_path, vault)` walks up directories finding `SOUL.md` / `AGENTS.md` / `CLAUDE.md` / `.harness/SOUL.md` / `.harness/AGENTS.md`
6. **Session manager** — `SessionManager::new(sessions.db)` builds the SQLite pool (WAL, 5s busy timeout), wraps it in `RetryingSessionService`, runs migrations
7. **Session** — creates a new session or resumes an existing one by ID
8. **MCP service** — `McpService::new()` loads `.harness/mcp.json`, starts configured servers
9. **Skill service** — `SkillService::new()` discovers skills from `.skills/`, `.claude/skills/`, `.harness/skills/`
10. **Team service** — `TeamService::new()` initializes team coordination (loads `.harness/team.json` if a team is running)
11. **Agent registry** — `AgentRegistry::new(project_path)` scans `.harness/agents/` for `.md` and `.yml` personality files
12. **Cost tracker** — `CostTracker::new()` loads cost data from JSON file
13. **Runner** — `build_runner()` creates the adk-rust `Runner` with:
    - LlmAgent with model, system prompt, tools, tool confirmation policy
    - Agent-specific system prompt if `--agent <name>` was provided
    - Orchestrator tools (`spawn_agent`, `send_message`, `receive_messages`) if agent has `orchestration` capability
    - MCP toolset registration
    - Skill context injection into system prompt
    - Task tool context for sub-agent spawning
    - Orchestrator context for dynamic agent spawning

### Step 4: Runner Construction (`Harness::build_runner()`)

1. Builds tool registry via `tools::build_tool_registry_with_orchestrator(sandbox, vault, is_orchestrator)` — sets thread-local context and returns `Vec<Arc<dyn Tool>>` (includes orchestrator tools if agent has `orchestration` capability)
2. Converts sandbox permission mode to `ToolConfirmationPolicy` (Strict→Always, Auto→PerTool, Yolo→Never)
3. Builds system prompt — if `--agent <name>` provided, uses `context_builder.system_prompt_for_agent(agent_def)` which includes agent personality; otherwise uses `context_builder.system_prompt()` (base instruction + SOUL.md + AGENTS.md + KMS TOC + skill context)
4. Sets task context for sub-agent spawning (ProviderManager clone, sandbox, vault, depth=0)
5. Sets orchestrator context if agent has `orchestration` capability (ProviderManager, sandbox, vault, agent registry, mailbox path, identity)
6. Creates `LlmAgentBuilder` → sets model, instruction, confirmation policy → registers tools
7. Registers MCP toolset (if any servers configured)
8. Creates `Runner::builder()` with agent + session service

---

## 3. REPL Turn Flow

### Step 5: REPL Loop (`src/cli/repl.rs`)

1. Prints MoMo banner (pitbull ASCII art) via `banner::print_banner()`
2. Shows startup info (provider/model, session ID, loaded context, MCP status, skills count excluding convention files)
3. Enters `rustyline` loop:
   - Reads input with `<model>>` prompt
   - If slash command (`/help`, `/model`, etc.) → `Command::parse()` + `Command::execute()`
     - If `Unknown`, tries `try_custom_command()` to resolve `.harness/commands/<name>.md`
     - If custom command found, resolved prompt goes through `run_turn_streaming()` (full LLM pipeline)
     - If not found, falls through to `Unknown` handler ("Unknown command" error)
   - If `!` prefix → shell escape (runs directly via `sh -c`)
   - If ` ``` ` detected → multi-line code block input
   - If `\` at end → backslash continuation
   - Otherwise → agent turn

### Step 6: Agent Turn (`repl::run_turn_streaming()`)

1. Resets cost tracker turn counters
2. Calls `harness.run_turn_enriched(input)` which:
   - Enriches input with relevant memories (if auto_search enabled)
   - Creates `Content::new("user").with_text(enriched_input)`
   - Calls `runner.run_str("default-user", session_id, content)` → returns `EventStream`
   - Wraps that stream in `transcript::recording_replies` (see below) — without it the agent's own text never reaches the session store
3. Consumes the `EventStream` via `consume_stream()`:
   - **Thinking spinner** → `ThinkingSpinner` shows braille animation while waiting for LLM
   - **Text parts** → spinner stops, `print!()` (streamed token by token)
   - **FunctionCall parts** → spinner stops, yellow `⏺ tool_name(args...)`
   - **FunctionResponse parts** → dimmed `→ result summary`, spinner restarts only after tool call responses (LLM thinking again)
   - **Tool confirmation** → spinner stops, interactive `[y/n]` prompt
   - **Errors** → red error message
   - **Ctrl+C** → interrupts generation (first), force quit (second)
   - **Ctrl+D** → graceful shutdown (waits for tool to finish)
4. If tool confirmation was pending:
   - Shows `? Allow <tool> to proceed? [y/n]` prompt
   - On approval: records tool in `Harness.approved_tools`, rebuilds runner with `RunConfig` containing `ToolConfirmationDecision::Approve`, sends follow-up turn
   - On denial: prints denial message
5. Records usage metadata for cost tracking
6. Finalizes cost tracking turn, checks budget alerts

### Step 7: Tool Execution

When the LLM calls a tool, adk-rust handles the dispatch:

1. **Confirmation check** — `ToolConfirmationPolicy` determines if user approval is needed
   - If confirmation required and no decision in `RunConfig.tool_confirmation_decisions`: yields `tool_confirmation` event, stream ends
   - REPL shows interactive `[y/n]` prompt via `prompt_tool_approval()`
   - On approval: tool name added to `Harness.approved_tools` (persists for session), runner rebuilt with `RunConfig` containing the decision
   - Subsequent calls to the same tool skip confirmation (decision baked into `RunConfig`)
2. **Tool function runs** — e.g., `file_read(args)` in `src/tools/file.rs`
3. **Tool accesses thread-local context** — `get_sandbox()` to get the `FilesystemSandbox`
4. **Result returned** to the LLM as `FunctionResponse`
5. LLM continues generating with the tool result

---

## 4. Tool System

### Tool Registry (`src/tools/mod.rs`)

`build_tool_registry_with_orchestrator()` creates all built-in tools. When `is_orchestrator=true`, also includes orchestrator tools:

| Module | Tools | Purpose |
|--------|-------|---------|
| `file.rs` | FileRead, FileWrite, FileEdit | File operations with sandbox scoping |
| `shell.rs` | ShellExec | Shell command execution with timeout |
| `search.rs` | Grep, Glob | File content/pattern search |
| `web.rs` | WebSearch, WebFetch | Web search (Serper/DuckDuckGo) and URL fetch |
| `memory.rs` | MemWrite, MemExtract, MemSearch, MemRead, MemGraph, MemProfile, MemConsolidate, MemValidateForesights, MemReflect, MemStats | Memory vault operations |
| `kms.rs` | KmsRead, KmsSearch, KmsWrite | Knowledge base operations |
| `task.rs` | Task | Sub-agent orchestration |
| `agent/orchestrator.rs` | SpawnAgent, SendMessage, ReceiveMessages | Dynamic agent spawning + mailbox messaging (orchestrator only) |

### Thread-Local Context Pattern

Tools cannot receive sandbox/vault via function parameters (adk-rust `#[tool]` macro limitation). Instead:

```rust
// Each tool module has:
thread_local! {
    static SANDBOX_CTX: RefCell<Option<Arc<FilesystemSandbox>>> = RefCell::new(None);
}

pub fn set_sandbox(sandbox: Arc<FilesystemSandbox>) { ... }
fn get_sandbox() -> Result<Arc<FilesystemSandbox>, AdkError> { ... }
```

- `build_tool_registry()` calls `set_sandbox()` / `set_vault()` for each module
- Tool functions call `get_sandbox()` / `get_vault()` to access shared state
- This is safe because tools execute synchronously within the same thread

### File Tools (`src/tools/file.rs`)

- **FileRead**: Resolves path via sandbox, reads file, formats with line numbers, supports line range
- **FileWrite**: Resolves path, creates parent dirs, atomic write (temp file → rename)
- **FileEdit**: Reads file, checks uniqueness of `old_string`, replaces, atomic write

### Shell Tool (`src/tools/shell.rs`)

- Runs `sh -c <command>` in project directory
- Default timeout: 120s
- Destructive command detection via `sandbox.check_destructive()`

### Memory Tools (`src/tools/memory.rs`)

- `set_vault()` / `get_vault()` thread-local pattern (same as sandbox)
- All tools delegate to `ObsidianVault` methods
- Vault access is synchronized via `Arc<Mutex<ObsidianVault>>` in Harness

---

## 5. Memory Vault Engine

### Vault Structure (`src/memory/vault.rs`)

```
memory-vault/
├── .vault-config.json     (counters, stats, settings)
├── index.md               (auto-generated table of contents)
├── 1-memcells/YYYY/MM/YYYY-MM-DD.md
├── 2-events/fact-NNNN.md
├── 3-foresights/pred-NNNN.md
├── 4-episodes/ep-NNNN.md
├── 5-profile/agent-profile.md
├── 6-reflections/weekly/  and  monthly/
├── clusters/
└── templates/
```

### Note Format (Obsidian-compatible)

Every note has YAML frontmatter + markdown body with `[[wikilinks]]`:

```yaml
---
type: memcell
date: 2026-05-07
project: my-project
tags: [memcell, rust, testing]
---
## MemCell 001 — 14:32
**Topic**: ...
**Context**: ...
**Actions**: ...
**Outcome**: ...
```

### Lifecycle (`src/memory/lifecycle.rs`)

1. **Write** — `mem_write` appends a MemCell to the daily log
2. **Extract** — `mem_extract` processes MemCells into Events, Foresights, Episodes
3. **Consolidate** — `mem_consolidate` clusters related MemCells (Jaccard similarity > 0.65)
4. **Reflect** — `mem_reflect` generates weekly/monthly reflections
5. **Validate** — `mem_validate_foresights` checks pending predictions

### Memory Sidecar (`src/memory/sidecar.rs`)

Auto-search before turns and auto-write after turns. Three options, selected via config:

- **Option A** (default, `$0 extra cost`): TF-IDF keyword extraction for writes, grep-based search
- **Option B** (opt-in): When `sidecar_model` is set, direct LLM call for memory extraction with fallback to Option A
- **Option C** (opt-in): Separate process via Mailbox IPC, auto-detected via `ready` signal

Key struct: `MemorySidecar` — holds `vault: Arc<Mutex<ObsidianVault>>` + `config: MemorySettings` + `sidecar_llm: OnceLock<Arc<dyn Llm>>`

Key methods:
- `write_turn_memory(summary, provider_mgr, mailbox_path)` — unified dispatch: Option C → B → A
- `write_turn_memory_option_a(summary)` — post-turn TF-IDF MemCell write (no extra LLM call)
- `write_turn_memory_option_b(summary, provider_mgr)` — direct LLM call, parse JSON, fallback to A
- `write_turn_memory_option_c(summary, mailbox)` — Mailbox IPC, fallback to A on timeout
- `check_thresholds(memcell_ref, turn)` — auto-extract + auto-consolidate at configurable thresholds
- `enrich_input(user_input)` — pre-turn search, prepends relevant memories to user input
- `build_system_prompt_addition()` — adds memory vault instructions to system prompt when auto features enabled

Config via `.harness/settings.json` → `memory` object:
- `auto_search: bool` — enable pre-turn vault search
- `auto_write: bool` — enable post-turn MemCell write
- `sidecar_model: Option<String>` — null = Option A (TF-IDF), model name = Option B
- `sidecar_provider: Option<String>` — provider for the sidecar model
- `search_mode`, `max_results_per_turn`, `extract_threshold` — fine-tuning knobs
- `consolidate_threshold: usize` — auto-consolidate every N MemCells (default: 30, 0 = disabled)

Also: `src/providers.rs` — `create_model()` is public, used by sidecar to construct LLM instances.

### Retrieval Modes (`src/memory/retrieval.rs`)

Four search strategies:

1. **grep_llm** (default): grep for keyword candidates → LLM ranks relevance → top N
2. **graph_walk**: follow `[[wikilinks]]` from a seed note, expanding N levels
3. **tag_filter**: filter by YAML frontmatter tags
4. **agentic**: multi-round grep → LLM sufficiency check → refined queries → merge

---

## 6. Provider System (`src/providers.rs`)

### Provider Detection Priority

`ProviderManager::from_env()` checks in order:
1. `ANTHROPIC_API_KEY` (env or keychain) → Anthropic Claude
2. `OPENAI_API_KEY` → OpenAI GPT
3. `DEEPSEEK_API_KEY` → DeepSeek
4. `GROQ_API_KEY` → Groq
5. `OPENROUTER_API_KEY` → OpenRouter
6. `ZAI_API_KEY` → z.ai (OpenAI-compatible, base URL configurable via `ZAI_LLM_URL`)
7. `LLM_URL` + `LLM_APIKEY` → custom (any OpenAI-compatible endpoint, model via `LLM_MODEL`)
8. Fallback → Ollama (localhost, no key needed)

### Hot-Swap

`switch()` / `switch_model()` / `switch_provider()` creates a new LLM instance and calls `rebuild_runner()` to recreate the agent + runner with the new model.

### Secret Store (`src/config/secrets.rs`)

- Primary: OS keychain via `keyring-core` (macOS Keychain, Linux Secret Service, Windows Credential Manager)
- Fallback: environment variables (e.g., `ANTHROPIC_API_KEY`)
- `/key set <provider>` stores interactively, `/key list` shows masked keys

---

## 7. Filesystem Sandbox (`src/sandbox/mod.rs`)

### Path Resolution (`resolve_path()`)

1. Clean path via `path_clean` (resolves `.` and `..`)
2. Reject paths starting with `..`
3. Join with canonicalized root
4. Canonicalize resolved path (resolves symlinks)
5. Verify `resolved.starts_with(root)` — blocks path traversal

### .agentignore

- Uses `ignore::gitignore::GitignoreBuilder` (same engine as ripgrep)
- Loaded from `<project_root>/.agentignore`
- Supports glob patterns, negation (`!important.log`), comments

### Permission Modes

| Mode | Behavior | ToolConfirmationPolicy |
|------|----------|----------------------|
| Strict | All mutating tools require approval | Always |
| Auto | Only destructive commands require approval | PerTool (mutating tools) |
| Yolo | Auto-approve everything | Never |

### Destructive Detection

Regex-based pattern matching for: `rm -rf /`, `git push --force`, `git reset --hard`, `DROP TABLE`, `DELETE FROM`, `mkfs`, `dd if=`, `chmod 777 /`

---

## 8. Context Injection (`src/context/mod.rs`)

`ContextBuilder` walks up from the project directory to the filesystem root, discovering:

- `SOUL.md` — agent personality/identity (highest behavioral priority)
- `AGENTS.md` — project instructions
- `CLAUDE.md` — Claude-specific instructions
- `.harness/SOUL.md` — harness-specific personality overrides
- `.harness/AGENTS.md` — harness-specific instruction overrides

Files closer to the project root have higher priority (appear last in the system prompt).

Two system prompt methods:

- `system_prompt()` — default prompt for normal mode
- `system_prompt_for_agent(agent_def)` — agent-specific prompt when `--agent <name>` is used

### Default mode system prompt assembly order:
1. Base instruction ("You are MOMO Fetch...")
2. **Work outside this turn** — `delegation_brief()`: the `momo-fetch team …` /
   `routine …` commands, the configs and routine names this project has, and the
   line that `task(...)` is not a team
3. **SOUL.md** — `"--- Soul from {path} ---\n{content}"`
4. AGENTS.md / CLAUDE.md — `"--- Context from {path} ---\n{content}"`
5. KMS TOC
6. Skill context (injected by Harness)
7. Memory system context (injected by Memory Sidecar)

### Agent-specific system prompt assembly order:
1. Agent identity ("You are MOMO Fetch operating as **{name}** — {description}...")
2. **Work outside this turn** — as above
3. **SOUL.md** — `"--- Soul from {path} ---\n{content}"`
4. **Agent personality** — `"--- Agent Personality: {name} ---\n{content}"`
5. AGENTS.md / CLAUDE.md — `"--- Context from {path} ---\n{content}"`
6. KMS TOC
7. Skill context (injected by Harness)
8. Memory system context (injected by Memory Sidecar)

The names in the brief are read when the session starts, so a config added
afterwards is not in it — the text says `list` is the live answer, and the agent
runs it. Adding the brief cost every turn a few hundred tokens and bought the
one thing the agent could not do before: know that teams and routines exist.

---

## 9. Session Persistence (`src/session.rs`)

- Backend: SQLite via `adk_session::SqliteSessionService`
- Location: `~/.config/momo-fetch/sessions.db`
- Auto-migrates schema on startup

### Concurrency: one database, many processes

Every momo-fetch process on the machine writes this one file, and a team starts
several at once. Two things had to be added before that worked *(both fixed
2026-08-25)*.

**The pool is built here, not by `SqliteSessionService::new`.** That
constructor calls `SqlitePool::connect` with defaults — rollback journal, **no
busy timeout** — so the second writer to arrive did not wait, it failed
instantly with `database is locked (code: 5)` and took the worker with it. We
build `SqliteConnectOptions` ourselves (WAL, `busy_timeout(5s)`,
`create_if_missing`, `foreign_keys`) and hand the pool to
`SqliteSessionService::from_pool`. This is why `sqlx` is a direct dependency,
pinned to the same 0.8 line adk-session uses — a second semver-incompatible
sqlx would mean a second `SqlitePool` type, and `from_pool` would not accept
ours.

**`RetryingSessionService` wraps the service.** WAL and a busy timeout only help
a transaction that starts as a writer. SQLite refuses *immediately*, timeout or
not, when a transaction that began as a reader tries to upgrade to a write —
deliberately, since two readers both waiting to upgrade would deadlock. Every
adk-session write is that shape (`BEGIN`, read the existing state, insert), so
concurrent agents still lost one. The wrapper retries the whole call up to
`RETRY_ATTEMPTS` times with jittered backoff; the failed transaction is already
rolled back when the error surfaces, so the retry starts clean. `is_locked()`
decides what is retryable — anything else surfaces on the first attempt.
### Origin: what started a session

One consequence of that shared database: the session list holds team workers and
scheduled runs beside the conversations, and they were indistinguishable — a
column of hex ids that all looked like chats nobody remembered having.

`SessionOrigin` (`Chat` | `Agent` | `Worker` | `Routine`) is stamped into session
state under `momo.origin` **at creation**, not at the first turn — a worker that
dies before it says anything still has to be identifiable. `Chat` writes
nothing, so an ordinary chat and a session older than origins look alike, which
is correct. It comes from the flags, narrowest first: `--team-worker` beats
`--origin` (set by the routine spawner) beats `-a`, because a worker is launched
with `-a` too and would otherwise be filed as "a specialist someone started".
`switch_agent` updates it for later sessions unless the process is already
something narrower.

`/v1/sessions` returns it; the sidebar marks each kind with its own glyph
(`web/src/lib/session-origin.ts`), never colour alone.

- Each REPL session gets a UUID; events are auto-saved by adk-runner
- `/sessions` lists past sessions, `/resume <id>` restores one
- `/clear` deletes current session and creates a fresh one
- `/compact` summarizes events into a new session (delete old + create new + append summary event)

### Reply persistence (`src/transcript.rs`)

"Events are auto-saved by adk-runner" is true but incomplete, and the gap cost
a real bug *(fixed 2026-08-19)*. In streaming mode — `RunConfig::default()` is
`StreamingMode::SSE` — adk emits one event per model chunk, all sharing an
event id, each carrying only its delta. The assembled text lives in an
accumulator that `adk-agent` turns into an event **only in the non-streaming
branch**. The runner persists only non-partial events, and the sole non-partial
event a text reply produces is the provider's terminal chunk: finish reason,
usage, `content: null`.

So tool calls were stored (they arrive non-partial) and every word the agent
said was streamed to the client and dropped. Worse than a blank transcript:
`Runner::run` reloads the session from the store at the top of **every** turn,
so the model was handed a history in which it had never spoken.

`transcript::recording_replies` wraps the turn's `EventStream`, reassembles the
deltas and writes one event per LLM call. Applied in all three entry points
(`run_turn`, `run_turn_enriched`, `run_confirmation_turn`), so the REPL and the
gateway both get it. Two things about it are load-bearing:

- **It writes at the terminal event, before yielding it.** Consumers stop
  polling at `Event::is_final_response()` — the gateway's SSE handler `return`s
  there — which drops the wrapper where it stands. A write deferred to the end
  of the loop never runs.
- **The event is stamped with the first chunk's time, not the flush time.**
  Events are read back `ORDER BY timestamp`, and a reply that preceded a tool
  call can only be flushed after it.

Ids are `{streamed_event_id}_text_{n}`; events are inserted, not upserted, and
one LLM call can speak twice around a tool call.

---

## 10. Custom Slash Commands (`src/cli/repl.rs`)

Users define custom slash commands by placing `.md` files in `.harness/commands/`.

### Resolution Flow

Custom commands are resolved in the REPL loop (`repl.rs`) **before** `Command::execute()`:

1. User types `/<name> [args...]`
2. `Command::parse()` returns `Command::Unknown(input)`
3. `try_custom_command(input, working_dir)` checks `.harness/commands/<name>.md`
4. If file exists: reads content, replaces `$ARG` with trailing text, sends through `run_turn_streaming()`
5. If not found: falls through to `Unknown` handler (prints error)

Key functions in `repl.rs`:

- `try_custom_command(input, working_dir) -> Option<String>` — resolves a custom command to a prompt. Skips built-in names via `BUILTIN_COMMANDS` constant. Validates name (alphanumeric + dash/underscore only). Returns `None` if no matching file.
- `list_custom_commands(working_dir) -> Vec<(String, String)>` — scans `.harness/commands/*.md`, returns sorted (name, first-line-description) tuples. Used by `/help` to display custom commands.

### Why not a Command enum variant?

`Command::execute()` returns `Result<bool>` and does not have access to `shutting_down`/`turn_active` flags needed by `run_turn_streaming()`. Intercepting in the REPL loop lets custom commands go through the full streaming pipeline (memory enrichment, cost tracking, tool confirmations, auto-memory write) without duplicating logic.

### Constraints

- Custom commands cannot override built-in commands (`BUILTIN_COMMANDS` bypass)
- Name validation prevents path traversal (only `[a-zA-Z0-9_-]` allowed)
- Commands are discovered at invocation time — no restart needed
- `$ARG` is replaced with empty string when no argument is provided

---

## 11. Cost Tracking (`src/cost.rs`)

- Records token usage from each LLM response event's `usage_metadata`
- Persists to `~/.config/momo-fetch/cost.json`
- Aggregation: session, today, week, project
- Budget alerts when approaching limits
- `/cost` commands display summaries

---

## 12. MCP Integration (`src/mcp/mod.rs`)

### Dual Transport Support

MCP supports two transport types, both configured in `.harness/mcp.json`:

| Transport | Config fields | Mechanism |
|-----------|--------------|-----------|
| **stdio** | `command`, `args`, `env` | Spawns a local child process |
| **HTTP/SSE** | `type: "http"`, `url`, `headers` | Connects to a remote HTTP server |

### Config Parsing

`parse_mcp_config()` deserializes `mcp.json` as `serde_json::Value`, then separates servers by type:

```rust
// Stdio servers have a "command" field
// HTTP/SSE servers have a "type" field ("http" or "sse") + "url" field
```

Two config structs:

- `McpServerConfig` — stdio servers (command + args), used by `McpServerManager`
- `HttpMcpServerConfig` — HTTP/SSE servers (url + headers), used by `McpHttpClientBuilder`

### McpService Architecture

`McpService` manages both transport types:

| Component | Purpose |
|-----------|---------|
| `configs` | Stdio server configs (managed by `McpServerManager`) |
| `http_configs` | HTTP/SSE server configs |
| `http_toolsets` | Connected HTTP server toolsets (`Vec<Arc<dyn Toolset>>`) |

Key methods:

- `new()` — loads config, parses into stdio/HTTP groups, starts stdio servers
- `connect_http_servers()` — connects all HTTP servers via `McpHttpClientBuilder`
- `toolset()` — returns `MergedToolset` combining stdio + HTTP toolsets
- `has_http_servers()` — checks if any HTTP servers are configured

### HTTP Connection Flow

1. `McpHttpClientBuilder::new(url)` creates a builder
2. Auth token extracted from `headers.Authorization` (strips `"Bearer "` prefix)
3. `builder.auth(McpAuth::bearer(token))` sets authentication
4. `builder.connect()` establishes the MCP session (uses `StreamableHttpClientTransport` from rmcp)
5. Resulting `Toolset` is wrapped in `Arc` and stored in `http_toolsets`

### Merged Toolset

When both stdio and HTTP servers are present, `McpService::toolset()` combines them:

```rust
// MergedToolset from adk_tool merges multiple toolsets into one
// Tools are prefixed with mcp_ namespace automatically
let merged = MergedToolset::new(vec![stdio_toolset, http_toolset1, http_toolset2, ...]);
```

### Patched rmcp

Some MCP servers return HTTP 200 with no Content-Type for notifications. The stock rmcp crate treats this as an error (`UnexpectedContentType(None)`). A local patch in `patches/rmcp-1.6.0/` adds:

```rust
// Handle 200 OK with no body / no content-type
if status == reqwest::StatusCode::OK && content_type.is_none() {
    return Ok(StreamableHttpPostResponse::Accepted);
}
```

Patched via `[patch.crates-io]` in `Cargo.toml`:

```toml
[patch.crates-io]
rmcp = { path = "patches/rmcp-1.6.0" }
```

### MCP Connectivity Testing

`--test-mcp` CLI flag runs `run_test_mcp()` in `src/cli/mod.rs`:

1. Creates a `TestCtx` struct implementing `ReadonlyContext` (required for tool listing)
2. Connects all configured stdio and HTTP servers
3. Lists available tools from each server
4. Prints a summary table with connection status and total tool count
5. Exits with code 0 on success, 1 on any failure

### REPL Commands

- `/mcp add`, `/mcp list`, `/mcp remove` — manage MCP servers
- Graceful shutdown on exit

---

## 13. Skill System (`src/skill/mod.rs`)

- Discovery: scans `.skills/`, `.claude/skills/`, `.harness/skills/`
- Uses `adk_skill::SkillIndex` for SKILL.md parsing and `whenToUse` matching
- Auto-match: agent input is lexically matched against skill triggers
- Explicit: `/<skill-name>` invokes directly
- `/skill install <git-url>` clones and installs a skill
- Skill context injected into system prompt on each runner build

---

## 14. Sub-Agent Orchestration (`src/tools/task.rs`)

- `Task` tool delegates subtasks to isolated sub-agents
- Recursion depth tracked via thread-local `TaskContext` (max 3 levels)
- Sub-agents get their own tool registry (no Task tool — prevents recursion)
- Sequential or parallel execution modes
- Configurable timeout per sub-agent (default 5 min)

---

## 15. Agent Teams (`src/team/mod.rs`)

- File-based mailbox for inter-agent messaging (`.harness/mailbox/`). Message
  filenames carry a uuid suffix: the timestamp is only milliseconds, and two
  messages from the same sender to the same recipient in the same millisecond
  used to overwrite each other *(fixed 2026-08-25)*
- Workers spawned as separate `momo-fetch` processes in tmux panes
- Optional git worktrees for isolation
- **Team config files** — `.harness/teams/<name>.{json,yml,yaml}` defines workers
  with `TeamConfig` (`name`, `workers[]`). Each `TeamWorkerConfig` has `name`,
  `task`, `agent`, `branch`, `worktree`, `permission`, `mode`
- `TeamService::load_team_config(name)` tries all three extensions in
  `TEAM_CONFIG_EXTENSIONS` order and parses through the YAML reader (YAML 1.2 is
  a superset of JSON), `list_team_configs()` discovers all of them,
  `config_to_workers()` converts `TeamConfig` → `Vec<WorkerDef>`. The REPL and
  the `team` subcommand share this loader so they cannot disagree

### The worker command (`build_worker_command`)

One function builds the shell line, from a `WorkerLaunch`, and nothing in it is
left implicit:

```sh
cd '<work_dir>' && { '<binary>' [-a '<agent>'] [--team-worker '<name>'] \
    --permission <mode> --mailbox '<lead>/.harness/mailbox' -p '<task>'
  echo $? > '<lead>/.harness/worker-<name>.exit'
} 2>&1 | tee -a '<lead>/.harness/worker-<name>.log'
```

- **`--permission`** defaults to `DEFAULT_WORKER_PERMISSION` (`auto`), never
  `strict`: a headless pane cannot answer a confirmation prompt, so a strict
  worker stops at its first mutating tool forever. Values parse through
  `PermissionMode` *before* any tmux session or worktree exists, so a typo fails
  the whole start and only the three canonical words reach the shell
- **`--mailbox`** points at the lead's mailbox — a worker in a worktree would
  otherwise resolve `.harness/mailbox` inside its own checkout
- **`echo $?`** inside the braces (so it is momo-fetch's status, not `tee`'s) is
  the only thing that can tell a finished worker from a dead one: the tmux
  window survives either way
- Logs, exit files and heartbeats live in the **lead's** `.harness/`, which is
  the one directory `team status` reads. The log is appended, so a restart keeps
  the crash that prompted it

### Worker modes (`WorkerMode`)

`oneshot` (default) runs the task and exits — what a worker has always been.
`standby` adds `--team-worker <name>`, which routes the process into
`cli/team_worker.rs`: it runs the opening task, then polls its inbox every 2s,
runs a turn per message, replies to whoever asked, and touches
`worker-<name>.heartbeat` each poll. It exits on a `shutdown` message.

A standby worker reporting `completed` means *that task* is done, so
`TeamService::status()` maps it back to `Running` rather than `Completed` — which
is why a team containing one never completes on its own.

### Liveness (`refresh_liveness`, `observed_status`)

Worker status used to move for exactly one reason: a message addressed to
`lead`. Nothing in a default run sends one, so a worker that died in its first
second sat at `starting` forever. `status()` now also reads the evidence the
worker could not send:

| Source | Result |
|--------|--------|
| `worker-<name>.exit` contains `0` | `Completed` |
| non-zero, and the worker never reported | `FailedToStart` (message names the log) |
| non-zero, after it had reported | `Crashed` |
| pane missing **while the session is alive** | `Crashed` |
| unparseable/absent exit file | nothing — it is still working |

The session check matters: without it, a machine with no tmux would report every
worker crashed. `WorkerStatus::is_terminal()` drives "is the team finished".

### Commands

REPL: `/team start [name]`, `/team status`, `/team merge`, `/team stop`.

Headless (`src/cli/team_cmd.rs`, JSON on stdout, exit `0`/`1`/`2`):
`team list`, `team start <name>`, `team status`, `team send <worker> <msg>`,
`team restart <worker>`, `team stop [--force]`. `send` refuses a one-shot worker
instead of queueing a message nothing will read; `restart` relaunches one worker
in a fresh pane and returns a team that had been written off as `completed` to
`running`. There is no headless `merge` — merging has conflicts to resolve.

Gateway (`src/gateway/team.rs`): `POST /v2/team/start|stop|restart` call the
*same* functions in `team_cmd` and translate the exit code into a status — `2`
into 409, a missing config into 404, anything else into 400. Three front-ends,
one implementation; a second one would disagree the first time either changed.

The agent reaches these the way the CLI intends — `shell_exec` — and knows they
exist because `ContextBuilder::delegation_brief` puts them in the system prompt
along with the names in `.harness/teams/` and `.harness/routines/`. Before that,
asked to start a squad it reached for `task(...)`, the only delegation it had
been told about, and reported the squad as started.

---

## 16. Routines (`src/routine/`)

Recurring work: a task template, a trigger, and an assignee. Five files, and the
split between them is the design.

### `mod.rs` — the store, and one pure function

`decide(routine, runtime, now_ms, active) -> (Decision, RoutineRuntime)` holds
**every** scheduling rule: catch-up, concurrency, queue depth, the anchor. It
takes no clock, no filesystem and no process, and returns the runtime it
implies — the caller persists that only if it acted on the decision. That is why
the scheduler is covered by ordinary unit tests on a machine with no tmux, no
API key and no wall-clock dependency.

`RoutineService` is the store around it: `.harness/routines/<id>.json` per
definition, `state.json` for the anchors and queues, `runs.json` for the last
`MAX_RUN_HISTORY` firings. Definitions and schedule state are separate files on
purpose — editing a routine in the UI must not rewrite its history, and a
hand-edited definition must not be able to corrupt the schedule. Writes go
through temp-file-then-rename, because the gateway ticks while the UI reads.

A definition that will not parse is **skipped with a warning**, not fatal. One
bad file must not take every other routine down with it.

### `cron.rs` — five fields, in a zone

`min hour dom month dow`, bitmask per field, and `next_after` skips by the
largest unit that cannot match rather than walking minutes. Two rules that are
easy to get wrong:

- **`dom` and `dow` are OR'd when both are restricted** — `0 0 1 * mon` is "the
  1st *or* any Monday". That is what crontab does.
- **The next occurrence is computed in the routine's own IANA zone, then mapped
  back to UTC.** A spring-forward gap returns no instant, so the search moves to
  the next match rather than inventing one; an autumn ambiguity takes the first
  pass through the repeated hour.

Search is bounded at five years: `0 0 30 2 *` matches nothing, ever.

### `exec.rs` — how a run actually starts

The same exit-code contract team workers use, for the same reason: a pid that is
gone tells you the process left, not whether it meant to.

```sh
# .harness/routines/runs/<run_id>.sh
cd '<project>' || exit 127
'<binary>' [-a '<agent>'] --permission auto -p '<prompt>'
echo $? > '<run_id>.exit'
```

The script is written to a file and `sh` is handed its **path**, so nothing
about the prompt is ever nested in a second layer of shell quoting. It is then
launched as `nohup sh '<script>' > '<log>' 2>&1 & echo $! > '<pid>'`: the
immediate `sh` exits at once and is reaped, and the run is orphaned with its pid
recorded. A gateway ticking every 30 s must never accumulate unreaped children.

`--permission` defaults to `auto` (`DEFAULT_ROUTINE_PERMISSION`), never
`strict`: nobody is at the keyboard of a detached run.

### Dispatch has two shapes

| Assignee | What happens | Terminal state |
|---|---|---|
| `worker:<name>` | `TeamService::send_to_worker` — a mailbox message | `delivered` |
| `lead` / `agent:<name>` | `exec::spawn` — its own process | `running` → `succeeded`/`failed` |

`delivered` is not a euphemism for success: once a message is in a standby
worker's inbox, nothing here can observe the outcome, so nothing claims to. It
goes through `TeamService` rather than writing the file directly, because "is
this worker standby, and does it exist" is decided there and two answers to that
question would eventually disagree.

### `view.rs` — one set of JSON shapes

`momo-fetch routine …` and `/v2/routines` both render through it, so the CLI an
agent drives through `shell_exec` and the panel a human clicks in cannot
describe the same routine differently.

### Who ticks

`gateway::spawn_routine_scheduler` runs `RoutineService::tick` every
`routines.tick_seconds` (default 30). Three properties it deliberately has:

- The service is **reopened each tick**, so a routine created by the CLI is
  picked up without a restart, and no harness state is held.
- The tick runs under `spawn_blocking` — it reads directories, writes JSON and
  spawns processes, none of which belongs on the reactor streaming a turn.
- A failed tick is logged and the next one still happens. A scheduler that gives
  up the first time a file is unreadable is worse than none, because it looks
  like one.

`momo-fetch routine tick` is the same step from the shell, for machines with no
gateway running.

### Surfacing it: `/v2/activity`

`gateway/activity.rs` answers "what is working in the background" for the UI's
right panel and its Squad rail — team workers, routine runs, and the team
configs that exist but are not running, in one document. It refreshes worker
liveness and reconciles finished runs on the way through, so it reports the
present rather than the last tick's memory of it, and it renders through
`team_cmd::team_payload` and `routine::view::run_payload` so it cannot disagree
with `momo-fetch team status` or `momo-fetch routine runs`.

It is polled, not pushed: nothing about a tmux pane or a scheduler tick is
caused by the tab asking, so there is no event to subscribe to. Like
`/v2/routines` it takes no harness lock, which is what stops a 10 s poll from
queueing behind a streaming turn.

### In the REPL

`/routine list|show|run|enable|disable` (`cli/commands.rs`). Read-only plus
`run` on purpose — `add` is a fourteen-field form, and a mistyped cron typed as
one REPL line would be saved without ever showing the field names. The name is
the rest of the line, not the next word: every routine anyone writes has a space
in it.

---

## 17. Agent Personalities (`src/agent/`)

### Agent Module Structure

```
src/agent/
├── mod.rs            — AgentDef, AgentRegistry (personality loading)
└── orchestrator.rs   — spawn_agent, send_message, receive_messages tools
```

### AgentDef

Each agent personality has:
- `name` — derived from filename (e.g., `researcher.md` → `researcher`)
- `description` — from `.yml` config (optional)
- `personality` — the markdown prompt content
- `model` / `provider` — optional overrides
- `tools` — optional restricted tool set
- `capabilities` — tags for orchestrator reference (e.g., `["orchestration"]`)

### AgentRegistry

Scans `.harness/agents/` for `.md` and `.yml` files:
- `.md` files: personality only (default model/tools)
- `.yml` files: personality + configuration (takes priority over `.md` for same name)

### Mid-Session Switching

`Harness::switch_agent(name)` and `Harness::clear_agent()` allow switching personalities without restarting:
1. Resolves agent from registry
2. Updates `config.agent_name`
3. Rebuilds runner with new system prompt via `rebuild_runner_with_agent()`
4. Updates task context with new system prompt
5. Sets or clears orchestrator context based on agent capabilities

### Orchestrator Tools (`src/agent/orchestrator.rs`)

Three tools registered only when agent has `orchestration` capability:

| Tool | Purpose | Modes |
|------|---------|-------|
| `spawn_agent` | Dynamically spawn a specialist | `inline` (in-process, synchronous) or `process` (separate OS process, async) |
| `send_message` | Send mailbox message to spawned agent | Any |
| `receive_messages` | Receive mailbox messages from spawned agents | Any |

Thread-local `OrchestratorContext` (same pattern as `TaskContext`) provides access to ProviderManager, sandbox, vault, agent registry, and mailbox path.

- File-based mailbox for inter-agent messaging (`.harness/mailbox/`)
- Workers spawned as separate `momo-fetch` processes in tmux panes
- Optional git worktrees for isolation
---

## 18. Configuration (`src/config/mod.rs`)

### Settings Hierarchy

1. CLI flags (`--model`, `--provider`, `--permission`) — highest priority
2. Project-level `.harness/settings.json`
3. Global `~/.config/momo-fetch/settings.json`
4. Environment variables
5. Defaults

### Key Config Fields

```rust
pub struct HarnessConfig {
    pub project_path: PathBuf,       // Working directory
    pub vault_path: PathBuf,         // Memory vault location
    pub session_db_path: PathBuf,    // SQLite DB path
    pub permission_mode: PermissionMode,
    pub provider: ProviderSettings,
    pub resume_session_id: Option<String>,
    pub agent_name: Option<String>,  // Agent personality name (from --agent flag)
}
```

---

## 19. Key Conventions

### Error Handling

- Tools return `Result<Value, AdkError>` — errors become tool responses for the LLM
- Harness/vault/sandbox return `anyhow::Result<T>` — logged but don't crash
- REPL displays user-friendly errors, never panics
- One-shot mode writes errors to stderr, exit code 1

### Atomic Writes

All file operations (tools + vault) use temp-file-then-rename:
```rust
let tmp = path.with_file_name("name.harness-tmp.ext");
tokio::fs::write(&tmp, &content).await?;
tokio::fs::rename(&tmp, &path).await?;
```

### Synchronous Vault Methods

`ObsidianVault` methods are synchronous (not async). The vault is wrapped in `Arc<Mutex<ObsidianVault>>` in Harness, and tools lock the mutex before calling vault methods.

### Testing Pattern

- Unit tests in each module's `#[cfg(test)] mod tests`
- Thread-local context set in test setup:
  ```rust
  let sandbox = Arc::new(FilesystemSandbox::new(tmp.path(), PermissionMode::Auto)?);
  set_sandbox(sandbox.clone());
  // ... run tool ...
  clear_sandbox();
  ```
- Session tests use `InMemorySessionService`
- Total: 465 tests across all modules

### UTF-8 Safe String Truncation

All string truncation for display uses `ceil_char_boundary()` to avoid panics with multibyte characters (Thai, CJK, emoji):

```rust
// NEVER do this (panics on multibyte UTF-8):
&summary[..200]

// Always use this:
let end = summary.ceil_char_boundary(200);
&summary[..end]
```

Applied in: `repl.rs`, `oneshot.rs`, `vault.rs`.

### Thinking Spinner

`ThinkingSpinner` in `repl.rs` shows a braille animation while the LLM is processing:
- Started when `consume_stream` begins and after tool call responses (LLM thinking again)
- Stopped when text/function call events arrive
- `stop()` is synchronous — uses `tokio::task::block_in_place` to ensure the spinner clears its line before any new output
- Runs on a `tokio::spawn` task, communicates via `AtomicBool` flag
- Restarts only after tool call responses (`FunctionCall` events), not on text parts
- Cycles through labels: Thinking → Analyzing → Processing → Generating

### Tool Confirmation Flow

When `ToolConfirmationPolicy` requires approval:

1. adk-agent yields `tool_confirmation` event, stream ends
2. `consume_stream` returns `StreamResult` with `pending_confirmation`
3. `run_turn_streaming` calls `prompt_tool_approval()` → interactive `[y/n]` prompt
4. On approval: `harness.run_confirmation_turn()` adds tool to `approved_tools` set, rebuilds runner with `RunConfig` containing `ToolConfirmationDecision::Approve`, sends follow-up turn
5. Subsequent calls to same tool skip confirmation (decision in `RunConfig`)

`approved_tools` is `Arc<Mutex<HashSet<String>>>` in Harness, passed through `build_runner()` to `RunConfig.tool_confirmation_decisions`.

### StreamResult

`consume_stream` returns `StreamResult` struct (not `()`):

```rust
struct StreamResult {
    has_output: bool,
    tool_calls: Vec<String>,
    response_parts: Vec<String>,
    pending_confirmation: Option<(String, String)>, // (tool_name, function_call_id)
}
```

This allows `run_turn_streaming` to handle tool confirmations interactively after the stream ends.
