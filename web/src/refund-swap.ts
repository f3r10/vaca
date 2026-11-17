import {
  Wallet,
  deserializeProofs,
  getEncodedToken,
  serializeProofs,
  sumProofs,
  type Proof,
  type ProofLike,
} from "@cashu/cashu-ts";

export class RefundSwapError extends Error {
  constructor(message: string) {
    super(message);
    this.name = "RefundSwapError";
  }
}

export function readRefundProofs(raw: unknown): Proof[] {
  if (!Array.isArray(raw)) {
    throw new RefundSwapError("The stored proof could not be read.");
  }
  let proofs: Proof[];
  try {
    proofs = deserializeProofs(raw as ProofLike[]);
  } catch {
    throw new RefundSwapError("The stored proof could not be read.");
  }
  if (proofs.length === 0) {
    throw new RefundSwapError("The stored proof could not be read.");
  }
  return proofs;
}

// The refund leaf is satisfiable only when this key is on it and the locktime
// is in the past. The exact locktime second is still the pot-key path.
export function assertRefundCanSign(proofs: Proof[], privkey: string, nowUnix: number): void {
  // spendOptions does not contact this URL.
  const wallet = new Wallet("http://127.0.0.1");
  for (const proof of proofs) {
    let options;
    try {
      options = wallet.spendOptions(proof, { privkeys: privkey, now: nowUnix });
    } catch {
      throw new RefundSwapError("The stored proof could not be read.");
    }
    const refundLeaf = options.script.find((entry) => entry.leaf.type === "after");
    if (!refundLeaf || refundLeaf.keys.length === 0) {
      throw new RefundSwapError("No proof is locked to that refund key.");
    }
    if (!refundLeaf.satisfiable) {
      throw new RefundSwapError("The refund lock has not expired.");
    }
  }
}

export async function swapRefundProofs(
  mintUrl: string,
  proofs: Proof[],
  privkey: string,
): Promise<Proof[]> {
  if (mintUrl.trim() === "") {
    throw new RefundSwapError("Mint URL is not set.");
  }
  assertRefundCanSign(proofs, privkey, Math.floor(Date.now() / 1000));
  const wallet = new Wallet(mintUrl);
  try {
    await wallet.loadMint();
    // Legacy NUT-11. planScriptPaths skips these proofs. receive().privkey()
    // adds the refund signature once the lock is expired. A script-path plan
    // is for a v3 tree and rejects this secret. asRandom() leaves the new
    // proofs unlocked, so the downloaded token is the only copy.
    const fresh = await wallet.ops.receive(proofs).privkey(privkey).asRandom().run();
    if (fresh.length === 0) {
      throw new RefundSwapError(mintKeptProofs);
    }
    return fresh;
  } catch (err) {
    throw scrubSwapError(err, proofs, privkey);
  }
}

export function refundSats(proofs: Proof[]): number {
  return sumProofs(proofs).toNumber();
}

export function refundToken(mintUrl: string, proofs: Proof[]): { filename: string; contents: string } {
  const bearer = proofs.map((proof) => {
    const copy: Proof = { ...proof };
    delete copy.dleq;
    return copy;
  });
  try {
    return {
      filename: "vaca-refund.cashu",
      contents: getEncodedToken({ mint: mintUrl, proofs: bearer, unit: "sat" }, { removeDleq: true }),
    };
  } catch {
    return {
      filename: "vaca-refund-proofs.json",
      contents: JSON.stringify(serializeProofs(bearer)),
    };
  }
}

const mintKeptProofs =
  "The mint did not return fresh proofs. If it already took the locked proofs, they cannot be swapped again.";

function scrubSwapError(err: unknown, proofs: Proof[], privkey: string): RefundSwapError {
  if (err instanceof RefundSwapError) return err;
  const message = err instanceof Error ? err.message : "";
  const leaked =
    message.includes(privkey) ||
    proofs.some((proof) => message.includes(proof.secret) || (proof.C !== "" && message.includes(proof.C)));
  if (leaked || message.length === 0 || message.length > 200) {
    return new RefundSwapError(mintKeptProofs);
  }
  return new RefundSwapError(message);
}
