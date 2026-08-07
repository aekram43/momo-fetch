#!/usr/bin/env bash
set -euo pipefail

# ── start-gateway.sh ──────────────────────────────────────────────
# Start MOMO Gateway + Cloudflare Tunnel (Named Tunnel — Option B)
#
# Operator script, not part of the build. See docs/user-guide.md §17.
# Usage:
#   ./start-gateway.sh                          # defaults
#   ./start-gateway.sh --port 8080               # custom port
#   ./start-gateway.sh --no-tunnel               # gateway only
#   ./start-gateway.sh --tunnel-only             # skip gateway, tunnel only
# ──────────────────────────────────────────────────────────────────

# Defaults
GATEWAY_PORT="${GATEWAY_PORT:-3000}"
PROJECT_DIR="${PROJECT_DIR:-$(git rev-parse --show-toplevel 2>/dev/null || pwd)}"
PERMISSION="${PERMISSION:-auto}"
PROVIDER="${MOMO_PROVIDER:-}"
MODEL="${MOMO_MODEL:-}"
TUNNEL="${TUNNEL_NAME:-momo-gateway}"
CF_DOMAIN="${CF_DOMAIN:-}"
ENABLE_TUNNEL=true
GATEWAY_ONLY=false
TUNNEL_ONLY=false

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
NC='\033[0m'

log()  { echo -e "${CYAN}[momo-gateway]${NC} $*"; }
ok()   { echo -e "${GREEN}[ok]${NC} $*"; }
warn() { echo -e "${YELLOW}[warn]${NC} $*"; }
err()  { echo -e "${RED}[error]${NC} $*" >&2; exit 1; }

# ── Parse args ─────────────────────────────────────────────────────
while [[ $# -gt 0 ]]; do
  case "$1" in
    --port)         GATEWAY_PORT="$2"; shift 2 ;;
    --project)      PROJECT_DIR="$2"; shift 2 ;;
    --provider)     PROVIDER="$2"; shift 2 ;;
    --model)        MODEL="$2"; shift 2 ;;
    --permission)   PERMISSION="$2"; shift 2 ;;
    --tunnel-name)  TUNNEL="$2"; shift 2 ;;
    --domain)       CF_DOMAIN="$2"; shift 2 ;;
    --no-tunnel)    ENABLE_TUNNEL=false; shift ;;
    --tunnel-only)  TUNNEL_ONLY=true; shift ;;
    # Implies --no-tunnel: the whole point of this flag is to run without one,
    # and the preflight below refuses to start when cloudflared is missing.
    --gateway-only) GATEWAY_ONLY=true; ENABLE_TUNNEL=false; shift ;;
    -h|--help)
      sed -n '2,/^$/{ s/^# //; s/^#//; p }' "$0"
      exit 0
      ;;
    *) err "Unknown flag: $1" ;;
  esac
done

# ── Preflight checks ──────────────────────────────────────────────
command -v momo-fetch >/dev/null 2>&1 || err "momo-fetch not found in PATH. Run 'cargo build --release' and put target/release on PATH."
command -v cloudflared >/dev/null 2>&1 || { [[ "$ENABLE_TUNNEL" == true ]] && err "cloudflared not installed. Run: brew install cloudflared"; }

if [[ "$TUNNEL_ONLY" == false ]]; then
  # Load .env *before* checking for a key. The other order warns "no API key"
  # at a project whose .env has one, which sends people looking for a problem
  # that is not there.
  if [[ -f "$PROJECT_DIR/.env" ]]; then
    log "Loading .env"
    set -a; # shellcheck disable=SC1091
    . "$PROJECT_DIR/.env"; set +a
  fi

  HAS_KEY=false
  for VAR in ANTHROPIC_API_KEY OPENAI_API_KEY DEEPSEEK_API_KEY GROQ_API_KEY OPENROUTER_API_KEY ZAI_API_KEY LLM_APIKEY; do
    if [[ -n "${!VAR:-}" ]]; then HAS_KEY=true; break; fi
  done
  [[ "$HAS_KEY" == true ]] || warn "No LLM provider API key in the environment or $PROJECT_DIR/.env. The gateway may fail to answer."
fi

# ── Build gateway command ──────────────────────────────────────────
GATEWAY_CMD="momo-fetch --gateway --gateway-port $GATEWAY_PORT --project \"$PROJECT_DIR\" --permission $PERMISSION"
[[ -n "$PROVIDER" ]] && GATEWAY_CMD="$GATEWAY_CMD --provider $PROVIDER"
[[ -n "$MODEL" ]] && GATEWAY_CMD="$GATEWAY_CMD --model $MODEL"

# ── Gateway function ───────────────────────────────────────────────
start_gateway() {
  log "Starting MOMO Gateway on :$GATEWAY_PORT"
  log "Project: $PROJECT_DIR"
  log "Permission: $PERMISSION"
  eval "$GATEWAY_CMD"
}

# ── Tunnel function ─────────────────────────────────────────────────
start_tunnel() {
  local LOCAL_URL="http://localhost:$GATEWAY_PORT"

  if [[ "$ENABLE_TUNNEL" == true ]]; then
    log "Starting Cloudflare Tunnel → $LOCAL_URL"
    log "Tunnel name: $TUNNEL"

    if [[ -n "$CF_DOMAIN" ]]; then
      log "Domain: $CF_DOMAIN"
      cloudflared tunnel --name "$TUNNEL" --url "$LOCAL_URL" &
      CF_PID=$!
    else
      cloudflared tunnel run "$TUNNEL" --url "$LOCAL_URL" &
      CF_PID=$!
    fi

    sleep 3
    log "Cloudflare tunnel PID: $CF_PID"
    ok "Gateway + Tunnel running"
  else
    ok "Gateway running (tunnel disabled)"
  fi
}

# ── Run ───────────────────────────────────────────────────────────
cleanup() {
  log "Shutting down..."
  [[ -n "${CF_PID:-}" ]] && kill "$CF_PID" 2>/dev/null
  exit 0
}
trap cleanup SIGINT SIGTERM

if [[ "$TUNNEL_ONLY" == true ]]; then
  start_tunnel
  wait
elif [[ "$GATEWAY_ONLY" == true ]]; then
  start_gateway
else
  start_gateway &
  GW_PID=$!
  log "Waiting for the gateway to answer /health…"
  for _ in $(seq 1 30); do
    curl -sf "http://localhost:$GATEWAY_PORT/health" >/dev/null 2>&1 && break
    sleep 1
  done
  curl -sf "http://localhost:$GATEWAY_PORT/health" >/dev/null 2>&1 \
    || { kill "$GW_PID" 2>/dev/null; err "Gateway did not become ready on :$GATEWAY_PORT"; }
  ok "Gateway ready (pid $GW_PID)"
  start_tunnel
  wait
fi
