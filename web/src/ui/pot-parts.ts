import type { Pot } from "../api";
import { destinationKind, duration, formatDate, formatNumber, formatSats, formatTime, hostOf } from "../format";
import { potPhase, type PotPhase } from "../pot-state";
import { t } from "../strings";
import { h } from "./dom";

/** Status is words plus a shape. Colour only supports it. */
export function statusBadge(phase: PotPhase): HTMLElement {
  const [tone, glyph, label] = badgeFor(phase);
  return h("span", { class: `badge badge-${tone}` }, h("span", { class: "badge-glyph", "aria-hidden": "true" }, glyph), label);
}

function badgeFor(p: PotPhase): [string, string, string] {
  switch (p.phase) {
    case "paid":
      return ["done", "✓", t.phase.paid];
    case "refundable":
      return ["done", "↩", t.phase.refundable];
    case "grace":
      return ["wait", "…", t.phase.grace];
    case "open":
      if (p.funding === "ready") return ["go", "●", t.phase.ready];
      if (p.funding === "awaiting-fee") return ["wait", "…", t.phase.awaitingFee];
      if (p.funding === "empty") return ["open", "○", t.phase.empty];
      return ["open", "○", t.phase.partial];
  }
}

export function kindChip(kind: string): HTMLElement {
  return h("span", { class: "chip" }, t.kind[kind] ?? kind);
}

/** The countdown as a phrase and as a date, so neither has to be worked out. */
export function deadlineBlock(pot: Pot, nowUnix: number): HTMLElement {
  const phase = potPhase(pot, nowUnix);
  let phrase: string;
  let date: string;
  if (phase.phase === "open") {
    phrase = `${capitalize(duration(pot.deadline_unix - nowUnix))} left`;
    date = `Closes ${formatDate(pot.deadline_unix)}`;
  } else if (phase.phase === "grace") {
    phrase = `Refunds open in ${duration(pot.locktime_unix - nowUnix)}`;
    date = `Deadline was ${formatDate(pot.deadline_unix)}`;
  } else if (phase.phase === "refundable") {
    phrase = `Ended ${duration(nowUnix - pot.deadline_unix)} ago`;
    date = `Deadline was ${formatDate(pot.deadline_unix)}`;
  } else {
    phrase = "Finished";
    date = `Deadline was ${formatDate(pot.deadline_unix)}`;
  }
  return h("p", { class: "deadline" }, h("strong", {}, phrase), h("span", { class: "muted" }, date));
}

export function graceNote(pot: Pot): HTMLElement {
  return h("p", { class: "notice notice-wait" }, t.graceNote(formatTime(pot.locktime_unix)));
}

export function copyButton(value: string, what: string): HTMLButtonElement {
  const btn = h("button", { type: "button", class: "btn-quiet", "aria-label": `Copy ${what}` }, t.copy);
  btn.addEventListener("click", () => {
    void navigator.clipboard?.writeText(value).then(() => {
      btn.textContent = t.copied;
      btn.setAttribute("aria-label", `Copied ${what}`);
      setTimeout(() => {
        btn.textContent = t.copy;
        btn.setAttribute("aria-label", `Copy ${what}`);
      }, 1600);
    });
  });
  return btn;
}

function fact(label: string, value: Node | string, extra?: Node): HTMLElement {
  return h("div", { class: "fact" }, h("dt", {}, label), h("dd", {}, value, extra));
}

/** Contribution count, destination kind, mint, fee reserve. Long values wrap and can be copied. */
export function facts(pot: Pot): HTMLElement {
  const kind = destinationKind(pot.destination);
  const destLabel = kind === "bolt11" ? t.bolt11 : kind === "bolt12" ? t.bolt12 : t.notLightning;
  const fee =
    pot.fee_quoted && pot.fee_reserve_sats !== null ? formatSats(pot.fee_reserve_sats) : t.feeNotQuoted;
  return h(
    "dl",
    { class: "facts" },
    fact("Contributions", String(pot.contribution_count)),
    fact(t.payingTo, destLabel),
    fact(t.feeReserve, fee),
    fact(t.mint, h("span", { class: "mono wrap", title: pot.mint_url }, hostOf(pot.mint_url)), copyButton(pot.mint_url, "mint URL")),
  );
}

export function amountLine(pot: Pot): { locked: string; target: string } {
  return { locked: formatNumber(pot.locked_sats), target: formatSats(pot.target_sats) };
}

function capitalize(s: string): string {
  return s.charAt(0).toUpperCase() + s.slice(1);
}

/** Timings, keys and flags for people who want the details. */
export function formatTechnical(getPot: () => Pot): HTMLElement {
  const pot = getPot();
  const row = (label: string, value: string | Node) => fact(label, value);
  const key = (k: string | null) => (k ? h("span", { class: "mono wrap" }, k) : "Not set");
  return h(
    "dl",
    { class: "facts" },
    row("Advertised deadline", formatDate(pot.deadline_unix)),
    row("Server stops paying out", formatDate(pot.co_sign_until_unix)),
    row("Refund lock time", `${formatDate(pot.locktime_unix)} (deadline + ${pot.grace_seconds / 60} min)`),
    row("Unused fee reserve goes to", `the ${pot.fee_reserve_recipient}`),
    row("Server public key", key(pot.server_pubkey)),
    row("Organizer public key", key(pot.organizer_pubkey)),
    row("Pot id", h("span", { class: "mono wrap" }, pot.id)),
  );
}
