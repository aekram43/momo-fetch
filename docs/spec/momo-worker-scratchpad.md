# MOMO WORK — Implementation Scratchpad / Handoff

> **Purpose:** Working state for the MOMO WORK build so another agent can continue without re-deriving anything.
> **Spec:** [`docs/spec/momo-worker.md`](./momo-worker.md) — read §0 (Code Audit) first; it is the load-bearing part.
> **Dispatch:** spec **§12** is the work plan (packages, model routing, waves). §9 below is now just a pointer into it.
> **Last updated:** 2026-08-05 · branch `dev` (0.9.1)
> **Build state:** `cargo check --all-targets` clean (warnings only) · rust **305 passed** · web **22 passed**, tsc + eslint clean

---

## 📊 Status at a glance

> **Keep this block current. It is the first thing anyone reads.**
> Update it at the end of every work package, together with §0 below.

**Theming · dev · 2026-08-06 · rust 305 · desktop 8 · web 24 · all clean · 🎉 ครบทุกอย่าง**

| ส่วน | เสร็จ | เหลือ | |
|---|---:|---:|---|
| **Gateway** (G1–G13 + R1) | **14** | **0** | ██████████████████ **100%** ✅ |
| **Wave-0 bugs** (B0–B4) | 5 | 0 | ██████████████████ 100% |
| **Frontend** (F1–F29) | **29** | **0** | ██████████████████ **100%** ✅ |
| **Desktop** (T1–T11, T12) | **11** | 0 | ██████████████████ **100%** ✅ |
| **Phase 1** (gateway + frontend) | **43** | **0** | ██████████████████ **100%** ✅ |
| **ทั้งโปรเจกต์** | **54** | 0 | ██████████████████ **100%** ✅ |

**🎉 แอป desktop รันได้จริง + deep link ปลอดภัย** — `.app` spawn gateway เอง, single-instance, ปิดแล้ว gateway ตายตาม, `momo://` ผ่านการ validate + ต้องกดยืนยัน เหลือแต่ signing/updater ที่ต้องใช้ credential

**T13 (code signing) / T11 (auto-updater) — ตัดออกจาก scope แล้ว ไม่ใช่งานค้าง**

Distribution model คือ **build จาก source** ซึ่งไม่ต้องใช้ signing เลย: ไฟล์ที่ build บนเครื่องตัวเองไม่มี `com.apple.quarantine` (flag นี้ browser เป็นคนติดตอนดาวน์โหลด) Gatekeeper จึงไม่ถาม และ SmartScreen ดูจาก Mark-of-the-Web ซึ่งก็ไม่มีเหมือนกัน `cargo tauri build` เซ็น ad-hoc ให้อยู่แล้ว — พอสำหรับรันบน Apple Silicon (ยืนยันบนเครื่องนี้: `Signature=adhoc`, ไม่มี quarantine)

Auto-update ก็ไม่เกี่ยว — source install อัปเดตด้วย `git pull` + rebuild

ถ้าวันหนึ่งจะแจก prebuilt binary ค่อยหยิบขึ้นมาทำ ขั้นตอนอยู่ใน `desktop/RELEASING.md` ครบแล้ว

**คู่มือติดตั้ง: [`docs/momo-desktop-install.md`](../momo-desktop-install.md)**

**✅ งานค้างทั้ง 3 อย่าง เคลียร์หมดแล้ว**

| | เรื่อง | ผล |
|---|---|---|
| ✅ | `security-review` บน G6 | รันแล้ว เจอ 2 ช่อง **แก้ทั้งคู่** — ดู §0 |
| ✅ | cost parity (G10 acceptance) | พิสูจน์แล้วบน paid model (zai) ทั้ง 3 path ตรงสูตรเป๊ะ |
| ✅ | `/v1/sessions` `event_count: 0` | เปลี่ยนเป็น `Option` → `null` (ไม่รู้) แทนที่จะโกหกว่า 0 |

**Human item เดียวที่เหลือ:** code-signing (T13 → T11) อยู่ ~สัปดาห์ 5 ไม่บล็อกอะไรตอนนี้ — build unsigned จาก T10 ใช้งานได้ปกติ

⚠️ **`ZAI_API_KEY` หลุดเข้า terminal transcript ระหว่าง session นี้ — ควร rotate**

---

## 0. Wave progress log

Newest first. One entry per work package, added on completion.

### ✅ Brand v2 — rebuilt from the artwork sheet · 2026-08-06

Replaced the first pass with assets cut from the proper brand sheet
(`brand/source/brand-sheet.png`, 1536×1024), which ships purpose-built **LOGO /
ICON / FAVICON** tiles rather than only a bare mark.

Palette re-sampled from it: navy **`#061a2b`**, orange **`#f76915`** — a shade
off the first values. Re-measured rather than assumed: orange on navy is 5.87:1,
still AA; every other token still passes in both themes.

**The muzzle problem is now settled for good.** Deriving from the hero mark eats
the dog's muzzle — its white background reaches the muzzle through the gap at
the chin, so a border flood-fill takes both. Confirmed again on this artwork.
The sheet's ICON and FAVICON tiles are already composed on navy, so they need no
keying at all; `make-assets.py` cuts those. Crop boxes were found by scanning for
bounding boxes, not measured by eye.

**Two edge-quality fixes worth keeping:**
- Colour-keying leaves a pale ring: the anti-aliased pixels between the white
  sheet and the navy shape are mid-grey, too dark for a white threshold to
  catch, and they render as a deliberate-looking stroke around the badge at
  22px. The round badge is now clipped with a **geometric circle mask** instead
  — the shape is known, so guessing it from colour is the wrong tool.
- Upscaling uses **bilinear**; the supersampler in `scale()` degrades to
  nearest-neighbour when enlarging and left stair-stepped curves.

Still soft at 1024 (the tiles are ~200px, so ~5×). Fine at real icon sizes; a
≥1024px master or an SVG remains the one asset worth requesting.

### ✅ Brand v1 — MOMO WORK · 2026-08-06

Logo, wordmark, icons and palette, from the supplied art. Product renamed
**MoMo Worker → MOMO WORK** across the app, bundle, docs and window chrome.

**Colours were sampled, not eyeballed:** navy `#041729`, orange `#f66614`.

**🔴 The brand orange and the design system collided, and the fix is the
interesting part.** The system reserves exactly one colour to mean *"the agent is
working, or wants something from you"* — that is what makes an approval
impossible to miss. The brand also has exactly one accent. Shipping both would
put two oranges on screen carrying different meanings and blunt the one that
matters.

So **the brand orange *is* the state colour**. `--signal` is `#f66614` in dark,
where it clears AA on the navy at 5.89:1 — it can carry meaning, not just
decorate. `--void` became the brand navy verbatim, so the app and the mark sit in
the same colour rather than near it.

Two consequences, both deliberate:

- **Light mode darkens the orange to `#bf4605`.** `#f66614` on white is 3.08:1 —
  fails AA, and a signal that reads as decoration has stopped being a signal.
- **The in-app wordmark is not orange**, though the brand lockup is. A
  permanently orange word in the corner is a standing false alarm. The mark keeps
  its orange bone; the words take ink, which is the brand navy in light anyway.
  Full-colour lockups belong on the icon and in docs, not in running chrome.

**Two traps hit while wiring it up:**

