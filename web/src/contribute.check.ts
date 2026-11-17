import { deserializeProofs, serializeProofs, type Proof } from "@cashu/cashu-ts";
import {
  ContributeError,
  assertPotCanTakeProofs,
  classifyProofs,
  contributionAmount,
  contributionProofs,
  lockKeysetOk,
  potLock,
  proofsToken,
  readTokenProofs,
  sameMint,
  scrubContributeError,
  takeLockedProofs,
} from "./contribute.ts";
import { createSecret } from "./lock.ts";
import { generateRefundKey } from "./refund-key.ts";

const server = generateRefundKey();
const refund = generateRefundKey();
const other = generateRefundKey();
const locktime = 1_700_000_600;
const lock = {
  serverPubkey: server.pubkey,
  refundPubkey: refund.pubkey,
  locktimeUnix: locktime,
};

const options = potLock(lock);
if (Object.keys(options).join(",") !== "mainKeys,refundKeys,locktime") throw new Error("lock options");
if (options.mainKeys.length !== 1 || options.refundKeys.length !== 1) throw new Error("lock keys");
if ("sigAll" in options) throw new Error("sigAll");
console.log("lock options stay v1");

if (lockKeysetOk("02" + "ab".repeat(32))) throw new Error("v3 keyset was accepted");
if (!lockKeysetOk("01" + "ab".repeat(32))) throw new Error("v1 keyset was refused");
console.log("a v3 keyset is refused");

const open = { status: "open", mint_url: "http://127.0.0.1:9", deadline_unix: 5_000, sats_still_needed: 20 };
assertPotCanTakeProofs(open, 5_000);
expectError(() => assertPotCanTakeProofs({ ...open, status: "paid" }, 5_000), "This pot is already paid.");
expectError(() => assertPotCanTakeProofs(open, 5_001), "The deadline has passed.");
expectError(
  () => assertPotCanTakeProofs({ ...open, sats_still_needed: 0 }, 5_000),
  "This pot already holds the target plus the fee reserve.",
);
console.log("a closed pot is refused before a swap");

if (contributionAmount(50, 20) !== 20) throw new Error("amount was not capped");
if (contributionAmount(5, 20) !== 5) throw new Error("small token was capped");
console.log("amount stops at the sats still needed");

const here = oneProof(createSecret(server.pubkey, refund.pubkey, locktime));
const elsewhere = oneProof(createSecret(server.pubkey, other.pubkey, locktime));
const unlocked = oneProof("plain-secret");
if (classifyProofs([unlocked], lock) !== "unlocked") throw new Error("unlocked");
if (classifyProofs([here], lock) !== "this-pot") throw new Error("this pot");
if (classifyProofs([elsewhere], lock) !== "other-lock") throw new Error("other lock");
if (classifyProofs([here, unlocked], lock) !== "mixed") throw new Error("mixed");
console.log("only an unlocked token is swapped");

const posted = contributionProofs([here]);
const body = JSON.stringify({ refund_pubkey: refund.pubkey, proofs: posted });
if (body.includes("witness")) throw new Error("witness was posted");
if (!posted[0] || posted[0].dleq.r === "") throw new Error("dleq was dropped");
if (body.includes(refund.privkey)) throw new Error("private key was posted");
console.log("the posted proof has no witness");

expectError(() => takeLockedProofs([here], lock, 5), "The mint returned a different amount.");
expectError(() => takeLockedProofs([elsewhere], lock, 4), "The mint did not return this pot's lock.");
if (takeLockedProofs([here], lock, 4)[0]?.amount !== 4) throw new Error("locked amount");
console.log("a matching lock is accepted");

const file = JSON.stringify(serializeProofs([here]));
const read = await readTokenProofs("http://127.0.0.1:9", file);
if (read.proofs.length !== 1 || read.mint !== null) throw new Error("json token");
if (!sameMint("http://127.0.0.1:8085/", "http://127.0.0.1:8085")) throw new Error("slash");
if (sameMint("http://mint.example", "http://127.0.0.1:8085")) throw new Error("other mint");
console.log("a json token is read without the mint");

const token = proofsToken("http://127.0.0.1:9", [here], "vaca-keep.cashu");
if (!token.contents.startsWith("cashuB")) throw new Error("token encoding");
if (token.filename !== "vaca-keep.cashu") throw new Error("token filename");
if (token.contents.includes(refund.privkey)) throw new Error("token contains the private key");
console.log("the remainder token omits the private key");

const leaked = scrubContributeError(new Error(here.secret), [here], "", "The token could not be read.");
if (leaked.message !== "The token could not be read." || leaked.message.includes(here.secret)) {
  throw new Error("secret leaked");
}
console.log("a mint error does not repeat the secret");

function oneProof(secret: string): Proof {
  const point = generateRefundKey();
  const [proof] = deserializeProofs([
    {
      amount: 4,
      id: "00" + "11".repeat(32),
      secret,
      C: point.pubkey,
      dleq: { e: "ab".repeat(32), s: "cd".repeat(32), r: "ef".repeat(32) },
      witness: { signatures: ["aa"] },
    },
  ]);
  if (!proof) throw new Error("proof");
  return proof;
}

function expectError(run: () => void, message: string): void {
  try {
    run();
  } catch (err) {
    if (err instanceof ContributeError && err.message === message) return;
    throw err;
  }
  throw new Error("missing " + message);
}
