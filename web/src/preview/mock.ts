import type { Pot } from "../api";

const SERVER_KEY = `02${"ab".repeat(32)}`;
const MINT = "http://127.0.0.1:8085";
// Not real invoices. Shaped like them so the card can tell BOLT11 from BOLT12.
const BOLT11 = "lnbcrt840u1pn7x8k2pp5qqqsyqcyq5rqwzqfqqqsyqcyq5rqwzqfqqqsyqcyq5rqwzqfqypqdqqcqzzsxqyz5vqsp5zyg3zyg3zyg3zyg3zyg3zyg3zyg3zyg3zyg3zyg3zyg3zyg3zyg3zygs";
const BOLT12 = "lno1pgx9getnwss8vetrw3hhyuckyypwa3eyt44h6txtxquqh7lz5djge4afgfjn7k4rgrkuag0jsd5xvxg";

export type MockName = "empty" | "partial" | "awaiting-fee" | "ready" | "grace" | "refundable" | "paid" | "bad-destination";

export function mockPot(name: MockName, now = Math.floor(Date.now() / 1000)): Pot {
  const base: Pot = {
    id: `demo-${name}`,
    title: "",
    kind: "purchase",
    target_sats: 84_000,
    deadline_unix: now + 6 * 86400 + 4 * 3600,
    destination: BOLT11,
    mint_url: MINT,
    organizer_pubkey: null,
    server_pubkey: SERVER_KEY,
    status: "open",
    created_at_unix: now - 3 * 86400,
    contribution_count: 0,
    locked_sats: 0,
    locktime_unix: 0,
    co_sign_until_unix: 0,
    grace_seconds: 600,
    fee_reserve_recipient: "organizer",
    sigflag_v1: "SIG_INPUTS",
    sigflag_v2: "SIG_ALL",
    fee_reserve_sats: null,
    sats_still_needed: 0,
    payout_allowed: false,
    fee_quoted: false,
    quote_amount_sats: null,
    change_sats: 0,
  };
  let pot: Pot;
  switch (name) {
    case "empty":
      pot = { ...base, title: "Espresso machine for the climbing gym", target_sats: 84_000, sats_still_needed: 84_000 };
      break;
    case "partial":
      pot = {
        ...base,
        title: "Espresso machine for the climbing gym",
        locked_sats: 31_500,
        contribution_count: 9,
        fee_quoted: true,
        fee_reserve_sats: 840,
        quote_amount_sats: 84_000,
        sats_still_needed: 84_000 + 840 - 31_500,
      };
      break;
    case "ready":
      pot = {
        ...base,
        kind: "cause",
        title: "A new roof for the Quito animal shelter",
        target_sats: 50_000,
        deadline_unix: now + 2 * 3600 + 14 * 60,
        destination: BOLT12,
        locked_sats: 50_120,
        contribution_count: 23,
        fee_quoted: true,
        fee_reserve_sats: 120,
        quote_amount_sats: 50_000,
        sats_still_needed: 0,
        payout_allowed: true,
      };
      break;
    case "awaiting-fee":
      pot = {
        ...base,
        title: "Espresso machine for the climbing gym",
        locked_sats: 84_000,
        contribution_count: 14,
        sats_still_needed: 0,
      };
      break;
    case "grace":
      pot = {
        ...base,
        kind: "split_bill",
        title: "Dinner at La Choza, table of eight",
        target_sats: 8_400,
        deadline_unix: now - 4 * 60,
        locked_sats: 8_460,
        contribution_count: 8,
        fee_quoted: true,
        fee_reserve_sats: 60,
        quote_amount_sats: 8_400,
        sats_still_needed: 0,
      };
      break;
    case "paid":
      pot = {
        ...base,
        kind: "cause",
        title: "A new roof for the Quito animal shelter",
        target_sats: 50_000,
        deadline_unix: now + 5 * 3600,
        destination: BOLT12,
        status: "paid",
        locked_sats: 50_120,
        contribution_count: 23,
        fee_quoted: true,
        fee_reserve_sats: 120,
        quote_amount_sats: 50_000,
        sats_still_needed: 0,
      };
      break;
    case "bad-destination":
      pot = {
        ...base,
        title: "Shared taxi to the airport",
        target_sats: 12_000,
        destination: "asd",
        locked_sats: 2_000,
        contribution_count: 2,
        sats_still_needed: 10_000,
      };
      break;
    case "refundable":
      pot = {
        ...base,
        kind: "split_bill",
        title: "Dinner at La Choza, table of eight",
        target_sats: 8_400,
        deadline_unix: now - 3 * 3600,
        destination: BOLT11,
        locked_sats: 5_200,
        contribution_count: 6,
        fee_quoted: true,
        fee_reserve_sats: 60,
        quote_amount_sats: 8_400,
        sats_still_needed: 3_260,
      };
      break;
  }
  pot.co_sign_until_unix = pot.deadline_unix;
  pot.locktime_unix = pot.deadline_unix + pot.grace_seconds;
  return pot;
}
