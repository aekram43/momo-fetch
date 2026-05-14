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
          |
          v
        harness.rs     (central orchestrator)
          |
          +---> providers.rs    (LLM provider management)
          +---> agent/          (agent personalities + orchestrator tools)
          +---> sandbox/        (filesystem isolation)
          +---> context/        (system prompt builder)
          +---> session.rs      (SQLite session persistence)
          +---> memory/         (Obsidian vault engine)
          +---> mcp/            (MCP server management)
          +---> skill/          (skill system)
          +---> team/           (multi-agent coordination)
          +---> cost.rs         (token cost tracking)
          +---> tools/          (agent tool implementations)
```

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

### Step 3: Harness Initialization (`src/harness.rs` — `Harness::build()`)

This is the central construction sequence. Order matters:

1. **Config directory** — ensures `~/.config/momo-fetch/` exists
2. **Memory vault** — `ObsidianVault::open(vault_path)` creates the directory structure if needed
3. **Provider manager** — `ProviderManager::from_env()` auto-detects LLM provider from env vars / OS keychain
4. **Filesystem sandbox** — `FilesystemSandbox::new(project_path, permission_mode)` canonicalizes root, loads `.agentignore`
5. **Context builder** — `ContextBuilder::new(project_path, vault)` walks up directories finding `SOUL.md` / `AGENTS.md` / `CLAUDE.md` / `.harness/SOUL.md` / `.harness/AGENTS.md`
6. **Session manager** — `SessionManager::new(sessions.db)` opens SQLite, runs migrations
7. **Session** — creates a new session or resumes an existing one by ID
8. **MCP service** — `McpService::new()` loads `.harness/mcp.json`, starts configured servers
9. **Skill service** — `SkillService::new()` discovers skills from `.skills/`, `.claude/skills/`, `.harness/skills/`
10. **Team service** — `TeamService::new()` initializes team coordination
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
2. Shows startup info (provider/model, session ID, loaded context, MCP status, skills)
3. Enters `rustyline` loop:
   - Reads input with `<model>>` prompt
   - If slash command (`/help`, `/model`, etc.) → `Command::parse()` + `Command::execute()`
   - If `!` prefix → shell escape (runs directly via `sh -c`)
   - If ` ``` ` detected → multi-line code block input
   - If `\` at end → backslash continuation
   - Otherwise → agent turn

### Step 6: Agent Turn (`repl::run_turn_streaming()`)

1. Resets cost tracker turn counters
2. Calls `harness.run_turn(input)` which:
   - Creates `Content::new("user").with_text(input)`
   - Calls `runner.run_str("default-user", session_id, content)` → returns `EventStream`
3. Consumes the `EventStream` via `tokio::select!`:
   - **Text parts** → `print!()` (streamed token by token)
   - **FunctionCall parts** → yellow `⏺ tool_name(args...)`
   - **FunctionResponse parts** → dimmed `→ result summary`
   - **Tool confirmation** → yellow `! tool_name requires approval`
   - **Errors** → red error message
   - **Ctrl+C** → interrupts generation (first), force quit (second)
   - **Ctrl+D** → graceful shutdown (waits for tool to finish)
4. Records usage metadata for cost tracking
5. Finalizes cost tracking turn, checks budget alerts

### Step 7: Tool Execution

When the LLM calls a tool, adk-rust handles the dispatch:

1. **Confirmation check** — `ToolConfirmationPolicy` determines if user approval is needed
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

Auto-search before turns and auto-write after turns. Two options:

- **Option A** (default, `$0 extra cost`): TF-IDF keyword extraction for writes, grep-based search
- **Option B** (opt-in): When `sidecar_model` is set in `.harness/settings.json`, spawns a sub-agent with a small model for memory extraction

Key struct: `MemorySidecar` — holds `vault: Arc<Mutex<ObsidianVault>>` + `config: MemorySettings`

Key methods:
- `enrich_input(user_input)` — pre-turn search, prepends relevant memories to user input
- `build_system_prompt_addition()` — adds memory vault instructions to system prompt when auto features enabled
- `write_turn_memory_option_a(summary)` — post-turn TF-IDF MemCell write (no extra LLM call)
- `write_turn_memory_option_b(summary)` — post-turn sub-agent MemCell write (uses sidecar model)

Config via `.harness/settings.json` → `memory` object:
- `auto_search: bool` — enable pre-turn vault search
- `auto_write: bool` — enable post-turn MemCell write
- `sidecar_model: Option<String>` — null = Option A (TF-IDF), model name = Option B
- `search_mode`, `max_results_per_turn`, `extract_threshold` — fine-tuning knobs

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
2. **SOUL.md** — `"--- Soul from {path} ---\n{content}"`
3. AGENTS.md / CLAUDE.md — `"--- Context from {path} ---\n{content}"`
4. KMS TOC
5. Skill context (injected by Harness)
6. Memory system context (injected by Memory Sidecar)

### Agent-specific system prompt assembly order:
1. Agent identity ("You are MOMO Fetch operating as **{name}** — {description}...")
2. **SOUL.md** — `"--- Soul from {path} ---\n{content}"`
3. **Agent personality** — `"--- Agent Personality: {name} ---\n{content}"`
4. AGENTS.md / CLAUDE.md — `"--- Context from {path} ---\n{content}"`
5. KMS TOC
6. Skill context (injected by Harness)
7. Memory system context (injected by Memory Sidecar)

---

## 9. Session Persistence (`src/session.rs`)

- Backend: SQLite via `adk_session::SqliteSessionService`
- Location: `~/.config/momo-fetch/sessions.db`
- Auto-migrates schema on startup
- Each REPL session gets a UUID; events are auto-saved by adk-runner
- `/sessions` lists past sessions, `/resume <id>` restores one

---

## 10. Cost Tracking (`src/cost.rs`)

- Records token usage from each LLM response event's `usage_metadata`
- Persists to `~/.config/momo-fetch/cost.json`
- Aggregation: session, today, week, project
- Budget alerts when approaching limits
- `/cost` commands display summaries

---

## 11. MCP Integration (`src/mcp/mod.rs`)

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

## 12. Skill System (`src/skill/mod.rs`)

- Discovery: scans `.skills/`, `.claude/skills/`, `.harness/skills/`
- Uses `adk_skill::SkillIndex` for SKILL.md parsing and `whenToUse` matching
- Auto-match: agent input is lexically matched against skill triggers
- Explicit: `/<skill-name>` invokes directly
- `/skill install <git-url>` clones and installs a skill
- Skill context injected into system prompt on each runner build

---

## 13. Sub-Agent Orchestration (`src/tools/task.rs`)

- `Task` tool delegates subtasks to isolated sub-agents
- Recursion depth tracked via thread-local `TaskContext` (max 3 levels)
- Sub-agents get their own tool registry (no Task tool — prevents recursion)
- Sequential or parallel execution modes
- Configurable timeout per sub-agent (default 5 min)

---

## 14. Agent Teams (`src/team/mod.rs`)

- File-based mailbox for inter-agent messaging (`.harness/mailbox/`)
- Workers spawned as separate `momo-fetch` processes in tmux panes
- Optional git worktrees for isolation
- `WorkerDef` has optional `agent` field — when set, worker spawn command includes `-a <agent_name>` so each worker loads its own personality from `.harness/agents/`
- **Team config files** — `.harness/teams/<name>.yml` defines workers with `TeamConfig` struct (`name`, `workers[]`). Each `TeamWorkerConfig` has `name`, `task`, `agent`, `branch`, `worktree`
- `TeamService::load_team_config(name)` parses YAML from `.harness/teams/`, `TeamService::list_team_configs()` discovers all available configs, `TeamService::config_to_workers()` converts `TeamConfig` → `Vec<WorkerDef>`
- `/team start <name>` — load workers from config file (no interactive input)
- `/team start` (no arg) — interactive mode, also shows available team configs
- `/team status` — check worker progress and mailbox
- `/team merge` — merge completed workers' branches
- `/team stop` — terminate workers, clean up worktrees

---

## 15. Agent Personalities (`src/agent/`)

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

## 16. Configuration (`src/config/mod.rs`)

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

## 17. Key Conventions

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
- Total: 191 tests across all modules
