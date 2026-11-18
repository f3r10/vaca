// Paying a contribution with Lightning instead of a ready-made Cashu token.
//
// The idea, in four steps:
//   1. Ask the pot's mint for a Lightning invoice (a "mint quote").
//   2. The visitor pays it with any Lightning wallet. On a test mint it pays itself.
//   3. Once the mint says PAID, we mint ecash for that amount.
//   4. contribute.ts then locks that ecash to the pot, exactly as it does for a pasted token.
//
// This file does steps 1 to 3 and the fee arithmetic. It has no UI and does not
// touch the DOM. It never stores, logs or sends the quote key.

import { MintQuoteState, Wallet, sumProofs, type MintQuoteBolt11Response, type Proof } from "@cashu/cashu-ts";
import { ContributeError, scrubContributeError } from "./contribute.ts";

/* ------------------------------------------------------------------ */
/* Fee arithmetic (pure: no network, easy to test)                     */
/* ------------------------------------------------------------------ */

// A mint charges an "input fee" every time proofs are spent. It is quoted in
// parts per thousand (ppk) per proof: 100 ppk is 0.1 sat per proof, rounded up
// for the whole batch (NUT-02).
export function feeFor(proofCount: number, inputFeePpk: number): number {
  return Math.ceil((proofCount * inputFeePpk) / 1000);
}

// How many bits it takes to write n in binary. Ecash amounts are powers of two,
// so a value n is split into at most this many proofs.
function bitLength(n: number): number {
  return n.toString(2).length;
}

export interface MintPlan {
  /** What the visitor asked to contribute. */
  amount: number;
  /** What ends up locked in the pot: amount plus the fee for spending it later. */
  lockedTotal: number;
  /** What the page must mint (and the visitor must pay) so the lock swap still has enough. */
  mintAmount: number;
  /** mintAmount minus amount: the mint's fees, shown to the visitor. */
  feeSats: number;
}

// Work out how many sats to mint so that `amount` can be locked.
//
// Two fees sit on top of the amount, both measured against what the real runs
// on the test mint did (ask for 11, lock 12, mint 13 at 100 ppk):
//
//   a) The lock includes the fee of spending it later (includeFees in
//      lockForPot). That is why a pot counts slightly more than you asked for,
//      and why a later refund still returns the full amount.
//   b) The lock swap itself spends the freshly minted proofs and pays their
//      input fee.
//
// Both are estimated from upper bounds on how many proofs there will be, so we
// tend to mint a sat too many rather than a sat too few. A sat too few would
// leave minted ecash that cannot be locked. A sat too many just comes back as
// a small remainder.
export function planMint(amount: number, inputFeePpk: number): MintPlan {
  if (!Number.isSafeInteger(amount) || amount < 1) {
    throw new ContributeError("Enter a whole number of sats.");
  }
  if (!Number.isSafeInteger(inputFeePpk) || inputFeePpk < 0) {
    throw new ContributeError("The mint did not say what a proof costs.");
  }
  // (a) The locked total L satisfies L = amount + fee(spending L). The number
  // of locked proofs is unknown in advance, so use bitLength(L) + 2 as a bound.
  // L appears on both sides, so repeat until it stops changing (it settles fast).
  let lockedTotal = amount;
  for (let i = 0; i < 10; i += 1) {
    const next = amount + feeFor(bitLength(lockedTotal) + 2, inputFeePpk);
    if (next === lockedTotal) break;
    lockedTotal = next;
  }
  // (b) The smallest mint amount M where, after the swap fee on up to
  // bitLength(M) input proofs, at least L remains.
  let mintAmount = lockedTotal;
  while (mintAmount - feeFor(bitLength(mintAmount), inputFeePpk) < lockedTotal) {
    mintAmount += 1;
  }
  return { amount, lockedTotal, mintAmount, feeSats: mintAmount - amount };
}

/* ------------------------------------------------------------------ */
/* Talking to the mint                                                 */
/* ------------------------------------------------------------------ */

