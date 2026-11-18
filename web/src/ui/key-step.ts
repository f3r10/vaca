// Step 1 of contributing: a refund key the visitor has saved AND proved they can read back.
//
// Before, step 1 ended with a "I saved it" checkbox that took the visitor's word,
// and the key file was asked for again at the lock step. Now the file is read
// once, here, right after the download. That does two things:
//   - it proves the backup works while nothing is at risk (a blank or wrong
//     file is caught now, not after sats are locked), and
//   - the lock step no longer needs the file at all, because from here on only
//     the PUBLIC key is kept, in memory.
//
// Key rules (the same as the README):
//   - the private key is never sent anywhere, never stored, never shown;
//   - it exists in the click handler that creates the file, and as a string
//     inside readKeyFile() below while the chosen file is checked;
//   - this module hands the rest of the page only the public key.

import { contribute as c } from "../strings";
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

export interface KeyStep {
  el: HTMLElement;
  /** The verified public key, or null until a file has been checked. */
  pubkey(): string | null;
}

export function keyStep(onChange: (pubkey: string | null) => void): KeyStep {
  let verified: string | null = null;
  let createdPubkey: string | null = null; // public half of a key made in this session

  const createBtn = h("button", { type: "button", class: "btn btn-primary" }, c.key.create);
  const haveBtn = h("button", { type: "button", class: "btn-quiet" }, c.key.haveFile);
  const changeBtn = h("button", { type: "button", class: "btn-quiet" }, c.key.change);
  changeBtn.hidden = true;

  const created = h("div", { class: "key-result", hidden: "" });
  const fileLabel = h("label", { for: "key-verify-file" }, c.key.verifyLabel);
  const fileHint = h("p", { class: "hint" }, c.key.verifyHint);
  const fileInput = h("input", { type: "file", id: "key-verify-file", accept: ".txt,text/plain" }) as HTMLInputElement;
  const verify = h("div", { class: "field", hidden: "" }, fileLabel, fileHint, fileInput);
  // A live region that exists before it has text, so screen readers announce updates.
  const result = h("div", { class: "status", role: "status", "aria-live": "polite" });

  const say = (text: string, tone: "success" | "danger" | "wait") => {
    result.replaceChildren(text ? h("p", { class: `notice notice-${tone}` }, text) : "");
  };

  function setVerified(pubkey: string | null): void {
    verified = pubkey;
    changeBtn.hidden = pubkey === null;
    createBtn.hidden = pubkey !== null;
    haveBtn.hidden = pubkey !== null;
    verify.hidden = pubkey !== null || (createdPubkey === null && !wantsFile);
    onChange(pubkey);
  }

  let wantsFile = false; // the visitor chose "I already have a key file"

  createBtn.addEventListener("click", () => {
    // The private key exists only inside this handler. The file goes to the
    // browser's download; only the public half is kept and shown.
    const key = generateRefundKey();
    downloadText(KEY_FILENAME, refundKeyFile(key));
    createdPubkey = key.pubkey;
    wantsFile = false;
    created.hidden = false;
    created.replaceChildren(
      h("p", { class: "notice notice-wait" }, c.key.saving(KEY_FILENAME)),
      h("p", { class: "fine" }, c.key.where),
      h("p", { class: "fine" }, c.key.pubLabel),
      h("p", { class: "mono wrap key-pub" }, key.pubkey),
      copyButton(key.pubkey, "public key"),
    );
    createBtn.textContent = c.key.createAgain;
    fileInput.value = "";
    fileLabel.textContent = c.key.verifyLabel;
    fileHint.hidden = false;
    say("", "wait");
    setVerified(null);
    verify.hidden = false;
  });

  haveBtn.addEventListener("click", () => {
    wantsFile = true;
    created.hidden = true;
    fileLabel.textContent = c.key.haveFileLabel;
    fileHint.hidden = true;
    haveBtn.hidden = true;
    verify.hidden = false;
  });

  changeBtn.addEventListener("click", () => {
    fileInput.value = "";
    createdPubkey = null;
    wantsFile = false;
    created.hidden = true;
    createBtn.textContent = c.key.create;
    say("", "wait");
    setVerified(null);
  });

  fileInput.addEventListener("change", () => {
    void (async () => {
      const chosen = fileInput.files?.[0];
      if (!chosen) return;
      const outcome = await readKeyFile(chosen);
      if ("error" in outcome) {
        say(outcome.error, "danger");
        setVerified(null);
        verify.hidden = false;
        return;
      }
      const note = createdPubkey !== null && createdPubkey !== outcome.pubkey ? ` ${c.key.differs}` : "";
      say(`${c.key.ok(outcome.pubkey.slice(0, 12))}${note}`, "success");
      setVerified(outcome.pubkey);
    })();
  });

  const el = h(
    "div",
    { class: "step-body" },
    h("p", {}, c.key.intro),
    h("p", { class: "fine" }, c.key.safe),
    createBtn,
    created,
    verify,
    result,
    changeBtn,
    haveBtn,
  );
  return { el, pubkey: () => verified };
}

// Read the chosen file and check it is a usable refund key. The private key is
// only a local string here: it is compared to the public key and then dropped.
async function readKeyFile(file: File): Promise<{ pubkey: string } | { error: string }> {
  const contents = await file.text();
  const pubkey = pubkeyFromRefundFile(contents);
  const privkey = privkeyFromRefundFile(contents);
  if (pubkey === null) return { error: c.key.errors.noPub };
  if (privkey === null) return { error: c.key.errors.noPriv };
  if (!refundKeyMatches(pubkey, privkey)) return { error: c.key.errors.mismatch };
  return { pubkey };
}
