import { svg } from "./dom";

/** A round pot with two cow spots. Uses currentColor and theme tokens only. */
export function logoMark(size = 32): SVGSVGElement {
  return svg(
    `<circle cx="20" cy="20" r="18" class="logo-disc"/>
     <path class="logo-spot" d="M9 14 q6 -6 11 -1 q3 6 -3 9 q-9 3 -8 -8z"/>
     <path class="logo-spot" d="M25 22 q7 -3 10 4 q-1 7 -8 9 q-7 -2 -2 -13z"/>
     <circle cx="20" cy="20" r="18" class="logo-ring"/>`,
    { viewBox: "0 0 40 40", width: String(size), height: String(size), "aria-hidden": "true", class: "logo" },
  );
}
