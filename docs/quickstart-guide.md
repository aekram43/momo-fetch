# Quick Start Guide

Get MOMO Fetch running in under 5 minutes.

---

## Prerequisites

- **Rust** 1.85.0+ (install via [rustup](https://rustup.rs))
- **An LLM provider** — at least one of:
  - Ollama running locally (no API key needed)
  - An Anthropic API key
  - An OpenAI API key
  - A DeepSeek, Groq, OpenRouter, z.ai, or any OpenAI-compatible API key

## 1. Clone and Build

```bash
git clone <repo-url> momo-fetch
cd momo-fetch
cargo build
```

Build takes 1-2 minutes on first run. Subsequent builds are instant.

## 2. Configure Your Provider

### Option A: Ollama (easiest, no key needed)

Install and start [Ollama](https://ollama.com), then pull a model:

```bash
ollama pull llama3.2
```

MOMO Fetch auto-detects Ollama and uses it as the default if no other provider is configured.

### Option B: Cloud Provider (set API key)

Set an environment variable for your preferred provider:

```bash
# Pick ONE:
export ANTHROPIC_API_KEY="sk-ant-..."
export OPENAI_API_KEY="sk-..."
export DEEPSEEK_API_KEY="sk-..."
export GROQ_API_KEY="gsk_..."
export OPENROUTER_API_KEY="sk-or-..."
export ZAI_API_KEY="your-z-ai-key"
# Or use any OpenAI-compatible endpoint:
export LLM_URL="https://your-llm-endpoint.com/v1"
export LLM_APIKEY="your-api-key"
export LLM_MODEL="your-model"  # optional, defaults to "default"
```

Or save it to a `.env` file in your project directory:

```bash
echo 'ANTHROPIC_API_KEY=sk-ant-...' > .env
```

Or store it in your OS keychain (no env vars needed):

```bash
./target/debug/momo-fetch
# then inside the REPL:
# /key set anthropic
# (prompts for the key, stores in macOS Keychain / Linux Secret Service)
```

## 3. Run

### Interactive Mode (REPL)

```bash
# From your project directory:
cd your-project
./target/debug/momo-fetch

# Or specify a project path:
./target/debug/momo-fetch --project /path/to/your-project
```

You'll see the MoMo banner and a prompt:

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
Type /help for commands, Ctrl+D to quit.

llama3.2>
```

Start chatting with the agent. It has file, shell, search, web, and memory tools.

### Customizing Agent Personality

Create a `SOUL.md` file in your project root to define the agent's personality:

```markdown
# SOUL.md
Speak in Thai by default.
Be concise and direct.
Use code examples over explanations.
```

Create an `AGENTS.md` file for project-specific instructions:

```markdown
# AGENTS.md
Use Rust 2024 edition.
All tests must pass before committing.
```

### Using Agent Personalities

Create specialist agents in `.harness/agents/`:

```bash
mkdir -p .harness/agents
```

```markdown
<!-- .harness/agents/researcher.md -->
You are a research specialist. Always cite your sources.
Deeply investigate before answering.
```

```markdown
<!-- .harness/agents/coder.md -->
You are an expert coder. Write clean, efficient code.
Always add tests for new functionality.
```

Then start with a specific personality:

```bash
./target/debug/momo-fetch --project . --agent researcher
```

Or switch mid-session in the REPL:

```
claude-sonnet-4-20250514> /agent list
Agent personalities (2):
  coder: (no description)
  researcher: (no description)

claude-sonnet-4-20250514> /agent switch coder
✓ Switched to agent 'coder'

claude-sonnet-4-20250514> /agent default
✓ Switched to default mode (no agent personality)
```

### One-Shot Mode

```bash
# Single prompt, prints response, exits:
./target/debug/momo-fetch -p "Explain the main.rs file"

# Pipe content in:
cat README.md | ./target/debug/momo-fetch -p "Summarize this"

# Specify provider and model:
./target/debug/momo-fetch -p "Write a hello world" --provider openai --model gpt-4o
```

## 4. Useful First Commands

Inside the REPL:

| Command | What it does |
|---------|-------------|
| `/help` | Show all available commands |
| `/models` | List available providers and models |
| `/model gpt-4o` | Switch model mid-session |
| `/permission auto` | Switch permission mode (strict/auto/yolo) |
| `/cost` | Show current session cost |
| `/mem` | Show memory vault status |
| `/agent list` | List agent personalities |
| `/agent switch <name>` | Switch to a specialist personality |
| `/agent default` | Switch back to default mode |
| `/team start <name>` | Start a team from config file |
| `/team status` | Check team progress |
| `/quit` | Exit (or Ctrl+D) |

### Team Config Files

Define a team of workers in `.harness/teams/<name>.yml` and start them with one command:

```yaml
# .harness/teams/auth-squad.yml
name: "Auth Refactor Squad"
workers:
  - name: researcher
    task: "Research auth best practices"
    agent: researcher
    worktree: true
  - name: coder
    task: "Implement JWT + refresh tokens"
    agent: coder
    branch: feature/auth
    worktree: true
```

```bash
mkdir -p .harness/teams
# Then in the REPL:
# /team start auth-squad
```

## 5. Next Steps

- [CLI Guide](cli-guide.md) — detailed command reference
- [User Guide](user-guide.md) — all features explained
- [Code Guideline](code-guideline.md) — how the codebase works

## Troubleshooting

**"No default store has been set"**
Set an API key via environment variable or use Ollama.

**"Ollama stream error"**
Make sure Ollama is running: `ollama serve`

**Build fails on adk-rust**
Ensure Rust 1.85.0+: `rustc --version`

**Install globally:**
```bash
cargo install --path .

echo 'export PATH="$HOME/.cargo/bin:$PATH"' >> ~/.zshrc
source ~/.zshrc
# Now use anywhere:
momo-fetch --project ~/my-project
```
