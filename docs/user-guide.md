# User Guide — MOMO Fetch

Everything you need to know to use MOMO Fetch effectively.

---

## Table of Contents

1. [Overview](#1-overview)
2. [Setup & Onboarding](#2-setup--onboarding)
3. [Providers & Models](#3-providers--models)
4. [Using the Agent](#4-using-the-agent)
5. [Agent Personalities (Multi-Agent)](#5-agent-personalities-multi-agent)
6. [Memory Vault](#6-memory-vault)
7. [Knowledge Base (KMS)](#7-knowledge-base-kms)
8. [MCP Servers](#8-mcp-servers)
9. [Skills](#9-skills)
10. [Secrets & Security](#10-secrets--security)
11. [Cost Tracking](#11-cost-tracking)
12. [Agent Teams](#12-agent-teams)
13. [Configuration Reference](#13-configuration-reference)
14. [Tips & Best Practices](#14-tips--best-practices)
15. [FAQ](#15-faq)

---

## 1. Overview

MOMO Fetch is a Rust-native AI coding assistant that runs in your terminal. It connects to LLM providers (Anthropic, OpenAI, DeepSeek, Groq, Ollama, OpenRouter, z.ai) and gives the AI tools to read/write files, run commands, search the web, and manage a persistent memory vault.

**What makes it different:**
- **Multi-provider** — switch between Anthropic, OpenAI, DeepSeek, Groq, Ollama, and OpenRouter mid-session
- **Memory vault** — the agent remembers across sessions using an Obsidian-compatible wiki
- **Secure** — API keys in OS keychain, path sandboxing, destructive command detection
- **Extensible** — MCP servers, custom skills, knowledge bases
- **Team mode** — multiple agents working in parallel on separate branches

---

## 2. Setup & Onboarding

### First-Time Setup

```bash
# 1. Build from source
git clone <repo-url>
cd momo-fetch
cargo build

# 2. Install globally (optional)
cargo install --path .
```

### Choosing a Provider

You need at least one LLM provider configured. Options from easiest to most powerful:

| Provider | Setup | Best For |
|----------|-------|----------|
| **Ollama** | Install + `ollama pull llama3.2` | Local/private, no API key, free |
| **Anthropic** | Set `ANTHROPIC_API_KEY` or `/key set anthropic` | Best coding model (Claude) |
| **OpenAI** | Set `OPENAI_API_KEY` or `/key set openai` | GPT-4o, strong general model |
| **DeepSeek** | Set `DEEPSEEK_API_KEY` | Cost-effective coding |
| **Groq** | Set `GROQ_API_KEY` | Fast inference |
| **OpenRouter** | Set `OPENROUTER_API_KEY` | Access to many models |
| **z.ai** | Set `ZAI_API_KEY` (optional `ZAI_LLM_URL`) | OpenAI-compatible coding endpoint |
| **Custom** | Set `LLM_URL` + `LLM_APIKEY` (optional `LLM_MODEL`) | Any OpenAI-compatible endpoint |

### Storing API Keys

Three ways to provide API keys (in priority order):

**1. Environment variable** (fastest, good for CI):
```bash
export ANTHROPIC_API_KEY="sk-ant-..."
```

**2. `.env` file** (per-project):
```bash
echo 'ANTHROPIC_API_KEY=sk-ant-...' > .env
```

**3. OS keychain** (most secure, persistent):
```
/key set anthropic
# Prompts for key, stores in macOS Keychain / Linux Secret Service / Windows Credential Manager
```

### Verifying Setup

```bash
momo-fetch --version    # Should print: momo-fetch 0.1.0
momo-fetch -p "hello"   # Should get a response from the agent
```

---

## 3. Providers & Models

### Auto-Detection

When you start MOMO Fetch, it auto-detects the first available provider from this priority list:

1. Anthropic (if `ANTHROPIC_API_KEY` set)
2. OpenAI (if `OPENAI_API_KEY` set)
3. DeepSeek (if `DEEPSEEK_API_KEY` set)
4. Groq (if `GROQ_API_KEY` set)
5. OpenRouter (if `OPENROUTER_API_KEY` set)
6. z.ai (if `ZAI_API_KEY` set)
7. Custom OpenAI-compatible (if `LLM_URL` + `LLM_APIKEY` set)
8. Ollama (always available if running locally)

### Switching Providers and Models

Inside the REPL, switch any time:

```
claude-sonnet-4-20250514> /models
Available providers/models:
  anthropic: claude-sonnet-4-20250514 ← current
  openai: gpt-4o
  ollama: llama3.2

claude-sonnet-4-20250514> /model gpt-4o
✓ Switched to openai/gpt-4o

gpt-4o> /provider deepseek
✓ Switched to deepseek/deepseek-chat
```

You can also set the provider at startup:

```bash
momo-fetch --provider anthropic --model claude-sonnet-4-20250514
momo-fetch --provider ollama --model llama3.2
```

### Default Models Per Provider

| Provider | Default Model |
|----------|--------------|
| Anthropic | `claude-sonnet-4-20250514` |
| OpenAI | `gpt-4o` |
| DeepSeek | `deepseek-chat` |
| Groq | `llama-3.3-70b-versatile` |
| Ollama | `llama3.2` |
| OpenRouter | `anthropic/claude-sonnet-4` |
| z.ai | `z-ai-default` (set your model via `--model` or `/model`) |
| Custom | Value of `LLM_MODEL` env var (defaults to `"default"`) |

---

## 4. Using the Agent

### Basic Conversation

The agent has access to these built-in tools:

| Tool | What it does |
|------|-------------|
| `file_read` | Read a file (with optional line range) |
| `file_write` | Write/create a file (atomic write) |
| `file_edit` | Replace a unique string in a file |
| `shell_exec` | Run a shell command with timeout |
| `grep` | Search file contents |
| `glob` | Find files by name pattern |
| `web_search` | Search the web (DuckDuckGo or Serper.dev) |
| `web_fetch` | Fetch and convert a URL to text |

Just ask naturally — the agent decides which tools to use:

```
gpt-4o> read the main.rs file and explain it

  ⏺ file_read(path="src/main.rs")
  → 34 lines read

The main.rs file contains the entry point...
```

```
gpt-4o> find all TODO comments in the codebase

  ⏺ grep(pattern="TODO", path="src/")
  → 5 matches found

Here are the TODOs:
1. src/main.rs:42 — TODO: handle edge case
2. src/tools/file.rs:108 — TODO: add encoding detection
...
```

### Permission Prompts

By default (strict mode), the agent asks before every write/shell action:

```
  ⏺ file_write(path="src/lib.rs")
  ! Tool file_write requires approval: path="src/lib.rs"
  Approve? [y/n]: y
```

Use `--permission auto` to only prompt for destructive commands, or `--permission yolo` to skip all prompts.

You can also switch mid-session without restarting:

```
GLM-5> /permission auto
✓ Switched to auto

GLM-5 [auto]> /perm yolo      # /perm is a shortcut
✓ Switched to yolo

GLM-5 [yolo]> /perm strict
✓ Switched to strict

GLM-5>
```

The prompt shows `[auto]` or `[yolo]` when not in strict mode.

### Destructive Command Protection

The agent detects and blocks dangerous commands regardless of permission mode:

| Pattern | Category |
|---------|----------|
| `rm -rf /` | Destructive deletion |
| `rm -rf ~` | Destructive deletion |
| `git push --force` | Destructive git |
| `git reset --hard` | Destructive git |
| `DROP TABLE` | Destructive SQL |
| `DELETE FROM` | Destructive SQL |
| `mkfs` | Destructive system |
| `dd if=` | Destructive system |
| `chmod 777 /` | Destructive system |

### Path Sandboxing

All file operations are scoped to your project directory. The agent cannot access files outside it:

```
gpt-4o> read /etc/passwd

  ⏺ file_read(path="../../etc/passwd")
  ✗ file_read: path resolution failed: Path traversal blocked
```

You can exclude additional files via `.agentignore` (same syntax as `.gitignore`):

```
# .agentignore
*.env
secrets/
node_modules/
target/
```

---

## 5. Agent Personalities (Multi-Agent)

Agent personalities let you define specialist agents with their own system prompts, each focused on a different task. You can switch between them mid-session, or use an orchestrator to coordinate multiple specialists.

### Default Mode (No Agent Flag)

When you start without `--agent`, MOMO Fetch works exactly as before — a single agent loading `AGENTS.md`, `CLAUDE.md`, and `SOUL.md` from the project:

```bash
momo-fetch --project .
```

### Creating Agent Personalities

Place agent config files in `.harness/agents/` within your project:

```
.harness/agents/
├── researcher.md        # Simple personality (markdown only)
├── coder.md             # Simple personality
├── reviewer.md          # Simple personality
├── orchestrator.yml     # Full config (personality + capabilities)
└── orchestrator.md      # Personality prompt for orchestrator
```

**Simple** (`.md` only) — just a personality prompt:

```markdown
# .harness/agents/researcher.md
You are a research specialist. Always cite your sources.
Deeply investigate before answering. Prefer web search and code archaeology.
```

**Full** (`.yml` + `.md`) — personality plus configuration:

```yaml
# .harness/agents/researcher.yml
description: "Research specialist that deeply investigates topics"
personality_file: researcher.md    # optional, defaults to <name>.md
model: deepseek-chat               # optional model override
provider: deepseek                 # optional provider override
tools: [file_read, grep, glob, web_search, web_fetch, mem_search]  # optional tool restrictions
capabilities: [research, analysis] # tags for orchestrator reference
```

If both `<name>.yml` and `<name>.md` exist, the YAML config takes priority.

### Starting with a Specific Agent

```bash
# Start as a specific specialist
momo-fetch --project . --agent researcher
momo-fetch --project . --agent coder
momo-fetch --project . --agent reviewer
```

The agent's system prompt changes to include the personality, but all other features (tools, memory, MCP, skills) work normally.

### Switching Agents Mid-Session

Inside the REPL, switch personalities without restarting:

```
claude-sonnet-4-20250514> /agent list
Agent personalities (3):
  coder: Coding specialist
  researcher: Research expert
  reviewer: Code review specialist

claude-sonnet-4-20250514> /agent switch researcher
✓ Switched to agent 'researcher'

claude-sonnet-4-20250514> /agent default
✓ Switched to default mode (no agent personality)
```

| Command | What it does |
|---------|-------------|
| `/agent list` | List all agent personalities |
| `/agent show <name>` | Show agent personality details |
| `/agent switch <name>` | Switch to an agent personality |
| `/agent default` | Switch back to default mode (AGENTS.md + SOUL.md) |

### Three Specialist Modes

#### Mode 1: Single Specialist

Run as one specialist agent. No team, no orchestration.

```bash
momo-fetch --project . --agent researcher
```

#### Mode 2: Squad Lead (Team with Personalities)

Start as an orchestrator and spawn workers with different personalities:

```bash
momo-fetch --project . --agent orchestrator
```

Then use `/team start` with the `--agent` flag per worker:

```
orchestrator> /team start
Define worker agents (one per line, empty line to finish):
  Format: <name> <task> [--agent <personality>] [--branch <name>] [--worktree]

  worker[0]: researcher "Research the auth architecture" --agent researcher --worktree
  worker[1]: coder "Implement the new auth flow" --agent coder --worktree

✓ Team 'team-20260514-143000' started
  Workers: 2
    researcher (branch: team/researcher) [worktree] [tmux]
    coder (branch: team/coder) [worktree] [tmux]
```

Each worker spawns with its own personality from `.harness/agents/<name>.md`.

#### Mode 3: Smart Orchestrator (AI-Driven)

An agent with `capabilities: [orchestration]` gets three extra tools:

| Tool | What it does |
|------|-------------|
| `spawn_agent` | Dynamically spawn a specialist (inline or process mode) |
| `send_message` | Send a message to a spawned agent via mailbox |
| `receive_messages` | Receive messages from spawned agents |

The orchestrator AI decides at runtime which specialist to invoke:

```yaml
# .harness/agents/orchestrator.yml
description: "Smart orchestrator that coordinates specialist agents"
personality_file: orchestrator.md
capabilities: [orchestration, coordination]
```

```markdown
<!-- .harness/agents/orchestrator.md -->
You are a project lead. Analyze each task and decide which specialist to use.
Use spawn_agent to delegate work. Available specialists: researcher, coder, reviewer.
For quick tasks use inline mode. For long tasks use process mode.
```

**Inline mode** — fast, synchronous (agent waits for result):

```
orchestrator> Please research the best approach for caching in this codebase

  ⏺ spawn_agent(agent="researcher", task="Research caching approaches...", mode="inline")
  → Agent 'researcher' completed

Based on the researcher's findings...
```

**Process mode** — async, separate OS process:

```
orchestrator> Refactor the entire auth module

  ⏺ spawn_agent(agent="coder", task="Refactor auth module...", mode="process")
  → Agent 'coder' started as separate process (worker: spawn-coder)
```

---

## 6. Memory Vault

The memory vault is the agent's persistent memory. It stores experiences, extracts knowledge, and recalls relevant context across sessions.

### How It Works

The vault lives in `<project>/memory-vault/` and uses an Obsidian-compatible wiki structure:

```
memory-vault/
├── 1-memcells/     ← Raw experiences (daily logs)
├── 2-events/       ← Extracted facts (fact-NNNN.md)
├── 3-foresights/   ← Predictions (pred-NNNN.md)
├── 4-episodes/     ← Summaries (ep-NNNN.md)
├── 5-profile/      ← Agent/user profile
├── 6-reflections/  ← Weekly/monthly reflections
├── clusters/       ← Grouped related memories
└── templates/      ← Note templates
```

### Memory Lifecycle

1. **MemCell** — the agent writes raw experiences during conversations (via `mem_write` tool)
2. **Extraction** — `mem_extract` processes MemCells into Events (facts), Foresights (predictions), and Episodes (summaries)
3. **Consolidation** — `mem_consolidate` clusters related MemCells and updates the agent profile
4. **Reflection** — `mem_reflect` generates weekly/monthly reflections

### Retrieval Modes

The agent can search memories using four strategies:

| Mode | How it works | Best for |
|------|-------------|----------|
| `grep_llm` (default) | Grep for candidates, then LLM ranks by relevance | General searches |
| `graph_walk` | Follow `[[wikilinks]]` from a seed note | Exploring connections |
| `tag_filter` | Filter by YAML frontmatter tags | Tag-based lookup |
| `agentic` | Multi-round search with LLM-guided query refinement | Complex research |

### Checking Vault Status

```
gpt-4o> /mem
Memory Vault Status:
  MemCells:    15
  Events:      42 (next: fact-0043)
  Foresights:  8 (3 pending, next: pred-0009)
  Episodes:    6 (next: ep-0007)
  Clusters:    2
  Reflections: 1

Vault path: /path/to/project/memory-vault
```

The vault is just markdown files — you can browse it with Obsidian or any text editor.

---

## 7. Knowledge Base (KMS)

Knowledge bases are project wikis the agent can search on demand.

### Creating a Knowledge Base

```bash
mkdir -p .kms/architecture/pages
mkdir -p .kms/conventions/pages
```

Add an `index.md` table of contents in each:

```markdown
# .kms/architecture/index.md
# Architecture

## Pages
- [System Overview](pages/system-overview.md)
- [Data Flow](pages/data-flow.md)
```

Add content pages:

```markdown
# .kms/architecture/pages/system-overview.md
# System Overview

The system has three layers: CLI, Harness, and Tools...
```

### Using KMS

The KMS table of contents is automatically injected into the agent's system prompt. The agent can also search and read KMS pages directly using `KmsSearch` and `KmsRead` tools.

```
gpt-4o> /kms list
Knowledge bases:
  architecture (3 pages)
  conventions (5 pages)
```

---

## 8. MCP Servers

MCP (Model Context Protocol) servers extend the agent with additional tools.

### Adding an MCP Server

```
gpt-4o> /mcp add filesystem npx -y @anthropic/mcp-filesystem /tmp
✓ Added MCP server 'filesystem' and starting...
```

Server config is saved to `.harness/mcp.json`:

```json
{
  "mcpServers": {
    "filesystem": {
      "command": "npx",
      "args": ["-y", "@anthropic/mcp-filesystem", "/tmp"],
      "env": {},
      "disabled": false
    }
  }
}
```

### Managing Servers

```
gpt-4o> /mcp list
MCP servers:
  filesystem: Running
    command: npx -y @anthropic/mcp-filesystem /tmp

gpt-4o> /mcp remove filesystem
✓ Removed MCP server 'filesystem'
```

MCP tools are automatically available to the agent with the `mcp_` namespace prefix (e.g., `mcp_filesystem__read_file`).

---

## 9. Skills

Skills are reusable prompt templates the agent can invoke.

### Installing a Skill

```
gpt-4o> /skill install https://github.com/example/code-review-skill
✓ Installed skill 'code-review'
```

Or manually place a `SKILL.md` file in one of:
- `.skills/`
- `.claude/skills/`
- `.harness/skills/`

### Listing Skills

```
gpt-4o> /skill list
Installed skills (2):
  code-review: Review code for issues [explicit]
  commit: Create well-formatted commits [git, commit]
```

### How Skills Work

- **Auto-match**: when your message matches a skill's `whenToUse` trigger, the skill context is injected automatically
- **Explicit invoke**: type `/<skill-name>` to invoke a skill directly (e.g., `/commit`)

---

## 10. Secrets & Security

### API Key Storage

Keys are stored in your OS keychain (never in config files or logs):

| Platform | Storage |
|----------|---------|
| macOS | Keychain |
| Linux | Secret Service (libsecret) |
| Windows | Credential Manager |

Fallback: environment variables or `.env` files for CI environments.

### Managing Keys

```
/key set anthropic       # Store a key (prompts securely)
/key list                # List stored keys (masked)
/key delete anthropic    # Remove a key
```

### Security Features

- **Path sandboxing**: all file ops restricted to project directory
- **Path traversal protection**: `../../etc/passwd` is blocked
- **Destructive command detection**: `rm -rf /`, `git push --force`, `DROP TABLE` flagged
- **`.agentignore`**: exclude sensitive files from agent access
- **Secret redaction**: keys never appear in logs

---

## 11. Cost Tracking

MOMO Fetch tracks token usage and estimated costs for every request.

### Viewing Costs

```
gpt-4o> /cost
Session: $0.0234 (1,240 in + 890 out tokens, 5 requests)
Today:   $0.4512 (12 sessions)
Project (my-app): $1.2304 (3 sessions)
```

### Cost Variants

| Command | Shows |
|---------|-------|
| `/cost` | Session + today + project |
| `/cost today` | Today's total |
| `/cost week` | This week's total |
| `/cost project my-app` | Specific project |
| `/cost project` | List all projects |

### Budget Alerts

When you approach a configured budget limit, the agent warns you:

```
⚠ Budget alert: daily cost ($4.52) approaching limit ($5.00)
```

---

## 12. Agent Teams

Run multiple agents in parallel on separate tasks.

### Starting a Team

```
gpt-4o> /team start
Define worker agents (one per line, empty line to finish):
  Format: <name> <task> [--agent <personality>] [--branch <name>] [--worktree]

  worker[0]: frontend Fix all TypeScript errors --branch fix-ts --worktree
  worker[1]: tests Add integration tests for auth --branch auth-tests

✓ Team 'team-abc123' started
  Workers: 2
    frontend (branch: fix-ts) [worktree] [tmux]
    tests (branch: auth-tests)
```

### Starting a Team from Config File

Create a team config in `.harness/teams/<name>.yml`:

```yaml
# .harness/teams/auth-squad.yml
name: "Auth Refactor Squad"
workers:
  - name: researcher
    task: "Research the auth architecture and find best practices"
    agent: researcher
    worktree: true
  - name: coder
    task: "Implement the new auth flow with JWT + refresh tokens"
    agent: coder
    branch: feature/auth
    worktree: true
  - name: reviewer
    task: "Review the implementation for security issues"
    agent: reviewer
```

Start the team with one command:

```
orchestrator> /team start auth-squad
✓ Loaded team config 'auth-squad' (3 workers)
  - researcher [agent: researcher] [worktree]: Research the auth architecture and find best practices
  - coder [agent: coder] [branch: feature/auth] [worktree]: Implement the new auth flow with JWT + refresh tokens
  - reviewer [agent: reviewer]: Review the implementation for security issues

✓ Team 'team-20260514-150000' started
  Workers: 3
    researcher (branch: team/researcher) [worktree] [tmux]
    coder (branch: feature/auth) [worktree] [tmux]
    reviewer (branch: team/reviewer)
```

### Starting a Team Interactively

Use `--agent <name>` to give each worker a specialist personality:

```
orchestrator> /team start
Define worker agents (one per line, empty line to finish):
  Format: <name> <task> [--agent <personality>] [--branch <name>] [--worktree]

  worker[0]: research "Investigate auth architecture" --agent researcher --worktree
  worker[1]: implement "Build the new auth flow" --agent coder --branch feature/auth --worktree

✓ Team 'team-20260514-143000' started
  Workers: 2
    research (branch: team/research) [worktree] [tmux]
    implement (branch: feature/auth) [worktree] [tmux]
```

Each worker loads its personality from `.harness/agents/<name>.md`.

### Monitoring Progress

```
gpt-4o> /team status
Team: team-abc123
Status: Running

Workers:
  frontend: Completed — Fixed 12 TypeScript errors
  tests: Running
```

### Merging Results

```
gpt-4o> /team merge
Merge results:
  ✓ frontend (fix-ts): Merged successfully
  ✗ tests (auth-tests): Still running
```

### Stopping a Team

```
gpt-4o> /team stop
✓ Team stopped. Workers terminated, worktrees cleaned up.
```

### Requirements

- **tmux** for pane isolation (workers still run without it, just in background)
- **git** for worktree isolation (optional — workers share the working directory without it)

---

## 13. Configuration Reference

### Global Config

Location: `~/Library/Application Support/momo-fetch/` (macOS) or `~/.config/momo-fetch/` (Linux)

| File | Purpose |
|------|---------|
| `sessions.db` | SQLite session database |
| `cost.json` | Cost tracking data |
| `history.txt` | REPL command history |

### Project Config

Location: `<project>/.harness/`

| File | Purpose |
|------|---------|
| `settings.json` | Project-level config overrides |
| `mcp.json` | MCP server definitions |
| `skills/` | Installed skills |
| `agents/` | Agent personality files (`.md` and `.yml`) |
| `teams/` | Team config files (`.yml`) |

### Context Files (auto-discovered)

The agent walks up from your project directory looking for:

| File | Purpose |
|------|---------|
| `SOUL.md` | Agent personality/identity — defines who the agent is, its communication style, values, and behavioral traits |
| `AGENTS.md` | Project instructions — coding standards, architecture, preferred patterns |
| `CLAUDE.md` | Claude-specific instructions |
| `.harness/SOUL.md` | Harness-specific personality overrides |
| `.harness/AGENTS.md` | Harness-specific instruction overrides |

**SOUL.md vs AGENTS.md:** SOUL.md defines the agent's *personality* (how it communicates, its tone, values, language preferences). AGENTS.md defines *project instructions* (coding standards, architecture, tools to use). SOUL.md appears first in the system prompt so it takes behavioral priority.

Files closer to the project root have higher priority (override parent directories).

---

## 14. Tips & Best Practices

### For Best Results

1. **Write good AGENTS.md** — include coding standards, project architecture, and preferred patterns
2. **Write a SOUL.md** — define the agent's personality, communication style, and language preferences for a more tailored experience
2. **Use `/cost` regularly** — track spending, especially with expensive models
3. **Start with strict mode** — switch to auto/yolo once you trust the agent (use `/permission auto`)
4. **Use the memory vault** — let the agent remember across sessions for better context
5. **Set up `.agentignore`** — prevent the agent from touching sensitive files

### One-Shot Mode Tips

```bash
# Great for CI/scripts:
momo-fetch -p "Generate a PR summary from recent commits" --permission yolo

# Pipe content for analysis:
cat error.log | momo-fetch -p "What caused this error?"

# Combine with other tools:
momo-fetch -p "Review all modified files" --model gpt-4o
```

### Switching Models Strategically

- Use **Claude** for complex reasoning and code generation
- Use **GPT-4o** for general tasks
- Use **Ollama** for offline/private work
- Use **Groq** for fast, simple tasks
- Use **DeepSeek** for cost-effective coding
- Use **z.ai** for the z.ai coding endpoint (set `ZAI_LLM_URL` to override the default base URL)
- Use **custom** for any OpenAI-compatible endpoint — just set `LLM_URL`, `LLM_APIKEY`, and optionally `LLM_MODEL`

### Memory Vault Tips

- The vault is just markdown files — back it up with git
- Browse it with Obsidian for a rich experience
- Check `/mem` periodically to see vault growth
- The `agentic` retrieval mode is best for complex multi-hop queries

### Syncing Memory Across Machines and Projects

The memory vault is a local folder of markdown files at `<project>/memory-vault/`. It is **per-project and per-machine** — MOMO Fetch does not use any cloud sync service. Each project has its own isolated vault.

**Obsidian-compatible means the file format** (YAML frontmatter + `[[wikilinks]]` + markdown) can be opened and browsed in Obsidian. It does NOT mean MOMO Fetch connects to Obsidian's cloud service.

#### Syncing the same project across machines

If you work on the same project from multiple machines, use one of these approaches:

**Git (recommended):**
```bash
# Add the vault to your repo
git add memory-vault/
git commit -m "Add memory vault"
git push

# On another machine
git pull  # memories synced
```

**File sync (iCloud, Dropbox, Google Drive):**
```bash
# Symlink the vault to a synced folder
ln -s ~/Dropbox/momo-vaults/my-app ./memory-vault
```

**Obsidian Sync (paid service):**
If you have an Obsidian Sync subscription, you can open the `memory-vault/` folder as an Obsidian vault and enable sync. Obsidian Sync will keep the files in sync across machines — but this is Obsidian's feature, not MOMO Fetch's.

#### Sharing memory between projects

By default, each project has its own isolated vault. To share memories across projects:

**Symlink to a shared vault:**
```bash
# All projects point to the same vault location
ln -s ~/shared-memory-vault ./memory-vault
```

**Copy specific memories manually:**
```bash
# Copy relevant memories from project A to project B
cp project-a/memory-vault/2-events/fact-0042.md project-b/memory-vault/2-events/
```

#### What does NOT sync automatically

- Memories from Project A do NOT appear in Project B
- Switching machines does NOT bring your memories unless you sync the files
- Logging into Obsidian (the app) does NOT connect MOMO Fetch to any cloud service

### Memory Auto-Flow (Active Memory)

The agent has an **active memory system** that automatically searches and writes memories. It has two independent toggles — `auto_search` and `auto_write` — giving you 4 possible modes.

#### All Configuration Fields

```jsonc
// .harness/settings.json
{
  "memory": {
    "auto_search": true,         // Pre-turn vault search (bool)
    "auto_write": true,          // Post-turn MemCell write (bool)
    "search_mode": "grep_llm",   // Retrieval mode: grep_llm | tag_filter
    "max_results_per_turn": 5,   // Max memories injected per turn
    "extract_threshold": 10,     // MemCells before auto-extract hint
    "sidecar_model": null,       // null = TF-IDF (Option A, $0 extra cost)
    "sidecar_provider": null     // Provider for sidecar model
  }
}
```

#### The 4 Modes

---

**Mode 1: `auto_search: true` + `auto_write: true` (default)**

Full active memory. The agent recalls past context and records new experiences every turn.

```
Pre-turn:
  User types: "fix the auth middleware"
       │
       ▼  vault.search() finds 3 relevant memories
  "--- Relevant memories ---
   - fact-0042 (score: 0.89): Fixed auth middleware JWT validation
   - 2026-05-06#MemCell 003 (score: 0.72): Deployed auth to staging
   - pred-0005 (score: 0.65): Predicted staging will need rate limiting
   ---
   fix the auth middleware"
       │
       ▼  LLM receives enriched input (memories + original message)

System prompt: includes "You have access to a persistent memory vault with N memories..."

Post-turn:
  Turn completes
       │
       ▼  write_turn_memory_option_a()
  MemCell written:
    topic:   "fix the auth middleware"
    context: "User asked: fix the auth middleware. Tools used: file_read(src/auth.rs)"
    actions: [{ description: "file_read(path=src/auth.rs)", result: "executed" }]
    outcome: "Fixed JWT validation by adding proper base64 decoding"
    keywords: ["auth", "middleware", "jwt", "validation", "rust"]
```

Best for: day-to-day coding sessions where you want full memory continuity.

---

**Mode 2: `auto_search: true` + `auto_write: false`**

The agent recalls past context, but does NOT save new experiences.

```
Pre-turn:
  Same as Mode 1 — memories injected before each turn.

Post-turn:
  Turn completes → auto_write_enabled() = false → skip
  No MemCell written.
```

Best for: quick Q&A sessions where you want recall but don't want to create noise in the vault.

---

**Mode 3: `auto_search: false` + `auto_write: true`**

The agent silently records experiences but does NOT inject past memories.

```
Pre-turn:
  User types: "fix the auth middleware"
       │
       ▼  auto_search = false → pass through unchanged
  LLM receives: "fix the auth middleware"  (no injected memories)

System prompt: includes memory vault section (because auto_write is true).

Post-turn:
  Same as Mode 1 — MemCell written after each turn.
```

Best for: silently building up memory for future sessions without bloating the current conversation with past context.

---

**Mode 4: `auto_search: false` + `auto_write: false`**

No automatic memory involvement at all. The agent must manually call `mem_search` / `mem_write` tools.

```
Pre-turn:
  No search, no injection.

System prompt:
  No memory section added.

Post-turn:
  No write.
```

Best for: when you want full control over when memory is accessed. Same behavior as before the auto-flow feature existed.

---

#### Quick Reference

| Mode | `auto_search` | `auto_write` | Pre-turn inject | Post-turn write | System prompt |
|:----:|:---:|:---:|:---:|:---:|:---:|
| 1 | `true` | `true` | yes | yes | includes memory section |
| 2 | `true` | `false` | yes | no | includes memory section |
| 3 | `false` | `true` | no | yes | includes memory section |
| 4 | `false` | `false` | no | no | no memory section |

In all 4 modes, `mem_search` and `mem_write` tools remain available for manual use.

#### Sidecar Options

**Cheap cloud model for extraction (Option B):**

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

**Local model (zero extra cost):**

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

#### Separate Process (Option C)

Run the memory sidecar as an independent process communicating via file-based Mailbox:

```bash
# Terminal 1: Main agent
momo-fetch --project /path/to/project

# Terminal 2: Memory sidecar (separate process)
momo-fetch --mode memory-sidecar --project /path/to/project
```

---

## 15. FAQ

### How do I start testing MOMO Fetch?

```bash
# 1. Build the binary
cargo build

# 2. Run all tests
cargo test

# 3. Start using it
./target/debug/momo-fetch              # REPL mode (interactive)
./target/debug/momo-fetch -p "hello"   # One-shot mode (single prompt)

# 4. Install globally (optional, so you can run `momo-fetch` from anywhere)
cargo install --path .
```

You need at least one LLM provider key set as an env var (e.g. `ANTHROPIC_API_KEY`, `OPENAI_API_KEY`, `DEEPSEEK_API_KEY`) or in a `.env` file in the project directory.

### What is REPL mode vs One-shot mode?

**REPL mode** (default) — interactive conversation where you chat back and forth. The agent remembers context within the session.

```
$ momo-fetch

  MoMo Fetch 0.1.0 — anthropic (claude-sonnet-4-20250514)
  Session: 3f8a1b2c
  Type /help for commands, Ctrl+D to quit.

claude-sonnet-4-20250514> read main.rs and explain it
  ⏺ file_read(path="src/main.rs")
  → 34 lines read

The main.rs file...

claude-sonnet-4-20250514> now add a --verbose flag
  ⏺ file_edit(path="src/main.rs", ...)

Done!

claude-sonnet-4-20250514> /cost
Session: $0.05 (3,200 tokens)

claude-sonnet-4-20250514> /quit
Goodbye!
```

**One-shot mode** — single prompt, then exit immediately.

```bash
$ momo-fetch -p "explain main.rs"

The main.rs file contains the entry point...
# exits immediately
```

| | REPL | One-shot |
|---|---|---|
| Trigger | `momo-fetch` | `momo-fetch -p "..."` |
| Turns | Unlimited (until `/quit`) | 1 turn, then exit |
| Context | Remembers previous messages | No prior context |
| Use case | Day-to-day coding | Scripts, CI, quick questions |
| Pipe support | No | Yes: `cat file \| momo-fetch -p "review"` |
| Exit code | Always 0 | 0 = success, 1 = error |

### What does `cargo install --path .` do?

It builds the binary in release mode (optimized) and copies it to `~/.cargo/bin/momo-fetch`. Since that directory is already in your `$PATH`, you can then run `momo-fetch` from anywhere on your system.

You must run this command from the project root directory (where `Cargo.toml` exists).

```bash
# Before: can only run from the project directory
./target/debug/momo-fetch

# After cargo install --path .
momo-fetch        # works from any directory
```

To uninstall later:
```bash
cargo uninstall momo-fetch
```

### What does `momo-fetch --project ~/my-project` do?

It starts MOMO Fetch with your project set to `~/my-project` instead of the current directory. The agent can only read/write files inside that folder (it's sandboxed).

Use it when your code lives in one place but you launched MOMO Fetch from somewhere else:

```bash
# You're in your home directory
cd ~

# But want to work on a specific project
momo-fetch --project ~/workspace/my-app

# Agent can only access files in ~/workspace/my-app
```

Without `--project`, it uses your current directory as the project root.

### What does "project config and memory" mean?

Every project has its own config and memory vault. When you point `--project` to a folder, MOMO Fetch reads from and writes to that project's `.harness/` and `memory-vault/` directories.

```
~/workspace/my-app/              ← your project
├── .harness/
│   ├── settings.json            ← project-specific settings (model, memory config, etc.)
│   ├── mcp.json                 ← MCP servers for this project
│   └── skills/                  ← skills for this project
├── memory-vault/                ← this project's memories only
│   ├── 1-memcells/
│   ├── 2-events/
│   └── ...
└── src/                           ← your project files
```

Each project has its own separate memory vault. Memories from `my-app` don't mix with memories from `other-app`:

```bash
# Project A has its own config + memories
momo-fetch --project ~/workspace/my-app

# Project B has different config + memories
momo-fetch --project ~/workspace/other-app
```

Global settings (shared across all projects) live at `~/.config/momo-fetch/settings.json`.

Priority: CLI flags > project `.harness/settings.json` > global `~/.config/momo-fetch/settings.json`
