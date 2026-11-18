import type { Pot } from "../api";
import { destinationKind, formatNumber, formatSats } from "../format";
import { destinationOk, potPhase, potProgress } from "../pot-state";
import { hostOf } from "../format";
import { t } from "../strings";
import { churn } from "./churn";
import { h } from "./dom";
import { transitionName } from "./pot-row";
import { deadlineBlock, facts, graceNote, kindChip, statusBadge } from "./pot-parts";

/** Problems the API cannot tell apart yet. Only the preview page passes these. */
export type Problem = "mint-unreachable" | "quote-expired";

export type CardAction = "contribute" | "payout" | "reclaim" | "retry";

export interface PotCardOptions {
  /** Called when a button on the card is pressed. */
  on?: (action: CardAction) => void;
  problem?: Problem;
  /** Set after this contributor reclaimed. A result of their action, not a pot status. */
  reclaimedSats?: number;
  /** Show the reclaim result on the card. The live page shows it once, in the reclaim panel. */
  reclaimNotice?: boolean;
  /** Fraction of the scale the bar starts from, so a new contribution visibly fills in. */
  fromFraction?: number;
}

function feeCaption(pot: Pot): string {
  if (!pot.fee_quoted || pot.fee_reserve_sats === null) return t.feeReserveNotQuoted;
  const reserve = `+ ${formatSats(pot.fee_reserve_sats)} fee reserve`;
  const input = typeof pot.input_fee_sats === "number" ? pot.input_fee_sats : 0;
  if (input > 0) return `${reserve} · + ${formatSats(input)} input fee`;
  return reserve;
}

function button(label: string, primary: boolean, o: PotCardOptions, act: CardAction, disabled = false): HTMLButtonElement {
  const b = h("button", { type: "button", class: primary ? "btn btn-primary" : "btn" }, label);
  if (disabled) b.disabled = true;
  b.addEventListener("click", () => o.on?.(act));
  return b;
}

/** Static text. Live announcements come from the persistent status regions in the panels. */
function notice(tone: "wait" | "warning" | "danger" | "success", text: string): HTMLElement {
  return h("p", { class: `notice notice-${tone}` }, text);
}

/** What the person can do next. One filled button at most. */
function action(pot: Pot, now: number, o: PotCardOptions): HTMLElement | null {
  const phase = potPhase(pot, now);
  if (phase.phase === "paid") return null;
  if (phase.phase === "grace") return graceNote(pot);
  if (phase.phase === "refundable") {
    if (o.reclaimedSats !== undefined) return null;
    return button(t.cta.reclaim, true, o, "reclaim");
  }
  if (o.problem === "mint-unreachable") return button(t.problem.retry, true, o, "retry");
  if (!destinationOk(pot)) return button(t.cta.contribute, false, o, "contribute", true);
  if (phase.funding === "ready") return button(t.cta.payout, true, o, "payout");
  if (pot.sats_still_needed === 0) return null; // nothing more can be added
  return button(t.cta.contribute, true, o, "contribute");
}

function celebration(pot: Pot): HTMLElement {
  return h(
    "div",
    { class: "celebrate" },
    h("div", { class: "celebrate-art" }, churn(1, "The churn is full")),
    h("div", {}, h("strong", {}, t.celebrate.title), h("p", {}, t.celebrate.body)),
    h("span", { class: "sr-only" }, formatSats(pot.locked_sats)),
  );
}

function meter(pot: Pot, fromFraction?: number): HTMLElement {
  const prog = potProgress(pot);
  const fill = h("div", { class: "meter-fill" });
  fill.style.setProperty("--p", String(fromFraction ?? 0));
  requestAnimationFrame(() => requestAnimationFrame(() => fill.style.setProperty("--p", String(prog.lockedPct))));
  return h(
    "div",
    { class: "meter", role: "img", "aria-label": `${formatSats(pot.locked_sats)} locked of ${formatSats(pot.target_sats)} target` },
    h(
      "div",
      { class: "meter-track" },
      fill,
      h("div", { class: "meter-target", style: `left:${prog.targetPct * 100}%` }),
      prog.reserveQuoted ? h("div", { class: "meter-reserve", style: `left:${prog.targetPct * 100}%` }) : null,
    ),
  );
}

export function potCard(pot: Pot, now: number, o: PotCardOptions = {}): HTMLElement {
  const phase = potPhase(pot, now);
  const prog = potProgress(pot);
  const kindName = destinationKind(pot.destination) === "bolt12" ? "Lightning offer" : "Lightning invoice";

  const outcome: (HTMLElement | null)[] = [];
  if (phase.phase === "paid") {
    outcome.push(notice("success", t.paid(formatSats(pot.quote_amount_sats ?? pot.target_sats), kindName)));
  }
  if (o.reclaimedSats !== undefined && o.reclaimNotice) outcome.push(notice("success", t.reclaimed(formatSats(o.reclaimedSats))));
  if (phase.phase === "open" && phase.funding === "awaiting-fee") outcome.push(notice("wait", t.waitingForQuote));
  if (o.problem === "mint-unreachable") outcome.push(notice("danger", t.problem.mintUnreachable(hostOf(pot.mint_url))));
  if (o.problem === "quote-expired") outcome.push(notice("warning", t.problem.quoteExpired));
  if (!destinationOk(pot)) outcome.push(notice("warning", t.problem.noPayout));

  const caption = feeCaption(pot);

  return h(
    "article",
    { class: "pot", "aria-labelledby": `pot-title-${pot.id}` },
    h("div", { class: "pot-meta" }, kindChip(pot.kind), statusBadge(phase)),
    h("h1", { class: "pot-title", id: `pot-title-${pot.id}`, style: `view-transition-name:${transitionName(pot.id)}` }, pot.title),
    phase.phase === "open" && phase.funding === "ready" ? celebration(pot) : null,
    h(
      "p",
      { class: "hero-amount" },
      h("span", { class: "hero-number" }, formatNumber(pot.locked_sats)),
      h("span", { class: "hero-unit" }, ` ${t.sats} ${t.locked}`),
    ),
    meter(pot, o.fromFraction),
    h(
      "p",
      { class: "meter-caption" },
      h("span", {}, `${t.of} ${formatSats(pot.target_sats)} · ${t.percentLocked(prog.targetPctOfTarget)}`),
      h("span", { class: "muted" }, caption),
    ),
    deadlineBlock(pot, now),
    ...outcome,
    action(pot, now, o),
    facts(pot),
    h("p", { class: "fine" }, t.feeExplain),
  );
}
