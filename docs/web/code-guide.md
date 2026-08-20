# Code Guide — Web UI

For developers about to change the code.

## File Structure

```
web/src/
├── app/
│   ├── layout.tsx          # Pre-paint theme script + font loading
│   ├── globals.css         # Theme tokens and CSS reset
│   └── page.tsx            # App root
├── components/
│   ├── chat/               # Composer, messages, tool cards, approval dialog
│   ├── layout/             # App shell, header, sidebar, status bar
│   ├── settings/           # Settings dialog and four panels
│   ├── detail/             # Right panel: this turn's tool calls + changed files
│   ├── sidebar/            # Session list, agent picker, tool status
│   └── shared/             # Reusable: toaster, buttons, etc.
├── hooks/
│   ├── use-chat-stream.ts  # Opens stream, fans events into store
│   ├── use-gateway-resource.ts  # Fetch + reload pattern
│   └── use-*.ts            # Keyboard shortcuts, theme, desktop integration
├── lib/
│   ├── api-client.ts       # Gateway URL resolution, all HTTP calls
│   ├── sse-parser.ts       # Hand-rolled SSE decoder
│   ├── types.ts            # Wire types from gateway (source of truth is src/gateway/v2_types.rs)
│   ├── preferences.ts      # localStorage: view state only, never server state
│   └── *.ts                # Utilities: sounds, highlighting, etc.
├── stores/
│   ├── ui-store.ts         # Panel visibility, theme, turn phase
│   └── chat-store.ts       # Messages, tools, approval, tokens
└── package.json            # npm scripts and deps
```

### Key Files by Task

| Task | File |
|------|------|
| Add a new API call | `web/src/lib/api-client.ts` |
| Handle new SSE event | `web/src/lib/sse-parser.ts` and `web/src/lib/types.ts` |
| Add a settings panel | `web/src/components/settings/` and `web/src/stores/ui-store.ts` |
| Change the layout | `web/src/components/layout/app-shell.tsx` |
| Add a store | `src/stores/`, export from the module |
| Change theme colors | `web/src/app/globals.css` (CSS variables) and `web/src/lib/preferences.ts` |

## Core Patterns

### The Turn

1. **Send** (`useChatStream.send()`)
   - Posts to `/v2/chat/stream` via `openChatStream()` with the user's text
   - Creates an `AbortController` to allow interruption

2. **Receive** (async generator `readStream()`)
   - Reads chunks from `response.body`
   - Passes them through `SseFrameParser`
   - Yields typed `StreamEvent` objects

3. **Process** (event handler in `useChatStream`)
   - Dispatches each event to the chat store (`onRole`, `onText`, `onToolStart`, etc.)
   - Updates the UI store turn phase (`running` → `awaiting-approval` → `idle`)

4. **Stop** (`useChatStream.stop()`)
   - Posts to `/v2/chat/interrupt` first (tells the gateway)
   - Then aborts the fetch (closes our end)
   - Both are necessary: interrupting tells the harness; aborting releases the connection

### Stores & Hydration

**UiStore** (`web/src/stores/ui-store.ts`)

- Holds view state (sidebar open/closed, theme, sound toggle)
- Persisted to `localStorage` with key `momo-worker.prefs.v1`
- **Hydration:** The pre-paint script in `layout.tsx` reads theme before first render to prevent a flash. The store's `hydrate()` method is called after mount (not during module evaluation) to apply saved prefs without triggering hydration mismatch.

**ChatStore** (`web/src/stores/chat-store.ts`)

- Holds the conversation, turn state, pending approval, tokens, session ID
- Not persisted; clears on refresh
- **Turn lifecycle:** `turnId` is set by the `role` event, cleared by `done` or `error`
- **Approval:** `pendingApproval` is set when `approval_required` arrives, cleared when `approval_resolved` arrives. The call id changes after approval (the harness spawns a new turn), so call ids are *not* stable across an approval.
- **Artifacts:** `turnArtifacts` holds the files the turn changed, from the gateway's `artifacts` event — a comparison of the project tree before and after the turn, so it arrives once at the end and covers writes made through the shell. Cleared when a turn starts and when history loads: changed files are observed live and never stored, so a session loaded from history has none. The detail panel renders these alongside the rows it still derives from write-shaped tool calls (deduplicated by path), because those appear *as* the agent works and cover writes outside the sandbox root, such as `mem_write` into the vault.

**Fetching gateway resources** (`useGatewayResource`)

Every settings panel and status component uses this hook:

```ts
const { data, error, reload } = useGatewayResource(getSettings);
```

- Fetches on mount
- Refetches on `serverStateNonce` change (when this tab mutates server state)
- Provides manual `reload()` for manual refetch (e.g., after a delete)
- Cancels inflight requests if the component unmounts

### SSE Parser

The hand-rolled `SseFrameParser` (not `EventSource`) handles:

- Frames split across chunk boundaries
- Multi-byte UTF-8 characters straddling chunks
- Both `\r\n` and `\n` line endings
- Multiline `data:` fields (joined with `\n`)
- Comment lines starting with `:` (keep-alive during waits)
- Unknown event types (surfaced, never silently dropped)

Each frame is a `RawFrame` (event, data, optional id), parsed into a typed `StreamEvent` discriminated union. Unknown events and unparseable JSON both yield an `unknown` event type so type drift is visible, not silent.

