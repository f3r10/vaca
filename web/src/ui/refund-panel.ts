import { postRefund, type Pot } from "../api";
import { friendly } from "../errors";
import { formatSats } from "../format";
import { privkeyFromRefundFile, pubkeyFromRefundFile, refundKeyMatches } from "../refund-key";
import { RefundSwapError, readRefundProofs, refundSats, refundToken, swapRefundProofs } from "../refund-swap";
import { h } from "./dom";
import { downloadText } from "./files";

/**
 * Reclaim after the lock time. The private key is read from the chosen file,
 * used to sign the swap inside this handler, and not kept anywhere.
 */
export function refundPanel(getPot: () => Pot, onReclaimed: (sats: number) => void): HTMLElement {
  let busy = false;
  let saved: { filename: string; contents: string } | null = null;

  const keyFile = h("input", { type: "file", id: "reclaim-key-file", accept: ".txt,text/plain" }) as HTMLInputElement;
  const go = h("button", { type: "button", class: "btn btn-primary" }, "Reclaim sats");
  const again = h("button", { type: "button", class: "btn" }, "Download the token again");
  again.hidden = true;
  const status = h("div", { class: "status", role: "status", "aria-live": "polite" });
  const say = (text: string, tone: "wait" | "danger" | "success") => {
    status.replaceChildren(text ? h("p", { class: `notice notice-${tone}` }, text) : "");
  };

  again.addEventListener("click", () => saved && downloadText(saved.filename, saved.contents));

  go.addEventListener("click", () => {
    void (async () => {
      if (busy) return;
      busy = true;
      go.disabled = true;
      go.textContent = "Reclaiming…";
      say("", "wait");
      const pot = getPot();
      try {
        const chosen = keyFile.files?.[0];
        if (!chosen) return say("Choose your refund key file first.", "danger");
        const contents = await chosen.text();
        const pubkey = pubkeyFromRefundFile(contents);
        const privkey = privkeyFromRefundFile(contents);
        if (pubkey === null || privkey === null) return say("That isn't a Vaca refund key file. Choose the file you downloaded when you contributed.", "danger");
        if (!refundKeyMatches(pubkey, privkey)) return say("The two keys in that file don't match. Choose another file.", "danger");
        if (pot.mint_url === "") return say("This pot has no mint set.", "danger");
        const body = await postRefund(pot.id, pubkey);
        const locked = readRefundProofs(body.proofs);
        const fresh = await swapRefundProofs(pot.mint_url, locked, privkey);
        saved = refundToken(pot.mint_url, fresh);
        downloadText(saved.filename, saved.contents);
        again.hidden = false;
        const sats = refundSats(fresh);
        say(`You reclaimed ${formatSats(sats)}. The token downloaded as a file. Import it into a Cashu wallet to spend it.`, "success");
        onReclaimed(sats);
      } catch (err) {
        say(err instanceof RefundSwapError ? err.message : friendly(err), "danger");
      } finally {
        busy = false;
        go.disabled = false;
        go.textContent = "Reclaim sats";
      }
    })();
  });

  return h(
    "section",
    { class: "panel", id: "reclaim", "aria-labelledby": "reclaim-title", tabindex: "-1" },
    h("h2", { id: "reclaim-title" }, "Reclaim your sats"),
    h("p", {}, "The pot ended without paying out, so each contributor can take their own sats back. Choose the refund key file you saved when you contributed."),
    h("p", { class: "fine" }, "Do this soon. After the lock time the pot's key can spend too, and the mint accepts whichever valid spend arrives first."),
    h("div", { class: "field" }, h("label", { for: "reclaim-key-file" }, "Your refund key file"), keyFile),
    go,
    again,
    status,
  );
}
