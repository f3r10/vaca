import {
  Wallet,
  deserializeProofs,
  findCashuPayload,
  getDecodedToken,
  getEncodedToken,
  isBlsKeyset,
  parseP2PKSecret,
  serializeProofs,
  sumProofs,
  type Proof,
  type ProofLike,
} from "@cashu/cashu-ts";
import { secretMatchesPot } from "./lock.ts";

export class ContributeError extends Error {
  constructor(message: string) {
    super(message);
    this.name = "ContributeError";
  }
}

export interface ContributionProof {
  amount: number;
  id: string;
  secret: string;
  C: string;
  dleq: { e: string; s: string; r: string };
}

export interface PotLock {
  serverPubkey: string;
  refundPubkey: string;
  locktimeUnix: number;
}

// The v1 lock, and nothing else. sigAll, a second main key, or an extra tag
// would make secretMatchesPot reject the proof after the mint had already
// taken the inputs. Absent sigflag means SIG_INPUTS.
export function potLock(lock: PotLock): {
  mainKeys: string[];
  refundKeys: string[];
  locktime: number;
} {
  return {
    mainKeys: [lock.serverPubkey],
    refundKeys: [lock.refundPubkey],
    locktime: lock.locktimeUnix,
  };
}

// A v3 keyset (id prefix 02) encodes a nutroot, not the NUT-10 secret the
// server counts. Refuse before the swap so the token is still unspent.
export function lockKeysetOk(keysetId: string): boolean {
  return !isBlsKeyset(keysetId);
}

export function assertPotCanTakeProofs(
  pot: { status: string; mint_url: string; deadline_unix: number; sats_still_needed: number },
  nowUnix: number,
): void {
  if (pot.status === "paid") {
    throw new ContributeError("This pot is already paid.");
  }
  if (pot.mint_url.trim() === "") {
    throw new ContributeError("Mint URL is not set.");
  }
  if (nowUnix > pot.deadline_unix) {
    throw new ContributeError("The deadline has passed.");
  }
  if (!Number.isSafeInteger(pot.sats_still_needed) || pot.sats_still_needed < 1) {
    throw new ContributeError("This pot already holds the target plus the fee reserve.");
  }
}

// Lock at most the amount the pot will still accept. A larger swap would
// spend the token and then the server would reject the post.
export function contributionAmount(tokenSats: number, stillNeeded: number): number {
  if (!Number.isSafeInteger(tokenSats) || tokenSats < 1) {
    throw new ContributeError("The token has no sats.");
  }
  if (!Number.isSafeInteger(stillNeeded) || stillNeeded < 1) {
    throw new ContributeError("This pot already holds the target plus the fee reserve.");
  }
  return Math.min(tokenSats, stillNeeded);
}

export type ProofClass = "unlocked" | "this-pot" | "other-lock" | "mixed";

// Unlocked proofs are swapped. Proofs already locked to this pot are posted
// as they are. Anything else is refused before a mint call.
export function classifyProofs(proofs: Proof[], lock: PotLock): ProofClass {
  let unlocked = 0;
  let here = 0;
  let other = 0;
  for (const proof of proofs) {
    const kind = proofClass(proof, lock);
    if (kind === "unlocked") unlocked += 1;
    else if (kind === "this-pot") here += 1;
    else other += 1;
  }
  if (other > 0) return "other-lock";
  if (unlocked > 0 && here > 0) return "mixed";
  if (here > 0) return "this-pot";
  return "unlocked";
}

// The contribution body. A witness is left out: the server rejects that field.
// dleq.r stays, because the server's NUT-12 check needs it.
export function contributionProofs(proofs: Proof[]): ContributionProof[] {
  if (proofs.length === 0) {
    throw new ContributeError("The proof has no signature.");
  }
  return proofs.map((proof) => {
    const amount = proof.amount.toNumber();
    const dleq = proof.dleq;
    if (
      !Number.isSafeInteger(amount) ||
      amount < 1 ||
      proof.id === "" ||
      proof.secret === "" ||
      proof.C === "" ||
      !dleq ||
      dleq.e === "" ||
      dleq.s === "" ||
      dleq.r === ""
    ) {
      throw new ContributeError("The proof has no signature.");
    }
    return {
      amount,
      id: proof.id,
      secret: proof.secret,
      C: proof.C,
      dleq: { e: dleq.e, s: dleq.s, r: dleq.r },
    };
  });
}

/** NUT-02: ceil(proofCount * inputFeePpk / 1000). No proofs, no fee. */
export function inputFeeSats(inputFeePpk: number, proofCount: number): number {
  if (
    !Number.isSafeInteger(inputFeePpk) ||
    inputFeePpk < 0 ||
    !Number.isSafeInteger(proofCount) ||
    proofCount < 0
  ) {
    throw new ContributeError("The mint did not say what a proof costs.");
  }
  if (inputFeePpk === 0 || proofCount === 0) return 0;
  const fee = (BigInt(inputFeePpk) * BigInt(proofCount) + 999n) / 1000n;
  if (fee > BigInt(Number.MAX_SAFE_INTEGER)) {
    throw new ContributeError("The mint returned a different amount.");
  }
  return Number(fee);
}

