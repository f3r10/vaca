import { app } from "../strings";
import { h } from "./dom";

/** The trust model, in plain words, always visible next to the action it affects. */
export function trustCard(): HTMLElement {
  return h(
    "section",
    { class: "panel trust", "aria-labelledby": "trust-title" },
    h("h2", { id: "trust-title" }, app.pot.trustTitle),
    h("ul", { class: "trust-list" }, ...app.pot.trust.map((line) => h("li", {}, line))),
    h("a", { href: "#/how" }, app.pot.trustLink),
  );
}
