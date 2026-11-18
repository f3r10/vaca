#!/usr/bin/env bash
# Deploy the API image to the existing Fly app. Does not create secrets.
set -euo pipefail

APP="${FLY_APP:-}"
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"

if [[ -z "$APP" ]]; then
  echo "Set FLY_APP to the Fly app name." >&2
  exit 1
fi
if [[ -z "${FLY_API_TOKEN:-}" ]]; then
  echo "Set FLY_API_TOKEN." >&2
  exit 1
fi
if [[ ! "$APP" =~ ^[a-z0-9][a-z0-9-]{1,30}$ ]]; then
  echo "FLY_APP must be a Fly app name: lowercase letters, digits, and hyphens." >&2
  exit 1
fi

if command -v flyctl >/dev/null 2>&1; then
  FLY=(flyctl)
elif command -v fly >/dev/null 2>&1; then
  FLY=(fly)
else
  echo "flyctl is not on PATH." >&2
  exit 1
fi

cd "$ROOT"
"${FLY[@]}" deploy --remote-only --ha=false --app "$APP" --config fly.toml
echo "API deploy finished for https://${APP}.fly.dev"
