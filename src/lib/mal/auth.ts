/**
 * MyAnimeList account linking — OAuth2 authorization code + PKCE, client side.
 *
 * Public client: only `client_id` (VITE_MAL_CLIENT_ID), no secret, no backend.
 * MAL supports `code_challenge_method=plain`, so the challenge equals the
 * verifier. The system browser returns to `tatakai://integration/mal/redirect`
 * (register that URL, nothing else, in the MAL app settings); the token exchange
 * and refresh go to https://myanimelist.net/v1/oauth2/token through `publicFetch`
 * (native HTTP in the apps — MAL sends no CORS headers, so a plain browser cannot
 * do this).
 *
 * Tokens live in the owner-only `user_integration_tokens` table (never in the
 * publicly readable `profiles`; only `mal_user_id` goes there). The access token is only ever sent to api.myanimelist.net and
 * the refresh token only to myanimelist.net.
 */

import type { ExtensionResponse } from '@/core/extensions/remote/transport';
import { publicFetch, publicJson, PublicApiError } from '@/lib/net/publicApi';

export const MAL_AUTHORIZE_URL = 'https://myanimelist.net/v1/oauth2/authorize';
export const MAL_TOKEN_URL = 'https://myanimelist.net/v1/oauth2/token';
export const MAL_API_URL = 'https://api.myanimelist.net/v2';
/** The redirect URL to register in the MAL app settings. */
export const MAL_DEFAULT_REDIRECT_URI = 'tatakai://integration/mal/redirect';

const K_VERIFIER = 'mal_code_verifier';
const K_REDIRECT = 'mal_redirect_uri';
const K_STATE = 'mal_oauth_state';
const K_STARTED = 'mal_oauth_started_at';
/** A login that never reached the callback within this window no longer counts as pending. */
export const MAL_PENDING_TTL_MS = 10 * 60 * 1000;
/** Refresh this long before the access token expires. */
const REFRESH_SKEW_MS = 5 * 60 * 1000;

const env = (name: string): string => {
  try {
    return String((import.meta as unknown as { env?: Record<string, unknown> }).env?.[name] ?? '').trim();
  } catch {
    return '';
  }
};

export function getMalClientId(): string {
  return env('VITE_MAL_CLIENT_ID');
}

/** False when this build has no MAL client id: the UI shows "not configured". */
export function isMalConfigured(): boolean {
  return getMalClientId() !== '';
}

export function getMalRedirectUri(): string {
  const first = (value: string) => value.split(/[\s,]+/)[0] ?? '';
  return first(env('VITE_MAL_DESKTOP_REDIRECT_URI')) || first(env('VITE_MAL_REDIRECT_URI')) || MAL_DEFAULT_REDIRECT_URI;
}

export class MalNotConfiguredError extends Error {
  constructor() {
    super('MyAnimeList is not configured in this build (missing VITE_MAL_CLIENT_ID).');
    this.name = 'MalNotConfiguredError';
  }
}

/** The MAL login can no longer be refreshed/used: credentials were cleared; re-link. */
export class MalAuthExpiredError extends Error {
  constructor() {
    super('Your MyAnimeList login expired. Reconnect MyAnimeList in Settings → Integrations.');
    this.name = 'MalAuthExpiredError';
  }
}

export class MalAuthError extends Error {
  constructor(message: string, readonly status: number, readonly code?: string) {
    super(message);
    this.name = 'MalAuthError';
  }
}

// ── Persistence (injectable for tests) ───────────────────────────────────────

export interface MalStoredTokens {
  accessToken: string | null;
  refreshToken: string | null;
  /** ISO timestamp, or null when unknown. */
  expiresAt: string | null;
}

export interface MalProfilePatch {
  mal_access_token?: string | null;
  mal_refresh_token?: string | null;
  mal_token_expires_at?: string | null;
  mal_user_id?: string | null;
}

export interface MalAuthDeps {
  loadTokens(): Promise<MalStoredTokens | null>;
  saveProfile(patch: MalProfilePatch): Promise<void>;
  now(): number;
}

const defaultDeps: MalAuthDeps = {
  async loadTokens() {
    const { loadIntegrationTokens } = await import('@/lib/integrationTokens');
    const row = await loadIntegrationTokens();
    if (!row) return null;
    return { accessToken: row.mal_access_token, refreshToken: row.mal_refresh_token, expiresAt: row.mal_token_expires_at };
  },
  async saveProfile(patch) {
    // Tokens -> user_integration_tokens (owner only); mal_user_id -> profiles.
    const { saveIntegrationPatch } = await import('@/lib/integrationTokens');
    await saveIntegrationPatch(patch);
  },
  now: () => Date.now(),
};
let deps: MalAuthDeps = defaultDeps;

/** Test hook. Pass null to restore the Supabase-backed defaults. */
export function setMalAuthDeps(next: Partial<MalAuthDeps> | null): void {
  deps = next ? { ...defaultDeps, ...next } : defaultDeps;
  refreshInFlight = null;
}

