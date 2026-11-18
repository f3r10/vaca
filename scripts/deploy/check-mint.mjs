#!/usr/bin/env node
// Confirm the public mint advertises NUT-11 and a bolt12 melt for sats.
// Usage: VACA_MINT_URL=https://mint.example node scripts/deploy/check-mint.mjs

const mint = process.env.VACA_MINT_URL ?? "";
if (
  !mint.startsWith("https://") ||
  mint.includes("@") ||
  mint.includes(" ") ||
  mint.includes("?") ||
  mint.includes("#")
) {
  console.error("Set VACA_MINT_URL to an https mint with no credentials.");
  process.exit(1);
}

let infoUrl;
try {
  const base = mint.endsWith("/") ? mint : `${mint}/`;
  infoUrl = new URL("v1/info", base);
} catch {
  console.error("VACA_MINT_URL is not a URL.");
  process.exit(1);
}
if (infoUrl.pathname !== "/v1/info") {
  console.error("VACA_MINT_URL must be the mint origin, with no path.");
  process.exit(1);
}

const response = await fetch(infoUrl, {
  headers: { "user-agent": "vaca-check", accept: "application/json" },
});
if (!response.ok) {
  console.error(`The mint info request failed (${response.status}).`);
  process.exit(1);
}

const info = await response.json();
const nuts = info && typeof info.nuts === "object" && info.nuts ? info.nuts : {};
const nut11 = nuts["11"];
if (!nut11 || nut11.supported !== true) {
  console.error("This mint does not advertise NUT-11.");
  process.exit(1);
}

const methods = []
  .concat(nuts["4"] && Array.isArray(nuts["4"].methods) ? nuts["4"].methods : [])
  .concat(nuts["5"] && Array.isArray(nuts["5"].methods) ? nuts["5"].methods : []);
const bolt12 = methods.some((method) => method.method === "bolt12" && method.unit === "sat");
if (!bolt12) {
  console.error("This mint does not advertise a bolt12 melt for sats.");
  process.exit(1);
}

console.log("Mint advertises NUT-11 and bolt12 for sats.");
