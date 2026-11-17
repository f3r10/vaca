import type { Proof } from "@cashu/cashu-ts";
import {
  RequestError,
  createPot,
  getHealth,
  getPot,
  listPots,
  postContribution,
  postRefund,
  postUnimplemented,
  type Health,
  type Pot,
} from "./api";
import { createSecret, secretMatchesPot } from "./lock";
import { CASHU_TS_COMMIT, CASHU_TS_VERSION, CDK_GIT_REV } from "./pins";
import { generateRefundKey, privkeyFromRefundFile, pubkeyFromRefundFile, refundKeyFile, refundKeyMatches } from "./refund-key";
import { RefundSwapError, readRefundProofs, refundSats, refundToken, swapRefundProofs } from "./refund-swap";
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
} from "./contribute";

let refundPubkey: string | null = null;

const app = document.querySelector<HTMLElement>("#app");
if (!app) {
  throw new Error("Missing #app");
}
const root = app;

window.addEventListener("hashchange", () => {
  void render();
});
void render();

async function render(): Promise<void> {
  const id = potIdFromHash();
  root.replaceChildren();
  root.append(banner());
  if (id) {
    await renderPot(id);
    return;
  }
  await renderHome();
}

function potIdFromHash(): string | null {
  const hash = window.location.hash;
  const prefix = "#/pots/";
  if (!hash.startsWith(prefix)) {
    return null;
  }
  const id = decodeURIComponent(hash.slice(prefix.length)).trim();
  return id || null;
}

function banner(): HTMLElement {
  const el = document.createElement("p");
  el.className = "banner";
  el.textContent =
    "Experimental. Regtest or Mutinynet only. This page can create a refund key, lock a token into an open pot, and, after the lock time, swap that contributor's proofs into a token file. It does not create the server key or pay the pot.";
  return el;
}

function trust(): HTMLElement {
  const section = document.createElement("section");
  section.className = "trust";
  section.append(
    heading("How the money is supposed to work"),
    paragraph(
      "Each contributor will hold a refund key in the browser. A backup of that key will be a file you download. A URL that contains the key would leak it, so that will not be the backup.",
    ),
    paragraph(
      "v1, the first working version: the server key is the only pot key. The server can spend the locked ecash by itself. That is custodial for theft. If the server disappears, a contributor who still has the refund key can take their own sats back after the lock time, because the mint checks that key.",
    ),
    paragraph(
      "Contribute reads a token and the refund file, locks proofs to the server key and that refund key, and posts them. The private key stays in the file. Sats above the amount still needed come back as another token. After the lock time, both the pot key and the refund key can spend. The mint settles whichever valid spend it sees first. Take the refund as soon as the lock expires. Refund swaps those proofs at the mint and downloads a token file.",
    ),
    paragraph(
      "The mint is trusted, as with any Cashu mint. Losing the refund key loses the refund. A lock with no refund key would let anyone spend after the deadline. A malformed lock cannot be spent.",
    ),
    paragraph(
      "v2, the pitch, is not built. The organizer and the server would both have to sign, and only toward the destination stored when the pot was created. v1 signatures are SIG_INPUTS. v2 signatures are SIG_ALL, so a signature is tied to one melt quote. Two people who both agree can still spend, including after the deadline.",
    ),
    paragraph(
      "Unused melt fee reserve goes to the organizer as change when the mint pays. The pot shows how many sats came back. A download of those proofs is not on the page yet. The server stops co-signing at the advertised deadline. The mint lock time is that deadline plus 10 minutes. The hard cap is 100,000 sats.",
    ),
  );
  return section;
}

async function renderHome(): Promise<void> {
  const title = document.createElement("h1");
  title.textContent = "Vaca";
  const lede = paragraph(
    "All-or-nothing pots. Contributors lock ecash toward a target. If the target is met, the pot pays the destination committed at creation. If the deadline passes first, each contributor reclaims their own sats. The mint enforces the refund. No smart contract.",
  );
  root.append(title, lede, trust());

  const btn_download_key = document.createElement("button");
  btn_download_key.type = "button";
  btn_download_key.className = "primary";
  btn_download_key.textContent = "Create refund key";
  btn_download_key.addEventListener("click", () => {
	  const key = generateRefundKey();
	  refundPubkey = key.pubkey;
	  const pubkeyBlock = document.createElement("p");
	  pubkeyBlock.textContent = key.pubkey;
	  root.append(pubkeyBlock)
	  downloadText("vaca-refund-key.txt", refundKeyFile(key));
  });
  root.append(btn_download_key)

  let health: Health | null = null;
  try {
    health = await getHealth();
    root.append(healthCard(health));
  } catch (err) {
    root.append(note(errorText(err)));
    root.append(offlinePins());
  }

  root.append(createForm(health));
  const listHost = document.createElement("div");
  root.append(heading("Pots"), listHost);
  try {
    const pots = await listPots();
    listHost.replaceChildren(potList(pots));
  } catch (err) {
    listHost.replaceChildren(note(errorText(err)));
  }
}

