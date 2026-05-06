# PRD: Agent Harness — Rust-Native AI Agent Workspace

## Introduction

Agent Harness เป็น AI agent workspace ที่ทำงาน locally บนเครื่องผู้ใช้ สร้างด้วย Rust บนฐาน **adk-rust v0.6.0** (zavora-ai/adk-rust) โดยมี **EverMemOS-compatible memory vault** (Obsidian wiki) เป็นระบบความจำแบบ 6-level hierarchy

เป้าหมาย: สร้างเครื่องมือใช้เองที่ทำงานได้จริง พร้อมเรียนรู้ agent architecture อย่างลึกซึ้ง

## adk-rust Capability Map (What We Get for Free)

adk-rust (zavora-ai) มี features ที่ครอบคลุมมาก — หลายอย่างที่เราวางแผนจะเขียนเอง **มีอยู่แล้ว**:

| Feature | adk-rust Crate | Status |
|---------|---------------|--------|
| Multi-provider LLM (17+ providers) | `adk-model`, `adk-gemini`, `adk-anthropic` | **Built-in** |
| LlmAgent + Builder | `adk-agent` | **Built-in** |
| `#[tool]` macro (zero-boilerplate) | `adk-tool` | **Built-in** |
| MCP stdio + HTTP + Elicitation | `adk-tool` | **Built-in** |
| Session (SQLite + in-memory + encrypted) | `adk-session` | **Built-in** |
| Skill system (SKILL.md parsing + discovery) | `adk-skill` | **Built-in** |
| Sequential / Parallel / Loop agents | `adk-agent` | **Built-in** |
| Graph workflows (LangGraph-style) | `adk-graph` | **Built-in** |
| Browser automation (46 tools) | `adk-browser` | **Built-in** |
| Agent evaluation | `adk-eval` | **Built-in** |
| Guardrails (PII redaction, content filter) | `adk-guardrail` | **Built-in** |
| RBAC / OAuth / audit logging | `adk-auth` | **Built-in** |
| Sandbox | `adk-sandbox` | **Built-in** |
| REST API server + A2A v1.0.0 | `adk-server` | **Built-in** |
| Interactive CLI REPL | `adk-cli` | **Built-in** |
| Runner (context, events, streaming) | `adk-runner` | **Built-in** |
| RAG pipeline (6 backends) | `adk-rag` | **Built-in** |
| OpenTelemetry tracing | `adk-telemetry` | **Built-in** |
| Artifact storage | `adk-artifact` | **Built-in** |
| `cargo adk new` scaffolding | `cargo-adk` | **Built-in** |
| Local inference (mistral.rs) | `adk-mistralrs` | **Built-in** |

### What We Must Build Ourselves

| Feature | Why Not in adk-rust |
|---------|---------------------|
| **EverMemOS memory vault** (Obsidian wiki) | adk-memory uses vector DB, we use markdown + grep + LLM |
| **File tools** (read/write/edit/grep/glob) | adk-tool has `#[tool]` macro but no built-in file/shell tools |
| **Shell execution** with destructive detection | Not provided |
| **Filesystem sandbox** with `.agentignore` | adk-sandbox exists but needs customization for our workflow |
| **AGENTS.md / CLAUDE.md** context injection | Not provided |
| **Custom CLI** with slash commands (`/model`, `/mcp`, etc.) | adk-cli is basic REPL, we need domain-specific commands |
| **Memory vault tools** (mem_write, mem_search, etc.) | Completely custom — our main differentiator |
| **Web search/fetch** tools | Not provided (use MCP or build custom) |
| **Cost tracking** | adk-anthropic has pricing, but no aggregate tracking |
| **OS keychain secrets** | Not provided (uses .env) |

## Goals

- สร้าง agent harness ที่ใช้งานได้จริงสำหรับงาน software engineering ประจำวัน
- มีระบบความจำที่จดจำ สรุป และเรียนรู้จากประสบการณ์ผ่าน EverMemOS 6-level hierarchy
- รองรับหลาย LLM provider โดยไม่ต้อง lock-in (ใช้ adk-model ที่มี 17+ providers)
- ปลอดภัย: sandbox, approval prompt, ไม่ทำลายงานโดยไม่ตั้งใจ
- เรียนรู้ agent architecture patterns: tool use, MCP, sub-agent, memory lifecycle

