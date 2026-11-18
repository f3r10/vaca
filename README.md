# Vaca

Vaca is an experimental web app for all-or-nothing pots: a cause, a shared purchase, or splitting a bill. Contributors lock ecash toward a target before a deadline. If the target is met, the pot pays a Lightning destination that was committed when the pot was created. If the deadline passes first, each contributor reclaims their own sats, because the mint enforces a NUT-11 refund key. There is no smart contract.

This repository stores pots. The page locks a token to the pot and posts the proofs. `POST /api/pots/{id}/contributions` checks each proof and, when the checks pass, adds its amount to the pot. When the locked total covers the target, the mint's Lightning fee reserve, and the input fee on the locked proofs, and the advertised deadline has not passed, `POST /api/pots/{id}/payout` signs each stored proof and asks the mint to pay the destination. The pot becomes `paid` only when the mint says the payment is paid. A mint failure leaves the pot open. Unused fee reserve comes back as NUT-08 change and is stored for the organizer. A download of those proofs is later. After the mint lock time, `POST /api/pots/{id}/refunds` returns that contributor's proofs with no server signature. The page signs with the refund key and swaps them at the mint. The server does not spend them.

Regtest or Mutinynet only. The hard cap is 100,000 sats per pot.

## How Vaca uses NUT-11

Vaca does not implement NUT-11. The mint does: `cdk-mintd` advertises NUT-11 and enforces it when a proof is spent. Vaca builds the locks and checks them.

**The lock.** Each contribution is a P2PK secret with:

- `data`: the pot key. In v1 that is the server's public key. This is the payout path.
- `locktime`: the advertised deadline plus 600 seconds.
- `refund`: the contributor's public key. The page's "Create refund key" button generates the key pair in the browser and downloads it as a file. The private key never goes to the server.

`createSecret` in `web/src/lock.ts` builds one secret with cashu-ts `createP2PKsecret`, parses it back, and checks each field. The page does not post that string. It asks the mint to swap the token, and the mint signs new proofs whose secrets carry the same lock. `lockForPot` in `web/src/contribute.ts` calls `wallet.ops.send` with `mainKeys`, `refundKeys`, and `locktime` only. A v3 keyset would encode a different secret, so the page refuses that keyset before the swap. `web/src/mint-pod-check.ts` is the same swap as a node script.

**The check before counting.** The mint does not know what a pot is. It only checks keys and time. So `secret_matches_pot` in `server/src/lock.rs` accepts a proof only when the kind is P2PK, `data` is the pot key, and there are exactly two tags: the pot's locktime and one refund key. 02 and 03 keys with the same x-coordinate count as the same key. The contributions route then asks the mint that the proof is signed and unspent before it adds the amount. Signed is a NUT-12 DLEQ check, `verify_dleq` in `server/src/mint.rs`, against the key for that amount on the active sat keyset. Unspent is NUT-07, `POST /v1/checkstate`. The refund route runs those two checks before it returns proofs.

**Spending.**

- Payout: once the fee gate is open and before the advertised deadline, the server signs each stored proof with the pot key (`sign_p2pk` in `server/src/http.rs`, SIG_INPUTS, one signature per proof) and posts those proofs to the mint's NUT-05 melt. The pot is `paid` only after the mint reports `Paid`. The melt sends NUT-08 blank outputs for the sats locked above the invoice amount. When the mint returns change, those proofs are stored for the organizer and the pot response shows the total. The proof JSON stays out of the response. A failed melt leaves the signatures in place and the pot `open`.
- Refund: after the lock time, the mint accepts a signature from the contributor's refund key. The mint enforces this, so the server does not co-sign. The refund route returns the matching stored proofs and strips any witness. The page reads the refund file locally, posts only the public key, and signs each returned proof with that key once the lock is expired. It swaps them at the mint (NUT-03) for unlocked proofs and downloads a token. The private key is not sent, stored, or shown.

The limits are in the trust model: after the lock time both paths are valid, and in v1 the server key alone can spend.

## How Vaca uses NUT-08

Vaca does not implement NUT-08. The mint does: a melt can include blank outputs, and the mint returns whatever Lightning did not spend as blind signatures in `change`. Vaca builds the blanks and stores the proofs.