**Why not `EventSource`?** It is GET-only; cannot set headers. The stream is a POST with JSON body and bearer token.

### Preferences & Theme

The theme resolves in layers:

1. **Pre-paint script** in `layout.tsx` runs inline in `<head>` and stamps `data-theme` on `<html>` before React hydrates. Prevents dark flash on light-mode machines.

2. **Store hydration** after mount reads `localStorage` and applies the full pref set.

3. **Runtime switches** via `setTheme()` update CSS vars and `localStorage`.

**Never store server state in localStorage.** Session, model, permissions are always gateway state (spec F28). A remembered value is a belief that can be wrong the moment another tab changes it. Use `/health` and event streams instead.

## Tests

Run tests with `npm run test` (one-shot) or `npm run test:watch`.

Key test files:

- `web/src/lib/sse-parser.test.ts` — Every edge case of the frame parser (split frames, encoding, etc.)
- `web/src/lib/preferences.test.ts` — Theme resolution and `localStorage` round-trip

## Building

### For the gateway (`/ui` mount)

```bash
npm run build
```

Sets `basePath: "/ui"`. The built `out/` directory is served by the gateway at `/ui/*`.

### For Tauri (`/` mount)

```bash
npm run build:desktop
```

Sets `basePath: ""` (via `MOMO_BASE_PATH=` env var). The built `out/` directory is embedded and served by the Tauri shell at `/*`.

Using the wrong build causes every asset to 404 — all CSS and JS URLs are absolute. The CSS files are generated without a server; there is no fallback route.

## Gateway Integration

**URL resolution** (`gatewayUrl()` in `api-client.ts`)

1. `window.__GATEWAY_URL__` — Tauri runtime injection
2. Same origin if served at `/ui` — the served origin is correct
3. `NEXT_PUBLIC_GATEWAY_URL` env var
4. Default to `http://localhost:3000`

**Auth** — Token is memory-only, never in `localStorage`. Set via `setAuthToken()`, read from `Authorization: Bearer` header on all requests.

**Error handling** — `ApiError` carries the typed `code` field from the gateway so callers can branch on `turn_in_progress`, `stale_approval`, etc., not string matching.

## Dependencies

Core dependencies:

- **Next.js 16.3.0** — SSR → static export
- **React 19.2.8** — UI framework
- **Zustand 5.0.14** — State management
- **Tailwind 4** — Styling with CSS variables for theme

**Important:** Read `AGENTS.md` in this directory before changing Next.js patterns. Next 16 has breaking changes from your training data.

## Common Mistakes

### Storing server state in localStorage

Settings panels sometimes want to remember the user's last choice. Don't. The gateway's session, model, and permissions are global (spec §2.3). Caching locally diverges from reality when another tab or the REPL changes it.

### Putting a collapsible panel around the approval dialog

The approval dialog must be a focus-trapped modal that nothing can cover. Nesting it inside a panel that can collapse means a narrow-viewport user can strand the turn until timeout.

### Auto-retrying the stream request

Never. Re-sending a turn double-bills and can re-run tools that already executed. Let failures be visible; the user can retry manually.

### Holding the Mutex across an await

Not a frontend concern, but worth knowing: `memory/vault.rs` has a `std::sync::Mutex`. Don't hold it across an `await` — it deadlocks. The Rust code is careful; the frontend never touches it.

## Extending the UI

### Add a settings panel

1. Create `web/src/components/settings/new-panel.tsx`
2. Add the new id to the `SettingsTab` union in `web/src/stores/ui-store.ts`
3. Add an entry to the `TABS` array in `settings-dialog.tsx` — `id`, `label` **and
   `blurb`**; the blurb is required and becomes the tab's tooltip
4. Render it in the content pane beside the other four

The panels are plain components taking no props. They fetch their own data with
`useGatewayResource`, so nothing has to be threaded through the dialog.

### Add a gateway resource fetch

1. Add the HTTP function to `web/src/lib/api-client.ts`
2. Call `useGatewayResource()` with that function in your component
3. If it mutates server state, call `useUiStore().bumpServerState()` after success

### Add a keyboard shortcut

Edit `web/src/hooks/use-shortcuts.ts`. The hook is already called once from
`AppShell`; there is nothing to register per shortcut. Document the new binding
in the [user guide](user-guide.md) and, if it should also appear in the desktop
app's native menu, in `desktop/src-tauri/src/menu.rs`.

### Change the theme

Color tokens are CSS custom properties in `globals.css`, swapped by `data-theme` attribute on `<html>`. Add a new token:

1. Define it in the `:root, :root[data-theme="dark"]` block and again under
   `:root[data-theme="light"]` — dark is the default, and the explicit
   `data-theme` selector is what the theme toggle stamps
2. Add it to the `@theme inline` block so Tailwind emits `var(--token-name)`
3. Use it in Tailwind classes: `text-signal`, `bg-consent`, etc.

## Resources

- Spec: `docs/spec/momo-worker.md` (§6 UI layout, §7 endpoints)
- Types mirror: `src/gateway/v2_types.rs` and `v2_handlers.rs` in the Rust harness
- House style: `docs/project-landscape.md` and `docs/harness/user-guide.md`
- Related: `docs/desktop/` for Tauri integration