1. **`next/image` does not prefix `basePath` for unoptimized static assets.** It
   emitted `/momo-mark.png` while the bundle is mounted at `/ui`, so the logo
   404'd and rendered as a broken-image box. Switched to a plain `<img>` with a
   *relative* src, which resolves correctly under both mounts — `/ui/…` on the
   gateway, `/…` under Tauri — and removes the basePath coupling entirely. Third
   basePath trap of this project; see also WP-3 and the Tauri build.
2. **Deriving the icon from `source/mark.png` eats the dog's muzzle.** Its white
   background reaches the muzzle through the gap at the chin, so a border
   flood-fill takes both. `brand/make-assets.py` derives from the *circle* asset
   instead, whose navy ring encloses every interior white.

**Known limit:** the circle asset is 212×198, so the 1024 icon is upscaled and
soft at the largest sizes. Fine where icons are actually seen. A ≥1024px master
or an SVG is the one asset worth asking the designer for — noted in
`brand/README.md`.

### ✅ API keys in the UI, and the keychain actually works now · 2026-08-06

**B4 was misdiagnosed.** I had recorded it as "nothing calls `set_default_store`"
and fixed it by degrading the error. The real cause is that the project depends
on **`keyring-core`**, which is the abstract half of the crate and ships only
`mock` and `sample` stores — there is no OS backend in it at all. Swapped to
**`keyring` 3.x** (per-target features, so the Linux D-Bus backend is not a macOS
build dependency) and the keychain works for the first time.

**The shell owns the keychain; the gateway never touches it.** macOS attaches an
ACL to each keychain item listing the binaries allowed to read it, so an item
written by `momo-worker` and read by `momo-fetch` is a cross-binary read — macOS
prompts, and a spawned child with no UI either confuses the user or hangs. Only
the shell reads and writes; stored keys reach the gateway as **environment
variables injected at spawn**. Windows and Linux then behave identically instead
of each having their own story.

**That inverts precedence, deliberately.** The gateway resolves env → keychain
and `dotenvy` does not override existing env, so an injected key beats the
workspace `.env`. In a GUI the thing you typed into the app should be the thing
it uses. But it has to be *said*: `secret_status` reports `also_in_env_file` per
provider and the panel explains which one is in effect, because otherwise "I
changed my key and nothing happened" is unexplainable.

**Write-only by construction.** There is no `get_secret` command. The UI knows
"set" or "not set" and nothing more, so a key cannot be lifted back out through
anything that reaches the app. Saving restarts the gateway, because the harness
resolves secrets while building rather than per request.

**Verified end to end**, with the workspace `.env` moved aside so only the
keychain could work: app fails with no key anywhere → key stored → gateway
starts → `openrouter available=true` → a real turn streams `keychain-works`.
Environment restored and the test keychain item deleted afterwards.

*Note for testing:* store test items with `security add-generic-password -T
<binary>` or the cross-binary ACL prompt will hang a headless run. The unit test
uses a per-process account name for the same reason.

### ✅ Theming — dark / light / auto · 2026-08-06

Spec §6 and Q7 both asked for this; I had shipped dark-only. Now three-way,
defaulting to `auto`.

**Light is a re-derivation, not an inversion.** The identity has to survive: same
cool cast, and amber still meaning "the agent wants something from you". Straight
inversion breaks the second one — `#ffb454` on white is ~1.7:1 and reads as
decoration, the one job amber must not do. Every signal colour is darkened until
it clears 4.5:1 (light: signal 4.96, consent 5.54, halt 6.03).

**Measuring light found a bug in the dark palette I had already shipped:** `--dim`
was 4.28:1 — under AA for the secondary text it carries everywhere. Now 4.81,
moved along the same hue rather than desaturated.

Two mechanics worth remembering:

- **Tailwind v4 needs `@theme inline`.** Without `inline` the utilities bake in
  the literal at build time and flipping `data-theme` does nothing.
- **The theme must be stamped before first paint**, by an inline `<head>` script.
  The export is prerendered, so React state is too late and a light-mode user
  gets a dark flash on every launch. That script cannot import from
  `preferences.ts`, so the storage key exists twice — `preferences.test.ts`
  asserts they agree, because drift would silently disable the stored theme.

**🔴 Trap that cost real time here: `cargo tauri build` can ship stale frontend
assets.** After rebuilding, the app showed `light` selected while the stored
preference was `auto`, and the same build in a browser was correct. Tauri embeds
`web/out` into the binary at compile time, and with no Rust changes cargo skips
the rebuild — so the old assets stay embedded. `touch src/lib.rs` and rebuild
fixed it. **If a UI change is not showing up in the app, rule this out before
debugging the UI.** Documented in the install guide.

### ✅ WP-6 — polish (F21–F28) · 2026-08-05 · **Phase 1 complete**

Wave 4. Toasts, skeletons, responsive drawers, Shiki, file attach, empty states, sounds, preferences. Verified at 1600×1000, 900×800 and 420×780; no console errors.

**🔴 Mobile was broken and only a screenshot caught it.** The first pass reused the desktop booleans for narrow viewports, so `sidebarOpen` *and* `detailOpen` both defaulted true and both rendered as fixed overlays — two panels stacked on top of each other with the chat and composer completely buried underneath. Every automated probe passed: the composer was in the DOM, had non-zero size, and was queryable. It was simply invisible.

Fixed by giving mobile its own model: `mobileDrawer: "sessions" | "detail" | null`, always starting `null`, and opening one closes the other. Panels are *columns* on desktop where two can coexist; they are *drawers* on mobile where one cannot. Re-verified with `elementFromPoint` on the composer's centre — not just "is it in the DOM" but "is it the thing you would actually touch" — plus a count of visible panels after opening a drawer (exactly 1).

**F24 Shiki, deferred from WP-4, is now in.** The deferral reason shaped the implementation: one lazily-created singleton highlighter (the *promise* is the singleton, so concurrent callers during a stream share one init), a build-time language allow-list since a static export cannot fetch grammars at runtime, and — the important part — **highlighting is skipped entirely while `streaming` is true**. A growing code block would otherwise re-run Shiki against a slightly longer string on every token.

**F28 stores view state only — never server state.** Panel visibility and the sound toggle are this browser's business. The active session, agent, provider and permission mode are not: the harness holds one global set (§2.3, C4), so a remembered value is a belief that can be wrong the moment another tab or the REPL changes it. Those come from `/health` and the `role` event. Never the bearer token either. Preferences are applied in a post-mount `hydrate()` rather than at module scope, so the first client render matches the exported HTML.

**F27 sounds are off by default** and synthesised with WebAudio rather than shipped as files — three tones pitched by urgency (approval rises, done falls, error is low). The `AudioContext` is created lazily because constructing one before a user gesture is blocked by autoplay policy and warns on every load. `prefers-reduced-motion` also silences them: someone who asked the OS to calm down did not ask for beeps.

**F21 never auto-retries.** A retry action is only offered where the caller knows the request is an idempotent GET. Re-sending a turn double-bills and can re-run tools that already executed.

**React 19 lint, third occurrence:** the Shiki effect tripped `set-state-in-effect` too. The fix that works and reads well is storing the derived value *with* the input it came from (`{code, html}`) and comparing, rather than adding a second "is stale" flag — every `setState` then lives inside the async callback.

---

### ✅ Backlog clearance — security-review gate, cost parity, event_count · 2026-08-05

All three carried items closed.

#### 🔴 `security-review` on G6 — ran, found two, fixed both

