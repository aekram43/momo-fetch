# User Guide — Web UI

Everything you need to know to use the agent from the browser.

## Overview

The interface is built around **three always-visible panels**:

| Panel | Shows |
|-------|-------|
| **Left** (sidebar) | Sessions · **Squad**: agent, team, routines · **Customization**: tools, memory, files |
| **Center** (chat) | The conversation and output in real time |
| **Right** (detail) | What the agent is doing: this turn (tool calls, written files) and what is running elsewhere |

Below `lg` (desktop width), panels collapse to drawers overlaying the chat. Below `xl`, the detail panel is hidden by default.

The header shows the current model, connection status, and cost. The status bar shows session info, context usage, cost, standing permissions, and turn phase.

## Header

Buttons and controls from left to right:

- **▤** (`aria-label="Toggle sessions panel"`) — Show/hide the sidebar
- **MOMO WORK** — Brand wordmark
- **Model name** (monospace) — Current model, polled from `/health` every 10s
- **"turn active elsewhere"** (if applicable) — Another tab or the REPL has a turn running (amber text)
- **Cost** (with hover tooltip showing tokens and request count) — Session total from `/v1/cost` poll
- **Connection dot** — Shows `connecting`, `connected`, or `offline` with a status indicator
- **⚙** (`aria-label="Settings"`) — Open the settings dialog (Cmd/Ctrl+,)
- **▥** (`aria-label="Toggle detail panel"`) — Show/hide the detail panel

## Sidebar

The left panel scrolls vertically and contains:

### Sessions

**List of your sessions.** Click one to load its history. The active session is highlighted with a ▸ marker.

Each row shows:
- First 8 characters of the session ID (full ID in title)
- Event count (number of messages), or nothing if unknown
- A mark for sessions that did not begin as a chat: **◆** started as a named
  agent, **▣** a team worker, **◷** a routine run. Every momo-fetch process
  writes into one sessions table, so this list holds workers and scheduled runs
  beside your conversations — hover for which one it was. No mark means someone
  typed it.

Controls:
- **"+ new"** button — Create a new session
- **× button** (appears on hover) — Delete a session

One session is active across all tabs; switching in one tab affects all of them.

### Squad (collapsible group)

**Who does the work.** Three registers, each an actor with a state: the agent
this session talks to, the team of workers running beside it, and the routines
that hand out work when nobody is here.

Separate from Customization below, which holds what they have to work *with*.
Agents used to sit with tools and files, which put "who is answering me" next to
"which MCP servers are up".

#### Agents

Choose which agent the model adopts. Click to switch. The gateway holds the current agent and broadcasts it to all tabs.

#### Team

**The squad's standing composition.** When a team is running: its config name,
a worker count in the header, and a row per worker with a state dot, the name,
`standby` if it can be given more work, and either its state or how many
messages are queued for it. `N waiting for you` appears when the lead has unread
mail.

When no team is running, it lists the configs in `.harness/teams/`, each with a
**start** button on hover. Each worker gets its own tmux pane, and a worktree
when the config asks for one.

While one is running the section offers **stop team**, which asks first — that
click removes every worktree and deletes each worker branch, so merge anything
you want to keep before confirming. A worker whose pane died shows **restart**
next to its row; a healthy worker does not, because restarting one would kill
work in progress.

If a note appears under the section — *"tmux not found — the team will be
recorded but no worker process starts"* — read it. It is the difference between
a team and a row in a file.

Not the same question as the right panel's **Running elsewhere**: that is what
is in flight this minute across teams *and* routines. This is who is on the
squad.

#### Routines

**Recurring work, and when it next fires.** Each row is a routine: a dot for its
state (green armed, grey off, amber a run in flight, red the last run failed),
its name, and when it is next due (`in 4h`, `manual`, `off`).

Click a row to open the **Routines dialog** on that routine; **manage** opens the
dialog with nothing selected.

