import { serializeProofs, sumProofs, Wallet } from "@cashu/cashu-ts"
import { readFileSync, writeFileSync } from "node:fs";
import { secretMatchesPot } from "./lock.ts";

const wallet = new Wallet("http://127.0.0.1:8085");
await wallet.loadMint();

const quoteId =  "01a1b0c4-64c8-762a-9dd3-bfabf0558f58";
const minted = await wallet.mintProofsBolt11(100, quoteId);

if (sumProofs(minted).toNumber() !== 100) {
  throw new Error("minted sum");
}
const amounts = minted.map((proof) => proof.amount.toNumber());
console.log("minted", amounts.join(" "))

writeFileSync(
  `${process.env.HOME}/.vaca/minted-proofs.json`,
  JSON.stringify(serializeProofs(minted)),
  {mode: 0o600, flag: "wx"},
);

function pubkeyFrom(path: string): string {
  const line = readFileSync(path, "utf8").split("\n").find((item) => item.startsWith("pubkey "));
  if (line === undefined) throw new Error("pubkey line");
  return line.slice("pubkey ".length).trim();
}

const serverPubkey = pubkeyFrom(`${process.env.HOME}/.vaca/server-key.txt`);
const refundPubkey = pubkeyFrom("/Users/f3r10/Downloads/vaca-refund-key.txt");
const locktimeUnix = 1794357360;

const { keep, send } = await wallet.send(100, minted, { includeFees: true }, {
  send: {
    type: "lock",
    options: {
      mainKeys: [serverPubkey],
      refundKeys: [refundPubkey],
      locktime: locktimeUnix,
    },
  },
});


if (keep.length !== 0) throw new Error("keep");
if (sumProofs(send).toNumber() !== 100) throw new Error("locked sum");
for (const proof of send) {
  if (!secretMatchesPot(serverPubkey, refundPubkey, locktimeUnix, proof.secret)) {
    throw new Error("lock");
  }
}
console.log("locked", send.map((proof) => proof.amount.toNumber()).join(" "));
writeFileSync(
  `${process.env.HOME}/.vaca/locked-proofs.json`,
  JSON.stringify(serializeProofs(send)),
  { mode: 0o600, flag: "wx" },
);
