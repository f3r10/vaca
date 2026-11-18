import "../tokens.css";
import "../base.css";
import "../components.css";
import "./preview.css";
import type { Pot } from "../api";
import { potCard, type PotCardOptions } from "../ui/pot-card";
import { h } from "../ui/dom";
import { mockPot, type MockName } from "./mock";
import { mountSwitcher, type Group } from "./switcher";

/* Throwaway state workbench. Mock data only. Every state below is a prop to the real card. */

interface Scenario {
  id: string;
  label: string;
  pot: MockName;
  options?: PotCardOptions;
  /** Preview-only: the demo cannot reach this state on regtest yet. */
  preview?: string;
}

const GROUPS: { group: string; items: Scenario[] }[] = [
  {
    group: "Open",
    items: [
      { id: "empty", label: "Empty", pot: "empty" },
      { id: "partly-funded", label: "Partly funded", pot: "partial" },
      { id: "awaiting-fee", label: "Funded, fee not quoted", pot: "awaiting-fee" },
      { id: "fee-gate-open", label: "Fee gate open", pot: "ready" },
    ],
  },
  {
    group: "After the deadline",
    items: [
      { id: "grace", label: "Waiting for the lock time", pot: "grace" },
      { id: "refundable", label: "Refunds open", pot: "refundable" },
    ],
  },
  {
    group: "Outcome",
    items: [
      { id: "paid", label: "Paid", pot: "paid", preview: "The server can pay out, but no demo pot has reached this yet." },
      { id: "refunded", label: "Refunded (your result)", pot: "refundable", options: { reclaimedSats: 1_200, reclaimNotice: true }, preview: "A contributor's own result after reclaiming. Not a pot status." },
    ],
  },
  {
    group: "Problems",
    items: [
      { id: "bad-destination", label: "Not an invoice", pot: "bad-destination" },
      { id: "mint-unreachable", label: "Mint unreachable", pot: "partial", options: { problem: "mint-unreachable" }, preview: "The API can't tell this apart from other fee problems yet." },
      { id: "quote-expired", label: "Quote expired", pot: "partial", options: { problem: "quote-expired" }, preview: "The server refetches expired quotes, so the page never sees this one." },
    ],
  },
];

const ALL = GROUPS.flatMap((g) => g.items);
const app = document.querySelector<HTMLElement>("#app");
if (!app) throw new Error("Missing #app");

const stage = h("div", { class: "variant-stage" });
const banner = h("p", { class: "preview-banner", role: "note", hidden: "" });
let pot: Pot;
let current: Scenario = ALL[0];
let from: number | undefined;

function show(): void {
  const now = Math.floor(Date.now() / 1000);
  banner.hidden = !current.preview;
  banner.textContent = current.preview ? `Preview only. ${current.preview}` : "";
  const open = pot.status === "open" && now <= pot.deadline_unix;
  const add = h("button", { type: "button", class: "btn preview-add" }, "Preview: add 5,000 sats");
  add.hidden = !open || pot.payout_allowed || current.preview !== undefined || current.id === "bad-destination";
  add.addEventListener("click", () => addSats());
  stage.replaceChildren(banner, potCard(pot, now, { ...current.options, fromFraction: from }), add);
  from = undefined;
}

/** Mock a contribution landing so the fill and the target-reached moment can be judged. */
function addSats(): void {
  const scale = pot.target_sats + (pot.fee_reserve_sats ?? 0) + (pot.input_fee_sats ?? 0);
  from = Math.min(1, pot.locked_sats / scale);
  const locked = Math.min(scale, pot.locked_sats + 5_000);
  const quoted = pot.fee_quoted;
  pot = {
    ...pot,
    locked_sats: locked,
    contribution_count: pot.contribution_count + 1,
    sats_still_needed: Math.max(0, scale - locked),
    payout_allowed: quoted && locked >= scale,
  };
  show();
}

const { list, select } = mountSwitcher("__state", GROUPS as Group[], (id) => {
  current = ALL.find((s) => s.id === id) ?? ALL[0];
  pot = mockPot(current.pot);
  from = undefined;
  show();
});

pot ??= mockPot(current.pot);
app.append(
  h(
    "main",
    { class: "variant-page" },
    h("div", { class: "variant-layout" }, list, select, stage),
  ),
);
