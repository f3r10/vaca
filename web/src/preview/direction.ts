import "../tokens.css";
import "../base.css";
import "../components.css";
import "./preview.css";
import { h } from "../ui/dom";
import { logoMark } from "../ui/logo";
import { mockPot } from "./mock";
import { potCard } from "../ui/pot-card";

const app = document.querySelector<HTMLElement>("#app");
if (!app) throw new Error("Missing #app");

const SWATCHES: [string, string][] = [
  ["color-bg", "Page"],
  ["color-bg-surface", "Card"],
  ["color-bg-sunken", "Sunken"],
  ["color-border-strong", "Control border"],
  ["color-text", "Text"],
  ["color-text-secondary", "Secondary text"],
  ["color-accent-solid", "Accent"],
  ["color-accent-soft", "Accent soft"],
  ["color-warning-soft", "Warning soft"],
  ["color-danger-soft", "Danger soft"],
];

function themeToggle(): HTMLElement {
  const wrap = h("div", { class: "theme-toggle", role: "group", "aria-label": "Theme" });
  for (const [value, label] of [["auto", "Auto"], ["light", "Light"], ["dark", "Dark"]] as const) {
    const b = h("button", { type: "button", "aria-pressed": String(value === "auto") }, label);
    b.addEventListener("click", () => {
      if (value === "auto") document.documentElement.removeAttribute("data-theme");
      else document.documentElement.dataset.theme = value;
      wrap.querySelectorAll("button").forEach((x) => x.setAttribute("aria-pressed", String(x === b)));
    });
    wrap.append(b);
  }
  return wrap;
}

function swatches(): HTMLElement {
  const grid = h("div", { class: "swatches" });
  for (const [token, label] of SWATCHES) {
    grid.append(
      h(
        "div",
        { class: "swatch" },
        h("div", { class: "swatch-chip", style: `background: var(--${token}); color: var(--color-text)` }, "Aa"),
        h("span", {}, label),
        h("code", { class: "muted" }, `--${token}`),
      ),
    );
  }
  return grid;
}

function typeScale(): HTMLElement {
  const rows: [string, string, string][] = [
    ["hero", "var(--text-hero)", "var(--font-display)"],
    ["2xl", "var(--text-2xl)", "var(--font-display)"],
    ["xl", "var(--text-xl)", "var(--font-display)"],
    ["lg", "var(--text-lg)", "var(--font-body)"],
    ["base", "var(--text-base)", "var(--font-body)"],
    ["sm", "var(--text-sm)", "var(--font-body)"],
    ["xs", "var(--text-xs)", "var(--font-body)"],
  ];
  const box = h("div", { class: "specimen" });
  for (const [name, size, family] of rows) {
    box.append(h("div", { class: "scale-row" }, h("code", {}, name), h("span", { style: `font-size:${size};font-family:${family};line-height:1.15;font-weight:${family.includes("display") ? 600 : 400}` }, name === "hero" ? "84,000 sats" : "A pot that holds until the deadline")));
  }
  return box;
}

function motion(): HTMLElement {
  const bar = h("div", { class: "meter-track", style: "width:14rem" }, h("div", { class: "meter-fill" }));
  const fill = bar.firstElementChild as HTMLElement;
  const replay = h("button", { type: "button", class: "btn" }, "Replay fill");
  const run = () => {
    fill.style.setProperty("--p", "0");
    requestAnimationFrame(() => requestAnimationFrame(() => fill.style.setProperty("--p", "0.62")));
  };
  replay.addEventListener("click", run);
  run();
  return h("div", { class: "motion-demo" }, bar, replay, h("p", { class: "muted" }, "700 ms, ease-out. Becomes instant under prefers-reduced-motion."));
}

function cards(): HTMLElement {
  const now = Math.floor(Date.now() / 1000);
  return h(
    "div",
    { class: "states" },
    h("p", { class: "states-label" }, "Partly funded, fee quoted"),
    potCard(mockPot("partial", now), now),
    h("p", { class: "states-label" }, "Target reached"),
    potCard(mockPot("ready", now), now),
    h("p", { class: "muted" }, "Every state lives on the state page: /states.html"),
  );
}

app.append(
  h(
    "main",
    { class: "direction" },
    h("header", { class: "direction-head" }, h("div", { class: "brand" }, logoMark(36), "Vaca"), themeToggle()),
    h("section", {}, h("h1", { class: "lede" }, "Everyone chips in, or nobody pays."),
      h("p", { class: "muted", style: "max-width:52ch" }, "Direction for the whole app: warm paper, one pasture green, a serif for numbers and titles, a plain sans for everything you read. A milk churn that fills is the one picture. Calm by default, one quiet moment of delight when a pot reaches its target."),
      h("ul", { class: "principles" },
        h("li", {}, h("strong", {}, "Warm, not crypto"), "Cream paper, soft green, no neon, no gradients."),
        h("li", {}, h("strong", {}, "Say the risk"), "The trust model sits in plain words next to the action it affects."),
        h("li", {}, h("strong", {}, "Honest states"), "Nothing shows success the mint has not confirmed."),
        h("li", {}, h("strong", {}, "Phone first"), "One column at 360 px. Wide screens add room, not new layout."),
      )),
    h("section", {}, h("h2", {}, "Colour"), swatches()),
    h("section", {}, h("h2", {}, "Type"), h("p", { class: "muted" }, "Fraunces for titles and amounts, Inter for text. Both self-hosted. Minor-third scale."), typeScale()),
    h("section", {}, h("h2", {}, "Motion"), motion()),
    h("section", {}, h("h2", {}, "Pot card"), h("p", { class: "muted" }, "Numbers lead. The milk churn appears when the target is reached."), cards()),
  ),
);