**The surplus.** The fee reserve on the melt quote is the most the mint may spend on Lightning to pay the invoice. It is not a fee Vaca charges. The mint also charges a NUT-02 input fee when it spends the proofs: `ceil(proof_count * input_fee_ppk / 1000)` on the active sat keyset. A rate of 0 adds nothing. Contributors lock the invoice amount, that Lightning reserve, and this input fee. The page's lock asks the wallet to put the input fee inside the proofs, so a later reclaim can return the amount that was asked for. The swap that creates the lock still spends one input fee from the original token, and that sat is not in the pot. `live_melt` in `server/src/mint.rs` takes the surplus as the locked total minus the invoice amount. It does not subtract the input fee again. The Lightning fee is known only after the mint pays, so the blanks cover the most that can come back. Sats locked above the invoice, including a reserve that a later quote no longer needs, are part of that same surplus. A surplus of 0 sends `outputs: null`.

**The blanks.** `PreMintSecrets::blank` builds them for the active sat keyset from `GET /v1/keysets`. The `wallet` feature of the `cashu` crate is what exposes that function. Each blinded message has amount 0. The count is `ceil(log2(surplus))`, and at least one: one sat is one blank, three sats are two. The mint assigns the real denominations. Each secret is random for this melt. There is no seed to derive it again. After unblinding, the secret exists only inside the stored proof.

**The signatures that come back.** `keep_change` drops a signature whose amount is 0. A signature from another keyset, or more signatures than blanks, fails the unblind, and the pot stays `open`. The mint keys for the amounts that remain come from `GET /v1/keys/{id}`. `construct_proofs` turns those signatures into proofs. If Lightning used the whole surplus, the change list is empty and the pot is still `paid`.

**What is stored.** `finish_payout` in `server/src/db.rs` sets the pot to `paid` and inserts the `change_proofs` rows in one transaction. The pot response shows `change_sats`, the sum, and leaves the proof JSON out. The page shows that number. A download of the proofs is later, so until then the server holds them.

The database write starts after the mint has already paid. A `Pending` response, or a crash before `finish_payout` commits, can leave the inputs spent while the pot still says `open`. The blank secrets cannot be derived again.

## Trust model

v1, the first version that moves sats: the server key is the only pot key (`n_sigs = 1`). The server can spend the locked ecash by itself. That is custodial for theft. Refunds still work when the server is gone, because the mint accepts the contributor's refund key after the lock time. The server key for that version stays in a file outside this repo, mode 600. Payout reads that file only after the fee gate is open and the advertised deadline has not passed. It does not log the path or the private key. Health still reports only whether the path is set, whether a file is there, and whether the public key is set. A pot response includes `server_pubkey`, the compressed key a contribution locks to. It is null when that key is unset. Health does not include the key.

v2, the pitch: the organizer and the server both sign (`n_sigs = 2`). The server co-signs only when the locked total covers the target plus the melt fee reserve, and only for the destination stored at creation. Neither signature is enough on its own. If the organizer and the server both agree, they can still spend, including after the deadline, and they can win the race against refunds.

After the lock time the mint accepts both paths. It settles whichever valid spend it sees first. Contributors should reclaim as soon as the lock expires. The mint is trusted, as with any Cashu mint. Losing the refund key loses the refund. A secret with no refund key can be spent by anyone after the deadline. A malformed lock cannot be spent. The mint does not check that a secret belongs to this pot. The contributions route does that check, then asks the mint that the proof is signed and unspent, before it counts the sats.

A refund backup will be a file the browser downloads. A URL that contains the key would leak it. The reclaimed token is a second file. The private key is not in it.

## Decisions for the demo

- v1 proofs use `SIG_INPUTS`, so the CDK wallet melt helper can sign them. v2 uses `SIG_ALL`, which binds the signatures to one melt quote. `prepare_melt` / `prepare_with_proofs` reject `SIG_ALL`, so v2 will post a melt built with `sign_sig_all`.
- Unused melt fee reserve (NUT-08 change) goes to the organizer. The payout stores it. The pot page shows the amount. A download of the proofs is later. The section "How Vaca uses NUT-08" is that path.
- The mint lock time is the advertised deadline plus 600 seconds. The server stops co-signing at the advertised deadline, which is earlier than the moment the refund path opens. The post-deadline race remains.
- The cap is 100,000 sats. `VACA_MAX_POT_SATS` may lower it. It cannot raise it. `VACA_GRACE_SECONDS` must be 600 if it is set.

## Pins

The server pins the `cashu` crate `0.18.0` from crates.io, the version on CDK git revision `24360fe5fabda9d8aa3ae98b1a7e5e15986aad5a`. It does not path-depend on `~/projects/cdk-nutnov`, and it does not link the cdk crate.

