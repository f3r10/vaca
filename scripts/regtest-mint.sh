#!/usr/bin/env bash
# Write and, only if asked, start a regtest cdk-mintd for Vaca.
# The default is to exit without launching cdk-mintd or bitcoind.
# Vaca's demo script never sets VACA_START_MINT.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TEMPLATE="$ROOT/scripts/regtest-mint.config.toml"
CDK_REV="24360fe5fabda9d8aa3ae98b1a7e5e15986aad5a"

if [[ "${VACA_START_MINT:-}" != "1" ]]; then
  cat <<EOF
mint_started=no
Vaca regtest mint was not started.
Nothing launched cdk-mintd or bitcoind.
To start it later, set VACA_START_MINT=1 and point CDK_MINTD_BIN at a
cdk-mintd built from CDK ${CDK_REV} with the ldk-node feature.
The local ~/projects/cdk checkout is older and still expects --config.
Do not use the fake_wallet backend. Secrets stay in the environment:
CDK_MINTD_MNEMONIC, CDK_MINTD_LDK_NODE_MNEMONIC,
CDK_MINTD_LDK_BITCOIND_RPC_PASSWORD.
A regtest bitcoind must already be listening on 127.0.0.1:18443.
Use the ldk-lab bitcoind (RPC user ldk). This script does not start bitcoind.
EOF
  exit 0
fi

echo "mint_started=pending" >&2
echo "VACA_START_MINT=1: checking the regtest mint setup." >&2

if [[ ! -f "$TEMPLATE" ]]; then
  echo "missing config template" >&2
  exit 1
fi

if grep -E -q 'fake_wallet|fakewallet' "$TEMPLATE"; then
  echo "refusing to start: template mentions fake_wallet" >&2
  exit 1
fi

if ! grep -E -q 'bitcoin_network[[:space:]]*=[[:space:]]*"regtest"' "$TEMPLATE"; then
  echo "refusing to start: bitcoin_network is not regtest" >&2
  exit 1
fi

for var in CDK_MINTD_MNEMONIC CDK_MINTD_LDK_NODE_MNEMONIC CDK_MINTD_LDK_BITCOIND_RPC_PASSWORD; do
  if [[ -z "${!var:-}" ]]; then
    echo "missing ${var} (value not shown)" >&2
    exit 1
  fi
done

BIN="${CDK_MINTD_BIN:-cdk-mintd}"
if ! command -v "$BIN" >/dev/null 2>&1; then
  echo "cdk-mintd not found. Set CDK_MINTD_BIN. This script does not build CDK." >&2
  exit 1
fi

WORK="${VACA_MINT_WORK_DIR:-$HOME/.vaca/regtest-mint/mintd}"
STORAGE="${VACA_LDK_STORAGE_DIR:-$HOME/.vaca/regtest-mint/ldk-node}"

case "$WORK" in
  "$ROOT"|"$ROOT"/*) echo "mint work dir must be outside the repo" >&2; exit 1 ;;
esac
case "$STORAGE" in
  "$ROOT"|"$ROOT"/*) echo "ldk storage dir must be outside the repo" >&2; exit 1 ;;
esac

if [[ "$WORK" != /* || "$STORAGE" != /* ]]; then
  echo "work dir and ldk storage dir must be absolute paths" >&2
  exit 1
fi

if [[ ! "$STORAGE" =~ ^[A-Za-z0-9/_.-]+$ ]]; then
  echo "ldk storage dir has characters this script will not substitute" >&2
  exit 1
fi

mkdir -p "$WORK" "$STORAGE"
GENERATED="$WORK/import.toml"
sed "s#__VACA_LDK_STORAGE_DIR__#${STORAGE}#g" "$TEMPLATE" > "$GENERATED"
chmod 600 "$GENERATED"

if grep -E -q 'fake_wallet|fakewallet|__VACA_LDK_STORAGE_DIR__' "$GENERATED"; then
  echo "refusing to start: generated config is not a regtest ldk-node document" >&2
  rm -f "$GENERATED"
  exit 1
fi

"$BIN" config validate --file "$GENERATED"

if [[ ! -f "$WORK/cdk-mintd.sqlite" ]]; then
  "$BIN" --work-dir "$WORK" config init --new-mint --file "$GENERATED"
else
  echo "Work dir already has a mint database. Not running config init again." >&2
fi

echo "mint_started=yes" >&2
exec "$BIN" --work-dir "$WORK"
