# Installing MoMo Worker from source

MoMo Worker is the desktop app for the momo-fetch agent: chat, tool approvals,
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

# 3. stage the gateway so the app bundles it
mkdir -p desktop/src-tauri/binaries
cp target/release/momo-fetch desktop/src-tauri/binaries/     # .exe on Windows

# 4. the app
cd desktop/src-tauri
cargo tauri build
```

> **`npm run build:desktop`, not `npm run build`.**
> Next.js writes absolute asset URLs, so the bundle has to know where it will be
> mounted. The gateway serves the UI under `/ui` and needs that prefix; the
> desktop app serves it at the root and must not have it. Use the wrong one and
> every stylesheet 404s inside the app — it opens, renders unstyled, and looks
> like a broken theme rather than a wrong path.

The bundle lands in `desktop/src-tauri/target/release/bundle/`:

| Platform | Artifact |
|---|---|
| macOS | `macos/MoMo Worker.app`, `dmg/*.dmg` |
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

> **The OS keychain is not wired up.** `momo-fetch secrets set` looks like it
> should work and does not — nothing registers a keyring backend, so every
> keychain lookup fails. `.env` (or a real environment variable) is the only
> working path today.

Pick the model in the app's **Models** panel. Providers you have no key for are
listed but greyed out, with the reason.

---

## 4. First run

The default workspace is **not** your home directory — it is the dedicated
folder above. That is deliberate: the project root *is* the agent's sandbox, so
a fresh install should not be able to read everything you own.

To point it at real work, use **Files → open another project…** in the right
panel. It confirms first, because re-rooting moves the sandbox, the memory vault
and the session history together, and restarts the agent.

The app starts in **strict** permission mode: it asks before anything that
changes your machine. When it asks, read the dialog — approving a tool grants it
for the rest of the session, by tool name, and the dialog says so.

---

## 5. Updating

```bash
git pull
cargo build --release
cd web && npm ci && npm run build:desktop && cd ..
cp target/release/momo-fetch desktop/src-tauri/binaries/
cd desktop/src-tauri && cargo tauri build
```

There is no auto-updater. For a source install `git pull` *is* the update
mechanism, and it is more transparent than a background download.

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

**Logs**

| Platform | Path |
|---|---|
| macOS | `~/Library/Logs/com.createder.momo-worker/MoMo Worker.log` |
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
origin, so nothing else needs configuring.

Do **not** widen `cors_origins` to `"*"` to make a separate dev server work
against it. With authentication off, that lets any website you visit read your
files through the gateway and run shell commands through the agent — the gateway
refuses to start in that combination for exactly this reason. Add the specific
origin instead.