**1. CORS `*` + auth off = RCE from any website the user visits. (HIGH)**

Not a G6 bug — a *pre-existing default* whose blast radius this project's own new endpoints made critical. `cors_origins` defaulted to `["*"]` → `CorsLayer::permissive()`, and `auth.enabled` defaults to `false`, so `auth_middleware` passes everything through. `Access-Control-Allow-Origin: *` means a hostile page doesn't just *send* requests, it **reads the replies**:

1. probe `/health` (auth-exempt by design) to find the gateway
2. `GET /v2/files/tree` + `/v2/files` → exfiltrate the working tree (simple GET, no preflight)
3. `POST /v2/settings/permission {"mode":"yolo"}`
4. `POST /v2/chat/stream` with a `shell_exec` prompt → **code execution**

Loopback is no defence: the browser is already inside the trust boundary.

**Fixed:** default is now `[]` (same-origin; `/ui` needs no entry), and the gateway **refuses to start** on `["*"]` with auth off — same fail-fast shape as the existing non-loopback bind refusal. Verified: default config serves no ACAO header cross-origin; `["*"]`+no-auth refuses with a readable message; an explicit origin list echoes only that origin.

**2. `.gitignore` layer only read the repo root. (MEDIUM)**

`gitignore_matcher` built from `root/.gitignore` alone, but git honours an ignore file in *every* directory. A monorepo's `services/api/.gitignore` excluding `config.local.yaml` was not applied, and the file was served — the deny-list only catches credential-*shaped* names. **Fixed:** the matcher now composes every `.gitignore` from root down to the target's parent, deepest last so it wins. Two regression tests.

#### ✅ Checked and sound (don't re-derive)

- **Symlink escape via the tree walk** — I expected one, since `walk()` filters entries without re-running `resolve_path`. There isn't: tokio's `DirEntry::metadata()` is `symlink_metadata` on Unix and explicitly does not traverse, so a symlink-to-dir reports `is_dir:false` and is never recursed into; following it later hits `resolve_path` → 403. **⚠️ This is load-bearing on a tokio detail — if that call ever becomes `fs::metadata`, the walk gains an escape.**
- Traversal, 403-vs-404 oracle, G9 static serving, the `thread_local!`→global refactor (sub-agents get the *same* `Arc`, so no boundary is crossed), token-in-memory, React XSS — all verified sound.

#### ✅ Cost parity (G10 acceptance) — proven, and a third path was missing entirely

The zai key made this measurable for the first time (a free model reads 0 on both sides, so it proves nothing). Same prompt, paid model, `cost.json` deltas:

| Path | prompt | completion | recorded | matches formula |
|---|---:|---:|---:|---|
| REPL | 8964 | 16 | 0.045060 | ✅ |
| Gateway | 8964 | 3 | 0.044865 | ✅ |

**Identical prompt accounting; the difference is purely completion length.** Parity holds.

**But `momo-fetch -p` recorded nothing at all** — 329 → 329 records across two paid turns. `oneshot.rs` never called the cost lifecycle, so every scripted invocation spent real money invisibly and `/cost` under-reported by all of it. The spec's G10 says "REPL and gateway" and missed that there are **three** callers. Now wired to the same three shared helpers. Verified: `-p` records, formula matches.

*(Aside: `zai` has no pricing entry, so it falls to the generic `$5/$15` per-million fallback. The parity result is unaffected — both paths use the same function — but the absolute figure for zai is a placeholder, not real pricing.)*

#### ✅ `GET /v1/sessions` → `event_count: 0`

Root cause was not the handler. `SqliteSessionService::list` builds each row with `events: Vec::new()` — it is a metadata-only query by design — so `s.events().len()` is *always* 0. Loading events per row would be N+1 (144 sessions here). `SessionInfo::event_count` is now `Option<usize>`, `None` from the list, and the UI renders nothing rather than a confident `0`. Same principle as `tool_count` in G4: **never report a number we don't have.**

---

### ✅ WP-5 — management panels (F12–F20) · 2026-08-05

Wave 3. Nine panels: agents, models, MCP status, memory, files, settings, cost, connection, shortcuts. Verified live against the release gateway with a screenshot at 1600×1000; no console errors.

**Contractual renderings that are easy to get wrong and were checked on screen:**
- **`tool_count: null` renders "—", never "0"** (G4). Confirmed: all four MCP servers show `—` with `4/4` running.
- **Unavailable providers are listed, greyed, *with the reason*** (G3) — not omitted. "Where did Anthropic go?" is worse than "Anthropic: no key".
- Every status dot is paired with a word or a count. No colour-only meaning.

**Bug found and fixed on screen: ollama said "no key".** Ollama needs no API key — it is unavailable when nothing is listening on its port. The generic "set `<PROVIDER>_API_KEY`" copy would send the user hunting for an `OLLAMA_API_KEY` that does not exist. It now says "offline / Not running. Start Ollama, or set `OLLAMA_HOST`."

**Naming bug caught by eslint, worth remembering:** the API client exported `useDefaultAgent`, which `react-hooks/rules-of-hooks` correctly flagged as a hook called outside a component. Renamed to `resetToDefaultAgent`. **Do not prefix plain API functions with `use`.**

**New shared pieces** (use these rather than re-rolling): `hooks/use-gateway-resource.ts` for fetch-once-plus-reload, and `components/shared/panel-section.tsx` for section chrome and the status `Dot`. The hook exists partly to get React 19's `set-state-in-effect` pattern right in one place instead of eight.

**`glm-5-turbo` on z.ai now works** — a `ZAI_API_KEY` appeared in `.env`, `POST /v2/switch {"provider":"zai","model":"glm-5-turbo"}` succeeded and the turn streamed normally. This closes the question left open in §5. ⚠️ That key was accidentally echoed into a terminal transcript during this session; **rotate it**.

**F20 shortcuts:** `Esc` interrupts a running turn (G13), `Ctrl/Cmd+B` and `Ctrl/Cmd+J` toggle the panels. Escape is deliberately **not** bound while the approval dialog is open — the dialog owns Escape as "deny", and stealing it would make the safe action unreachable by keyboard.

---

### ✅ WP-4 — chat is usable (F4, F6–F11, F29) · 2026-08-05

Wave 2. The browser can now drive a full turn: type → stream → tool call → approval → result.

**Verified by driving the real UI with Playwright** against the *release* gateway in strict mode with `approved_tools` cleared: prompt → approval dialog appears with the right title and sticky copy, Deny holds initial focus → Approve → **exactly one** green `shell_exec done` card, no duplicate, no stuck spinner, status bar returns to `idle`.

**Two harness behaviours the UI has to encode, or it renders lies:**

1. **`call_id` does not survive an approval** (§5). The follow-up leg's `tool_call_start` carries a *different* id, so keying cards purely on id leaves the pre-approval card spinning forever *and* draws a second card for the same user-approved action. `onApprovalResolved` retires the awaiting card instead — the follow-up leg's own start/result pair tells the whole story. This is the single most likely thing for a future refactor to break; the Playwright check above is what catches it.

2. **`usage` is per-leg, `cost_usd` is a running session total.** Token counts accumulate across legs; cost is *replaced*, not summed. Summing cost across legs of one turn inflates it by roughly the number of legs.

**F9 copy is a security requirement, not UX polish.** It states that approving inserts the tool *name* into a process-wide set (one Approve on `shell_exec` = never asked again for the life of the process), and that denial is **not** remembered because `run_confirmation_turn(name, false)` records nothing. Do not soften either sentence — both are literally what the code does. Deny takes initial focus so a stray Enter picks the safe action; Escape denies.

