# Installing MOMO WORK from source

MOMO WORK is the desktop app for the momo-fetch agent: chat, tool approvals,
memory search and sandboxed file access in one window. It starts and supervises
its own gateway — there is no server to run separately.

This guide builds it from source. That is the supported way to install it, and
it means **no code signing is involved**: an app you build on your own machine
carries no download quarantine, so macOS Gatekeeper and Windows SmartScreen have
nothing to warn about. (`cargo tauri build` applies an ad-hoc signature on Apple
Silicon automatically, which is all macOS requires to run it.)

---

## 1. Prerequisites

| | Version | Check with |
|---|---|---|
| Rust | 1.85+ | `rustc --version` |
| Node | 20+ | `node -v` |
| Tauri CLI | 2.x | `cargo tauri --version` |

Install the Tauri CLI if it is missing:

```bash
cargo install tauri-cli --version "^2"
```

**macOS** also needs the Command Line Tools:

```bash
xcode-select --install
```

**Linux** needs the webview and tray libraries. On Debian/Ubuntu:

```bash
sudo apt-get install -y \
  libwebkit2gtk-4.1-dev libappindicator3-dev librsvg2-dev patchelf
```

**Windows** needs the [Microsoft C++ Build Tools][msvc] and WebView2, which ships
with Windows 11 and current Windows 10.

[msvc]: https://visualstudio.microsoft.com/visual-cpp-build-tools/

---

## 2. Build

```bash
git clone https://github.com/aekram43/momo-fetch.git
cd momo-fetch

# 1. the gateway binary
cargo build --release

# 2. the web UI — build:desktop, NOT build
cd web
npm ci
npm run build:desktop
cd ..

# 3. the app — this rebuilds the gateway and stages it automatically
cd desktop/src-tauri
cargo tauri build
```

Step 3 runs `cargo build --release --bin momo-fetch` and copies the result into
`desktop/src-tauri/binaries/` before bundling, so the app can never ship a
gateway older than the source you built it from. That copy used to be a manual
step, and it went stale in exactly the way you would expect: a UI calling an
endpoint the bundled gateway did not have yet.

> **`npm run build:desktop`, not `npm run build`.**
> Next.js writes absolute asset URLs, so the bundle has to know where it will be
> mounted. The gateway serves the UI under `/ui` and needs that prefix; the
> desktop app serves it at the root and must not have it. Use the wrong one and
> every stylesheet 404s inside the app — it opens, renders unstyled, and looks
> like a broken theme rather than a wrong path.

The bundle lands in `desktop/src-tauri/target/release/bundle/`:

| Platform | Artifact |
|---|---|
| macOS | `macos/MOMO WORK.app`, `dmg/*.dmg` |
| Windows | `msi/*.msi`, `nsis/*.exe` |
| Linux | `deb/*.deb`, `appimage/*.AppImage` |

On macOS you can drag the `.app` to `/Applications`, or just double-click it
where it is.

---

## 3. Add an API key — the app will not start without one

The app runs the agent locally, but the agent still needs a model provider.
On first launch it creates a workspace and then **exits with an error** until a
key is present. The boot screen names the exact file; it is here:

| Platform | Path |
|---|---|
| macOS | `~/Library/Application Support/com.createder.momo-worker/workspace/.env` |
| Linux | `~/.local/share/com.createder.momo-worker/workspace/.env` |
| Windows | `%APPDATA%\com.createder.momo-worker\workspace\.env` |

> **Or set it in the app instead.** The desktop app has an **API keys** section
> in Settings (⚙ in the header, or `Cmd+,`) that stores keys in the OS keychain
> — no file editing. It only appears in the desktop app, and it needs the app to start
> once first, so if you are stuck at the boot screen use `.env` for the first
> key and switch later if you prefer.

Create `.env` there with one line:

```
OPENROUTER_API_KEY=sk-or-...
```

Any of these work — use whichever provider you have:

```
OPENROUTER_API_KEY=...
ANTHROPIC_API_KEY=...
OPENAI_API_KEY=...
DEEPSEEK_API_KEY=...
GROQ_API_KEY=...
ZAI_API_KEY=...
```

Then reopen the app.

### Which key wins

A secret is resolved per *variable*, not per file, so different providers can
come from different places at the same time. For one variable:

| | CLI / browser | Desktop app |
|---|---|---|
| 1 | exported environment variable | key set in the app (injected by the shell) |
| 2 | `.env` | exported environment variable |
| 3 | OS keychain | `.env` |

The desktop app inverts the usual order on purpose: the shell reads the keychain
and hands the key to the agent as an environment variable when it starts it, so
what you typed into the app is what it uses. The panel says so when a `.env`
also defines that variable, because otherwise "I changed my key and nothing
happened" has no visible explanation.

Pick the model in the app's **Models** panel. Providers you have no key for are
listed but greyed out, with the reason.

---

## 4. First run

The default workspace is **not** your home directory — it is the dedicated
folder above. That is deliberate: the project root *is* the agent's sandbox, so
a fresh install should not be able to read everything you own.

To point it at real work, use **Customization → Files → open another project…**
in the left panel. It confirms first, because re-rooting moves the sandbox, the
memory vault and the session history together, and restarts the agent.

The app starts in **strict** permission mode: it asks before anything that
changes your machine. When it asks, read the dialog — approving a tool grants it
for the rest of the session, by tool name, and the dialog says so.