function healthCard(health: Health): HTMLElement {
  const card = document.createElement("section");
  card.className = "card";
  card.append(
    heading("This server"),
    line("Experimental", health.experimental ? "yes" : "no"),
    line("Mint URL set", health.mint_url_set ? "yes" : "no"),
    line("Cap", `${health.max_pot_sats} sats`),
    line("Grace", `${health.grace_seconds} seconds`),
    line("Unused fee reserve", health.fee_reserve_recipient),
    line("v1 signature flag", health.sigflag_v1),
    line("v2 signature flag", health.sigflag_v2),
    line("Counts contributions", health.counts_contributions ? "yes" : "no"),
    line("Server key path set", health.server_key_path_set ? "yes" : "no"),
    line("Server key file present", health.server_key_file_present ? "yes" : "no"),
    line("Server pubkey set", health.server_pubkey_set ? "yes" : "no"),
    line("CDK linked into this binary", health.cdk_linked ? "yes" : "no"),
    paragraph(`CDK pin ${health.cdk_rev}. cashu-ts ${health.cashu_ts} (${health.cashu_ts_commit}). The page uses cashu-ts in the browser.`),
  );
  return card;
}

function offlinePins(): HTMLElement {
  return paragraph(
    `Server is down. Pins on this page: CDK ${CDK_GIT_REV}, cashu-ts ${CASHU_TS_VERSION} (${CASHU_TS_COMMIT}).`,
  );
}

function createForm(health: Health | null): HTMLFormElement {
  const form = document.createElement("form");
  const cap = health?.max_pot_sats ?? 100_000;
  form.append(heading("Create a pot"));

  const title = field("Title", "text", "title");
  const kind = document.createElement("label");
  kind.textContent = "Kind";
  const kindSelect = document.createElement("select");
  kindSelect.name = "kind";
  for (const [value, label] of [
    ["cause", "Cause"],
    ["purchase", "Shared purchase"],
    ["split_bill", "Split the bill"],
  ] as const) {
    const option = document.createElement("option");
    option.value = value;
    option.textContent = label;
    kindSelect.append(option);
  }
  kind.append(kindSelect);

  const target = field("Target (sats)", "number", "target");
  const targetInput = target.querySelector("input");
  if (targetInput) {
    targetInput.min = "1";
    targetInput.max = String(cap);
    targetInput.step = "1";
    targetInput.required = true;
    targetInput.value = "1000";
  }

  const deadline = field("Advertised deadline", "datetime-local", "deadline");
  const deadlineInput = deadline.querySelector("input");
  if (deadlineInput) {
    deadlineInput.required = true;
    deadlineInput.value = localInput(hoursFromNow(24 * 7));
  }

  const destinationLabel = document.createElement("span");
  destinationLabel.textContent =
    "Destination. A BOLT12 offer can be stored now and paid later. This box only stores text.";
  const destination = field("", "text", "destination");
  const destinationInput = destination.querySelector("input");
  if (destinationInput) {
    destinationInput.required = true;
    destinationInput.maxLength = 2048;
    destinationInput.autocomplete = "off";
    destinationInput.spellcheck = false;
  }
  destination.prepend(destinationLabel);

  const pubkey = field("Organizer pubkey (optional, compressed hex)", "text", "pubkey");
  const pubkeyInput = pubkey.querySelector("input");
  if (pubkeyInput) {
    pubkeyInput.autocomplete = "off";
    pubkeyInput.spellcheck = false;
    pubkeyInput.placeholder = "02… or 03…, 66 hex characters. This page does not generate it.";
  }

  const preset = document.createElement("button");
  preset.type = "button";
  preset.className = "secondary";
  preset.textContent = "Use the split-the-bill preset";
  preset.addEventListener("click", () => {
    kindSelect.value = "split_bill";
    if (deadlineInput) {
      deadlineInput.value = localInput(hoursFromNow(2));
    }
    updateDestinationLabel();
    destinationInput?.focus();
  });

  const submit = document.createElement("button");
  submit.type = "submit";
  submit.textContent = "Create pot";

  const result = document.createElement("p");
  result.className = "result";

  function updateDestinationLabel(): void {
    destinationLabel.textContent =
      kindSelect.value === "split_bill"
        ? "Destination. BOLT11 invoice for the bill. The preset only fills this form. Payout rules are not implemented."
        : "Destination. A BOLT12 offer can be stored now and paid later. This box only stores text.";
  }
  kindSelect.addEventListener("change", updateDestinationLabel);

  form.append(title, kind, target, deadline, destination, pubkey, preset, submit, result);
  form.addEventListener("submit", (event) => {
    event.preventDefault();
    void (async () => {
      result.textContent = "";
      const deadlineValue = deadlineInput?.value ?? "";
      const deadlineMs = new Date(deadlineValue).getTime();
      const targetValue = Number(targetInput?.value ?? "");
      if (!Number.isInteger(targetValue)) {
        result.textContent = "Target must be a whole number of sats.";
        return;
      }
      if (Number.isNaN(deadlineMs)) {
        result.textContent = "Deadline is not a valid date.";
        return;
      }
      try {
        const pot = await createPot({
          title: valueOf(title),
          kind: kindSelect.value,
          target_sats: targetValue,
          deadline_unix: Math.floor(deadlineMs / 1000),
          destination: valueOf(destination),
          organizer_pubkey: valueOf(pubkey),
        });
        window.location.hash = `#/pots/${encodeURIComponent(pot.id)}`;
      } catch (err) {
        result.textContent = errorText(err);
      }
    })();
  });
  return form;
}

