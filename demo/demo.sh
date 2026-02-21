#!/usr/bin/env bash
set -euo pipefail

# ─────────────────────────────────────────────
# Context Harness — Interactive Demo
# ─────────────────────────────────────────────
# This script:
#   1. Builds the ctx binary
#   2. Initializes the SQLite database
#   3. Syncs the knowledge base
#   4. Starts the MCP server
#   5. Opens the interactive UI in your browser
# ─────────────────────────────────────────────

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
CONFIG="$PROJECT_ROOT/demo/config/ctx.toml"
DATA_DIR="$PROJECT_ROOT/demo/data"
UI_FILE="$PROJECT_ROOT/demo/ui/index.html"
CTX="$PROJECT_ROOT/target/release/ctx"

CYAN='\033[0;36m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
DIM='\033[0;90m'
BOLD='\033[1m'
NC='\033[0m'

banner() {
  echo ""
  echo -e "${CYAN}${BOLD}  ⚡ Context Harness — Live Demo${NC}"
  echo -e "${DIM}  ─────────────────────────────────${NC}"
  echo ""
}

step() {
  echo -e "  ${GREEN}▸${NC} ${BOLD}$1${NC}"
}

info() {
  echo -e "    ${DIM}$1${NC}"
}

banner

# ── Step 1: Build ──────────────────────────────
step "Building ctx binary (release)..."
cd "$PROJECT_ROOT"
cargo build --release --quiet 2>&1
info "Binary: $CTX"
echo ""

# ── Step 2: Clean & Init ──────────────────────
step "Initializing database..."
rm -rf "$DATA_DIR"
mkdir -p "$DATA_DIR"
"$CTX" --config "$CONFIG" init 2>&1 | sed 's/^/    /'
echo ""

# ── Step 3: Sync knowledge base ───────────────
step "Syncing knowledge base ($(ls "$PROJECT_ROOT/demo/knowledge-base/" | wc -l | tr -d ' ') documents)..."
"$CTX" --config "$CONFIG" sync filesystem 2>&1 | sed 's/^/    /'
echo ""

# ── Step 4: Quick search demo ─────────────────
step "Testing search (keyword: 'incident response')..."
echo ""
"$CTX" --config "$CONFIG" search "incident response" --limit 3 2>&1 | sed 's/^/    /'
echo ""

# ── Step 5: Show sources ──────────────────────
step "Connected sources:"
"$CTX" --config "$CONFIG" sources 2>&1 | sed 's/^/    /'
echo ""

# ── Step 6: Start server ──────────────────────
step "Starting MCP server on http://127.0.0.1:7331 ..."
"$CTX" --config "$CONFIG" serve mcp &
SERVER_PID=$!

# Wait for server to be ready
for i in $(seq 1 30); do
  if curl -s http://127.0.0.1:7331/health > /dev/null 2>&1; then
    break
  fi
  sleep 0.2
done

echo ""
info "Server PID: $SERVER_PID"
info "Health: $(curl -s http://127.0.0.1:7331/health)"
echo ""

# ── Step 7: Open UI ───────────────────────────
step "Opening interactive UI..."

if command -v open &> /dev/null; then
  open "$UI_FILE"
elif command -v xdg-open &> /dev/null; then
  xdg-open "$UI_FILE"
else
  info "Open this file in your browser: $UI_FILE"
fi

echo ""
echo -e "  ${CYAN}${BOLD}┌────────────────────────────────────────────┐${NC}"
echo -e "  ${CYAN}${BOLD}│${NC}  Demo is running!                          ${CYAN}${BOLD}│${NC}"
echo -e "  ${CYAN}${BOLD}│${NC}                                            ${CYAN}${BOLD}│${NC}"
echo -e "  ${CYAN}${BOLD}│${NC}  ${DIM}MCP Server:${NC}  http://127.0.0.1:7331       ${CYAN}${BOLD}│${NC}"
echo -e "  ${CYAN}${BOLD}│${NC}  ${DIM}Search:${NC}      POST /tools/search          ${CYAN}${BOLD}│${NC}"
echo -e "  ${CYAN}${BOLD}│${NC}  ${DIM}Get:${NC}         POST /tools/get             ${CYAN}${BOLD}│${NC}"
echo -e "  ${CYAN}${BOLD}│${NC}  ${DIM}Sources:${NC}     GET  /tools/sources         ${CYAN}${BOLD}│${NC}"
echo -e "  ${CYAN}${BOLD}│${NC}  ${DIM}Health:${NC}      GET  /health                ${CYAN}${BOLD}│${NC}"
echo -e "  ${CYAN}${BOLD}│${NC}                                            ${CYAN}${BOLD}│${NC}"
echo -e "  ${CYAN}${BOLD}│${NC}  ${YELLOW}Press Ctrl+C to stop${NC}                      ${CYAN}${BOLD}│${NC}"
echo -e "  ${CYAN}${BOLD}└────────────────────────────────────────────┘${NC}"
echo ""

# Trap Ctrl+C to clean up
cleanup() {
  echo ""
  echo -e "  ${DIM}Shutting down server (PID $SERVER_PID)...${NC}"
  kill $SERVER_PID 2>/dev/null || true
  wait $SERVER_PID 2>/dev/null || true
  echo -e "  ${GREEN}Done.${NC}"
  echo ""
}

trap cleanup EXIT INT TERM

# Wait for server to exit
wait $SERVER_PID

