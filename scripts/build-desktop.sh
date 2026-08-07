#!/usr/bin/env bash
set -euo pipefail

# ── build-desktop.sh ──────────────────────────────────────────────
# Build the MOMO WORK desktop app, correctly, in one command.
#
# Usage:
#   ./scripts/build-desktop.sh                 # .app (or platform equivalent)
#   ./scripts/build-desktop.sh --dmg           # also produce a .dmg (macOS)
#   ./scripts/build-desktop.sh --open          # launch it when finished
#   ./scripts/build-desktop.sh --no-restore    # leave web/out as the desktop build
#   ./scripts/build-desktop.sh --skip-web      # reuse the existing desktop web build
#
# Three steps in this build are easy to get wrong by hand, and each one fails in
# a way that looks like something else. That is why this script exists.
#
#   1. `npm run build:desktop`, never `npm run build`. The gateway build sets
#      basePath /ui; Tauri serves at the root. Use the wrong one and every asset
#      404s — the app opens and renders unstyled, which reads as a broken theme
#      rather than a wrong path.
#
#   2. `touch src/lib.rs` before bundling. Tauri compiles web/out into the
#      binary, and that only happens when the Rust crate itself rebuilds. Change
#      nothing but the UI and cargo can reasonably decide there is nothing to
#      do, shipping the previous assets. The app then looks half-updated while
#      the source is fine.
#
#   3. Put web/out back afterwards. The desktop build leaves web/out with no
#      basePath, so the *gateway* would then serve a broken /ui. Nothing warns
#      you; you just get an unstyled page the next time you run the gateway.
#
# The gateway binary itself needs no handling here — tauri.conf.json's
# beforeBuildCommand rebuilds and stages it during the bundle. This script
# verifies that it actually happened rather than assuming it.
# ──────────────────────────────────────────────────────────────────

BUNDLES="app"
DO_OPEN=false
RESTORE=true
SKIP_WEB=false

CYAN='\033[0;36m'; GREEN='\033[0;32m'; YELLOW='\033[1;33m'; RED='\033[0;31m'; NC='\033[0m'
log()  { echo -e "${CYAN}[build-desktop]${NC} $*"; }
ok()   { echo -e "${GREEN}[ok]${NC} $*"; }
warn() { echo -e "${YELLOW}[warn]${NC} $*"; }
err()  { echo -e "${RED}[error]${NC} $*" >&2; exit 1; }

while [[ $# -gt 0 ]]; do
  case "$1" in
    --dmg)        BUNDLES="app,dmg"; shift ;;
    --open)       DO_OPEN=true; shift ;;
    --no-restore) RESTORE=false; shift ;;
    --skip-web)   SKIP_WEB=true; shift ;;
    -h|--help)    awk '/^# ──/{n++; if(n==2) exit; next} n==1{ sub(/^# ?/,""); print }' "$0"; exit 0 ;;
    *)            err "Unknown flag: $1" ;;
  esac
done

ROOT="$(git rev-parse --show-toplevel 2>/dev/null)" || err "Not inside a git repository."
cd "$ROOT"

# ── Preflight ─────────────────────────────────────────────────────
command -v npm   >/dev/null || err "npm not found."
command -v cargo >/dev/null || err "cargo not found. Install Rust: https://rustup.rs"
cargo tauri --version >/dev/null 2>&1 \
  || err "cargo-tauri not found. Install it: cargo install tauri-cli --version '^2'"

[[ -d web/node_modules ]] || { log "Installing web dependencies"; (cd web && npm ci); }

# ── 1. The frontend, in its desktop shape ─────────────────────────
if [[ "$SKIP_WEB" == true ]]; then
  warn "Skipping the web build — make sure web/out is the desktop variant."
else
  log "Building the web UI (build:desktop — no basePath)"
  (cd web && npm run build:desktop >/dev/null) || err "Web build failed."
fi

# Assert the shape rather than trusting the flag: the gateway build writes
# /ui/_next into index.html, the desktop build writes /_next.
if grep -q '"/ui/_next\|/ui/_next/' web/out/index.html 2>/dev/null; then
  err "web/out is the GATEWAY build (basePath /ui). Run without --skip-web."
fi
ok "web/out is the desktop build"

# ── 2. Force the frontend to be re-embedded ───────────────────────
touch desktop/src-tauri/src/lib.rs

# ── 3. Bundle. beforeBuildCommand rebuilds and stages the gateway. ─
log "Bundling ($BUNDLES) — this also rebuilds and stages momo-fetch"
(cd desktop/src-tauri && cargo tauri build --bundles "$BUNDLES") \
  || err "Bundle failed. If only the .dmg step failed, the .app is still usable — see below."

# ── 4. Prove the staged gateway is the one we just built ──────────
# This is the whole reason beforeBuildCommand exists. Assert it, because the
# failure it prevents is invisible: a UI calling an endpoint its own bundled
# gateway does not have.
BUNDLE_DIR="desktop/src-tauri/target/release/bundle"
STAGED=""
case "$(uname -s)" in
  Darwin) STAGED="$BUNDLE_DIR/macos/MOMO WORK.app/Contents/Resources/binaries/momo-fetch" ;;
  Linux)  STAGED="$(find "$BUNDLE_DIR" -name momo-fetch -type f 2>/dev/null | head -1)" ;;
  *)      STAGED="$(find "$BUNDLE_DIR" -name 'momo-fetch*' -type f 2>/dev/null | head -1)" ;;
esac

if [[ -n "$STAGED" && -f "$STAGED" ]]; then
  if cmp -s "$STAGED" target/release/momo-fetch; then
    ok "Bundled gateway matches target/release/momo-fetch"
  else
    err "Bundled gateway differs from target/release/momo-fetch — the staging step did not run. Check beforeBuildCommand in desktop/src-tauri/tauri.conf.json."
  fi
else
  warn "Could not locate the staged gateway to verify it on this platform."
fi

# ── 5. Put web/out back for the gateway ───────────────────────────
if [[ "$RESTORE" == true ]]; then
  log "Restoring web/out to the gateway build (basePath /ui)"
  (cd web && npm run build >/dev/null) || warn "Could not restore the gateway build; run 'cd web && npm run build' before serving /ui."
  ok "web/out is the gateway build again"
else
  warn "web/out is left as the desktop build. The gateway will serve a broken /ui until you run 'cd web && npm run build'."
fi

# ── Done ──────────────────────────────────────────────────────────
echo
ok "Built:"
# `rw.*.dmg` is the scratch image bundle_dmg.sh leaves behind when it fails —
# not an artefact, and listing it would send someone off with a broken file.
find "$BUNDLE_DIR" -maxdepth 2 \
     \( -name '*.app' -o -name '*.dmg' -o -name '*.AppImage' \
        -o -name '*.deb' -o -name '*.msi' -o -name '*.exe' \) \
     ! -name 'rw.*' 2>/dev/null | sed 's/^/    /'

if [[ "$DO_OPEN" == true ]]; then
  case "$(uname -s)" in
    Darwin) open "$BUNDLE_DIR/macos/MOMO WORK.app" ;;
    *)      warn "--open is only wired up for macOS." ;;
  esac
fi
