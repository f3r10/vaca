// The "pay with Lightning" way to contribute.
//
// The visitor picks an amount, sees exactly what it will cost, and pays one
// Lightning invoice. The page does the rest: mint ecash, lock it to the pot,
// count it. No Cashu wallet, no token, no file picker.
//
// Order of events (each line is a progress message the visitor sees):
//   amount -> plan the fees -> ask the mint for an invoice -> show it
//   -> wait until PAID -> mint ecash -> lock it -> post it -> counted
//
// The wallet work is in ../lightning.ts. The money-safe part that runs after the
// ecash exists is in contribute-session.ts. This file is the form around them.

import { assertPotCanTakeProofs } from "../contribute";
import { friendly } from "../errors";
import { mintKind, type MintKind } from "../format";
import { isCancelled, mintPaid, openInvoice, planMint, waitUntilPaid, type MintPlan, type OpenInvoice } from "../lightning";
import { contribute as c } from "../strings";
import type { Session } from "./contribute-session";
import { h } from "./dom";
import { copyButton } from "./pot-parts";
import { qrCode } from "./qr";
import type { PayForm } from "./token-form";

const PRESETS = [100, 500, 1000];

// A paid invoice whose ecash we could not mint yet. The key is still in memory,
// so the visitor can simply try minting again.
interface Resume {
  invoice: OpenInvoice;
  plan: MintPlan;
  paid: Awaited<ReturnType<typeof waitUntilPaid>>;
}

