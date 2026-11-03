# Vaca

Vaca is an experimental web app for all-or-nothing pots: a cause, a shared purchase, or splitting a bill. Contributors lock ecash toward a target before a deadline. If the target is met, the pot pays a Lightning destination that was committed when the pot was created. If the deadline passes first, each contributor reclaims their own sats, because the mint enforces a NUT-11 refund key. There is no smart contract.

This repository is the skeleton. It stores pots and refuses the money routes. It does not build locks, verify proofs, sign, melt, or refund.

Regtest or Mutinynet only. The hard cap is 100,000 sats per pot.

## Trust model

v1, the first version that moves sats: the server key is the only pot key (`n_sigs = 1`). The server can spend the locked ecash by itself. That is custodial for theft. Refunds still work when the server is gone, because the mint accepts the contributor's refund key after the lock time. The server key for that version stays in a file outside this repo, mode 600. This skeleton does not create or read that file. Set `VACA_SERVER_KEY_PATH` later; the process only checks whether the path is set and whether a file is there.

v2, the pitch: the organizer and the server both sign (`n_sigs = 2`). The server co-signs only when the locked total covers the target plus the melt fee reserve, and only for the destination stored at creation. Neither signature is enough on its own. If the organizer and the server both agree, they can still spend, including after the deadline, and they can win the race against refunds.

After the lock time the mint accepts both paths. It settles whichever valid spend it sees first. Contributors should reclaim as soon as the lock expires. The mint is trusted, as with any Cashu mint. Losing the refund key loses the refund. A secret with no refund key can be spent by anyone after the deadline. A malformed lock cannot be spent. The mint does not check that a secret belongs to this pot; the server has to verify that before it counts a proof. That check is not written yet.

A refund backup will be a file the browser downloads. A URL that contains the key would leak it.

## Decisions for the demo

- v1 proofs use `SIG_INPUTS`, so the CDK wallet melt helper can sign them. v2 uses `SIG_ALL`, which binds the signatures to one melt quote. `prepare_melt` / `prepare_with_proofs` reject `SIG_ALL`, so v2 will post a melt built with `sign_sig_all`.
- Unused melt fee reserve (NUT-08 change) goes to the organizer. The pot page says so. The reserve is not moved in this skeleton.
- The mint lock time is the advertised deadline plus 600 seconds. The server stops co-signing at the advertised deadline, which is earlier than the moment the refund path opens. The post-deadline race remains.
- The cap is 100,000 sats. `VACA_MAX_POT_SATS` may lower it. It cannot raise it. `VACA_GRACE_SECONDS` must be 600 if it is set.

## Who writes what

C, in this repo now: the crate, the Vite + TypeScript page, HTTP and sqlite plumbing, CORS, the demo script, and the regtest mint script.

Y, not in this repo yet: NUT-11 lock construction, the pot state machine, the target-met check including the fee reserve, multi-party signing, the refund flow, and browser key handling. `POST /api/pots/{id}/contributions`, `/payout`, and `/refunds` return 501. The contributions table has no proof column and no insert path.

## Pins

The server does not link CDK. When the money path is added, pin CDK git revision `24360fe5fabda9d8aa3ae98b1a7e5e15986aad5a` (the `cashu` crate on that commit is 0.18.0). Do not path-depend on `~/projects/cdk-nutnov`.

The browser pin is `@cashu/cashu-ts@5.0.0-rc.12`, commit `4d6e1f86aaae0627bd4b06f4497da9fce0abe37b`. The page does not import the library. `dd8bbe3` is a later main commit, not this tag.

CDK deserializes a P2PK witness as a string of JSON. cashu-ts signs and returns an object. A later proof parser has to stringify the witness before the `cashu` crate reads it. This skeleton has no proof parser.

## Run the page

```bash
bash scripts/demo.sh
```

The API listens on `http://127.0.0.1:8787`. The page is `http://127.0.0.1:5173`. The demo script unsets `VACA_START_MINT` for itself and does not start a mint.

Useful checks without the page:

```bash
cargo test --manifest-path server/Cargo.toml
curl -sS http://127.0.0.1:8787/api/health
```

`VACA_BIND`, `VACA_DB_PATH`, `VACA_MINT_URL`, and `VACA_CORS_ORIGIN` override the defaults. Leave the mint URL empty until a mint exists. The sqlite file defaults to `data/vaca.sqlite` and is gitignored. The process logs whether a server-key path is set. It does not log the path or open the file.

Creating a pot:

```bash
curl -sS -X POST http://127.0.0.1:8787/api/pots \
  -H 'content-type: application/json' \
  -d "{\"title\":\"Milk\",\"kind\":\"purchase\",\"target_sats\":1000,\"deadline_unix\":$(($(date +%s) + 86400)),\"destination\":\"lno1example\"}"
```

`deadline_unix` has to be in the future and within 366 days. Kinds are `cause`, `purchase`, and `split_bill`. Status stays `open`. An unknown JSON field, including `proofs`, is rejected.

The split-the-bill control on the page fills a two-hour deadline and labels the destination as the bill's BOLT11 invoice. It does not change payout rules.

## Regtest mint script

`scripts/regtest-mint.sh` prints `mint_started=no` and exits unless `VACA_START_MINT=1`. It was written against CDK `24360fe5` and was not run.

On that revision, `cdk-mintd --config` is rejected. The operator imports `scripts/regtest-mint.config.toml` with `config init --new-mint`, then starts `cdk-mintd --work-dir`. The document sets `bitcoin_network = "regtest"`, `chain_source_type = "bitcoinrpc"`, RPC `127.0.0.1:18443`, and `backend = "ldk-node"`. Mnemonics and the RPC password are `env:` references. The script does not start bitcoind, and it does not build CDK. Point `CDK_MINTD_BIN` at a binary built from that revision with the ldk-node feature. The checkout at `~/projects/cdk` is older than this CLI.

Mint files go under `~/.vaca/regtest-mint/` (override with `VACA_MINT_WORK_DIR` and `VACA_LDK_STORAGE_DIR`). Both directories must sit outside this repo.

## Not in this skeleton

Live `/v1/info` was not fetched. No mint was running, and this session did not start one. v2 co-signing, strict lock verification, and the split-the-bill payout rules come later. "Pay together now" (NUT-15, several mints paying one invoice) is a later note in this README, after v1 and v2 work. Today's `cdk-ldk-node` reports `mpp: false`.

Local git only. No remote.

## Safety

Experimental. Regtest or Mutinynet only. Never log keys. Never put a key in a URL, a command line, or this repo.
