// Mints one unlocked token from the pot mint. Prints the invoice and waits.
// Does not pay it, and does not print the token or the proofs.

import { writeFileSync } from "node:fs";
import { homedir } from "node:os";
import { Wallet, getEncodedToken, sumProofs } from "../web/node_modules/@cashu/cashu-ts/lib/cashu-ts.es.js";

try {
  if (process.argv.length > 4) {
    throw new Error("usage: node scripts/mint-test-token.mjs [amount] [path]");
  }
  const amount = parseAmount(process.argv[2]);
  const output = process.argv[3] ?? `${homedir()}/Downloads/vaca-test-token.cashu`;
  const mintUrl = (process.env.VACA_MINT_URL ?? "http://127.0.0.1:8085").replace(/\/$/, "");
  await mintToken(amount, output, mintUrl);
} catch (error) {
  const message = error instanceof Error ? error.message : "the token was not minted";
  process.stderr.write(`${message}\n`);
  process.exitCode = 1;
}

function parseAmount(raw) {
  if (raw === undefined) {
    return 100;
  }
  if (!/^[1-9][0-9]*$/.test(raw)) {
    throw new Error("amount must be a whole number of sats");
  }
  const value = Number(raw);
  if (value > 100_000) {
    throw new Error("amount must be from 1 to 100000 sats");
  }
  return value;
}

async function mintToken(amount, output, mintUrl) {
  const created = await postJson(`${mintUrl}/v1/mint/quote/bolt11`, {
    amount,
    unit: "sat",
  });
  const quoteId = created.quote;
  const invoice = created.request;
  if (typeof quoteId !== "string" || quoteId === "" || typeof invoice !== "string" || invoice === "") {
    throw new Error("the mint did not return an invoice");
  }
  process.stderr.write(
    `Pay this invoice from the lab payer, then leave this script running.\n${invoice}\n`,
  );
  await waitUntilPaid(mintUrl, quoteId, amount);
  const wallet = new Wallet(mintUrl);
  try {
    await wallet.loadMint();
  } catch {
    throw new Error("The mint could not be reached.");
  }
  let proofs;
  try {
    proofs = await wallet.mintProofsBolt11(amount, quoteId);
  } catch {
    throw new Error("the mint did not return proofs");
  }
  if (sumProofs(proofs).toNumber() !== amount) {
    throw new Error("the mint returned a different amount");
  }
  const token = getEncodedToken({ mint: mintUrl, proofs, unit: "sat" });
  try {
    writeFileSync(output, `${token}\n`, { mode: 0o600, flag: "wx" });
  } catch (error) {
    if (error && error.code === "EEXIST") {
      throw new Error("that token file already exists");
    }
    throw new Error("the token file could not be written");
  }
  process.stdout.write(`sats ${amount}\n${output}\n`);
}

async function waitUntilPaid(mintUrl, quoteId, amount) {
  const deadline = Date.now() + 10 * 60 * 1000;
  while (Date.now() < deadline) {
    const body = await getJson(`${mintUrl}/v1/mint/quote/bolt11/${quoteId}`);
    if (body.state === "ISSUED") {
      throw new Error("this quote was already minted");
    }
    if (body.state === "PAID") {
      if (body.amount_paid !== amount || body.amount_issued !== 0) {
        throw new Error("the paid quote does not match the amount");
      }
      return;
    }
    await new Promise((resolve) => setTimeout(resolve, 2000));
  }
  throw new Error("the quote was not paid within 10 minutes");
}

async function postJson(url, payload) {
  let response;
  try {
    response = await fetch(url, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify(payload),
    });
  } catch {
    throw new Error("The mint could not be reached.");
  }
  return readJson(response);
}

async function getJson(url) {
  let response;
  try {
    response = await fetch(url);
  } catch {
    throw new Error("The mint could not be reached.");
  }
  return readJson(response);
}

async function readJson(response) {
  if (!response.ok) {
    throw new Error("The mint could not be reached.");
  }
  let body;
  try {
    body = await response.json();
  } catch {
    throw new Error("The mint could not be reached.");
  }
  if (body === null || typeof body !== "object") {
    throw new Error("The mint could not be reached.");
  }
  return body;
}
