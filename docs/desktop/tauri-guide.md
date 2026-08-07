# Tauri-Specific Mechanics

This document details how MOMO WORK uses Tauri's features: sidecar supervision, IPC, menus, deep links, and bundling.

For the code guide and task-oriented instructions, see [Code Guide: Modifying the Desktop Shell](./code-guide.md).

## Sidecar supervision

The gateway (`momo-fetch --gateway`) runs as a child process spawned by the shell at startup. This is "sidecar" architecture — a companion process managed by the main app.

### Port allocation

The shell tells the gateway to use port 0, which means the OS allocates an ephemeral port. Between the request and the child's bind, another process could claim the port you guessed, so the shell never picks a port itself.

**Command**: `momo-fetch --gateway --gateway-port 0 --project <dir>`

The gateway reports its actual port on a single stdout line:

```
MOMO_GATEWAY_LISTENING http://127.0.0.1:51686
```

The shell parses this exact format (see `gateway.rs` line 156) and blocks until it arrives, timing out after 30 seconds.

**Why the format is strict**: Tests in `gateway.rs` (line 295) verify exact parsing. Loosening the parse would silently accept wrong formats; fixing the producer (the gateway) is better than accepting sloppy input.

### CORS origins

The Tauri webview is not same-origin with `127.0.0.1:<port>`, so the shell grants both webview origins CORS access:

```
--gateway-allow-origin tauri://localhost
--gateway-allow-origin http://tauri.localhost
```

Both are passed regardless of platform because neither hurts.

### Working directory

The gateway must run with cwd set to the project directory. A bundled macOS app inherits cwd `/` (read-only), so without this the child dies with "Read-only file system". The shell sets it explicitly:

```rust
cmd.current_dir(project)
```

### Shutdown

Shutdown is the load-bearing part. The shell gets one chance to shut down cleanly before the OS force-kills it.

**On Unix** (`gateway.rs` line 234):

1. `SIGTERM` to the process *group* (see `setsid` below), so MCP servers the gateway spawned die with it
2. Wait up to 5 seconds for graceful exit
3. `SIGKILL` anything left

The SIGTERM is sent to the *group* with a negative PID to ensure MCP servers (which run as subprocesses of the gateway) are terminated, not reparented to init.

**On Windows** (`gateway.rs` line 254):

1. Call `TerminateProcess` immediately

Windows has no SIGTERM; there is no graceful variant.

**Timing matters**: The shell hooks to `tauri::RunEvent::Exit`, which fires on every exit path (Cmd-Q, close window, etc). The old code used `WindowEvent::Destroyed`, which does not reliably fire on Cmd-Q on macOS; that leaked the gateway process and its port.

### Process group isolation (Unix only)

On Unix, the shell calls `libc::setsid()` in the child before spawning (via `pre_exec`) so the gateway becomes the leader of its own process group. This ensures:

- Ctrl-C in a terminal-launched app is not delivered to the child
- MCP subprocesses go down with the gateway, not reparented to init

Windows has no process groups; this is Unix-only.

### Output capture

Both stdout and stderr are piped and drained on separate threads (lines 127–134). The last 50 lines are kept in a bounded buffer for the crash screen.

Stdout is watched for the `MOMO_GATEWAY_LISTENING` line; all other output goes to the buffer. Stderr is entirely captured. A failed startup shows up in both places.

**Why both streams**: The harness prints startup failures (missing API key, unreadable config) to stdout, not stderr. Watching only stderr would leave the crash screen blank for errors users can actually fix.

## IPC command dispatch

IPC (Inter-Process Communication) between the web UI and shell happens through Tauri's `invoke` mechanism. The shell defines handlers; the UI calls them.

### Defining a command

In `lib.rs`:

```rust
#[tauri::command]
fn my_command(app: tauri::AppHandle, state: State<'_, AppState>) -> Result<String, String> {
    Ok("result".to_string())
}
```

Register it:

```rust
.invoke_handler(tauri::generate_handler![my_command, /* ... */])
```

The `#[tauri::command]` macro generates the glue code. Arguments and return values must implement `serde::{Serialize, Deserialize}`.

### Gateway URL injection

The gateway URL is tricky: it is only known at runtime, and the web bundle is compiled ahead of time. The shell solves this two ways:

1. **Injection**: After the gateway is ready, the shell injects JavaScript:

   ```rust
   window.eval(format!(
       "window.__GATEWAY_URL__ = {};",
       serde_json::to_string(url)?
   ))
   ```

2. **Fallback command**: If the injection races and misses, the UI calls `gateway_url()` to fetch it.

