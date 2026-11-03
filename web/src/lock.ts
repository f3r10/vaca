import { createP2PKsecret, parseP2PKSecret } from "@cashu/cashu-ts"; 

export function createSecret(serverPubkey: string, refundPubkey: string, locktimeUnix: number): string {
	const tags: string[][] = [
		["locktime", String(locktimeUnix)],
		["refund", refundPubkey], 
	]
	const secret = createP2PKsecret(serverPubkey, tags);
	const [kind, body] = parseP2PKSecret(secret);
	if (kind !== "P2PK") throw new Error("kind");
	if (body.data !== serverPubkey) throw new Error("data");
	const tags_parsed = body.tags ?? [];
	if (tags_parsed.length !== 2) throw new Error("tags");
	if (tags_parsed[0][0] !== "locktime" || tags_parsed[0][1] !== String(locktimeUnix)) throw new Error("locktime");
	if (tags_parsed[1][0] !== "refund" || tags_parsed[1][1] !== refundPubkey) throw new Error("refund");
	return secret
}
