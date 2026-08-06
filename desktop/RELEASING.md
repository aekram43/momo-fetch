# Releasing MOMO WORK

> **Installing it, not shipping it?** See
> [`docs/momo-desktop-install.md`](../docs/momo-desktop-install.md). Nothing on
> this page applies to a build-from-source install — signing exists to remove
> warnings on *downloaded* binaries, and a locally built bundle has no download
> quarantine to warn about.
>
> This page is for the day prebuilt binaries get distributed. Until then T11 and
> T13 are **enhancements, not outstanding work.**

Everything here except signing is automated by
[`.github/workflows/desktop.yml`](../.github/workflows/desktop.yml) (T10). Push a
`v*` tag, or run the workflow by hand, and it builds macOS (both arches),
Windows and Linux bundles with the `momo-fetch` gateway embedded as a resource.

## What ships today

Unsigned bundles. They install and run:

- **macOS** — Gatekeeper blocks the first launch. Right-click → **Open**, then
  **Open** again. Once per install.
- **Windows** — SmartScreen shows "Windows protected your PC". **More info** →
  **Run anyway**.
- **Linux** — `.deb` and `.AppImage` install normally; nothing to bypass.

Auto-update (**T11**) is **off**. `plugins.updater.active` is `false` in
`tauri.conf.json`, because the updater verifies a signature before applying
anything and an unsigned build has nothing to verify against. Turning it on
without a key would give you an update path that either fails on every check or,
worse, is trusted for the wrong reasons.

## T13 — code signing (needs credentials, cannot be automated here)

Three separate things, none of which can be generated from this repo.

### 1. Tauri updater key

The only one you can make locally. It signs the *update artifacts*, not the app.

```bash
cd desktop/src-tauri
cargo tauri signer generate -w ~/.tauri/momo-worker.key
```

Then:

- put the **public** key in `tauri.conf.json` → `plugins.updater.pubkey`
- add the **private** key as the `TAURI_SIGNING_PRIVATE_KEY` repository secret,
  and its password as `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` (the workflow already
  reads both)
- flip `plugins.updater.active` to `true`

Keep the private key out of the repo. Losing it means existing installs can
never be updated again — they will not accept artifacts signed by a new key.

### 2. Apple (macOS notarization)

Needs a paid **Apple Developer Program** membership. Export a *Developer ID
Application* certificate as `.p12`, then add these secrets:

| Secret | What it is |
|---|---|
| `APPLE_CERTIFICATE` | base64 of the `.p12` |
| `APPLE_CERTIFICATE_PASSWORD` | its export password |
| `APPLE_SIGNING_IDENTITY` | e.g. `Developer ID Application: Name (TEAMID)` |
| `APPLE_ID` | the Apple ID email |
| `APPLE_PASSWORD` | an app-specific password, **not** the account password |
| `APPLE_TEAM_ID` | 10-character team id |

Tauri picks these up from the environment during `tauri build`; the workflow's
`env:` block is where to add them.

### 3. Windows

An **EV** or standard code-signing certificate from a CA. Configure
`bundle.windows.certificateThumbprint` (and `digestAlgorithm`, `timestampUrl`)
in `tauri.conf.json`, with the cert installed on the runner or supplied via
Azure Trusted Signing.

## Order

`T13 → T11`. Do the updater key first — it is free and unblocks auto-update on
platforms where the OS warning is tolerable. Apple and Windows certificates only
remove the first-launch friction; they are not prerequisites for shipping.

## Local build

```bash
cd web && npm run build:desktop      # basePath "" — Tauri serves at /
cd desktop/src-tauri && cargo tauri build --bundles app
```

`beforeBuildCommand` rebuilds `momo-fetch` and stages it into `binaries/`, so the
bundled gateway always matches the tree. Do not stage it by hand — that was the
old flow and it shipped a stale gateway.

**`build:desktop`, not `build`.** The gateway serves the UI at `/ui` and needs
`basePath: "/ui"`; Tauri serves it at `/` and must not have one. Use the wrong
build and every asset 404s inside the app — it renders unstyled, which looks
like a CSS problem rather than a path problem.
