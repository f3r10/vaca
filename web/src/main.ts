import "./tokens.css";
import "./base.css";
import "./components.css";
import "./app.css";
import { RequestError, getHealth, getPot, listPots } from "./api";
import { friendly, isNetworkError } from "./errors";
import { app as s } from "./strings";
import { h } from "./ui/dom";
import { logoMark } from "./ui/logo";
import { createView } from "./views/create";
import { homeView } from "./views/home";
import { howView } from "./views/how";
import { potView } from "./views/pot";

const mount = document.querySelector<HTMLElement>("#app");
if (!mount) throw new Error("Missing #app");

const main = h("main", { id: "main", tabindex: "-1" });
const nav = h("nav", { class: "nav", "aria-label": "Main" },
  h("a", { href: "#/", "data-route": "home" }, s.nav.home),
  h("a", { href: "#/how", "data-route": "how" }, s.nav.how),
  h("a", { class: "nav-cta", href: "#/new", "data-route": "new" }, s.nav.create));
// The public deploy sets VITE_PUBLIC_DEMO=1. Local runs against a regtest mint do not show this.
const demoBanner =
  import.meta.env.VITE_PUBLIC_DEMO === "1"
    ? h("aside", { class: "demo-banner", "aria-label": s.demo.label },
        h("span", { class: "demo-banner-icon", "aria-hidden": "true" }, "!"),
        h("p", {}, `${s.demo.text} `, h("a", { href: "#/how" }, s.demo.link)))
    : null;

mount.append(
  h("a", { class: "skip", href: "#main" }, "Skip to content"),
  ...(demoBanner ? [demoBanner] : []),
  h("header", { class: "site-header" }, h("a", { class: "brand", href: "#/" }, logoMark(26), "Vaca"), nav),
  main,
  h("footer", { class: "site-footer" },
    h("p", { role: "note" }, s.banner),
    h("p", {}, h("a", { href: s.github.url, target: "_blank", rel: "noopener noreferrer" }, s.github.label))),
);

let disposeView: (() => void) | null = null;
let routeId = 0;
const reduced = () => matchMedia("(prefers-reduced-motion: reduce)").matches;

function swap(el: HTMLElement, title: string, route: string, dispose: (() => void) | null = null): void {
  const apply = () => {
    disposeView?.();
    disposeView = dispose;
    main.replaceChildren(el);
    document.title = `${title} · Vaca`;
    nav.querySelectorAll("a").forEach((a) => (a.dataset.route === route ? a.setAttribute("aria-current", "page") : a.removeAttribute("aria-current")));
    window.scrollTo(0, 0);
    (main.querySelector("h1") as HTMLElement | null)?.setAttribute("tabindex", "-1");
    (main.querySelector("h1") as HTMLElement | null)?.focus({ preventScroll: true });
  };
  if (typeof document.startViewTransition === "function" && !reduced()) {
    const vt = document.startViewTransition(apply);
    // A transition can be skipped (hidden tab, quick navigation). The update still runs.
    vt.ready.catch(() => undefined);
    vt.finished.catch(() => undefined);
  }
  else apply();
}

function errorView(err: unknown, retry: () => void): HTMLElement {
  const notFound = err instanceof RequestError && err.status === 404;
  const retryBtn = h("button", { type: "button", class: "btn btn-primary" }, s.retry);
  retryBtn.addEventListener("click", retry);
  return h("div", { class: "page page-narrow" },
    h("h1", { class: "page-title" }, notFound ? "Pot not found" : isNetworkError(err) ? s.serverDown : "Something went wrong"),
    h("p", { class: "lede" }, notFound ? friendly(err) : isNetworkError(err) ? s.serverDownHelp : friendly(err)),
    h("div", { class: "actions" }, notFound ? h("a", { class: "btn btn-primary", href: "#/" }, s.pot.back) : retryBtn));
}

async function route(): Promise<void> {
  const id = ++routeId;
  const hash = location.hash.replace(/^#/, "") || "/";
  const now = Math.floor(Date.now() / 1000);
  try {
    if (hash === "/how") return swap(howView(), s.nav.how, "how");
    if (hash === "/new") {
      let cap = 100_000;
      try { cap = (await getHealth()).max_pot_sats; } catch { /* the server's own check still applies */ }
      if (id !== routeId) return;
      return swap(createView(cap), s.nav.create, "new");
    }
    const m = hash.match(/^\/pots\/([^/?]+)/);
    if (m) {
      const pot = await getPot(decodeURIComponent(m[1]));
      if (id !== routeId) return;
      const view = potView(pot);
      return swap(view.el, pot.title, "", view.dispose);
    }
    const pots = await listPots();
    if (id !== routeId) return;
    swap(homeView(pots, now), s.nav.home, "home");
  } catch (err) {
    if (id !== routeId) return;
    swap(errorView(err, () => void route()), "Error", "");
  }
}

window.addEventListener("hashchange", () => void route());
void route();
