// Offline checks for the QR matrix. Run with:  node src/ui/qr.check.ts
// A scan test needs a camera (or a decoder library), so this checks structure only:
// the shape of the grid and the three corner markers every QR code has.
import { qrMatrix } from "./qr.ts";

function check(name: string, ok: boolean): void {
  if (!ok) throw new Error(`failed: ${name}`);
}

// A regtest-sized invoice: about 400 letters and digits, like the real ones.
const invoice = "lnbcrt100n1p40mtfsdqqnp4qtc9yx3sev7xjdh8fqwkfvqrg3z90clqcrm69ggyafrd030j06f0qpp5k70v7fxfnhj60s3jr3cgeqp2hk5ampva6uyek7ygjlx5qrffpmuqsp5qhncpngnw872zvuh4q8s8dyyg3cdrn8ets7tty9zdtfjy324v3js9qyysgqcqzp2xqzjcrzjqd97twwssx3zzntwsnyz5d0anwkvnph4kg0etwgw0lhddn7hmy69jqqqqyqqykcqqcqqqqlgqqqqqqqqfqze0tp2z924uf3jpqwzn9j4965jgc8vh9h98rk5q0pvue7lmz5lvjs3xqqyhl8ptasxrz7m0jmk8zl3wyf2d62uceafhuse97sy39u0gqkhg5k5";
const rows = qrMatrix(invoice);
check("an invoice can be encoded", rows !== null);
const grid = rows as boolean[][];
const n = grid.length;
check("the grid is square", grid.every((row) => row.length === n));
check("the size is a valid QR size (21 + 4 per version)", n >= 21 && n <= 177 && (n - 21) % 4 === 0);

// Every QR code has a 7x7 finder pattern in three corners: a solid ring of dark squares.
function finderOk(r0: number, c0: number): boolean {
  for (let i = 0; i < 7; i += 1) {
    for (let j = 0; j < 7; j += 1) {
      const ring = i === 0 || i === 6 || j === 0 || j === 6;
      const core = i >= 2 && i <= 4 && j >= 2 && j <= 4;
      if (grid[r0 + i][c0 + j] !== (ring || core)) return false;
    }
  }
  return true;
}
check("top-left finder pattern", finderOk(0, 0));
check("top-right finder pattern", finderOk(0, n - 7));
check("bottom-left finder pattern", finderOk(n - 7, 0));

// Text far too long for any QR code is refused rather than drawn wrong.
check("an impossible length returns null", qrMatrix("A".repeat(8000)) === null);

// Lowercase and uppercase give the same picture, because the code uppercases first.
check("case does not matter", JSON.stringify(qrMatrix("lnbc1abc")) === JSON.stringify(qrMatrix("LNBC1ABC")));

console.log("qr checks passed");
