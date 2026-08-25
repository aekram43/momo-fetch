# Quick Start — Web UI

Get the web UI running in under 5 minutes.

## Prerequisites

- Node.js 20+ (for `npm`)
- A running `momo-fetch` gateway (or REPL)

## Install & Run

```bash
cd web
npm install
npm run dev
```

Open `http://localhost:3000` in your browser.

You should see the shell boot, display the three panels (sessions, chat, detail), and the status bar.

## Two Builds, One Choice

The same source compiles to two different outputs:

| Build | Command | Use for | Result |
|-------|---------|---------|--------|
| **Gateway** | `npm run build` | Gateway at `/ui` | `basePath: "/ui"` |
| **Tauri desktop** | `npm run build:desktop` | Tauri at `/` | `basePath: ""` |

Pick the wrong one and every asset 404s. The difference is set at build time in `next.config.ts` via the `MOMO_BASE_PATH` environment variable.

## Against a Separate Gateway

For development against a gateway on a different host:

```bash
NEXT_PUBLIC_GATEWAY_URL=http://your-gateway:3001 npm run dev
```

The dev server resolves the gateway URL in this order:

1. `window.__GATEWAY_URL__` — Tauri, injected at runtime
2. Same origin, when served by the gateway at `/ui`
3. `NEXT_PUBLIC_GATEWAY_URL` environment variable
4. `http://localhost:3000`

## What You Get

- **Left panel** — Sessions list, agents, tools, routines, memory, files
- **Center** — Chat composer and message history
- **Right panel** — What the agent is working on: this turn, the files it
  changed, and anything running elsewhere (team workers, scheduled runs)
- **Header** — Current model, cost, gateway health
- **Status bar** — Standing permissions, token usage

All three regions scroll independently; the page never scrolls.

## Verify It Works

Send a message in the composer. You should see:

1. A spinner in the turn rail
2. The agent's text appending in real time
3. Tool calls showing as cards
4. Tokens accumulating in the header

If you see errors, check:

- Is the gateway running? (polled at `/health`)
- Can you reach it? (gateway URL must be correct)
- Do you have an API key set? (check Settings > API keys)

## Next Steps

- Read the [user guide](user-guide.md) for every control
- Review the [code guide](code-guide.md) before making changes
- Check [gateway API guide](gateway-api-guide.md) to understand the wire format
