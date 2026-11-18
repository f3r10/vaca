import { h, svg } from "../ui/dom";

export interface Group { group: string; items: { id: string; label: string }[] }

const ARROW = `<path d="m4 6 4 4 4-4"/>`;

/** Grouped list, select fallback, ?__<param>=, arrows, H to hide. Outside the design system on purpose. */
export function mountSwitcher(param: string, groups: Group[], onChange: (id: string) => void): { list: HTMLElement; select: HTMLElement } {
  const all = groups.flatMap((g) => g.items);
  const fromUrl = new URLSearchParams(location.search).get(param);
  let active = all.find((v) => v.id === fromUrl)?.id ?? all[0].id;

  const nav = h("nav", { class: "variant-picker", "aria-label": "States" });
  const select = h("select", { "aria-label": "State" });
  const buttons = new Map<string, HTMLButtonElement>();

  for (const g of groups) {
    const heading = h("div", { class: "variant-picker-heading", id: `vg-${g.group}` }, svg(`<path d="m6 4 4 4-4 4"/>`, { viewBox: "0 0 16 16", width: "10", height: "10", fill: "none", stroke: "currentColor", "stroke-width": "1.5" }), g.group);
    const group = h("div", { class: "variant-picker-group", role: "group", "aria-labelledby": `vg-${g.group}` }, heading);
    const opt = h("optgroup", { label: g.group });
    for (const v of g.items) {
      const b = h("button", { type: "button", "data-variant": v.id, "aria-pressed": "false" }, v.label);
      b.addEventListener("click", () => set(v.id));
      buttons.set(v.id, b);
      group.append(b);
      opt.append(h("option", { value: v.id }, v.label));
    }
    nav.append(group);
    select.append(opt);
  }
  select.addEventListener("change", () => set((select as HTMLSelectElement).value));
  const selectWrap = h("div", { class: "variant-picker-select" }, select, svg(ARROW, { "aria-hidden": "true", viewBox: "0 0 16 16", width: "12", height: "12", fill: "none", stroke: "currentColor", "stroke-width": "1.5", "stroke-linecap": "round", "stroke-linejoin": "round" }));

  function set(id: string): void {
    active = id;
    for (const [k, b] of buttons) b.setAttribute("aria-pressed", String(k === id));
    (select as HTMLSelectElement).value = id;
    buttons.get(id)?.scrollIntoView({ block: "nearest" });
    const url = new URL(location.href);
    url.searchParams.set(param, id);
    history.replaceState(null, "", url);
    onChange(id);
  }

  document.addEventListener("keydown", (e) => {
    if (e.metaKey || e.ctrlKey || e.altKey || e.shiftKey || e.defaultPrevented) return;
    const target = e.target as HTMLElement | null;
    if (target && /^(INPUT|TEXTAREA|SELECT)$/.test(target.tagName)) return;
    if (target?.isContentEditable) return;
    if (e.key.toLowerCase() === "h") {
      const hidden = nav.hidden;
      nav.hidden = !hidden;
      selectWrap.hidden = !hidden;
      return;
    }
    const step = e.key === "ArrowDown" || e.key === "ArrowRight" ? 1 : e.key === "ArrowUp" || e.key === "ArrowLeft" ? -1 : 0;
    if (!step) return;
    const i = all.findIndex((v) => v.id === active);
    set(all[(i + step + all.length) % all.length].id);
    e.preventDefault();
  });

  set(active);
  return { list: nav, select: selectWrap };
}
