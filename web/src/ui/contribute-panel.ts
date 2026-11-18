// The Contribute panel: two steps, and a choice of how to pay in the second.
//
//   Step 1  Back up a refund key   (key-step.ts)   -> gives us the public key
//   Step 2  Pay                    (this file)     -> Lightning, or a Cashu token
//
// This file only puts the pieces together and wires them to one shared Session
// (contribute-session.ts), which owns the status line, the money-safety
// downloads and the "count it" logic. The two ways to pay are one file each:
// lightning-form.ts and token-form.ts.

import type { Pot } from "../api";
import { contribute as c } from "../strings";
import { createSession } from "./contribute-session";
import { h } from "./dom";
import { keyStep } from "./key-step";
import { lightningForm } from "./lightning-form";
import { tokenForm } from "./token-form";

export interface ContributePanel {
  el: HTMLElement;
  /** Stop polling and remove listeners. Called when the panel is replaced or the page changes. */
  dispose(): void;
}

export function contributePanel(getPot: () => Pot, onPot: (pot: Pot) => void): ContributePanel {
  let refundPubkey: string | null = null; // public key only, from step 1

  /* ---- the Advanced server key box (only when the server publishes no key) ---- */
  const serverKey = h("input", { type: "text", id: "server-key", name: "server-key", spellcheck: "false", autocomplete: "off", placeholder: "02… or 03…" }) as HTMLInputElement;
  const serverKeyBox = h(
    "details",
    { class: "advanced" },
    h("summary", {}, c.pay.serverKey.summary),
    h("p", { class: "fine" }, c.pay.serverKey.help),
    h("label", { for: "server-key" }, c.pay.serverKey.label),
    serverKey,
  );
  serverKeyBox.hidden = !!getPot().server_pubkey;

  /* ---- shared session, then the two payment forms that use it ---- */
  const parts = createSession(
    getPot,
    onPot,
    () => refundPubkey,
    () => serverKey.value,
    (pot) => { if (pot.sats_still_needed === 0) root.classList.add("panel-done"); },
  );
  const lightning = lightningForm(parts.session);
  const token = tokenForm(parts.session);
  token.el.hidden = true;

  /* ---- how do you want to pay? ---- */
  const methods = h("fieldset", { class: "kinds methods" }, h("legend", {}, c.pay.legend));
  function addMethod(value: "lightning" | "token", copy: readonly [string, string], checked: boolean): void {
    const input = h("input", { type: "radio", name: "method", value, id: `method-${value}` }) as HTMLInputElement;
    input.checked = checked;
    input.addEventListener("change", () => {
      lightning.el.hidden = value !== "lightning";
      token.el.hidden = value !== "token";
    });
    methods.append(h("label", { class: "kind", for: `method-${value}` }, input, h("span", { class: "kind-text" }, h("strong", {}, copy[0]), h("span", { class: "muted" }, copy[1]))));
  }
  addMethod("lightning", c.pay.lightning, true);
  addMethod("token", c.pay.token, false);

  /* ---- step 2 stays locked until step 1 produced a verified key ---- */
  const lockedHint = h("p", { class: "hint", id: "locked-hint" }, c.pay.locked);
  const payFields = h("fieldset", { class: "lock-fields", disabled: "" }, h("legend", { class: "sr-only" }, c.pay.title), methods, lightning.el, token.el, serverKeyBox);

  const key = keyStep((pubkey) => {
    refundPubkey = pubkey;
    payFields.disabled = pubkey === null;
    lockedHint.hidden = pubkey !== null;
    stepTwo.dataset.locked = String(pubkey === null);
  });

  const stepOne = h("li", { class: "step" }, h("h3", {}, h("span", { class: "step-n", "aria-hidden": "true" }, "1"), c.key.title), key.el);
  const stepTwo = h("li", { class: "step", "data-locked": "true" }, h("h3", {}, h("span", { class: "step-n", "aria-hidden": "true" }, "2"), c.pay.title), lockedHint, payFields);

  const root = h(
    "section",
    { class: "panel", id: "contribute", "aria-labelledby": "contribute-title", tabindex: "-1" },
    h("h2", { id: "contribute-title" }, c.title),
    h("ol", { class: "steps" }, stepOne, stepTwo),
    parts.status,
    parts.downloads,
  );
  return { el: root, dispose: () => { lightning.dispose(); token.dispose(); } };
}