Rows are a summary and a way in, not a control surface — everything you can *do*
to a routine lives in the dialog. Run and delete buttons in a 288px rail would be
hit by accident.

### Customization (collapsible group)

**What the squad has to work with.** Three registers, all inert — none of them
does anything on its own.

#### Tools

**MCP server status.** Shows which servers are running and how many tools they expose.

Each row displays:
- Status dot (green = running, amber = error, grey = offline)
- Server ID (monospace)
- Tool count (or `—` if unknown; `null` never renders as `0`)

Header shows `X/Y` (running servers / total servers).

#### Memory

Search the vault. Shows stats: total memories, events, foresights, pending foresights, and auto-write status.

#### Files

Browse the project tree from the sandbox root. Click a file to preview it in the detail panel (read-only).

Both groups remember whether they were open (`localStorage`, view state only).
A collapsed group **unmounts** its panels rather than hiding them, so it stops
polling the gateway for a view nobody is looking at.

## Chat Panel (center)

The conversation, scrolling vertically. Messages are stacked top-to-bottom.

- **User messages** — Aligned left
- **Assistant messages** — Aligned left
- **Tool call cards** — Below assistant text, showing:
  - Tool name and current status (running/done/error)
  - Arguments
  - Output preview (truncated if necessary)

Below the history is the **message composer**:

- Large text field
- **Send** button (or press Enter to send)
- **Stop** button (appears during a turn) — Interrupts the current turn

## Detail Panel (right panel; `xl` and up)

What the agent is doing — in this turn, and outside it. Three fixed sections:

### This turn

**Current tool calls from the trailing assistant message.** Shows as cards (same as in chat). If empty, a hint says "Tool calls appear here as the agent makes them."

Header optionally shows token count in monospace: `{prompt}↑ {completion}↓` (only shown if either is > 0).

### Artifacts

**Every file the turn changed.** Two sources, shown as one list of paths in monospace, deduplicated:

- **The filesystem diff.** The gateway compares the project tree before the turn and after it, so this covers files written any way at all — the file tools, a shell heredoc, `sed -i`, `>`, a formatter, a codegen step. Each row carries a tag: `new`, `mod` or `del`. It arrives once, when the turn ends. Build output and `.git` are excluded (as is anything `.gitignore` or `.agentignore` covers); dotfiles like `.env` are not.
- **Write-shaped tool calls**, as the agent makes them — `file_write`, `file_edit`, and also `mem_write`, which writes into the memory vault rather than the project. These appear immediately, without waiting for the turn to end, and cover writes that land outside the project folder. They have no tag.

Turn-scoped and not stored: reopening a past session shows none.

If empty, a hint says "Files this turn changed show up here."

### Running elsewhere

**Work the agent started that is not this turn**: team workers in their tmux
panes, and routine runs in their own processes.

Worker rows show the worker name, its agent (if it has one), and its state —
`running`, `completed`, `crashed`, `failed_to_start`. A failure prints its
reason underneath, clamped to two lines with the full text (including the log
path) in the tooltip. A worker with queued mail or a heartbeat shows
`3 queued · beat 40s ago`.

Run rows show the routine name and how long it has been going; clicking one
opens the Routines dialog on that routine. The section header shows how many
routines are armed, and is also a way into the dialog.

**Polled every 10 seconds**, because none of it is caused by this tab — the
gateway fires routines on its own timer and tmux never reports to anyone. The
poll refreshes worker liveness and settles finished runs as it goes, so a worker
whose pane died reads `crashed` here before anything else notices.

If nothing is in flight, a hint says so — which is a real answer, not an empty
state.

## Status Bar (bottom)

Fixed row showing:

- **Session ID** — `session {first-8-chars}` (or `session —` if no session)
- **Context usage** — `context {percent}%` (or `context —` if unknown)
- **Session cost** — `${cost}` (formatted to 4 decimal places)
- **Standing permissions badge** — Appears only if there are approved tools or permission mode is `yolo`
- **Turn phase** — `idle`, `running`, or `awaiting approval` (amber when not idle)

