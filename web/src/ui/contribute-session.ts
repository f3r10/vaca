// What happens AFTER the page holds some ecash, whichever way it got there.
//
// There are two ways to get ecash: the visitor pastes a token, or pays a
// Lightning invoice and the page mints it. Both end the same way, so that part
// lives here, once:
//
//   unlocked ecash --lockForPot--> locked to this pot --postContribution--> counted
//
// The most important job of this file is not losing money. Several things can
// fail in the middle, and each leaves ecash somewhere that only this page knows
// about. So every failure path saves that ecash as a file the visitor keeps:
//
//   - minted here, then the lock swap fails      -> "Download my sats"       (unlocked)
//   - swap worked, then posting to the server fails -> "Download the locked proofs", plus a retry
//   - swap left a remainder                      -> "Download the remainder" (unlocked change)

import type { Proof } from "@cashu/cashu-ts";
import { postContribution, type Pot } from "../api";
import {
  assertPotCanTakeProofs,
  contributionProofs,
  lockForPot,
  proofsToken,
  takeLockedProofs,
  type ContributionProof,
  type PotLock,
} from "../contribute";
import { friendly } from "../errors";
import { formatSats } from "../format";
import { contribute as c } from "../strings";
import { h } from "./dom";
import { downloadText } from "./files";

type Tone = "info" | "error" | "success";
interface Saved { filename: string; contents: string }

/** What the two payment forms are allowed to ask of the panel. */
export interface Session {
  getPot(): Pot;
  /** The verified public refund key from step 1, or null if step 1 is not done. */
  refundPubkey(): string | null;
  /** The pot's server key, or null (with a message already shown) if it is unusable. */
  serverPubkey(): string | null;
  say(text: string, tone?: Tone): void;
  /** One contribution at a time. Returns false if one is already running. */
  begin(): boolean;
  end(): void;
  /** Lock unlocked ecash to the pot, post it, and show the result. True when counted. */
  lockAndCount(proofs: Proof[], amount: number, options?: { minted?: boolean; onStep?: (text: string) => void }): Promise<boolean>;
  /** Post ecash that is already locked to this pot. True when counted. */
  countLocked(proofs: Proof[]): Promise<boolean>;
}

export interface SessionParts {
  session: Session;
  status: HTMLElement;
  downloads: HTMLElement;
}

