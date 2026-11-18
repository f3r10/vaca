// The "I already have a Cashu token" way to pay. This is the original flow,
// moved out of the big panel so each payment method is one small file.
//
// What it does: read the token, check it is for this pot's mint, then either
//   - lock it to the pot first (an ordinary token), or
//   - post it as is (a token someone already locked to this pot).
// The shared work after that lives in contribute-session.ts.

import {
  assertPotCanTakeProofs,
  classifyProofs,
  contributionAmount,
  inputFeeSats,
  proofSats,
  readTokenProofs,
  sameMint,
  type PotLock,
} from "../contribute";
import { friendly } from "../errors";
import { hostOf } from "../format";
import { contribute as c } from "../strings";
import type { Session } from "./contribute-session";
import { h } from "./dom";

export interface PayForm {
  el: HTMLElement;
  dispose(): void;
}

export function tokenForm(s: Session): PayForm {
  const text = h("textarea", { id: "token-text", rows: "3", spellcheck: "false", autocomplete: "off", placeholder: "cashuB…" }) as HTMLTextAreaElement;
  const file = h("input", { type: "file", id: "token-file", accept: ".cashu,.json,text/plain" }) as HTMLInputElement;
  const pasteBtn = h("button", { type: "button", class: "btn-quiet" }, c.token.paste);
  const go = h("button", { type: "button", class: "btn btn-primary" }, c.token.button);

  // One tap on a phone: read the clipboard instead of long-pressing a text box.
  pasteBtn.addEventListener("click", () => {
    void navigator.clipboard.readText().then(
      (clip) => { text.value = clip.trim(); },
      () => s.say("Your browser blocked clipboard access. Paste into the box instead.", "error"),
    );
  });

  go.addEventListener("click", () => {
    void (async () => {
      if (!s.begin()) return;
      go.disabled = true;
      s.say("");
      try {
        const pot = s.getPot();
        const refund = s.refundPubkey();
        const server = s.serverPubkey();
        if (!refund) return s.say("Back up your refund key first.", "error");
        if (!server) return; // serverPubkey() already said why
        const ppk = pot.input_fee_ppk;
        if (typeof ppk !== "number" || !Number.isSafeInteger(ppk) || ppk < 0) return s.say(c.pay.noFee, "error");
        assertPotCanTakeProofs(pot, Math.floor(Date.now() / 1000));

        const raw = text.value.trim() || (await file.files?.[0]?.text())?.trim() || "";
        if (raw === "") return s.say(c.token.empty, "error");
        go.textContent = "Reading the token…";
        const token = await readTokenProofs(pot.mint_url, raw);
        if (token.unit !== null && token.unit !== "sat") return s.say("That token isn't in sats.", "error");
        if (!sameMint(token.mint, pot.mint_url)) return s.say(`That token is from a different mint. This pot uses ${hostOf(pot.mint_url)}.`, "error");

        const lock: PotLock = { serverPubkey: server, refundPubkey: refund, locktimeUnix: pot.locktime_unix };
        const kind = classifyProofs(token.proofs, lock);
        if (kind === "other-lock") return s.say("Those proofs are locked to a different key.", "error");
        if (kind === "mixed") return s.say("That token mixes locked and unlocked proofs. Use one or the other.", "error");

        let counted: boolean;
        if (kind === "this-pot") {
          const extra = inputFeeSats(ppk, token.proofs.length);
          if (proofSats(token.proofs) > pot.sats_still_needed + extra) return s.say("That is more than the pot still needs. Use a smaller token.", "error");
          go.textContent = "Counting…";
          counted = await s.countLocked(token.proofs);
        } else {
          const amount = contributionAmount(proofSats(token.proofs), pot.sats_still_needed);
          go.textContent = "Swapping with the mint…";
          counted = await s.lockAndCount(token.proofs, amount, { onStep: (step) => { go.textContent = step; } });
        }
        if (counted) {
          text.value = "";
          file.value = "";
        }
      } catch (err) {
        s.say(friendly(err), "error");
      } finally {
        go.textContent = c.token.button;
        go.disabled = false;
        s.end();
      }
    })();
  });

  const el = h(
    "div",
    { class: "pay-form" },
    h("div", { class: "field" },
      h("label", { for: "token-text" }, c.token.label),
      h("p", { class: "hint", id: "token-hint" }, c.token.hint(hostOf(s.getPot().mint_url))),
      text,
      pasteBtn,
      h("label", { for: "token-file", class: "file-label" }, c.token.chooseFile),
      file),
    go,
  );
  return { el, dispose: () => undefined };
}