The browser pin is `@cashu/cashu-ts@5.0.0-rc.12`, commit `4d6e1f86aaae0627bd4b06f4497da9fce0abe37b`. The page does not import the library. `dd8bbe3` is a later main commit, not this tag.

CDK deserializes a P2PK witness as a string of JSON. cashu-ts signs and returns an object. The contributions body rejects a witness field. A later melt still has to stringify a witness before the `cashu` crate reads it.

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

`VACA_BIND`, `VACA_DB_PATH`, `VACA_MINT_URL`, and `VACA_CORS_ORIGIN` override the defaults. Leave the mint URL empty until a mint exists. Creating a pot copies that URL onto the pot. Proof checks use the stored URL, so a later change of `VACA_MINT_URL` applies to pots created after the change. A pot from an older database receives the current URL once, on the first start that has one set. The sqlite file defaults to `data/vaca.sqlite` and is gitignored. The process logs whether a server-key path is set. It does not log the path or open the file. `VACA_SERVER_PUBKEY`, when set, is the compressed public key the server-key script printed. Startup stores that string. It does not open the key file to find the public key.

Creating a pot:

```bash
curl -sS -X POST http://127.0.0.1:8787/api/pots \
  -H 'content-type: application/json' \
  -d "{\"title\":\"Milk\",\"kind\":\"purchase\",\"target_sats\":1000,\"deadline_unix\":$(($(date +%s) + 86400)),\"destination\":\"lno1example\"}"
```

`deadline_unix` has to be in the future and within 366 days. Kinds are `cause`, `purchase`, and `split_bill`. Status stays `open`. A new pot reports `locked_sats` 0. An unknown JSON field, including `proofs`, is rejected on create.

The split-the-bill control on the page fills a two-hour deadline and labels the destination as the bill's BOLT11 invoice. It does not change payout rules.

## Regtest mint script

`scripts/regtest-mint.sh` prints `mint_started=no` and exits unless `VACA_START_MINT=1`. It targets CDK `24360fe5`.

On that revision, `cdk-mintd --config` is rejected. The operator imports `scripts/regtest-mint.config.toml` with `config init --new-mint`, then starts `cdk-mintd --work-dir`. The document sets `bitcoin_network = "regtest"`, `chain_source_type = "bitcoinrpc"`, RPC `127.0.0.1:18443`, and `backend = "ldk-node"`. Mnemonics and the RPC password are `env:` references. The script does not start bitcoind, and it does not build CDK. Regtest bitcoind is the ldk-lab one, RPC user `ldk`, on `127.0.0.1:18443`. Set `CDK_MINTD_LDK_BITCOIND_RPC_PASSWORD` to that lab password. The mint listens for Lightning on `0.0.0.0:8090` so a lab container can reach it. The mint and the demo stay on the host. Point `CDK_MINTD_BIN` at a binary built from that revision with the ldk-node feature. The checkout at `~/projects/cdk` is older than this CLI.

Mint files go under `~/.vaca/regtest-mint/` (override with `VACA_MINT_WORK_DIR` and `VACA_LDK_STORAGE_DIR`). Both directories must sit outside this repo.

## Dev: pay a mint quote from node A

A mint quote is a BOLT11 invoice. The mint's own Lightning node cannot pay it, and `bitcoin-cli` cannot pay it either. Lab node A is the payer. Node B stays stopped. These commands are for fish. They fund node A on the lab chain, open a channel to the mint, and pay one quote. A paid quote has `state` `PAID` and `amount_issued` `0`. It does not mint proofs or post a contribution.

Leave the mint process running. Do not start this repo's compose bitcoind beside the lab one. Both want port 18443. Do not run `~/projects/ldk-lab/scripts/fund`. That script waits for node B. Do not delete `~/projects/ldk-lab/data/a`. That directory is node A's wallet. If node A reports that the chain does not match, stop and read the error.

One chain, already listening:

```fish
cd ~/projects/ldk-lab
./scripts/compose up -d bitcoind
```

RPC user is `ldk`. Set `CDK_MINTD_LDK_BITCOIND_RPC_PASSWORD` to the password in the lab compose file. Do not print it.

Build `cdk-mintd` from `~/projects/cdk-nutnov` at `24360fe5` with `--features ldk-node`. `~/projects/cdk` is older. In the mint shell, set two different quoted 12-word phrases. Fish keeps only the first word of an unquoted value.