export function createSession(
  getPot: () => Pot,
  onPot: (pot: Pot) => void,
  refundPubkey: () => string | null,
  serverKeyFromForm: () => string,
  onCounted: (pot: Pot) => void,
): SessionParts {
  let busy = false;
  let pending: { refundPubkey: string; proofs: ContributionProof[] } | null = null;
  let keep: Saved | null = null;
  let locked: Saved | null = null;
  let minted: Saved | null = null;

  // A live region that exists before it has text, so screen readers announce what we put in.
  const status = h("div", { class: "status", role: "status", "aria-live": "polite" });
  const say = (text: string, tone: Tone = "info") => {
    status.replaceChildren();
    if (!text) return;
    const style = tone === "error" ? "danger" : tone === "success" ? "success" : "wait";
    status.append(h("p", { class: `notice notice-${style}` }, text));
  };

  /* ---- downloads: the safety net ---- */
  const downloads = h("div", { class: "downloads" });
  function saveButton(label: string, file: () => Saved | null): HTMLButtonElement {
    const b = h("button", { type: "button", class: "btn" }, label);
    b.addEventListener("click", () => { const f = file(); if (f) downloadText(f.filename, f.contents); });
    b.hidden = true;
    downloads.append(b);
    return b;
  }
  const keepBtn = saveButton(c.result.keepBtn, () => keep);
  const lockedBtn = saveButton(c.result.lockedBtn, () => locked);
  const mintedBtn = saveButton(c.lightning.saved, () => minted);
  const retryBtn = h("button", { type: "button", class: "btn" }, c.result.retry);
  retryBtn.hidden = true;
  downloads.append(retryBtn);

  function saveFile(kind: "keep" | "locked" | "minted", mintUrl: string, proofs: Proof[]): void {
    if (proofs.length === 0) return;
    const names = { keep: "vaca-keep.cashu", locked: "vaca-locked.cashu", minted: "vaca-minted.cashu" };
    const file = proofsToken(mintUrl, proofs, names[kind]);
    if (kind === "keep") { keep = file; keepBtn.hidden = false; }
    if (kind === "locked") { locked = file; lockedBtn.hidden = false; }
    if (kind === "minted") { minted = file; mintedBtn.hidden = false; }
    downloadText(file.filename, file.contents); // also offered as a button, in case the browser blocks it
  }

  function lockFor(pot: Pot, refund: string): PotLock | null {
    const server = (pot.server_pubkey ?? serverKeyFromForm()).trim().toLowerCase();
    if (!/^(02|03)[0-9a-f]{64}$/.test(server)) return null;
    return { serverPubkey: server, refundPubkey: refund, locktimeUnix: pot.locktime_unix };
  }

  /* ---- counting ---- */
  async function post(pot: Pot, refund: string, posting: ContributionProof[]): Promise<Pot> {
    const next = await postContribution(pot.id, refund, posting);
    onPot(next);
    onCounted(next);
    return next;
  }

  const session: Session = {
    getPot,
    refundPubkey,
    serverPubkey: () => {
      const pot = getPot();
      const key = (pot.server_pubkey ?? serverKeyFromForm()).trim().toLowerCase();
      if (/^(02|03)[0-9a-f]{64}$/.test(key)) return key;
      say(pot.server_pubkey ? c.pay.serverKey.wrong : c.pay.serverKey.bad, "error");
      return null;
    },
    say,
    begin: () => {
      if (busy || pending) return false;
      busy = true;
      return true;
    },
    end: () => { busy = false; },

    async lockAndCount(proofs, amount, options = {}) {
      const pot = getPot();
      const refund = refundPubkey();
      const lock = refund ? lockFor(pot, refund) : null;
      const ppk = pot.input_fee_ppk;
      let swapped: { send: Proof[]; keep: Proof[] } | null = null;
      try {
        if (!refund || !lock) throw new Error("The refund key or the server key is missing.");
        if (typeof ppk !== "number" || !Number.isSafeInteger(ppk) || ppk < 0) throw new Error(c.pay.noFee);
        assertPotCanTakeProofs(pot, Math.floor(Date.now() / 1000));
        options.onStep?.(c.lightning.progress.locking);
        swapped = await lockForPot(pot.mint_url, proofs, amount, lock);
        const posting = takeLockedProofs(swapped.send, lock, amount, ppk);
        options.onStep?.(c.lightning.progress.counting);
        try {
          await post(pot, refund, posting);
        } catch (err) {
          // Locked but not counted: keep the locked proofs so they can be posted again.
          pending = { refundPubkey: refund, proofs: posting };
          retryBtn.hidden = false;
          saveFile("locked", pot.mint_url, swapped.send);
          saveFile("keep", pot.mint_url, swapped.keep);
          throw err;
        }
        const counted = posting.reduce((sum, p) => sum + p.amount, 0);
        if (swapped.keep.length > 0) {
          saveFile("keep", pot.mint_url, swapped.keep);
          say(c.result.countedKeep(formatSats(counted)), "success");
        } else {
          say(c.result.counted(formatSats(counted)), "success");
        }
        return true;
      } catch (err) {
        // The swap never produced locked proofs. Ecash we minted here exists only
        // in this page, so it must be saved before we report the error.
        if (!swapped && options.minted) {
          saveFile("minted", pot.mint_url, proofs);
          say(`${friendly(err)} ${c.lightning.savedNote}`, "error");
        } else {
          const msg = friendly(err);
          say(pending ? `${msg}${c.result.pendingRetry}` : msg, "error");
        }
        return false;
      }
    },

    async countLocked(proofs) {
      const pot = getPot();
      const refund = refundPubkey();
      try {
        if (!refund) throw new Error("The refund key is missing.");
        assertPotCanTakeProofs(pot, Math.floor(Date.now() / 1000));
        const posting = contributionProofs(proofs);
        await post(pot, refund, posting);
        say(c.result.counted(formatSats(posting.reduce((s, p) => s + p.amount, 0))), "success");
        return true;
      } catch (err) {
        say(friendly(err), "error");
        return false;
      }
    },
  };

  retryBtn.addEventListener("click", () => {
    void (async () => {
      if (busy || !pending) return;
      busy = true;
      say("");
      const { proofs, refundPubkey: refund } = pending;
      try {
        await post(getPot(), refund, proofs);
        pending = null;
        retryBtn.hidden = true;
        say(c.result.counted(formatSats(proofs.reduce((s, p) => s + p.amount, 0))), "success");
      } catch (err) {
        say(friendly(err), "error");
      } finally {
        busy = false;
      }
    })();
  });

  return { session, status, downloads };
}
