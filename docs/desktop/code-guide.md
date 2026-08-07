# Code Guide: Modifying the Desktop Shell

This guide is for developers working on the MOMO WORK desktop shell — the Tauri application and its integration with the gateway. It assumes you know Rust and Tauri basics.

The shell is small and single-purpose: it owns the keychain, supervises the gateway child process, and exposes commands to the web UI. There is no agent logic here, no tool implementation. See [project-landscape.md](../project-landscape.md) for the architecture overview.

## Key files and their roles

| File | Lines | Purpose |
|---|---:|---|
| `desktop/src-tauri/src/lib.rs` | ~482 | Main entry point; IPC command handlers; startup and shutdown logic |
| `desktop/src-tauri/src/gateway.rs` | ~317 | Spawn the `momo-fetch --gateway` child; supervise it; drain its output |
| `desktop/src-tauri/src/secrets.rs` | ~218 | Keychain read/write; environment variable injection; key precedence |
| `desktop/src-tauri/src/menu.rs` | ~140 | Native menus and tray icon; emit `momo:menu` events |
| `desktop/src-tauri/src/deeplink.rs` | ~218 | Parse and validate `momo://` URLs; emit proposal events |
| `desktop/src-tauri/tauri.conf.json` | ~87 | Bundling, resources, beforeBuildCommand, deep link scheme |
| `web/src/lib/desktop.ts` | ~170 | IPC contract definition; event listeners; TypeScript types |

Each Rust file starts with a long doc comment explaining *why* it is built that way. Read those first.

## IPC command contract

The web UI calls the shell through these commands (defined in `web/src/lib/desktop.ts`):

| Command | Args | Returns | Purpose |
|---|---|---|---|
| `gateway_url` | none | `string \| null` | Get the gateway's listening URL (fallback; URL is also injected as `window.__GATEWAY_URL__`) |
| `startup_error` | none | `string \| null` | Error message if gateway failed to start |
| `gateway_stderr` | none | `string` | Last 50 lines of gateway output (for crash screen) |
| `secret_status` | none | `SecretStatus[]` | Per-provider key status (configured? also in .env?) |
| `set_secret` | `{ provider, key }` | `void` | Store a key in the keychain and restart the gateway |
| `delete_secret` | `{ provider }` | `void` | Remove a key from the keychain and restart the gateway |
| `open_project` | `{ path }` | `string` (new URL) | Re-root the gateway at a new project directory |

None of these commands return the actual key value — that is deliberate. The UI only needs to know whether a key is set, never what it is.

To add a new command: 
1. Write the handler in `lib.rs` as a function with `#[tauri::command]`
2. Add it to the `invoke_handler!` macro
3. Export the call signature from `web/src/lib/desktop.ts`

## Menu and tray events

Menus and the tray icon (defined in `desktop/src-tauri/src/menu.rs`) emit a single event type: `momo:menu` with the menu item ID as detail.

The available menu item IDs are:

| ID | Source | Action |
|---|---|---|
| `new-session` | File menu, tray menu | Create a new session |
| `open-project` | File menu | Show the files panel to choose a project |
| `toggle-sidebar` | View menu | Toggle the left panel |
| `toggle-detail` | View menu | Toggle the right panel |
| `interrupt` | View menu | Stop the running turn |
| `settings` | File menu | Open Settings |
| `tray-show` | Tray menu | Show the window (no routing to the web UI) |
| `tray-quit` | Tray menu | Exit the app (goes through the normal shutdown path) |

The web UI listens for `momo:menu` events in `web/src/hooks/use-desktop-menu.ts` and routes them to the same handlers as keyboard shortcuts, so an action never has multiple implementations.

To add a new menu item:
1. Create it in `menu.rs` with `MenuItem::with_id(app, "id", "Label", true, Some("Shortcut"))?`
2. Add it to the File, Edit, or View submenu
3. Its ID will be emitted in the `momo:menu` event
4. Add the ID to the `MenuAction` union in `web/src/lib/desktop.ts`
5. Handle it in `web/src/hooks/use-desktop-menu.ts`

## Deep links

The `momo://` URL scheme is defined in `tauri.conf.json` and implemented in `desktop/src-tauri/src/deeplink.rs`.

A deep link proposes opening a project directory. The shell:

1. Parses the URL: `momo://open?path=/absolute/path`
2. Validates the path (must be absolute, no `..`, must exist and be a directory)
3. Emits `momo:open-project-request` with the validated path — the UI then shows a confirmation
4. If validation fails, emits `momo:deep-link-rejected` with the reason

The web UI handles these events in `web/src/lib/desktop.ts` and shows the same confirmation as the Files panel's "open another project" button.

The parsing is deliberately strict: unknown actions, relative paths, and traversal are rejected outright, not ignored. This prevents silent mishandling by old builds when new link types are added later.