**Deferred deliberately:** Shiki (spec §3.2) is not wired. It wants one reused highlighter instance, and re-highlighting a growing code block on every streamed chunk is the wrong shape. Blocks are styled and copyable; F24 can add highlighting without changing the component's interface.

**Known wart, not fixed:** after an approval the assistant message still contains the pre-approval leg's synthetic text — *"Tool confirmation required for 'shell_exec'. Provide approve/deny decision to continue."* — which reads as stale once the tool has run. It is emitted by the harness as a real `text` event, so the honest fix is server-side (don't forward it) rather than a client-side match on that exact string. Visible in the WP-4 screenshot.

**Lint note for future components:** React 19's `react-hooks/set-state-in-effect` rejects calling an extracted `useCallback` fetcher straight from an effect. Use the inline `let cancelled = false` async pattern (see `header.tsx` and `session-list.tsx`) — same behaviour, and it is the shape the rule wants.

---

### ✅ WP-3 — frontend foundation (F1, F2, F3, F5) · 2026-08-05

Wave 1, lane 3. First frontend code in the project — `web/` did not exist before this.

| Task | Landed | Notes |
|---|---|---|
| **F1** | `web/` scaffold | Next **16.3** + React 19.2 + Tailwind v4 + zustand, `output:'export'` → `web/out` |
| **F2** | `lib/types.ts`, `lib/api-client.ts` | full type surface + URL resolution + §8 error parsing |
| **F3** | `lib/sse-parser.ts` | `fetch` + `ReadableStream`, hand-rolled frames, **22 vitest cases** |
| **F5** | `components/layout/*` | 3-panel shell, collapsible, dark-first |

**Spec deviation:** §3.2 specifies Next 15; `create-next-app@latest` installs **16.3**. Kept 16 — it is the current line and nothing in the spec depends on 15. Note `web/AGENTS.md` (written by `next dev`) warns that Next 16 has breaking changes and to read `node_modules/next/dist/docs/` before writing config; that is how `basePath` below was confirmed.

**🔴 Two integration bugs that only appear when you actually serve the build through G9.** Both are the kind that a unit test never catches:

1. **Completely unstyled page.** Next emits **absolute** asset URLs, so a bundle mounted at `/ui` links `/_next/static/…` — not under `/ui`, so the browser 404s every stylesheet and script. Fixed with **`basePath: '/ui'`** in `next.config.ts`. This is easy to mis-diagnose as a Tailwind problem: fetching those same assets by hand *with* a `/ui` prefix returns 200, which is exactly what I did first and it told me nothing was wrong. **`basePath` must match G9's mount point**, and it is inlined at build time — changing one without the other silently breaks asset loading.

2. **"offline" on a page the gateway itself served.** `NEXT_PUBLIC_GATEWAY_URL` is baked in at build time, but the gateway binds an ephemeral port under `--gateway-port 0`, so the value is guaranteed wrong. `gatewayUrl()` now prefers `window.location.origin` when the path starts with `/ui`. Side benefit: those calls become same-origin, so the permissive CORS default stops mattering on the served-by-gateway path. This step is **not** in spec §3.2's resolution chain — it was added from testing and the spec should absorb it.

**Verified end to end:** `npm run build` → `web/out/` → gateway serves `/ui` with correct content types → SPA fallback works on `/ui/anything/deep` → header shows **`● connected`** and the live model name read back from the running harness. Screenshotted at 1440×900.

**Design direction** (F5, for whoever does F6–F9 and must stay consistent): *instrument panel, not terminal cosplay.* Two rules carry it —
- **Amber (`--color-signal`) is a state, never decoration.** It means the agent is working or wants something from you. If amber is on screen, the user has something to attend to. Do not use it for emphasis, links, or branding.
- **Monospace is machine speech** (tool names, paths, session ids, token counts); **sans is the interface talking to you.** They never swap.

The signature element is the **turn rail** — the 3px strip on the chat panel's leading edge. Idle it is dark; running, an amber segment travels; awaiting approval, the whole column pulses. It exists because the turn lifecycle is this product's core and is otherwise invisible. Status is never colour-alone: every dot is paired with a word.

Placeholders say what is missing and which task fills it (`F10`, `F14`, …) rather than faking content — a shell with mock messages hides exactly the integration problems above.

---

### ✅ G9 — static UI serving · 2026-08-05 · **Gateway is now complete**

`GET /ui/*` from `ServeDir`, with `ServeFile(index.html)` as the fallback so client-side routes survive a refresh. Path comes from `gateway.json` → `ui_dir`, defaulting to **`web/out`** (Next.js static export writes `out/`, not `dist/` — spec C12). Relative values resolve against the project root. Needed `tower-http`'s `fs` feature, now enabled.

**Auth-exempt, and for a stronger reason than `/health`:** a browser navigating to a page cannot attach an `Authorization` header, so an HTML shell behind a bearer token is unreachable by construction. Only the static bundle is exposed; the token still guards every `/v1` and `/v2` call the loaded app makes.

**Absent `web/out` is not an error.** The gateway is useful headless and the frontend may simply not be built yet, so the route is skipped and a one-line hint is logged. Verified both ways: without the directory `/ui/` → 404 and `/health` → 200; with it, `/ui/`, `/ui/index.html` and `/ui/_next/app.css` all → 200.

**Traversal check.** `/ui/../.env`, `/ui/../../.env`, `/ui/%2e%2e/%2e%2e/.env` and `/ui/..%2f.env` all return 200 — but the body is `index.html`, not `.env`. `ServeDir` rejects the traversal and the SPA fallback answers. Confirmed by grepping the response for `API_KEY`/`sk-or-`: no match on any of them. **Do not "fix" that 200 into a 404 without re-checking the body** — it is the fallback behaving correctly, and the same 200 is what makes `/ui/settings/deep` work.

---

### ✅ WP-2 — G6 sandboxed file access · 2026-08-05

Wave 1, lane 2. Landed in `src/gateway/files.rs` (own module — this is the surface a security review should read first).

**🔴 The spec's security assumption was wrong, and it would have leaked the API key.**

Spec G6 says to "filter with `is_ignored` so `.gitignore`d files and secrets don't leak into the browser". `is_ignored` consults **`.agentignore` only** (`src/sandbox/mod.rs:170`). This repo has **no `.agentignore`**, so `ignore_matcher` is `None` and *every* in-root path reports as not-ignored. `.env` is gitignored but not agentignored — so as specified, `GET /v2/files?path=.env` would have returned `OPENROUTER_API_KEY` in plaintext. CORS is permissive by default, so any page the user merely visited could have fetched it.

**Fixed with four layers, in this order:**

| # | Layer | Catches |
|---|---|---|
| 1 | `resolve_path` | `..`, symlink escape, absolute paths (`Path::join` discards the root, so the trailing prefix re-check is what saves it) |
| 2 | `is_ignored` | `.agentignore` |
| 3 | `gitignored()` **new** | `.gitignore` — the layer the spec assumed layer 2 provided |
| 4 | `is_sensitive()` **new** | unconditional deny-list: `.env*`, `*.pem/key/p12/pfx/jks`, `id_rsa`, `.ssh`, `.git`, `.netrc`, `credentials`, `secrets.*` |

