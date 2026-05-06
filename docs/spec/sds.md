# Software Design Specification (SDS)
# Agent Harness — Rust-Native AI Agent Workspace

| Field | Value |
|-------|-------|
| **Version** | 1.0.0 |
| **Date** | 2026-05-06 |
| **Status** | Draft |
| **Source PRD** | `docs/prd/prd-agent-harness.md` |
| **Source SRS** | `docs/prd/srs.md` |
| **Framework** | adk-rust v0.7.0 (zavora-ai/adk-rust) |
| **Rust Edition** | 2024 (rust-version 1.85.0) |

---

## Table of Contents

1. [Technology Stack](#1-technology-stack)
2. [System Architecture](#2-system-architecture)
3. [Component Design](#3-component-design)
4. [Data Architecture](#4-data-architecture)
5. [Interface Design](#5-interface-design)
6. [Security Architecture](#6-security-architecture)
7. [Error Handling Strategy](#7-error-handling-strategy)
8. [Testing Strategy](#8-testing-strategy)
9. [Build & Deployment](#9-build--deployment)
10. [Performance Engineering](#10-performance-engineering)
11. [Code Standards & Conventions](#11-code-standards--conventions)
12. [Risk Assessment](#12-risk-assessment)

---

## 1. Technology Stack

### 1.1 Language & Runtime

| Component | Version | Notes |
|-----------|---------|-------|
| Rust | 1.85.0+ (stable) | Edition 2024 |
| Tokio | 1.52.x | Async runtime, `features = ["full"]` |
| Cargo | Latest stable | Build tool |

**Why Rust:** Memory safety without GC, zero-cost abstractions, async ecosystem maturity, pattern matching for tool routing, trait system for provider abstraction.

### 1.2 Framework — adk-rust

The entire agent layer is built on adk-rust v0.7.0 (zavora-ai), a production framework with 30 crates:

| Crate | Version | Purpose | Integration Point |
|-------|---------|---------|-------------------|
| `adk-agent` | 0.7.0 | `LlmAgentBuilder`, `SequentialAgent`, `ParallelAgent` | Core agent construction |
| `adk-model` | 0.7.0 | 17+ LLM providers, `Llm` trait | Multi-provider connectivity |
| `adk-runner` | 0.7.0 | `Runner`, event streaming, callbacks | Execution lifecycle |
| `adk-session` | 0.7.0 | `SqliteSessionService` | Conversation persistence |
| `adk-tool` | 0.7.0 | `#[tool]` macro, `McpToolset`, `FunctionTool` | Tool system + MCP |
| `adk-skill` | 0.7.0 | SKILL.md parsing, `SkillIndex`, selection | Skill system |
| `adk-sandbox` | 0.7.0 | `SandboxBackend`, `ToolConfirmationPolicy` | Code isolation + HITL |
| `adk-core` | 0.7.0 | `Content`, `Part`, `Event`, `Tool`, `Agent` traits | Shared types |
| `adk-cli` | 0.7.0 | Base REPL | CLI foundation |

**Dependency declaration:**
```toml
[dependencies]
adk-rust = { version = "0.7.0", features = [
    "openai", "anthropic", "deepseek", "ollama", "groq", "openrouter"
] }
```

### 1.3 CLI & Terminal

| Crate | Version | Purpose |
|-------|---------|---------|
| `clap` | 4.6.x | CLI argument parsing (`features = ["derive"]`) |
| `rustyline` | 18.0.0 | Interactive REPL (history, completion, hints) |
| `colored` | 2.2.x | ANSI color output |
| `crossterm` | 0.28.x | Terminal manipulation (if needed beyond rustyline) |

### 1.4 Memory Vault — Parsing & Storage

| Crate | Version | Purpose |
|-------|---------|---------|
| `serde` | 1.0.228 | Serialization framework (`features = ["derive"]`) |
| `serde_json` | 1.0.149 | JSON serialization |
| `serde_yaml` | 0.9.x | YAML frontmatter parsing |
| `pulldown-cmark` | 0.13.x | Markdown parsing for wikilink extraction |
| `regex` | 1.11.x | Pattern matching for grep, destructive detection |
| `chrono` | 0.4.x | Date/time handling |
| `uuid` | 1.x | Note ID generation (v4) |

### 1.5 Filesystem & Sandbox

| Crate | Version | Purpose |
|-------|---------|---------|
| `ignore` | 0.4.25 | `.agentignore` / `.gitignore` filtering (ripgrep ecosystem) |
| `walkdir` | 2.5.x | Directory traversal for context discovery |
| `path-clean` | 0.1.x | Path normalization and traversal prevention |
| `tempfile` | 3.x | Atomic writes (write to temp, then rename) |

### 1.6 Networking

| Crate | Version | Purpose |
|-------|---------|---------|
| `reqwest` | 0.13.x | HTTP client (`features = ["json", "rustls-tls", "stream"]`) |
| `tokio-tungstenite` | 0.26.x | WebSocket support (if needed for MCP HTTP) |

**Note:** `reqwest` 0.13 uses `rustls` + `aws-lc` as default TLS. No `native-tls` dependency.

### 1.7 Secrets & Configuration

| Crate | Version | Purpose |
|-------|---------|---------|
| `keyring-core` | 1.0.x | OS keychain access (macOS Keychain, Windows Credential Manager, Linux Secret Service) |
| `directories` | 6.0.x | XDG-compliant config paths |
| `dotenvy` | 0.15.x | `.env` file loading (fallback for secrets) |

### 1.8 Observability

| Crate | Version | Purpose |
|-------|---------|---------|
| `tracing` | 0.1.x | Structured logging facade |
| `tracing-subscriber` | 0.3.x | Log output formatting |
| `tracing-appender` | 0.2.x | Log file rotation |

### 1.9 Testing

| Crate | Version | Purpose |
|-------|---------|---------|
| `tokio-test` | 0.4.x | Async test utilities |
| `assert_cmd` | 2.x | CLI integration testing |
| `tempfile` | 3.x | Test fixtures |
| `wiremock` | 0.6.x | HTTP mock server (for web tools, provider tests) |
| `mockall` | 0.13.x | Trait mocking (for trait-based testing) |

### 1.10 Complete Cargo.toml

```toml
[package]
name = "agent-harness"
version = "0.1.0"
edition = "2024"
rust-version = "1.85.0"
description = "Rust-native AI agent workspace with EverMemOS memory vault"
license = "MIT"

[dependencies]
# Agent framework
adk-rust = { version = "0.7.0", features = [
    "openai", "anthropic", "deepseek", "ollama", "groq", "openrouter"
] }

# Async runtime
tokio = { version = "1", features = ["full"] }

# CLI
clap = { version = "4", features = ["derive"] }
rustyline = "18"
colored = "2"

# Serialization
serde = { version = "1", features = ["derive"] }
serde_json = "1"
serde_yaml = "0.9"

# Memory vault
pulldown-cmark = "0.13"
regex = "1"
chrono = { version = "0.4", features = ["serde"] }
uuid = { version = "1", features = ["v4", "serde"] }

# Filesystem
ignore = "0.4"
walkdir = "2"
path-clean = "0.1"
tempfile = "3"

# Networking
reqwest = { version = "0.13", features = ["json", "rustls-tls"] }

# Configuration & secrets
keyring-core = "1.0"
directories = "6"
dotenvy = "0.15"

# Observability
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter", "json"] }
tracing-appender = "0.2"

[dev-dependencies]
tokio-test = "0.4"
assert_cmd = "2"
tempfile = "3"
wiremock = "0.6"
mockall = "0.13"

[profile.release]
lto = true
codegen-units = 1
strip = true

[profile.dev]
opt-level = 1  # Faster incremental builds
```

---

## 2. System Architecture

### 2.1 High-Level Architecture

```
┌─────────────────────────────────────────────────────────────────────┐
│                        USER INTERFACE LAYER                         │
│  ┌──────────────────┐  ┌──────────────────┐  ┌─────────────────┐   │
│  │    REPL Mode      │  │  One-Shot Mode   │  │   Team Mode     │   │
│  │   (rustyline)     │  │   (clap -p)      │  │  (future)       │   │
│  │                   │  │                   │  │                 │   │
│  │  Slash commands   │  │  Stdout response  │  │  Lead + workers │   │
│  │  !shell escape    │  │  Exit codes       │  │  A2A protocol   │   │
│  │  Streaming output │  │  Stdin pipe       │  │                 │   │
│  └────────┬──────────┘  └────────┬──────────┘  └────────┬────────┘   │
└───────────┼─────────────────────┼──────────────────────┼────────────┘
            │                     │                      │
            ▼                     ▼                      ▼
┌─────────────────────────────────────────────────────────────────────┐
│                       HARNESS ORCHESTRATOR                           │
│                                                                     │
│  ┌──────────────┐  ┌────────────────────┐  ┌────────────────────┐  │
│  │              │  │  ContextBuilder    │  │                    │  │
│  │  Provider    │  │  ├─ AGENTS.md      │  │   CostTracker      │  │
│  │  Manager     │  │  ├─ Skill loader   │  │   ├─ Token counts  │  │
│  │  ├─ model()  │  │  ├─ KMS injector   │  │   ├─ Cost calc     │  │
│  │  ├─ switch() │  │  └─ Memory inject  │  │   └─ Budget alert  │  │
│  │  └─ list()   │  │                    │  │                    │  │
│  │              │  └────────────────────┘  └────────────────────┘  │
│  └──────────────┘                                                   │
│                                                                     │
│  ┌──────────────┐  ┌────────────────────────────────────────────┐  │
│  │  Filesystem  │  │             TOOL REGISTRY                   │  │
│  │  Sandbox     │  │                                            │  │
│  │  ├─ scope()  │  │  ┌──────────────┐  ┌──────────────────┐  │  │
│  │  ├─ ignore() │  │  │ Built-in     │  │ Memory Vault     │  │  │
│  │  └─ approve()│  │  │ ├─ FileRead  │  │ ├─ mem_write      │  │  │
│  │              │  │  │ ├─ FileWrite │  │ ├─ mem_extract    │  │  │
│  └──────────────┘  │  │ ├─ FileEdit  │  │ ├─ mem_search     │  │  │
│                    │  │ ├─ ShellExec │  │ ├─ mem_read       │  │  │
│                    │  │ ├─ Grep      │  │ ├─ mem_graph      │  │  │
│                    │  │ ├─ Glob      │  │ ├─ mem_consolidate│  │  │
│                    │  │ ├─ WebSearch │  │ ├─ mem_reflect    │  │  │
│                    │  │ └─ WebFetch  │  │ └─ mem_stats      │  │  │
│                    │  └──────────────┘  └──────────────────┘  │  │
│                    │                                            │  │
│                    │  ┌──────────────────────────────────────┐│  │
│                    │  │ MCP Toolsets (adk-tool McpToolset)   ││  │
│                    │  │ + Skill Tools (adk-skill SkillIndex)  ││  │
│                    │  └──────────────────────────────────────┘│  │
│                    └────────────────────────────────────────────┘  │
└────────────────────────────────┬────────────────────────────────────┘
                                 │
┌────────────────────────────────▼────────────────────────────────────┐
│                     ADK-RUST FRAMEWORK                              │
│                                                                     │
│  ┌──────────┐ ┌──────────┐ ┌──────────┐ ┌──────────┐ ┌──────────┐ │
│  │adk-agent │ │adk-model │ │adk-runner│ │adk-session│ │adk-tool  │ │
│  │LlmAgent  │ │Gemini    │ │Runner    │ │SQLite    │ │#[tool]   │ │
│  │Sequential│ │OpenAI    │ │Events    │ │Sessions  │ │MCP client│ │
│  │Parallel  │ │Anthropic │ │Callbacks │ │State     │ │FunctionT │ │
│  │Loop      │ │DeepSeek  │ │Streaming │ │CRUD      │ │Toolset   │ │
│  │Transfer  │ │Groq      │ │Interrupt │ │          │ │          │ │
│  └──────────┘ │Ollama    │ │          │ │          │ │          │ │
│  ┌──────────┐ │OpenRouter│ │          │ │          │ │          │ │
│  │adk-skill │ │Compatible│ │          │ │          │ │          │ │
│  │SKILL.md  │ │          │ │          │ │          │ │          │ │
│  │SkillIndex│ │          │ │          │ │          │ │          │ │
│  └──────────┘ └──────────┘ └──────────┘ └──────────┘ └──────────┘ │
│  ┌──────────┐ ┌──────────┐                                        │
│  │adk-sandbox│ │adk-core  │                                        │
│  │Sandbox   │ │Content   │                                        │
│  │HITL      │ │Part      │                                        │
│  │Policy    │ │Event     │                                        │
│  └──────────┘ │LlmReq    │                                        │
│               │LlmResp   │                                        │
│               └──────────┘                                        │
└────────────────────────────────┬───────────────────────────────────┘
                                 │
┌────────────────────────────────▼───────────────────────────────────┐
│                     MEMORY VAULT (Obsidian Wiki)                    │
│                                                                     │
│  ┌──────────────────────────────────────────────────────────────┐  │
│  │                    ObsidianVault Engine                       │  │
│  │                                                              │  │
│  │  ┌──────────┐ ┌──────────┐ ┌──────────┐ ┌──────────────────┐│  │
│  │  │ Parser   │ │Retrieval │ │Lifecycle │ │  Config          ││  │
│  │  │frontmatter│ │grep_llm  │ │extract   │ │  .vault-config   ││  │
│  │  │wikilinks │ │graph_walk│ │consolidate│ │  counters        ││  │
│  │  │markdown  │ │tag_filter│ │reflect   │ │  templates       ││  │
│  │  └──────────┘ │agentic   │ │validate  │ └──────────────────┘│  │
│  │                └──────────┘ └──────────┘                     │  │
│  └──────────────────────────────────────────────────────────────┘  │
│                                                                     │
│  1-memcells/  2-events/  3-foresights/  4-episodes/               │
│  5-profile/   6-reflections/  clusters/  templates/                │
│                                                                     │
│  Retrieval: grep + LLM (no vector DB)                              │
└─────────────────────────────────────────────────────────────────────┘
```

### 2.2 Execution Flow

#### 2.2.1 REPL Turn — Sequence Diagram

```
User          REPL          Harness         Runner        LlmAgent        LLM        Tools
 │              │              │               │              │             │           │
 │── "prompt" ─▶│              │               │              │             │           │
 │              │── run_str() ─▶│              │              │             │           │
 │              │              │── build ctx ──▶│              │             │           │
 │              │              │  (content +   │              │             │           │
 │              │              │   callbacks)  │              │             │           │
 │              │              │              │── run() ─────▶│             │           │
 │              │              │              │              │── generate ─▶│           │
 │              │              │              │              │◀── stream ───│           │
 │◀── tokens ──│◀─ events ───│◀─ EventStream │              │             │           │
 │              │              │              │              │             │           │
 │              │              │              │              │◀── tool_call─│           │
 │              │              │              │              │── execute ───────────────▶│
 │              │              │              │◀─ before_tool │             │◀── result ─│
 │◀── confirm ──│◀─ HITL check│              │── after_tool ─▶│             │           │
 │── approve ──▶│── decision ─▶│              │              │             │           │
 │              │              │              │              │── generate ─▶│           │
 │◀── response ─│◀─ events ───│◀─ EventStream │◀── stream ───│             │           │
 │              │              │              │              │             │           │
 │              │              │              │── after_agent ─▶│             │           │
 │              │              │              │              │             │           │
 │              │              │── save session─▶│             │             │           │
 │              │              │── mem_write ──▶│             │             │           │
 │              │              │── mem_extract ▶│             │             │           │
```

#### 2.2.2 Memory Lifecycle Flow

```
Conversation End
       │
       ▼
┌─────────────────────────┐
│ AfterToolCallback       │  ← adk-runner callback
│ triggers mem_write       │
└───────────┬─────────────┘
            │
            ▼
┌─────────────────────────┐
│ mem_extract             │
│ ├─ Extract Events       │  → 2-events/fact-NNNN.md
│ │  (5-15 per MemCell)   │
│ ├─ Extract Foresights   │  → 3-foresights/pred-NNNN.md
│ │  (4-10 per MemCell)   │
│ └─ Generate Episode     │  → 4-episodes/ep-NNNN.md
└───────────┬─────────────┘
            │
            ▼  (when threshold reached: 5+ MemCells on same topic)
┌─────────────────────────┐
│ mem_consolidate         │
│ ├─ Cluster detection    │  → clusters/cluster-NNN.md
│ │  (grep tags + LLM)    │
│ └─ Profile update       │  → 5-profile/agent-profile.md
│    (ADD/UPDATE/DELETE)  │
└───────────┬─────────────┘
            │
            ▼  (when profile items > 37)
┌─────────────────────────┐
│ Profile compaction      │
│ LLM consolidates to ~17 │
│ (preserves impactful)   │
└─────────────────────────┘
```

### 2.3 Module Dependency Graph

```
main.rs
  │
  ├── cli/mod.rs
  │     ├── repl.rs           rustyline REPL loop
  │     ├── oneshot.rs        Single-prompt execution
  │     └── commands.rs       Slash command dispatch
  │
  ├── harness.rs              Central orchestrator (struct Harness)
  │     ├── providers.rs      ProviderManager
  │     ├── config.rs         Settings loader
  │     └── cost.rs           CostTracker
  │
  ├── tools/mod.rs
  │     ├── file.rs           FileRead, FileWrite, FileEdit
  │     ├── shell.rs          ShellExec
  │     ├── search.rs         Grep, Glob
  │     ├── web.rs            WebSearch, WebFetch
  │     └── memory.rs         mem_write, mem_search, mem_read, etc.
  │
  ├── memory/mod.rs
  │     ├── vault.rs          ObsidianVault (struct)
  │     ├── parser.rs         Frontmatter + wikilink parser
  │     ├── retrieval.rs      4 retrieval modes
  │     ├── lifecycle.rs      Extract, consolidate, reflect
  │     ├── types.rs          Data types (MemCell, Event, etc.)
  │     └── config.rs         VaultConfig, counters
  │
  ├── context/mod.rs
  │     ├── agents_md.rs      AGENTS.md / CLAUDE.md walker
  │     └── kms.rs            KMS loader
  │
  └── sandbox/mod.rs
        ├── path.rs           Path scoping + traversal prevention
        ├── agentignore.rs    .agentignore rule loading
        └── approval.rs       HITL approval flow
```

**Dependency rule:** `memory/` and `sandbox/` have ZERO dependencies on `cli/`, `harness.rs`, or `tools/`. They are pure library modules testable in isolation.

---

## 3. Component Design

### 3.1 Harness Orchestrator

The `Harness` struct is the central coordinator. It owns the adk Runner, agent, vault, and all subsystems.

```rust
// src/harness.rs

use std::sync::Arc;
use adk_agent::LlmAgentBuilder;
use adk_runner::Runner;
use adk_session::SqliteSessionService;
use adk_tool::ToolConfirmationPolicy;

use crate::memory::vault::ObsidianVault;
use crate::providers::ProviderManager;
use crate::sandbox::FilesystemSandbox;
use crate::context::ContextBuilder;
use crate::config::HarnessConfig;
use crate::cost::CostTracker;

pub struct Harness {
    runner: Runner,
    agent: Arc<dyn adk_core::Agent>,
    vault: ObsidianVault,
    provider_mgr: ProviderManager,
    sandbox: FilesystemSandbox,
    context_builder: ContextBuilder,
    cost_tracker: CostTracker,
    session_service: Arc<dyn adk_session::SessionService>,
    config: HarnessConfig,
}

impl Harness {
    /// Build a new Harness from configuration.
    /// This is the main entry point after CLI argument parsing.
    pub async fn build(config: HarnessConfig) -> Result<Self, HarnessError> {
        // 1. Initialize session service (SQLite)
        let session_service: Arc<dyn adk_session::SessionService> = Arc::new(
            SqliteSessionService::new(&config.session_db_path)?
        );
        session_service.migrate().await?;

        // 2. Initialize memory vault
        let vault = ObsidianVault::open(&config.vault_path)?;

        // 3. Initialize provider
        let provider_mgr = ProviderManager::from_config(&config.provider)?;

        // 4. Initialize sandbox
        let sandbox = FilesystemSandbox::new(
            &config.project_path,
            config.permission_mode,
        )?;

        // 5. Build context (AGENTS.md + KMS + memory)
        let context_builder = ContextBuilder::new(&config.project_path, &vault)?;

        // 6. Build tool registry
        let tools = self::build_tool_registry(
            &sandbox,
            &vault,
            &config,
        )?;

        // 7. Build agent
        let agent = LlmAgentBuilder::new("agent-harness")
            .model(provider_mgr.current_model())
            .system_instruction(context_builder.system_prompt())
            .require_tool_confirmation(ToolConfirmationPolicy::Always)
            .before_tool_callback(harness_before_tool_cb(sandbox.clone()))
            .after_tool_callback(harness_after_tool_cb(vault.clone()))
            .build()?;

        // 8. Build runner
        let runner = Runner::builder()
            .app_name("agent-harness")
            .agent(agent.clone())
            .session_service(session_service.clone())
            .build()?;

        // 9. Cost tracker
        let cost_tracker = CostTracker::new(session_service.clone());

        Ok(Self {
            runner,
            agent,
            vault,
            provider_mgr,
            sandbox,
            context_builder,
            cost_tracker,
            session_service,
            config,
        })
    }

    /// Run a single conversational turn.
    /// Returns a stream of Events for the REPL to consume.
    pub async fn run_turn(
        &self,
        user_message: &str,
        session_id: &str,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<Event>> + Send>>> {
        let content = Content::with_text(user_message);
        let stream = self.runner
            .run_str("default-user", session_id, content)
            .await?;
        Ok(stream)
    }

    /// Switch provider/model mid-session.
    /// Returns a new Runner with the updated model.
    pub async fn switch_model(
        &mut self,
        provider: &str,
        model: &str,
    ) -> Result<()> {
        self.provider_mgr.switch(provider, model).await?;
        // Rebuild agent + runner with new model
        self.rebuild_agent().await
    }

    /// Interrupt current generation.
    pub fn interrupt(&self, session_id: &str) -> bool {
        self.runner.interrupt(session_id)
    }
}
```

### 3.2 Provider Manager

```rust
// src/providers.rs

use std::sync::Arc;
use std::collections::HashMap;
use adk_core::Llm;
use serde::{Serialize, Deserialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderConfig {
    pub name: String,
    pub model: String,
    pub api_key_env: String,       // e.g., "ANTHROPIC_API_KEY"
    pub base_url: Option<String>,  // For OpenAI-compatible endpoints
    pub is_openai_compatible: bool,
}

pub struct ProviderManager {
    current: Arc<dyn Llm>,
    current_provider: String,
    current_model: String,
    custom_endpoints: HashMap<String, ProviderConfig>,
}

impl ProviderManager {
    /// Auto-detect from environment variables.
    pub fn from_env() -> Result<Self> {
        // Check env vars in priority order:
        // ANTHROPIC_API_KEY → OpenAI_API_KEY → GEMINI_API_KEY →
        // DEEPSEEK_API_KEY → GROQ_API_KEY → OLLAMA (no key needed)

        if let Ok(key) = std::env::var("ANTHROPIC_API_KEY") {
            let model = AnthropicClient::new(&key, "claude-sonnet-4-20250514")?;
            return Ok(Self {
                current: Arc::new(model),
                current_provider: "anthropic".into(),
                current_model: "claude-sonnet-4-20250514".into(),
                custom_endpoints: HashMap::new(),
            });
        }
        // ... similar for other providers
        // Fallback: check for custom endpoints in config
    }

    /// Switch to a different provider/model.
    pub async fn switch(&mut self, provider: &str, model: &str) -> Result<()> {
        let new_model = self.create_model(provider, model)?;
        self.current_provider = provider.into();
        self.current_model = model.into();
        self.current = Arc::new(new_model);
        Ok(())
    }

    /// Create model instance for any provider (including custom endpoints).
    fn create_model(&self, provider: &str, model: &str) -> Result<Box<dyn Llm>> {
        match provider {
            "anthropic" => {
                let key = std::env::var("ANTHROPIC_API_KEY")?;
                Ok(Box::new(AnthropicClient::new(&key, model)?))
            }
            "openai" => {
                let key = std::env::var("OPENAI_API_KEY")?;
                Ok(Box::new(OpenAIClient::new(
                    OpenAIConfig::new(&key, model)
                )))
            }
            "ollama" => {
                Ok(Box::new(OllamaModel::new(model)?))
            }
            "deepseek" => {
                let key = std::env::var("DEEPSEEK_API_KEY")?;
                Ok(Box::new(DeepSeekClient::new(&key, model)?))
            }
            "groq" => {
                let key = std::env::var("GROQ_API_KEY")?;
                Ok(Box::new(GroqClient::new(&key, model)?))
            }
            other if let Some(config) = self.custom_endpoints.get(other) => {
                // OpenAI-compatible endpoint
                Ok(Box::new(OpenAICompatible::new(
                    OpenAICompatibleConfig::new(&config.api_key_env, model)
                        .with_base_url(config.base_url.as_deref().unwrap_or(""))
                        .with_provider_name(other),
                )?))
            }
            _ => Err(HarnessError::UnknownProvider(provider.into())),
        }
    }

    pub fn current_model(&self) -> Arc<dyn Llm> {
        self.current.clone()
    }

    pub fn list_available(&self) -> Vec<(&str, &str)> {
        // Return (provider, default_model) for all configured providers
        let mut list = Vec::new();
        if std::env::var("ANTHROPIC_API_KEY").is_ok() {
            list.push(("anthropic", "claude-sonnet-4-20250514"));
        }
        if std::env::var("OPENAI_API_KEY").is_ok() {
            list.push(("openai", "gpt-4o"));
        }
        // ... etc
        // Add custom endpoints
        for (name, config) in &self.custom_endpoints {
            list.push((name, &config.model));
        }
        list
    }
}
```

### 3.3 Tool Implementations

#### 3.3.1 File Tools

```rust
// src/tools/file.rs

use adk_tool::AdkError;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;
use std::path::PathBuf;

// ─── FileRead ───────────────────────────────────────────────

#[derive(Deserialize, JsonSchema)]
pub struct FileReadArgs {
    /// File path relative to working directory
    pub path: String,
    /// Optional line range, e.g., "1-50"
    pub range: Option<String>,
}

/// Read file contents with optional line range.
/// Returns content with line numbers (cat -n format).
#[tool]
async fn file_read(
    args: FileReadArgs,
    ctx: Arc<dyn ToolContext>,
) -> Result<Value, AdkError> {
    let sandbox = ctx.get_sandbox()?;  // custom extension
    let resolved = sandbox.resolve_path(&args.path)?;
    sandbox.check_readable(&resolved)?;

    let content = tokio::fs::read_to_string(&resolved).await
        .map_err(|e| AdkError::tool_error("file_read", &e.to_string()))?;

    let result = if let Some(range) = &args.range {
        let (start, end) = parse_line_range(range, content.lines().count())?;
        content
            .lines()
            .enumerate()
            .skip(start)
            .take(end - start + 1)
            .map(|(i, line)| format!("{:>6}\t{}", i + 1, line))
            .collect::<Vec<_>>()
            .join("\n")
    } else {
        content
            .lines()
            .enumerate()
            .map(|(i, line)| format!("{:>6}\t{}", i + 1, line))
            .collect::<Vec<_>>()
            .join("\n")
    };

    Ok(serde_json::json!({ "content": result, "path": args.path }))
}

// ─── FileWrite ──────────────────────────────────────────────

#[derive(Deserialize, JsonSchema)]
pub struct FileWriteArgs {
    /// File path relative to working directory
    pub path: String,
    /// Content to write
    pub content: String,
}

/// Write content to a file. Creates parent directories if needed.
/// Overwrites existing content.
#[tool]
async fn file_write(
    args: FileWriteArgs,
    ctx: Arc<dyn ToolContext>,
) -> Result<Value, AdkError> {
    let sandbox = ctx.get_sandbox()?;
    let resolved = sandbox.resolve_path(&args.path)?;
    sandbox.check_writable(&resolved)?;

    // Create parent directories
    if let Some(parent) = resolved.parent() {
        tokio::fs::create_dir_all(parent).await
            .map_err(|e| AdkError::tool_error("file_write", &e.to_string()))?;
    }

    // Atomic write: temp file → rename
    let tmp = resolved.with_extension("tmp");
    tokio::fs::write(&tmp, &args.content).await
        .map_err(|e| AdkError::tool_error("file_write", &e.to_string()))?;
    tokio::fs::rename(&tmp, &resolved).await
        .map_err(|e| AdkError::tool_error("file_write", &e.to_string()))?;

    Ok(serde_json::json!({
        "success": true,
        "path": args.path,
        "bytes_written": args.content.len()
    }))
}

// ─── FileEdit ───────────────────────────────────────────────

#[derive(Deserialize, JsonSchema)]
pub struct FileEditArgs {
    /// File path relative to working directory
    pub path: String,
    /// Exact string to find (must be unique in file)
    pub old_string: String,
    /// Replacement string
    pub new_string: String,
}

/// Replace a unique string in a file with a new string.
/// Fails if old_string is not found or has multiple matches.
#[tool]
#[tool(read_only)]
async fn file_edit(
    args: FileEditArgs,
    ctx: Arc<dyn ToolContext>,
) -> Result<Value, AdkError> {
    let sandbox = ctx.get_sandbox()?;
    let resolved = sandbox.resolve_path(&args.path)?;
    sandbox.check_writable(&resolved)?;

    let content = tokio::fs::read_to_string(&resolved).await
        .map_err(|e| AdkError::tool_error("file_edit", &e.to_string()))?;

    // Check uniqueness
    let matches = content.matches(&args.old_string).count();
    if matches == 0 {
        return Err(AdkError::tool_error(
            "file_edit",
            "old_string not found in file",
        ));
    }
    if matches > 1 {
        return Err(AdkError::tool_error(
            "file_edit",
            &format!(
                "old_string found {} times in file — must be unique. \
                 Add more surrounding context to make it unique.",
                matches
            ),
        ));
    }

    let new_content = content.replacen(&args.old_string, &args.new_string, 1);

    // Atomic write
    let tmp = resolved.with_extension("tmp");
    tokio::fs::write(&tmp, &new_content).await?;
    tokio::fs::rename(&tmp, &resolved).await?;

    Ok(serde_json::json!({
        "success": true,
        "path": args.path,
        "replacements": 1
    }))
}
```

#### 3.3.2 Shell Tool

```rust
// src/tools/shell.rs

use std::time::Duration;

#[derive(Deserialize, JsonSchema)]
pub struct ShellExecArgs {
    /// Shell command to execute
    pub command: String,
    /// Timeout in seconds (default: 120)
    pub timeout: Option<u64>,
}

/// Execute a shell command.
/// Destructive commands require user approval (HITL).
#[tool]
async fn shell_exec(
    args: ShellExecArgs,
    ctx: Arc<dyn ToolContext>,
) -> Result<Value, AdkError> {
    let sandbox = ctx.get_sandbox()?;
    let timeout = Duration::from_secs(args.timeout.unwrap_or(120));

    // Destructive detection
    let destructive = sandbox.check_destructive(&args.command);
    if destructive.is_destructive {
        // Signal HITL via ToolConfirmationPolicy
        // This is handled by the before_tool_callback in Harness
        // If we reach here, user already approved
    }

    let output = tokio::time::timeout(timeout, async {
        tokio::process::Command::new("sh")
            .arg("-c")
            .arg(&args.command)
            .current_dir(sandbox.root())
            .output()
            .await
    })
    .await
    .map_err(|_| AdkError::tool_error("shell_exec", "Command timed out"))?
    .map_err(|e| AdkError::tool_error("shell_exec", &e.to_string()))?;

    Ok(serde_json::json!({
        "stdout": String::from_utf8_lossy(&output.stdout),
        "stderr": String::from_utf8_lossy(&output.stderr),
        "exit_code": output.status.code().unwrap_or(-1),
    }))
}
```

#### 3.3.3 Memory Tools

```rust
// src/tools/memory.rs

#[derive(Deserialize, JsonSchema)]
pub struct MemWriteArgs {
    pub project: String,
    pub topic: String,
    pub context: String,
    pub actions: Vec<ActionRecord>,
    pub outcome: String,
    pub keywords: Vec<String>,
}

#[derive(Deserialize, JsonSchema, Serialize)]
pub struct ActionRecord {
    pub description: String,
    pub result: String,
}

/// Append a MemCell (raw experience) to the daily log.
#[tool]
async fn mem_write(
    args: MemWriteArgs,
    ctx: Arc<dyn ToolContext>,
) -> Result<Value, AdkError> {
    let vault: &ObsidianVault = ctx.get_vault()?;
    let memcell_ref = vault.write_memcell(
        &args.project,
        &args.topic,
        &args.context,
        &args.actions,
        &args.outcome,
        &args.keywords,
    ).await?;

    Ok(serde_json::json!({
        "memcell_ref": memcell_ref,
        "status": "written"
    }))
}

#[derive(Deserialize, JsonSchema)]
pub struct MemSearchArgs {
    /// Natural language search query
    pub query: String,
    /// Retrieval mode: grep_llm | graph_walk | tag_filter | agentic
    pub mode: Option<String>,
    /// Filter by memory levels
    pub levels: Option<Vec<String>>,
    /// Filter by project
    pub project: Option<String>,
    /// Filter by tags
    pub tags: Option<Vec<String>>,
    /// Max results (default: 20)
    pub limit: Option<usize>,
}

/// Search memories across all levels using specified retrieval mode.
#[tool]
async fn mem_search(
    args: MemSearchArgs,
    ctx: Arc<dyn ToolContext>,
) -> Result<Value, AdkError> {
    let vault: &ObsidianVault = ctx.get_vault()?;
    let query = MemoryQuery {
        query: args.query,
        mode: args.mode.unwrap_or_else(|| "grep_llm".into())
            .parse()?,
        levels: args.levels,
        project: args.project,
        tags: args.tags,
        limit: args.limit.unwrap_or(20),
    };

    let results = vault.search(&query).await?;

    let json_results: Vec<Value> = results.iter().map(|r| {
        serde_json::json!({
            "ref_id": r.ref_id,
            "level": r.level.as_str(),
            "relevance": r.relevance_score,
            "snippet": r.snippet,
        })
    }).collect();

    Ok(serde_json::json!({ "results": json_results }))
}
```

### 3.4 Memory Vault Engine

```rust
// src/memory/vault.rs

pub struct ObsidianVault {
    vault_path: PathBuf,
    config: VaultConfig,
    counters: AtomicCounters,
}

impl ObsidianVault {
    /// Open an existing vault or create a new one.
    pub fn open(path: &Path) -> Result<Self> {
        let config_path = path.join(".vault-config.json");
        let config: VaultConfig = if config_path.exists() {
            let content = std::fs::read_to_string(&config_path)?;
            serde_json::from_str(&content)?
        } else {
            VaultConfig::default()
        };

        // Ensure directory structure exists
        for dir in &[
            "1-memcells", "2-events", "3-foresights",
            "4-episodes", "5-profile", "6-reflections/weekly",
            "6-reflections/monthly", "clusters", "templates",
        ] {
            std::fs::create_dir_all(path.join(dir))?;
        }

        Ok(Self {
            vault_path: path.to_path_buf(),
            config,
            counters: AtomicCounters::new(),
        })
    }

    /// Write a MemCell to the daily log file.
    pub async fn write_memcell(
        &self,
        project: &str,
        topic: &str,
        context: &str,
        actions: &[ActionRecord],
        outcome: &str,
        keywords: &[&str],
    ) -> Result<String> {
        let now = chrono::Local::now();
        let date = now.format("%Y-%m-%d").to_string();
        let time = now.format("%H:%M").to_string();

        // Determine next MemCell number
        let memcell_path = self.vault_path
            .join("1-memcells")
            .join(now.format("%Y").to_string())
            .join(now.format("%m").to_string())
            .join(format!("{}.md", date));

        let memcell_count = self.count_memcells_today(&memcell_path).await?;
        let memcell_num = memcell_count + 1;

        let memcell_ref = format!("[[{}#MemCell {:03}]]", date, memcell_num);
        let section = format!(
            "\n\n## MemCell {:03} — {}\n\n\
             **Topic**: {}\n\n\
             **Context**: {}\n\n\
             **Actions**:\n{}\n\n\
             **Outcome**: {}\n\n\
             **Keywords**: {}\n",
            memcell_num, time, topic, context,
            actions.iter().enumerate().map(|(i, a)|
                format!("{}. {} → {}", i+1, a.description, a.result)
            ).collect::<Vec<_>>().join("\n"),
            outcome,
            keywords.join(", "),
        );

        // Atomic append
        tokio::fs::create_dir_all(memcell_path.parent().unwrap()).await?;
        let tmp = memcell_path.with_extension("tmp");
        if memcell_path.exists() {
            let existing = tokio::fs::read_to_string(&memcell_path).await?;
            tokio::fs::write(&tmp, format!("{}{}", existing, section)).await?;
        } else {
            let frontmatter = format!(
                "---\ntype: memcell\ndate: {}\nproject: {}\n\
                 tags: [memcell, {}]\nmemcell_count: {}\n---\n",
                date, project,
                project.replace(" ", "-"),
                memcell_num,
            );
            tokio::fs::write(&tmp, format!("{}{}", frontmatter, section)).await?;
        }
        tokio::fs::rename(&tmp, &memcell_path).await?;

        // Update counters and index
        self.counters.increment_memcell();
        self.save_config().await?;
        self.regenerate_index().await?;

        Ok(memcell_ref)
    }

    /// Search using specified retrieval mode.
    pub async fn search(&self, query: &MemoryQuery) -> Result<Vec<MemoryResult>> {
        match query.mode {
            RetrievalMode::GrepLlm => self.retrieve_grep_llm(query).await,
            RetrievalMode::GraphWalk => self.retrieve_graph_walk(query).await,
            RetrievalMode::TagFilter => self.retrieve_tag_filter(query).await,
            RetrievalMode::Agentic => self.retrieve_agentic(query).await,
        }
    }
}
```

### 3.5 Filesystem Sandbox

```rust
// src/sandbox/mod.rs

use std::path::{Path, PathBuf};
use ignore::WalkBuilder;
use path_clean::PathClean;

pub struct FilesystemSandbox {
    root: PathBuf,
    ignore_rules: ignore::Override,
    permission_mode: PermissionMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum PermissionMode {
    Strict,  // Prompt for all mutating ops
    Auto,    // Auto-approve non-destructive
    Yolo,    // Auto-approve everything
}

#[derive(Debug, Clone)]
pub struct DestructiveCheck {
    pub is_destructive: bool,
    pub pattern: Option<String>,
    pub category: Option<String>,
}

impl FilesystemSandbox {
    pub fn new(root: &Path, mode: PermissionMode) -> Result<Self> {
        let root = root.canonicalize()?;
        let agentignore = root.join(".agentignore");

        let mut builder = ignore::WalkBuilder::new(&root);
        builder.hidden(false);  // We handle hidden files ourselves
        if agentignore.exists() {
            builder.add_custom_ignore_filename(".agentignore");
        }

        // Extract override rules from builder
        let override_builder = ignore::OverrideBuilder::new(&root);
        if agentignore.exists() {
            override_builder.add(&agentignore)?;
        }

        Ok(Self {
            root,
            ignore_rules: override_builder.build()?,
            permission_mode: mode,
        })
    }

    /// Resolve a path and ensure it's within the sandbox root.
    pub fn resolve_path(&self, relative: &str) -> Result<PathBuf> {
        // Clean the path to resolve .. and .
        let cleaned = PathBuf::from(relative).clean();

        // Join with root
        let resolved = self.root.join(&cleaned);

        // Canonicalize to resolve symlinks
        let canonical = resolved.canonicalize()
            .or_else(|_| {
                // File doesn't exist yet — check parent
                if let Some(parent) = resolved.parent() {
                    parent.canonicalize().map(|p| p.join(
                        resolved.file_name().unwrap_or_default()
                    ))
                } else {
                    Err(std::io::Error::new(
                        std::io::ErrorKind::NotFound, "path not found"
                    ))
                }
            })?;

        // Verify within root
        if !canonical.starts_with(&self.root) {
            return Err(HarnessError::PathTraversal {
                path: relative.into(),
                resolved: canonical.display().to_string(),
            }.into());
        }

        Ok(canonical)
    }

    /// Check if a path is hidden or ignored.
    pub fn is_ignored(&self, path: &Path) -> bool {
        self.ignore_rules.matched(path, false).is_ignore()
    }

    /// Check readability permission.
    pub fn check_readable(&self, path: &Path) -> Result<()> {
        if self.is_ignored(path) {
            return Err(HarnessError::PathIgnored(path.display().to_string()).into());
        }
        Ok(())
    }

    /// Check writability permission.
    pub fn check_writable(&self, path: &Path) -> Result<()> {
        if self.is_ignored(path) {
            return Err(HarnessError::PathIgnored(path.display().to_string()).into());
        }
        // Permission mode checks are handled by HITL in the REPL layer
        Ok(())
    }

    /// Detect destructive shell commands.
    pub fn check_destructive(&self, command: &str) -> DestructiveCheck {
        let patterns: &[(&str, &str)] = &[
            (r"(?i)\brm\s+(-[rfRF]+\s+)?/\s*$", "Destructive deletion"),
            (r"(?i)\brm\s+(-[rfRF]+\s+)?~\s*$", "Destructive deletion"),
            (r"(?i)git\s+push\s+.*--force", "Destructive git"),
            (r"(?i)git\s+reset\s+--hard", "Destructive git"),
            (r"(?i)DROP\s+TABLE", "Destructive SQL"),
            (r"(?i)DELETE\s+FROM\s+\w+\s*;", "Destructive SQL"),
            (r"(?i)\bmkfs\b", "Destructive system"),
            (r"(?i)\bdd\s+.*if=", "Destructive system"),
            (r"(?i)chmod\s+(-R\s+)?777\s+/", "Destructive system"),
            (r"(?i)>\s*/dev/sd", "Destructive system"),
        ];

        for (pattern, category) in patterns {
            if let Ok(re) = regex::Regex::new(pattern) {
                if re.is_match(command) {
                    return DestructiveCheck {
                        is_destructive: true,
                        pattern: Some(pattern.to_string()),
                        category: Some(category.to_string()),
                    };
                }
            }
        }

        DestructiveCheck {
            is_destructive: false,
            pattern: None,
            category: None,
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn permission_mode(&self) -> PermissionMode {
        self.permission_mode
    }
}
```

### 3.6 Context Builder

```rust
// src/context/mod.rs

pub struct ContextBuilder {
    project_path: PathBuf,
    agents_md_content: Vec<(PathBuf, String)>,  // (path, content)
    kms_toc: Option<String>,
    memory_context: Option<String>,
}

impl ContextBuilder {
    pub fn new(project_path: &Path, vault: &ObsidianVault) -> Result<Self> {
        let mut agents_md_content = Vec::new();
        let mut current = project_path.to_path_buf();

        // Walk up from cwd looking for context files
        loop {
            for filename in &["AGENTS.md", "CLAUDE.md"] {
                let candidate = current.join(filename);
                if candidate.exists() {
                    let content = std::fs::read_to_string(&candidate)?;
                    agents_md_content.push((candidate, content));
                }
            }

            // Check .harness/AGENTS.md
            let harness_dir = current.join(".harness");
            if harness_dir.exists() {
                let candidate = harness_dir.join("AGENTS.md");
                if candidate.exists() {
                    let content = std::fs::read_to_string(&candidate)?;
                    agents_md_content.push((candidate, content));
                }
            }

            if !current.pop() {
                break;  // Reached filesystem root
            }
        }

        // Reverse so closer-to-cwd files come last (higher priority)
        agents_md_content.reverse();

        // Load KMS TOC
        let kms_toc = Self::load_kms_toc(project_path)?;

        Ok(Self {
            project_path: project_path.to_path_buf(),
            agents_md_content,
            kms_toc,
            memory_context: None,
        })
    }

    /// Build the system prompt by concatenating all context sources.
    pub fn system_prompt(&self) -> String {
        let mut parts = Vec::new();

        parts.push("You are Agent Harness, an AI coding assistant. \
                     You have access to tools for file operations, \
                     shell execution, web search, and memory management. \
                     Always prefer using dedicated tools over Bash commands. \
                     Be concise. Do not add unnecessary comments or documentation \
                     to code you didn't change.".into());

        // AGENTS.md / CLAUDE.md (closest = highest priority)
        for (path, content) in &self.agents_md_content {
            parts.push(format!(
                "\n--- Context from {} ---\n{}", 
                path.display(), content
            ));
        }

        // KMS TOC
        if let Some(toc) = &self.kms_toc {
            parts.push(format!("\n--- Project Knowledge Base ---\n{}", toc));
        }

        parts.join("\n\n")
    }

    fn load_kms_toc(project_path: &Path) -> Result<Option<String>> {
        let kms_dir = project_path.join(".kms");
        if !kms_dir.exists() {
            return Ok(None);
        }

        // Find all KMS index files
        let mut tocs = Vec::new();
        for entry in std::fs::read_dir(&kms_dir)? {
            let entry = entry?;
            let index = entry.path().join("index.md");
            if index.exists() {
                let content = std::fs::read_to_string(&index)?;
                tocs.push(format!("### {}\n{}", entry.file_name().display(), content));
            }
        }

        if tocs.is_empty() {
            Ok(None)
        } else {
            Ok(Some(tocs.join("\n\n")))
        }
    }
}
```

---

## 4. Data Architecture

### 4.1 File Layout

```
~/.config/agent-harness/
├── settings.json          ← User-level config (provider, model, permissions)
├── sessions.db            ← SQLite (adk-session)
├── memory-vault/          ← Global vault (if configured)
└── logs/
    └── harness.log         ← Rotating log files

<project>/
├── .harness/
│   ├── settings.json      ← Project-level config overrides
│   ├── mcp.json           ← MCP server definitions
│   └── skills/            ← Installed skills (SKILL.md files)
├── .agentignore           ← Sandbox exclusion rules
├── .kms/                  ← Knowledge bases (optional)
│   ├── conventions/
│   │   ├── index.md
│   │   └── pages/
│   └── architecture/
│       ├── index.md
│       └── pages/
├── AGENTS.md              ← Auto-discovered context
├── CLAUDE.md              ← Auto-discovered context
├── memory-vault/          ← Project-scoped vault
│   ├── .vault-config.json
│   ├── index.md
│   ├── 1-memcells/
│   ├── 2-events/
│   ├── 3-foresights/
│   ├── 4-episodes/
│   ├── 5-profile/
│   ├── 6-reflections/
│   ├── clusters/
│   └── templates/
└── .env                   ← API keys (fallback, gitignored)
```

### 4.2 Configuration Schema

```jsonc
// ~/.config/agent-harness/settings.json
{
  // Default provider and model
  "default_provider": "anthropic",
  "default_model": "claude-sonnet-4-20250514",

  // Permission mode: "strict" | "auto" | "yolo"
  "permission_mode": "strict",

  // Memory vault
  "vault_path": null,  // null = use project-local memory-vault/

  // Retrieval defaults
  "retrieval_mode": "grep_llm",
  "retrieval_limit": 20,

  // Cost limits (USD)
  "max_cost_daily": null,
  "max_cost_session": null,

  // Custom providers (OpenAI-compatible)
  "custom_providers": {
    "zai": {
      "api_key_env": "ZAI_API_KEY",
      "base_url": "https://api.z.ai/v1",
      "model": "z-ai-default"
    }
  },

  // Web search
  "search_provider": "duckduckgo",  // "duckduckgo" | "serper"
  "serper_api_key_env": "SERPER_API_KEY",

  // Logging
  "log_level": "info",  // "trace" | "debug" | "info" | "warn" | "error"
  "log_file": true
}
```

### 4.3 MCP Configuration Schema

```jsonc
// .harness/mcp.json
{
  "mcpServers": {
    "filesystem": {
      "type": "stdio",
      "command": "npx",
      "args": ["-y", "@anthropic/mcp-filesystem", "/tmp"],
      "env": {}
    },
    "web-search": {
      "type": "http",
      "url": "https://mcp.example.com/sse"
    }
  }
}
```

### 4.4 SQLite Schema (managed by adk-session)

adk-session manages these tables internally via migrations:

| Table | Purpose |
|-------|---------|
| `sessions` | Session metadata (id, app_name, user_id, timestamps) |
| `events` | Conversation events (serialized) |
| `app_states` | Application-level state (key-value) |
| `user_states` | User-level state (key-value) |

We extend with a custom table for cost tracking:

```sql
-- Migration: 001_cost_tracking.sql
CREATE TABLE IF NOT EXISTS cost_records (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id TEXT NOT NULL,
    timestamp INTEGER NOT NULL,
    provider TEXT NOT NULL,
    model TEXT NOT NULL,
    prompt_tokens INTEGER NOT NULL,
    completion_tokens INTEGER NOT NULL,
    estimated_cost REAL NOT NULL,
    FOREIGN KEY (session_id) REFERENCES sessions(id)
);

CREATE INDEX idx_cost_session ON cost_records(session_id);
CREATE INDEX idx_cost_timestamp ON cost_records(timestamp);
```

### 4.5 Memory Vault — Note Formats

All notes follow the Obsidian wiki convention:

```yaml
---
type: memcell|event|foresight|episode|profile|reflection|cluster
id: fact-NNNN|pred-NNNN|ep-NNNN|cluster-NNN
created: 2026-05-06T14:32:00
# ... type-specific fields
---
```

**Wikilink convention:**
- Outgoing: `[[fact-0142]]` or `[[2026-05-06#MemCell 001]]`
- Backlinks are computed at query time (grep for references)

**Atomic write guarantee:**
All vault writes use the temp-file-then-rename pattern:
```
write → /path/to/file.tmp
rename → /path/to/file.md  (atomic on POSIX)
```

---

## 5. Interface Design

### 5.1 CLI Startup Banner

On every REPL start, the CLI displays the MoMo banner — an ASCII art pitbull head (the user's dog) followed by "MOMO" text art:

```
    /(
   //\\
  //   )_.-"""-._,-""-.
  \\ ^,'_\     /_\     )
   `./ /O\|   |/O\\   /
    \ \_/|   |\_/ \_/
     \ .'  _  `. /
      ( .:(_):. )
       `._.-._,'
         `-

   __  __  __  __
  |  \/  ||  \/  |
  | .  . || .  . |
  | |\/| || |\/| |
  | |  | || |  | |
  |_|  |_||_|  | |
  your AI coding companion
```

- Pitbull head in `bright_white` (ANSI bold)
- "MOMO" text art in `bright_cyan` bold
- Tagline in `dim`
- Respects `NO_COLOR` env var — falls back to plain text

Implementation:

```rust
// src/cli/banner.rs

const PITBULL_HEAD: &str = r#"
    /(
   //\\
  //   )_.-"""-._,-""-.
  \\ ^,'_\     /_\     )
   `./ /O\|   |/O\\   /
    \ \_/|   |\_/ \_/
     \ .'  _  `. /
      ( .:(_):. )
       `._.-._,'
         `-
"#;

const MOMO_TEXT: &str = r#"
   __  __  __  __
  |  \/  ||  \/  |
  | .  . || .  . |
  | |\/| || |\/| |
  | |  | || |  | |
  |_|  |_||_|  |_|
"#;

pub fn print_banner() {
    let use_color = std::env::var("NO_COLOR").is_err();
    if use_color {
        println!(
            "{}\n{}{}",
            PITBULL_HEAD.bright_white(),
            MOMO_TEXT.bright_cyan().bold(),
            "  your AI coding companion".dimmed(),
        );
    } else {
        println!(
            "{}\n{}  your AI coding companion",
            PITBULL_HEAD, MOMO_TEXT,
        );
    }
    println!();
}
```

The banner is called once at REPL startup, before the first prompt. Not shown in one-shot mode (`-p`).

### 5.2 CLI Argument Specification

```
agent-harness [OPTIONS] [COMMAND]

Commands:
  (default)     Start interactive REPL
  help           Show help

Options:
  -p, --prompt <TEXT>       Run one-shot prompt and exit
      --model <NAME>         Override model
      --provider <NAME>      Override provider
      --project <PATH>       Set working directory [default: cwd]
      --permission <MODE>    Permission mode: strict|auto|yolo [default: strict]
      --vault <PATH>         Memory vault path [default: ./memory-vault]
      --config <PATH>        Config file path
  -v, --verbose              Increase log verbosity
  -q, --quiet                Decrease log verbosity
  -V, --version              Show version
```

### 5.2 REPL Slash Command Protocol

All slash commands follow the pattern: `/<command> [args...]`

Commands that mutate state return confirmation output. Commands that fail return error with suggestion.

```
> /model claude-sonnet-4-20250514
✓ Switched to anthropic/claude-sonnet-4-20250514

> /model nonexistent
✗ Unknown model 'nonexistent'. Available: claude-sonnet-4-20250514, gpt-4o

> /cost
Session: $0.0234 (1,240 in + 890 out tokens)
Today: $0.4512 (12 sessions)
```

### 5.3 Tool Context Extension

Since adk-rust's `ToolContext` trait doesn't expose our custom `sandbox` and `vault`, we use a thread-local context pattern:

```rust
// src/tools/context_ext.rs

thread_local! {
    static TOOL_CONTEXT: RefCell<Option<ToolContextData>> = RefCell::new(None);
}

pub struct ToolContextData {
    pub sandbox: FilesystemSandbox,
    pub vault: ObsidianVault,
    pub config: HarnessConfig,
}

pub fn set_tool_context(ctx: ToolContextData) {
    TOOL_CONTEXT.with(|tc| *tc.borrow_mut() = Some(ctx));
}

pub fn with_tool_context<F, R>(f: F) -> Result<R, AdkError>
where
    F: FnOnce(&ToolContextData) -> Result<R, AdkError>,
{
    TOOL_CONTEXT.with(|tc| {
        let borrow = tc.borrow();
        let ctx = borrow.as_ref()
            .ok_or_else(|| AdkError::tool_error("_", "Tool context not initialized"))?;
        f(ctx)
    })
}
```

This is set before each tool execution in the `before_tool_callback` and accessed by tools via `with_tool_context()`.

### 5.4 Streaming Output Format

The REPL consumes the `EventStream` from `Runner::run_str()` and formats output:

```
User prompt
...

⏺ Running file_read(path="src/main.rs")...  [yellow]
  → 42 lines read

⏺ Running shell_exec(command="cargo build")...  [yellow]
  → Compiling agent-harness v0.1.0
  → Finished `dev` profile target

Here is the analysis of the code:
The main.rs file contains the entry point for...  [normal]
```

Color scheme:
| Element | Color | ANSI Code |
|---------|-------|-----------|
| User input | White/Bold | `\x1b[1m` |
| Tool call (running) | Yellow | `\x1b[33m` |
| Tool call (result) | Dim | `\x1b[2m` |
| Agent response | White | default |
| Error | Red | `\x1b[31m` |
| Success indicator | Green | `\x1b[32m` |
| System message | Dim | `\x1b[2m` |

---

## 6. Security Architecture

### 6.1 Threat Model

| Threat | Vector | Mitigation |
|--------|--------|-----------|
| Path traversal | `../../etc/passwd` in file tools | `resolve_path()` canonicalizes + checks prefix |
| Symlink escape | Symlink points outside sandbox | Canonicalization resolves symlinks before check |
| Destructive commands | `rm -rf /` in ShellExec | Regex pattern detection + HITL approval |
| Secret leakage | API keys in logs/config | `keyring-core` for storage, `tracing` filters secrets |
| Shell injection | Malicious args in tool params | `sh -c` with isolated env, no shell var expansion in prompts |
| .agentignore bypass | Hidden files/dirs | `ignore` crate with same semantics as ripgrep |
| MCP tool abuse | Malicious MCP server | Namespace prefix `mcp_`, tool confirmation policy |
| Tool prompt injection | LLM generates malicious tool args | Input guardrails (adk-guardrail), confirmation for mutating ops |

### 6.2 Permission Mode Implementation

```rust
impl Harness {
    fn before_tool_callback(
        sandbox: Arc<FilesystemSandbox>,
    ) -> BeforeToolCallback {
        Box::new(move |ctx: Arc<dyn CallbackContext>| {
            let sandbox = sandbox.clone();
            Box::pin(async move {
                let tool_name = ctx.tool_name()?;
                let tool_args = ctx.tool_input()?;

                // File tools: check path scoping
                if let Some(path) = tool_args.get("path") {
                    if let Err(e) = sandbox.resolve_path(path.as_str()?) {
                        return Ok(Some(Content::with_text(
                            &format!("BLOCKED: {}", e)
                        )));
                    }
                }

                // Shell tool: check for destructive commands
                if tool_name == "shell_exec" {
                    if let Some(cmd) = tool_args.get("command") {
                        let check = sandbox.check_destructive(cmd.as_str()?);
                        if check.is_destructive {
                            // Trigger HITL via ToolConfirmationPolicy
                            // This is handled by the Runner automatically
                        }
                    }
                }

                Ok(None)  // Continue normally
            })
        })
    }
}
```

### 6.3 Secret Management

```rust
// src/config/secrets.rs

use keyring_core::{Entry, Error as KeyringError};

const SERVICE_NAME: &str = "agent-harness";

pub struct SecretStore;

impl SecretStore {
    /// Get a secret from OS keychain, fallback to .env
    pub fn get(provider: &str) -> Result<String, HarnessError> {
        let entry = Entry::new(SERVICE_NAME, provider)?;
        match entry.get_password() {
            Ok(key) => Ok(key),
            Err(KeyringError::NoEntry) => {
                // Fallback to environment variable
                let env_var = format!("{}_API_KEY", provider.to_uppercase());
                std::env::var(&env_var).map_err(|_| {
                    HarnessError::SecretNotFound {
                        provider: provider.into(),
                        env_var,
                    }
                })
            }
            Err(e) => Err(HarnessError::KeyringError(e.to_string())),
        }
    }

    /// Store a secret in OS keychain
    pub fn set(provider: &str, key: &str) -> Result<(), HarnessError> {
        let entry = Entry::new(SERVICE_NAME, provider)?;
        entry.set_password(key)?;
        Ok(())
    }

    /// Delete a secret from OS keychain
    pub fn delete(provider: &str) -> Result<(), HarnessError> {
        let entry = Entry::new(SERVICE_NAME, provider)?;
        entry.delete_credential()?;
        Ok(())
    }

    /// List providers that have stored secrets (masked output)
    pub fn list() -> Result<Vec<String>, HarnessError> {
        // keyring-core doesn't have a list API,
        // so we check known providers
        let known = ["anthropic", "openai", "gemini", "deepseek", "groq", "ollama"];
        let mut found = Vec::new();
        for provider in &known {
            let entry = Entry::new(SERVICE_NAME, provider)?;
            if entry.get_password().is_ok() {
                found.push(provider.to_string());
            }
        }
        Ok(found)
    }
}
```

### 6.4 Secret Redaction in Logs

```rust
// src/config/logging.rs

use tracing_subscriber::filter::EnvFilter;
use tracing_subscriber::fmt::format::FmtSpan;

/// Install logging with secret redaction.
pub fn init_logging(level: &str, log_file: bool) {
    let filter = EnvFilter::try_new(level)
        .unwrap_or_else(|_| EnvFilter::new("info"));

    let builder = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .with_span_events(FmtSpan::NONE);

    if log_file {
        let file_appender = tracing_appender::rolling::daily(
            dirs::state_dir()
                .unwrap_or_else(|| PathBuf::from("/tmp"))
                .join("agent-harness/logs"),
            "harness.log",
        );
        builder
            .with_writer(file_appender)
            .with_ansi(false)
            .init();
    } else {
        builder.with_writer(std::io::stderr).init();
    }
}
```

### 6.5 Input Guardrails

Leverage adk-guardrail for PII redaction and content filtering:

```rust
use adk_guardrail::{GuardrailSet, PiiRedactionGuardrail, ContentFilterGuardrail};

fn build_guardrails() -> GuardrailSet {
    let mut guardrails = GuardrailSet::new();
    guardrails.add_input_guardrail(PiiRedactionGuardrail::default());
    guardrails.add_input_guardrail(ContentFilterGuardrail::default());
    guardrails
}
```

---

## 7. Error Handling Strategy

### 7.1 Error Hierarchy

```rust
// src/error.rs

use thiserror::Error;

#[derive(Error, Debug)]
pub enum HarnessError {
    // ─── Configuration Errors ───
    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Secret not found for provider '{provider}'. Set {env_var} or use /key set {provider}")]
    SecretNotFound { provider: String, env_var: String },

    #[error("Unknown provider: {0}")]
    UnknownProvider(String),

    // ─── Sandbox Errors ───
    #[error("Path traversal blocked: '{path}' resolved to '{resolved}' which is outside project root")]
    PathTraversal { path: String, resolved: String },

    #[error("Path is ignored by .agentignore rules: {0}")]
    PathIgnored(String),

    #[error("Destructive command detected: {category}. Pattern: {pattern}")]
    DestructiveCommand { pattern: String, category: String },

    // ─── Memory Vault Errors ───
    #[error("Vault error: {0}")]
    Vault(String),

    #[error("Note not found: {0}")]
    NoteNotFound(String),

    #[error("Invalid retrieval mode: {0}")]
    InvalidRetrievalMode(String),

    #[error("Profile compaction failed: {0}")]
    ProfileCompaction(String),

    // ─── Tool Errors ───
    #[error("Tool '{tool}' failed: {message}")]
    ToolFailed { tool: String, message: String },

    #[error("Tool confirmation denied for '{tool}'")]
    ConfirmationDenied { tool: String },

    // ─── Provider Errors ───
    #[error("Provider '{provider}' error: {message}")]
    Provider { provider: String, message: String },

    #[error("Model not found: {0}")]
    ModelNotFound(String),

    // ─── I/O Errors ───
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("YAML error: {0}")]
    Yaml(#[from] serde_yaml::Error),

    // ─── External ───
    #[error("Keychain error: {0}")]
    KeyringError(String),

    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),
}
```

### 7.2 Error Propagation Rules

| Layer | Strategy |
|-------|----------|
| Tool implementation | Return `Result<Value, AdkError>` — errors become tool responses |
| Memory vault | Return `Result<T, HarnessError>` — logged but don't crash agent |
| Sandbox | Return `Result<T, HarnessError>` — block operation, return error message |
| REPL | Display user-friendly error with suggestion, never panic |
| One-shot mode | Return exit code 1, write error to stderr |
| Runner callbacks | Return `Ok(Some(error_content))` to signal agent error |

### 7.3 Panic Policy

- **NEVER panic** in tool implementations or sandbox checks
- Use `expect()` only in startup code where failure means cannot proceed
- All fallible operations use `?` with proper error conversion
- `tokio::spawn` tasks catch panics with `JoinHandle` and log them

---

## 8. Testing Strategy

### 8.1 Test Pyramid

```
        ╱╲
       ╱  ╲        Integration Tests (10%)
      ╱────╲       - Tool execution with real filesystem
     ╱      ╲      - REPL command handling
    ╱────────╲     - Provider switching
   ╱          ╲    - Memory vault full lifecycle
  ╱────────────╲
 ╱              ╲   Unit Tests (70%)
╱                ╲  - Vault parser/retrieval
╲                ╱  - Sandbox path resolution
 ╲────────────╱   - Destructive detection
  ╲          ╱    - Context builder
   ╲────────╱
    ╲      ╱
     ╲────╱        Property Tests (20%)
      ╲  ╱         - Path scoping invariants
       ╲╱          - Wikilink integrity
```

### 8.2 Unit Tests

```rust
// tests/unit/sandbox_test.rs

#[test]
fn test_path_traversal_blocked() {
    let sandbox = FilesystemSandbox::new(
        Path::new("/tmp/test-project"),
        PermissionMode::Strict,
    ).unwrap();

    let result = sandbox.resolve_path("../../etc/passwd");
    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        HarnessError::PathTraversal { .. }
    ));
}

#[test]
fn test_destructive_detection() {
    let sandbox = FilesystemSandbox::new(
        Path::new("/tmp/test-project"),
        PermissionMode::Strict,
    ).unwrap();

    let cases = vec![
        ("rm -rf /", true),
        ("git push --force origin main", true),
        ("DROP TABLE users", true),
        ("DELETE FROM users WHERE id = 1", true),
        ("ls -la", false),
        ("cargo build", false),
        ("git status", false),
    ];

    for (cmd, expected_destructive) in cases {
        let check = sandbox.check_destructive(cmd);
        assert_eq!(check.is_destructive, expected_destructive, "Failed for: {}", cmd);
    }
}
```

```rust
// tests/unit/vault_test.rs

#[tokio::test]
async fn test_memcell_write_and_read() {
    let tmp = tempfile::tempdir().unwrap();
    let vault = ObsidianVault::open(tmp.path()).unwrap();

    let memcell_ref = vault.write_memcell(
        "test-project",
        "Write test MemCell",
        "Testing the vault write functionality",
        &[],
        "Success",
        &["test", "vault", "write"],
    ).await.unwrap();

    assert!(memcell_ref.contains("MemCell 001"));

    // Verify file exists with correct structure
    let now = chrono::Local::now();
    let date = now.format("%Y-%m-%d").to_string();
    let memcell_path = tmp.path()
        .join("1-memcells")
        .join(now.format("%Y").to_string())
        .join(now.format("%m").to_string())
        .join(format!("{}.md", date));

    assert!(memcell_path.exists());
    let content = tokio::fs::read_to_string(&memcell_path).await.unwrap();
    assert!(content.contains("## MemCell 001"));
    assert!(content.contains("type: memcell"));
}

#[test]
fn test_wikilink_extraction() {
    let input = "See [[fact-001]] and [[fact-002]] for details. \
                 Also check [[2026-05-06#MemCell 001]].";

    let links = extract_wikilinks(input);
    assert_eq!(links, vec![
        "fact-001",
        "fact-002",
        "2026-05-06#MemCell 001",
    ]);
}
```

### 8.3 Integration Tests

```rust
// tests/integration/tool_execution_test.rs

#[tokio::test]
async fn test_file_write_and_read() {
    let tmp = tempfile::tempdir().unwrap();
    let sandbox = FilesystemSandbox::new(tmp.path(), PermissionMode::Auto).unwrap();

    set_tool_context(ToolContextData {
        sandbox: sandbox.clone(),
        vault: ObsidianVault::open(&tmp.path().join("vault")).unwrap(),
        config: HarnessConfig::default(),
    });

    // Write
    let result = file_write(FileWriteArgs {
        path: "test/hello.txt".into(),
        content: "Hello, world!".into(),
    }, /* ctx */).await.unwrap();

    assert_eq!(result["success"], true);

    // Read
    let result = file_read(FileReadArgs {
        path: "test/hello.txt".into(),
        range: None,
    }, /* ctx */).await.unwrap();

    assert!(result["content"].as_str().unwrap().contains("Hello, world!"));
}

#[tokio::test]
async fn test_file_edit_uniqueness() {
    // Should fail if old_string appears multiple times
    let tmp = tempfile::tempdir().unwrap();

    // ... setup ...

    let result = file_edit(FileEditArgs {
        path: "test.txt".into(),
        old_string: "common".into(),  // appears 3 times
        new_string: "replaced".into(),
    }, /* ctx */).await;

    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("found 3 times"));
}
```

### 8.4 CLI Integration Tests

```rust
// tests/cli/repl_commands_test.rs

use assert_cmd::Command;

#[test]
fn test_version_flag() {
    Command::cargo_bin("agent-harness")
        .unwrap()
        .arg("--version")
        .assert()
        .success()
        .stdout(predicates::str::contains("0.1.0"));
}

#[tokio::test]
async fn test_one_shot_mode() {
    let tmp = tempfile::tempdir().unwrap();

    Command::cargo_bin("agent-harness")
        .unwrap()
        .arg("-p")
        .arg("say hello")
        .arg("--project")
        .arg(tmp.path())
        .env("ANTHROPIC_API_KEY", "sk-test")  // mock provider
        .assert()
        .success();
}
```

### 8.5 Property-Based Tests

```rust
// tests/property/path_safety_test.rs

use proptest::prelude::*;

proptest! {
    #[test]
    fn path_never_escapes_root(
        relative in "[a-zA-Z0-9_/.-]+",
    ) {
        let root = tempfile::tempdir().unwrap();
        let sandbox = FilesystemSandbox::new(
            root.path(), PermissionMode::Strict
        ).unwrap();

        // Any resolved path must start with root
        if let Ok(resolved) = sandbox.resolve_path(&relative) {
            assert!(
                resolved.starts_with(root.path()),
                "Path '{}' escaped to '{}'",
                relative,
                resolved.display()
            );
        }
        // If error, it must be PathTraversal or NotFound (not Ok)
    }
}
```

---

## 9. Build & Deployment

### 9.1 Build Targets

```toml
# Cargo.toml profiles

[profile.release]
lto = true           # Link-time optimization
codegen-units = 1    # Single codegen unit for better optimization
strip = true         # Strip debug symbols
opt-level = 3        # Maximum optimization

[profile.dev]
opt-level = 1        # Faster incremental builds than 0
```

### 9.2 Build Commands

```bash
# Development build
cargo build

# Release build (optimized binary)
cargo build --release

# Run tests
cargo test

# Run with logging
RUST_LOG=debug cargo run

# Run one-shot
cargo run -- -p "explain this code" --project /path/to/project
```

### 9.3 Binary Distribution

The binary is self-contained (no runtime dependencies beyond the OS):

| Platform | Target | Size (estimate) |
|----------|--------|----------------|
| macOS aarch64 | `aarch64-apple-darwin` | ~15MB |
| macOS x86_64 | `x86_64-apple-darwin` | ~15MB |
| Linux x86_64 | `x86_64-unknown-linux-gnu` | ~14MB |
| Windows x86_64 | `x86_64-pc-windows-msvc` | ~12MB |

### 9.4 Installation

```bash
# From source
git clone <repo>
cd agent-harness
cargo install --path .

# Or via cargo-install (future)
cargo install agent-harness
```

### 9.5 CI Pipeline (recommended)

```yaml
# .github/workflows/ci.yml
on: [push, pull_request]
jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - run: cargo test --all-features
      - run: cargo clippy --all-targets -- -D warnings
      - run: cargo fmt --check

  build:
    strategy:
      matrix:
        os: [ubuntu-latest, macos-latest, windows-latest]
    runs-on: ${{ matrix.os }}
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - run: cargo build --release
```

---

## 10. Performance Engineering

### 10.1 Performance Targets

| Metric | Target | Measurement |
|--------|--------|-------------|
| Cold start (REPL) | < 2s | Time from `cargo run` to first prompt |
| Warm turn (first token) | < 2s | Time from Enter to first token (excludes LLM latency) |
| Memory search (500 notes) | < 3s | `mem_search` with grep_llm mode |
| Memory search (5000 notes) | < 10s | `mem_search` with grep_llm mode |
| Tool execution overhead | < 50ms | Time from tool call to tool start (excluding I/O) |
| File write (1MB) | < 100ms | Atomic write + rename |
| Vault index regeneration | < 200ms | After single MemCell write |

### 10.2 Optimization Strategies

#### 10.2.1 Lazy Vault Loading

The vault index (`index.md`) is only regenerated on write operations. On read/search, it's loaded from cache.

```rust
pub struct ObsidianVault {
    // ...
    index_cache: Arc<RwLock<Option<String>>>,
}

impl ObsidianVault {
    fn get_index(&self) -> Result<String> {
        if let Some(cached) = self.index_cache.read().unwrap().as_ref() {
            return Ok(cached.clone());
        }
        let index = tokio::fs::read_to_string(self.vault_path.join("index.md"))
            .await?;
        *self.index_cache.write().unwrap() = Some(index.clone());
        Ok(index)
    }
}
```

#### 10.2.2 Parallel grep

For vault search, use `ignore` crate's parallel walker instead of sequential grep:

```rust
use ignore::WalkParallel;

fn grep_vault_parallel(
    vault_path: &Path,
    pattern: &str,
    levels: &[&str],
) -> Vec<(PathBuf, usize, String)> {
    let pattern = Regex::new(pattern)?;
    let results = Arc::new(Mutex::new(Vec::new()));

    WalkParallel::new(vault_path)
        .run(|| {
            let results = results.clone();
            let pattern = pattern.clone();
            Box::new(move |entry| {
                // Filter by level directory
                // Count matches
                // Push to shared results
            })
        });

    let mut results = Arc::try_unwrap(results)
        .unwrap()
        .into_inner()
        .unwrap();
    results.sort_by(|a, b| b.1.cmp(&a.1));  // Sort by match count desc
    results
}
```

#### 10.2.3 LLM Call Batching

For memory retrieval with `grep_llm` mode, batch all candidate notes into a single LLM call rather than one per note:

```rust
async fn rank_candidates_batch(
    query: &str,
    candidates: &[(PathBuf, String)],  // (path, content)
    llm: &dyn Llm,
) -> Result<Vec<MemoryResult>> {
    // Build single prompt with all candidates
    let candidates_text = candidates
        .iter()
        .enumerate()
        .map(|(i, (path, content))| {
            format!("[{}] {}: {}...", i, path.display(), &content[..200.min(content.len())])
        })
        .collect::<Vec<_>>()
        .join("\n\n");

    let prompt = format!(
        "Query: '{}'\n\nRank these notes by relevance (0-1). \
         Return JSON: [{{\"id\":0,\"score\":0.95},...]\n\n{}",
        query, candidates_text
    );

    let response = llm.generate_content(
        LlmRequest::new(Content::with_text(&prompt)),
        false,  // no streaming
    ).await?;

    // Parse JSON from response
    let rankings = parse_rankings(&response)?;
    Ok(rankings)
}
```

### 10.4 Memory Footprint

| Component | Expected Memory |
|-----------|----------------|
| Vault index (500 notes) | ~2MB |
| Vault index (5000 notes) | ~20MB |
| Session history (1000 turns) | ~50MB |
| grep results cache | ~5MB |
| Total (typical usage) | < 100MB |

---

## 11. Code Standards & Conventions

### 11.1 Naming Conventions

| Element | Convention | Example |
|---------|-----------|---------|
| Modules | `snake_case` | `mem_cell.rs`, `file_system.rs` |
| Structs | `PascalCase` | `ObsidianVault`, `ProviderManager` |
| Enums | `PascalCase` | `PermissionMode`, `RetrievalMode` |
| Functions | `snake_case` | `write_memcell()`, `resolve_path()` |
| Traits | `PascalCase` (no suffix) | `MemoryStore`, `SecretProvider` |
| Constants | `SCREAMING_SNAKE` | `MAX_RETRIES`, `DEFAULT_TIMEOUT` |
| Tool structs | `PascalCase` | `FileReadArgs`, `MemWriteArgs` |
| Tool functions | `snake_case` | `file_read()`, `mem_search()` |

### 11.2 File Organization

```
src/
├── main.rs              # Entry point, clap parsing
├── lib.rs               # Re-exports for integration testing
├── harness.rs           # Harness struct (orchestrator)
├── error.rs             # HarnessError enum
├── cli/
│   ├── mod.rs           # CLI module
│   ├── repl.rs          # REPL loop
│   ├── oneshot.rs       # One-shot execution
│   └── commands.rs      # Slash command dispatch
├── tools/
│   ├── mod.rs           # Tool registry builder
│   ├── context_ext.rs   # Thread-local tool context
│   ├── file.rs          # FileRead, FileWrite, FileEdit
│   ├── shell.rs         # ShellExec
│   ├── search.rs        # Grep, Glob
│   ├── web.rs           # WebSearch, WebFetch
│   └── memory.rs        # mem_write, mem_search, mem_read, etc.
├── memory/
│   ├── mod.rs           # Memory module
│   ├── vault.rs         # ObsidianVault struct
│   ├── parser.rs        # Frontmatter + wikilink parser
│   ├── retrieval.rs     # 4 retrieval modes
│   ├── lifecycle.rs     # Extract, consolidate, reflect
│   ├── types.rs         # Data types
│   └── config.rs        # VaultConfig
├── context/
│   ├── mod.rs
│   ├── agents_md.rs     # AGENTS.md / CLAUDE.md walker
│   └── kms.rs           # KMS loader
├── sandbox/
│   ├── mod.rs
│   ├── path.rs          # Path resolution + scoping
│   ├── agentignore.rs   # .agentignore rule loading
│   └── approval.rs      # HITL approval flow
├── config/
│   ├── mod.rs
│   ├── settings.rs      # Settings loading + merging
│   └── secrets.rs       # OS keychain access
└── providers.rs         # ProviderManager
```

### 11.3 Documentation Standards

```rust
/// Brief one-line description of what this does.
///
/// # Arguments
///
/// * `path` - The file path to read
/// * `range` - Optional line range (e.g., "1-50")
///
/// # Errors
///
/// Returns `HarnessError::PathTraversal` if the path escapes the sandbox.
/// Returns `HarnessError::Vault` if the file cannot be read.
///
/// # Safety
///
/// This function performs filesystem I/O. All paths are validated
/// against the sandbox before access.
async fn file_read(args: FileReadArgs) -> Result<Value, AdkError> {
    // ...
}
```

### 11.4 Git Commit Conventions

Follow Conventional Commits:
- `feat(tool): add FileEdit tool with uniqueness check`
- `fix(sandbox): resolve symlink escape vulnerability`
- `refactor(memory): batch LLM ranking calls`
- `test(vault): add wikilink extraction property tests`
- `docs(srs): update memory vault data model`

### 11.5 Code Quality Gates

```bash
# Pre-commit checks (run in CI)
cargo fmt --check          # Formatting
cargo clippy --all-targets -- -D warnings  # Linting
cargo test --all-features   # All tests
cargo audit                 # Security vulnerabilities
```

---

## 12. Risk Assessment

### 12.1 Technical Risks

| Risk | Probability | Impact | Mitigation |
|------|------------|--------|-----------|
| adk-rust API breaking changes | Medium | High | Pin to `=0.7.0`, review changelogs before upgrade |
| `#[tool]` macro limitations | Low | Medium | Fallback to `FunctionTool::new()` manual registration |
| grep_llm retrieval accuracy at scale | Medium | Medium | Implement `agentic` mode as fallback; add evaluation benchmarks |
| Thread-local context pattern breaks | Low | High | Alternative: pass context via Arc in tool closures |
| OS keychain compatibility issues | Low | Low | `.env` fallback always available |
| pulldown-cmark no wikilink support | Low (known) | Low | Custom pre-processing regex for `[[...]]` extraction |
| Memory vault corruption (crash during write) | Low | High | Atomic writes (temp + rename); SQLite WAL mode for sessions |

### 12.2 Dependency Risks

| Crate | Risk | Mitigation |
|-------|------|-----------|
| adk-rust | Single-point dependency; API changes | Pin version; thin wrapper layer isolates our code from framework changes |
| keyring-core v1.0 | New major version, may have bugs | `.env` fallback; test on all 3 platforms |
| reqwest 0.13 | Default TLS changed to rustls | Explicit `rustls-tls` feature; no need for native-tls |
| rustyline 18 | REPL complexity | Thin wrapper; could swap for custom implementation |
| ignore 0.4 | Mature, stable, ripgrep ecosystem | Low risk; well-maintained by BurntSushi |

### 12.3 Scope Risks

| Risk | Mitigation |
|------|-----------|
| Feature creep | Phase-gated delivery; Phase 1 is minimal viable agent |
| Memory vault becomes too complex | Start with grep_llm only; add modes incrementally |
| MCP server compatibility varies | Test against official MCP servers; namespace isolation |
| Provider API rate limits | Cost tracking; configurable rate limits on web tools |
