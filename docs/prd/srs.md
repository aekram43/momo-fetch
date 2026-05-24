# Software Requirements Specification (SRS)
# Agent Harness — Rust-Native AI Agent Workspace

| Field | Value |
|-------|-------|
| **Version** | 1.0.0 |
| **Date** | 2026-05-06 |
| **Status** | Draft |
| **Source PRD** | `docs/prd/prd-momo-fetch.md` |
| **Framework** | adk-rust v0.6.0 (zavora-ai/adk-rust) |
| **Language** | Rust |

---

## 1. Introduction

### 1.1 Purpose

This document specifies the software requirements for **Agent Harness**, a Rust-native AI agent workspace that runs locally on the user's machine. It serves as the authoritative reference for implementation, testing, and validation.

### 1.2 Scope

Agent Harness is a personal CLI agent workspace (similar to Claude Code) built on the adk-rust framework. It provides:

- Multi-provider LLM connectivity (17+ providers)
- Tool system for file, shell, web, and memory operations
- MCP (Model Context Protocol) server integration
- Filesystem sandbox with approval prompts
- EverMemOS-compatible 6-level memory vault (Obsidian wiki)
- Skill/plugin system
- Sub-agent orchestration
- Multiple interface modes (REPL, one-shot, future GUI)

### 1.3 Definitions, Acronyms, and Abbreviations

| Term | Definition |
|------|-----------|
| adk-rust | Rust agent framework (zavora-ai/adk-rust v0.6.0) providing LLM abstraction, tools, sessions, MCP, skills |
| EverMemOS | 6-level memory hierarchy: MemCell → Event → Foresight → Episode → Profile → Reflection |
| Obsidian Vault | Markdown files with YAML frontmatter + `[[wikilinks]]` used as knowledge store |
| MCP | Model Context Protocol — stdio/HTTP protocol for tool servers |
| HITL | Human-in-the-Loop — approval flow for agent actions |
| MemCell | Level 1 raw experience record (per conversation turn) |
| Event (EventLogEntry) | Level 2 atomic fact extracted from MemCell |
| Foresight | Level 3 time-bounded prediction with validation |
| Episode | Level 4 narrative summary of related MemCells |
| Profile | Level 5 evolving behavioral traits (agent + user) |
| Reflection | Level 6 weekly/monthly pattern synthesis |
| Cluster | Virtual semantic grouping of related MemCells |
| `#[tool]` | adk-rust macro for zero-boilerplate tool definitions |
| SKILL.md | Markdown file defining a reusable agent skill |
| AGENTS.md / CLAUDE.md | Project instruction files auto-discovered by the agent |
| SOUL.md | Agent personality/identity file — defines communication style, values, behavioral traits |
| KMS | Knowledge Management System — project wiki for the agent |

### 1.4 References

| Reference | Location |
|-----------|----------|
| Source PRD | `docs/prd/prd-momo-fetch.md` |
| Memory Vault Schema Plan | `.claude/plans/async-dazzling-boole.md` |
| Memory Vault Templates | `memory-vault/templates/*.md` |
| adk-rust Framework | https://github.com/zavora-ai/adk-rust |
| EverMemOS Concept | EverMemOS PersonaSpawn 6-level memory hierarchy |

### 1.5 Overview

Section 2 describes the product context and user characteristics. Section 3 details all functional requirements. Section 4 covers non-functional requirements. Section 5 specifies system architecture and design constraints. Section 6 defines data requirements. Section 7 lists interfaces. Section 8 traces requirements to user stories.

---

## 2. Overall Description

### 2.1 Product Perspective

Agent Harness is a **single-user, local-only CLI application**. It wraps the adk-rust framework with a custom tool set, memory vault engine, and enhanced CLI.

```
┌──────────────────────────────────────────────────┐
│                Custom CLI Layer                    │
│   REPL (rustyline)    │    One-shot    │   Team  │
│   + slash commands    │    -p flag     │         │
└──────────────┬───────────────────────────────────┘
               │
┌──────────────▼───────────────────────────────────┐
│              Harness (our wrapper)                  │
│  ┌──────────────┐  ┌───────────────────────────┐  │
│  │ ProviderMgr  │  │ ContextBuilder            │  │
│  │ (model swap) │  │  ├─ SOUL.md walker       │  │
│  │              │  │  ├─ AGENTS.md walker     │  │
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
│  adk-agent, adk-model, adk-runner, adk-session,   │
│  adk-tool, adk-skill, adk-sandbox, adk-cli        │
└──────────────┬───────────────────────────────────┘
               │
┌──────────────▼───────────────────────────────────┐
│             Memory Vault (Obsidian wiki)            │
│  1-memcells/ → 2-events/ → 3-foresights/ →       │
│  4-episodes/ → 5-profile/ → 6-reflections/        │
│  clusters/                                           │
│  Retrieval: grep + LLM (no vector DB)               │
└────────────────────────────────────────────────────┘
```

### 2.2 User Characteristics

| Characteristic | Description |
|---------------|-------------|
| **Primary User** | Software engineer, comfortable with CLI tools |
| **Technical Level** | Advanced — writes code daily, uses Git, understands shell |
| **Usage Pattern** | Interactive REPL sessions (primary), one-shot scripts (secondary) |
| **Language** | English (primary), Thai (optional) |
| **Workload** | Personal use — 1 user, 1 machine |

### 2.3 Constraints

| ID | Constraint |
|----|-----------|
| C-1 | MUST use adk-rust v0.6.0 as foundation — no forking or replacing core crates |
| C-2 | MUST compile with stable Rust toolchain (no nightly features) |
| C-3 | MUST run on macOS, Linux, and Windows |
| C-4 | MUST work offline for local operations (only LLM calls require network) |
| C-5 | Memory vault MUST be human-readable Obsidian-compatible markdown |
| C-6 | No vector database dependency — retrieval uses grep + LLM only |
| C-7 | Single-user only — no multi-user, auth, or cloud hosting |

### 2.4 Assumptions and Dependencies