// ── PKCE login ───────────────────────────────────────────────────────────────

const PKCE_CHARS = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-._~';

/** RFC 7636 verifier: 43–128 chars from the unreserved set (rejection sampled, unbiased). */
export function generateRandomString(length: number): string {
  let out = '';
  const limit = 256 - (256 % PKCE_CHARS.length);
  while (out.length < length) {
    for (const byte of crypto.getRandomValues(new Uint8Array(length))) {
      if (byte < limit) out += PKCE_CHARS[byte % PKCE_CHARS.length];
      if (out.length === length) break;
    }
  }
  return out;
}

export function buildMalAuthUrl(input: { clientId: string; redirectUri: string; codeVerifier: string; state: string }): string {
  const params = new URLSearchParams({
    response_type: 'code',
    client_id: input.clientId,
    code_challenge: input.codeVerifier, // method "plain": challenge === verifier
    code_challenge_method: 'plain',
    state: input.state,
    redirect_uri: input.redirectUri,
  });
  return `${MAL_AUTHORIZE_URL}?${params.toString()}`;
}

/** Start a login: persist verifier/state/redirect, return the authorize URL. */
export function startMalLogin(redirectUriOverride?: string): string {
  const clientId = getMalClientId();
  if (!clientId) throw new MalNotConfiguredError();
  const redirectUri = (redirectUriOverride || getMalRedirectUri()).trim();
  const codeVerifier = generateRandomString(128);
  const state = generateRandomString(32);
  localStorage.setItem(K_VERIFIER, codeVerifier);
  localStorage.setItem(K_REDIRECT, redirectUri);
  localStorage.setItem(K_STATE, state);
  localStorage.setItem(K_STARTED, String(deps.now()));
  return buildMalAuthUrl({ clientId, redirectUri, codeVerifier, state });
}

/** True when a login was started recently but its callback has not run. */
export function isMalLoginPending(): boolean {
  try {
    const at = Number(localStorage.getItem(K_STARTED) || 0);
    return at > 0 && deps.now() - at < MAL_PENDING_TTL_MS;
  } catch {
    return false;
  }
}

export function readMalRedirectUri(): string | null {
  try { return localStorage.getItem(K_REDIRECT); } catch { return null; }
}

export function readMalExpectedState(): string | null {
  try { return localStorage.getItem(K_STATE); } catch { return null; }
}

export function clearMalPendingMarker(): void {
  try { localStorage.removeItem(K_STARTED); } catch { /* ignore */ }
}

export function clearMalLoginStorage(): void {
  try {
    for (const key of [K_VERIFIER, K_REDIRECT, K_STATE, K_STARTED]) localStorage.removeItem(key);
  } catch { /* ignore */ }
}

// ── Token endpoint ───────────────────────────────────────────────────────────

export interface MalTokenResponse {
  access_token: string;
  refresh_token?: string;
  expires_in?: number;
  token_type?: string;
}

async function postToken(body: URLSearchParams): Promise<MalTokenResponse> {
  const res = await publicFetch(MAL_TOKEN_URL, {
    method: 'POST',
    headers: { 'Content-Type': 'application/x-www-form-urlencoded', Accept: 'application/json' },
    body: body.toString(),
    retries: 1,
  });
  const text = await res.text();
  let json: Record<string, unknown> = {};
  try { json = JSON.parse(text) as Record<string, unknown>; } catch { /* non-JSON error page */ }
  if (!res.ok || typeof json.access_token !== 'string') {
    const code = typeof json.error === 'string' ? json.error : undefined;
    const detail = [json.error, json.message, json.hint].filter((v) => typeof v === 'string').join(': ');
    throw new MalAuthError(`MyAnimeList token request failed (${res.status})${detail ? `: ${detail}` : ''}`, res.status, code);
  }
  return json as unknown as MalTokenResponse;
}

/** Authorization-code exchange (PKCE). */
export function exchangeMalAuthorizationCode(input: { code: string; codeVerifier: string; redirectUri: string; clientId?: string }): Promise<MalTokenResponse> {
  return postToken(new URLSearchParams({
    client_id: input.clientId || getMalClientId(),
    grant_type: 'authorization_code',
    code: input.code,
    code_verifier: input.codeVerifier,
    redirect_uri: input.redirectUri,
  }));
}

export function refreshMalAccessToken(refreshToken: string, clientId = getMalClientId()): Promise<MalTokenResponse> {
  return postToken(new URLSearchParams({ client_id: clientId, grant_type: 'refresh_token', refresh_token: refreshToken }));
}

function expiryOf(tokens: MalTokenResponse): string | null {
  return tokens.expires_in ? new Date(deps.now() + tokens.expires_in * 1000).toISOString() : null;
}

export interface MalLoginResult { userId: string; username: string | null }

/**
 * Finish the login from the callback parameters: check `state` against the one
 * stored when the login started, exchange the code, look up the MAL user and
 * persist everything.
 */
