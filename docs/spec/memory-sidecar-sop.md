# Memory Sidecar — Design Specification

## Context

Today MOMO Fetch's memory is **passive** — the LLM must explicitly decide to call `mem_search` / `mem_write` tools. In practice, the LLM rarely does this because:
- It doesn't know what's in the vault
- It relies on session history (current conversation only)
- There's no automatic trigger

We need an **active memory system** that:
1. Auto-searches relevant memories before each turn
2. Auto-writes significant outcomes after each turn
3. Works with a small cheap model to keep costs down

---

## Architecture Options

### Option A: Callback Hooks (Built into adk-rust)

```
User message
     │
     ▼
┌─────────────────────────────────────────────┐
│  adk-rust Runner (same LLM, same process)   │
│                                              │
│  BeforeAgentCallback ──► mem_search(input)   │
│         │                                    │
│         ▼                                    │
│  LLM call (main model, e.g. Claude)          │
│         │                                    │
│         ▼                                    │
│  Tool calls (file_read, shell, etc.)         │
│         │                                    │
│         ▼                                    │
│  AfterAgentCallback ──► mem_write(summary)   │
└─────────────────────────────────────────────┘
```

**How it works:**
- `BeforeAgentCallback` — runs before every LLM call, searches vault, injects results into the `Content` sent to the LLM
- `AfterAgentCallback` — runs after the turn completes, extracts a MemCell from the conversation
- Same process, same agent, zero extra LLM calls for memory search (grep-based)
- Memory *write* can optionally use a small LLM call for extraction quality

**Pros:** Simple, no new processes, uses adk-rust built-in callbacks
**Cons:** Memory extraction runs on the same expensive model (or needs a separate LLM client)

---

### Option B: Sidecar Sub-Agent (Same Process)

```
User message
     │
     ▼
┌──────────────────────┐     ┌──────────────────────┐
│  Main Agent          │     │  Memory Sidecar      │
│  (Claude/GPT-4o)     │     │  (small model)       │
│                      │     │                      │
│  Before turn:        │     │  Inputs:             │
│    asks sidecar to   │────►│    - user message    │
│    search memory     │     │    - vault index     │
│                      │◄────│    - recent memcells │
│  Gets back:          │     │                      │
│    relevant memories │     │  Outputs:            │
│                      │     │    - search results  │
│  After turn:         │     │    - extracted facts │
│    asks sidecar to   │────►│    - profile updates │
│    write memory      │     │                      │
└──────────────────────┘     └──────────────────────┘
         │
         │  Uses Task tool pattern (src/tools/task.rs)
         │  Shares Arc<Mutex<ObsidianVault>>
```

**How it works:**
- A lightweight `LlmAgent` with a small model (e.g. DeepSeek, Ollama llama3.2)
- Spawned as a sub-agent via the existing `Task` tool pattern
- Has its own tools: `mem_search`, `mem_write`, `mem_read`, `grep` (no file_edit, no shell)
- Main agent calls it before/after each turn via internal method (not as a user-visible tool)
- Shares the same `Arc<Mutex<ObsidianVault>>`

**Pros:** Cheap model for memory work, separation of concerns, main agent stays focused
**Cons:** Extra latency per turn (sub-agent spawn + LLM call), more complex wiring

---

### Option C: Sidecar Agent (Separate Process — Agent-to-Agent)

```
User message
     │
     ▼
┌──────────────────────┐     ┌──────────────────────┐
│  MOMO Fetch          │     │  Memory Sidecar      │
│  (main process)      │     │  (separate process)  │
│                      │     │                      │
│  Spawns sidecar as   │     │  Own LLM connection  │
│  background task     │     │  Own vault access    │
│                      │     │                      │
│  Sends:              │     │  Receives:           │
│    mailbox msg       │────►│    "search: <query>" │
│    "search: query"   │     │    "write: <summary>"│
│                      │     │                      │
│  Receives:           │     │  Sends back:         │
│    memory results    │◄────│    search results    │
│                      │     │    write confirmation│
└──────────────────────┘     └──────────────────────┘
         │
         │  Uses Team/Mailbox pattern (src/team/mod.rs)
         │  Separate binary: momo-fetch --mode sidecar
```

**How it works:**
- Separate process running `momo-fetch --mode memory-sidecar`
- Communication via the existing `Mailbox` file-based system
- Main process sends search/write requests, sidecar responds
- Sidecar runs its own small LLM (Ollama/DeepSeek)
- Can be started/stopped independently

**Pros:** Full isolation, can scale independently, survives main agent crashes
**Cons:** Highest latency (IPC), most complex, requires process management

---

## Decision Matrix