Layer 4 exists because layer 3 is only as good as the user's `.gitignore`, and a project that never gitignored its keys is exactly the project whose keys most need protecting. It is deliberately **not configurable**.

**Verified live — every one of these returns 403:** `.env` · `.env.local` · `../../etc/passwd` · `/etc/passwd` · `.git/config` · `../.env` · `src/../.env` · `.ssh/id_rsa` · `server.pem` · `target/debug/momo-fetch` (gitignored). The tree walk applies the same rules — `.` at depth 1 lists 18 entries with `.env`, `.git` and `target/` all absent.

Other acceptance criteria: a 3 MB file → capped at exactly 1 MiB with `truncated:true` and no OOM (the cap is applied *while reading*, via `File::take`, not after); a binary file → `binary:true` with `content:null` rather than lossy-decoded garbage; a directory via `/v2/files` → 400; a missing but permitted in-sandbox path → 404.

**403 vs 404 is load-bearing.** Everything denied returns 403, including paths that do not exist but would be denied. 404 is reserved for in-sandbox, permitted, genuinely absent. Splitting those would make the endpoint an oracle for mapping the filesystem.

**Still owed:** the `security-review` skill has *not* been run on this yet — spec §12.7 lists it as a blocking gate for WP-2. Everything above is my own testing. Run it before treating G6 as shippable.

---

### ✅ WP-1 — Gateway read endpoints (G4, G5, G8) · 2026-08-05

Wave 1, lane 1. All three landed, verified live against a running gateway.

| Task | Endpoint | Verified |
|---|---|---|
| **G4** | `GET /v2/mcp/servers` | 4 servers listed (3 http + 1 stdio) with transport and status |
| **G5** | `GET /v2/memory/search`, `/v2/memory/stats` | search hits a real vault (22 memcells); `limit=1000` → clamped to 100; empty `q` → 400 |
| **G8** | `GET /v2/sessions/{id}/messages` | 21 events → 5 tool calls, paired by call id, previews populated; missing session → 404 |

Deadlock canary still green: `GET /v2/agents` → 200 in 0.36 ms.

**Two problems found and fixed while building — both would have shipped a lying UI:**

1. **`running` under-reported.** `McpService::running_count()` only knows about the stdio manager, so with 3 HTTP servers connected it returned `1` while the `servers` array showed 4 × `running` — F14 would have printed "1 running" beside four green dots. `running` is now derived from the reported array; the manager's figure is kept as `running_stdio` rather than discarded.

2. **`pending` tool calls that never resolve.** The pre-approval `FunctionCall` is abandoned when `run_confirmation_turn` starts a fresh turn, and the post-approval call gets a **different id** (see §5, "`call_id` does not survive an approval"). On replay that left a card marked `pending` forever — F11 would spin on it on every reload. Unpaired calls are now marked **`unresolved`**, which is the honest state for history: nothing is pending in a finished session. Confirmed on the real approval-test session — exactly one `unresolved`, the rest `done`.

**Also noticed, not fixed (out of WP-1 scope):** `GET /v1/sessions` reports `event_count: 0` for every session, while `GET /v1/sessions/{id}` on the same id correctly reports 21. Pre-existing bug in the list handler's projection, not in G8. Worth a task if F10 wants to show event counts in the session list.

**Notes for whoever does F14:** `tool_count` is `null` for *both* transports right now. The scratchpad §4.7 revision says HTTP servers are attributable in principle — they are, but `Toolset::tools()` needs an `Arc<dyn ReadonlyContext>` and is async, which is not worth plumbing into a status endpoint. The response shape is unchanged and the UI contract stands: render `null` as **"—", never "0"**.

---

## 1. Context

The task is a web UI + desktop app for momo-fetch, built as: Next.js frontend → existing axum gateway → Rust harness. Two things happened in the originating session:

1. **The spec was audited against the code and rewritten.** The original draft assumed behaviour the harness does not have — the corrections are §0 of the spec, table C1–C12. Anyone continuing this work must read that table; several "obvious" implementations are wrong for this codebase.
2. **The Sprint-1 P0 gateway slab was implemented.** Rust only. No frontend code exists yet.

**The gateway work is committed** as `546ef04` *"✨ feat(gateway): add V2 API with tool approval, turn guard and cost tracking"*, and the version has moved to **0.9.1**. Earlier revisions of this document described `src/gateway/` as untracked WIP — that is no longer true.

### Uncommitted work in the tree (not part of the gateway slab)

`git status` shows `M src/mcp/mod.rs` — an unrelated in-progress change that adds `PrefixedToolset` wrapping so MCP tools can't collide with built-in tool names. It matters here for one reason: **it partially invalidates §4.7 below**, which is a load-bearing constraint for G4. See that section.

Also modified and unrelated: `Cargo.toml`, `Cargo.lock`, `docs/user-guide.md`. Also untracked and unrelated: `.harness/skills/`, `.pi/`, `AGENTS.md`, `docs/skills/`, `src/momo-gateway/` (a shell script, not a Rust module).

---

## 2. What is DONE

All in Rust, all compiling, all covered by unit tests where testable without a live LLM.

| Task | What landed | Where |
|---|---|---|
| **G10** Cost lifecycle | `begin_turn` / `record_usage` / `end_turn` / `context_usage` as the single definition; REPL rerouted through them; v1 handlers wired too | `src/harness.rs:440,446,451,456`; `src/cli/repl.rs` |
| **G1** Rich SSE stream | `POST /v2/chat/stream` — typed events, part mapping, 4-phase approval stitch, disconnect cleanup, provider timeout | `src/gateway/v2_handlers.rs` |
| — Turn guard + approvals | RAII lease, `(turn_id, tool_name)` approval keying, stale `call_id` rejection, `deny_all` for interrupt | `src/gateway/turn.rs` (7 unit tests) |
| **G2** Agents | `GET /v2/agents`, `POST /v2/agents/switch`, `POST /v2/agents/default` | `v2_handlers.rs` |
| **G3** Providers | `GET /v2/providers`, `POST /v2/switch-model` · `switch-provider` · `switch`; added `ProviderManager::list_all()` so unconfigured providers report `available:false` instead of being omitted | `v2_handlers.rs`; `src/providers.rs:334,456` |
| **G7** Settings | `GET /v2/settings`, `POST /v2/settings/permission`, `DELETE /v2/settings/approved-tools` | `v2_handlers.rs` |
| **G13** Interrupt | `POST /v2/chat/interrupt` — marks interrupted, denies parked approvals, calls `Harness::interrupt()` | `v2_handlers.rs` |
| **G11** Bind/port flags | `--gateway-port` (0 = OS-assigned), `--gateway-bind`, `MOMO_GATEWAY_LISTENING <url>` stdout line | `src/cli/mod.rs`; `src/gateway/mod.rs` |
| **G12** Health | `/health` auth-exempt via router split; readiness payload with `turn_active`, `turn_id`, provider/model/session/permission | `src/gateway/handlers.rs`; `mod.rs` |
| — V2 types + error model | Event enum with `name()`/`data()`, uniform `{"error":{code,message,details}}` via `v2_error()` | `src/gateway/v2_types.rs` |

### Two bugs found and fixed en route (not in the spec's task list)