## User Stories

---

### Phase 1: Core Agent (Tier 1 Must-Have)

---

### US-001: Scaffold project with adk-rust

**Description:** As a developer, I want to scaffold the project using `cargo adk new` and customize it for our harness.

**Acceptance Criteria:**
- [ ] `cargo adk new agent-harness` สร้าง project ที่ถูกต้อง
- [ ] `Cargo.toml` ใช้ `adk-rust = "0.6.0"` พร้อม features: `["openai", "anthropic", "deepseek", "ollama", "groq"]`
- [ ] `cargo build` ผ่านไม่มี error
- [ ] Custom source structure:
  ```
  agent-harness/
  ├── Cargo.toml
  ├── src/
  │   ├── main.rs            ← entry point (custom CLI)
  │   ├── harness.rs         ← Harness struct (wraps adk Runner)
  │   ├── cli/               ← REPL + one-shot + slash commands
  │   ├── tools/             ← custom #[tool] implementations
  │   │   ├── file.rs        ← FileRead, FileWrite, FileEdit
  │   │   ├── shell.rs       ← ShellExec with destructive detection
  │   │   ├── search.rs      ← Grep, Glob
  │   │   ├── web.rs         ← WebSearch, WebFetch
  │   │   └── memory.rs      ← mem_write, mem_search, etc.
  │   ├── memory/            ← memory-vault engine
  │   │   ├── vault.rs       ← ObsidianVault struct
  │   │   ├── parser.rs      ← frontmatter + wikilink parsing
  │   │   ├── retrieval.rs   ← grep_llm, graph_walk, tag_filter, agentic
  │   │   ├── lifecycle.rs   ← extract, consolidate, reflect
  │   │   └── types.rs       ← MemCell, Event, Foresight, Episode, etc.
  │   ├── context/           ← AGENTS.md / CLAUDE.md loader
  │   ├── sandbox/           ← filesystem sandbox + .agentignore
  │   ├── config/            ← settings management
  │   └── providers.rs       ← provider_from_env + mid-session switch
  ├── memory-vault/          ← already created (Obsidian wiki)
  └── tests/
  ```

---

### US-002: Multi-provider LLM — configure and switch mid-session

**Description:** As a user, I want to use adk-rust's built-in multi-provider support and switch between models mid-session via slash commands.

