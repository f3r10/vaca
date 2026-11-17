export interface Health {
  ok: boolean;
  experimental: boolean;
  mint_url_set: boolean;
  max_pot_sats: number;
  grace_seconds: number;
  fee_reserve_recipient: string;
  sigflag_v1: string;
  sigflag_v2: string;
  server_key_path_set: boolean;
  server_key_file_present: boolean;
  server_pubkey_set: boolean;
  counts_contributions: boolean;
  cdk_rev: string;
  cdk_linked: boolean;
  cashu_ts: string;
  cashu_ts_commit: string;
}

export interface Pot {
  id: string;
  title: string;
  kind: string;
  target_sats: number;
  deadline_unix: number;
  destination: string;
  mint_url: string;
  organizer_pubkey: string | null;
  status: string;
  created_at_unix: number;
  contribution_count: number;
  locked_sats: number;
  locktime_unix: number;
  co_sign_until_unix: number;
  grace_seconds: number;
  fee_reserve_recipient: string;
  sigflag_v1: string;
  sigflag_v2: string;
  fee_reserve_sats: number | null;
  sats_still_needed: number;
  payout_allowed: boolean;
  fee_quoted: boolean;
  quote_amount_sats: number | null;
  change_sats: number;
}

export interface CreatePotInput {
  title: string;
  kind: string;
  target_sats: number;
  deadline_unix: number;
  destination: string;
  organizer_pubkey: string;
}

export class RequestError extends Error {
  readonly status: number;

  constructor(status: number, message: string) {
    super(message);
    this.status = status;
  }
}

function apiBase(): string {
  return import.meta.env.VITE_API_BASE ?? "";
}

async function request<T>(path: string, init?: RequestInit): Promise<T> {
  const response = await fetch(`${apiBase()}${path}`, init);
  const text = await response.text();
  let body: unknown = null;
  if (text) {
    try {
      body = JSON.parse(text) as unknown;
    } catch {
      body = null;
    }
  }
  if (!response.ok) {
    const message = messageFrom(body) ?? `${response.status} ${response.statusText}`;
    throw new RequestError(response.status, message);
  }
  return body as T;
}

function messageFrom(body: unknown): string | null {
  if (!body || typeof body !== "object") {
    return null;
  }
  const record = body as Record<string, unknown>;
  const message = typeof record.message === "string" ? record.message : "";
  const route = typeof record.route === "string" ? record.route : "";
  if (message && route) {
    return `${message} (${route})`;
  }
  return message || null;
}

export function getHealth(): Promise<Health> {
  return request<Health>("/api/health");
}

export function listPots(): Promise<Pot[]> {
  return request<{ pots: Pot[] }>("/api/pots").then((body) => body.pots);
}

export function getPot(id: string): Promise<Pot> {
  return request<Pot>(`/api/pots/${encodeURIComponent(id)}`);
}

export function createPot(input: CreatePotInput): Promise<Pot> {
  return request<Pot>("/api/pots", {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(input),
  });
}

export function postUnimplemented(id: string, route: "contributions" | "payout" | "refunds"): Promise<never> {
  return request<never>(`/api/pots/${encodeURIComponent(id)}/${route}`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: "{}",
  });
}
