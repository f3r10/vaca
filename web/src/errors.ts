import { RequestError } from "./api";
import { app, serverErrors, walletErrors } from "./strings";

/** Plain-language text for anything a request or a wallet step can throw. */
export function friendly(err: unknown): string {
  if (err instanceof RequestError) return mapServer(err.message) ?? err.message;
  if (err instanceof TypeError) return app.serverDown;
  if (err instanceof Error && err.message) return mapServer(err.message) ?? mapWallet(err.message) ?? err.message;
  return "Something went wrong. Try again.";
}

function mapWallet(message: string): string | null {
  for (const [pattern, text] of walletErrors) {
    const m = message.match(pattern);
    if (m) return text(m);
  }
  return null;
}

function mapServer(message: string): string | null {
  const base = message.replace(/ \([^)]*\)$/, "");
  const hit = serverErrors.find(([raw]) => raw === base);
  return hit ? hit[1] : null;
}

export function isNetworkError(err: unknown): boolean {
  return err instanceof TypeError;
}
