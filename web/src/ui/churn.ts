import { svg } from "./dom";

const BODY = "M40 24 C40 33 24 31 22 46 L18 100 Q18 108 26 108 L74 108 Q82 108 82 100 L78 46 C76 31 60 33 60 24 Z";

let uid = 0;

/** A milk churn. `fraction` is how full it is (0 to 1). Fill animates when `--p` changes. */
export function churn(fraction: number, label: string): SVGSVGElement {
  const id = `churn-${uid++}`;
  const el = svg(
    `<defs><clipPath id="${id}"><path d="${BODY}"/></clipPath></defs>
     <path class="churn-handle" d="M22 48 q-13 0 -13 12 q0 11 11 11"/>
     <path class="churn-handle" d="M78 48 q13 0 13 12 q0 11 -11 11"/>
     <rect class="churn-lid" x="35" y="4" width="30" height="9" rx="4.5"/>
     <rect class="churn-neck" x="40" y="12" width="20" height="13"/>
     <path class="churn-body" d="${BODY}"/>
     <g clip-path="url(#${id})">
       <path class="churn-spot" d="M26 62 q10 -9 17 1 q4 11 -7 15 q-13 2 -10 -16z"/>
       <path class="churn-spot" d="M58 86 q12 -6 19 4 q2 12 -10 14 q-13 -2 -9 -18z"/>
       <rect class="churn-fill" x="0" y="24" width="100" height="84"/>
       <rect class="churn-shine" x="-30" y="24" width="18" height="84"/>
     </g>
     <path class="churn-outline" d="${BODY}"/>
     <rect class="churn-foot" x="15" y="106" width="70" height="6" rx="3"/>`,
    { viewBox: "0 0 100 116", role: "img", "aria-label": label, class: "churn" },
  );
  const fill = el.querySelector<SVGElement>(".churn-fill");
  if (fill) {
    fill.style.setProperty("--p", "0");
    requestAnimationFrame(() => requestAnimationFrame(() => fill.style.setProperty("--p", String(fraction))));
  }
  return el;
}