| ID | Assumption |
|----|-----------|
| A-1 | adk-rust v0.6.0 is published on crates.io with API stability |
| A-2 | User has Rust toolchain installed (rustup) |
| A-3 | User has at least one LLM API key configured (or Ollama running locally) |
| A-4 | `memory-vault/` directory structure already exists (created in prior session) |
| A-5 | OpenAI-compatible endpoints work via `OpenAICompatibleConfig` for any provider not natively supported |

---

## 3. Functional Requirements

### 3.1 Project Scaffolding (FR-Scaffold)

#### FR-S-01: Initialize project structure

The system SHALL provide a standard project layout after `cargo adk new` with custom source structure:

```
momo-fetch/
├── Cargo.toml
├── src/
│   ├── main.rs
│   ├── harness.rs
│   ├── cli/
│   ├── tools/
│   │   ├── file.rs
│   │   ├── shell.rs
│   │   ├── search.rs
│   │   ├── web.rs
│   │   └── memory.rs
│   ├── memory/
│   │   ├── vault.rs
│   │   ├── parser.rs
│   │   ├── retrieval.rs
│   │   ├── lifecycle.rs
│   │   └── types.rs
│   ├── context/
│   ├── sandbox/
│   ├── config/
│   └── providers.rs
├── memory-vault/
└── tests/
```

#### FR-S-02: Cargo dependencies

The system SHALL declare the following core dependencies in `Cargo.toml`:

```toml
[dependencies]
adk-rust = { version = "0.6.0", features = ["openai", "anthropic", "deepseek", "ollama", "groq"] }
clap = { version = "4", features = ["derive"] }
rustyline = "14"
colored = "2"
frontmatter = "0.4"
pulldown-cmark = "0.11"
regex = "1"
ignore = "0.4"
tracing = "0.1"
tracing-subscriber = "0.3"
```

---

### 3.2 LLM Provider Management (FR-Provider)

#### FR-P-01: Multi-provider support

The system SHALL support the following LLM providers via adk-model:
- Google Gemini (native)
- OpenAI + Azure OpenAI (native)
- Anthropic Claude (native)
- DeepSeek (native)
- Groq (native)
- Ollama (native, local)
- OpenRouter (native)
- Amazon Bedrock (native)
- Any OpenAI-compatible endpoint via `OpenAICompatibleConfig`

#### FR-P-02: Environment-based auto-detection

The system SHALL auto-detect provider and API key from environment variables using adk-model's `provider_from_env()`.

#### FR-P-03: Mid-session provider switching

The system SHALL support switching providers and models mid-session via `ProviderManager` struct:

```rust
struct ProviderManager {
    current_model: Arc<dyn Llm>,
    current_provider: String,
    current_model_name: String,
}
```

Operations:
- `switch_model(name: &str)` — change model within current provider
- `switch_provider(name: &str, model: &str)` — change provider entirely
- `list_models()` — query available models from current provider

#### FR-P-04: OpenAI-compatible custom endpoints

The system SHALL allow connecting arbitrary OpenAI-compatible endpoints:

```rust
OpenAICompatible::new(
    OpenAICompatibleConfig::new("api_key", "model_name")
        .with_base_url("https://custom-endpoint.com/v1")
        .with_provider_name("custom"),
)
```

---

### 3.3 Tool System (FR-Tool)

All tools SHALL use adk-rust's `#[tool]` macro for zero-boilerplate registration with `LlmAgentBuilder::tool()`.

#### 3.3.1 File Tools (FR-Tool-File)

##### FR-TF-01: FileRead

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `path` | `String` | Yes | File path relative to working directory |
| `range` | `Option<String>` | No | Line range, e.g. "1-50" |

- SHALL return file contents with line numbers (cat -n format)
- SHALL scope path to working directory via sandbox
- SHALL resolve `.` and `..` components safely

##### FR-TF-02: FileWrite

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `path` | `String` | Yes | File path relative to working directory |
| `content` | `String` | Yes | Content to write |

- SHALL create parent directories if they don't exist
- SHALL overwrite existing file content
- SHALL require approval in `strict` permission mode

##### FR-TF-03: FileEdit

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `path` | `String` | Yes | File path relative to working directory |
| `old_string` | `String` | Yes | Exact string to find and replace |
| `new_string` | `String` | Yes | Replacement string |

- SHALL fail if `old_string` is not unique in the file (multiple matches)
- SHALL fail if `old_string` is not found
- SHALL require approval in `strict` permission mode

#### 3.3.2 Shell & Search Tools (FR-Tool-Shell)

##### FR-TS-01: ShellExec

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `command` | `String` | Yes | Shell command to execute |
| `timeout` | `Option<u64>` | No | Timeout in seconds (default: 120) |

- SHALL execute command via system shell (`sh -c` on Unix, `cmd /c` on Windows)
- SHALL detect destructive commands (see FR-Safety-02)
- SHALL return stdout, stderr, and exit code
- SHALL support `! command` escape in REPL for direct execution without agent involvement

##### FR-TS-02: Grep

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `pattern` | `String` | Yes | Regex pattern to search |
| `path` | `Option<String>` | No | Directory to search (default: cwd) |
| `include` | `Option<String>` | No | File glob filter, e.g. "*.rs" |
| `output_mode` | `Option<String>` | No | "content" or "files" (default: "content") |

- SHALL use `grep-regex` crate internally
- SHALL respect `.agentignore` rules

##### FR-TS-03: Glob

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `pattern` | `String` | Yes | Glob pattern, e.g. "**/*.rs" |
| `path` | `Option<String>` | No | Base directory (default: cwd) |

- SHALL use `glob` crate internally
- SHALL respect `.agentignore` rules

#### 3.3.3 Web Tools (FR-Tool-Web)

##### FR-TW-01: WebSearch

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `query` | `String` | Yes | Search query |
| `max_results` | `Option<usize>` | No | Max results (default: 10) |

- SHALL search via Serper.dev API or DuckDuckGo
- SHALL return structured results: `[{title, url, snippet}]`
- SHALL enforce rate limiting (configurable, default 10 req/min)

