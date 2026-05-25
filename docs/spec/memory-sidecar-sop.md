# Memory Sidecar — Design Specification

## Context

Today MOMO Fetch's memory is **passive** — the LLM must explicitly decide to call `mem_search` / `mem_write` tools. In practice, the LLM rarely does this because:
- It doesn't know what's in the vault
- It relies on session history (current conversation only)
- There's no automatic trigger

We need an **active memory system** that:
1. Auto-searches relevant memories before each turn
2. Auto-writes significant outcomes after each turn
3. Auto-extracts events/foresights/episodes at configurable thresholds
4. Works with a small cheap model to keep costs down (optional)

---

## Architecture Options

### Option A: Callback Hooks (Built into adk-rust) — DEFAULT

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
- Memory *write* uses TF-IDF keyword extraction (no extra LLM call)

**Pros:** Simple, no new processes, uses adk-rust built-in callbacks
**Cons:** TF-IDF extraction is less sophisticated than LLM-based extraction

---

### Option B: Sidecar LLM Call (Same Process)

```
User message
     │
     ▼
┌──────────────────────┐     ┌──────────────────────┐
│  Main Agent          │     │  Sidecar LLM Call    │
│  (Claude/GPT-4o)     │     │  (small model)       │
│                      │     │                      │
│  After turn:         │     │  Inputs:             │
│    sends prompt to   │────►│    - turn summary    │
│    sidecar model     │     │                      │
│                      │◄────│  Outputs:            │
│  Gets back:          │     │    - JSON: topic,    │
│    extracted memory  │     │      context, actions│
│    data (JSON)       │     │      outcome,        │
│                      │     │      keywords        │
└──────────────────────┘     └──────────────────────┘
         │
         │  Direct LLM call (no agent loop)
         │  Shares Arc<Mutex<ObsidianVault>>
```

**How it works:**
- A direct LLM call to the configured sidecar model (no Runner, no agent loop)
- `ProviderManager::create_model()` constructs an `Arc<dyn Llm>` for the sidecar provider/model pair
- `MemorySidecar.build_sidecar_prompt()` generates the extraction prompt
- Response is parsed as JSON into `SidecarExtraction` struct
- Falls back to Option A if JSON parsing fails or LLM call errors

**Pros:** Better extraction quality, cheap model for memory work, separation of concerns
**Cons:** Extra latency per turn (~1-3s), requires API key for sidecar provider

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
- Sidecar runs its own LLM (or TF-IDF if no model configured)
- Can be started/stopped independently

**Pros:** Full isolation, can scale independently, survives main agent crashes
**Cons:** Highest latency (IPC), most complex, requires process management

---

## Decision Matrix

| Factor | A: Callbacks | B: Sidecar LLM | C: Separate Process |
|--------|:------------:|:--------------:|:--------------------:|
| Implementation complexity | Low | Medium | High |
| Latency per turn | ~50ms | ~1-3s | ~3-8s |
| Extra LLM cost (search) | $0 (grep) | $0 (grep) | $0 (grep) |
| Extra LLM cost (write) | $0 (TF-IDF) | ~$0.001 | ~$0.001 |
| Extraction quality | TF-IDF | LLM-based | LLM or TF-IDF |
| Failure isolation | Crashes together | Crashes together | Independent |
| Configurable per project | Yes | Yes | Yes |
| Works in one-shot mode | Yes | Yes | Yes |
| **Implementation status** | **Done** | **Done** | **Done** |

---

## Implementation Status (2026-05-25)

### Option A: Callback Hooks — FULLY IMPLEMENTED

**Files changed:**
- `src/memory/sidecar.rs` — `MemorySidecar` struct with:
  - `search_for_context()` — grep-based search, zero LLM cost
  - `write_turn_memory_option_a()` — TF-IDF keyword extraction
  - `enrich_input()` — prepends relevant memories to user input
  - `build_system_prompt_addition()` — informs agent about auto-memory
- `src/harness.rs` — integrated `MemorySidecar` into `Harness` struct
- `src/cli/repl.rs` — pre-turn enrichment + post-turn unified dispatch
- `src/cli/oneshot.rs` — same enrichment for one-shot mode
- `src/memory/mod.rs` — registered `sidecar` module

**How it works:**
1. Before each turn: `MemorySidecar::enrich_input()` searches vault via grep, prepends relevant memories
2. After each turn: `write_turn_memory()` dispatches to Option A (TF-IDF) when no sidecar configured
3. System prompt includes memory context when auto features are enabled