export function lightningForm(s: Session): PayForm {
  let waiting: AbortController | null = null;
  let resume: Resume | null = null;
  // True from "invoice created" until the contribution is counted or failed.
  // While true, closing the tab asks for confirmation (see `guard`).
  let inFlight = false;

  const amount = h("input", {
    type: "text", inputmode: "numeric", id: "ln-amount", name: "amount", autocomplete: "off",
    "aria-describedby": "ln-hint ln-error ln-summary",
  }) as HTMLInputElement;
  const presetRow = h("div", { class: "presets", role: "group", "aria-label": "Quick amounts" });
  const errorEl = h("p", { class: "field-error", id: "ln-error", hidden: "" });
  const summary = h("p", { class: "summary", id: "ln-summary", "aria-live": "polite" });
  const pay = h("button", { type: "button", class: "btn btn-primary" }, "");
  const progress = h("p", { class: "progress", role: "status", "aria-live": "polite" });
  const invoiceHost = h("div", {});

  /* ---- reading the amount ---- */
  const maxSats = () => Math.max(0, s.getPot().sats_still_needed);

  function parseAmount(): { value: number } | { error: string } {
    const raw = amount.value.replace(/[,\s]/g, "");
    if (!/^\d+$/.test(raw)) return { error: c.lightning.errors.whole };
    const value = Number(raw);
    if (value < 1) return { error: c.lightning.errors.min };
    if (value > maxSats()) return { error: c.lightning.errors.max(maxSats()) };
    return { value };
  }

  function currentPlan(): MintPlan | null {
    const parsed = parseAmount();
    const ppk = s.getPot().input_fee_ppk;
    if ("error" in parsed || typeof ppk !== "number") return null;
    return planMint(parsed.value, ppk);
  }

  // Redraw the error, the fee summary and the button label for what is typed.
  function refresh(): void {
    const parsed = parseAmount();
    const ppk = s.getPot().input_fee_ppk;
    const bad = "error" in parsed;
    errorEl.hidden = !bad;
    errorEl.textContent = bad ? parsed.error : "";
    amount.setAttribute("aria-invalid", String(bad));
    if (resume) return; // keep "Try minting again" until it works
    if (bad || typeof ppk !== "number") {
      // Nothing to price yet: either the amount is wrong, or the server gave no fee rate.
      summary.textContent = !bad ? c.pay.noFee : "";
      pay.disabled = true;
      pay.textContent = c.lightning.buttonIdle;
      return;
    }
    const plan = planMint(parsed.value, ppk);
    summary.textContent = c.lightning.summary(plan.mintAmount, plan.amount, plan.feeSats);
    pay.textContent = c.lightning.button(plan.mintAmount);
    pay.disabled = false;
  }

  function buildPresets(): void {
    const max = maxSats();
    presetRow.replaceChildren();
    const values = PRESETS.filter((n) => n < max);
    const add = (value: number, label: string) => {
      const b = h("button", { type: "button", class: "chip-btn" }, label);
      b.addEventListener("click", () => { amount.value = String(value); refresh(); });
      presetRow.append(b);
    };
    for (const v of values) add(v, v.toLocaleString("en"));
    if (max > 0) add(max, c.lightning.fill(max));
  }

  amount.value = String(Math.min(100, maxSats()) || "");
  buildPresets();
  amount.addEventListener("input", refresh);

  /* ---- the invoice card ---- */
  function showInvoice(invoice: OpenInvoice): void {
    const text = invoice.quote.request;
    const short = text.length > 40 ? `${text.slice(0, 22)}…${text.slice(-10)}` : text;
    // The mint chose this text, so only build a wallet link from something that
    // really looks like an invoice. The scheme is fixed to lightning: either way.
    const looksLikeInvoice = /^ln[a-z0-9]+$/i.test(text);
    // The QR code is only drawn for text that looks like an invoice. If the text
    // is too long for a QR code, qrCode returns null and the visitor still has
    // the Copy button.
    const qr = looksLikeInvoice ? qrCode(text, c.lightning.qrLabel) : null;
    const cancel = h("button", { type: "button", class: "btn" }, c.lightning.cancel);
    cancel.addEventListener("click", () => waiting?.abort());
    invoiceHost.replaceChildren(
      h("div", { class: "invoice" },
        h("h4", {}, c.lightning.invoiceTitle),
        h("p", { class: "fine" }, c.lightning.invoiceHelp),
        qr ? h("div", { class: "qr-wrap" }, qr) : null,
        h("p", { class: "mono wrap invoice-text", title: "Tap Copy to use the full invoice" }, short),
        h("div", { class: "actions" },
          looksLikeInvoice ? h("a", { class: "btn btn-primary", href: `lightning:${text}` }, c.lightning.open) : null,
          copyButton(text, "invoice"),
          cancel),
        h("p", { class: "notice notice-wait" }, c.lightning.keepOpen),
        testNote(mintKind(s.getPot().mint_url))),
    );
  }

  // Say "pays itself" only for mints that really do. A local or signet mint needs a real payment.
  function testNote(kind: MintKind): HTMLElement | null {
    if (kind === "fake") return h("p", { class: "fine" }, c.lightning.testMintFake);
    if (kind === "other") return null;
    return h("p", { class: "fine" }, c.lightning.testMintReal);
  }

  function setControls(disabled: boolean): void {
    amount.disabled = disabled;
    pay.disabled = disabled;
    presetRow.querySelectorAll("button").forEach((b) => { b.disabled = disabled; });
  }

  /* ---- the run ---- */
  // Everything after the invoice is paid. Split out so "Try minting again" can reuse it.
  async function finish(ctx: Resume): Promise<void> {
    progress.textContent = c.lightning.progress.paid;
    let proofs;
    try {
      proofs = await mintPaid(ctx.invoice, ctx.plan.mintAmount, ctx.paid);
    } catch (err) {
      // Paid but not minted. The quote key is still in memory, so offer a retry
      // instead of giving up on money that has already moved.
      resume = ctx;
      pay.textContent = c.lightning.retryMint;
      s.say(`${friendly(err)} ${c.lightning.paidSafe}`, "error");
      return;
    }
    resume = null;
    const counted = await s.lockAndCount(proofs, ctx.plan.amount, { minted: true, onStep: (step) => { progress.textContent = step; } });
    inFlight = false;
    progress.textContent = "";
    invoiceHost.replaceChildren();
    void counted; // the session already said whether it worked
  }

  pay.addEventListener("click", () => {
    void (async () => {
      if (!s.begin()) return;
      s.say("");
      try {
        if (resume) {
          setControls(true);
          await finish(resume);
          return;
        }
        const parsed = parseAmount();
        if ("error" in parsed) { amount.focus(); return; }
        const refund = s.refundPubkey();
        if (!refund) return s.say("Back up your refund key first.", "error");
        if (!s.serverPubkey()) return; // it already said why
        const pot = s.getPot();
        assertPotCanTakeProofs(pot, Math.floor(Date.now() / 1000));
        const plan = currentPlan();
        if (!plan) return s.say(c.pay.noFee, "error");

        setControls(true);
        inFlight = true;
        progress.textContent = c.lightning.progress.invoice;
        const invoice = await openInvoice(pot.mint_url, plan.mintAmount);
        showInvoice(invoice);
        progress.textContent = c.lightning.progress.waiting;
        waiting = new AbortController();
        const paid = await waitUntilPaid(invoice, { signal: waiting.signal });
        waiting = null;
        invoiceHost.querySelector(".actions")?.remove(); // too late to cancel or re-pay
        await finish({ invoice, plan, paid });
      } catch (err) {
        inFlight = resume !== null;
        progress.textContent = "";
        invoiceHost.replaceChildren();
        waiting = null;
        if (isCancelled(err)) s.say(c.lightning.cancelled, "info");
        else s.say(friendly(err), "error");
      } finally {
        buildPresets();
        // After "paid but not minted" the amount is frozen and only the retry is live.
        setControls(resume !== null);
        if (resume) pay.disabled = false;
        else refresh();
        s.end();
      }
    })();
  });

  /* ---- do not lose a paid invoice by closing the tab ---- */
  const guard = (event: BeforeUnloadEvent): void => {
    if (!inFlight) return;
    event.preventDefault();
    event.returnValue = c.lightning.leave; // most browsers show their own text instead
  };
  window.addEventListener("beforeunload", guard);

  refresh();
  const el = h(
    "div",
    { class: "pay-form" },
    h("div", { class: "field" },
      h("label", { for: "ln-amount" }, c.lightning.amountLabel),
      h("p", { class: "hint", id: "ln-hint" }, c.lightning.amountHint(maxSats())),
      amount,
      presetRow,
      errorEl),
    summary,
    pay,
    progress,
    invoiceHost,
  );
  return {
    el,
    dispose: () => {
      waiting?.abort();
      window.removeEventListener("beforeunload", guard);
    },
  };
}