##### FR-TW-02: WebFetch

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `url` | `String` | Yes | URL to fetch |
| `format` | `Option<String>` | No | "markdown" or "text" (default: "markdown") |

- SHALL fetch URL content and convert to specified format
- SHALL follow redirects (max 5)
- SHALL respect robots.txt

#### 3.3.4 Memory Tools (FR-Tool-Memory)

See Section 3.6 for full memory tool specifications.

---

### 3.4 Safety & Sandboxing (FR-Safety)

#### FR-SF-01: Path scoping

The system SHALL restrict all file operations to the configured working directory:
- Resolve symlinks before checking scope
- Block paths containing `..` that escape the working directory
- Log blocked access attempts

#### FR-SF-02: Destructive command detection

The system SHALL detect the following destructive patterns in `ShellExec`:

| Pattern | Category |
|---------|----------|
| `rm -rf /`, `rm -rf ~` | Destructive deletion |
| `git push --force`, `git push -f` | Destructive git |
| `git reset --hard` | Destructive git |
| `DROP TABLE`, `DELETE FROM` | Destructive SQL |
| `mkfs`, `dd if=` | Destructive system |
| `> /dev/sd`, `chmod -R 777 /` | Destructive system |

Detection SHALL return `needs_approval: true` flag to the CLI layer for user confirmation.

#### FR-SF-03: Permission modes

The system SHALL support three permission modes:

| Mode | Non-destructive | Destructive | Description |
|------|----------------|-------------|-------------|
| `strict` | Prompt | Prompt | Default — approve every mutating tool call |
| `auto` | Auto-approve | Prompt | Trust read ops, ask for writes |
| `yolo` | Auto-approve | Auto-approve | No prompts (not recommended) |

Mode SHALL be configurable via:
1. CLI flag: `--permission <mode>`
2. REPL slash command: `/permission <mode>` (mid-session switching)
3. Project config: `.harness/settings.json`
4. User config: `~/.config/momo-fetch/settings.json`

Priority: CLI flag > project config > user config > default (`strict`)

#### FR-SF-04: .agentignore

The system SHALL support `.agentignore` files using the same format as `.gitignore` (via `ignore` crate). Rules:
- `.agentignore` in project root applies to all operations
- Nested `.agentignore` files apply to their subdirectories
- Patterns: node_modules, .git, target, *.min.js, etc.

#### FR-SF-05: HITL approval flow

The system SHALL integrate with adk-rust's `ToolConfirmationPolicy`:
- Blocking approval: pause execution, wait for user Y/N
- Approval prompt shows: tool name, parameters, risk level
- User can approve once or approve-all for session

---

### 3.5 CLI Interface (FR-CLI)

#### 3.5.1 REPL Mode (FR-CLI-REPL)

##### FR-CR-01: Interactive REPL

The system SHALL provide an interactive REPL using `rustyline` with:
- Streaming token output (via adk-runner events)
- Ctrl+C cancels current generation
- `! command` shell escape for direct shell execution
- Multi-line input (shift+enter or paste)
- Colored output: tool calls (yellow), errors (red), success (green), system (dim)

##### FR-CR-02: Slash commands

| Command | Description |
|---------|-------------|
| `/help` | List all commands with descriptions |
| `/model <name>` | Switch model (e.g., `/model gpt-4o`) |
| `/provider <name>` | Switch provider (e.g., `/provider ollama`) |
| `/models` | List available models from current provider |
| `/sessions` | List past sessions |
| `/resume <id>` | Resume a previous session |
| `/clear` | Clear all context and start a fresh session |
| `/compact` | Summarize current context into a compacted new session |
| `/cost` | Show current session token usage and cost |
| `/permission [mode]` | Show or switch permission mode (strict/auto/yolo) |
| `/mem <query>` | Search memory vault |
| `/kms` | List attached knowledge bases |
| `/skill list` | List installed skills |
| `/skill install <url>` | Install skill from git URL |
| `/mcp list` | List connected MCP servers (stdio + HTTP) |
| `/mcp add <name> <cmd>` | Add stdio MCP server |
| `/key set <provider>` | Store API key in OS keychain |
| `/key list` | List stored keys (masked) |
| `/team start` | Start agent team |
| `/team status` | Show team status |
| `/team stop` | Stop agent team |
| `/quit` | Exit REPL |

#### 3.5.2 Custom Slash Commands (FR-CLI-Custom)

##### FR-CC-01: User-defined slash commands

The system SHALL support user-defined custom slash commands via `.md` files placed in `.harness/commands/`:

- Each `.md` file defines a command named after the file (e.g., `review.md` becomes `/review`)
- The file content is used as the prompt sent to the agent as a user message
- The `$ARG` placeholder SHALL be replaced with any text following the command name
- Command names SHALL only contain alphanumeric characters, dashes, and underscores
- Custom commands SHALL NOT override built-in commands
- Custom commands SHALL be listed in `/help` output under a "Custom commands" heading
- Commands are discovered at invocation time — no restart required

Example:

```
.harness/commands/review.md → /review src/main.rs
Content: "Review this code for bugs: $ARG"
Prompt sent: "Review this code for bugs: src/main.rs"
```

#### 3.5.3 One-Shot Mode (FR-CLI-OneShot)

##### FR-CO-01: Single prompt execution

The system SHALL support one-shot mode via `-p` flag:

```
momo-fetch -p "explain this code" --project ./my-project
```

| Flag | Description | Required |
|------|-------------|----------|
| `-p, --prompt` | Prompt to send to agent | Yes |
| `--model` | Override model | No |
| `--provider` | Override provider | No |
| `--project` | Set working directory | No |
| `--permission` | Set permission mode | No |

Behavior:
- Execute 1 turn, output response, exit
- Exit code 0 = success, 1 = error
- stdout = agent response, stderr = debug/logging
- Support stdin pipe: `cat file.md | momo-fetch -p "summarize"`

---

### 3.6 Memory Vault (FR-Memory)

### 3.6.1 Vault Structure