async function renderPot(id: string): Promise<void> {
  const back = document.createElement("a");
  back.href = "#/";
  back.textContent = "All pots";
  root.append(back, trust());
  try {
    const pot = await getPot(id);
    let card = potCard(pot);
    const pubkey = field("compresed pubkey", "text", "pubkey");
    root.append(
      card,
      moneyActions(pot, pubkey, (next) => {
        const replacement = potCard(next);
        card.replaceWith(replacement);
        card = replacement;
      }),
    );
    const pubkeySection = document.createElement("section");
    const button = document.createElement("button");
    button.type = "button";
    button.className = "secondary";
    button.textContent = "generate";
    button.addEventListener("click", () => {
	    if (refundPubkey == null) {
		    const messagealert = document.createElement("h1");
		    messagealert.textContent = "Create a refund key on the home page first.";
		    pubkeySection.append(messagealert)
	    } else {
		    const serverPubkey = valueOf(pubkey);
		    const secret = createSecret(serverPubkey, refundPubkey, pot.locktime_unix);
		    const secretSection = document.createElement("h2");
		    secretSection.textContent = secret;
		    pubkeySection.append(secretSection)
		    const isPot = secretMatchesPot(serverPubkey, refundPubkey, pot.locktime_unix, secret);
		    if (isPot) {
			    const isPotSection = document.createElement("h1");
			    isPotSection.textContent = "matches";
			    pubkeySection.append(isPotSection)
		    }

	    }
    });
    pubkeySection.append(button);
    root.append(pubkeySection)
  } catch (err) {
    root.append(note(errorText(err)));
  }
}

function potCard(pot: Pot): HTMLElement {
  const card = document.createElement("article");
  card.className = "card";
  const title = document.createElement("h1");
  title.textContent = pot.title;
  const destination = document.createElement("p");
  destination.className = "destination";
  destination.textContent = pot.destination;
  card.append(
    title,
    line("Kind", kindLabel(pot.kind)),
    line("Status", pot.status),
    line("Target", `${pot.target_sats} sats`),
    line("Contributions counted", String(pot.contribution_count)),
    line("Locked", `${pot.locked_sats} of ${pot.target_sats} sats`),
    line("Melt fee reserve", pot.fee_quoted && pot.fee_reserve_sats !== null ? `${pot.fee_reserve_sats} sats`: "not quoted yet"),
    ...(pot.quote_amount_sats !== null && pot.quote_amount_sats !== pot.target_sats
      ? [line("Invoice amount", `${pot.quote_amount_sats} sats`)]
      : []),
    line("Still needed", `${pot.sats_still_needed} sats`),
    line("Mint", pot.mint_url === "" ? "not set" : pot.mint_url),
    line("Advertised deadline", formatUnix(pot.deadline_unix)),
    line("Server stops co-signing", formatUnix(pot.co_sign_until_unix)),
    line("Mint lock time (deadline + grace)", formatUnix(pot.locktime_unix)),
    line("Grace", `${pot.grace_seconds} seconds`),
    line("Unused fee reserve", `goes to the ${pot.fee_reserve_recipient}`),
    line("Change", `${pot.change_sats} sats`),
    line("v1 / v2 signature flags", `${pot.sigflag_v1} / ${pot.sigflag_v2}`),
    line("Organizer pubkey", pot.organizer_pubkey ?? "not set"),
    paragraph("Destination, stored as text. Nothing has been paid."),
    destination,
  );
  return card;
}

