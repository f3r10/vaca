import { parseP2PKSecret } from "@cashu/cashu-ts";
import { createSecret } from "./lock.ts";

const serverPubkey = "02" + "11".repeat(32);
const refundPubkey = "03" + "22".repeat(32);
const locktimeUnix = 1_800_000_000;

const first = createSecret(serverPubkey, refundPubkey, locktimeUnix);
const second = createSecret(serverPubkey, refundPubkey, locktimeUnix);

const nonceA = parseP2PKSecret(first)[1].nonce;
const nonceB = parseP2PKSecret(second)[1].nonce;
if (nonceA === nonceB) throw new Error("same nonce");
console.log("nonce differs");