The memory vault SHALL use the following Obsidian wiki structure:

```
memory-vault/
├── .vault-config.json
├── index.md
├── 1-memcells/YYYY/MM/YYYY-MM-DD.md
├── 2-events/fact-NNNN.md
├── 3-foresights/pred-NNNN.md
├── 4-episodes/ep-NNNN.md
├── 5-profile/agent-profile.md
├── 5-profile/user-profile.md
├── 6-reflections/weekly/YYYY-WNN.md
├── 6-reflections/monthly/YYYY-MM.md
├── clusters/cluster-NNN.md
└── templates/*.md
```

### 3.6.2 Write Tools (FR-MW)

##### FR-MW-01: mem_write

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `project` | `String` | Yes | Project identifier |
| `topic` | `String` | Yes | Short topic description |
| `context` | `String` | Yes | Full context of what happened |
| `actions` | `Vec<Action>` | Yes | List of actions taken |
| `outcome` | `String` | Yes | Result of the interaction |
| `keywords` | `Vec<String>` | Yes | Searchable keywords |

- SHALL append MemCell section to daily log file `1-memcells/YYYY/MM/YYYY-MM-DD.md`
- SHALL auto-create directory and file if not exists
- SHALL update `.vault-config.json` counters
- SHALL regenerate `index.md`

##### FR-MW-02: mem_extract

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `memcell_refs` | `Vec<String>` | Yes | Wikilinks to MemCells to process |
| `extract_events` | `bool` | No | Extract atomic facts (default: true) |
| `extract_foresights` | `bool` | No | Extract predictions (default: true) |
| `extract_episode` | `bool` | No | Generate episode summary (default: true) |

- SHALL use LLM to extract events (5-15 per MemCell)
- SHALL use LLM to extract foresights (4-10 per MemCell)
- SHALL create linked notes with correct `[[wikilinks]]`
- SHALL update all parent references
- SHALL be auto-triggered via adk-runner's `AfterToolCallback`

### 3.6.3 Read/Retrieval Tools (FR-MR)

##### FR-MR-01: mem_search

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `query` | `String` | Yes | Natural language search query |
| `mode` | `String` | No | One of: `grep_llm`, `graph_walk`, `tag_filter`, `agentic` (default: `grep_llm`) |
| `levels` | `Vec<String>` | No | Filter by memory level (default: all) |
| `project` | `Option<String>` | No | Filter by project |
| `tags` | `Vec<String>` | No | Filter by tags |
| `limit` | `usize` | No | Max results (default: 20) |
| `since` | `Option<String>` | No | ISO date filter |

**Retrieval modes:**

| Mode | Algorithm |
|------|-----------|
| `grep_llm` | grep keywords → sort by match count → LLM ranks relevance → top N |
| `graph_walk` | Start from seed note → follow `[[wikilinks]]` outward → collect backlinks → LLM rank |
| `tag_filter` | Parse YAML frontmatter → filter by tag intersection |
| `agentic` | grep_llm (round 1, 20 candidates) → LLM sufficiency check → if insufficient: LLM generates refined queries → grep again → merge → LLM final rank |

Result format:
```rust
struct MemoryResult {
    ref_id: String,           // "[[fact-0142]]"
    level: MemoryLevel,
    relevance_score: f32,     // 0.0-1.0 from LLM
    snippet: String,          // First 200 chars
    full_content: String,     // When agent requests full read
}
```

##### FR-MR-02: mem_read

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `ref` | `String` | Yes | Note ID or wikilink (e.g., "fact-0142" or "[[2026-05-06#MemCell 001]]") |

- SHALL return full note content with frontmatter

##### FR-MR-03: mem_graph

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `ref` | `String` | Yes | Starting note |
| `depth` | `usize` | No | Link traversal depth (default: 1, max: 3) |

- SHALL return outgoing `[[wikilinks]]` and backlinks (who links to this note)

##### FR-MR-04: mem_profile

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `profile` | `String` | Yes | "agent" or "user" |
| `action` | `String` | Yes | "read" or "update" |
| `updates` | `Option<ProfileUpdate>` | No | Only for action="update" |

### 3.6.4 Lifecycle Tools (FR-ML)

##### FR-ML-01: mem_consolidate

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `project` | `Option<String>` | No | Filter by project |
| `force` | `bool` | No | Force even if no new MemCells (default: false) |

- SHALL cluster recent MemCells (threshold: same project + overlapping keywords, similarity ≥ 0.65)
- SHALL create/update `clusters/cluster-NNN.md`
- SHALL update `5-profile/agent-profile.md` (ADD / UPDATE / DELETE items)
- SHALL trigger profile compaction if items > 37

##### FR-ML-02: mem_validate_foresights

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `project` | `Option<String>` | No | Filter by project |
| `foresight_ids` | `Option<Vec<String>>` | No | Specific foresights to validate |

- SHALL check pending foresights against current state
- SHALL update status: `pending` → `validated` / `failed` / `expired`
- SHALL record evidence for status change

##### FR-ML-03: mem_reflect

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `period` | `String` | Yes | "weekly" or "monthly" |
| `date` | `Option<String>` | No | Target date (default: current) |

- SHALL generate reflection with pattern analysis, decision weight adjustments, and foresight validation summary

##### FR-ML-04: mem_stats

No parameters. SHALL return vault statistics from `.vault-config.json`.

### 3.6.5 Profile Compaction

When `5-profile/agent-profile.md` item count exceeds 37:
1. LLM reads all items
2. LLM merges similar items
3. LLM removes low-value items
4. Target: ~17 items (70% of threshold)
5. LLM preserves the most impactful and recent items

### 3.6.6 Memory Auto-Flow (FR-MA)

The system SHALL provide an active memory system that automatically searches and writes memories without requiring the LLM to explicitly invoke memory tools.

#### FR-MA-01: Auto-search (Pre-Turn)

The system SHALL automatically search the memory vault before each conversational turn:

- Extract keywords from the user's input using TF-IDF-like scoring
- Execute `vault.search()` with `MemoryQuery` using the configured `search_mode`
- Inject relevant memories into the user's message, prefixed with `"--- Relevant memories ---"`
- If no results found, pass the original input unchanged
- Configurable via `memory.auto_search` in settings (default: `true`)
- Maximum results per turn configurable via `memory.max_results_per_turn` (default: 5)

#### FR-MA-02: Auto-write (Post-Turn)

The system SHALL automatically write a MemCell after each completed conversational turn:

- Collect a `TurnSummary` containing: user message (truncated 200 chars), tool calls made, response preview (truncated 300 chars), project name
- Extract keywords using TF-IDF with stop-word filtering (Option A) or a sidecar model (Option B)
- Write MemCell to vault with extracted topic, context, actions, outcome, and keywords
- Configurable via `memory.auto_write` in settings (default: `true`)
- Auto-extract trigger: log when `memcells_since_extract >= extract_threshold` (default: 10)

#### FR-MA-03: Sidecar Model (Option B)

The system SHALL support a small sidecar model for memory extraction when configured:

- When `memory.sidecar_model` is set, generate an extraction prompt for the sidecar sub-agent
- The sidecar sub-agent uses the specified model (e.g., `deepseek-chat`, `llama3.2`) via the specified `sidecar_provider`
- The sub-agent returns structured JSON: `{ topic, context, actions, outcome, keywords }`
- When `sidecar_model` is null (default), use TF-IDF keyword extraction (Option A, zero extra LLM cost)

#### FR-MA-04: Separate Process Mode (Option C)

The system SHALL support running the memory sidecar as a completely separate process:

- CLI flag: `--mode memory-sidecar` starts the sidecar process
- Communication via file-based Mailbox protocol in `.harness/mailbox/`
- Well-known message types: `search_request`, `search_response`, `write_request`, `write_response`, `ready`, `shutdown`
- Main process sends search/write requests; sidecar responds
- Sidecar signals `ready` on startup and responds to `shutdown` for graceful termination
- Polling interval: 100ms

#### FR-MA-05: Memory System Prompt

When `auto_search` or `auto_write` is enabled, the system SHALL append a memory context block to the system prompt informing the agent:

- Total number of memories and sessions in the vault
- That relevant memories are automatically injected (marked with `"--- Relevant memories ---"`)
- That `mem_search` is available for deeper manual recall
- That memory entries are automatically created and `mem_write` is only needed for specific saves

### 3.6.7 Memory Settings Configuration

```jsonc
// .harness/settings.json — memory section
{
  "memory": {
    "auto_search": true,           // Pre-turn vault search
    "auto_write": true,            // Post-turn MemCell write
    "search_mode": "grep_llm",     // Retrieval mode: grep_llm | tag_filter
    "max_results_per_turn": 5,     // Max injected memories
    "extract_threshold": 10,       // MemCells before auto-extract hint
    "sidecar_model": null,         // null = Option A; "deepseek-chat" = Option B
    "sidecar_provider": null       // Provider for sidecar model
  }
}
```

---

### 3.7 MCP Integration (FR-MCP)

#### FR-MC-01: MCP server management

The system SHALL use adk-tool's built-in MCP client:
- Support stdio and HTTP Streamable transports
- `/mcp add <name> <command-or-url>` to add servers
- `/mcp list` to show connected servers
- Config stored in `.harness/mcp.json`
- `--test-mcp` CLI flag to verify MCP server connectivity and report status per transport type

#### FR-MC-02: MCP tool namespace

MCP-provided tools SHALL be auto-registered with `mcp_` namespace prefix to avoid conflicts with built-in tools.

#### FR-MC-03: MCP Elicitation

The system SHALL support MCP Elicitation protocol — MCP servers can request user input during tool execution.

#### FR-MC-04: MCP connection testing

The system SHALL provide a `--test-mcp` CLI flag that connects to all configured MCP servers (stdio and HTTP), reports their connection status, transport type, and total available tool count, then exits.

---

### 3.8 Context System (FR-Context)

#### FR-CX-01: SOUL.md / AGENTS.md / CLAUDE.md discovery

The system SHALL walk up from `cwd` to filesystem root, looking for:
- `SOUL.md`
- `AGENTS.md`
- `CLAUDE.md`
- `.harness/SOUL.md`
- `.harness/AGENTS.md`

Priority: files closer to `cwd` override files further up.

#### FR-CX-02: Context injection

Discovered context files SHALL be injected into the system prompt via `LlmAgentBuilder::instruction()` in this order:
1. Base instruction
2. SOUL.md content (with `"--- Soul from {path} ---"` header)
3. AGENTS.md / CLAUDE.md content (with `"--- Context from {path} ---"` header)
4. KMS TOC
5. Skill context

Log message: `"Loaded context from: ./SOUL.md, ./AGENTS.md, ../CLAUDE.md"`

#### FR-CX-03: Graceful fallback

If no context files are found, the system SHALL continue without error.

---

### 3.9 Session Management (FR-Session)

#### FR-SS-01: SQLite persistence

The system SHALL use adk-session's `SqliteSessionService` with path `~/.config/momo-fetch/sessions.db`.

#### FR-SS-02: Session metadata

Each session SHALL record:
- Project path
- Model name
- Provider name
- Created timestamp
- Last updated timestamp

#### FR-SS-03: Auto-save

The system SHALL auto-save every conversation turn via adk-runner's built-in persistence.

#### FR-SS-04: Session CLI commands

| Command | Description |
|---------|-------------|
| `/sessions` | List past sessions (id, project, model, date) |
| `/resume <id>` | Load and continue a previous session |

---

### 3.10 Skill System (FR-Skill)

#### FR-SK-01: SKILL.md parsing

The system SHALL use adk-skill crate for:
- Parsing SKILL.md files
- Matching user requests via `whenToUse` lexical matching
- Auto-triggering skills when request matches

#### FR-SK-02: Skill discovery

The system SHALL scan for skills in:
- `.harness/skills/`
- `.claude/skills/`

#### FR-SK-03: Skill commands