function moneyActions(initial: Pot, serverKeyField: HTMLLabelElement, showPot: (next: Pot) => void): HTMLElement {
  let pot = initial;
  const box = document.createElement("section");
  box.className = "card";
  const result = document.createElement("p");
  result.className = "result";
  box.append(
    heading("Money"),
    paragraph(
      "Contribute reads a token and the refund key file. It locks up to the amount still needed and posts those proofs. The private key stays in the file. Pay out asks the server to melt. After the lock time, Refund signs with the refund key and swaps the proofs at the mint. The fresh proofs download as a token file.",
    ),
  );
  const tokenInput = document.createElement("input");
  tokenInput.type = "file";
  tokenInput.accept = ".cashu,.json,text/plain";
  const keyInput = document.createElement("input");
  keyInput.type = "file";
  keyInput.accept = ".txt,text/plain";
  let busy = false;
  let pending: { refundPubkey: string; proofs: ContributionProof[] } | null = null;
  let reclaimed: { filename: string; contents: string } | null = null;
  let keepFile: { filename: string; contents: string } | null = null;
  let lockedFile: { filename: string; contents: string } | null = null;

  const contribute = document.createElement("button");
  contribute.type = "button";
  contribute.className = "secondary";
  contribute.textContent = "Contribute";
  const payout = document.createElement("button");
  payout.type = "button";
  payout.className = "secondary";
  payout.textContent = "Pay out";
  const refund = document.createElement("button");
  refund.type = "button";
  refund.className = "secondary";
  refund.textContent = "Refund";
  const postAgain = document.createElement("button");
  postAgain.type = "button";
  postAgain.className = "secondary";
  postAgain.textContent = "Post locked proofs";
  postAgain.hidden = true;
  const saveKeep = downloadButton("Download remainder", () => keepFile);
  const saveLocked = downloadButton("Download locked proofs", () => lockedFile);
  const save = downloadButton("Download token", () => reclaimed);

  function setDisabled(on: boolean): void {
    busy = on;
    contribute.disabled = on || pending !== null;
    payout.disabled = on;
    refund.disabled = on;
    postAgain.disabled = on;
  }

  payout.addEventListener("click", () => {
    void (async () => {
      if (busy) return;
      result.textContent = "";
      try {
        await postUnimplemented(pot.id, "payout");
        result.textContent = "The server accepted a money route. That is a bug in this skeleton.";
      } catch (err) {
        result.textContent = errorText(err);
      }
    })();
  });

  contribute.addEventListener("click", () => {
    void (async () => {
      if (busy || pending) return;
      setDisabled(true);
      result.textContent = "";
      let swapped: { send: Proof[]; keep: Proof[] } | null = null;
      const mintUrl = pot.mint_url;
      try {
        const key = await refundKeyFrom(keyInput.files?.[0]);
        if ("error" in key) {
          result.textContent = key.error;
          return;
        }
        const serverPubkey = valueOf(serverKeyField).trim().toLowerCase();
        if (serverPubkey === "") {
          result.textContent = "Enter the server public key first.";
          return;
        }
        if (!/^(02|03)[0-9a-f]{64}$/.test(serverPubkey)) {
          result.textContent = "The server key is not a compressed public key.";
          return;
        }
        assertPotCanTakeProofs(pot, Math.floor(Date.now() / 1000));
        const chosen = tokenInput.files?.[0];
        if (!chosen) {
          result.textContent = "Choose the token file first.";
          return;
        }
        const token = await readTokenProofs(mintUrl, await chosen.text());
        if (token.unit !== null && token.unit !== "sat") {
          result.textContent = "The token is not in sats.";
          return;
        }
        if (!sameMint(token.mint, mintUrl)) {
          result.textContent = "That token is from a different mint.";
          return;
        }
        const lock: PotLock = {
          serverPubkey,
          refundPubkey: key.pubkey,
          locktimeUnix: pot.locktime_unix,
        };
        const kind = classifyProofs(token.proofs, lock);
        if (kind === "other-lock") {
          result.textContent = "Those proofs are locked to a different key.";
          return;
        }
        if (kind === "mixed") {
          result.textContent = "The token mixes locked and unlocked proofs.";
          return;
        }
        let posting: ContributionProof[];
        if (kind === "this-pot") {
          if (proofSats(token.proofs) > pot.sats_still_needed) {
            result.textContent = "That amount passes the target plus the fee reserve.";
            return;
          }
          posting = contributionProofs(token.proofs);
        } else {
          const amount = contributionAmount(proofSats(token.proofs), pot.sats_still_needed);
          swapped = await lockForPot(mintUrl, token.proofs, amount, lock);
          posting = takeLockedProofs(swapped.send, lock, amount);
        }
        try {
          pot = await postContribution(pot.id, key.pubkey, posting);
        } catch (err) {
          remember(mintUrl, key.pubkey, posting, swapped?.send ?? [], swapped?.keep ?? []);
          throw err;
        }
        showPot(pot);
        const counted = posting.reduce((sum, proof) => sum + proof.amount, 0);
        if (swapped && swapped.keep.length > 0) {
          keepFile = proofsToken(mintUrl, swapped.keep, "vaca-keep.cashu");
          saveKeep.hidden = false;
          downloadText(keepFile.filename, keepFile.contents);
          result.textContent = `Locked ${counted} sats. The pot has counted them. The rest of the token downloaded as a file.`;
        } else {
          result.textContent = `Locked ${counted} sats. The pot has counted them.`;
        }
      } catch (err) {
        if (swapped && pending === null) {
          remember(mintUrl, "", null, swapped.send, swapped.keep);
        }
        const message = contributeErrorText(err);
        result.textContent = pending ? `${message} Post the locked proofs again without another swap.` : message;
      } finally {
        setDisabled(false);
      }
    })();
  });

  postAgain.addEventListener("click", () => {
    void (async () => {
      if (busy || !pending) return;
      setDisabled(true);
      result.textContent = "";
      const posting = pending.proofs;
      const refundPubkey = pending.refundPubkey;
      try {
        pot = await postContribution(pot.id, refundPubkey, posting);
        showPot(pot);
        pending = null;
        postAgain.hidden = true;
        const counted = posting.reduce((sum, proof) => sum + proof.amount, 0);
        result.textContent = `Locked ${counted} sats. The pot has counted them.`;
      } catch (err) {
        result.textContent = contributeErrorText(err);
      } finally {
        setDisabled(false);
      }
    })();
  });

  refund.addEventListener("click", () => {
    void (async () => {
      if (busy) return;
      setDisabled(true);
      result.textContent = "";
      try {
        const key = await refundKeyFrom(keyInput.files?.[0]);
        if ("error" in key) {
          result.textContent = key.error;
          return;
        }
        if (pot.mint_url === "") {
          result.textContent = "Mint URL is not set.";
          return;
        }
        const body = await postRefund(pot.id, key.pubkey);
        const locked = readRefundProofs(body.proofs);
        const fresh = await swapRefundProofs(pot.mint_url, locked, key.privkey);
        reclaimed = refundToken(pot.mint_url, fresh);
        save.hidden = false;
        result.textContent = `Reclaimed ${refundSats(fresh)} sats. Download the token file if the browser did not. The mint has taken the locked proofs.`;
        downloadText(reclaimed.filename, reclaimed.contents);
      } catch (err) {
        result.textContent = err instanceof RefundSwapError ? err.message : errorText(err);
      } finally {
        setDisabled(false);
      }
    })();
  });

  function remember(
    mintUrl: string,
    refundPubkey: string,
    proofs: ContributionProof[] | null,
    send: Proof[],
    keep: Proof[],
  ): void {
    if (keep.length > 0) {
      keepFile = proofsToken(mintUrl, keep, "vaca-keep.cashu");
      saveKeep.hidden = false;
      downloadText(keepFile.filename, keepFile.contents);
    }
    if (send.length > 0) {
      lockedFile = proofsToken(mintUrl, send, "vaca-locked.cashu");
      saveLocked.hidden = false;
      downloadText(lockedFile.filename, lockedFile.contents);
    }
    if (proofs !== null && refundPubkey !== "") {
      pending = { refundPubkey, proofs };
      postAgain.hidden = false;
    }
  }

  box.append(
    serverKeyField,
    fileLabel("Token", tokenInput),
    fileLabel("Refund key", keyInput),
    contribute,
    payout,
    refund,
    postAgain,
    saveLocked,
    saveKeep,
    save,
    result,
  );
  return box;
}

