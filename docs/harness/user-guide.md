# User Guide — MOMO Fetch

Everything you need to know to use MOMO Fetch effectively.

> Looking for the code rather than the product? [`project-landscape.md`](../project-landscape.md)
> maps the modules and how they depend on each other.

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
17. [Gateway & Cloudflare Tunnel](#17-gateway--cloudflare-tunnel)
13. [Configuration Reference](#13-configuration-reference)
14. [Custom Slash Commands](#14-custom-slash-commands)
15. [Tips & Best Practices](#15-tips--best-practices)
16. [FAQ](#16-faq)

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

By default (strict mode), the agent asks before every write/shell action. A thinking spinner shows while waiting for the LLM to respond:

```
  ⠹ Thinking
  ⏺ file_write(path="src/lib.rs")
  ! Tool file_write requires approval: path="src/lib.rs"

  ? Allow file_write to proceed? [y/n]
y

  ⠹ Thinking
  → File updated successfully
```

Once you approve a tool, it won't ask again for the same tool name during the session. Type `n` to deny.

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

### Managing Context

When the context window fills up, use these commands to free space:

| Command | What it does |
|---------|-------------|
| `/clear` | Drop all conversation history and start a fresh session |
| `/compact` | Summarize the current context into a compacted new session |

`/compact` preserves a summary of the conversation so the agent retains key context, while `/clear` starts completely fresh.

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

MCP (Model Context Protocol) servers extend the agent with additional tools. MOMO Fetch supports both **stdio** (local child process) and **HTTP/SSE** (remote server) transports.

### Adding an MCP Server

**Stdio server** (local process):

```
gpt-4o> /mcp add filesystem npx -y @anthropic/mcp-filesystem /tmp
✓ Added MCP server 'filesystem' and starting...
```

**HTTP server** (remote endpoint) — configured via `.harness/mcp.json`:

```json
{
  "mcpServers": {
    "filesystem": {
      "command": "npx",
      "args": ["-y", "@anthropic/mcp-filesystem", "/tmp"],
      "env": {},
      "disabled": false
    },
    "zread": {
      "type": "http",
      "url": "https://api.z.ai/api/mcp/zread/mcp",
      "headers": {
        "Authorization": "Bearer your-api-key"
      }
    }
  }
}
```

**Stdio servers** use `command` + `args` to spawn a local process. **HTTP/SSE servers** use `"type": "http"` (or `"sse"`) with a `url` and optional `headers` for authentication (e.g., Bearer tokens).

### Managing Servers

```
gpt-4o> /mcp list
MCP servers:
  filesystem: Running
    command: npx -y @anthropic/mcp-filesystem /tmp

gpt-4o> /mcp remove filesystem
✓ Removed MCP server 'filesystem'
```

### Testing MCP Connections

Use `--test-mcp` to verify all configured servers connect successfully without entering the REPL:

```bash
momo-fetch --test-mcp
MCP Connection Test
──────────────────────────────────────────────────

Stdio servers:
  ✓ zai-mcp-server: Running
    command: npx -y @z_ai/mcp-server

HTTP servers:
  ✓ zread (type: http)
    url: https://api.z.ai/api/mcp/zread/mcp
    headers: Authorization
  ✓ web-reader (type: http)
    url: https://api.z.ai/api/mcp/web_reader/mcp
    headers: Authorization

──────────────────────────────────────────────────
Total tools available: 13
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
| `commands/` | Custom slash command files (`.md`) |
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

## 14. Custom Slash Commands

Define your own slash commands by placing `.md` files in `.harness/commands/`. Each file becomes a `/command` you can invoke in the REPL.

### Creating a Custom Command

```bash
mkdir -p .harness/commands
```

Create `.harness/commands/review.md`:

```markdown
Review the following code for bugs, security issues, and performance: $ARG
```

Create `.harness/commands/status.md` (no argument needed):

```markdown
Summarize the current project status, recent changes, and any open issues.
```

### Using Custom Commands

```
you> /review src/auth.rs
you> /status
```

The `$ARG` placeholder is replaced with everything typed after the command name. If no `$ARG` exists, the content is sent as-is.

### Rules

- Command names must be alphanumeric with dashes or underscores
- Custom commands cannot override built-in commands (`/help`, `/model`, etc.)
- Files must end in `.md` and be placed in `.harness/commands/`
- Use `/help` to see both built-in and custom commands
- Changes are picked up immediately — no restart needed

---

## 15. Tips & Best Practices

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
    "extract_threshold": 10,     // MemCells before auto-extract (events/foresights/episodes)
    "sidecar_model": null,       // null = TF-IDF (Option A, $0 extra cost)
    "sidecar_provider": null     // Provider for sidecar model
    "consolidate_threshold": 30, // MemCells before auto-consolidate (clusters/profile), 0 = disabled
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
       ▼  write_turn_memory()
       (Option C -> Option B -> Option A, with fallback)
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

When `sidecar_model` is set, the system makes a direct LLM call for memory extraction
instead of using TF-IDF. Falls back to TF-IDF (Option A) on any error.

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

Run the memory sidecar as an independent process communicating via file-based Mailbox.
The main process auto-detects the sidecar via a `ready` signal and routes requests through IPC.
Falls back to in-process extraction if the sidecar is unavailable.

```bash
# Terminal 1: Main agent
momo-fetch --project /path/to/project

# Terminal 2: Memory sidecar (separate process)
momo-fetch --mode memory-sidecar --project /path/to/project
```

#### Auto-Extract and Auto-Consolidate

The system automatically processes raw MemCells into higher-level structures at configurable thresholds:

- **Auto-extract** (`extract_threshold`, default: 10): Every N MemCells, extracts events, foresights, and episodes.
- **Auto-consolidate** (`consolidate_threshold`, default: 30): Every N MemCells, clusters related MemCells and updates profiles.
- Set `consolidate_threshold: 0` to disable auto-consolidate.

```json
{
  "memory": {
    "extract_threshold": 5,
    "consolidate_threshold": 20
  }
}
```

---

## 16. FAQ

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

---

## 17. Gateway & Cloudflare Tunnel

The gateway exposes MOMO Fetch's agent capabilities as an HTTP API, compatible with the OpenAI Chat Completions format. Combined with Cloudflare Tunnel, you can securely expose the agent to the internet without opening any firewall ports.

### Architecture

```
Clients (Discord bot, Web UI, SDK, curl)
         ↓ HTTPS
  Cloudflare Tunnel (trycloudflare.com or custom domain)
         ↓ HTTP
  ┌──────────────────────────────────┐
  │   MOMO Gateway (axum :3000)       │
  │  ┌──────────────────────────────┐ │
  │  │ Auth / Rate Limit            │ │
  │  ├──────────────────────────────┤ │
  │  │ /v1/chat/completions        │ │  ← OpenAI-compatible
  │  │ /v1/chat/completions/stream  │ │  ← SSE streaming
  │  │ /v1/sessions                 │ │  ← session management
  │  │ /v1/models                   │ │
  │  │ /v1/cost                     │ │
  │  │ /health                     │ │
  │  └──────────────────────────────┘ │
  │              ↓                    │
  │    harness.rs (agent core)        │
  │    tools, memory, MCP, vault      │
  └──────────────────────────────────┘
```

### Starting the Gateway

```bash
# Start gateway on default port 3000
momo-fetch --gateway

# Start with custom project and permission mode
momo-fetch --gateway --project ~/my-app --permission auto

# Start with a specific provider/model
momo-fetch --gateway --provider anthropic --model claude-sonnet-4-20250514

# Let the OS pick a free port, and print it on startup
momo-fetch --gateway --gateway-port 0
```

The gateway starts the full harness (tools, memory vault, MCP servers, skills) and wraps it in an HTTP API.

| Flag | What it does |
|------|--------------|
| `--gateway-port <PORT>` | Overrides the configured port. `0` lets the OS choose; the chosen port is printed as `MOMO_GATEWAY_LISTENING <url>`. |
| `--gateway-bind <IP>` | Interface to bind. **Anything other than loopback requires `auth.enabled`** — the gateway refuses to start otherwise, because it can execute shell commands. |
| `--gateway-allow-origin <ORIGIN>` | Grants one CORS origin for this run only, without editing `gateway.json`. This is how the desktop app admits its own webview (`tauri://localhost`). |
| `--project <DIR>` | Roots the sandbox. Required when the launcher's working directory is not the project — a macOS `.app` starts in `/`. |

### The bundled web UI

If a frontend build exists (`web/out` by default, `ui_dir` in `gateway.json`), the gateway serves it at **`/ui`** on its own origin. Same origin means it needs no CORS entry.

```bash
cd web && npm run build      # basePath /ui — the gateway build
momo-fetch --gateway         # → http://localhost:3000/ui/
```

`/ui` redirects to `/ui/`. The trailing slash is load-bearing: assets are referenced relatively so the same bundle works under the gateway at `/ui` *and* under the desktop shell at `/`, and a browser resolves relative URLs against the last path segment.

Building for the desktop app is a different command — `npm run build:desktop`, which sets no basePath. Use the wrong one and every asset 404s. See [`docs/desktop/momo-desktop-install.md`](../desktop/momo-desktop-install.md).

### Endpoints

**v1 — OpenAI-compatible.** Stable surface for scripts, SDKs and the Discord bot.

| Method | Endpoint | Description |
|--------|----------|-------------|
| `POST` | `/v1/chat/completions` | Chat (OpenAI-compatible, non-streaming) |
| `POST` | `/v1/chat/completions/stream` | Chat (SSE streaming) |
| `GET` | `/v1/sessions` | List all sessions |
| `POST` | `/v1/sessions` | Create a new session |
| `GET` | `/v1/sessions/:id` | Get session details + messages |
| `DELETE` | `/v1/sessions/:id` | Delete a session |
| `POST` | `/v1/sessions/:id/compact` | Compact (summarize) a session |
| `GET` | `/v1/models` | List current model info |
| `GET` | `/v1/cost` | Cost tracking for current session |
| `GET` | `/health` | Health check — **unauthenticated**, so a readiness probe works before a key is configured. Exposes no session content. |

**v2 — the rich surface.** What the web UI and desktop app are built on: tool-level streaming, an approval round trip, and live control over provider, model, agent and permissions.

| Method | Endpoint | Description |
|--------|----------|-------------|
| `POST` | `/v2/chat/stream` | Streaming turn with per-tool events (see the v2 event list below) |
| `POST` | `/v2/chat/approve` | Approve a pending tool call |
| `POST` | `/v2/chat/deny` | Deny a pending tool call |
| `POST` | `/v2/chat/interrupt` | Stop the running turn |
| `GET` | `/v2/agents` | Configured agent personalities |
| `POST` | `/v2/agents/switch` | Switch the active agent |
| `POST` | `/v2/agents/default` | Return to the default agent |
| `GET` | `/v2/providers` | Providers, with availability and *why* an unavailable one is unavailable |
| `GET` | `/v2/providers/:provider/models` | **Model catalogue, queried live from the provider.** Cached 30 minutes |
| `DELETE` | `/v2/providers/:provider/models` | Drop the cached catalogue so the next read refetches |
| `POST` | `/v2/switch-model` | Change model within the current provider |
| `POST` | `/v2/switch-provider` | Change provider |
| `POST` | `/v2/switch` | Change both at once |
| `GET` | `/v2/settings` | Permission mode + the approved-tools set |
| `POST` | `/v2/settings/permission` | Set `strict` \| `auto` \| `yolo` |
| `DELETE` | `/v2/settings/approved-tools` | Revoke every standing approval |
| `GET` | `/v2/mcp/servers` | MCP server status |
| `GET` | `/v2/memory/search` | Search the memory vault |
| `GET` | `/v2/memory/stats` | Vault counts |
| `GET` | `/v2/files` | Read one sandboxed file |
| `GET` | `/v2/files/tree` | List a sandboxed directory |
| `GET` | `/v2/sessions/:id/messages` | Replay a session's messages |

#### Model catalogue

`/v2/providers/:provider/models` asks the provider itself rather than returning a baked-in list, which would be wrong the week after it was written. OpenRouter, Ollama, Anthropic and Gemini each have their own shape; everything else is queried as OpenAI-compatible `GET {base}/models`.

**A failure here is a `200`, not an error.** No key, provider unreachable, or no catalogue API at all comes back as `available: false` with the reason and the model currently in use, so a UI can fall back to letting someone type a name. A directory lookup failing must never leave you unable to change models.

```bash
curl http://localhost:3000/v2/providers/openrouter/models
# {"provider":"openrouter","models":[...],"available":true,"cached":false,"error":null}

curl http://localhost:3000/v2/providers/ollama/models
# {"provider":"ollama","models":["llama3.2"],"available":false,...,"error":"Ollama is not running."}
```

#### Files are sandboxed, and denials are 403

`/v2/files` and `/v2/files/tree` refuse anything outside the project root, anything matched by `.agentignore` or any `.gitignore` between the root and the file, and an unconditional deny-list of secret-shaped paths (`.env`, keys, credentials) that applies even if you un-ignore them. Denials return **403, never 404** — a 404 would leak whether the path exists.

### Chat Completions (Non-Streaming)

```bash
curl http://localhost:3000/v1/chat/completions \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer sk-momo-xxx" \
  -d '{
    "model": "momo-fetch",
    "messages": [
      {"role": "user", "content": "scan the project and summarize it"}
    ]
  }'
```

Response (OpenAI-compatible):

```json
{
  "id": "chatcmpl-abc123",
  "object": "chat.completion",
  "created": 1700000000,
  "model": "anthropic/claude-sonnet-4-20250514",
  "choices": [{
    "index": 0,
    "message": {
      "role": "assistant",
      "content": "Here's a summary of your project..."
    },
    "finish_reason": "stop"
  }],
  "usage": {
    "prompt_tokens": 0,
    "completion_tokens": 0,
    "total_tokens": 0
  }
}
```

### Chat Completions (SSE Streaming)

```bash
curl http://localhost:3000/v1/chat/completions/stream \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer sk-momo-xxx" \
  -d '{
    "model": "momo-fetch",
    "messages": [
      {"role": "user", "content": "explain the architecture"}
    ]
  }'
```

Response (Server-Sent Events):

```
data: {"id":"chatcmpl-abc","object":"chat.completion.chunk","created":1700000000,"model":"anthropic/claude-sonnet-4-20250514","choices":[{"index":0,"delta":{"role":"assistant","content":null},"finish_reason":null}]}

data: {"id":"chatcmpl-abc","object":"chat.completion.chunk","created":1700000000,"model":"anthropic/claude-sonnet-4-20250514","choices":[{"index":0,"delta":{"role":null,"content":"This"},"finish_reason":null}]}

data: {"id":"chatcmpl-abc","object":"chat.completion.chunk","created":1700000000,"model":"anthropic/claude-sonnet-4-20250514","choices":[{"index":0,"delta":{"role":null,"content":" project"},"finish_reason":null}]}

data: {"id":"chatcmpl-abc","object":"chat.completion.chunk","created":1700000000,"model":"anthropic/claude-sonnet-4-20250514","choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}

data: [DONE]
```

### Session Management

Each request creates a new session automatically. To continue a conversation, pass the `session_id` in the request body:

```bash
# First message
curl http://localhost:3000/v1/chat/completions \
  -H "Content-Type: application/json" \
  -d '{
    "messages": [{"role": "user", "content": "hello"}]
  }'
# Response includes session_id in the "id" field: "chatcmpl-<session_id>"

# Follow-up (use the session_id from the first response)
curl http://localhost:3000/v1/chat/completions \
  -H "Content-Type: application/json" \
  -d '{
    "session_id": "abc123",
    "messages": [{"role": "user", "content": "now explain the architecture"}]
  }'
```

You can also manage sessions explicitly:

```bash
# List sessions
curl http://localhost:3000/v1/sessions

# Create a new session
curl -X POST http://localhost:3000/v1/sessions

# Get session messages
curl http://localhost:3000/v1/sessions/abc123

# Compact (summarize) a session to free context
curl -X POST http://localhost:3000/v1/sessions/abc123/compact

# Delete a session
curl -X DELETE http://localhost:3000/v1/sessions/abc123
```

### Configuration

Create `.harness/gateway.json` in your project to configure the gateway:

```jsonc
{
  // Port to listen on (default: 3000)
  "port": 3000,

  // CORS origins (default: ["*"] = allow all)
  "cors_origins": ["https://my-app.com", "https://discord-bot.example.com"],

  // Authentication
  "auth": {
    // Enable API key auth (default: false)
    "enabled": true,

    // API key definitions
    "keys": {
      "sk-momo-discord-bot": {
        "name": "Discord Bot",
        "rate_limit": 60,      // max 60 requests/minute
        "daily_quota": 10000  // max 10,000 requests/day
      },
      "sk-momo-web-ui": {
        "name": "Web UI",
        "rate_limit": 120,
        "daily_quota": 50000
      },
      "sk-momo-internal": {
        "name": "Internal Services",
        "rate_limit": 0,       // 0 = unlimited
        "daily_quota": 0
      }
    }
  }
}
```

#### Config Fields

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `port` | `u16` | `3000` | HTTP listen port |
| `cors_origins` | `string[]` | `[]` | Allowed CORS origins. `[]` = same-origin only (the bundled `/ui` needs no entry). `"*"` requires `auth.enabled` — the gateway refuses to start otherwise, since it would let any visited website read your files and run shell commands. |
| `auth.enabled` | `bool` | `false` | Enable Bearer token authentication |
| `auth.keys` | `map` | `{}` | API key → metadata mapping |
| `auth.keys.*.name` | `string` | `""` | Human-readable key name |
| `auth.keys.*.rate_limit` | `u32` | `0` | Max requests/minute (0 = unlimited) |
| `auth.keys.*.daily_quota` | `u32` | `0` | Max requests/day (0 = unlimited) |

#### Auth Behavior

- When `auth.enabled` is `false` (default), all requests are accepted without authentication.
- When `auth.enabled` is `true`, every request must include an `Authorization: Bearer <key>` header.
- If the key is missing or invalid, the gateway returns `401 Unauthorized`.
- If a rate limit or daily quota is exceeded, the gateway returns `429 Too Many Requests`.

### Setting Up Cloudflare Tunnel

Cloudflare Tunnel exposes your local gateway to the internet via Cloudflare's edge network. No public IP, no open ports needed. Cloudflare handles TLS, DDoS protection, and routing automatically.

There are **3 options** depending on your situation:

| Option | Account needed? | Domain needed? | URL | Persistent? | Best for |
|--------|:---:|:---:|------|:---:|----------|
| **A. Quick Tunnel** | No | No | `*.trycloudflare.com` (random) | No | Testing, demos, quick share |
| **B. Named Tunnel** | Yes | Yes | `api.yourdomain.com` | Yes | Production, persistent public URL |
| **C. Named Tunnel (No Config)** | Yes | No | `*.cfargotunnel.com` | Yes | Persistent but no custom domain |

---

#### Install cloudflared

```bash
# macOS
brew install cloudflared

# Linux (Debian/Ubuntu)
curl -L https://github.com/cloudflare/cloudflared/releases/latest/download/cloudflared-linux-amd64.deb -o cloudflared.deb
sudo dpkg -i cloudflared.deb

# Linux (RHEL/CentOS/Fedora)
curl -L https://github.com/cloudflare/cloudflared/releases/latest/download/cloudflared-linux-amd64.rpm -o cloudflared.rpm
sudo rpm -i cloudflared.rpm

# Arch Linux
pacman -S cloudflared

# Or download any platform from https://github.com/cloudflare/cloudflared/releases

# Verify
cloudflared --version
```

---

#### Option A: Quick Tunnel (No Account, No Domain)

The fastest way. Zero setup — just run one command and get a public URL instantly.

**Prerequisites:** None (no Cloudflare account, no domain)

```bash
# Terminal 1: Start the gateway
momo-fetch --gateway --project ~/my-app
# 🚀 MOMO Gateway listening on http://127.0.0.1:3000

# Terminal 2: Start a quick tunnel
cloudflared tunnel --url http://localhost:3000
```

Output:

```
2024-01-15T10:00:00Z INF Starting tunnel tunnelID=xxx
2024-01-15T10:00:01Z INF +----------------------------+
2024-01-15T10:00:01Z INF |  Your quick tunnel has been created! Visit it at:
2024-01-15T10:00:01Z INF |  https://random-words-abc123.trycloudflare.com
2024-01-15T10:00:01Z INF +----------------------------+
```

Your gateway is now publicly accessible:

```bash
curl https://random-words-abc123.trycloudflare.com/health
# {"status":"ok","version":"0.8.0"}

curl https://random-words-abc123.trycloudflare.com/v1/chat/completions \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer sk-momo-xxx" \
  -d '{
    "messages": [{"role": "user", "content": "hello from the internet!"}]
  }'
```

**Pros:** Zero config, works immediately
**Cons:** URL changes every restart, no custom domain, rate limited by Cloudflare

> **⚠️ Quick tunnels are ephemeral.** The URL changes every time you restart `cloudflared`. Use Option B or C for persistent URLs.

---

#### Option B: Named Tunnel with Custom Domain (Production)

Permanent URL with your own domain. Requires a Cloudflare account and a domain managed by Cloudflare.

**Prerequisites:**
- Cloudflare account (free plan works)
- A domain added to your Cloudflare dashboard (Free DNS)
- `cloudflared` installed

##### If you already have a Cloudflare account and cloudflared installed:

```bash
# Check if you're already logged in
cloudflared tunnel list
# If this shows your tunnels, skip to "If you already have a tunnel"

# If it says "error", you need to login first
cloudflared tunnel login
# → Opens browser → select your account and domain → done
```

##### Step-by-step setup:

**Step 1: Login to Cloudflare**

```bash
cloudflared tunnel login
# Opens your browser → select your Cloudflare account → choose the zone (domain) to authorize
# Certificate saved to ~/.cloudflared/cert.pem
```

> 💡 **Already logged in?** If you ran `cloudflared tunnel login` before (or used Cloudflare WARP / Zero Trust), you may already have `~/.cloudflared/cert.pem`. Run `cloudflared tunnel list` to check — if it works, skip to Step 2.

**Step 2: Create a tunnel**

```bash
cloudflared tunnel create momo-gateway
# Output:
# Tunnel credentials written to ~/.cloudflared/<tunnel-id>.json
# Created tunnel momo-gateway with id <tunnel-id>
```

> 💡 **Already have a tunnel?** If you already created a tunnel before, run `cloudflared tunnel list` to see it. You can reuse an existing tunnel — just add a new ingress rule in the config (Step 4).

**Step 3: Route DNS to the tunnel**

```bash
# This creates a CNAME record automatically in your Cloudflare DNS
cloudflared tunnel route dns momo-gateway api.yourdomain.com
# Output: Route created for api.yourdomain.com -> tunnel <tunnel-id>
```

> If you want multiple subdomains (e.g., `api.domain.com` and `webhook.domain.com`), run this command for each one, then configure multiple ingress rules in Step 4.

**Step 4: Create config file**

Create `~/.cloudflared/config.yml`:

```yaml
# ~/.cloudflared/config.yml
tunnel: <tunnel-id>                       # from Step 2 (e.g., a1b2c3d4-5e6f-...)
credentials-file: ~/.cloudflared/<tunnel-id>.json

ingress:
  # Route your domain to the gateway
  - hostname: api.yourdomain.com
    service: http://localhost:3000

  # Optional: route another hostname (e.g., health check endpoint without auth)
  # - hostname: status.yourdomain.com
  #   service: http://localhost:3000

  # Catch-all: return 404 for unmatched hostnames (required)
  - service: http_status:404
```

**Step 5: Run the tunnel**

```bash
# Terminal 1: Gateway
momo-fetch --gateway --project ~/my-app

# Terminal 2: Tunnel
cloudflared tunnel run momo-gateway
# Output:
# INF Connection registered tunnel=<tunnel-id> connIndex=0
# INF Started tunnel tunnelID=<tunnel-id> name=momo-gateway
```

Verify:

```bash
curl https://api.yourdomain.com/health
# {"status":"ok","version":"0.8.0"}
```

Your gateway is now permanently at `https://api.yourdomain.com`. 🎉

---

#### Option C: Named Tunnel without Custom Domain

Same as Option B but skips DNS routing. You get a stable `*.cfargotunnel.com` URL instead of a custom domain. Useful if you don't have a domain but want a persistent URL.

**Prerequisites:** Cloudflare account only (no domain needed)

**Step 1: Login**

```bash
cloudflared tunnel login
# Opens browser → select your account (any zone, even a free one)
```

**Step 2: Create a tunnel**

```bash
cloudflared tunnel create momo-gateway
```

**Step 3: Create config (no hostname needed)**

Create `~/.cloudflared/config.yml`:

```yaml
# ~/.cloudflared/config.yml
tunnel: <tunnel-id>
credentials-file: ~/.cloudflared/<tunnel-id>.json

ingress:
  # No hostname = catches all traffic on the tunnel's default .cfargotunnel.com URL
  - service: http://localhost:3000
  - service: http_status:404
```

**Step 4: Run**

```bash
cloudflared tunnel run momo-gateway
```

Get your tunnel's public URL:

```bash
cloudflared tunnel info momo-gateway
# Look for the URL in the output
```

Or find it in your [Cloudflare Dashboard → Zero Trust → Networks → Tunnels](https://one.dash.cloudflare.com/?to=/:account/tunnels).

---

#### Choosing the Right Option

```
Need a quick test right now?
  → Option A (Quick Tunnel)
     No setup, no account, works in 10 seconds
     URL changes on restart

Want a permanent URL?
  → Have a domain?
      → Yes → Option B (Named + Custom Domain)
                api.yourdomain.com
                Most professional, production-ready

      → No → Option C (Named without Domain)
               *.cfargotunnel.com
               Persistent URL, no domain needed
```

---

#### Running Tunnel as a Background Service

##### macOS (launchd)

```bash
# Find the full path to cloudflared
which cloudflared
# e.g., /opt/homebrew/bin/cloudflared

# Create the plist
cat > ~/Library/LaunchAgents/com.cloudflare.cloudflared.plist << EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>com.cloudflare.cloudflared</string>
    <key>ProgramArguments</key>
    <array>
        <string>/opt/homebrew/bin/cloudflared</string>
        <string>tunnel</string>
        <string>run</string>
        <string>momo-gateway</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <true/>
</dict>
</plist>
EOF

# Load it (starts immediately)
launchctl load ~/Library/LaunchAgents/com.cloudflare.cloudflared.plist

# Check status
launchctl list | grep cloudflared

# Stop it later
launchctl unload ~/Library/LaunchAgents/com.cloudflare.cloudflared.plist
```

##### Linux (systemd)

```bash
# cloudflared installs the service automatically on Debian/RPM
# Just enable it:
sudo cloudflared service install
sudo systemctl enable cloudflared
sudo systemctl start cloudflared

# Check status
sudo systemctl status cloudflared

# View logs
sudo journalctl -u cloudflared -f
```

##### Run Gateway + Tunnel Together (script)

The repo ships one: [`scripts/start-gateway.sh`](../../scripts/start-gateway.sh).

```bash
./scripts/start-gateway.sh                        # gateway :3000 + named tunnel
./scripts/start-gateway.sh --port 8080            # different port
./scripts/start-gateway.sh --gateway-only         # no tunnel, no cloudflared needed
./scripts/start-gateway.sh --tunnel-only          # tunnel against an already-running gateway
./scripts/start-gateway.sh --domain api.you.com --tunnel-name momo-gateway
```

It loads the project `.env`, waits for `/health` to answer before bringing the
tunnel up rather than sleeping and hoping, and kills the tunnel on `Ctrl-C`.

> This guide used to paste its own copy of the script here. Two copies drifted,
> and both got the gateway's port flag wrong — it is `--gateway-port`, and
> `--port` has never existed. One script, in the repo, where it can be run and
> fixed.

---

#### Cloudflare Dashboard Management

You can also manage tunnels from the [Cloudflare Dashboard](https://one.dash.cloudflare.com/?to=/:account/tunnels):

1. Go to **Zero Trust → Networks → Tunnels**
2. Click **Create a tunnel**
3. Choose **Cloudflared** → name it → install connector
4. Configure **Public Hostname** → `api.yourdomain.com` → `http://localhost:3000`
5. Save

This is equivalent to Option B but done through the web UI. The dashboard is also where you can:
- Monitor tunnel health and connection status
- View request logs and analytics
- Add Access policies (Cloudflare Zero Trust) for extra security
- Configure multiple public hostnames per tunnel

#### Using Cloudflare Zero Trust Access (Optional Extra Security)

For additional security on top of API key auth, you can add Cloudflare Access policies:

1. Go to **Zero Trust → Access → Applications**
2. Create an application protecting `api.yourdomain.com`
3. Add a policy (e.g., allow specific emails, IP ranges, or service tokens)
4. Now users must pass both Cloudflare Access AND your gateway's Bearer token auth

This gives you defense-in-depth: Cloudflare blocks unauthorized users at the edge, and your gateway validates API keys for authorized ones.

---

### Integrating with Discord

The gateway's OpenAI-compatible API makes it easy to build a Discord bot that talks to your agent.

#### Minimal Discord Bot Example (Node.js)

```javascript
// discord-bot.mjs
import Discord from 'discord.js';

const client = new Discord.Client({
  intents: [Discord.GatewayIntentBits.Guilds, Discord.GatewayIntentBits.MessageContent],
});

const GATEWAY_URL = process.env.GATEWAY_URL || 'http://localhost:3000';
const API_KEY = process.env.GATEWAY_API_KEY || '';

client.on('messageCreate', async (message) => {
  // Ignore bot messages
  if (message.author.bot) return;

  // Only respond when mentioned or in DM
  if (message.guild && !message.mentions.has(client.user)) return;

  // Strip the mention prefix
  const content = message.content.replace(/<@\d+>\s*/, '').trim();
  if (!content) return;

  // Show typing indicator
  await message.channel.sendTyping();

  try {
    const res = await fetch(`${GATEWAY_URL}/v1/chat/completions`, {
      method: 'POST',
      headers: {
        'Content-Type': 'application/json',
        'Authorization': `Bearer ${API_KEY}`,
      },
      body: JSON.stringify({
        messages: [{ role: 'user', content }],
        session_id: message.author.id,  // per-user session
      }),
    });

    const data = await res.json();
    const reply = data.choices?.[0]?.message?.content || 'No response';

    // Discord has a 2000 char limit — split if needed
    if (reply.length <= 2000) {
      await message.reply(reply);
    } else {
      // Send as a text file attachment
      const { Buffer } = await import('node:buffer');
      const buf = Buffer.from(reply, 'utf-8');
      await message.reply({
        content: 'Response too long, attached as file:',
        files: [new Discord.AttachmentBuilder(buf, { name: 'response.txt' })],
      });
    }
  } catch (err) {
    await message.reply(`Error: ${err.message}`);
  }
});

client.login(process.env.DISCORD_TOKEN);
```

```bash
# Run the bot
GATEWAY_URL=https://api.yourdomain.com \
GATEWAY_API_KEY=sk-momo-discord-bot \
DISCORD_TOKEN=your-discord-bot-token \
  node discord-bot.mjs
```

#### Streaming Response to Discord

For a typing-effect experience, use the streaming endpoint:

```javascript
const res = await fetch(`${GATEWAY_URL}/v1/chat/completions/stream`, {
  method: 'POST',
  headers: {
    'Content-Type': 'application/json',
    'Authorization': `Bearer ${API_KEY}`,
  },
  body: JSON.stringify({
    messages: [{ role: 'user', content }],
    session_id: message.author.id,
  }),
});

// Send initial message and edit it as chunks arrive
const botMessage = await message.reply('...');
let fullText = '';

const reader = res.body.getReader();
const decoder = new TextDecoder();

while (true) {
  const { done, value } = await reader.read();
  if (done) break;

  const chunk = decoder.decode(value);
  for (const line of chunk.split('\n')) {
    if (!line.startsWith('data: ')) continue;
    const data = line.slice(6);
    if (data === '[DONE]') break;

    try {
      const parsed = JSON.parse(data);
      const content = parsed.choices?.[0]?.delta?.content;
      if (content) {
        fullText += content;
        // Edit message with accumulated text (Discord rate limit: ~5 edits/5s)
        if (fullText.length % 50 < content.length) {
          await botMessage.edit(fullText + '▌');
        }
      }
    } catch {}
  }
}

await botMessage.edit(fullText); // final edit, remove cursor
```

### Connecting from Any OpenAI-Compatible Client

Since the gateway speaks the OpenAI Chat Completions protocol, you can use any OpenAI SDK:

```python
# Python (openai SDK)
from openai import OpenAI

client = OpenAI(
    base_url="https://api.yourdomain.com/v1",
    api_key="sk-momo-discord-bot",
)

response = client.chat.completions.create(
    model="momo-fetch",
    messages=[{"role": "user", "content": "scan my project"}],
)
print(response.choices[0].message.content)
```

```javascript
// TypeScript (openai SDK)
import OpenAI from 'openai';

const client = new OpenAI({
  baseURL: 'https://api.yourdomain.com/v1',
  apiKey: 'sk-momo-discord-bot',
});

const response = await client.chat.completions.create({
  model: 'momo-fetch',
  messages: [{ role: 'user', content: 'scan my project' }],
});
console.log(response.choices[0].message.content);
```

### Security Considerations

- **Always enable `auth.enabled: true`** when exposing the gateway publicly.
- **Use separate API keys** for each client (Discord bot, web UI, internal services) with appropriate rate limits.
- **Cloudflare Tunnel** provides DDoS protection and TLS termination automatically.
- **The gateway binds to `127.0.0.1`** by default — it's only accessible via the tunnel, not directly from the network.
- **Sandbox enforcement** still applies — the agent cannot access files outside the project directory.
- **Destructive command protection** remains active regardless of gateway mode.

### Troubleshooting

| Problem | Solution |
|---------|----------|
| `Connection refused` on gateway URL | Ensure `momo-fetch --gateway` is running and the port matches |
| `401 Unauthorized` | Check `Authorization: Bearer <key>` header and `gateway.json` config |
| `429 Too Many Requests` | Increase `rate_limit` or `daily_quota` for the key in `gateway.json` |
| Tunnel URL changed | Quick tunnels are ephemeral — use a named tunnel for persistence |
| `cloudflared: command not found` | Install via `brew install cloudflared` or download from GitHub releases |
| CORS errors from browser | Add your origin to `cors_origins` in `gateway.json` |
| Slow first response | The harness initializes MCP servers and memory vault on startup — wait for ready message |