### Finding your way around

Three regions, and each answers one question.

| Where | Question it answers |
|-------|--------------------|
| **Left panel** | What does this agent have to work with? Sessions, plus a collapsible **Customization** group over agents, tools, memory and files. |
| **Right panel** | What is it doing *right now*? The current turn's tool calls, and the files this turn wrote. |
| **Settings** (⚙ / `Cmd+,`) | How is it set up? Models, API keys, permissions, appearance. |

**Models** — a provider dropdown, then a model list for that provider. The model
list is queried from the provider itself, so it is what your account can
actually reach rather than a list baked into the app. It shows five entries —
the current model plus the four you last switched to — and the whole catalogue
as soon as you type. Providers with no key stay in the list, disabled, labelled
with why.

**Appearance** — dark, light, or auto (follows the OS, and keeps following it if
you change the system setting while the app is open). The sound toggle is here
too; it is off by default.

**The status bar tells you what is standing.** Bottom right, next to the turn
phase:

- `● N auto-run` — N tools will run without asking. Click it to see which, or
  revoke them.
- `● yolo · nothing asks` — you are in yolo. Nothing will be confirmed at all.

Nothing appears there when nothing is granted. **The badge exists because
approval is by tool name and lasts until you quit the app** — one "Approve" on
`shell_exec` means every later shell command runs silently, in this session and
any session you open afterwards. Quitting clears it; nothing is written to disk.

### Keyboard

| Key | Action |
|-----|--------|
| `Cmd/Ctrl+,` | Settings |
| `Cmd/Ctrl+B` | Toggle the left panel |
| `Cmd/Ctrl+J` | Toggle the right panel |
| `Cmd/Ctrl+N` | New session |
| `Cmd/Ctrl+O` | Open a project |
| `Esc` | Interrupt the running turn — or close Settings, or deny a pending approval. The approval dialog owns it first, then Settings; only then does it reach the agent. |

---

## 5. Updating

```bash
git pull
cd web && npm ci && npm run build:desktop && cd ..
cd desktop/src-tauri && cargo tauri build
```

`cargo tauri build` rebuilds the gateway and stages it into the bundle itself,
so the app can never ship a gateway older than the source you built it from.
That copy used to be a manual step and it went stale exactly as you would
expect — a UI calling an endpoint the bundled gateway did not have yet, working
in the browser and 404ing in the app.

There is no auto-updater. For a source install `git pull` *is* the update
mechanism, and it is more transparent than a background download.

> **If the app still shows the old UI after rebuilding, the frontend was not
> re-embedded.** Tauri compiles `web/out` into the binary, and that only happens
> when the Rust crate itself rebuilds — change nothing but the UI and cargo can
> reasonably decide there is nothing to do, leaving the previous assets in
> place. Force it:
>
> ```bash
> touch desktop/src-tauri/src/lib.rs
> cargo tauri build
> ```
>
> This is easy to lose an hour to: the app looks broken or half-updated when the
> source is fine. If a UI change is not showing up, rule this out first.

---

## 6. Troubleshooting

**The window opens but everything says "Could not load".**
The UI cannot reach its gateway. Check the log (below) for a `gateway listening`
line. If it is missing, the gateway failed to start and the boot screen should
say why.

**"Secret not found for 'openrouter'".**
Section 3. The message includes the exact `.env` path to create.

**The app opens unstyled, plain black text on white.**
The web UI was built with `npm run build` instead of `npm run build:desktop`.
Rebuild with the right one and rebuild the app.

**The model list says it could not list them.**
Not a failure you have to fix to keep working. The catalogue is fetched from the
provider, and no key, an unreachable provider or one with no catalogue API all
end up here — the reason is printed, and the picker turns into a text field so
you can type a model name and carry on. `refresh` re-asks; the answer is cached
for 30 minutes.

**A UI change does not appear after rebuilding.**
The frontend was not re-embedded — see the note in section 5.
`touch desktop/src-tauri/src/lib.rs` and rebuild.

**Logs**

| Platform | Path |
|---|---|
| macOS | `~/Library/Logs/com.createder.momo-worker/MOMO WORK.log` |
| Linux | `~/.local/share/com.createder.momo-worker/logs/` |
| Windows | `%LOCALAPPDATA%\com.createder.momo-worker\logs\` |

The log includes the gateway's own output, so a startup failure shows up there
even if the window never renders.

**Two copies of the app.**
Only one can run: the harness keeps process-global state, and two gateways over
one workspace would corrupt the session database. Launching again focuses the
existing window.

---

## Using the browser instead

The desktop app is a wrapper. The same UI runs in a browser:

```bash
cd web && npm run build          # note: build, not build:desktop
cd .. && cargo run --release -- --gateway
```

Then open <http://localhost:3000/ui>. The gateway serves the UI from its own
origin, so nothing else needs configuring, and it redirects to `/ui/` — the
trailing slash matters, because assets are referenced relatively so that one
build works both here and at `/` inside the app.

Two things are desktop-only, and the browser says so rather than hiding them:
**API keys** (keychain storage needs the shell — use `.env` in a browser) and
**open another project**.

Do **not** widen `cors_origins` to `"*"` to make a separate dev server work
against it. With authentication off, that lets any website you visit read your
files through the gateway and run shell commands through the agent — the gateway
refuses to start in that combination for exactly this reason. Add the specific
origin instead.
