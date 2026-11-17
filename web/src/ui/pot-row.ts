import type { Pot } from "../api";
import { duration, formatSats } from "../format";
import { potPhase, potProgress } from "../pot-state";
import { t } from "../strings";
import { h } from "./dom";
import { kindChip, statusBadge } from "./pot-parts";

export function transitionName(id: string): string {
  return `pot-${id.replace(/[^a-zA-Z0-9]/g, "")}`;
}

export function potRow(pot: Pot, now: number): HTMLElement {
  const phase = potPhase(pot, now);
  const prog = potProgress(pot);
  const when =
    phase.phase === "open" ? `${duration(pot.deadline_unix - now)} left` : phase.phase === "paid" ? "Paid out" : "Ended";
  const title = h("h3", { class: "row-title", style: `view-transition-name:${transitionName(pot.id)}` }, pot.title);
  const fill = h("div", { class: "meter-fill" });
  fill.style.setProperty("--p", String(prog.lockedPct));
  return h(
    "a",
    { class: "pot-row", href: `#/pots/${encodeURIComponent(pot.id)}` },
    h("div", { class: "pot-meta" }, kindChip(pot.kind), statusBadge(phase)),
    title,
    h("div", { class: "meter-track meter-thin", "aria-hidden": "true" }, fill),
    h("p", { class: "row-foot" },
      h("span", {}, `${formatSats(pot.locked_sats)} ${t.of} ${formatSats(pot.target_sats)}`),
      h("span", { class: "muted" }, when)),
  );
}
