# AGENTS.md — Project Instructions

> Pi auto-loads this file on startup. Edit to match your project conventions.

---

## Project Overview

**momo-fetch** — Rust-native AI agent workspace with EverMemOS memory vault.

- **Language:** Rust (edition 2024, rust 1.85.0)
- **Binary:** `momo-fetch`
- **Framework:** adk-rust (agent dev kit)
- **Runtime:** tokio (async)
- **License:** MIT

This is a terminal-based AI coding agent with REPL mode, memory vault, MCP integration,
multi-provider LLM support, skill system, and sub-agent orchestration.

---

## Architecture

```
main.rs
  └─> cli/mod.rs (clap arg parsing)
        ├─> oneshot.rs    (momo-fetch -p "prompt")
        └─> repl.rs       (interactive REPL, default)
              └─> harness.rs     (central orchestrator)
                    ├─> providers.rs    (LLM provider management)
                    ├─> agent/          (agent personalities + orchestrator)
                    ├─> sandbox/        (filesystem isolation)
                    ├─> context/        (system prompt builder)
                    ├─> session.rs      (SQLite session persistence)
                    ├─> memory/         (Obsidian vault engine)
                    ├─> mcp/            (MCP server management)
                    ├─> skill/          (skill system)
                    ├─> team/           (multi-agent coordination)
                    ├─> cost.rs         (token cost tracking)
                    └─> tools/          (agent tool implementations)
```

**Key principle:** `harness.rs` owns everything. The CLI layer calls Harness methods.
Tools use thread-local context to access the sandbox and vault.

---

## Build & Run Commands

```bash
cargo build                    # Debug build
cargo build --release          # Release build (LTO, stripped)
cargo test                    # Run all tests (280+ tests)
cargo test -- --nocapture     # Tests with stdout
cargo run                     # Run REPL mode
cargo run -- -p "prompt"      # One-shot mode
cargo run -- --help           # CLI help
cargo clippy                  # Lint
```

Build artifacts: `target/debug/momo-fetch` / `target/release/momo-fetch`

---

## Source Layout

| Path | Purpose |
|------|----------|
| `src/main.rs` | Entry point, tracing init |
| `src/harness.rs` | Central orchestrator, owns all subsystems |
| `src/session.rs` | SQLite session persistence |
| `src/context_window.rs` | Context window management |
| `src/cost.rs` | Token cost tracking |
| `src/providers.rs` | LLM provider detection & hot-swap |
| `src/cli/` | CLI parsing, REPL, oneshot, banner, commands, status |
| `src/agent/` | Agent personalities, orchestrator tools |
| `src/config/` | Settings, secrets (OS keychain) |
| `src/context/` | System prompt builder, context injection |
| `src/memory/` | Vault engine, sidecar, retrieval, lifecycle, types, parser |
| `src/tools/` | Built-in tools (file, shell, search, web, memory, kms, task) |
| `src/sandbox/` | Filesystem isolation, .agentignore, permission modes |
| `src/skill/` | Skill discovery and execution |
| `src/team/` | Multi-agent coordination, mailbox, worktrees, worker modes + liveness |
| `src/mcp/` | MCP server management (stdio + HTTP) |
| `docs/` | PRD, SRS, SDS, user guides, code guidelines |
| `memory-vault/` | EverMemOS structured memory store |
| `.harness/` | Runtime settings, MCP config, agents, teams, commands, skills |
| `patches/rmcp-1.6.0/` | Patched rmcp (200 OK no Content-Type fix) |

---

## Code Conventions

- Follow existing patterns in the codebase — read nearby code before writing
- All tests must pass before committing (`cargo test`)
- One concern per module/file
- Use `anyhow::Result<T>` for harness/vault/sandbox errors
- Tools return `Result<Value, AdkError>` — errors become tool responses for the LLM
- Atomic writes: temp file → rename (never write in-place)
- **UTF-8 safe truncation:** use `ceil_char_boundary()` never slice `&s[..n]` directly

### Style

- Rust 2024 edition idioms
- `#[tool]` macro from adk-rust for tool definitions
- Thread-local context pattern for shared state in tools (`thread_local! { static CTX: RefCell<...> }`)
- Synchronous vault methods wrapped in `Arc<Mutex<ObsidianVault>>`

### Testing

- Unit tests in each module's `#[cfg(test)] mod tests`
- Thread-local context must be set up in test setup
- Session tests use `InMemorySessionService`
- Run `cargo test` before committing

---

## Key Patterns

### Thread-Local Context for Tools

Tools can't receive sandbox/vault via parameters (adk-rust limitation). Instead:

```rust
thread_local! {
    static SANDBOX_CTX: RefCell<Option<Arc<FilesystemSandbox>>> = RefCell::new(None);
}
pub fn set_sandbox(sandbox: Arc<FilesystemSandbox>) { ... }
fn get_sandbox() -> Result<Arc<FilesystemSandbox>, AdkError> { ... }
```

### Provider Detection Priority

`ProviderManager::from_env()` checks env vars in order:
ANTHROPIC_API_KEY → OPENAI_API_KEY → DEEPSEEK_API_KEY → GROQ_API_KEY →
OPENROUTER_API_KEY → ZAI_API_KEY → LLM_URL+LLM_APIKEY → Ollama fallback

### Memory Sidecar Options

- **Option A** (default): TF-IDF keyword extraction, grep-based search (no extra LLM cost)
- **Option B**: Direct LLM call for memory extraction (when `sidecar_model` configured)
- **Option C**: Separate process via Mailbox IPC

### Config Hierarchy

CLI flags > project `.harness/settings.json` > global `~/.config/momo-fetch/settings.json` > env vars > defaults

---

## Tool Usage

### Priority Order
1. **Built-in tools** — `file_read`, `file_edit`, `grep`, `glob` for code tasks
2. **MCP tools** — `mcp_*` tools for extended capabilities
3. **Shell** — only when built-in tools cannot do the job
4. **Task** — delegate subtasks when work can be parallelized

### File Editing
- Always read a file before editing — never edit a file you haven't read
- Use targeted replacements (old → new)
- Use file_write only for new files or complete rewrites

---

## Context Files (Claude Code / Harness)

Pi auto-loads `AGENTS.md` / `CLAUDE.md` from parent dirs and current dir.

The harness additionally loads:
- `SOUL.md` — agent personality/identity (highest behavioral priority)
- `AGENTS.md` — project instructions
- `CLAUDE.md` — Claude-specific instructions
- `.harness/SOUL.md` — harness-specific personality overrides
- `.harness/AGENTS.md` — harness-specific instruction overrides

---

## Communication

- Be concise and direct
- Show code, not paragraphs of explanation
- State what changed and why — not a step-by-step replay
- If something fails, explain the error and the fix
