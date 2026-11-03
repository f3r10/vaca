#!/usr/bin/env bash
# Run the Vaca API and the Vite page on localhost.
# This script does not start a mint. It clears VACA_START_MINT for its own
# process so a leaked shell variable cannot reach a child.
set -euo pipefail
set -m

unset VACA_START_MINT

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
mkdir -p "$ROOT/data"

export VACA_BIND="${VACA_BIND:-127.0.0.1:8787}"
export VACA_DB_PATH="${VACA_DB_PATH:-$ROOT/data/vaca.sqlite}"
export VACA_CORS_ORIGIN="${VACA_CORS_ORIGIN:-http://127.0.0.1:5173}"
export VACA_MINT_URL="${VACA_MINT_URL:-}"

echo "Building the API and the page. No mint is started."
cargo build --manifest-path "$ROOT/server/Cargo.toml"
(cd "$ROOT/web" && npm install)

SERVER_PID=""
WEB_PID=""
cleanup() {
  if [[ -n "$SERVER_PID" ]]; then
    kill -- -"$SERVER_PID" 2>/dev/null || kill "$SERVER_PID" 2>/dev/null || true
  fi
  if [[ -n "$WEB_PID" ]]; then
    kill -- -"$WEB_PID" 2>/dev/null || kill "$WEB_PID" 2>/dev/null || true
  fi
}
trap cleanup EXIT INT TERM

"$ROOT/server/target/debug/vaca-server" &
SERVER_PID=$!
(cd "$ROOT/web" && npm run dev -- --host 127.0.0.1 --port 5173) &
WEB_PID=$!

echo "API:  http://${VACA_BIND}"
echo "Page: http://127.0.0.1:5173"
echo "Stop with Ctrl-C. The regtest mint script is separate and was not run."
wait