**Acceptance Criteria:**
- [ ] Use `adk-model` built-in providers: Gemini, OpenAI, Anthropic, DeepSeek, Groq, Ollama + OpenAI-compatible presets
- [ ] `provider_from_env()` auto-detect from env vars (built-in adk-rust)
- [ ] Custom `/model <name>` slash command: switch model mid-session (re-create model instance)
- [ ] Custom `/provider <name>` slash command: switch provider
- [ ] `/models` command: list available models (call provider's list API if available)
- [ ] API keys from `.env` (Phase 1) — OS keychain in Phase 3
- [ ] Wrapper struct `ProviderManager` that holds current model + allows hot-swap

---

### US-003: Built-in file tools using `#[tool]` macro

**Description:** As an agent, I need file read/write/edit tools implemented using adk-rust's `#[tool]` macro.

**Acceptance Criteria:**
- [ ] `FileRead` tool using `#[tool]` macro:
  ```rust
  #[derive(Deserialize, JsonSchema)]
  struct FileReadArgs {
      /// Path relative to working directory
      path: String,
      /// Optional line range (e.g., "1-50")
      range: Option<String>,
  }
  #[tool]
  async fn file_read(args: FileReadArgs) -> Result<Value, AdkError> { ... }
  ```
- [ ] `FileWrite` tool: write content to file
- [ ] `FileEdit` tool: string replacement in existing file (old_string → new_string)
- [ ] All file operations scoped to working directory via sandbox (US-006)
- [ ] Register tools with `LlmAgentBuilder::tool(Arc::new(FileRead))`

---

### US-004: Shell + Grep/Glob tools using `#[tool]` macro

**Description:** As an agent, I need shell execution and file search tools with destructive command protection.

**Acceptance Criteria:**
- [ ] `ShellExec` tool using `#[tool]` macro: run shell command with timeout (default 120s)
- [ ] `Grep` tool: search file content (use `grep-regex` crate internally)
- [ ] `Glob` tool: find files by pattern (use `glob` crate)
- [ ] Destructive command detection in ShellExec:
  - Pattern match: `rm -rf`, `git push --force`, `DROP TABLE`, `DELETE FROM`, `git reset --hard`
  - Return `needs_approval: true` flag → CLI prompts user before executing
- [ ] `! command` shell escape in REPL: run directly without agent (no tokens)

---

### US-005: Web search/fetch tools

**Description:** As an agent, I need web search and URL fetching capabilities.

**Acceptance Criteria:**
- [ ] `WebSearch` tool: search via Serper.dev API or DuckDuckGo
- [ ] `WebFetch` tool: fetch URL content, convert to markdown/text
- [ ] Results returned as structured JSON (title, URL, snippet)
- [ ] Rate limiting (configurable, default 10 req/min)

---

### US-006: Filesystem sandbox & safety

**Description:** As a user, I want all agent operations scoped to my project directory with approval prompts for dangerous actions.

**Acceptance Criteria:**
- [ ] Leverage `adk-sandbox` crate as base, extend with custom rules
- [ ] Path scoping: all file ops restricted to working directory
- [ ] Path traversal protection: `../../etc/passwd` → blocked
- [ ] `.agentignore` file support (use `ignore` crate, same format as `.gitignore`)
- [ ] `ToolConfirmationPolicy` (from adk-rust): integrate with adk-rust's built-in HITL system
- [ ] 3 permission modes via `.harness/settings.json` or `--permission` flag:
  - `strict` (default): approve every mutating tool call
  - `auto`: auto-approve non-destructive, flag destructive
  - `yolo`: auto-approve all (not recommended)

---

### US-007: Session persistence using adk-session

**Description:** As a user, I want conversation history saved using adk-rust's built-in SQLite session backend.

**Acceptance Criteria:**
- [ ] Use `adk-session` SQLite backend: `SqliteSessionService`
- [ ] Configure path: `~/.config/agent-harness/sessions.db`
- [ ] `/sessions` command: list past sessions
- [ ] `/resume <id>` command: resume previous session
- [ ] Auto-save every turn (built-in via adk-runner)
- [ ] Session metadata: project path, model, provider, timestamps

---

### US-008: Custom CLI REPL with slash commands

**Description:** As a user, I want an enhanced REPL built on top of adk-cli with domain-specific slash commands.

**Acceptance Criteria:**
- [ ] Extend `adk-cli` or build custom REPL using `rustyline`
- [ ] Streaming token output (use adk-runner's event streaming)
- [ ] Ctrl+C cancels current generation
- [ ] Slash commands:
  - `/help` — list all commands
  - `/model <name>` — switch model
  - `/provider <name>` — switch provider
  - `/models` — list available models
  - `/sessions` — list past sessions
  - `/resume <id>` — resume session
  - `/cost` — show token usage
  - `/mem <query>` — search memory vault
  - `/kms` — list knowledge bases
  - `/skill list` — list skills
  - `/mcp list` — list MCP servers
  - `/quit` — exit
- [ ] `! command` shell escape: run shell directly
- [ ] Multi-line input support (paste code blocks)
- [ ] Colored output: tool calls (yellow), errors (red), success (green)

---

### US-009: One-shot mode

**Description:** As a user, I want to run a single prompt and exit for scripts/CI.

**Acceptance Criteria:**
- [ ] `agent-harness -p "prompt"` runs 1 turn then exits
- [ ] `--model <name>` flag: override model
- [ ] `--provider <name>` flag: override provider
- [ ] `--project <path>` flag: set working directory
- [ ] `--permission <mode>` flag: set permission mode
- [ ] Exit code: 0 = success, 1 = error
- [ ] stdout: agent response, stderr: debug/logging
- [ ] `cat file.md | agent-harness -p "summarize"` stdin support

---

### US-010: AGENTS.md / CLAUDE.md context injection

**Description:** As a user, I want the agent to auto-discover project instruction files and inject them into the system prompt.

**Acceptance Criteria:**
- [ ] Walk up from `cwd` finding: `AGENTS.md`, `CLAUDE.md`, `.harness/AGENTS.md`
- [ ] Read all discovered files, inject into system prompt via `LlmAgentBuilder::instruction()`
- [ ] Files closer to cwd have higher priority (closer overrides further)
- [ ] Log: "Loaded context from: ./AGENTS.md, ../CLAUDE.md"
- [ ] No files found → continue without error

---

---

### Phase 2: Memory Vault & Intelligence (Tier 2 Should-Have)

---

### US-011: MCP integration using adk-tool's built-in MCP support

**Description:** As a user, I want to connect to MCP servers using adk-rust's built-in MCP client.

**Acceptance Criteria:**
- [ ] Use `adk-tool` MCP integration (stdio + HTTP Streamable)
- [ ] `/mcp add <name> <command-or-url>` add MCP server
- [ ] `/mcp list` show connected servers
- [ ] MCP tools auto-registered with `mcp_` namespace prefix
- [ ] Server config stored in `.harness/mcp.json`
- [ ] Support MCP Elicitation (servers can request user input)

---

### US-012: Memory vault — Write & Extract lifecycle

**Description:** As an agent, I want to automatically create MemCells and extract Events/Foresights/Episodes after each conversation turn into the Obsidian wiki vault.

**Acceptance Criteria:**
- [ ] `mem_write` tool (using `#[tool]` macro): append MemCell to daily log
- [ ] `mem_extract` tool: extract events + foresights + episode from MemCells
- [ ] `ObsidianVault` struct manages all vault I/O:
  ```rust
  struct ObsidianVault {
      vault_path: PathBuf,
      config: VaultConfig,
      counters: AtomicCounters,
  }
  ```
- [ ] Auto-trigger extraction via adk-runner's `AfterToolCallback`
- [ ] Notes created following schemas in `memory-vault/`
- [ ] Wikilinks `[[ref]]` connect parent-child correctly
- [ ] `.vault-config.json` counters updated after every write
- [ ] `index.md` regenerated after every write

---

### US-013: Memory vault — Retrieval (4 modes)

**Description:** As an agent, I want to search my memory vault using multiple retrieval strategies.

**Acceptance Criteria:**
- [ ] `mem_search` tool with 4 modes:
  - `grep_llm` (default): grep candidates → LLM rank → top N
  - `graph_walk`: follow `[[wikilinks]]` from seed note
  - `tag_filter`: filter by YAML frontmatter tags
  - `agentic`: multi-round grep → LLM sufficiency check → refined queries → merge
- [ ] `mem_read` tool: read specific note by ID or wikilink
- [ ] `mem_graph` tool: get connected notes (outgoing + backlinks)
- [ ] `mem_profile` tool: read agent/user profile
- [ ] Results include `relevance_score` (0-1) from LLM
- [ ] Relevant results auto-injected into context

---

### US-014: Memory vault — Consolidation & Reflection

**Description:** As an agent, I want to consolidate memories into clusters and update my profile over time.

**Acceptance Criteria:**
- [ ] `mem_consolidate` tool: cluster MemCells + update agent profile
- [ ] Cluster detection: same project + overlapping keywords (threshold: 0.65)
- [ ] Profile operations: ADD / UPDATE / DELETE (LLM-driven)
- [ ] Profile compaction: when items > 37 → LLM consolidates to ~17
- [ ] `mem_validate_foresights` tool: validate pending predictions
- [ ] `mem_reflect` tool: generate weekly/monthly reflection
- [ ] `mem_stats` tool: vault statistics

---

### US-015: Skill system using adk-skill

**Description:** As a user, I want to use adk-rust's built-in skill system (SKILL.md parsing + discovery) with custom skill installation.

**Acceptance Criteria:**
- [ ] Use `adk-skill` crate for SKILL.md parsing + `whenToUse` matching
- [ ] Skill discovery: scan `.harness/skills/` and `.claude/skills/`
- [ ] Auto-match: adk-skill matches user request via lexical matching
- [ ] Explicit invoke: `/<skill-name>` slash command
- [ ] `/skill install <git-url>` custom command: clone + install skill
- [ ] `/skill list` show installed skills

---

---

### Phase 3: Advanced Features (Tier 3 Nice-to-Have)

---

### US-016: Sub-agent orchestration using adk-agent

**Description:** As an agent, I want to delegate subtasks to isolated sub-agents using adk-rust's built-in agent types.

**Acceptance Criteria:**
- [ ] Use `SequentialAgent` and `ParallelAgent` from `adk-agent`
- [ ] `Task` tool: delegate subtask → spawn sub-agent with own tool registry
- [ ] Recursion limit: 3 levels deep (enforced by runner config)
- [ ] Sub-agent reports back as structured result
- [ ] Timeout per sub-agent (configurable, default 5 min)
- [ ] Parallel sub-agents via `ParallelAgent::SharedState`

---

### US-017: Knowledge base (KMS)

**Description:** As a user, I want a project wiki that the agent can search on demand.

**Acceptance Criteria:**
- [ ] `.kms/<name>/pages/*.md` directory structure
- [ ] `.kms/<name>/index.md` table of contents
- [ ] `KmsRead` tool: read a KMS page
- [ ] `KmsSearch` tool: full-text search across KMS pages
- [ ] `KmsWrite` tool: agent creates/edits KMS pages
- [ ] `/kms list` show attached knowledge bases
- [ ] KMS TOC injected into system prompt every turn

---

### US-018: Secrets management — OS keychain

**Description:** As a user, I want API keys stored in OS keychain.

**Acceptance Criteria:**
- [ ] macOS: Keychain via `security` CLI
- [ ] Linux: Secret Service (libsecret)
- [ ] Windows: Credential Manager
- [ ] `.env` fallback for CI
- [ ] `/key set <provider>` store key interactively
- [ ] `/key list` list stored providers (masked)
- [ ] Keys NEVER written to config files or logs

---

### US-019: Streaming & cancellation (enhance adk-runner defaults)

**Description:** As a user, I want real-time token streaming and cancellation.

**Acceptance Criteria:**
- [ ] Use adk-runner's built-in event streaming
- [ ] Tokens displayed as they arrive
- [ ] Ctrl+C cancels current generation
- [ ] Partial response saved to session on cancel
- [ ] Graceful shutdown: finish current tool call before exit

---

### US-020: Cost tracking

**Description:** As a user, I want to track token usage and costs.

**Acceptance Criteria:**
- [ ] Token counting per request (input + output) from adk-runner events
- [ ] Cost estimation using `adk-anthropic` pricing data (extend to all providers)
- [ ] Daily/session/project cost aggregation
- [ ] `/cost` command: show current session cost
- [ ] `/cost today` / `/cost week` / `/cost project <name>`
- [ ] Budget alert when approaching limit
- [ ] Cost data persisted in session DB

---

### US-021: Agent teams (multi-process coordination)

**Description:** As a user, I want multiple agents working in parallel using adk-rust's ParallelAgent + A2A protocol.

**Acceptance Criteria:**
- [ ] Use `adk-server` A2A v1.0.0 for inter-agent communication
- [ ] Lead agent spawns worker agents as separate processes
- [ ] Shared mailbox: file-based message queue
- [ ] Each worker in own tmux pane + optional git worktree
- [ ] `/team start` / `/team status` / `/team stop` commands
- [ ] Merge coordination: lead merges branches when workers done

---

## Functional Requirements

### Core Engine (from adk-rust)

- **FR-1**: System MUST use adk-rust v0.6.0 as foundation (`adk-agent` LlmAgent, `adk-runner` Runner, `adk-session` SessionService)
- **FR-2**: System MUST use adk-model's built-in multi-provider support (Gemini, OpenAI, Anthropic, DeepSeek, Groq, Ollama + presets)
- **FR-3**: System MUST use adk-tool's `#[tool]` macro for all custom tool implementations
- **FR-4**: System MUST use adk-runner's built-in streaming for token output
- **FR-5**: System MUST use adk-session SQLite backend for conversation persistence

### Custom Tools (build ourselves)

- **FR-6**: System MUST implement FileRead, FileWrite, FileEdit tools via `#[tool]` macro
- **FR-7**: System MUST implement ShellExec tool with destructive command detection and approval
- **FR-8**: System MUST implement Grep and Glob tools via `#[tool]` macro
- **FR-9**: System MUST implement WebSearch and WebFetch tools

### Safety

- **FR-10**: System MUST scope all file operations to working directory (leverage adk-sandbox)
- **FR-11**: System MUST detect destructive shell commands and require user approval
- **FR-12**: System MUST support `.agentignore` for excluding paths
- **FR-13**: System MUST provide 3 permission modes: `strict`, `auto`, `yolo`
- **FR-14**: System MUST use adk-rust's `ToolConfirmationPolicy` for HITL approval flow

### Memory Vault (build ourselves — main differentiator)

- **FR-15**: System MUST implement EverMemOS 6-level memory hierarchy as Obsidian wiki notes
- **FR-16**: System MUST auto-extract events, foresights, and episodes after each conversation turn
- **FR-17**: System MUST support 4 retrieval modes: `grep_llm`, `graph_walk`, `tag_filter`, `agentic`
- **FR-18**: System MUST consolidate MemCells into clusters and update agent profile
- **FR-19**: System MUST compact profile when items exceed threshold (37 → ~17)
- **FR-20**: System MUST validate foresight predictions against current state

### Context

- **FR-21**: System MUST walk up from cwd to find AGENTS.md / CLAUDE.md files
- **FR-22**: System MUST inject discovered context files into system prompt
- **FR-23**: System MUST support per-project configuration in `.harness/settings.json`

### MCP (from adk-tool)

- **FR-24**: System MUST use adk-tool's built-in MCP integration (stdio + HTTP)
- **FR-25**: System MUST auto-register MCP tools with `mcp_` namespace prefix
- **FR-26**: System MUST support MCP Elicitation for runtime user input

### Interface (custom CLI on top of adk-cli)

- **FR-27**: System MUST provide enhanced REPL with domain-specific slash commands
- **FR-28**: System MUST provide one-shot mode (`-p "prompt"`) for scripts/CI
- **FR-29**: System MUST support `! command` shell escape

### Skills (from adk-skill)

- **FR-30**: System MUST use adk-skill for SKILL.md parsing + `whenToUse` matching
- **FR-31**: System MUST support skill installation from git URLs
- **FR-32**: System MUST support explicit skill invocation via `/<skill-name>`

### Sub-Agents (from adk-agent)

- **FR-33**: System MUST use adk-agent's SequentialAgent / ParallelAgent for sub-task delegation
- **FR-34**: System MUST limit sub-agent recursion to 3 levels

## Non-Goals (Out of Scope)

- **No GUI** in this PRD — future phase
- **No multi-user / auth system** — single user only (use adk-auth if needed later)
- **No cloud hosting / SaaS** — runs locally only
- **No mobile app** — desktop/CLI only
- **No fine-tuning / model training** — use existing models only
- **No plugin marketplace** — install from git URL only
- **No built-in editor** — use external editors
- **No real-time collaboration** — single user workspace
- **No realtime voice** — out of scope (adk-realtime available if needed later)
- **No browser automation** — out of scope (adk-browser available if needed later)

## Design Considerations

### Architecture

```
┌──────────────────────────────────────────────────┐
│                Custom CLI Layer                    │
│   REPL (rustyline)    │    One-shot    │   Team  │
│   + slash commands    │    -p flag     │         │
└──────────────┬───────────────────────────────────┘
               │
┌──────────────▼───────────────────────────────────┐
│              Harness (our wrapper)                  │
│                                                    │
│  ┌──────────────┐  ┌───────────────────────────┐  │
│  │ ProviderMgr  │  │ ContextBuilder            │  │
│  │ (model swap) │  │  ├─ AGENTS.md walker     │  │
│  └──────────────┘  │  ├─ Skill loader (adk)   │  │
│                     │  └─ Memory context inject │  │
│  ┌──────────────┐  └───────────────────────────┘  │
│  │ Sandbox      │                                  │
│  │ + .agentign  │  ┌───────────────────────────┐  │
│  │ + approval   │  │ Custom #[tool] tools       │  │
│  └──────────────┘  │  ├─ FileRead/Write/Edit   │  │
│                     │  ├─ ShellExec (+safety)   │  │
│                     │  ├─ Grep/Glob             │  │
│                     │  ├─ WebSearch/Fetch       │  │
│                     │  ├─ Memory tools (10)     │  │
│                     │  └─ MCP tools (adk)       │  │
│                     └───────────────────────────┘  │
└──────────────┬───────────────────────────────────┘
               │ uses
┌──────────────▼───────────────────────────────────┐
│              adk-rust (framework)                   │
│                                                    │
│  adk-agent     LlmAgent, Sequential, Parallel     │
│  adk-model     Gemini, OpenAI, Anthropic, Ollama   │
│  adk-runner    streaming, events, callbacks        │
│  adk-session   SQLite persistence                  │
│  adk-tool      #[tool] macro, MCP client           │
│  adk-skill     SKILL.md parser + matcher           │
│  adk-sandbox   filesystem isolation                │
│  adk-cli       base REPL                           │
└────────────────────────────────────────────────────┘
               │
┌──────────────▼───────────────────────────────────┐
│             Memory Vault (Obsidian wiki)            │
│                                                    │
│  1-memcells/   ← raw experiences                   │
│  2-events/     ← atomic facts                      │
│  3-foresights/ ← predictions                       │
│  4-episodes/   ← narrative summaries               │
│  5-profile/    ← evolving traits                   │
│  6-reflections/← pattern analysis                  │
│  clusters/     ← semantic groupings                │
│                                                    │
│  Retrieval: grep + LLM (no vector DB needed)       │
└────────────────────────────────────────────────────┘
```

### Key Design Decisions

1. **adk-rust v0.6.0 as foundation**: Use LlmAgent, Runner, SessionService, `#[tool]` macro, MCP, skills from adk-rust directly — no reinventing wheels
2. **Memory-vault as Obsidian wiki**: Our main custom build — replaces adk-memory's vector approach with human-readable, git-friendly markdown
3. **Session in adk-session SQLite**: Production-ready, no need to build our own
4. **grep + LLM retrieval**: No vector DB for personal scale — LLM is the semantic engine
5. **`#[tool]` macro for all tools**: Zero-boilerplate tool definitions using adk-rust's built-in macro
6. **adk-runner callbacks**: Use `AfterToolCallback` for memory extraction triggers

## Technical Considerations

### Dependencies (Cargo.toml)

```toml
[dependencies]
# Agent framework (production-ready, 17+ providers)
adk-rust = { version = "0.6.0", features = [
    "openai", "anthropic", "deepseek", "ollama", "groq"
] }

# CLI (beyond adk-cli defaults)
clap = { version = "4", features = ["derive"] }
rustyline = "14"
colored = "2"

# Memory vault
frontmatter = "0.4"        # YAML frontmatter parsing
pulldown-cmark = "0.11"    # Markdown parsing + wikilink extraction
regex = "1"

# Tools
ignore = "0.4"             # .agentignore support

# Logging
tracing = "0.1"
tracing-subscriber = "0.3"
```

### File Locations

| File | Purpose |
|------|---------|
| `~/.config/agent-harness/settings.json` | User-level config |
| `~/.config/agent-harness/sessions.db` | Session persistence (adk-session) |
| `.harness/settings.json` | Project-level config |
| `.harness/mcp.json` | MCP server config |
| `.harness/skills/` | Installed skills (adk-skill scans this) |
| `memory-vault/` | EverMemOS memory vault (already created) |
| `AGENTS.md` / `CLAUDE.md` | Project context (auto-discovered) |
| `.agentignore` | Sandbox exclusion rules |

### adk-rust Crates We Use Directly

| Crate | What we use it for |
|-------|-------------------|
| `adk-agent` | `LlmAgentBuilder`, `SequentialAgent`, `ParallelAgent` |
| `adk-model` | `GeminiModel`, `OpenAIClient`, `AnthropicClient`, `OllamaModel`, `DeepSeekClient`, `GroqClient`, `provider_from_env()` |
| `adk-runner` | `Launcher`, event streaming, `AfterToolCallback` |
| `adk-session` | `SqliteSessionService` for conversation persistence |
| `adk-tool` | `#[tool]` macro, `FunctionTool`, MCP client |
| `adk-skill` | SKILL.md parsing, `.skills/` discovery, `whenToUse` matching |
| `adk-sandbox` | Base filesystem isolation |
| `adk-cli` | Base REPL (extend with custom slash commands) |
| `adk-core` | `Content`, `Part`, error types, streaming primitives |

## Success Metrics

| Metric | Target |
|--------|--------|
| Phase 1 completion | REPL works: receives input → sends to LLM → streams output |
| Tool accuracy | File edit 95%+ correct on test cases |
| Memory recall | mem_search returns relevant top-5 in 3/4 queries |
| Safety | Destructive commands never execute without approval (100%) |
| Provider switch | Switch provider in 1 command (`/model`) |
| First token latency | < 2 seconds (excluding cold start) |
| Memory vault size | < 100MB for 10,000 notes |

## Implementation Phases

### Phase 1: Core Agent (2-3 วัน)

| Order | Task | Depends on | adk-rust leverage |
|-------|------|------------|-------------------|
| 1.1 | US-001: Scaffold project | — | `cargo adk new` |
| 1.2 | US-002: Provider setup + switching | 1.1 | `adk-model` providers + `provider_from_env()` |
| 1.3 | US-003: File tools | 1.1 | `#[tool]` macro |
| 1.4 | US-004: Shell + Grep/Glob tools | 1.1 | `#[tool]` macro |
| 1.5 | US-006: Sandbox + safety | 1.3, 1.4 | `adk-sandbox` + `ToolConfirmationPolicy` |
| 1.6 | US-007: Session persistence | 1.1 | `adk-session` SQLite |
| 1.7 | US-008: Custom CLI REPL | 1.2, 1.5, 1.6 | `adk-cli` base + `adk-runner` streaming |
| 1.8 | US-009: One-shot mode | 1.7 | `adk-runner` |
| 1.9 | US-005: Web tools | 1.1 | `#[tool]` macro |
| 1.10 | US-010: AGENTS.md context | 1.7 | `LlmAgentBuilder::instruction()` |

### Phase 2: Memory Vault + Intelligence (1-2 สัปดาห์)

| Order | Task | Depends on | adk-rust leverage |
|-------|------|------------|-------------------|
| 2.1 | US-011: MCP integration | Phase 1 | `adk-tool` MCP client |
| 2.2 | US-012: Memory write + extract | Phase 1 | `#[tool]` macro + `AfterToolCallback` |
| 2.3 | US-013: Memory retrieval | 2.2 | `#[tool]` macro |
| 2.4 | US-014: Memory consolidation | 2.3 | `#[tool]` macro |
| 2.5 | US-015: Skill system | Phase 1 | `adk-skill` |

### Phase 3: Advanced (ตามความจำเป็น)

| Order | Task | Depends on | adk-rust leverage |
|-------|------|------------|-------------------|
| 3.1 | US-016: Sub-agent orchestration | Phase 2 | `SequentialAgent` / `ParallelAgent` |
| 3.2 | US-017: Knowledge base (KMS) | Phase 2 | `#[tool]` macro |
| 3.3 | US-018: OS keychain secrets | Phase 1 | Custom |
| 3.4 | US-019: Streaming improvements | 1.7 | `adk-runner` events |
| 3.5 | US-020: Cost tracking | 1.6, 3.4 | `adk-anthropic` pricing data |
| 3.6 | US-021: Agent teams | 3.1 | `adk-server` A2A v1.0.0 |

## Open Questions

- [x] ~~adk-rust มี built-in provider support แล้วหรือยัง?~~ → **YES: 17+ providers built-in**
- [x] ~~ต้องเขียน Tool trait เองไหม?~~ → **NO: `#[tool]` macro handles everything**
- [x] ~~ต้องเขียน session เองไหม?~~ → **NO: `adk-session` has SQLite backend**
- [x] ~~ต้องเขียน MCP client เองไหม?~~ → **NO: `adk-tool` has MCP built-in**
- [x] ~~ต้องเขียน skill system เองไหม?~~ → **NO: `adk-skill` parses SKILL.md**
- [ ] Memory vault path: relative to project หรือ global (`~/.config/agent-harness/memory-vault/`)?
- [ ] ต้องรองรับ ThaiLLM endpoint แยกต่างหากหรือใช้ OpenAI-compatible preset?
- [ ] Context compression: use adk-gemini's context compaction หรือ custom?
- [ ] adk-cli REPL: extend หรือ replace entirely? ต้อว่าดู source ก่อนว่า extensible แค่ไหน
- [ ] adk-sandbox: เพียงพอหรือต้องเขียน custom sandbox ทับ?
