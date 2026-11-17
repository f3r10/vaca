import { signP2PKProofs } from "@cashu/cashu-ts";
import { createSecret } from "./lock.ts";
import { generateRefundKey, privkeyFromRefundFile, pubkeyFromRefundFile, refundKeyFile, refundKeyMatches } from "./refund-key.ts";
import { RefundSwapError, assertRefundCanSign, readRefundProofs, refundToken } from "./refund-swap.ts";

const key = generateRefundKey();
const file = refundKeyFile(key);
const pubkey = pubkeyFromRefundFile(file);
const privkey = privkeyFromRefundFile(file);
if (pubkey !== key.pubkey.toLowerCase()) throw new Error("pubkey line");
if (privkey !== key.privkey.toLowerCase()) throw new Error("privkey line");
if (pubkeyFromRefundFile(file) === privkey) throw new Error("pubkey helper returned the private key");
if (!refundKeyMatches(pubkey!, privkey!)) throw new Error("key pair did not match");
const flipped = (pubkey!.startsWith("02") ? "03" : "02") + pubkey!.slice(2);
if (refundKeyMatches(flipped, privkey!)) throw new Error("flipped prefix matched");
if (privkeyFromRefundFile("pubkey " + pubkey) !== null) throw new Error("pubkey line was read as a private key");
console.log("file lines stay apart");

const other = generateRefundKey();
if (refundKeyMatches(pubkey!, other.privkey)) throw new Error("another key matched");
console.log("another key does not match");

const server = generateRefundKey();
const point = generateRefundKey();
const now = Math.floor(Date.now() / 1000);
const futureLock = now + 3600;
const pastLock = now - 10;
const futureSecret = createSecret(server.pubkey, key.pubkey, futureLock);
const pastSecret = createSecret(server.pubkey, key.pubkey, pastLock);
const future = oneProof(futureSecret, point.pubkey);
const past = oneProof(pastSecret, point.pubkey);

expectRefundError(() => assertRefundCanSign([future], key.privkey, futureLock), "The refund lock has not expired.");
assertRefundCanSign([future], key.privkey, futureLock + 1);
expectRefundError(() => assertRefundCanSign([past], other.privkey, now), "No proof is locked to that refund key.");
console.log("refund leaf waits, then opens");

const early = signP2PKProofs([future], key.privkey)[0];
if (early?.witness) throw new Error("signed before the lock");
const signed = signP2PKProofs([past], key.privkey)[0];
const signatures = witnessSignatures(signed?.witness);
if (signatures !== 1) throw new Error("signature missing after the lock");
console.log("signature added only after the lock");

const token = refundToken("http://127.0.0.1:8085", readRefundProofs([proofJson(pastSecret, point.pubkey)]));
if (!token.contents.startsWith("cashuB")) throw new Error("token encoding");
if (token.contents.includes(key.privkey)) throw new Error("token contains the private key");
if (token.filename !== "vaca-refund.cashu") throw new Error("token filename");
console.log("token omits the private key");

function oneProof(secret: string, c: string) {
  const [proof] = readRefundProofs([proofJson(secret, c)]);
  if (!proof) throw new Error("proof");
  return proof;
}

function proofJson(secret: string, c: string): unknown {
  return {
    amount: 4,
    id: "00" + "11".repeat(32),
    secret,
    C: c,
  };
}

function witnessSignatures(witness: unknown): number {
  if (!witness || typeof witness !== "object") return 0;
  const signatures = (witness as { signatures?: unknown }).signatures;
  return Array.isArray(signatures) ? signatures.length : 0;
}

function expectRefundError(run: () => void, message: string): void {
  try {
    run();
  } catch (err) {
    if (err instanceof RefundSwapError && err.message === message) return;
    throw err;
  }
  throw new Error("missing " + message);
}