// The outputs must be this pot's lock. includeFees may add up to one input
// fee for these proofs on top of the amount the page asked to lock.
export function takeLockedProofs(
  proofs: Proof[],
  lock: PotLock,
  amount: number,
  inputFeePpk: number,
): ContributionProof[] {
  const sum = sumProofs(proofs).toNumber();
  const allowance = inputFeeSats(inputFeePpk, proofs.length);
  if (!Number.isSafeInteger(amount) || sum < amount || sum > amount + allowance) {
    throw new ContributeError("The mint returned a different amount.");
  }
  for (const proof of proofs) {
    if (!secretMatchesPot(lock.serverPubkey, lock.refundPubkey, lock.locktimeUnix, proof.secret)) {
      throw new ContributeError("The mint did not return this pot's lock.");
    }
  }
  return contributionProofs(proofs);
}

export async function readTokenProofs(
  mintUrl: string,
  text: string,
): Promise<{ proofs: Proof[]; mint: string | null; unit: string | null }> {
  const trimmed = text.trim();
  if (trimmed === "") {
    throw new ContributeError("The token could not be read.");
  }
  const found = findCashuPayload(trimmed);
  if (found?.kind === "token") {
    if (mintUrl.trim() === "") {
      throw new ContributeError("Mint URL is not set.");
    }
    try {
      const wallet = new Wallet(mintUrl);
      await wallet.loadMint();
      const token = getDecodedToken(found.payload, wallet.keyChain.getAllKeysetIds());
      if (token.proofs.length === 0) {
        throw new ContributeError("The token could not be read.");
      }
      return { proofs: token.proofs, mint: token.mint, unit: token.unit ?? null };
    } catch (err) {
      throw scrubContributeError(err, [], "", "The token could not be read.");
    }
  }
  try {
    const parsed: unknown = JSON.parse(trimmed);
    const proofs = deserializeProofs(parsed as string | string[] | ProofLike[]);
    if (proofs.length === 0) {
      throw new ContributeError("The token could not be read.");
    }
    return { proofs, mint: null, unit: null };
  } catch (err) {
    if (err instanceof ContributeError) throw err;
    throw new ContributeError("The token could not be read.");
  }
}

export function sameMint(tokenMint: string | null, potMint: string): boolean {
  if (tokenMint === null || tokenMint.trim() === "") return true;
  return stripSlash(tokenMint) === stripSlash(potMint);
}

// Swap unlocked proofs into this pot's lock. No private key: these inputs are
// not locked, and a privkey must not leave the refund file. keepAsRandom leaves
// the remainder unlocked, with no seed, so that file is the only copy. The
// wallet puts unselected inputs into keep as well.
export async function lockForPot(
  mintUrl: string,
  proofs: Proof[],
  amount: number,
  lock: PotLock,
): Promise<{ send: Proof[]; keep: Proof[] }> {
  if (mintUrl.trim() === "") {
    throw new ContributeError("Mint URL is not set.");
  }
  const wallet = new Wallet(mintUrl);
  try {
    await wallet.loadMint();
    if (!lockKeysetOk(wallet.keysetId)) {
      throw new ContributeError("This mint's keyset cannot hold this pot's lock.");
    }
    // includeFees puts the next spend's input fee inside the locked proofs.
    // The pot counts that extra. A later reclaim can return the amount asked
    // for. This swap still spends one input fee from the original token, and
    // that sat is not in the pot.
    const { send, keep } = await wallet.ops
      .send(amount, proofs)
      .asLocked(potLock(lock))
      .keepAsRandom()
      .includeFees(true)
      .run();
    if (send.length === 0) {
      throw new ContributeError(mintKeptToken);
    }
    return { send, keep };
  } catch (err) {
    throw scrubContributeError(err, proofs, "", mintKeptToken);
  }
}

// The downloaded file keeps dleq. A later post needs r, and the remainder has
// to be spendable. Encoding falls back to the proof JSON when the keyset id
// is not hex.
export function proofSats(proofs: Proof[]): number {
  return sumProofs(proofs).toNumber();
}

export function proofsToken(
  mintUrl: string,
  proofs: Proof[],
  filename: string,
): { filename: string; contents: string } {
  try {
    return {
      filename,
      contents: getEncodedToken({ mint: mintUrl, proofs, unit: "sat" }),
    };
  } catch {
    return {
      filename: filename.replace(/\.cashu$/, ".json"),
      contents: JSON.stringify(serializeProofs(proofs)),
    };
  }
}

const mintKeptToken =
  "The mint did not return locked proofs. If it already took the token, those sats cannot be locked again.";

export function scrubContributeError(
  err: unknown,
  proofs: Proof[],
  privkey: string,
  fallback: string,
): ContributeError {
  if (err instanceof ContributeError) return err;
  const message = err instanceof Error ? err.message : "";
  const leaked =
    (privkey !== "" && message.includes(privkey)) ||
    proofs.some((proof) => message.includes(proof.secret) || (proof.C !== "" && message.includes(proof.C)));
  if (leaked || message.length === 0 || message.length > 200) {
    return new ContributeError(fallback);
  }
  return new ContributeError(message);
}

function proofClass(proof: Proof, lock: PotLock): "unlocked" | "this-pot" | "other-lock" {
  if (secretMatchesPot(lock.serverPubkey, lock.refundPubkey, lock.locktimeUnix, proof.secret)) {
    return "this-pot";
  }
  try {
    const [kind] = parseP2PKSecret(proof.secret);
    if (kind === "P2PK") return "other-lock";
  } catch {
    return "unlocked";
  }
  return "unlocked";
}

function stripSlash(url: string): string {
  return url.trim().replace(/\/+$/, "");
}
