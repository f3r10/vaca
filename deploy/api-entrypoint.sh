#!/bin/sh
# Write the server key from the Fly secret, then start the API as user vaca.
# The secret is base64 so it can be one line. This script does not print it
# or the key path.
set -eu

export VACA_BIND="${VACA_BIND:-0.0.0.0:8787}"
export VACA_DB_PATH="${VACA_DB_PATH:-/data/vaca.sqlite}"
export VACA_SERVER_KEY_PATH="${VACA_SERVER_KEY_PATH:-/data/server-key.txt}"
unset VACA_START_MINT

if [ -z "${VACA_MINT_URL:-}" ] || [ -z "${VACA_CORS_ORIGIN:-}" ]; then
  echo "VACA_MINT_URL and VACA_CORS_ORIGIN must be set." >&2
  exit 1
fi
if [ -z "${VACA_SERVER_PUBKEY:-}" ] || [ -z "${VACA_SERVER_KEY_MATERIAL_B64:-}" ]; then
  echo "The server key is not set." >&2
  exit 1
fi

key_path="$VACA_SERVER_KEY_PATH"
key_dir=$(dirname "$key_path")
mkdir -p "$key_dir"

tmp="${key_path}.tmp"
rm -f "$tmp"
umask 077
if ! printf '%s' "$VACA_SERVER_KEY_MATERIAL_B64" | base64 -d > "$tmp"; then
  rm -f "$tmp"
  unset VACA_SERVER_KEY_MATERIAL_B64
  echo "The server key is not set." >&2
  exit 1
fi
unset VACA_SERVER_KEY_MATERIAL_B64
printf '\n' >> "$tmp"

if ! grep -q '^pubkey ' "$tmp" || ! grep -q '^privkey ' "$tmp"; then
  rm -f "$tmp"
  echo "The server key is not set." >&2
  exit 1
fi

chmod 600 "$tmp"
mv "$tmp" "$key_path"
chown vaca:vaca "$key_dir" "$key_path"
chmod 700 "$key_dir"
chmod 600 "$key_path"

exec setpriv --reuid=vaca --regid=vaca --init-groups --inh-caps=-all /usr/local/bin/vaca-server
