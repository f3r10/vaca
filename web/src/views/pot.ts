import { getPot, postPayout, type Pot } from "../api";
import { friendly } from "../errors";
import { formatSats } from "../format";
import { destinationOk, potPhase, potProgress } from "../pot-state";
import { app } from "../strings";
import { contributePanel } from "../ui/contribute-panel";
import { h } from "../ui/dom";
import { copyButton, formatTechnical } from "../ui/pot-parts";
import { potCard, type CardAction } from "../ui/pot-card";
import { refundPanel } from "../ui/refund-panel";
import { trustCard } from "../ui/trust";

const POLL_MS = 15_000;

export interface PotView { el: HTMLElement; dispose: () => void }

export function potView(initial: Pot): PotView {
  let pot = initial;
  let reclaimedSats: number | undefined;
  let panelKind = "";
  const now = () => Math.floor(Date.now() / 1000);

  const cardHost = h("div", { class: "card-host" });
  const payoutHost = h("div", { class: "payout-host" });
  const panelHost = h("div", { class: "panel-host" });

  function focusPanel(id: string): void {
    const el = document.getElementById(id);
    el?.scrollIntoView({ behavior: matchMedia("(prefers-reduced-motion: reduce)").matches ? "auto" : "smooth", block: "start" });
    el?.focus({ preventScroll: true });
  }

  function onAction(action: CardAction): void {
    if (action === "contribute") focusPanel("contribute");
    else if (action === "reclaim") focusPanel("reclaim");
    else if (action === "payout") showPayoutConfirm();
    else void refresh();
  }

  function renderCard(from?: number): void {
    cardHost.replaceChildren(potCard(pot, now(), { on: onAction, reclaimedSats, fromFraction: from }));
  }

  function wantedPanel(): string {
    const p = potPhase(pot, now());
    if (p.phase === "refundable") return "refund";
    if (p.phase === "open") return destinationOk(pot) ? "contribute" : "closed";
    return "closed";
  }

  function renderPanels(): void {
    const want = wantedPanel();
    if (want === panelKind) return; // keep in-progress input and retry state
    panelKind = want;
    if (want === "contribute") panelHost.replaceChildren(contributePanel(() => pot, update));
    else if (want === "refund") panelHost.replaceChildren(refundPanel(() => pot, (sats) => { reclaimedSats = sats; renderCard(); }));
    else panelHost.replaceChildren(h("p", { class: "notice notice-wait" }, app.pot.closed));
  }

  function update(next: Pot): void {
    const from = potProgress(pot).lockedPct;
    pot = next;
    renderCard(from);
    renderPanels();
    if (pot.status === "paid") payoutHost.replaceChildren();
  }

  async function refresh(): Promise<void> {
    try {
      const next = await getPot(pot.id);
      // Re-render only on a real change, so focus, announcements and animations stay put.
      if (JSON.stringify(next) !== JSON.stringify(pot)) update(next);
    } catch { /* keep what is shown */ }
  }

  function showPayoutConfirm(): void {
    const status = h("div", { role: "status", "aria-live": "polite" });
    const go = h("button", { type: "button", class: "btn btn-primary" }, "Confirm and pay out");
    const cancel = h("button", { type: "button", class: "btn" }, "Cancel");
    cancel.addEventListener("click", () => {
      payoutHost.replaceChildren();
      cardHost.querySelector<HTMLElement>(".btn-primary")?.focus();
    });
    go.addEventListener("click", () => {
      go.disabled = true;
      go.textContent = "Paying…";
      void postPayout(pot.id)
        .then((next) => {
          update(next);
          if (next.status !== "paid") status.replaceChildren(h("p", { class: "notice notice-wait" }, "The mint hasn't confirmed the payment yet. Check again in a minute."));
        })
        .catch((err) => {
          status.replaceChildren(h("p", { class: "notice notice-danger" }, friendly(err)));
          go.disabled = false;
          go.textContent = "Confirm and pay out";
        });
    });
    payoutHost.replaceChildren(
      h("section", { class: "panel", id: "payout", tabindex: "-1", "aria-labelledby": "payout-title" },
        h("h2", { id: "payout-title" }, `Pay out ${formatSats(pot.quote_amount_sats ?? pot.target_sats)}?`),
        h("p", {}, "The server will ask the mint to pay the destination fixed when this pot was created. Once the mint pays, it can't be undone."),
        h("p", { class: "fine" }, "Anyone with this link can start the payout. The mint's fee reserve is held back and any unused part goes to the organizer."),
        h("div", { class: "actions" }, go, cancel),
        status),
    );
    focusPanel("payout");
  }

  const link = `${location.origin}${location.pathname}#/pots/${encodeURIComponent(pot.id)}`;
  const shareInput = h("input", { type: "text", readonly: "", id: "share-link", value: link, "aria-label": "Link to this pot" }) as HTMLInputElement;
  shareInput.addEventListener("focus", () => shareInput.select());
  const share = h("section", { class: "panel" }, h("h2", {}, app.pot.share), h("p", { class: "fine" }, app.pot.shareHelp), h("div", { class: "share-row" }, shareInput, copyButton(link, "pot link")));

  const details = h("details", { class: "panel details" }, h("summary", {}, app.pot.details), formatTechnical(() => pot));

  renderCard();
  renderPanels();
  const timer = window.setInterval(() => { if (!document.hidden) void refresh(); }, POLL_MS);

  const el = h(
    "div",
    { class: "page page-narrow" },
    h("a", { class: "back", href: "#/" }, `← ${app.pot.back}`),
    cardHost,
    payoutHost,
    panelHost,
    trustCard(),
    share,
    details,
  );
  return { el, dispose: () => window.clearInterval(timer) };
}