1. **`GatewayConfig` derived `Default`.** With no `.harness/gateway.json`, `unwrap_or_default()` produced `port: 0` (random OS port) and `cors_origins: []` (empty allow-list → every browser blocked). Derived `Default` zeroes fields; serde `#[serde(default = …)]` only applies when the file exists. Now a manual `impl Default` mirrors the serde defaults. `src/gateway/mod.rs`.
2. **v1 chat handlers had no turn guard.** They take the write lock to resume a session. Against an in-flight v2 turn that queues a writer on the `RwLock`, and because it is write-preferring, every subsequent reader blocks behind it. Both v1 handlers now take the same `TurnLease`. `src/gateway/handlers.rs`.

Also added, from spec §9: the gateway now **refuses to start** on a non-loopback bind when auth is disabled, rather than silently exposing shell execution to the network.

---

## 3. What is NOT done

**Gateway (Sprint 1–2 remainder):**
- **G4** `GET /v2/mcp/servers` — MCP status
- **G5** `GET /v2/memory/search`, `/v2/memory/stats`
- **G6** `GET /v2/files`, `/v2/files/tree` — sandboxed read
- **G8** `GET /v2/sessions/{id}/messages` — session messages with tool calls
- **G9** `GET /ui/*` — static serving

**Frontend:** nothing. F1–F29 all open. No `web/` directory exists.

**Phase 2 (Tauri):** nothing. T1–T13 all open.

> **Superseded ordering advice.** This section used to say: finish G4 → G5 → G6 → G8 first, *then* scaffold the frontend, because building the UI against a half-finished API means reworking `lib/types.ts` twice.
>
> **Spec §12.3 reverses this.** That reasoning was correct while the API surface was still moving; spec §5 and §7 have since frozen it, so `types.ts` is fully derivable today without a single endpoint existing. The new plan lands `501 not_implemented` route stubs first (R1), then runs gateway and frontend **in parallel** — which takes ~4 days off the critical path. See spec §12.3 for the contention map and §12.5 for the wave schedule.

---

## 4. Hard-won knowledge — read before writing code

These cost real investigation time. Getting them wrong produces code that compiles and deadlocks.

### 4.1 The approval flow is not a pause/resume
adk **ends** the `EventStream` and sets `event.actions.tool_confirmation`. There is nothing to resume. The decision is applied by `Harness::run_confirmation_turn(tool_name, approved)` (`src/harness.rs:405`), which calls `rebuild_runner()` → needs `&mut self` → **write lock** → and starts a *brand new turn* whose user content is the literal string `"approved"` or `"denied"`.

The gateway hides this seam: one SSE response, four phases — read lock (stream leg) → **no lock** (human wait) → write lock (confirmation turn) → read lock (next leg). See `run_v2_turn` in `v2_handlers.rs`.

### 4.2 Approval is sticky, by tool NAME
`approved_tools: HashSet<String>` (`src/harness.rs:38`) is per-process and baked into `RunConfig::tool_confirmation_decisions`, which adk keys by tool name. **One "Approve" on `shell_exec` disarms confirmation for that tool for the rest of the process.** The `approval_required` event carries `sticky: true` and the UI is required to disclose this (spec F9). `GET /v2/settings` exposes the set; `DELETE /v2/settings/approved-tools` clears it.

Denial is **not** recorded — the model can re-request immediately. Spec Q10 tracks whether to add a `denied_tools` set.

### 4.3 Lock discipline is not optional
`tokio::sync::RwLock` is write-preferring: a waiting writer blocks new readers. Rules that are already implemented and must be preserved:
- Never hold any guard across the approval wait (up to 5 min).
- All mutating `/v2` endpoints call `require_idle()` and **409 before** taking the write lock — so a writer never queues behind a turn.
- The vault is behind a **`std::sync::Mutex`** (`src/harness.rs:28`). Never lock it across an `await` — copy results out or use `spawn_blocking`. This matters for **G5**.

### 4.4 Turn concurrency is process-wide
`Harness` has one `current_session_id`, one runner, one provider. There is no per-session isolation. `TurnRegistry` admits one turn at a time globally; a second returns 409 `turn_in_progress`. Do not "fix" this by making it per-session — the underlying state is shared.

### 4.5 Disconnect handling
The turn runs in a spawned task feeding an mpsc channel; the SSE body owns the receiver. Client disconnects → receiver dropped → next `send` fails → task interrupts the runner and returns → `TurnLease` drops → harness released. This is why the channel exists rather than inlining into `async_stream!`. It also lets helper fns emit events (a `stream!` macro body can only `yield` inline).

### 4.6 adk API shapes (saves a registry dig)
Source: `~/.cargo/registry/src/*/adk-core-0.7.0/src/`
- `Part::Text { text }` · `Part::FunctionCall { name, args, id: Option<String>, thought_signature }` · `Part::FunctionResponse { function_response: FunctionResponseData, id: Option<String> }`; `FunctionResponseData { name, response: Value, .. }` — `types.rs:178+`
- `ToolConfirmationRequest { tool_name, function_call_id: Option<String>, args: Value }` — `context.rs:594`
- `UsageMetadata { prompt_token_count, candidates_token_count, total_token_count, .. }` (all `i32`) — `model.rs:148`
- `event.llm_response.usage_metadata` · `event.actions.tool_confirmation` · `event.llm_response.error_message` · `event.is_final_response()`

### 4.7 MCP tool counts are attributable for HTTP servers, not stdio — *revised 2026-08-05*

**This changed under the uncommitted `src/mcp/mod.rs` edit (see §1).** The original claim — "no per-server attribution at all" — is now only half true, and G4 should be built against the new shape:

- **HTTP servers: countable.** `http_toolsets` became `Vec<(String, Arc<dyn Toolset>)>` (was `Vec<Arc<dyn Toolset>>`), each wrapped in `PrefixedToolset::new(ts, server_id)`. The server id is retained, so a real `tool_count` is available per server.
- **stdio servers: still not countable.** All stdio servers go through one `McpServerManager` wrapped in a *single* `PrefixedToolset::new(manager, "mcp")` — one shared prefix, no per-server split. Still `null`.

**Caveat before relying on this:** `Toolset::tools()` takes `Arc<dyn ReadonlyContext>` and is async (`adk-tool-0.7.0/src/toolset/compose.rs:159`), so counting is not a free field read — it needs a context and an `.await`. If plumbing a context into the G4 handler proves ugly, report `null` for everything and keep the endpoint shape.

Either way the **response shape does not change** and neither does the UI contract: `tool_count` stays nullable, and F14 must render null as **"—", never "0"**. Decide per-server-count-vs-null as an implementation detail inside G4; do not let it change `/v2/mcp/servers`.

⚠️ If the `src/mcp/mod.rs` change is reverted or lands differently, re-check this section before building G4.

### 4.8 Memory search: use the right function
`MemorySidecar::search_for_context` is enrichment-shaped (input → prompt injection, with its own relevance gate). General search for **G5** is `ObsidianVault::search(&MemoryQuery)` (`src/memory/vault.rs:321`), plus `stats()` / `counters()`.

---

## 5. Environment blocker — live verification is incomplete

## 5. ~~Environment blocker~~ — RESOLVED 2026-08-05

> **This section previously said no LLM turn could complete. That is no longer true.**
> A working free model was found. **Everything in the "NOT verified" list below has now been verified end-to-end**, including the approval round-trip.

### The working configuration

```
provider: openrouter
model:    nvidia/nemotron-3-ultra-550b-a55b:free      ← free tier, tool-capable, 1M context
```

Verified: streaming text, **tool calling**, **the full approval handshake**, stickiness, interrupt-free completion. This model is the reference config for all remaining verification work.