| Command | Description |
|---------|-------------|
| `/<skill-name>` | Explicitly invoke a skill |
| `/skill list` | Show installed skills |
| `/skill install <git-url>` | Clone and install skill from git |

---

### 3.11 Sub-Agent Orchestration (FR-SubAgent)

#### FR-SA-01: Task delegation

The system SHALL provide a `Task` tool that spawns sub-agents using adk-agent's `SequentialAgent` and `ParallelAgent`.

#### FR-SA-02: Recursion limit

Sub-agent spawning SHALL be limited to 3 levels deep (enforced by runner config).

#### FR-SA-03: Sub-agent isolation

Each sub-agent SHALL have:
- Its own tool registry
- Its own conversation context
- Configurable timeout (default: 5 minutes)

#### FR-SA-04: Parallel execution

The system SHALL support parallel sub-agents via `ParallelAgent::SharedState`.

---

### 3.12 Knowledge Base (FR-KMS)

#### FR-KB-01: KMS directory structure

```
.kms/<name>/
├── pages/
│   ├── page-001.md
│   └── ...
└── index.md    ← table of contents
```

#### FR-KB-02: KMS tools

| Tool | Description |
|------|-------------|
| `KmsRead` | Read a KMS page by path |
| `KmsSearch` | Full-text search across KMS pages |
| `KmsWrite` | Create or edit KMS pages |

#### FR-KB-03: KMS context injection

KMS table of contents SHALL be injected into system prompt every turn. `/kms list` shows attached knowledge bases.

---

### 3.13 Secrets Management (FR-Secrets)

#### FR-SC-01: OS keychain storage

| Platform | Backend |
|----------|---------|
| macOS | Keychain via `security` CLI |
| Linux | Secret Service (libsecret) |
| Windows | Credential Manager |

#### FR-SC-02: Fallback

`.env` file SHALL be used as fallback for CI environments.

#### FR-SC-03: Security rules

- API keys SHALL NEVER be written to config files or logs
- `/key list` SHALL display keys as masked (e.g., `sk-****...abcd`)
- `/key set <provider>` SHALL prompt for key interactively

---

### 3.14 Cost Tracking (FR-Cost)

#### FR-CT-01: Token counting

The system SHALL count input and output tokens per request from adk-runner events.

#### FR-CT-02: Cost estimation

The system SHALL estimate costs using provider pricing data (initially from adk-anthropic, extended to all providers).

#### FR-CT-03: Aggregation

The system SHALL aggregate costs by:
- Current session
- Today
- This week
- Per project

#### FR-CT-04: Commands

| Command | Description |
|---------|-------------|
| `/cost` | Current session cost |
| `/cost today` | Today's total cost |
| `/cost week` | This week's total cost |
| `/cost project <name>` | Cost for a specific project |

#### FR-CT-05: Budget alerts

The system SHALL alert when approaching configurable cost limits.

#### FR-CT-06: Persistence

Cost data SHALL be stored in the session database.

---

## 4. Non-Functional Requirements

### 4.1 Performance

| ID | Requirement | Target |
|----|-------------|--------|
| NFR-P-01 | First token latency (warm) | < 2 seconds |
| NFR-P-02 | Memory search retrieval | < 3 seconds for 500 notes |
| NFR-P-03 | Tool execution overhead | < 50ms (excluding actual I/O) |
| NFR-P-04 | REPL startup time | < 1 second |
| NFR-P-05 | Memory vault size | < 100MB for 10,000 notes |

### 4.2 Reliability

| ID | Requirement |
|----|-------------|
| NFR-R-01 | Destructive commands SHALL NEVER execute without approval (100%) |
| NFR-R-02 | File edit operations SHALL fail safely (no partial writes on error) |
| NFR-R-03 | Session data SHALL survive process crashes (SQLite WAL mode) |
| NFR-R-04 | Memory vault writes SHALL be atomic (write to temp file, then rename) |

### 4.3 Usability

| ID | Requirement |
|----|-------------|
| NFR-U-01 | Provider switch SHALL complete in 1 command (`/model`) |
| NFR-U-02 | All slash commands SHALL be discoverable via `/help` |
| NFR-U-03 | Error messages SHALL include actionable next steps |
| NFR-U-04 | Colored output SHALL improve readability (not overwhelm) |

### 4.4 Security

| ID | Requirement |
|----|-------------|
| NFR-SEC-01 | API keys SHALL be stored in OS keychain, never in config files or logs |
| NFR-SEC-02 | Path traversal attacks SHALL be blocked by sandbox |
| NFR-SEC-03 | Shell injection in tool parameters SHALL be prevented |
| NFR-SEC-04 | .agentignore SHALL be respected for sensitive paths |

### 4.5 Portability

| ID | Requirement |
|----|-------------|
| NFR-PORT-01 | SHALL compile and run on macOS (aarch64 + x86_64) |
| NFR-PORT-02 | SHALL compile and run on Linux (x86_64 + aarch64) |
| NFR-PORT-03 | SHALL compile and run on Windows (x86_64) |
| NFR-PORT-04 | Memory vault files SHALL be Obsidian-compatible on all platforms |

---

## 5. System Architecture

### 5.1 Module Dependency Graph

```
main.rs
  ├── cli/           (REPL, one-shot, slash commands)
  │     └── depends on: harness, config
  ├── harness.rs     (central orchestrator)
  │     ├── depends on: providers, tools/*, memory/*, context, sandbox, config
  │     └── wraps: adk-agent, adk-runner, adk-session
  ├── providers.rs   (ProviderManager)
  │     └── depends on: adk-model (all providers)
  ├── tools/
  │     ├── file.rs        → adk-tool (#[tool])
  │     ├── shell.rs       → adk-tool (#[tool])
  │     ├── search.rs      → adk-tool (#[tool])
  │     ├── web.rs         → adk-tool (#[tool])
  │     └── memory.rs      → adk-tool (#[tool]) + memory/*
  ├── memory/
  │     ├── vault.rs       → filesystem I/O
  │     ├── parser.rs      → frontmatter + markdown parsing
  │     ├── retrieval.rs   → grep + LLM ranking
  │     ├── lifecycle.rs   → extract, consolidate, reflect
  │     └── types.rs       → data structures
  ├── context/        → filesystem walk + file read
  ├── sandbox/        → adk-sandbox + ignore crate
  └── config/         → settings.json + .harness/ config
```

