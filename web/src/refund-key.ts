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
