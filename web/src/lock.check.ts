import { createP2PKsecret, parseP2PKSecret } from "@cashu/cashu-ts";
import { createSecret, secretMatchesPot } from "./lock.ts";

const serverPubkey = "02" + "11".repeat(32);
const refundPubkey = "03" + "22".repeat(32);
const locktimeUnix = 1_800_000_000;

const first = createSecret(serverPubkey, refundPubkey, locktimeUnix);
const second = createSecret(serverPubkey, refundPubkey, locktimeUnix);

const nonceA = parseP2PKSecret(first)[1].nonce;
const nonceB = parseP2PKSecret(second)[1].nonce;
if (nonceA === nonceB) throw new Error("same nonce");
console.log("nonce differs");

if (!secretMatchesPot(serverPubkey, refundPubkey, locktimeUnix, first)) {
	throw new Error("createSecret was rejected");
}

const flipped = createSecret(
	"03" + "11".repeat(32),
	"02" + "22".repeat(32),
	locktimeUnix,
);
if (!secretMatchesPot(serverPubkey, refundPubkey, locktimeUnix, flipped)) {
	throw new Error("same x-coordinate was rejected");
}

const reversed = createP2PKsecret(serverPubkey, [
	["refund", refundPubkey],
	["locktime", String(locktimeUnix)],
]);
if (!secretMatchesPot(serverPubkey, refundPubkey, locktimeUnix, reversed)) {
	throw new Error("reversed tags were rejected");
}

const rejects: Array<[string, string]> = [
	["wrong data", createP2PKsecret("02" + "44".repeat(32), [
		["locktime", String(locktimeUnix)],
		["refund", refundPubkey],
	])],
	["wrong refund", createP2PKsecret(serverPubkey, [
		["locktime", String(locktimeUnix)],
		["refund", "02" + "55".repeat(32)],
	])],
	["wrong locktime", createP2PKsecret(serverPubkey, [
		["locktime", String(locktimeUnix + 1)],
		["refund", refundPubkey],
	])],
	["missing refund", createP2PKsecret(serverPubkey, [
		["locktime", String(locktimeUnix)],
	])],
	["extra pubkey", createP2PKsecret(serverPubkey, [
		["locktime", String(locktimeUnix)],
		["refund", refundPubkey],
		["pubkeys", "02" + "33".repeat(32)],
	])],
	["sig all", createP2PKsecret(serverPubkey, [
		["locktime", String(locktimeUnix)],
		["refund", refundPubkey],
		["sigflag", "SIG_ALL"],
	])],
	["explicit sig inputs", createP2PKsecret(serverPubkey, [
		["locktime", String(locktimeUnix)],
		["refund", refundPubkey],
		["sigflag", "SIG_INPUTS"],
	])],
	["not a secret", "nope"],
];

for (const [name, secret] of rejects) {
	if (secretMatchesPot(serverPubkey, refundPubkey, locktimeUnix, secret)) {
		throw new Error(`accepted ${name}`);
	}
}
console.log("pot match holds");