function downloadButton(
  text: string,
  file: () => { filename: string; contents: string } | null,
): HTMLButtonElement {
  const button = document.createElement("button");
  button.type = "button";
  button.className = "secondary";
  button.textContent = text;
  button.hidden = true;
  button.addEventListener("click", () => {
    const chosen = file();
    if (!chosen) return;
    downloadText(chosen.filename, chosen.contents);
  });
  return button;
}

function fileLabel(text: string, input: HTMLInputElement): HTMLLabelElement {
  const label = document.createElement("label");
  label.append(document.createTextNode(text), input);
  return label;
}

async function refundKeyFrom(
  chosen: File | undefined,
): Promise<{ pubkey: string; privkey: string } | { error: string }> {
  if (!chosen) return { error: "Choose the refund key file first." };
  const contents = await chosen.text();
  const pubkey = pubkeyFromRefundFile(contents);
  const privkey = privkeyFromRefundFile(contents);
  if (pubkey === null) return { error: "The file has no public key line." };
  if (privkey === null) return { error: "The file has no private key line." };
  if (!refundKeyMatches(pubkey, privkey)) {
    return { error: "The private key does not match the public key in that file." };
  }
  return { pubkey, privkey };
}

function contributeErrorText(err: unknown): string {
  if (err instanceof ContributeError) return err.message;
  return errorText(err);
}

