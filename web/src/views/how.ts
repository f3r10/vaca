import { how } from "../strings";
import { h } from "../ui/dom";

function node(n: string, title: string, body: string): HTMLElement {
  return h("div", { class: "flow-node" }, h("span", { class: "step-n" }, n), h("div", {}, h("h3", {}, title), h("p", {}, body)));
}

/** Four steps and a branching diagram. The steps are real HTML, so the diagram needs no alt text. */
export function howView(): HTMLElement {
  const [lock, met, pay, reclaim] = how.steps;
  return h(
    "div",
    { class: "page" },
    h("h1", { class: "page-title" }, how.title),
    h("p", { class: "lede" }, how.lede),
    h(
      "div",
      { class: "flow" },
      node("1", lock[0], lock[1]),
      h(
        "div",
        { class: "flow-lanes" },
        h("div", { class: "flow-lane lane-good" }, h("p", { class: "lane-label" }, how.lanes.good), node("2", met[0], met[1]), h("span", { class: "flow-arrow", "aria-hidden": "true" }, "↓"), node("3", pay[0], pay[1])),
        h("div", { class: "flow-lane lane-bad" }, h("p", { class: "lane-label" }, how.lanes.bad), node("4", reclaim[0], reclaim[1])),
      ),
    ),
    h("section", { class: "section" }, h("h2", {}, how.whoTitle), h("div", { class: "who" }, ...how.who.map(([when, what]) => h("div", { class: "who-row" }, h("h3", {}, when), h("p", {}, what))))),
    h("section", { class: "section" }, h("h2", {}, how.trustTitle), h("ul", { class: "trust-list" }, ...how.trust.map((l) => h("li", {}, l)))),
    h("p", {}, h("a", { href: "#/" }, how.back)),
  );
}