export async function completeMalLogin(input: { code: string; state: string | null }): Promise<MalLoginResult> {
  const verifier = localStorage.getItem(K_VERIFIER);
  const expected = readMalExpectedState();
  if (!verifier || !expected) throw new Error('This MyAnimeList login was not started from this app. Start it again from Settings → Integrations.');
  if (!input.state || input.state !== expected) throw new Error('This MyAnimeList login does not match the one this app started. Start it again.');
  const redirectUri = readMalRedirectUri() || getMalRedirectUri();

  const tokens = await exchangeMalAuthorizationCode({ code: input.code, codeVerifier: verifier, redirectUri });
  const me = await publicJson<{ id?: number; name?: string }>(`${MAL_API_URL}/users/@me`, {
    headers: { Authorization: `Bearer ${tokens.access_token}` },
  });
  await deps.saveProfile({
    mal_access_token: tokens.access_token,
    mal_refresh_token: tokens.refresh_token ?? null,
    mal_token_expires_at: expiryOf(tokens),
    mal_user_id: me.id != null ? String(me.id) : null,
  });
  clearMalLoginStorage();
  return { userId: String(me.id ?? ''), username: me.name ?? null };
}

// ── Using the stored login ───────────────────────────────────────────────────

export async function clearMalCredentials(): Promise<void> {
  await deps.saveProfile({ mal_access_token: null, mal_refresh_token: null, mal_token_expires_at: null, mal_user_id: null });
}

let refreshInFlight: Promise<string> | null = null;

async function refreshStored(stored: MalStoredTokens): Promise<string> {
  if (!stored.refreshToken) {
    await clearMalCredentials().catch(() => undefined);
    throw new MalAuthExpiredError();
  }
  try {
    const next = await refreshMalAccessToken(stored.refreshToken);
    await deps.saveProfile({
      mal_access_token: next.access_token,
      // MAL rotates the refresh token; keep the old one if none came back.
      mal_refresh_token: next.refresh_token ?? stored.refreshToken,
      mal_token_expires_at: expiryOf(next),
    });
    return next.access_token;
  } catch (err) {
    // 400/401 from the token endpoint = refresh token revoked/expired.
    if (err instanceof MalAuthError && (err.status === 400 || err.status === 401)) {
      await clearMalCredentials().catch(() => undefined);
      throw new MalAuthExpiredError();
    }
    throw err;
  }
}

/** Refresh (de-duplicated across concurrent callers). */
function refreshOnce(stored: MalStoredTokens): Promise<string> {
  refreshInFlight ??= refreshStored(stored).finally(() => { refreshInFlight = null; });
  return refreshInFlight;
}

/**
 * The user's MAL access token, refreshed first when it expires within five
 * minutes. Throws `MalAuthExpiredError` when MAL is not linked or the login is
 * no longer refreshable.
 */
export async function getValidMalAccessToken(): Promise<string> {
  const stored = await deps.loadTokens();
  if (!stored?.accessToken && !stored?.refreshToken) throw new MalAuthExpiredError();
  const expiresAt = stored.expiresAt ? Date.parse(stored.expiresAt) : NaN;
  const stale = Number.isFinite(expiresAt) && expiresAt - deps.now() < REFRESH_SKEW_MS;
  if (stored.accessToken && !stale) return stored.accessToken;
  return refreshOnce(stored);
}

/**
 * Authenticated MAL API call. A 401 triggers one refresh + retry; a second 401
 * clears the stored login and raises `MalAuthExpiredError`.
 */
export async function malFetch(path: string, init: { method?: 'GET' | 'PATCH' | 'DELETE'; form?: Record<string, string | number | undefined>; signal?: AbortSignal } = {}): Promise<ExtensionResponse> {
  const url = path.startsWith('http') ? path : `${MAL_API_URL}${path}`;
  const body = init.form
    ? new URLSearchParams(Object.entries(init.form).filter(([, v]) => v !== undefined).map(([k, v]) => [k, String(v)])).toString()
    : undefined;
  const call = (token: string) => publicFetch(url, {
    method: init.method ?? 'GET',
    headers: { Authorization: `Bearer ${token}`, Accept: 'application/json', ...(body !== undefined ? { 'Content-Type': 'application/x-www-form-urlencoded' } : {}) },
    body,
    signal: init.signal,
  });
  let res = await call(await getValidMalAccessToken());
  if (res.status === 401) {
    const stored = await deps.loadTokens();
    if (!stored) throw new MalAuthExpiredError();
    res = await call(await refreshOnce({ ...stored, accessToken: null }));
    if (res.status === 401) {
      await clearMalCredentials().catch(() => undefined);
      throw new MalAuthExpiredError();
    }
  }
  return res;
}

/** `malFetch` + JSON parse; non-2xx raises PublicApiError. */
export async function malJson<T>(path: string, signal?: AbortSignal): Promise<T> {
  const res = await malFetch(path, { signal });
  const text = await res.text();
  if (!res.ok) throw new PublicApiError(`api.myanimelist.net ${res.status}`, res.status, path, text.slice(0, 300));
  return JSON.parse(text) as T;
}