### What was actually wrong (for the record)

| # | Finding | Status |
|---|---|---|
| 1 | Default `google/gemma-4-26b-a4b-it:free` → `model.invalid_input` | still broken — **it is a bogus slug**, and it is still the default in `.harness/settings.json` (both `default_model` and `memory.sidecar_model`) |
| 2 | OpenRouter account has no credits → `model.forbidden: Insufficient credits` | still true — reconfirmed against paid `deepseek/deepseek-chat-v3-0324` |
| 3 | OpenRouter retired the `:free` tier on the *obvious* fallbacks | true for `deepseek-chat-v3-0324:free`, `llama-3.3-70b-instruct:free`, `qwen-2.5-72b-instruct:free` — but **not universal**; nemotron's free tier is live |

The whole blocker was **a bad default model plus an unlucky choice of fallbacks**, not an account problem. Credits are still zero and are no longer needed.

### Where API keys actually come from — not the keychain

`SecretStore::get` (`src/config/secrets.rs:58`) tries the env var first, then the OS keychain. In practice **only the env-var path works**:

- `dotenvy::dotenv()` at `src/cli/mod.rs:68` loads **`.env`** at the repo root, which currently defines `OPENROUTER_API_KEY` and nothing else. That is the sole reason openrouter is the only available provider.
- **The keychain path is entirely non-functional.** `keyring-core` 1.0 requires a concrete store to be registered at startup, and nothing in the codebase ever calls `set_default_store` — so every `Entry::new` fails with *"No default store has been set"*. `SecretStore::set`/`delete` cannot work either.

**To add a provider, put its key in `.env`** (`ZAI_API_KEY`, `GROQ_API_KEY`, …). Do not expect `secrets set` to work until a keyring store is registered.

### z.ai / `glm-5-turbo` — untested, no key

`POST /v2/switch {"provider":"zai","model":"glm-5-turbo"}` → `503 provider_unavailable`, `Keychain error: … No default store has been set`. That is the missing-key path above, **not** a statement about the model. The switch was correctly rejected and harness state was left unchanged (the in-flight stream continued on the previous model). Add `ZAI_API_KEY` to `.env` to evaluate it.

### Bugs found while verifying — **all fixed 2026-08-05**

> Summary of the fixes, then the original findings for context. All verified live; `cargo test --bin momo-fetch` → **297 passed**.
>
> | ID | Fix | Verified |
> |---|---|---|
> | **B0** | `.harness/settings.json` → `nvidia/nemotron-3-ultra-550b-a55b:free` (both `default_model` and `memory.sidecar_model`) | gateway starts on a working model |
> | **B1** | `ollama_reachable()` — 150 ms TCP probe, honours `OLLAMA_HOST` (`src/providers.rs`) | `/v2/providers` → `ollama available:false` |
> | **B2** | `get_pricing` returns zero for any model slug ending `:free` (`src/cost.rs`) | `/v1/cost` → `total_cost: 0.0` over 30k tokens |
> | **B3** | all 5 tool contexts moved `thread_local!` → process-global `RwLock` | 0 occurrences of `not initialized` across a full approval turn |
> | **B4** | `SecretStore::get` maps a missing keyring store to `NotFound` (`src/config/secrets.rs`) | zai switch → *"Set ZAI_API_KEY or use /key set zai"* |
>
> **B3 turned out to be much bigger than the symptom suggested — read finding 3 below.**

### Original findings

1. **`ollama` availability is a false positive.** `src/providers.rs:343` — `available: *provider == "ollama" || SecretStore::get(provider).is_ok()`. Ollama is hard-coded available because it needs no key, with **no liveness probe**; `/v2/providers` reports `ollama available:true` while nothing listens on `:11434`. Misleads **F13**, which greys out unavailable providers *and explains why*. Filed as **B1** in spec §12.1.
2. **`:free` models accrue nonzero cost.** The nemotron `:free` model reported `cost_usd` climbing to `0.596` over 118k tokens, and `GET /v1/cost` agrees. The pricing table does not understand the `:free` suffix. This directly corrupts **G10**'s acceptance criterion and **F18**'s cost badge. Filed as **B2**.
3. **`shell tool sandbox not initialized`** — first seen as an intermittent failure on the turn following `run_confirmation_turn`, so it was initially filed as an approval-path bug. **That diagnosis was wrong, and the real one is worse.**

   **Root cause: thread affinity.** `file`, `shell`, `search`, `kms` and `memory` each stored their sandbox/vault in a **`thread_local!`**. `build_tool_registry` (`src/tools/mod.rs:37-40`) sets it on whichever thread builds the registry — but tools *execute* later on an arbitrary tokio worker thread, where the thread-local is unset. So **any** tool call could fail with `"… not initialized"` depending purely on which worker picked up the task. `rebuild_runner` only changed the timing enough to make it visible; `file_read`, `grep`, and every `mem_*` tool were equally exposed. This had nothing to do with approvals.

   **Fix:** all five contexts are now a process-global `RwLock<Option<Arc<…>>>`. That is the correct shape here — the harness has exactly one sandbox and one vault by construction (§4.4) — and `get_*` clones the `Arc` out, so no guard is held across an `await` (§4.3 still holds).

   **Watch out when writing tool tests:** the thread-locals were also providing test isolation, and 15 tests began clobbering each other the moment the context went global. Tests that install a sandbox or vault must now take `crate::tools::test_support::sandbox_guard()` and hold it for the body of the test.

**No credits were spent** — the working model is free tier, and every paid probe failed before billing. All model and permission switches were runtime-only (`POST /v2/switch-model`, `/v2/settings/permission`); **`.harness/settings.json` was not modified**, so its broken default model is still there (see finding 1).

### Verified live
Port-0 bind + listening line · `/health` · `/v2/agents` · `/v2/settings` · `/v2/providers` · model + permission switching · error model (400 bad permission mode, 409 `stale_approval`, 409 `turn_in_progress`) · SSE framing, event names and ordering (`role` → … → `context_usage` → `done`) · error path surfaces a typed `error` + `done` where v1 just stops mid-stream · **6 concurrent turns → 1 admitted, 5 rejected 409, lease released after**.

### Verified live — 2026-08-05 session (the previously blocked items)

All on `nvidia/nemotron-3-ultra-550b-a55b:free`.

