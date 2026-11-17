import type { Pot } from "../api";
import { app } from "../strings";
import { h } from "../ui/dom";
import { logoMark } from "../ui/logo";
import { potRow } from "../ui/pot-row";

export function homeView(pots: Pot[], now: number): HTMLElement {
  const list = pots.length
    ? h("ul", { class: "pot-list" }, ...pots.map((p) => h("li", {}, potRow(p, now))))
    : h("div", { class: "empty" }, logoMark(56), h("p", {}, h("strong", {}, app.home.empty)), h("p", { class: "muted" }, app.home.emptyHelp));
  return h(
    "div",
    { class: "page" },
    h("section", { class: "hero" },
      h("h1", { class: "page-title hero-title" }, app.home.title),
      h("p", { class: "lede" }, app.home.lede),
      h("div", { class: "hero-actions" },
        h("a", { class: "btn btn-primary", href: "#/new" }, app.home.create),
        h("a", { class: "btn", href: "#/how" }, app.home.how))),
    h("section", { class: "section" }, h("h2", {}, app.home.pots), list),
  );
}
