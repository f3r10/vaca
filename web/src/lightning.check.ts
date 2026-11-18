// Offline checks for the fee arithmetic in lightning.ts. Run with:
//   node src/lightning.check.ts
// No network. These numbers come from real runs against the testnut mint
// (input fee 100 ppk): ask for 11 sats, the pot locked 12, the token was 13.
import { feeFor, planMint } from "./lightning.ts";

function check(name: string, actual: unknown, expected: unknown): void {
  if (JSON.stringify(actual) !== JSON.stringify(expected)) {
    throw new Error(`${name}: expected ${JSON.stringify(expected)}, got ${JSON.stringify(actual)}`);
  }
}

// feeFor: 100 ppk is a tenth of a sat per proof, rounded up per batch.
check("no proofs, no fee", feeFor(0, 100), 0);
check("one proof", feeFor(1, 100), 1);
check("ten proofs", feeFor(10, 100), 1);
check("eleven proofs", feeFor(11, 100), 2);
check("free mint", feeFor(50, 0), 0);

// A mint with no input fee: nothing on top.
check("free mint plan", planMint(100, 0), { amount: 100, lockedTotal: 100, mintAmount: 100, feeSats: 0 });

// The real run: asking for 11 locked 12 and needed a 13 sat token.
check("real run, 11 sats", planMint(11, 100), { amount: 11, lockedTotal: 12, mintAmount: 13, feeSats: 2 });

// Small and medium amounts on a 100 ppk mint.
check("one sat", planMint(1, 100).feeSats, 2);
check("100 sats", planMint(100, 100).mintAmount >= 101, true);

// Invariants that must hold for every amount and every fee rate:
//   - the page never mints less than what must be locked after the swap fee
//   - the fee shown to the visitor is never negative
for (const ppk of [0, 50, 100, 250, 1000]) {
  for (const amount of [1, 2, 3, 7, 10, 11, 64, 100, 255, 500, 1000, 4096, 100_000]) {
    const plan = planMint(amount, ppk);
    const bits = plan.mintAmount.toString(2).length;
    if (plan.mintAmount - feeFor(bits, ppk) < plan.lockedTotal) {
      throw new Error(`mint too small for ${amount} at ${ppk} ppk`);
    }
    if (plan.lockedTotal < amount || plan.feeSats < 0) {
      throw new Error(`bad plan for ${amount} at ${ppk} ppk`);
    }
  }
}

// Bad input is refused with a plain message.
for (const [amount, ppk] of [[0, 100], [1.5, 100], [-3, 100], [10, -1], [10, 0.5]] as const) {
  let threw = false;
  try { planMint(amount, ppk); } catch { threw = true; }
  if (!threw) throw new Error(`planMint(${amount}, ${ppk}) should have thrown`);
}

console.log("lightning plan checks passed");