## Dialogs

### Approval Dialog (F9)

**When:** A tool requires confirmation before running.

Shows:
- Title: `Run {tool_name}?`
- Arguments as monospace JSON
- If destructive: `Destructive — {category}.` (in red banner)
- Sticky disclosure: "Approving lets the agent run {tool_name} for the rest of this session without asking again. Denying applies only to this request — the agent can ask again."
- **Deny** button (default focus)
- **Approve for this session** button (green, consent color)

**Keyboard:**
- Escape — Deny
- Tab — Cycle focus within dialog
- Shift+Tab — Reverse cycle

This is a focus-trapped modal at all breakpoints. It outranks the settings dialog.

### Settings Dialog

Opens as a modal (triggered by the ⚙ in the header, Cmd/Ctrl+,, or the native menu).

Escape closes it (unless an approval is pending). Contains four tabs:

#### Models tab

**Provider** — Native `<select>` dropdown. Available providers are clickable; unavailable ones are disabled with a reason (e.g., "no key").

**Model** — Filterable list that shows:
- Current model + four most recently used (before typing)
- Once you type: full filtered catalogue (up to 200 shown)
- If the catalogue cannot be fetched: an error message and a free-text input field

Controls:
- **refresh** button — Refresh the catalogue (shows "cached" if currently cached)
- **close** button — Close the picker

#### API Keys tab

**Desktop only.** In a browser, shows a hint: "Keychain storage needs the desktop app — a browser has no way to reach the OS keychain. Set keys in the workspace `.env` instead, then restart the gateway."

In the desktop app:
- List of providers with status indicator (green dot = set, grey dot = not set)
- Each row shows: provider name, "set" or "not set", and "add" or "replace" button
- If a key is configured, an × button appears to remove it
- If a key is also in `.env`, a warning shows which one is being used (the keychain version overrides `.env`)

#### Permissions tab

**Three mode buttons:**

| Mode | Description |
|------|---|
| strict | Asks before anything that changes your machine |
| auto | Asks only for destructive commands |
| yolo | Runs everything without asking |

Below the mode buttons: the description of the current mode.

If you click `yolo`, a confirmation prompt appears (red border): "In `yolo` the agent runs shell commands and edits files with no confirmation. Continue?" with `cancel` and `enable yolo` buttons.

**Approved tools section:**

- Header: "Approved tools" with a `revoke all` button (if any tools are approved)
- List of tools shown as chips (signal/orange color)
- If empty: "None granted. Every tool will ask."
- Below list: "These run without asking for the rest of this session."

#### Appearance tab

**Theme** — Three buttons: `dark`, `light`, `auto` (follows OS preference).

**Sound** — Toggle (off by default; an app that makes noise unasked is muted at the OS level).

### Routines Dialog

Opens from the left rail's **Routines** section (**manage**, or any row), and
from the right panel's **Running elsewhere** header. Escape closes it, unless an
approval is pending — dismissing a dialog is never a request to stop the agent.

Wider than settings, because the two halves have to be visible together: a
schedule list you cannot see the outcomes beside is how a routine fails quietly
for a week.