### Option B: Sidecar LLM Call — FULLY IMPLEMENTED

**Files changed:**
- `src/memory/sidecar.rs` — `write_turn_memory_option_b()` with:
  - Lazy-initialized `OnceLock<Arc<dyn Llm>>` for sidecar model
  - Direct LLM call via `tokio::runtime::Handle::block_on()`
  - `SidecarExtraction` struct for JSON response parsing
  - `strip_json_fences()` helper for markdown-wrapped responses
  - Automatic fallback to Option A on parse/LLM errors
- `src/providers.rs` — `create_model()` made public for sidecar LLM construction
- `src/config/mod.rs` — `MemorySettings.sidecar_model` and `sidecar_provider` fields

**How to enable:**
```json
{
  "memory": {
    "sidecar_model": "deepseek-chat",
    "sidecar_provider": "deepseek"
  }
}
```

**How it works:**
1. On first Option B write, lazy-initializes the sidecar LLM via `ProviderManager::create_model()`
2. Builds extraction prompt via `build_sidecar_prompt()`
3. Makes a direct LLM call (no agent loop, no tools)
4. Parses JSON response into `SidecarExtraction` (with markdown fence stripping)
5. Falls back to Option A if anything fails

### Option C: Separate Process — FULLY IMPLEMENTED

**Files changed:**
- `src/cli/mod.rs` — `--mode memory-sidecar` flag + `run_memory_sidecar()` entry point
- `src/team/mod.rs` — `sidecar_protocol` module with well-known message types
- `src/memory/sidecar.rs` — `write_turn_memory_option_c()` with Mailbox dispatch + fallback

**How to run:**
```bash
# Terminal 1: Main agent
momo-fetch --project /path/to/project

# Terminal 2: Memory sidecar (separate process)
momo-fetch --mode memory-sidecar --project /path/to/project
```

**How it works:**
1. Main process peeks Mailbox for `ready` signal from sidecar process
2. If sidecar is alive, sends `WRITE_REQUEST` via Mailbox
3. Polls for `WRITE_RESPONSE` (5s timeout, 50ms intervals)
4. Falls back to Option A on timeout or error

**Mailbox protocol:**
- Main → Sidecar: `search_request` (body = query string)
- Sidecar → Main: `search_response` (body = JSON `{context_block, count}`)
- Main → Sidecar: `write_request` (body = JSON TurnSummary)
- Sidecar → Main: `write_response` (body = JSON `{status, memcell_ref}`)
- Sidecar → Main: `ready` (on startup)
- Main → Sidecar: `shutdown` (graceful termination)

### Auto-Extract / Auto-Consolidate — FULLY IMPLEMENTED

**Files changed:**
- `src/memory/sidecar.rs` — `check_thresholds()` method
- `src/config/mod.rs` — `consolidate_threshold` field added to `MemorySettings`

**How it works:**
1. After every MemCell write, increments `memcells_since_extract` counter
2. When counter reaches `extract_threshold` (default: 10):
   - Calls `vault.extract_from_memcell()` → creates events, foresights, episodes
   - Resets counter
3. When counter reaches `consolidate_threshold` (default: 30):
   - Calls `vault.consolidate()` → creates clusters, updates profiles
4. All errors are logged but non-fatal (turn continues normally)

---

## Recommended: Option A + Optional B/C

**Default (Option A):** TF-IDF callback hooks with grep-based search (zero extra LLM cost).

**Opt-in (Option B):** When `sidecar_model` is set in config, direct LLM call to a small model for extraction.

**Opt-in (Option C):** When a sidecar process is detected via Mailbox, route through IPC.

```json
// .harness/settings.json
{
  "memory": {
    "auto_search": true,
    "auto_write": true,
    "search_mode": "grep_llm",
    "max_results_per_turn": 5,
    "extract_threshold": 10,          // auto-extract every 10 MemCells
    "consolidate_threshold": 30,      // auto-consolidate every 30 MemCells
    "sidecar_model": null,            // null = Option A (TF-IDF, $0)
                                      // "deepseek-chat" = Option B (LLM call)
    "sidecar_provider": "deepseek"    // provider for the sidecar model
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
        "--- Relevant memories ---
         - fact-0042: Fixed auth middleware JWT validation (score: 0.89)
         - 2026-05-06#MemCell 003: Deployed auth to staging (score: 0.72)
         ---
         {original user message}"
4. Send enriched message to LLM
```

### Post-Turn (Write Phase) — Unified Dispatch

