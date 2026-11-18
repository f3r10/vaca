// Draw a Lightning invoice as a QR code a phone wallet can scan.
//
// Two small functions:
//   qrMatrix(text)  pure: text in, a grid of dark/light squares out (no DOM, easy to test)
//   qrCode(text)    draws that grid as an SVG
//
// Why uppercase: a QR code has a compact "alphanumeric" mode that only allows
// 0-9, A-Z and a few symbols. A bech32 invoice is letters and digits, and
// invoices are case-insensitive, so uppercasing it lets the code use that mode.
// The result is a smaller, easier-to-scan picture. This is the same trick
// Lightning wallets use for their own QR codes.

import qrcode from "qrcode-generator";
import { svg } from "./dom.ts";

// The characters the compact mode accepts.
const ALPHANUMERIC = /^[0-9A-Z $%*+\-./:]+$/;

// A quiet zone of 4 empty squares around the code is part of the QR standard.
// Scanners need it to find the edges.
const QUIET_ZONE = 4;

/** The squares of the QR code: true is a dark square. Null if the text cannot be encoded. */
export function qrMatrix(text: string): boolean[][] | null {
  const upper = text.toUpperCase();
  try {
    // Type 0 picks the smallest size that fits. Level L (about 7% repair) keeps
    // the code as small as possible, which matters for long invoices. A screen
    // does not get scratched or smudged like paper.
    const qr = qrcode(0, "L");
    qr.addData(upper, ALPHANUMERIC.test(upper) ? "Alphanumeric" : "Byte");
    qr.make();
    const size = qr.getModuleCount();
    const rows: boolean[][] = [];
    for (let r = 0; r < size; r += 1) {
      const row: boolean[] = [];
      for (let c = 0; c < size; c += 1) row.push(qr.isDark(r, c));
      rows.push(row);
    }
    return rows;
  } catch {
    return null; // too long for any QR code, so the caller shows only the text and Copy
  }
}

/** The QR code as an SVG, or null if the text cannot be encoded. */
export function qrCode(text: string, label: string): SVGSVGElement | null {
  const rows = qrMatrix(text);
  if (!rows) return null;
  const size = rows.length + QUIET_ZONE * 2;
  // One path for every dark square: "move to the square, draw a 1x1 box".
  // The data only ever becomes numbers in this path, never markup.
  let path = "";
  rows.forEach((row, r) => row.forEach((dark, c) => {
    if (dark) path += `M${c + QUIET_ZONE} ${r + QUIET_ZONE}h1v1h-1z`;
  }));
  // Black on white whatever the theme. Scanners expect dark on light, so this
  // is deliberately not a theme colour.
  return svg(`<rect width="${size}" height="${size}" fill="#fff"/><path d="${path}" fill="#000"/>`, {
    viewBox: `0 0 ${size} ${size}`,
    role: "img",
    "aria-label": label,
    "shape-rendering": "crispEdges",
    class: "qr",
  });
}