### 5.2 Key Structs

```rust
/// Central orchestrator wrapping adk-rust
pub struct Harness {
    runner: Runner,
    agent: LlmAgent,
    vault: ObsidianVault,
    provider_mgr: ProviderManager,
    sandbox: FilesystemSandbox,
    context_builder: ContextBuilder,
    session_service: SqliteSessionService,
}

/// Memory vault engine
pub struct ObsidianVault {
    vault_path: PathBuf,
    config: VaultConfig,
    counters: AtomicCounters,
}

/// LLM provider hot-swap
pub struct ProviderManager {
    current_model: Arc<dyn Llm>,
    current_provider: String,
    current_model_name: String,
}

/// Filesystem sandbox with .agentignore
pub struct FilesystemSandbox {
    root: PathBuf,
    ignore_rules: Ignore,
    permission_mode: PermissionMode,
}
```

### 5.3 Configuration Files

| File | Scope | Purpose |
|------|-------|---------|
| `~/.config/momo-fetch/settings.json` | User-level | Default provider, model, permission mode, vault path |
| `.harness/settings.json` | Project-level | Project-specific overrides |
| `.harness/mcp.json` | Project-level | MCP server definitions |
| `.harness/skills/` | Project-level | Installed skills |
| `.harness/commands/` | Project-level | User-defined custom slash commands |
| `.agentignore` | Project-level | Sandbox exclusion rules |
| `AGENTS.md` / `CLAUDE.md` | Project-level | Agent context injection |
| `SOUL.md` | Project-level | Agent personality/identity injection |
| `.env` | Project-level | API keys (fallback for keychain) |

---

## 6. Data Requirements

### 6.1 Memory Vault Data Model

#### 6.1.1 MemCell (Level 1)

```yaml
# frontmatter
type: memcell
date: YYYY-MM-DD
session_id: string
project: string
tags: [string]
memcell_count: integer
```

Body sections per MemCell: Topic, Context, Actions, Outcome, Keywords, Linked Events, Foresights.

One file per day (`1-memcells/YYYY/MM/YYYY-MM-DD.md`), multiple MemCell sections per file.

#### 6.1.2 Event (Level 2)

```yaml
type: event
id: fact-NNNN
created: ISO 8601 datetime
confidence: float (0.0-1.0)
status: active | inactive | superseded
parent: "[[wikilink]]"
project: string
tags: [string]
```

One file per event (`2-events/fact-NNNN.md`).

#### 6.1.3 Foresight (Level 3)

```yaml
type: foresight
id: pred-NNNN
created: ISO 8601 datetime
status: pending | validated | failed | expired
confidence: float (0.0-1.0)
start_time: ISO 8601 date
end_time: ISO 8601 date
duration_days: integer
parent: "[[wikilink]]"
project: string
tags: [string]
validation_criteria: string
```

One file per foresight (`3-foresights/pred-NNNN.md`).

#### 6.1.4 Episode (Level 4)

```yaml
type: episode
id: ep-NNNN
created: ISO 8601 datetime
subject: string
cluster: "[[wikilink]]"
source_memcells: ["[[wikilink]]"]
project: string
tags: [string]
```

Body: Summary, Key Decisions, Outcomes, Lessons, Related links.

One file per episode (`4-episodes/ep-NNNN.md`).

#### 6.1.5 Profile (Level 5)

```yaml
type: profile
id: agent-profile | user-profile
last_updated: ISO 8601 datetime
version: integer
item_count: integer
```

Body: Explicit Info (factual knowledge) + Implicit Traits (inferred from behavior).

Files: `5-profile/agent-profile.md`, `5-profile/user-profile.md`.

#### 6.1.6 Reflection (Level 6)

```yaml
type: reflection
period: weekly | monthly
week: YYYY-WNN          # for weekly
month: YYYY-MM           # for monthly
date_range: [date, date]
episode_count: integer
tags: [string]
```

Body: Patterns Identified, Decision Weight Adjustments table, Foresight Validation table.

Files: `6-reflections/weekly/YYYY-WNN.md`, `6-reflections/monthly/YYYY-MM.md`.

#### 6.1.7 Cluster

```yaml
type: cluster
id: cluster-NNN
created: ISO 8601 date
centroid_keywords: [string]
memcell_count: integer
last_updated: ISO 8601 date
tags: [string]
```

One file per cluster (`clusters/cluster-NNN.md`).

### 6.2 Session Data

Stored in SQLite via adk-session `SqliteSessionService`. Schema managed by adk-session. Location: `~/.config/momo-fetch/sessions.db`.

### 6.3 Configuration Data

```jsonc
// ~/.config/momo-fetch/settings.json
{
  "default_provider": "anthropic",
  "default_model": "claude-sonnet-4-20250514",
  "permission_mode": "strict",
  "vault_path": "~/.config/momo-fetch/memory-vault",
  "max_cost_daily": 10.00,
  "retrieval_mode": "grep_llm"
}
```

---

## 7. Interfaces

### 7.1 User Interfaces

#### 7.1.1 REPL (primary)

- stdin/stdout via `rustyline`
- Streaming token output
- ANSI color codes for tool calls, errors, success
- Slash command prefix: `/`
- Shell escape prefix: `!`

#### 7.1.2 One-shot (secondary)

- CLI arguments via `clap`
- stdout/stderr separation
- Exit codes: 0 (success), 1 (error)

### 7.2 Software Interfaces

#### 7.2.1 adk-rust framework