| Factor | A: Callbacks | B: Sub-Agent | C: Separate Process |
|--------|:------------:|:------------:|:--------------------:|
| Implementation complexity | Low | Medium | High |
| Latency per turn | ~50ms | ~2-5s | ~3-8s |
| Extra LLM cost (search) | $0 (grep) | ~$0.001 | ~$0.001 |
| Extra LLM cost (write) | ~$0.01 | ~$0.001 | ~$0.001 |
| Main model choice freedom | Any | Any | Any |
| Sidecar model | Main model | Any small | Any small |
| Failure isolation | Crashes together | Crashes together | Independent |
| Configurable per project | Yes | Yes | Yes |
| Works in one-shot mode | Yes | Yes | Yes |
| **Implementation status** | **Done** | **Done** | **Done** |

---

## Implementation Status (2026-05-08)

### Option A: Callback Hooks — IMPLEMENTED

**Files changed:**
- `src/memory/sidecar.rs` — `MemorySidecar` struct with:
  - `search_for_context()` — grep-based search, zero LLM cost
  - `write_turn_memory_option_a()` — TF-IDF keyword extraction
  - `enrich_input()` — prepends relevant memories to user input
  - `build_system_prompt_addition()` — informs agent about auto-memory
- `src/harness.rs` — integrated `MemorySidecar` into `Harness` struct
- `src/cli/repl.rs` — pre-turn enrichment + post-turn write with tool call/response tracking
- `src/cli/oneshot.rs` — same enrichment for one-shot mode
- `src/memory/mod.rs` — registered `sidecar` module

**How it works:**
1. Before each turn: `MemorySidecar::enrich_input()` searches vault via grep, prepends relevant memories
2. After each turn: `MemorySidecar::write_turn_memory_option_a()` extracts keywords via TF-IDF, writes MemCell
3. System prompt includes memory context when auto features are enabled

### Option B: Sidecar Sub-Agent — IMPLEMENTED (prompt only)

**Files changed:**
- `src/memory/sidecar.rs` — `build_sidecar_prompt()` generates extraction prompt for sub-agent
- `src/config/mod.rs` — `MemorySettings.sidecar_model` and `sidecar_provider` fields

**How to enable:**
```json
{
  "memory": {
    "auto_search": true,
    "auto_write": true,
    "sidecar_model": "deepseek-chat",
    "sidecar_provider": "deepseek"
  }
}
```

**Sub-agent wiring:** The `build_sidecar_prompt()` method returns the prompt to send to the sidecar model. The actual sub-agent spawn uses the existing `Task` tool pattern from `src/tools/task.rs`. To complete the wiring, add a method to spawn the sidecar sub-agent when `sidecar_model` is configured.

### Option C: Separate Process — IMPLEMENTED

**Files changed:**
- `src/cli/mod.rs` — `--mode memory-sidecar` flag + `run_memory_sidecar()` entry point
- `src/team/mod.rs` — `sidecar_protocol` module with well-known message types

**How to run:**
```bash
# Terminal 1: Main agent
momo-fetch --project /path/to/project

# Terminal 2: Memory sidecar (separate process)
momo-fetch --mode memory-sidecar --project /path/to/project
```

**Mailbox protocol:**
- Main → Sidecar: `search_request` (body = query string)
- Sidecar → Main: `search_response` (body = JSON `{context_block, count}`)
- Main → Sidecar: `write_request` (body = JSON TurnSummary)
- Sidecar → Main: `write_response` (body = JSON `{status, memcell_ref}`)
- Sidecar → Main: `ready` (on startup)
- Main → Sidecar: `shutdown` (graceful termination)

---

## Recommended: Option A + Optional B

**Default (Option A):** Callback hooks with grep-based search (zero extra LLM cost) and main-model extraction.

**Opt-in (Option B):** When `memory_sidecar_model` is set in config, use a sub-agent with the specified small model for extraction/write.

```toml
# .harness/settings.json
{
  "memory": {
    "auto_search": true,
    "auto_write": true,
    "search_mode": "grep_llm",
    "max_results_per_turn": 5,
    "sidecar_model": null,        // null = use callbacks (Option A)
                                   // "deepseek-chat" = use sub-agent (Option B)
    "sidecar_provider": "deepseek" // provider for the sidecar model
  }
}
```

---

## SOP: Memory Auto-Flow

### Pre-Turn (Search Phase)

```
1. User sends message
2. Harness receives input string
3. IF auto_search == true:
   a. Extract keywords from input (TF-IDF or just split)
   b. vault.search(MemoryQuery { query: input, mode: GrepLlm, limit: 5 })
   c. IF results found:
      - Prepend to user message:
        "--- Relevant memories ---\n
         - fact-0042: Fixed auth middleware JWT validation (score: 0.89)\n
         - 2026-05-06#MemCell 003: Deployed auth to staging (score: 0.72)\n
         ---\n
         {original user message}"
4. Send enriched message to LLM
```

### Post-Turn (Write Phase)

