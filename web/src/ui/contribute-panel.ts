import type { Proof } from "@cashu/cashu-ts";
import { postContribution, type Pot } from "../api";
import {
  ContributeError,
  assertPotCanTakeProofs,
  classifyProofs,
  contributionAmount,
  contributionProofs,
  lockForPot,
  proofSats,
  proofsToken,
  readTokenProofs,
  sameMint,
  takeLockedProofs,
  type ContributionProof,
  type PotLock,
} from "../contribute";
import { friendly } from "../errors";
import { formatSats, hostOf } from "../format";
import {
  generateRefundKey,
  privkeyFromRefundFile,
  pubkeyFromRefundFile,
  refundKeyFile,
  refundKeyMatches,
} from "../refund-key";
import { h } from "./dom";
import { downloadText } from "./files";
import { copyButton } from "./pot-parts";

const KEY_FILENAME = "vaca-refund-key.txt";
const SERVER_KEY_RE = /^(02|03)[0-9a-f]{64}$/;

interface Saved { filename: string; contents: string }

/**
 * Backup step first, then the lock step. The private key exists only inside the
 * click handler that creates the file, and in the key file the person picks later.
 * Nothing here writes a key to storage, the URL, the console or the server.
 */
export function contributePanel(getPot: () => Pot, onPot: (pot: Pot) => void): HTMLElement {
  let backedUp = false;
  let busy = false;
  let pending: { refundPubkey: string; proofs: ContributionProof[] } | null = null;
  let keep: Saved | null = null;
  let locked: Saved | null = null;

  const status = h("div", { class: "status", role: "status", "aria-live": "polite" });
  const say = (text: string, tone: "info" | "error" | "success" = "info") => {
    status.replaceChildren();
    if (text) status.append(h("p", { class: `notice notice-${tone === "error" ? "danger" : tone === "success" ? "success" : "wait"}` }, text));
  };

  /* ---- Step 1: create and back up the refund key ---- */
  const keyBody = h("div", { class: "step-body" });
  const confirm = h("input", { type: "checkbox", id: "backup-confirm" }) as HTMLInputElement;
  const confirmLabel = h("label", { class: "check", for: "backup-confirm" }, confirm, "I saved the key file somewhere I won't lose it");
  const createBtn = h("button", { type: "button", class: "btn btn-primary" }, "Create refund key and download it");
  const haveLink = h("button", { type: "button", class: "btn-quiet" }, "I already have a key file");
  const keyResult = h("div", { class: "key-result", hidden: "" });

  createBtn.addEventListener("click", () => {
    const key = generateRefundKey();
    downloadText(KEY_FILENAME, refundKeyFile(key));
    keyResult.hidden = false;
    keyResult.replaceChildren(
      h("p", { class: "notice notice-wait" }, `Downloaded as ${KEY_FILENAME}. This is the only copy.`),
      h("p", { class: "fine" }, "Public key (safe to share):"),
      h("p", { class: "mono wrap key-pub" }, key.pubkey),
      copyButton(key.pubkey, "public key"),
    );
    createBtn.textContent = "Create a different key";
    confirmLabel.hidden = false;
    confirm.checked = false;
    setBackedUp(false);
  });
  haveLink.addEventListener("click", () => {
    setBackedUp(true);
    haveLink.hidden = true;
  });
  confirm.addEventListener("change", () => setBackedUp(confirm.checked));
  confirmLabel.hidden = true;

  keyBody.append(
    h("p", {}, "This key is how you get your sats back if the pot misses its target. Vaca can't recover it for you, so losing the file means losing your refund."),
    h("p", { class: "fine" }, "Keep it somewhere safe, like a password manager or cloud drive. It never leaves this browser except as this download."),
    createBtn,
    keyResult,
    confirmLabel,
    haveLink,
  );

  /* ---- Step 2: token and key file ---- */
  const tokenText = h("textarea", { id: "token-text", rows: "3", spellcheck: "false", autocomplete: "off", placeholder: "cashuB…" }) as HTMLTextAreaElement;
  const tokenFile = h("input", { type: "file", id: "token-file", accept: ".cashu,.json,text/plain" }) as HTMLInputElement;
  const keyFile = h("input", { type: "file", id: "key-file", accept: ".txt,text/plain" }) as HTMLInputElement;
  const serverKeyInput = h("input", { type: "text", id: "server-key", spellcheck: "false", autocomplete: "off", placeholder: "02… or 03…" }) as HTMLInputElement;
  const lockBtn = h("button", { type: "button", class: "btn btn-primary" }, "Lock sats in this pot");
  const retryBtn = h("button", { type: "button", class: "btn" }, "Post the locked proofs again");
  retryBtn.hidden = true;
  const downloads = h("div", { class: "downloads" });

  const serverKeyAdvanced = h(
    "details",
    { class: "advanced" },
    h("summary", {}, "Advanced: server key"),
    h("p", { class: "fine" }, "This server didn't publish its key, so paste it here. It's the compressed public key proofs are locked to."),
    h("label", { for: "server-key" }, "Server public key"),
    serverKeyInput,
  );

  const lockFields = h("fieldset", { class: "lock-fields", disabled: "" });
  function refreshFields(): void {
    const pot = getPot();
    serverKeyAdvanced.hidden = !!pot.server_pubkey;
    lockFields.disabled = !backedUp || busy;
    lockBtn.disabled = busy || pending !== null;
    retryBtn.disabled = busy;
  }
  function setBackedUp(v: boolean): void {
    backedUp = v;
    stepTwo.dataset.locked = String(!v);
    refreshFields();
  }

  lockFields.append(
    h("div", { class: "field" },
      h("label", { for: "token-text" }, "Your ecash token"),
      h("p", { class: "hint", id: "token-hint" }, `From a Cashu wallet that uses this pot's mint (${hostOf(getPot().mint_url)}). Paste it or choose a file.`),
      tokenText,
      h("label", { for: "token-file", class: "file-label" }, "Or choose a token file"),
      tokenFile),
    h("div", { class: "field" },
      h("label", { for: "key-file" }, "Your refund key file"),
      h("p", { class: "hint" }, "Choose the file you saved in step 1. Vaca checks it works now, so you aren't surprised later."),
      keyFile),
    serverKeyAdvanced,
    lockBtn,
    retryBtn,
  );

  const stepOne = h("li", { class: "step" }, h("h3", {}, h("span", { class: "step-n" }, "1"), "Back up a refund key"), keyBody);
  const stepTwo = h("li", { class: "step", "data-locked": "true" }, h("h3", {}, h("span", { class: "step-n" }, "2"), "Lock your sats"), lockFields);

  function saveButton(label: string, file: () => Saved | null): HTMLButtonElement {
    const b = h("button", { type: "button", class: "btn" }, label);
    b.addEventListener("click", () => { const f = file(); if (f) downloadText(f.filename, f.contents); });
    return b;
  }
  const keepBtn = saveButton("Download the remainder", () => keep);
  const lockedBtn = saveButton("Download the locked proofs", () => locked);
  keepBtn.hidden = true;
  lockedBtn.hidden = true;
  downloads.append(keepBtn, lockedBtn);

  async function readKeyFile(): Promise<{ pubkey: string } | { error: string }> {
    const chosen = keyFile.files?.[0];
    if (!chosen) return { error: "Choose your refund key file first." };
    const contents = await chosen.text();
    const pubkey = pubkeyFromRefundFile(contents);
    const privkey = privkeyFromRefundFile(contents);
    if (pubkey === null) return { error: "That file has no public key line. Choose the file Vaca downloaded." };
    if (privkey === null) return { error: "That file has no private key line. Choose the file Vaca downloaded." };
    if (!refundKeyMatches(pubkey, privkey)) return { error: "The two keys in that file don't match, so it couldn't refund you. Choose another file." };
    return { pubkey }; // The private key string goes out of scope here.
  }

  function remember(mintUrl: string, refundPubkey: string, proofs: ContributionProof[] | null, send: Proof[], keepProofs: Proof[]): void {
    if (keepProofs.length > 0) {
      keep = proofsToken(mintUrl, keepProofs, "vaca-keep.cashu");
      keepBtn.hidden = false;
      downloadText(keep.filename, keep.contents);
    }
    if (send.length > 0) {
      locked = proofsToken(mintUrl, send, "vaca-locked.cashu");
      lockedBtn.hidden = false;
      downloadText(locked.filename, locked.contents);
    }
    if (proofs !== null && refundPubkey !== "") {
      pending = { refundPubkey, proofs };
      retryBtn.hidden = false;
    }
  }

  function setBusy(v: boolean, label?: string): void {
    busy = v;
    lockBtn.textContent = v && label ? label : "Lock sats in this pot";
    refreshFields();
  }

  lockBtn.addEventListener("click", () => {
    void (async () => {
      if (busy || pending) return;
      setBusy(true, "Locking…");
      say("");
      let swapped: { send: Proof[]; keep: Proof[] } | null = null;
      let pot = getPot();
      const mintUrl = pot.mint_url;
      try {
        const key = await readKeyFile();
        if ("error" in key) return say(key.error, "error");
        const serverPubkey = (pot.server_pubkey ?? serverKeyInput.value).trim().toLowerCase();
        if (!SERVER_KEY_RE.test(serverPubkey)) {
          return say(pot.server_pubkey ? "The server's key looks wrong. Tell whoever runs this pot." : "The server key must be 66 hex characters starting with 02 or 03.", "error");
        }
        assertPotCanTakeProofs(pot, Math.floor(Date.now() / 1000));
        const raw = tokenText.value.trim() || (await tokenFile.files?.[0]?.text())?.trim() || "";
        if (raw === "") return say("Paste your token or choose a token file.", "error");
        const token = await readTokenProofs(mintUrl, raw);
        if (token.unit !== null && token.unit !== "sat") return say("That token isn't in sats.", "error");
        if (!sameMint(token.mint, mintUrl)) return say(`That token is from a different mint. This pot uses ${hostOf(mintUrl)}.`, "error");
        const lock: PotLock = { serverPubkey, refundPubkey: key.pubkey, locktimeUnix: pot.locktime_unix };
        const kind = classifyProofs(token.proofs, lock);
        if (kind === "other-lock") return say("Those proofs are locked to a different key.", "error");
        if (kind === "mixed") return say("That token mixes locked and unlocked proofs. Use one or the other.", "error");
        let posting: ContributionProof[];
        if (kind === "this-pot") {
          if (proofSats(token.proofs) > pot.sats_still_needed) return say("That is more than the pot still needs. Use a smaller token.", "error");
          posting = contributionProofs(token.proofs);
        } else {
          const amount = contributionAmount(proofSats(token.proofs), pot.sats_still_needed);
          setBusy(true, "Swapping with the mint…");
          swapped = await lockForPot(mintUrl, token.proofs, amount, lock);
          posting = takeLockedProofs(swapped.send, lock, amount);
        }
        setBusy(true, "Counting…");
        try {
          pot = await postContribution(pot.id, key.pubkey, posting);
        } catch (err) {
          remember(mintUrl, key.pubkey, posting, swapped?.send ?? [], swapped?.keep ?? []);
          throw err;
        }
        onPot(pot);
        tokenText.value = "";
        const counted = posting.reduce((sum, p) => sum + p.amount, 0);
        if (swapped && swapped.keep.length > 0) {
          remember(mintUrl, "", null, [], swapped.keep);
          say(`Locked ${formatSats(counted)} and the pot counted them. The rest of your token downloaded as a file. Keep it: those sats are yours.`, "success");
        } else {
          say(`Locked ${formatSats(counted)} and the pot counted them. Keep your refund key file until the pot ends.`, "success");
        }
      } catch (err) {
        if (swapped && pending === null) remember(mintUrl, "", null, swapped.send, swapped.keep);
        const msg = err instanceof ContributeError ? err.message : friendly(err);
        say(pending ? `${msg} Your locked proofs downloaded as a file. You can post them again without another swap.` : msg, "error");
      } finally {
        setBusy(false);
      }
    })();
  });

  retryBtn.addEventListener("click", () => {
    void (async () => {
      if (busy || !pending) return;
      setBusy(true, "Counting…");
      say("");
      const { proofs, refundPubkey } = pending;
      try {
        const pot = await postContribution(getPot().id, refundPubkey, proofs);
        onPot(pot);
        pending = null;
        retryBtn.hidden = true;
        say(`Locked ${formatSats(proofs.reduce((s, p) => s + p.amount, 0))} and the pot counted them.`, "success");
      } catch (err) {
        say(friendly(err), "error");
      } finally {
        setBusy(false);
      }
    })();
  });

  refreshFields();
  return h(
    "section",
    { class: "panel", id: "contribute", "aria-labelledby": "contribute-title", tabindex: "-1" },
    h("h2", { id: "contribute-title" }, "Contribute"),
    h("ol", { class: "steps" }, stepOne, stepTwo),
    status,
    downloads,
  );
}
