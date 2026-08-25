# MOMO WORK User Guide

MOMO WORK is the desktop app for the momo-fetch agent. This guide covers day-to-day use.

For installation and first run, see [Building MOMO WORK from source](./momo-desktop-install.md). The install guide also includes a "Finding your way around" section that explains the three regions of the app (left panel, right panel, Settings) and a keyboard table — refer to those rather than duplicating them here.

## API keys and settings

The desktop app manages API keys differently from the browser version. Keys are stored in the OS keychain — not as plain text in `.env` — and you configure them in Settings (⚙ in the header, or `Cmd/Ctrl+,`).

### Which key wins when you have multiple

If you have set a key in the app **and** defined the same variable in the workspace `.env`, the app-configured key takes precedence. This is deliberate: in a GUI, the thing you type into the app is what it uses.

The Settings panel tells you which keys overlap; it notes when a `.env` value exists but is being shadowed. For full details on precedence rules and which file is read when, see "Which key wins" in the [installation guide](./momo-desktop-install.md#which-key-wins).

The app will never return a key you have stored. The status panel shows you whether a key is configured, never what it is. This prevents keys from being exfiltrated by anything that compromises the app.

### Supported providers

Open the Models panel in Settings to see all available providers. You can configure keys for:

- Anthropic (claude models)
- OpenAI (GPT models)
- DeepSeek
- Groq
- OpenRouter (a router in front of many providers)
- ZAI (z.ai)
- Google Gemini

If you have no key for a provider, it stays in the provider list but is greyed out with the reason — Settings → Models shows it as `anthropic — no key` rather than hiding it.

Ollama is in that provider list too, but not in API keys: it takes no key at all. When it is unavailable the reason is different — `ollama — offline` means nothing is listening on its port, so the fix is to start Ollama, not to find a key.

## Projects and sandboxing

The agent's file tools can only read and write inside the **project directory** — the agent's sandbox root. On first launch, this is a dedicated workspace folder that is deliberately not your home directory.

To point the agent at a real project:

1. Open **Customization → Files → open another project…** in the left panel
2. The app asks for confirmation — this re-roots the sandbox, memory vault and session database at once
3. Click **choose folder**; a native folder picker opens
4. Pick the directory. The gateway restarts under the new root and the window reloads

The Files tree never shows anything above the project root. That is the sandbox
boundary, and it applies to the agent's own file tools too — re-rooting is the
only way to move it.

The app will then restart the gateway with the new project root. You can switch projects as often as you need; each switch resets the session history and moves the memory vault.

## Sessions

A session is a conversation thread with the agent. The left panel lists all sessions for the current project.

To start a new session, use:

- The **New Session** button at the top of the left panel
- Menu: File → **New Session** (`Cmd/Ctrl+N`)
- Tray icon: **New Session**

Sessions live in one SQLite database per machine, not per project:
`~/.config/momo-fetch/sessions.db` (`src/harness.rs:49`). Switching projects does
not change which sessions are listed — the list is global.

## Scheduled work

**Customization → Routines** in the left panel. A routine hands a task to the
agent — or to a standby team worker — on a cron schedule or a fixed interval,
without anyone at the keyboard.

The app's gateway advances the schedule while the app is open, every 30 seconds.
**Close the app and nothing fires**: routines are not a background service, and
the desktop shell does not keep one alive after you quit. For schedules that
have to hold on a machine you are not sitting at, put `momo-fetch routine tick`
in system cron — it is the same step.

Routines belong to the **project**, in `<project>/.harness/routines/`. Switching
projects switches which routines exist, the same way it switches the memory
vault.

Each firing runs in its **own process**, beside your session rather than inside
it — so a routine that fires while you are mid-conversation does not interleave
with it, and its output goes to the run's log rather than into your chat. The
right panel's **Running elsewhere** section shows anything in flight; the
Routines dialog shows each run's exit code and log path.

A scheduled run defaults to `auto` permission, because nothing can answer a
confirmation prompt at 03:00 — see [Approvals and
permissions](#approvals-and-permissions) for what that allows.

## Approvals and permissions

The app starts in strict permission mode: the agent asks before taking any action that changes your machine (running shell commands, reading files outside the sandbox, etc).

When the agent asks for approval, the dialog tells you:

- What tool it wants to use (e.g. `shell_exec`, `file_read`)
- Whether you're approving it once or for the rest of the session

**Approving a tool grants it by *name*, for as long as the app is running — not
just for the current session.** Approve `shell_exec` once and every later shell
command runs silently, including in sessions you open afterwards. The set is held
in memory (`src/harness.rs`, `approved_tools`) and never written to disk, so
quitting clears it.

That is why the status bar carries `● N auto-run` in the bottom right whenever
anything is granted. Click it to see exactly which tools, and to revoke them all.

To stop being asked at all, switch to **yolo** in Settings → **Permissions**.
It asks you to confirm first, because it removes the confirmation boundary
entirely. While it is on, the status bar says `● yolo · nothing asks` in red at
every window size.

## Deep links

You can open a project from a `momo://` link — for example, from a chat message or web page. The link must specify an absolute path to an existing directory:

```
momo://open?path=/path/to/project
```

The app validates the path (it must exist and be a directory) but **does not automatically open it**. Instead, the app shows the same confirmation dialog as "open another project…" in the Files panel, so you are never blindsided. If the link contains an invalid path or traversal (`..`), the app rejects it and explains why.

## Display and appearance

Settings → **Appearance** controls:

- **Theme**: Dark, light, or auto (follows the OS and updates as you change the system setting)
- **Sound**: Toggle notifications on or off (default: off)

The app remembers your window size, position, and whether it was maximized. These settings are restored on each launch.

## Hiding and restoring the window

Left-click the tray icon to raise the window — it unminimises, shows and focuses
it (`desktop/src-tauri/src/menu.rs`). It does not toggle: clicking again leaves
the window up. Right-click for a menu with:

- Show the window
- Create a new session
- Quit the app

## Updating

See [Updating](./momo-desktop-install.md#5-updating) in the installation guide for the update process and how to work around a stuck UI.

## Troubleshooting

For startup failures and common issues, see [Troubleshooting](./momo-desktop-install.md#6-troubleshooting) in the installation guide. It includes log paths for all platforms.