function downloadText(filename: string, contents: string): void {
  const blob = new Blob([contents], { type: "text/plain" });
  const url = URL.createObjectURL(blob);
  const link = document.createElement("a");
  link.href = url;
  link.download = filename;
  root.append(link);
  link.click();
  link.remove();
  setTimeout(() => URL.revokeObjectURL(url), 0);
}

function potList(pots: Pot[]): HTMLElement {
  if (pots.length === 0) {
    return paragraph("No pots yet.");
  }
  const list = document.createElement("ul");
  for (const pot of pots) {
    const item = document.createElement("li");
    item.className = "card";
    const link = document.createElement("a");
    link.href = `#/pots/${encodeURIComponent(pot.id)}`;
    link.textContent = pot.title;
    item.append(
      link,
      line("Kind", kindLabel(pot.kind)),
      line("Target", `${pot.target_sats} sats`),
      line("Status", pot.status),
      line("Deadline", formatUnix(pot.deadline_unix)),
    );
    list.append(item);
  }
  return list;
}

function kindLabel(kind: string): string {
  if (kind === "cause") return "Cause";
  if (kind === "purchase") return "Shared purchase";
  if (kind === "split_bill") return "Split the bill";
  return kind;
}

function heading(text: string): HTMLElement {
  const el = document.createElement("h2");
  el.textContent = text;
  return el;
}

function paragraph(text: string): HTMLElement {
  const el = document.createElement("p");
  el.textContent = text;
  return el;
}

function note(text: string): HTMLElement {
  const el = paragraph(text);
  el.className = "result";
  return el;
}

function line(label: string, value: string): HTMLElement {
  const row = document.createElement("p");
  row.className = "row";
  const name = document.createElement("span");
  name.textContent = label;
  const val = document.createElement("strong");
  val.textContent = value;
  row.append(name, val);
  return row;
}

function field(label: string, type: string, name: string): HTMLLabelElement {
  const wrap = document.createElement("label");
  wrap.textContent = label;
  const input = document.createElement("input");
  input.type = type;
  input.name = name;
  if (type === "text") {
    input.required = name !== "pubkey";
  }
  wrap.append(input);
  return wrap;
}

function valueOf(label: HTMLLabelElement): string {
  return label.querySelector("input")?.value ?? "";
}

function hoursFromNow(hours: number): Date {
  return new Date(Date.now() + hours * 60 * 60 * 1000);
}

function localInput(date: Date): string {
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}T${pad(date.getHours())}:${pad(date.getMinutes())}`;
}

function formatUnix(unix: number): string {
  return `${new Date(unix * 1000).toLocaleString()} (${unix})`;
}

function errorText(err: unknown): string {
  if (err instanceof RequestError) {
    return `${err.status}: ${err.message}`;
  }
  if (err instanceof Error) {
    return err.message;
  }
  return "Request failed.";
}