| Crate | Interface | Usage |
|-------|-----------|-------|
| `adk-agent` | `LlmAgentBuilder`, `SequentialAgent`, `ParallelAgent` | Agent construction and orchestration |
| `adk-model` | `provider_from_env()`, provider clients | LLM connectivity |
| `adk-runner` | `Launcher`, event streaming, callbacks | Execution and lifecycle |
| `adk-session` | `SqliteSessionService` | Conversation persistence |
| `adk-tool` | `#[tool]` macro, `FunctionTool`, MCP client | Tool system |
| `adk-skill` | SKILL.md parser, matcher | Skill system |
| `adk-sandbox` | Sandbox trait, `ToolConfirmationPolicy` | Safety |
| `adk-cli` | Base REPL | CLI foundation |

#### 7.2.2 External APIs

| API | Protocol | Purpose |
|-----|----------|---------|
| LLM providers | HTTPS (OpenAI-compatible or native) | Inference |
| Serper.dev / DuckDuckGo | HTTPS | Web search |
| MCP servers | stdio or HTTP Streamable | Tool extension |

### 7.3 Hardware Interfaces

None. The system runs on standard consumer hardware.

### 7.4 Communications Interfaces

- stdin/stdout (REPL)
- Filesystem (memory vault, config)
- Network (HTTPS to LLM providers, MCP servers, search APIs)

---

## 8. Requirements Traceability

### 8.1 Functional Requirements → User Stories

| FR ID | User Story | Phase |
|-------|-----------|-------|
| FR-S-01, FR-S-02 | US-001: Scaffold project | 1 |
| FR-P-01, FR-P-02, FR-P-03, FR-P-04 | US-002: Multi-provider LLM | 1 |
| FR-TF-01, FR-TF-02, FR-TF-03 | US-003: File tools | 1 |
| FR-TS-01, FR-TS-02, FR-TS-03 | US-004: Shell + Grep/Glob tools | 1 |
| FR-TW-01, FR-TW-02 | US-005: Web tools | 1 |
| FR-SF-01, FR-SF-02, FR-SF-03, FR-SF-04, FR-SF-05 | US-006: Sandbox & safety | 1 |
| FR-SS-01, FR-SS-02, FR-SS-03, FR-SS-04 | US-007: Session persistence | 1 |
| FR-CR-01, FR-CR-02 | US-008: CLI REPL | 1 |
| FR-CO-01 | US-009: One-shot mode | 1 |
| FR-CX-01, FR-CX-02, FR-CX-03 | US-010: Context injection | 1 |
| FR-MC-01, FR-MC-02, FR-MC-03, FR-MC-04 | US-011: MCP integration | 2 |
| FR-MW-01, FR-MW-02 | US-012: Memory write & extract | 2 |
| FR-MR-01, FR-MR-02, FR-MR-03, FR-MR-04 | US-013: Memory retrieval | 2 |
| FR-ML-01, FR-ML-02, FR-ML-03, FR-ML-04 | US-014: Memory consolidation | 2 |
| FR-MA-01, FR-MA-02, FR-MA-03, FR-MA-04, FR-MA-05 | US-022: Memory auto-flow | 2 |
| FR-SK-01, FR-SK-02, FR-SK-03 | US-015: Skill system | 2 |
| FR-SA-01, FR-SA-02, FR-SA-03, FR-SA-04 | US-016: Sub-agent orchestration | 3 |
| FR-KB-01, FR-KB-02, FR-KB-03 | US-017: Knowledge base | 3 |
| FR-SC-01, FR-SC-02, FR-SC-03 | US-018: Secrets management | 3 |
| NFR-P-01, FR-CR-01 | US-019: Streaming & cancellation | 3 |
| FR-CT-01, FR-CT-02, FR-CT-03, FR-CT-04, FR-CT-05, FR-CT-06 | US-020: Cost tracking | 3 |
| FR-SA-04, FR-MC-01 | US-021: Agent teams | 3 |

### 8.2 Implementation Order

#### Phase 1: Core Agent (Priority 1)

| Step | Story | Depends On |
|------|-------|-----------|
| 1.1 | US-001: Scaffold | — |
| 1.2 | US-002: Providers | 1.1 |
| 1.3 | US-003: File tools | 1.1 |
| 1.4 | US-004: Shell + search tools | 1.1 |
| 1.5 | US-006: Sandbox | 1.3, 1.4 |
| 1.6 | US-007: Session | 1.1 |
| 1.7 | US-008: CLI REPL | 1.2, 1.5, 1.6 |
| 1.8 | US-009: One-shot | 1.7 |
| 1.9 | US-005: Web tools | 1.1 |
| 1.10 | US-010: Context injection | 1.7 |

#### Phase 2: Memory Vault & Intelligence (Priority 2)

| Step | Story | Depends On |
|------|-------|-----------|
| 2.1 | US-011: MCP | Phase 1 |
| 2.2 | US-012: Memory write/extract | Phase 1 |
| 2.3 | US-013: Memory retrieval | 2.2 |
| 2.4 | US-014: Memory consolidation | 2.3 |
| 2.5 | US-022: Memory auto-flow | 2.2 |
| 2.6 | US-015: Skills | Phase 1 |

#### Phase 3: Advanced (Priority 3)

| Step | Story | Depends On |
|------|-------|-----------|
| 3.1 | US-016: Sub-agents | Phase 2 |
| 3.2 | US-017: KMS | Phase 2 |
| 3.3 | US-018: Secrets | Phase 1 |
| 3.4 | US-019: Streaming | 1.7 |
| 3.5 | US-020: Cost tracking | 1.6, 3.4 |
| 3.6 | US-021: Agent teams | 3.1 |

---

## 9. Open Issues

| ID | Question | Status |
|----|----------|--------|
| OI-01 | Memory vault path: relative to project or global (`~/.config/momo-fetch/memory-vault/`)? | Open |
| OI-02 | Should ThaiLLM endpoint have separate support or use OpenAI-compatible preset? | Open |
| OI-03 | Context compression: use adk-gemini's context compaction or custom? | Open |
| OI-04 | adk-cli REPL: extend or replace entirely? Need to review adk-cli source for extensibility. | Open |
| OI-05 | adk-sandbox: sufficient or need custom sandbox layer? | Open |