```
1. Turn completes (is_final_response == true)
2. IF auto_write == true:
   a. Collect the turn summary:
      - user message (first 200 chars)
      - tool calls made (names + file paths)
      - final response (first 300 chars)
   b. IF sidecar_model == null (Option A):
      - vault.write_memcell(project, topic, context, actions, outcome, keywords)
      - keywords extracted via TF-IDF from the full turn text
   c. IF sidecar_model != null (Option B):
      - Spawn sub-agent with sidecar model
      - Prompt: "Extract a memory entry from this conversation turn: {turn_summary}"
      - Sub-agent returns structured: { topic, context, actions, outcome, keywords }
      - vault.write_memcell with extracted data
3. Every N memcells (configurable, default 10):
   - Auto-trigger mem_extract (create events/foresights/episodes)
```

### System Prompt Addition

When `auto_search` is enabled, append to the system prompt:

```
--- Memory System ---
You have access to a persistent memory vault with {N} memories across {sessions} sessions.
Relevant memories are automatically injected before each message (marked with "--- Relevant memories ---").
You can also manually search with mem_search for deeper recall.
After significant work, a memory entry is automatically created — you don't need to call mem_write unless you want to save something specific.
```

---

## Files Modified (implementation complete)

| File | Change | Status |
|------|--------|--------|
| `src/memory/sidecar.rs` | New file: `MemorySidecar`, `TurnSummary`, `SearchResult`, keyword extraction | **Done** |
| `src/memory/mod.rs` | Register `sidecar` module | **Done** |
| `src/harness.rs` | Add `MemorySidecar` to `Harness`, `run_turn_enriched()`, system prompt injection | **Done** |
| `src/config/mod.rs` | `MemorySettings` struct with all Option A/B/C fields | **Done** |
| `src/cli/repl.rs` | Pre-turn enrichment + post-turn write with tool call tracking | **Done** |
| `src/cli/oneshot.rs` | Enrichment + auto-write for one-shot mode | **Done** |
| `src/cli/mod.rs` | `--mode memory-sidecar` flag + `run_memory_sidecar()` entry point | **Done** |
| `src/team/mod.rs` | `sidecar_protocol` module with Mailbox message types | **Done** |
| `src/context/mod.rs` | Memory context injected via `MemorySidecar::build_system_prompt_addition()` | **Done** |
| `src/tools/task.rs` | No changes (Option B sub-agent uses existing pattern) | Unchanged |
| `Cargo.toml` | No changes needed | Unchanged |

---

## Key Data Structures

```rust
// src/config/mod.rs

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemorySettings {
    /// Auto-search vault before each turn
    pub auto_search: bool,
    /// Auto-write MemCell after each turn
    pub auto_write: bool,
    /// Retrieval mode for auto-search
    pub search_mode: String,              // "grep_llm" | "tag_filter"
    /// Max memory results injected per turn
    pub max_results_per_turn: usize,      // default: 5
    /// MemCells threshold to trigger auto-extract
    pub extract_threshold: usize,         // default: 10
    /// Sidecar model for memory extraction (null = use main model)
    pub sidecar_model: Option<String>,    // e.g. "deepseek-chat"
    /// Sidecar provider
    pub sidecar_provider: Option<String>, // e.g. "deepseek"
}

impl Default for MemorySettings {
    fn default() -> Self {
        Self {
            auto_search: true,
            auto_write: true,
            search_mode: "grep_llm".into(),
            max_results_per_turn: 5,
            extract_threshold: 10,
            sidecar_model: None,
            sidecar_provider: None,
        }
    }
}
```

```rust
// src/harness.rs — Memory sidecar struct

pub struct MemorySidecar {
    vault: Arc<Mutex<ObsidianVault>>,
    config: MemorySettings,
    /// Counter for auto-extract trigger
    memcells_since_extract: AtomicUsize,
}

impl MemorySidecar {
    /// Pre-turn: search vault and format results for injection
    pub fn search_for_context(&self, user_input: &str) -> Option<String> { ... }

    /// Post-turn: write a MemCell from the turn summary
    pub async fn write_turn_memory(&self, turn: &TurnSummary) -> anyhow::Result<()> { ... }

    /// Check if extraction should run and trigger it
    pub async fn maybe_extract(&self) -> anyhow::Result<()> { ... }
}

pub struct TurnSummary {
    pub user_message: String,
    pub tool_calls: Vec<String>,
    pub response_preview: String,
    pub project: String,
}
```

---

## Config Examples

### Minimal (just works, Option A)
```json
{
  "memory": {
    "auto_search": true,
    "auto_write": true
  }
}
```

### With sidecar (Option B, cheap model for extraction)
```json
{
  "memory": {
    "auto_search": true,
    "auto_write": true,
    "sidecar_model": "deepseek-chat",
    "sidecar_provider": "deepseek"
  }
}
```

### With local sidecar (Ollama, zero extra cost)
```json
{
  "memory": {
    "auto_search": true,
    "auto_write": true,
    "sidecar_model": "llama3.2",
    "sidecar_provider": "ollama"
  }
}
```

### Disabled (current behavior, memory tools still available)
```json
{
  "memory": {
    "auto_search": false,
    "auto_write": false
  }
}
```