| Check | Result |
|---|---|
| `tool_call_start`/`tool_call_result` part mapping | ✅ `file_read` — `{id,name,args}` out, result paired on the **same `id`**, `output_preview` + `truncated` populated |
| **Approval handshake round-trip** | ✅ full 4-phase stitch, single unbroken SSE response — see transcript below |
| **Deadlock canary ([§2.2](#43-lock-discipline-is-not-optional))** | ✅ `GET /v2/agents` while an approval was parked → **200 in 0.4 ms**. The lock discipline is correct. |
| Sticky approval (**C2**) | ✅ after approving once, `GET /v2/settings` → `approved_tools:["shell_exec"]`, and a **second** `shell_exec` in strict mode produced **zero** `approval_required` events |
| Keep-alive comment frames | ✅ a bare `:` line appears mid-stream — **confirms the F3 parser must handle comment lines**, this is not theoretical |
| Event ordering guarantees | ✅ `role` first, `done` last, `tool_call_result` after its `tool_call_start`, `usage`/`context_usage` before `done` |
| Cost parity REPL vs gateway (**G10**) | ⚠️ **still open** — gateway side records correctly, but see bug **B2**: `:free` models are billed as paid, so the number itself is wrong on both sides |

Approval transcript (abridged, one response):

```
role → usage → tool_call_start{shell_exec} → text("Tool confirmation required…")
     → approval_required{sticky:true, destructive:false, expires_at:…}
     ⏸  [POST /v2/chat/approve → {"resolved":true,"approved":true,"sticky":true}]
     → approval_resolved{approved:true, reason:"user"}
     → :                                    ← keep-alive
     → usage → tool_call_start → tool_call_result{stdout:"hello-from-approval-test\n", exit_code:0}
     → text×5 → usage → context_usage → done{stop_reason:"complete"}
```

Note the follow-up leg emits a **new `call_id`** for the same logical call — the pre-approval `tool_call_start` and the post-approval one do **not** share an id, because it is genuinely a new turn (C1). **F8/F9 must correlate on tool *name* plus ordering, not on `call_id`**, or the UI will render two separate tool cards for one user-approved action.

Unit tests cover the registry semantics, preview truncation on char boundaries, error detection, and event naming; the wire path above is now covered by manual probe.

---

## 6. Reproducing the test setup

```bash
# Start on an OS-assigned port; parse the URL from stdout
cargo run -- --gateway --gateway-port 0
# → MOMO_GATEWAY_LISTENING http://127.0.0.1:51686

U=http://127.0.0.1:51686
curl -s $U/health                  # no auth required (G12)
curl -s $U/v2/agents
curl -s $U/v2/providers
curl -s $U/v2/settings

# Error model
curl -s -X POST $U/v2/settings/permission -H 'content-type: application/json' -d '{"mode":"banana"}'   # 400
curl -s -X POST $U/v2/chat/approve -H 'content-type: application/json' \
     -d '{"turn_id":"t_nope","tool_name":"shell_exec","call_id":"c1"}'                                 # 409 stale_approval

# Turn guard: fire several concurrently, expect exactly one admitted
for i in 1 2 3 4 5 6; do (curl -sN -X POST $U/v2/chat/stream -H 'content-type: application/json' \
  -d '{"messages":[{"role":"user","content":"hi"}]}' > /tmp/race_$i.txt &); done

# Live turn (blocked until a working model is configured)
curl -N -X POST $U/v2/chat/stream -H 'content-type: application/json' \
  -d '{"messages":[{"role":"user","content":"read Cargo.toml"}]}'
```

Note: `timeout` is not available on this macOS shell — use `curl --max-time`.

---

## 7. Artifacts

All of the below landed in **`546ef04`**.

- [`docs/spec/momo-worker.md`](./momo-worker.md) — the spec. §0 audit table, §2.2 lock discipline, §2.4 corrected approval flow, §5 event protocol, §8 error model, §9 security, §10 testing, **§12 execution plan**.
- `src/gateway/turn.rs` — **new**, turn guard + approval broker (7 tests)
- `src/gateway/v2_handlers.rs` — **new**, all `/v2` handlers (17 fns, 4 tests)
- `src/gateway/v2_types.rs` — **new**, V2 request/event types + `v2_error()`
- `src/gateway/mod.rs` — modified: state, config, routes (`:184-206`), router split (`:213`), bind overrides
- `src/gateway/handlers.rs` — modified: turn lease on v1, cost wiring, readiness payload
- `src/harness.rs` — modified: turn lifecycle (`:440-456`) + approved-tools management
- `src/providers.rs` — modified: `list_all()` (`:334,456`) + `KNOWN_PROVIDERS`
- `src/cli/mod.rs`, `src/cli/repl.rs` — modified: flags; REPL routed through shared lifecycle

**Routes verified present at `546ef04`:** `/v2/chat/{stream,approve,deny,interrupt}` · `/v2/agents{,/switch,/default}` · `/v2/providers` · `/v2/{switch-model,switch-provider,switch}` · `/v2/settings{,/permission}` · `DELETE /v2/settings/approved-tools` · `/health` on the auth-exempt public router.

For uncommitted, unrelated tree state see §1.

---

## 8. Suggested skills for the next agent

Spec §12.2 now assigns these per work package. Summary:

- **`code-review`** — gate on WP-1 (G4/G5/G8) and WP-7 (Tauri supervisor); the concurrency and lock-ordering logic is the risky part and deserves a second pass.
- **`security-review`** — **blocking** gate on WP-2 (**G6**, sandboxed file read). Path traversal, symlink escape, gitignored-secret leakage, and 404-vs-403 information disclosure are all in scope; hardening requirements are in spec G6 and §9.
- **`frontend-design`** or **`example-skills:frontend-design`** — opening move on WP-3 (F1/F5), for the 3-panel layout and visual direction.
- **`example-skills:webapp-testing`** — Playwright driving of the approval dialog; WP-4 exit gate.
- **`commit`** — conventional commit at each package boundary.

Do **not** reach for `prd-generator` — the spec already exists and is reconciled against the code.

---

## 9. Next steps

**The full plan is spec §12** — work packages, model routing, wave schedule, quality gates. Do not re-derive an ordering here; this section records only what is immediately actionable.

**Start here (spec WP-0, blocks everything):**

1. ⚠️ **Human action — unblock the provider.** Add OpenRouter credits or configure a second provider key, then set a tool-capable model. Nothing in §5's "NOT verified" list can be closed without it, and it gates confidence in the whole committed slab.
2. **R1 — route stubs.** Register `/v2/mcp/servers`, `/v2/memory/search`, `/v2/memory/stats`, `/v2/files`, `/v2/files/tree`, `/v2/sessions/{id}/messages` returning `501 not_implemented` in the spec §8 error shape. Independent of item 1, so it can start now — and it is what unlocks parallel dispatch (spec §12.3).
3. **G0 — run the two unverified acceptance checks** from §5 once item 1 lands: the approval round-trip, and REPL-vs-gateway cost parity.

**Then, per spec §12.5, three lanes run in parallel:** WP-1 (G4/G5/G8) · WP-2 (G6 — Opus + `security-review`) · WP-3 (F1/F2/F3/F5).

**Task-specific gotchas** — these stay here because they are what cost investigation time:

- **G4** — mind the **revised §4.7**: HTTP servers are now countable, stdio still are not. The response shape is unchanged either way.
- **G5** — mind §4.8 (`ObsidianVault::search`, not the sidecar) and §4.3 (`std::sync::Mutex`, no `await` while held).
- **G8** — extends the existing `/v1/sessions/{id}` walk to map `FunctionCall`/`FunctionResponse`, pairing by call id.
- **F3** — note **C3**: `EventSource` cannot POST, so use `fetch` + `ReadableStream`. The hand-rolled frame parser is the most likely source of silent breakage; unit-test it against split frames, `\r\n`, multi-line `data:`, comment keep-alives, and unknown event names.
- **F9** — spec **Q9 is now decided**: ship the honest "approve for the rest of this session" copy. Per-call approval is deferred as a harness change. No need to re-open it.

---

## 10. Sensitive information

No secrets are included in this document. Specifically redacted/omitted:

- **API keys and values** — never read or printed; `SecretStore` was only ever queried for *presence* (`available: true/false`).
- **Provider credential inventory** — described only as "only OpenRouter is configured", with no key material, account identifier, or billing detail.
- **Session IDs and local absolute paths** seen during testing are ephemeral and have been omitted from the examples above.
