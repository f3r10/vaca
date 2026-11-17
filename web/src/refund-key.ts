import {secp256k1 } from "@noble/curves/secp256k1.js";
import { bytesToHex } from "@cashu/cashu-ts";

export type RefundKey = {
  pubkey: string;
  privkey: string;
};


export function generateRefundKey(): RefundKey {
	const privkeyBytes = secp256k1.utils.randomSecretKey();

	// NUT-11 requires the compressed form.
	const pubkeyBytes = secp256k1.getPublicKey(privkeyBytes, true);
	
	const key: RefundKey = {
		pubkey: bytesToHex(pubkeyBytes),
		privkey: bytesToHex(privkeyBytes),
	};
	return key;
}

export function refundKeyFile(key: RefundKey): string {
	return `pubkey ${key.pubkey}\nprivkey ${key.privkey}`;
}

// The public key line only. The private key stays in the file the browser read.
export function pubkeyFromRefundFile(contents: string): string | null {
	for (const line of contents.split("\n")) {
		const parts = line.trim().split(/\s+/)
		if (parts.length >= 2 && parts[0] === "pubkey") {
			return parts[1].toLowerCase()
		}
	}
	return null
}

// The private key line only. Callers may sign with it and must not send it.
export function privkeyFromRefundFile(contents: string): string | null {
	for (const line of contents.split("\n")) {
		const parts = line.trim().split(/\s+/);
		if (parts.length >= 2 && parts[0] === "privkey" && /^[0-9a-fA-F]{64}$/.test(parts[1])) {
			return parts[1].toLowerCase();
		}
	}
	return null;
}

// True when the private key's compressed pubkey is the public key from the file.
export function refundKeyMatches(pubkey: string, privkey: string): boolean {
	if (!/^(02|03)[0-9a-f]{64}$/.test(pubkey)) return false;
	if (!/^[0-9a-f]{64}$/.test(privkey)) return false;
	try {
		const derived = bytesToHex(secp256k1.getPublicKey(hexToBytes(privkey), true));
		return derived.toLowerCase() === pubkey.toLowerCase();
	} catch {
		return false;
	}
}

function hexToBytes(hex: string): Uint8Array {
	const bytes = new Uint8Array(hex.length / 2);
	for (let i = 0; i < bytes.length; i += 1) {
		bytes[i] = Number.parseInt(hex.slice(i * 2, i * 2 + 2), 16);
	}
	return bytes;
}
