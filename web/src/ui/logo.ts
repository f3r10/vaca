import { svg } from "./dom";

/** The brand mark: a spotted milk churn. Theme tokens only. */
export function logoMark(size = 32): SVGSVGElement {
  return svg(
    `<path class="logo-body" d="M40 24 C40 33 24 31 22 46 L18 100 Q18 108 26 108 L74 108 Q82 108 82 100 L78 46 C76 31 60 33 60 24 Z"/>
     <rect class="logo-body" x="35" y="4" width="30" height="9" rx="4.5"/>
     <rect class="logo-body" x="40" y="12" width="20" height="13"/>
     <path class="logo-spot" d="M26 62 q10 -9 17 1 q4 11 -7 15 q-13 2 -10 -16z"/>
     <path class="logo-spot" d="M58 86 q12 -6 19 4 q2 12 -10 14 q-13 -2 -9 -18z"/>`,
    { viewBox: "0 0 100 116", width: String(size), height: String(Math.round(size * 1.16)), "aria-hidden": "true", class: "logo" },
  );
}
