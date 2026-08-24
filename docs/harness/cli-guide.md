# CLI Guide

A step-by-step guide to using MOMO Fetch from the command line.

---

## Starting Up

### Interactive REPL (default)

```bash
momo-fetch                          # Use current directory as project
momo-fetch --project /path/to/repo  # Specify a project directory
```

### One-Shot Mode

```bash
momo-fetch -p "Explain this code"
momo-fetch -p "Summarize" < README.md    # Pipe stdin
cat file.rs | momo-fetch -p "Review"     # Alternative pipe syntax
```

One-shot mode prints the agent's response to stdout and tool calls to stderr. Exit code 0 on success, 1 on error.

### Subcommands

```
momo-fetch [OPTIONS]            # REPL, or one prompt with -p
momo-fetch team <ACTION>        # Headless team control, JSON on stdout
```

`team` is the only subcommand; with none given, every flag below behaves exactly
as it always has. See [Agent Teams](#agent-teams).

### CLI Flags

```
momo-fetch [OPTIONS]

Options:
  -p, --prompt <TEXT>         Run one prompt and exit
  -a, --agent <NAME>          Start as a specific agent specialist (from .harness/agents/)
      --model <NAME>          Override model (e.g., gpt-4o, claude-sonnet-4-20250514)
      --provider <NAME>       Override provider (anthropic, openai, deepseek, groq, ollama, openrouter, zai, custom)
      --project <PATH>        Working directory (default: current directory)
      --permission <MODE>     Permission mode: strict (default), auto, yolo
      --mode <MODE>           Run mode: default (repl) or memory-sidecar
      --resume <SESSION_ID>   Resume a previous session
      --test-mcp              Test MCP server connections and exit
      --gateway               Start the API gateway server
      --gateway-port <PORT>   Gateway port (0 = let the OS pick a free one)
      --gateway-bind <ADDR>   Gateway bind address (default: 127.0.0.1)
      --gateway-allow-origin <ORIGIN>  Extra allowed CORS origin (repeatable)
  -h, --help                  Show help
  -V, --version               Show version
```

---

## The REPL

When you start the REPL, you see:

```
                      /(
                     //\\
                    //   )_.-"""-._,-""-.
                    \\ ^,'_\     /_\     )
                     `./ /O\|   |/O\\   /
                      \ \_/|   |\_/ \_/
                       \ .'  _  `. /
                   .-.  ( .:(_):. )  ,-.
                  (   `._`._.-._,'_,'   )
                   )                   (
                  (   .-------------.   )
                   `-'               `-'

                                     __      _       _
  _ __ ___   ___  _ __ ___   ___    / _| ___| |_ ___| |__
 | '_ ` _ \ / _ \| '_ ` _ \ / _ \  | |_ / _ \ __/ __| '_ \
 | | | | | | (_) | | | | | | (_) | |  _|  __/ || (__| | | |
 |_| |_| |_|\___/|_| |_| |_|\___/  |_|  \___|\__\___|_| |_|

  Play with MOMO, Let MOMO Fetch Your Perfect Match

MOMO Fetch 0.1.0 — anthropic (claude-sonnet-4-20250514)
Session: a1b2c3d4
Loaded context from: ./SOUL.md, ./AGENTS.md, ./CLAUDE.md
Type /help for commands, Ctrl+D to quit.

claude-sonnet-4-20250514>
```

### Chatting with the Agent

Type your message and press Enter. The agent streams its response token by token.

```
claude-sonnet-4-20250514> what does the main function do?

  ⏺ file_read(path="src/main.rs")
  → 34 lines read

The main function is the entry point. It initializes tracing,
parses CLI arguments via clap, and delegates to cli::run()...
```

**What the colors mean:**
- Dimmed spinner `⠹ Thinking` — agent is waiting for LLM response
- Yellow `⏺` — agent is calling a tool
- Dimmed `→` — tool result summary
- White text — agent's response
- Red — errors

### Multi-Line Input

**Code blocks** — type ` ``` ` and the REPL keeps reading until the closing ` ``` `:

```
claude-sonnet-4-20250514> analyze this code:
... ```rust
... fn main() {
...     println!("hello");
... }
... ```
```

**Backslash continuation** — end a line with `\`:

```
claude-sonnet-4-20250514> please explain \
... the error handling strategy
```

### Shell Escape

Prefix any input with `!` to run a shell command directly (bypasses the agent):

```
claude-sonnet-4-20250514> !git status
On branch main
nothing to commit, working tree clean
```

### Cancellation and Exit

| Action | What happens |
|--------|-------------|
| **Ctrl+C** during generation | Cancels current response, partial text is saved |
| **Ctrl+C** during idle | Shows `^C`, does nothing |
| **Ctrl+D** during idle | Exits immediately |
| **Ctrl+D** during generation | Waits for current tool to finish, then exits |
| **Double Ctrl+C** | Force quit (during graceful shutdown) |
| **/quit** or **/exit** | Exits the REPL |

---

## Slash Commands

### Provider & Model Management

| Command | Description |
|---------|-------------|
| `/model` | Show current model |
| `/model <name>` | Switch model (e.g., `/model gpt-4o`) |
| `/provider` | Show current provider |
| `/provider <name>` | Switch provider (e.g., `/provider openai`) |
| `/models` | List all available providers and their default models |

Example — switching models mid-session:

```
claude-sonnet-4-20250514> /model gpt-4o
✓ Switched to openai/gpt-4o

gpt-4o> /provider deepseek
✓ Switched to deepseek/deepseek-chat
```

### Cost Tracking

| Command | Description |
|---------|-------------|
| `/cost` | Show session + today + project cost |
| `/cost today` | Show today's total cost |
| `/cost week` | Show this week's total cost |
| `/cost project <name>` | Show cost for a specific project |
| `/cost project` | List all projects with cost data |

Example:

```
gpt-4o> /cost
Session: $0.0234 (1,240 in + 890 out tokens, 5 requests)
Today:   $0.4512 (12 sessions)
Project (my-app): $1.2304 (3 sessions)
```

### Session Management

| Command | Description |
|---------|-------------|
| `/sessions` | List all past sessions (most recent first) |
| `/resume <id>` | Resume a previous session |
| `/clear` | Clear all context and start a fresh session |
| `/compact` | Compact context — summarize into a new session |
| `/permission` | Show current permission mode |
| `/permission <mode>` | Switch mode: strict, auto, or yolo (`/perm` shortcut) |

Example:

```
gpt-4o> /sessions
Sessions:
  ea7f9f08-...  (12 events, updated 2026-05-08 14:32) ← current
  42f31564-...  (45 events, updated 2026-05-07 09:15)
  1725d652-...  (8 events, updated 2026-05-06 16:44)

gpt-4o> /resume 42f31564
✓ Resumed session 42f31564-...
```

### Memory Vault

| Command | Description |
|---------|-------------|
| `/mem` | Show vault status (counts, path) |

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

The vault auto-populates higher-level tables at configurable thresholds:
- **Auto-extract** (every 10 MemCells): creates events, foresights, episodes
- **Auto-consolidate** (every 30 MemCells): clusters related MemCells, updates profiles

Configure thresholds in `.harness/settings.json`:
```json
{
  "memory": {
    "extract_threshold": 10,
    "consolidate_threshold": 30
  }
}
```

#### Memory Sidecar Options

Memory extraction has three modes, selected via config:

| Option | Config | Behavior | Cost |
|--------|--------|----------|------|
| A (default) | `sidecar_model: null` | TF-IDF keyword extraction | $0 |
| B | `sidecar_model: "deepseek-chat"` | Direct LLM call for extraction | ~$0.001/turn |
| C | `--mode memory-sidecar` | Separate process via Mailbox IPC | Varies |

Option B and C automatically fall back to Option A on any error.

**Option B config:**
```json
{
  "memory": {
    "sidecar_model": "deepseek-chat",
    "sidecar_provider": "deepseek"
  }
}
```

**Option C — run sidecar in a separate terminal:**
```bash
momo-fetch --mode memory-sidecar --project /path/to/project
```

The agent interacts with the vault via tools (`mem_write`, `mem_search`, etc.) — you don't need to run vault commands manually.

### Knowledge Base (KMS)

| Command | Description |
|---------|-------------|
| `/kms` or `/kms list` | List project knowledge bases |

Knowledge bases live in `.kms/<name>/pages/*.md` in your project. Create one:

```bash
mkdir -p .kms/architecture/pages
echo "# Architecture\n\n## Overview\n..." > .kms/architecture/index.md
```

### Skills

| Command | Description |
|---------|-------------|
| `/skill list` | List installed skills |
| `/skill install <git-url>` | Install a skill from a git repo |

```
gpt-4o> /skill list
Installed skills (2):
  code-review: Review code for issues [explicit]
    /path/to/.skills/code-review/SKILL.md
  commit: Create well-formatted commits [git, commit]
    /path/to/.skills/commit/SKILL.md

Project instructions (2):
  agents: /path/to/project/AGENTS.md
  soul: /path/to/project/SOUL.md
```

Skills are auto-matched when relevant, or invoked explicitly with `/<skill-name>`.

### MCP Servers

| Command | Description |
|---------|-------------|
| `/mcp list` | List configured MCP servers and their status |
| `/mcp add <name> <command>` | Add and start an MCP server |
| `/mcp remove <name>` | Remove an MCP server |

Example — adding a filesystem MCP server:

```
gpt-4o> /mcp add fs npx -y @anthropic/mcp-filesystem /tmp
✓ Added MCP server 'fs' and starting...
```

MCP tools are automatically available to the agent with the `mcp_` prefix.

**Testing MCP connections** — use `--test-mcp` to verify all servers connect successfully:

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

### Secrets (OS Keychain)

| Command | Description |
|---------|-------------|
| `/key set <provider>` | Store an API key in your OS keychain |
| `/key list` | List stored providers (keys are masked) |
| `/key delete <provider>` | Delete a key from the keychain |

```
gpt-4o> /key set anthropic
Enter API key for anthropic (default model: claude-sonnet-4-20250514):
  (The key will be stored in your OS keychain)
API key: ****
✓ Stored API key for 'anthropic' in OS keychain
```

### Agent Teams

| Command | Description |
|---------|-------------|
| `/team start <name>` | Start a team from `.harness/teams/<name>.json` (`.yml`, `.yaml` also accepted) |
| `/team start` | List the available configs, then define workers interactively |
| `/team status` | Read the mailbox and print what workers have reported |
| `/team merge` | Merge completed workers' branches (needs team status `Completed`) |
| `/team stop` | Kill the tmux session, remove worktrees, end the team — **destructive**, see below |

**Requires tmux.** Workers are launched by sending a command to a tmux window;
without tmux the team is created and no worker ever starts, despite the warning
saying otherwise. Requires git only for `worktree: true` — in a non-repo,
worktrees are disabled for the whole team with a warning.

**`/team stop` discards unmerged work.** It removes each worktree with
`--force` (uncommitted changes go with it) and deletes the worker's branch,
falling back to `git branch -D` when the branch is unmerged. Merge, or branch a
copy, before stopping.

**`/team status` is not a process check.** A worker's state changes only when a
message arrives in `<project>/.harness/mailbox` addressed to `lead`, and nothing
in a worker's default run posts one — so workers can sit at `starting` while
they are in fact working, and `/team merge` will refuse. Read the tmux windows
or `.harness/worker-<name>.log`, and merge the branches by hand if needed. Full
explanation in the [user guide §12](user-guide.md#12-agent-teams).

#### Headless: `momo-fetch team`

The same lifecycle without the REPL, for the agent in a session driving it
through `shell_exec` (or a script, or you in another terminal):

| Command | Description |
|---------|-------------|
| `momo-fetch team list` | Every config in `.harness/teams/`, plus which team is active |
| `momo-fetch team start <name>` | Start from a config — exits **2** if a team is already active |
| `momo-fetch team status` | Team, workers, pane ids, mailbox backlog |
| `momo-fetch team stop [--force]` | Stop the team; exits **0** when there is nothing to stop |

Every action takes `--project <dir>` (default: the current directory, resolved
to an absolute path). There is no `team merge` — merging has conflicts to
resolve, so it stays in the REPL.

**stdout is one JSON document and nothing else**; narration and errors go to
stderr. Exit codes: `0` success, `1` error, `2` state conflict
(`team_already_active`). `team start` returns the same document as
`team status`, so the pane ids come back with the start.

```bash
$ momo-fetch team status --project ~/workspace/momo-assistant | jq -r '.workers[].name'
analyst
executor
validator
```

Same destructive stop as `/team stop` — merge before stopping. Full reference in
the [user guide §12](user-guide.md#driving-a-team-from-the-shell).

### Agent Personalities

| Command | Description |
|---------|-------------|
| `/agent list` | List all agent personalities |
| `/agent show <name>` | Show agent personality details |
| `/agent switch <name>` | Switch to an agent personality mid-session |
| `/agent default` | Switch back to default mode (no personality) |

```
claude-sonnet-4-20250514> /agent list
Agent personalities (3):
  coder: Coding specialist [deepseek-chat]
  researcher: Research expert
  reviewer: Code review specialist

claude-sonnet-4-20250514> /agent switch researcher
✓ Switched to agent 'researcher'

claude-sonnet-4-20250514> /agent default
✓ Switched to default mode (no agent personality)
```

### Custom Slash Commands

Place `.md` files in `.harness/commands/` to create your own slash commands.

```bash
mkdir -p .harness/commands
```

Example — `.harness/commands/review.md`:

```markdown
Review the following code for bugs, security issues, and performance: $ARG
```

Then invoke in the REPL:

```
claude-sonnet-4-20250514> /review src/main.rs
```

`$ARG` is replaced with everything typed after the command name. If no `$ARG` placeholder exists, the content is sent as-is.

| Rule | Detail |
|------|--------|
| Names | Alphanumeric, dash, underscore only |
| Location | `.harness/commands/<name>.md` |
| Placeholder | `$ARG` — replaced with trailing text |
| Built-in bypass | Built-in commands (`/help`, `/model`, etc.) cannot be overridden |
| Discovery | `/help` lists custom commands under a "Custom commands" heading |

---

## Permission Modes

Control how the agent asks for approval before taking actions.

| Mode | Flag | Behavior |
|------|------|----------|
| **Strict** (default) | `--permission strict` | Every mutating action (file write, shell exec, etc.) asks for approval |
| **Auto** | `--permission auto` | Only destructive commands (rm -rf, force push) ask for approval |
| **Yolo** | `--permission yolo` | No approval prompts — the agent does everything automatically |

You can switch permission mode mid-session using the `/permission` command:

```
GLM-5> /permission            # Show current mode
Current: strict

GLM-5> /permission auto       # Switch to auto
✓ Switched to auto

GLM-5 [auto]> /perm yolo      # /perm is a shortcut
✓ Switched to yolo

GLM-5 [yolo]> /perm strict
✓ Switched to strict

GLM-5>
```

The prompt shows `[auto]` or `[yolo]` when not in strict mode (strict is the default, so no tag is shown).

When the agent needs approval:

```
  ⏺ file_write(path="src/main.rs")
  ! Tool file_write requires approval: path="src/main.rs", content="..."

  ? Allow file_write to proceed? [y/n]
> y

  ⠹ Thinking
  → File updated successfully
```

Type `y` to proceed, `n` to block the action. Once approved, the tool won't ask again for the same tool name during the session.

---

## Environment Variables

| Variable | Purpose |
|----------|---------|
| `ANTHROPIC_API_KEY` | Anthropic API key |
| `OPENAI_API_KEY` | OpenAI API key |
| `DEEPSEEK_API_KEY` | DeepSeek API key |
| `GROQ_API_KEY` | Groq API key |
| `OPENROUTER_API_KEY` | OpenRouter API key |
| `ZAI_API_KEY` | z.ai API key |
| `ZAI_LLM_URL` | z.ai base URL (default: `https://api.z.ai/api/coding/paas/v4`) |
| `LLM_URL` | Custom OpenAI-compatible endpoint URL |
| `LLM_APIKEY` | Custom endpoint API key |
| `LLM_MODEL` | Custom endpoint model name (default: `default`) |
| `HARNESS_LOG` | Log level (`trace`, `debug`, `info`, `warn`, `error`). Default: `warn` |
| `NO_COLOR` | Disable colored output (any value) |

You can also set these in a `.env` file in your project directory.

---

## Project Configuration

### `.harness/settings.json`

Project-level settings (overrides global defaults):

```json
{
  "default_provider": "anthropic",
  "default_model": "claude-sonnet-4-20250514",
  "permission_mode": "auto"
}
```

### `.harness/mcp.json`

MCP server definitions. Supports both **stdio** (local child process) and **HTTP/SSE** (remote server) transports:

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

**Stdio servers** use `command` + `args` to spawn a local process. **HTTP/SSE servers** use `type: "http"` (or `"sse"`) with a `url` and optional `headers` for authentication.

### `.agentignore`

Exclude files from agent access (same format as `.gitignore`):

```
*.env
secrets/
*.key
node_modules/
target/
```

### `AGENTS.md` / `CLAUDE.md` / `SOUL.md`

Project instructions and agent personality files auto-discovered and injected into the system prompt. Place in your project root:

```markdown
# SOUL.md — Agent Personality
- Speak in Thai by default
- Be concise and direct
- Prefer code examples over explanations

# AGENTS.md — Project Instructions
- Use Rust 2024 edition
- All tests must pass before committing
- Prefer dedicated tools over shell commands
```

**SOUL.md** defines the agent's personality (communication style, tone, language). **AGENTS.md** defines project instructions (coding standards, architecture). SOUL.md takes behavioral priority in the system prompt.

---

## File Locations

| File | Location |
|------|----------|
| Sessions DB | `~/Library/Application Support/momo-fetch/sessions.db` (macOS) or `~/.config/momo-fetch/sessions.db` (Linux) |
| Cost data | `~/Library/Application Support/momo-fetch/cost.json` |
| REPL history | `~/.config/momo-fetch/history.txt` |
| Memory vault | `<project>/memory-vault/` |
| Sidecar mailbox | `<project>/.harness/mailbox/` |
| MCP config | `<project>/.harness/mcp.json` |
| Skills | `<project>/.harness/skills/`, `<project>/.skills/`, `<project>/.claude/skills/` |
| Agent personalities | `<project>/.harness/agents/` |
| Custom commands | `<project>/.harness/commands/` |
| Team configs | `<project>/.harness/teams/` |
| Knowledge bases | `<project>/.kms/` |
