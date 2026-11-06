#!/bin/bash
# Regtest bitcoind for the Vaca mint. The RPC password comes from the
# environment and is written into the data directory, not into the image.
set -euo pipefail

password="${CDK_MINTD_LDK_BITCOIND_RPC_PASSWORD:-}"
if [[ -z "$password" ]]; then
  echo "missing CDK_MINTD_LDK_BITCOIND_RPC_PASSWORD (value not shown)" >&2
  exit 1
fi
case "$password" in
  *[[:space:]]*|*'#'*)
    echo "CDK_MINTD_LDK_BITCOIND_RPC_PASSWORD must be one token, with no spaces or #" >&2
    exit 1
    ;;
esac

datadir="${BITCOIN_DATA:-/home/bitcoin/.bitcoin}"
conf="$datadir/bitcoin.conf"
mkdir -p "$datadir"
umask 077
cat > "$conf" <<EOF
regtest=1
server=1
rpcuser=testuser
rpcpassword=${password}
txindex=1
# Bitcoin Core 29 applies rpcbind and rpcport only from the network section.
# The host publishes 18443 on 127.0.0.1 only. Inside the container the
# connection arrives from the Docker bridge, so RPC must listen beyond
# localhost. rpcallowip is safe because that host port is not on the LAN.
[regtest]
rpcbind=0.0.0.0
rpcallowip=0.0.0.0/0
rpcport=18443
port=18444
bind=0.0.0.0
EOF
chmod 600 "$conf"
unset password

pid=""
stop() {
  if [[ -n "$pid" ]]; then
    kill -TERM "$pid" 2>/dev/null || true
    wait "$pid" || true
  fi
}
trap stop TERM INT

/entrypoint.sh bitcoind -conf="$conf" &
pid="$!"

ready=0
for _ in $(seq 1 60); do
  if gosu bitcoin bitcoin-cli -datadir="$datadir" -conf="$conf" getblockchaininfo >/dev/null 2>&1; then
    ready=1
    break
  fi
  if ! kill -0 "$pid" 2>/dev/null; then
    echo "bitcoind exited before RPC was ready" >&2
    wait "$pid" || true
    exit 1
  fi
  sleep 0.5
done
if [[ "$ready" != "1" ]]; then
  echo "bitcoind RPC did not answer" >&2
  stop
  exit 1
fi

cli=(gosu bitcoin bitcoin-cli -datadir="$datadir" -conf="$conf")
if "${cli[@]}" listwalletdir | grep -q '"name": "vaca"'; then
  "${cli[@]}" loadwallet "vaca" >/dev/null
else
  "${cli[@]}" createwallet "vaca" >/dev/null
fi

# Coinbase outputs mature after 100 blocks. 101 leaves one spendable output.
blocks="$("${cli[@]}" getblockcount)"
if [[ "$blocks" -lt 101 ]]; then
  address="$("${cli[@]}" -rpcwallet=vaca getnewaddress)"
  "${cli[@]}" -rpcwallet=vaca generatetoaddress "$((101 - blocks))" "$address" >/dev/null
  blocks="$("${cli[@]}" getblockcount)"
fi
chain="$("${cli[@]}" getblockchaininfo | sed -n 's/.*"chain": "\([^"]*\)".*/\1/p')"
echo "bitcoind ready chain=${chain} blocks=${blocks}"

wait "$pid"
