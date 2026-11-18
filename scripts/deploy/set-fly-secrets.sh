#!/usr/bin/env bash
# Import the public-demo secrets into Fly. Does not print the key or the public key.
# Usage: scripts/deploy/set-fly-secrets.sh /absolute/path/to/server-key.txt
set -euo pipefail

KEY_FILE="${1:-}"
APP="${FLY_APP:-}"
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"

if [[ -z "$APP" ]]; then
  echo "Set FLY_APP to the Fly app name." >&2
  exit 1
fi
if [[ -z "$KEY_FILE" || "$KEY_FILE" != /* ]]; then
  echo "Pass the server key as an absolute path." >&2
  exit 1
fi
if [[ "$KEY_FILE" == "$ROOT"/* ]]; then
  echo "The server key must stay outside this repo." >&2
  exit 1
fi
if [[ ! -f "$KEY_FILE" ]]; then
  echo "The server key file is missing." >&2
  exit 1
fi

perm="$(stat -f '%OLp' "$KEY_FILE" 2>/dev/null || stat -c '%a' "$KEY_FILE")"
if [[ "$perm" != "600" ]]; then
  echo "The server key file must be mode 600." >&2
  exit 1
fi

for name in VACA_MINT_URL VACA_CORS_ORIGIN VACA_SERVER_PUBKEY; do
  if [[ -z "${!name:-}" ]]; then
    echo "Set ${name}." >&2
    exit 1
  fi
done

if [[ "$VACA_MINT_URL" != https://* || "$VACA_CORS_ORIGIN" != https://* ]]; then
  echo "VACA_MINT_URL and VACA_CORS_ORIGIN must be https origins." >&2
  exit 1
fi
if [[ "$VACA_CORS_ORIGIN" == */ || "$VACA_CORS_ORIGIN" == *\?* || "$VACA_CORS_ORIGIN" == *\#* ]]; then
  echo "VACA_CORS_ORIGIN must be an origin, with no path or trailing slash." >&2
  exit 1
fi

node "$ROOT/scripts/deploy/check-mint.mjs"

file_pub="$(awk '$1 == "pubkey" { print $2; exit }' "$KEY_FILE")"
env_pub="$(printf '%s' "$VACA_SERVER_PUBKEY" | tr '[:upper:]' '[:lower:]')"
if [[ ! "$file_pub" =~ ^0[23][0-9a-f]{64}$ || "$file_pub" != "$env_pub" ]]; then
  unset file_pub env_pub
  echo "The pubkey line must match VACA_SERVER_PUBKEY, lowercase." >&2
  exit 1
fi
if ! awk '$1 == "privkey" && $2 ~ /^[0-9a-f]{64}$/ { found = 1 } END { exit found ? 0 : 1 }' "$KEY_FILE"; then
  unset file_pub env_pub
  echo "The server key file does not match VACA_SERVER_PUBKEY." >&2
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

tmp="$(mktemp)"
chmod 600 "$tmp"
cleanup() {
  rm -f "$tmp"
}
trap cleanup EXIT

{
  printf 'VACA_MINT_URL=%s\n' "$VACA_MINT_URL"
  printf 'VACA_CORS_ORIGIN=%s\n' "$VACA_CORS_ORIGIN"
  printf 'VACA_SERVER_PUBKEY=%s\n' "$env_pub"
  printf 'VACA_SERVER_KEY_MATERIAL_B64=%s\n' "$(base64 < "$KEY_FILE" | tr -d '\n')"
} > "$tmp"
unset file_pub env_pub

"${FLY[@]}" secrets import --app "$APP" < "$tmp"
echo "Fly secrets imported for ${APP}."
