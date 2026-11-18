import { t } from "./strings";

const grouped = new Intl.NumberFormat("en", { useGrouping: true, maximumFractionDigits: 0 });

export function formatNumber(n: number): string {
  return grouped.format(n);
}

/** Grouped digits, unit always attached. */
export function formatSats(n: number): string {
  return n === 1 ? `1 ${t.sat}` : `${formatNumber(n)} ${t.sats}`;
}

export type DestinationKind = "bolt11" | "bolt12" | "unknown";

/** Client-side guess from the prefix. The server stores the text and does not check it. */
export function destinationKind(destination: string): DestinationKind {
  const d = destination.trim().toLowerCase();
  if (!/^[a-z0-9]+$/.test(d) || d.length < 20) return "unknown";
  if (/^ln(bc|bcrt|tbs|tb)\d/.test(d) || /^ln(bc|bcrt|tbs|tb)1/.test(d)) return "bolt11";
  if (d.startsWith("lno1")) return "bolt12";
  return "unknown";
}

export type MintKind = "local" | "test" | "other";

/** A guess from the address, used only to label test money honestly. */
export function mintKind(url: string): MintKind {
  const host = hostOf(url).toLowerCase();
  if (/^(localhost|127\.|\[::1\])/.test(host)) return "local";
  if (/(^|\.)testnut\.|mutinynet|signet|regtest|testnet/.test(host)) return "test";
  return "other";
}

export function hostOf(url: string): string {
  try {
    return new URL(url).host;
  } catch {
    return url;
  }
}

const dateFormat = new Intl.DateTimeFormat("en", { weekday: "short", month: "short", day: "numeric", hour: "numeric", minute: "2-digit" });
const timeFormat = new Intl.DateTimeFormat("en", { hour: "numeric", minute: "2-digit" });

export function formatDate(unix: number): string {
  return dateFormat.format(new Date(unix * 1000));
}

export function formatTime(unix: number): string {
  return timeFormat.format(new Date(unix * 1000));
}

function plural(n: number, one: string): string {
  return `${n} ${one}${n === 1 ? "" : "s"}`;
}

/** Two largest units: "6 days 4 hours", "2 hours 14 minutes", "9 minutes". */
export function duration(seconds: number): string {
  const s = Math.max(0, Math.floor(seconds));
  const d = Math.floor(s / 86400);
  const h = Math.floor((s % 86400) / 3600);
  const m = Math.floor((s % 3600) / 60);
  if (d > 0) return h > 0 ? `${plural(d, "day")} ${plural(h, "hour")}` : plural(d, "day");
  if (h > 0) return m > 0 ? `${plural(h, "hour")} ${plural(m, "minute")}` : plural(h, "hour");
  if (m > 0) return plural(m, "minute");
  return "under a minute";
}
