import type { Pot } from "../api";
import { formatNumber, formatSats } from "../format";
import { destinationOk, potPhase, potProgress } from "../pot-state";
import { t } from "../strings";
import { h, svg } from "../ui/dom";
import { deadlineBlock, destinationNotice, facts, graceNote, kindChip, statusBadge } from "../ui/pot-parts";

export type CardBuilder = (pot: Pot, nowUnix: number) => HTMLElement;

function cta(pot: Pot, nowUnix: number): HTMLElement | null {
  const p = potPhase(pot, nowUnix);
  if (p.phase === "grace") return graceNote(pot);
  if (p.phase === "paid") return null;
  if (p.phase === "refundable") return h("button", { type: "button", class: "btn btn-primary" }, t.cta.reclaim);
  if (p.funding === "ready") return h("button", { type: "button", class: "btn btn-primary" }, t.cta.payout);
  return h("button", { type: "button", class: "btn btn-primary" }, t.cta.contribute);
}

function notices(pot: Pot): HTMLElement[] {
  return destinationOk(pot) ? [] : [destinationNotice()];
}

function animateTo(el: HTMLElement, value: number): void {
  el.style.setProperty("--p", "0");
  requestAnimationFrame(() => requestAnimationFrame(() => el.style.setProperty("--p", String(value))));
}

/* ------------------------------------------------------------------ */
/* Variant 1. Numbers first: the amount is the hero, the bar is a rule */
/* ------------------------------------------------------------------ */
export const numbersFirst: CardBuilder = (pot, now) => {
  const phase = potPhase(pot, now);
  const prog = potProgress(pot);
  const fill = h("div", { class: "meter-fill" });
  const meter = h(
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
  animateTo(fill, prog.lockedPct);

  return h(
    "article",
    { class: "pot pot-numbers" },
    h("div", { class: "pot-meta" }, kindChip(pot.kind), statusBadge(phase)),
    h("h1", { class: "pot-title" }, pot.title),
    h(
      "p",
      { class: "hero-amount" },
      h("span", { class: "hero-number" }, formatNumber(pot.locked_sats)),
      h("span", { class: "hero-unit" }, ` ${t.sats} ${t.locked}`),
    ),
    meter,
    h(
      "p",
      { class: "meter-caption" },
      h("span", {}, `${t.of} ${formatSats(pot.target_sats)} · ${t.percentLocked(prog.targetPctOfTarget)}`),
      h("span", { class: "muted" }, pot.fee_quoted && pot.fee_reserve_sats !== null ? `+ ${formatSats(pot.fee_reserve_sats)} fee reserve` : "Fee reserve not quoted yet"),
    ),
    deadlineBlock(pot, now),
    ...notices(pot),
    cta(pot, now),
    facts(pot),
    h("p", { class: "fine" }, t.feeExplain),
  );
};

/* ------------------------------------------------------------------ */
/* Variant 2. Picture first: a milk churn fills; numbers sit beside it  */
/* ------------------------------------------------------------------ */
const CHURN_BODY = "M38 30 L62 30 C62 42 80 42 80 58 L80 126 Q80 134 72 134 L28 134 Q20 134 20 126 L20 58 C20 42 38 42 38 30 Z";

function churn(pot: Pot): SVGSVGElement {
  const prog = potProgress(pot);
  const top = 30;
  const bottom = 134;
  const targetY = bottom - (bottom - top) * prog.targetPct;
  const el = svg(
    `<defs>
       <clipPath id="churn-clip-${pot.id}"><path d="${CHURN_BODY}"/></clipPath>
       <pattern id="hatch-${pot.id}" width="6" height="6" patternUnits="userSpaceOnUse" patternTransform="rotate(45)">
         <rect width="6" height="6" class="hatch-bg"/><line x1="0" y1="0" x2="0" y2="6" class="hatch-line" stroke-width="2"/>
       </pattern>
     </defs>
     <rect x="34" y="6" width="32" height="9" rx="4" class="churn-lid"/>
     <rect x="38" y="14" width="24" height="16" class="churn-neck"/>
     <path d="${CHURN_BODY}" class="churn-body"/>
     <g clip-path="url(#churn-clip-${pot.id})">
       <path class="churn-spot" d="M24 66 q10 -8 16 2 q4 10 -6 14 q-12 2 -10 -16z"/>
       <path class="churn-spot" d="M58 100 q12 -6 18 4 q2 12 -10 14 q-12 -2 -8 -18z"/>
       <rect class="churn-fill" x="0" y="${top}" width="100" height="${bottom - top}"/>
       ${prog.reserveQuoted ? `<rect x="0" y="${top}" width="100" height="${Math.max(3, targetY - top)}" fill="url(#hatch-${pot.id})"/>` : ""}
     </g>
     <line class="churn-target" x1="14" x2="86" y1="${targetY}" y2="${targetY}" stroke-dasharray="3 3"/>
     <path d="${CHURN_BODY}" class="churn-outline"/>`,
    {
      viewBox: "0 0 100 140",
      role: "img",
      "aria-label": `Churn ${prog.targetPctOfTarget}% full. ${formatSats(pot.locked_sats)} locked of ${formatSats(pot.target_sats)}.`,
      class: "churn",
    },
  );
  const fill = el.querySelector<SVGElement>(".churn-fill");
  if (fill) animateTo(fill as unknown as HTMLElement, prog.lockedPct);
  return el;
}

export const pictureFirst: CardBuilder = (pot, now) => {
  const phase = potPhase(pot, now);
  const prog = potProgress(pot);
  return h(
    "article",
    { class: "pot pot-churn" },
    h(
      "div",
      { class: "churn-layout" },
      h("div", { class: "churn-art" }, churn(pot)),
      h(
        "div",
        { class: "churn-copy" },
        h("div", { class: "pot-meta" }, kindChip(pot.kind), statusBadge(phase)),
        h("h1", { class: "pot-title" }, pot.title),
        h(
          "p",
          { class: "churn-amount" },
          h("strong", {}, formatSats(pot.locked_sats)),
          h("span", { class: "muted" }, ` ${t.of} ${formatSats(pot.target_sats)} · ${t.percentLocked(prog.targetPctOfTarget)}`),
        ),
        deadlineBlock(pot, now),
      ),
    ),
    ...notices(pot),
    cta(pot, now),
    facts(pot),
    h("p", { class: "fine" }, t.feeExplain),
  );
};

export const VARIANTS: { group: string; items: { id: string; label: string; build: CardBuilder }[] }[] = [
  {
    group: "Hero",
    items: [
      { id: "numbers", label: "Numbers first", build: numbersFirst },
      { id: "churn", label: "Picture first", build: pictureFirst },
    ],
  },
];
