type Child = Node | string | null | undefined | false;

/** Tiny element builder. Text goes in as text nodes, never as HTML. */
export function h<K extends keyof HTMLElementTagNameMap>(
  tag: K,
  attrs: Record<string, string> = {},
  ...children: Child[]
): HTMLElementTagNameMap[K] {
  const el = document.createElement(tag);
  for (const [k, v] of Object.entries(attrs)) el.setAttribute(k, v);
  for (const c of children) {
    if (c === null || c === undefined || c === false) continue;
    el.append(typeof c === "string" ? document.createTextNode(c) : c);
  }
  return el;
}

const SVG = "http://www.w3.org/2000/svg";

export function svg(markup: string, attrs: Record<string, string> = {}): SVGSVGElement {
  const doc = new DOMParser().parseFromString(`<svg xmlns="${SVG}">${markup}</svg>`, "image/svg+xml");
  const el = document.createElementNS(SVG, "svg");
  for (const [k, v] of Object.entries(attrs)) el.setAttribute(k, v);
  for (const node of Array.from(doc.documentElement.childNodes)) el.append(document.importNode(node, true));
  return el;
}