To add a new deep link action:
1. Add a new `parse_<action>()` function in `deeplink.rs`
2. Extend the parsing logic in `parse()` to call it
3. Emit an appropriate event (or extend `OpenRequest` if it fits the existing one)
4. Handle the event in the web UI

## Startup and shutdown

**Startup** (in `lib.rs` setup):

1. The window appears immediately
2. A background thread spawns the gateway with `Supervisor::start()`
3. While the gateway is starting, the UI shows a boot screen
4. Once the gateway's `/health` endpoint responds, the UI loads

The boot screen is essential: gateway startup can take several seconds (opening the session database, starting MCP servers). Without it, the window appears unresponsive.

**Shutdown** happens on `tauri::RunEvent::Exit`, not `WindowEvent::Destroyed`. The comment in `lib.rs` at line ~425 explains why: Cmd-Q on macOS terminates the app without reliably delivering the window-destroyed event.

When shutting down, `gateway.shutdown()` is called, which:

- On Unix: sends SIGTERM to the child process group, waits 5 seconds, then SIGKILL
- On Windows: calls `TerminateProcess` immediately (Windows has no SIGTERM)

Getting this wrong leaks the gateway process and its ephemeral port; the next launch will fail to bind.

## The keychain and environment variables

**Why the shell owns the keychain**: On macOS, keychain items carry an ACL listing which binaries can read them. A cross-binary read (written by the shell, read by the gateway child) prompts the user for permission. Since a spawned child with no UI cannot answer that prompt, the shell reads the keychain and passes keys as environment variables instead.

**Key precedence** (see `secrets.rs` for details):

1. Environment variables injected by the shell (from keychain)
2. Workspace `.env` variables
3. OS keychain

This is the opposite of the CLI, where `.env` wins. In the GUI, what you type into Settings takes precedence, which is the right default.

**There is no "get secret" command.** The UI calls `secret_status()` to learn whether a key is set and whether `.env` defines it too; there is no way to read the key back out. This prevents exfiltration.

## Gateway supervision

`gateway.rs` is the load-bearing file. It spawns `momo-fetch --gateway` with:

- Port 0 (kernel-assigned); the child reports its actual port on stdout
- Both `tauri://localhost` and `http://tauri.localhost` CORS origins (for the webview)
- Working directory set to the project root (so `.harness/` is writable)

The supervisor drains both stdout and stderr on separate threads. It watches for the `MOMO_GATEWAY_LISTENING <url>` line and times out after 30 seconds.

The last 50 lines of output are kept in memory for the crash screen. A failed gateway should always show up there, even if the UI never renders.

## Testing the shell

The Rust files have unit tests; run them with:

```bash
cd desktop/src-tauri
cargo test
```

Tests cover:

- URL parsing (`deeplink.rs`)
- Key precedence and `.env` parsing (`secrets.rs`)
- Listening line parsing (`gateway.rs`)

For integration testing (the full app), build and run it:

```bash
cd web && npm run build:desktop
cd ../desktop/src-tauri && cargo tauri build --bundles app
# Then open the .app / .exe / .AppImage
```

Or use `cargo tauri dev` for a live-reloading dev build (slower startup because it watches the file system).

## Common tasks

### Add an IPC command

1. Write a handler in `lib.rs` with `#[tauri::command]`
2. Add its name to the `tauri::generate_handler![…]` list passed to `.invoke_handler()` (`lib.rs:329`)
3. Export the signature in `web/src/lib/desktop.ts`
4. The web code can then call it via `window.__TAURI__.core.invoke("command_name", args)`

### Add a menu item

1. Create it in `menu.rs` with `MenuItem::with_id()`
2. Add it to a submenu
3. Add its ID to the `MenuAction` union in `web/src/lib/desktop.ts`
4. Handle it in `web/src/hooks/use-desktop-menu.ts`

### Handle a new event from the gateway

The gateway is a child process spawned by `Supervisor::start()`. It communicates back through:

- **Stdout**: a single `MOMO_GATEWAY_LISTENING` line (parsed, then drained for debug output)
- **Stderr**: diagnostic output (kept for the crash screen)

There is no IPC between the shell and gateway. The web UI talks to the gateway over HTTP once it's running.

### Change the bundling or resources

Edit `tauri.conf.json`. The `beforeBuildCommand` runs before every `cargo tauri build` and:

1. Rebuilds `momo-fetch` with `cargo build --release`
2. Copies it to `desktop/src-tauri/binaries/`

This ensures the bundled gateway is never stale. Do not stage the binary by hand.

## See also

- [Tauri-Specific Mechanics](./tauri-guide.md) — detailed reference for sidecar supervision, IPC, menus, deep links, and bundling
- [MOMO WORK Installation Guide](./momo-desktop-install.md) — building and first run
- [Project Landscape](../project-landscape.md) — the four parts of the system and dependency graph