The web code checks `window.__GATEWAY_URL__` first (instant, if injection worked), then calls the command (slower, but catches the edge case).

### State management

The shell keeps three pieces of state:

```rust
pub struct AppState {
    pub supervisor: Mutex<Option<Supervisor>>,
    pub url: Mutex<Option<String>>,
    pub startup_error: Mutex<Option<String>>,
}
```

- `supervisor`: The `Supervisor` struct holding the child process and output buffer
- `url`: The gateway's listening URL, or `None` if startup is pending or failed
- `startup_error`: An error message if startup failed (for the UI to display)

All three are wrapped in `Mutex` because Tauri command handlers run on the main thread and may be called concurrently.

### The gateway restart dance

When a key is stored or deleted, the gateway must restart to pick up the new environment. `restart_gateway()` does this:

1. Stop the old gateway (shutdown with SIGTERM/grace period/SIGKILL)
2. Wait for it to exit
3. Inject the new keys from the keychain
4. Spawn a new one
5. Poll `/health` until ready
6. Inject the URL into the webview
7. Reload the page

The reload is necessary because the URL changed and the web code cached the old one at boot.

## Menus and tray

Menus and the system tray are integrated through a single event channel.

### Menu structure

Defined in `menu.rs` line 21:

- **File**: New Session, Open Project, Settings, Close Window
- **Edit**: Undo, Redo, Cut, Copy, Paste, Select All (platform-standard)
- **View**: Toggle Sessions Panel, Toggle Detail Panel, Interrupt Turn

Each item is created with `MenuItem::with_id(app, "id", "Label", true, Some("CmdOrCtrl+N"))?` to associate an ID and keyboard shortcut.

### Event dispatch

When a menu item is clicked, `app.on_menu_event()` fires and dispatches a `CustomEvent`:

```rust
let _ = window.eval(format!(
    "window.dispatchEvent(new CustomEvent('momo:menu', {{ detail: {} }}))",
    serde_json::to_string(id)?
))
```

The event name is `momo:menu`, and the detail is the menu item's ID as a string.

### Tray icon

The tray (lines 89–139) shows these items:

- **Show Window** (id: `tray-show`) — handled in Rust; shows/focuses the window
- **New Session** (id: `new-session`) — emits `momo:menu` event
- **Quit** (id: `tray-quit`) — calls `app.exit(0)` to go through the normal shutdown path

Left-clicking the tray icon also shows/focuses the window (`show_menu_on_left_click` is off).

The tray is installed in `setup()` as best-effort; if it fails, the app continues (see line 354).

### Binding menu actions to shortcuts

The `MenuAction` union in `web/src/lib/desktop.ts` lists all possible menu IDs:

```typescript
export type MenuAction =
  | "new-session"
  | "open-project"
  | "toggle-sidebar"
  | "toggle-detail"
  | "settings"
  | "interrupt";
```

The web UI listens for `momo:menu` events in `use-desktop-menu.ts` and routes them to the same handlers as keyboard shortcuts. This ensures an action never has two implementations, so menus and shortcuts always behave the same way.

## Deep links

The `momo://` URL scheme allows external sources (web pages, chat messages, documents) to propose opening a project directory.

### Registration

In `tauri.conf.json`:

```json
"plugins": {
  "deep-link": {
    "desktop": {
      "schemes": ["momo"]
    }
  }
}
```

This registers the OS-level protocol handler. On macOS, deep links arriving while the app is running come through a Tauri plugin callback (line 339–348 in `lib.rs`). On Windows and Linux, they arrive as argv to a second instance, which the single-instance plugin intercepts (line 312–314).

### Parsing and validation

All parsing lives in `deeplink.rs`. The URL format is:

```
momo://open?path=/absolute/path
```

Validation (lines 62–99):

1. **Scheme**: Must be `momo://`
2. **Action**: Only `open` is recognized; anything else is rejected (not ignored)
3. **Path parameter**: Required; percent-encoded paths are decoded
4. **Absolute path**: Must not be relative
5. **No traversal**: No `..` allowed, even percent-encoded (`%2F..%2F`)
6. **Directory check**: Must exist and be a directory

Rejection reasons are distinct enums (lines 24–32), so the UI can explain why a link was refused.

**Why strict parsing**: A malformed deep link from an old build must not be silently accepted or ignored. Future actions cannot be added without the possibility of old builds mishandling them.

### Event emission

If a link parses and validates, the shell emits:

```rust
app.emit("momo:open-project-request", req)
```

where `req.path` is the validated absolute path.

If validation fails:

```rust
app.emit("momo:deep-link-rejected", reason.message())
```