/** An open invoice. `privkey` unlocks the quote and must stay in memory only. */
export interface OpenInvoice {
  wallet: Wallet;
  quote: MintQuoteBolt11Response;
  privkey: string;
}

// Step 1: ask the mint for an invoice for `mintAmount` sats.
//
// Since cashu-ts v5, every mint quote is locked to a key (NUT-20), so only
// whoever holds the key can mint the paid quote. We make a throwaway key here.
// It is NOT the refund key. It lives in this object, in memory, until the
// ecash is minted a few seconds later. That is also why the page must stay open
// between "paid" and "minted": lose this key and a paid invoice cannot be
// redeemed.
export async function openInvoice(mintUrl: string, mintAmount: number): Promise<OpenInvoice> {
  if (mintUrl.trim() === "") throw new ContributeError("Mint URL is not set.");
  const wallet = new Wallet(mintUrl);
  let privkey = "";
  try {
    await wallet.loadMint();
    const lockKey = await wallet.createQuoteLockKey({ random: true });
    privkey = lockKey.privkey;
    const quote = await wallet.createMintQuoteBolt11(mintAmount, lockKey.pubkey);
    if (typeof quote.request !== "string" || quote.request === "") {
      throw new ContributeError("The mint did not return an invoice.");
    }
    return { wallet, quote, privkey };
  } catch (err) {
    throw scrubContributeError(err, [], privkey, "The mint could not create an invoice.");
  }
}

/** The message of the error thrown when the visitor stops waiting. Compare with isCancelled(). */
const CANCELLED = "Cancelled.";
export function isCancelled(err: unknown): boolean {
  return err instanceof ContributeError && err.message === CANCELLED;
}

export interface WaitOptions {
  signal?: AbortSignal;
  intervalMs?: number;
  /** Give up after this long. Real invoices can take a while to pay. */
  timeoutMs?: number;
}

// Step 2: poll the mint until the invoice is paid. Returns the paid quote.
// A test mint answers PAID almost at once. A real mint waits for the wallet.
export async function waitUntilPaid(invoice: OpenInvoice, options: WaitOptions = {}): Promise<MintQuoteBolt11Response> {
  const interval = options.intervalMs ?? 1500;
  const deadline = Date.now() + (options.timeoutMs ?? 10 * 60 * 1000);
  while (Date.now() < deadline) {
    if (options.signal?.aborted) throw new ContributeError(CANCELLED);
    let checked: MintQuoteBolt11Response;
    try {
      checked = await invoice.wallet.checkMintQuoteBolt11(invoice.quote.quote);
    } catch (err) {
      throw scrubContributeError(err, [], invoice.privkey, "The mint could not be reached.");
    }
    if (checked.state === MintQuoteState.PAID) return checked;
    if (checked.state === MintQuoteState.ISSUED) {
      throw new ContributeError("This invoice was already used to mint ecash.");
    }
    await sleep(interval, options.signal);
  }
  throw new ContributeError("The invoice was not paid in time. If you paid it, wait a moment and try again.");
}

// Step 3: turn the paid quote into ecash. These proofs are NOT locked yet.
export async function mintPaid(invoice: OpenInvoice, mintAmount: number, paid: MintQuoteBolt11Response): Promise<Proof[]> {
  let proofs: Proof[] = [];
  try {
    proofs = await invoice.wallet.mintProofsBolt11(mintAmount, paid, { privkey: invoice.privkey });
  } catch (err) {
    throw scrubContributeError(err, proofs, invoice.privkey, "The mint did not return ecash for the paid invoice.");
  }
  if (sumProofs(proofs).toNumber() !== mintAmount) {
    throw new ContributeError("The mint returned a different amount than the invoice.");
  }
  return proofs;
}

// A sleep that wakes early when cancelled.
function sleep(ms: number, signal?: AbortSignal): Promise<void> {
  return new Promise((resolve) => {
    const timer = setTimeout(resolve, ms);
    signal?.addEventListener("abort", () => { clearTimeout(timer); resolve(); }, { once: true });
  });
}