**Left rail** — every routine with its state dot and next window, `+ New
routine`, and **check** (advance the schedule now instead of waiting for the
gateway's next tick).

**Right pane**, one of three things:

- **Detail** — the trigger, the assignee, counters (`fired`, `skipped`, `in
  flight`, `queued`), the task template, and **recent runs** with their exit
  codes and log paths. Buttons: **Run now**, **Edit**, **Enable/Disable**, and
  **Delete** off on its own at the right.
- **New routine / Edit** — the form below.
- Nothing selected — a short explanation of what a routine is.

#### The routine form

| Field | Notes |
|---|---|
| **Name** | What the routine is called |
| **Assignee** | `lead`, an agent from `.harness/agents/`, or a standby team worker. Workers only appear while a team is running; one that has gone away stays listed as "not currently available" rather than silently reassigning the routine |
| **Trigger** | Cron schedule · Every N minutes (heartbeat) · Manual only |
| **Concurrency** | Queue · Skip · Parallel — the hint under it says what each does |
| **Cron helper** | Preset dropdown, timezone, the expression itself, catch-up |
| **Task template** | Title, priority, and the description — which *is* the prompt |
| **Permission** | `auto` recommended; the hint says why `strict` and `yolo` are both wrong for an unattended run |

The presets **write into the expression field**; the field is always the truth.
A schedule builder that hides the expression cannot say what crontab can, and
the people most likely to want a routine already know the syntax.

The timezone defaults to **this machine's IANA zone**, not UTC. Somebody typing
"09:00" means nine in the morning where they are; storing that as UTC is the bug
the field exists to prevent.

Validation happens in the form before the request goes out (field count on the
cron expression, minimum interval, required fields), and the gateway's own
message is shown if it still refuses.

## Keyboard Shortcuts

| Shortcut | Action |
|----------|--------|
| Escape | Interrupt the current turn (not active during the approval, settings or routines dialogs) |
| Cmd+B (Mac) / Ctrl+B | Toggle sidebar |
| Cmd+J (Mac) / Ctrl+J | Toggle detail panel |
| Cmd+, (Mac) / Ctrl+, | Open Settings |

## How Data Flows

- **Composition** — You type, hit Enter, message posts to `/v2/chat/stream` and streams back via SSE
- **Real-time updates** — Tool calls, approvals, usage arrive as SSE events
- **Server state always wins** — Active session, model, permissions come from `/health` and event streams, not cached locally
- **Polling** — Header polls `/health` and `/v1/cost` every 10 seconds; the right panel polls `/v2/activity` on the same cadence for work outside this turn; changes in one tab appear in all others

One global session is held by the gateway (spec §2.3). Switching sessions in one tab affects all tabs.

## If Something Goes Wrong

### "Offline" in the header

The gateway is unreachable. Check:
- Is it running? (`momo-fetch --gateway`)
- Is the URL correct? (env var `NEXT_PUBLIC_GATEWAY_URL` or `window.__GATEWAY_URL__` in Tauri)
- Are you behind a firewall?

### Approval dialog is stuck or times out

The turn waited 5 minutes for approval. Click Deny or wait for timeout. Refresh and retry.

### Asset 404s, page is unstyled

You built with the wrong `basePath`:
- Built for desktop but serving via gateway → rebuild with `npm run build`
- Built for gateway but running in Tauri → rebuild with `npm run build:desktop`

The build is a static export with absolute asset URLs; wrong `basePath` means every CSS and JS 404s.

### Theme not applying

The theme is set by a pre-paint script in `<head>` (to prevent flash). Check:
- Browser console for errors
- `localStorage` for key `momo-worker.prefs.v1` (should have a `theme` field)
- `data-theme` attribute on `<html>` (should be "dark" or "light")

## Architecture Notes

**Static export** — No Node.js server. The UI is a Next.js static export (`output: 'export'` in `next.config.ts`). All dynamic behavior comes from the gateway over HTTP.

**Streaming** — Uses `fetch` + `ReadableStream`, not `EventSource`, because the request is a POST with JSON body and authorization header.

**Two stores** — `useChatStore` holds the conversation; `useUiStore` holds panel visibility and preferences. Both use Zustand.

**Preferences stored locally, server state fetched.** Only view state (panel toggles, theme) lives in `localStorage`. Session, model, and permissions are gateway state and never cached locally.

**No JavaScript server.** The page is completely static; every dynamic call goes to the gateway. This keeps CORS load-bearing (spec §9.2, §9.3) and the token in memory-only, never in localStorage.
