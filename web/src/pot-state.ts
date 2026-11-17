import type { Pot } from "./api";
import { destinationKind } from "./format";

export type Funding = "empty" | "partial" | "awaiting-fee" | "ready";

export type PotPhase =
  | { phase: "open"; funding: Funding }
  | { phase: "grace" } // past the advertised deadline, before the lock time
  | { phase: "refundable" }
  | { phase: "paid" };

/** One place that turns API fields and the clock into the state the card shows. */
export function potPhase(pot: Pot, nowUnix: number): PotPhase {
  if (pot.status === "paid") return { phase: "paid" };
  if (nowUnix >= pot.locktime_unix) return { phase: "refundable" };
  if (nowUnix > pot.deadline_unix) return { phase: "grace" };
  if (pot.payout_allowed) return { phase: "open", funding: "ready" };
  if (pot.locked_sats === 0) return { phase: "open", funding: "empty" };
  if (pot.locked_sats >= pot.target_sats && !pot.fee_quoted) return { phase: "open", funding: "awaiting-fee" };
  return { phase: "open", funding: "partial" };
}

export interface Progress {
  /** Everything the pot has to hold: the target plus the reserve, when quoted. */
  scale: number;
  lockedPct: number;
  targetPct: number;
  reserveQuoted: boolean;
  targetPctOfTarget: number;
}

export function potProgress(pot: Pot): Progress {
  const reserve = pot.fee_quoted && pot.fee_reserve_sats !== null ? pot.fee_reserve_sats : 0;
  const scale = pot.target_sats + reserve;
  return {
    scale,
    lockedPct: Math.min(1, pot.locked_sats / scale),
    targetPct: pot.target_sats / scale,
    reserveQuoted: reserve > 0,
    targetPctOfTarget: Math.min(100, Math.round((pot.locked_sats / pot.target_sats) * 100)),
  };
}

export function destinationOk(pot: Pot): boolean {
  return destinationKind(pot.destination) !== "unknown";
}
