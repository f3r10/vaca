// Every user-facing string lives here so a Spanish version is one more object.
export const en = {
  kind: { cause: "Cause", purchase: "Shared purchase", split_bill: "Split the bill" } as Record<string, string>,
  phase: {
    empty: "Open · nothing locked yet",
    partial: "Open",
    awaitingFee: "Waiting for a fee quote",
    ready: "Ready to pay out",
    grace: "Deadline passed",
    refundable: "Refunds open",
    paid: "Paid",
  },
  sats: "sats",
  of: "of",
  target: "Target",
  locked: "locked",
  feeReserve: "Fee reserve",
  feeNotQuoted: "Not quoted yet",
  feeExplain: "The mint holds back a small reserve when it pays. The pot has to cover the target plus that reserve.",
  contributions: (n: number) => (n === 1 ? "1 contribution" : `${n} contributions`),
  payingTo: "Pays to",
  mint: "Mint",
  bolt11: "Lightning invoice (BOLT11)",
  bolt12: "Lightning offer (BOLT12)",
  notLightning: "Not a Lightning invoice or offer",
  notLightningHelp: "This pot's destination doesn't look like a Lightning invoice or offer, so it can't be paid out.",
  copy: "Copy",
  copied: "Copied",
  cta: {
    contribute: "Contribute sats",
    payout: "Pay out now",
    reclaim: "Reclaim my sats",
  },
  graceNote: (when: string) => `Refunds open at ${when}. Until then, nobody can reclaim.`,
  percentLocked: (p: number) => `${p}% of the target`,
} as const;

export const t = en;