```
1. Turn completes (is_final_response == true)
2. IF auto_write == true:
   a. Collect the turn summary:
      - user message (first 200 chars)
      - tool calls made (names + file paths)
      - final response (first 300 chars)
   b. write_turn_memory(turn, provider_mgr, mailbox_path):
      ┌─ Option C: Mailbox sidecar process alive?
      │   YES → send WRITE_REQUEST, poll WRITE_RESPONSE (5s timeout)
      │   FAIL/TIMEOUT → fallback to Option A
      │
      ├─ Option B: sidecar_model configured?
      │   YES → direct LLM call → parse JSON → write_memcell
      │   PARSE/LLM FAIL → fallback to Option A
      │
      └─ Option A (default):
          TF-IDF keywords → write_memcell
3. After write, check thresholds:
   a. memcells_since_extract >= extract_threshold (default 10):
      → vault.extract_from_memcell() → events + foresights + episodes
   b. memcells_since_extract >= consolidate_threshold (default 30):
      → vault.consolidate() → clusters + profiles
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

## Files Modified (all options implemented)

| File | Change | Status |
|------|--------|--------|
| `src/memory/sidecar.rs` | `MemorySidecar`, `TurnSummary`, `SearchResult`, unified dispatch, Option B/C, auto-extract/consolidate | **Done** |
| `src/memory/mod.rs` | Register `sidecar` module | **Done** |
| `src/harness.rs` | `MemorySidecar` in `Harness`, `mailbox_path()` accessor | **Done** |
| `src/config/mod.rs` | `MemorySettings` with all Option A/B/C fields + `consolidate_threshold` | **Done** |
| `src/providers.rs` | `create_model()` made public for sidecar LLM construction | **Done** |
| `src/cli/repl.rs` | Pre-turn enrichment + post-turn unified dispatch | **Done** |
| `src/cli/oneshot.rs` | Enrichment + unified dispatch for one-shot mode | **Done** |
| `src/cli/mod.rs` | `--mode memory-sidecar` flag + `run_memory_sidecar()` entry point | **Done** |
| `src/team/mod.rs` | `sidecar_protocol` module with Mailbox message types | **Done** |
| `src/context/mod.rs` | Memory context injected via `MemorySidecar::build_system_prompt_addition()` | **Done** |

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
    /// MemCells threshold to trigger auto-extract (events/foresights/episodes)
    pub extract_threshold: usize,         // default: 10
    /// Sidecar model for memory extraction (null = TF-IDF Option A)
    pub sidecar_model: Option<String>,    // e.g. "deepseek-chat"
    /// Sidecar provider (used when sidecar_model is set)
    pub sidecar_provider: Option<String>, // e.g. "deepseek"
    /// MemCells threshold to trigger auto-consolidate (clusters + profile)
    pub consolidate_threshold: usize,     // default: 30, 0 = disabled
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
            consolidate_threshold: 30,
        }
    }
}
```

```rust
// src/memory/sidecar.rs — MemorySidecar struct

pub struct MemorySidecar {
    vault: Arc<Mutex<ObsidianVault>>,
    config: MemorySettings,
    /// Counter for auto-extract trigger
    memcells_since_extract: AtomicUsize,
    /// Lazy-initialized sidecar LLM for Option B
    sidecar_llm: OnceLock<Arc<dyn Llm>>,
}

impl MemorySidecar {
    /// Pre-turn: search vault and format results for injection
    pub fn search_for_context(&self, user_input: &str) -> SearchResult { ... }

    /// Unified dispatch: Option C → Option B → Option A
    pub fn write_turn_memory(
        &self, turn: &TurnSummary,
        provider_mgr: &ProviderManager,
        mailbox_path: Option<&Path>,
    ) -> anyhow::Result<String> { ... }

    /// Check auto-extract and auto-consolidate thresholds
    fn check_thresholds(&self, memcell_ref: &str, turn: &TurnSummary) { ... }
}

/// Parsed JSON from Option B sidecar LLM
struct SidecarExtraction {
    topic: String, context: String,
    actions: Vec<ActionRecord>, outcome: String, keywords: Vec<String>,
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

### With auto-extract and auto-consolidate tuning
```json
{
  "memory": {
    "auto_search": true,
    "auto_write": true,
    "extract_threshold": 5,
    "consolidate_threshold": 20
  }
}
```

### Disabled (memory tools still available manually)
```json
{
  "memory": {
    "auto_search": false,
    "auto_write": false
  }
}
```
