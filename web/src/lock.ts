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

// True only for a v1 pot lock: server key in data, contributor key in refund,
// locktime equal to the pot, and no other tag. 02 and 03 with the same
// x-coordinate are the same key. A throw from the parser is a reject.
export function secretMatchesPot(
	serverPubkey: string,
	refundPubkey: string,
	locktimeUnix: number,
	secret: string,
): boolean {
	if (!Number.isSafeInteger(locktimeUnix) || locktimeUnix < 0) return false
	const serverX = pubkeyX(serverPubkey)
	const refundX = pubkeyX(refundPubkey)
	if (serverX === null || refundX === null) return false
	let parsed: ReturnType<typeof parseP2PKSecret>
	try {
		parsed = parseP2PKSecret(secret)
	} catch {
		return false
	}
	const [kind, body] = parsed
	if (kind !== "P2PK") return false
	if (pubkeyX(body.data) !== serverX) return false
	const tags = body.tags ?? []
	if (tags.length !== 2) return false
	const locktime = singleTag(tags, "locktime")
	const refund = singleTag(tags, "refund")
	if (locktime !== String(locktimeUnix)) return false
	if (refund === null || pubkeyX(refund) !== refundX) return false
	return true
}

function pubkeyX(pubkey: string): string | null {
	const trimmed = pubkey.trim().toLowerCase()
	if (!/^(02|03)[0-9a-f]{64}$/.test(trimmed)) return null
	return trimmed.slice(2)
}

function singleTag(tags: string[][], name: string): string | null {
	const matches = tags.filter((tag) => tag[0] === name)
	if (matches.length !== 1) return null
	const [, value, ...rest] = matches[0]
	if (value === undefined || value.length === 0 || rest.length !== 0) return null
	return value
}