The web UI (in `lib/desktop.ts`) listens for both events. For the open request, it shows the same confirmation dialog as "open another project…" in the Files panel. For rejection, it shows the reason.

**Validation is not authorization**: A validated deep link is a *proposal*, not a command. The user must confirm before the sandbox re-roots.

## Bundling

Bundling is configured in `tauri.conf.json` and automated by `beforeBuildCommand`.

### beforeBuildCommand

Runs before every `cargo tauri build`:

```bash
sh -c 'cd "$(git rev-parse --show-toplevel)" && cargo build --release --bin momo-fetch && cp target/release/momo-fetch desktop/src-tauri/binaries/'
```

This:

1. Rebuilds the `momo-fetch` binary at release optimisation
2. Copies it to `desktop/src-tauri/binaries/`

The binary lands at `binaries/momo-fetch` (or `.exe` on Windows), not in the root, because `resources: ["binaries/"]` in the config copies the *directory* itself.

**Never stage the binary by hand.** The old flow did; it shipped stale gateways. The build system now guarantees the bundled binary matches the source tree.

### Resources

```json
"resources": ["binaries/"]
```

This copies the `binaries/` directory into the app bundle. At runtime, the shell looks for the binary at:

```rust
app.path().resource_dir().join("binaries").join("momo-fetch")
```

or (if the config changes to flatten it):

```rust
app.path().resource_dir().join("momo-fetch")
```

Both paths are checked for forward compatibility.

### Platforms and bundle types

| Platform | Bundle types |
|---|---|
| macOS | `.app` (directory), `.dmg` (disk image) |
| Windows | `.msi` (installer), `.nsis` (portable exe) |
| Linux | `.deb` (package), `.AppImage` (portable binary) |

All are built together by `cargo tauri build`. See [RELEASING.md](../../desktop/RELEASING.md) for signing and publishing.

### Development builds

In dev mode (`cargo tauri dev`), the shell looks for the gateway at:

```rust
root.join("target/release/momo-fetch")
root.join("target/debug/momo-fetch")
```

If neither exists, it tries `which momo-fetch`. This allows working on the gateway and shell simultaneously without rebuilding the bundle.

## Key precedence and environment variables

Keys reach the gateway as environment variables injected at spawn time. This affects which value wins when multiple sources define the same variable.

### The precedence order (for the gateway)

1. **Environment variables injected by the shell** (from the keychain)
2. **Workspace `.env` file** (read by `dotenvy` when the gateway starts)
3. **OS keychain** (gateway's direct keychain access, not used in the desktop app)

### Why the shell injects

On macOS, keychain items carry an ACL listing which binaries can access them. The gateway (a child process) cannot read an item created by the shell without a user prompt. Instead, the shell reads the keychain and passes the keys as environment variables.

This design choice makes all three platforms (macOS, Linux, Windows) behave the same way.

### In the desktop app specifically

1. You set a key in Settings
2. The shell reads it from the keychain via `keyring::Entry::new(SERVICE, provider).get_password()`
3. The shell calls `Supervisor::start()` with `env_overrides()` — a HashMap of injected vars
4. The gateway sees those variables first, before `.env`

So a key configured in the app overrides the same key in `.env`. This is the opposite of the CLI, where `.env` wins (because there is no injection).

### The SERVICE name

The shell and gateway must use the same keychain service name, or the gateway would look in the wrong place. Both use:

```
SERVICE = "momo-fetch"
```

See `secrets.rs` line 51 and the gateway's `src/config/secrets.rs` for the matching value.

## Single-instance plugin

The shell uses `tauri_plugin_single_instance` to ensure only one copy runs at a time. This is not convenience — it is correctness.

The harness keeps process-global state: the session database, the ephemeral port. Two instances over one `.harness/` would corrupt the DB and race for the port.

When a second instance is launched, the plugin:

1. Focuses and shows the existing window
2. On Windows and Linux, passes any argv to the existing instance
3. The existing instance handles deep links that arrive this way

See `lib.rs` line 302–315 for the handler.

## Window state persistence

The `tauri-plugin-window-state` plugin (line 322) remembers:

- Window width and height
- Window position
- Maximised state

These are restored on each launch. The state is stored in the platform's config directory (e.g., `~/Library/Preferences/` on macOS).

## See also

- [Code Guide: Modifying the Desktop Shell](./code-guide.md) — task-oriented instructions for common changes
- [MOMO WORK Installation Guide](./momo-desktop-install.md) — building and troubleshooting
- [Project Landscape](../project-landscape.md) — the four parts of the system and dependency graph