```fish
set -gx CDK_MINTD_BIN $HOME/projects/cdk-nutnov/target/debug/cdk-mintd
set -gx CDK_MINTD_MNEMONIC "word1 word2 word3 word4 word5 word6 word7 word8 word9 word10 word11 word12"
set -gx CDK_MINTD_LDK_NODE_MNEMONIC "word1 word2 word3 word4 word5 word6 word7 word8 word9 word10 word11 word12"
set -gx CDK_MINTD_LDK_BITCOIND_RPC_PASSWORD <lab rpc password>
echo (count (string split " " -- $CDK_MINTD_MNEMONIC))
echo (count (string split " " -- $CDK_MINTD_LDK_NODE_MNEMONIC))
```

Each count must print 12. From `~/projects/vaca`:

```fish
VACA_START_MINT=1 ./scripts/regtest-mint.sh
```

Cashu listens on `127.0.0.1:8085`. Lightning listens on `0.0.0.0:8090`. Fee-estimate lines that fall back to 1 sat/vb are expected on this regtest chain. Confirm the mint before paying:

```fish
curl -sS http://127.0.0.1:8085/v1/info | jq '.nuts["11"].supported, [.nuts["5"].methods[].method], .nuts["25"]'
```

Wanted: `true`, `bolt11` and `bolt12`, and `null`. A second start does not import the template again while `~/.vaca/regtest-mint/mintd/cdk-mintd.sqlite` exists. To recreate the mint on this chain, stop it and delete `~/.vaca/regtest-mint/ldk-node` plus `cdk-mintd.sqlite`, `cdk-mintd.sqlite-wal`, and `cdk-mintd.sqlite-shm`. Leave `~/.vaca/server-key.txt`.

Node A uses the lab's existing `data/a`. Leave `LDK_SERVER_DIR` unset so `./scripts/compose` builds `../ldk-server-review`. The first `up` compiles the image.

```fish
cd ~/projects/ldk-lab
./scripts/compose up -d node-a
./scripts/cli a get-node-info
./scripts/cli a get-balances
```

Wait until `get-node-info` answers and its height has caught the chain. If `spendable_onchain_balance_sats` is `0`, mine a mature coinbase, then check again:

```fish
./scripts/mine 101 a
./scripts/cli a get-balances
```

Copy the mint node id from the mint log line `Created node <id> with address`. The same Lightning mnemonic keeps the same id. Open a private channel and leave the balance on node A. From inside the container the mint is `host.docker.internal:8090`.

```fish
set mint_id <id from that line>
./scripts/cli a open-channel $mint_id host.docker.internal:8090 100000sat
./scripts/mine 6 a
./scripts/cli a list-channels
```

Repeat `list-channels` until `is_usable` is true. Create the quote after that, then pay the `request` field. Fish does not split `$invoice`.

```fish
set resp (curl -s -X POST http://127.0.0.1:8085/v1/mint/quote/bolt11 -H 'content-type: application/json' -d '{"amount":100,"unit":"sat"}')
set quote_id (echo $resp | python3 -c 'import json,sys; print(json.load(sys.stdin)["quote"])')
set invoice (echo $resp | python3 -c 'import json,sys; print(json.load(sys.stdin)["request"])')
./scripts/cli a bolt11-send $invoice
curl -sS http://127.0.0.1:8085/v1/mint/quote/bolt11/$quote_id
```

The quote object should show `amount` 100, `amount_paid` 100, `amount_issued` 0, and `state` `PAID`.

## Dev: mint a test token

A pot on this regtest mint needs a token from that mint. A normal wallet cannot mint one. `scripts/mint-test-token.mjs` asks the mint for a bolt11 quote, prints the invoice, and waits. Pay that invoice from lab node A with `./scripts/cli a bolt11-send`, from `~/projects/ldk-lab`, while the script is still running. When the quote is `PAID`, the script writes an unlocked token. It prints the amount and the path. It does not print the token.

```fish
cd ~/projects/vaca
node scripts/mint-test-token.mjs 100 ~/Downloads/vaca-test-token.cashu
```

The amount defaults to 100 sats. The path defaults to `~/Downloads/vaca-test-token.cashu`. `VACA_MINT_URL` defaults to `http://127.0.0.1:8085`. The file is mode 600. The script refuses to replace a file that already exists.

## Not in this skeleton

v2 co-signing comes later. The refund route returns proofs after the lock time. The page swaps them and downloads a token. The server does not spend them. NUT-08 change is stored on payout. "Pay together now" (NUT-15, several mints paying one invoice) is a later note in this README, after v1 and v2 work. Today's `cdk-ldk-node` reports `mpp: false`.

Local git only. No remote.

## Safety

Experimental. Regtest or Mutinynet only. Never log keys. Never put a key in a URL, a command line, or this repo.
