import { createPot } from "../api";
import { friendly } from "../errors";
import { destinationKind, duration, formatDate } from "../format";
import { create as c } from "../strings";
import { h } from "../ui/dom";

// The server's real limits, from server/src/pot.rs.
const MAX_TITLE = 80;
const MAX_DEST = 2048;
const MAX_HORIZON_S = 366 * 24 * 60 * 60;
const PUBKEY_RE = /^0[23][0-9a-fA-F]{64}$/;
const hasControl = (s: string) => /[\u0000-\u001f\u007f-\u009f]/.test(s);

function localInput(d: Date): string {
  const p = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}T${p(d.getHours())}:${p(d.getMinutes())}`;
}

function field(id: string, label: string, hint: string, input: HTMLElement): { el: HTMLElement; error: HTMLElement; hint: HTMLElement } {
  const error = h("p", { class: "field-error", id: `${id}-error`, hidden: "" });
  const hintEl = h("p", { class: "hint", id: `${id}-hint` }, hint);
  input.setAttribute("aria-describedby", `${id}-hint ${id}-error`);
  return { el: h("div", { class: "field" }, h("label", { for: id }, label), hintEl, input, error), error, hint: hintEl };
}

export function createView(cap: number): HTMLElement {
  let kind = "cause";
  let deadlineTouched = false;

  const title = h("input", { type: "text", id: "pot-title", autocomplete: "off", maxlength: String(MAX_TITLE * 2) }) as HTMLInputElement;
  const target = h("input", { type: "text", inputmode: "numeric", id: "pot-target", autocomplete: "off", value: "1000" }) as HTMLInputElement;
  const deadline = h("input", { type: "datetime-local", id: "pot-deadline", value: localInput(new Date(Date.now() + 7 * 86400_000)) }) as HTMLInputElement;
  const dest = h("textarea", { id: "pot-dest", rows: "3", spellcheck: "false", autocomplete: "off" }) as HTMLTextAreaElement;
  const organizer = h("input", { type: "text", id: "pot-organizer", spellcheck: "false", autocomplete: "off" }) as HTMLInputElement;

  const fTitle = field("pot-title", c.titleLabel, c.titleHint(MAX_TITLE), title);
  const fTarget = field("pot-target", c.targetLabel, c.targetHint(cap), target);
  const fDeadline = field("pot-deadline", c.deadlineLabel, c.deadlineHint, deadline);
  const fDest = field("pot-dest", c.destLabel.default, c.destHint.default, dest);
  const fOrg = field("pot-organizer", c.organizerLabel, c.organizerHint, organizer);
  const destLabel = fDest.el.querySelector("label") as HTMLLabelElement;
  const deadlineEcho = h("p", { class: "echo", "aria-live": "polite" });
  const destEcho = h("p", { class: "echo", "aria-live": "polite" });
  fDeadline.el.insertBefore(deadlineEcho, fDeadline.error);
  fDest.el.insertBefore(destEcho, fDest.error);

  const setErr = (f: { error: HTMLElement }, input: HTMLElement, msg: string | null) => {
    f.error.hidden = msg === null;
    f.error.textContent = msg ?? "";
    input.setAttribute("aria-invalid", String(msg !== null));
    return msg;
  };

  const vTitle = () => {
    const v = title.value.trim();
    return setErr(fTitle, title, v === "" ? c.errors.titleEmpty : [...v].length > MAX_TITLE ? c.errors.titleLong(MAX_TITLE) : hasControl(v) ? c.errors.titleControl : null);
  };
  const vTarget = () => {
    const raw = target.value.replace(/[,\s]/g, "");
    const n = Number(raw);
    return setErr(fTarget, target, !/^\d+$/.test(raw) || n < 1 || n > cap ? c.errors.target(cap) : null);
  };
  const vDeadline = () => {
    const ms = new Date(deadline.value).getTime();
    const now = Date.now();
    let msg: string | null = null;
    if (Number.isNaN(ms)) msg = c.errors.deadlineInvalid;
    else if (ms <= now) msg = c.errors.deadlinePast;
    else if ((ms - now) / 1000 > MAX_HORIZON_S) msg = c.errors.deadlineFar;
    deadlineEcho.textContent = msg === null ? `Closes ${formatDate(Math.floor(ms / 1000))}, in ${duration((ms - now) / 1000)}.` : "";
    return setErr(fDeadline, deadline, msg);
  };
  const vDest = () => {
    const v = dest.value.trim();
    const k = destinationKind(v);
    destEcho.textContent = v === "" ? "" : k === "unknown" ? c.destLooksOff : c.destLooksOk(k === "bolt11" ? "BOLT11 invoice" : "BOLT12 offer");
    destEcho.className = `echo${v !== "" && k === "unknown" ? " echo-warn" : ""}`;
    return setErr(fDest, dest, v === "" ? c.errors.destEmpty : [...v].length > MAX_DEST ? c.errors.destLong(MAX_DEST) : hasControl(v) ? c.errors.destControl : null);
  };
  const vOrg = () => {
    const v = organizer.value.trim();
    return setErr(fOrg, organizer, v !== "" && !PUBKEY_RE.test(v) ? c.errors.organizer : null);
  };

  title.addEventListener("input", vTitle); title.addEventListener("blur", vTitle);
  target.addEventListener("input", vTarget); target.addEventListener("blur", vTarget);
  deadline.addEventListener("input", () => { deadlineTouched = true; vDeadline(); });
  dest.addEventListener("input", vDest); dest.addEventListener("blur", vDest);
  organizer.addEventListener("blur", vOrg);

  /* Kind chooser: radios styled as cards. */
  const kinds = h("fieldset", { class: "kinds" }, h("legend", {}, c.kind));
  for (const [value, [label, blurb]] of Object.entries(c.kinds)) {
    const input = h("input", { type: "radio", name: "kind", value, id: `kind-${value}` }) as HTMLInputElement;
    input.checked = value === kind;
    input.addEventListener("change", () => {
      kind = value;
      const split = value === "split_bill";
      destLabel.textContent = split ? c.destLabel.split_bill : c.destLabel.default;
      fDest.hint.textContent = split ? c.destHint.split_bill : c.destHint.default;
      if (split && !deadlineTouched) { deadline.value = localInput(new Date(Date.now() + 2 * 3600_000)); vDeadline(); }
      if (!split && !deadlineTouched) { deadline.value = localInput(new Date(Date.now() + 7 * 86400_000)); vDeadline(); }
    });
    kinds.append(h("label", { class: "kind", for: `kind-${value}` }, input, h("span", { class: "kind-text" }, h("strong", {}, label), h("span", { class: "muted" }, blurb))));
  }

  const summary = h("p", { class: "notice notice-danger", role: "alert", hidden: "" });
  const submit = h("button", { type: "submit", class: "btn btn-primary" }, c.submit);
  const form = h(
    "form",
    { class: "form", novalidate: "" },
    kinds, fTitle.el, fTarget.el, fDeadline.el, fDest.el,
    h("details", { class: "advanced" }, h("summary", {}, c.advanced), fOrg.el),
    summary, submit,
  );
  vDeadline();

  form.addEventListener("submit", (e) => {
    e.preventDefault();
    summary.hidden = true;
    const bad = [vTitle(), vTarget(), vDeadline(), vDest(), vOrg()].some((m) => m !== null);
    if (bad) {
      (form.querySelector('[aria-invalid="true"]') as HTMLElement | null)?.focus();
      return;
    }
    submit.disabled = true;
    submit.textContent = c.submitting;
    void createPot({
      title: title.value.trim(),
      kind,
      target_sats: Number(target.value.replace(/[,\s]/g, "")),
      deadline_unix: Math.floor(new Date(deadline.value).getTime() / 1000),
      destination: dest.value.trim(),
      organizer_pubkey: organizer.value.trim(),
    })
      .then((pot) => { location.hash = `#/pots/${encodeURIComponent(pot.id)}`; })
      .catch((err) => {
        summary.textContent = friendly(err);
        summary.hidden = false;
        submit.disabled = false;
        submit.textContent = c.submit;
      });
  });

  return h("div", { class: "page page-narrow" }, h("h1", { class: "page-title" }, c.title), h("p", { class: "lede" }, c.lede), form);
}
