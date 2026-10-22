/**
 * AniList account linking — OAuth implicit grant, entirely client side.
 *
 * No client secret and no backend: the app opens
 *   https://anilist.co/api/v2/oauth/authorize?client_id=<id>&response_type=token
 * in the system browser. AniList redirects to the redirect URL registered on the
 * AniList client (`tatakai://integration/anilist/redirect`) with the token in the
 * URL FRAGMENT:
 *   tatakai://integration/anilist/redirect#access_token=…&token_type=Bearer&expires_in=…
 * The deep link reaches the SPA (desktop: deep-link.cjs -> DesktopDeepLinkBridge,
 * Android: MobileDeepLinkBridge) and `AniListRedirectPage` calls
 * `completeAniListLogin` with `location.hash`.
 *
 * There is no refresh token (the token lasts about a year). On a 401 the stored
 * credentials are cleared and the user must re-link.
 *
 * The token is stored in the owner-only `user_integration_tokens` table (never in
 * the publicly readable `profiles`); only the AniList user id goes on `profiles`.
 * It is only ever sent to graphql.anilist.co.
 */

import { anilistGql } from '@/lib/anilist/client';

export const ANILIST_AUTHORIZE_URL = 'https://anilist.co/api/v2/oauth/authorize';
/** The redirect URL to register on the AniList client. */
export const ANILIST_DEFAULT_REDIRECT_URI = 'tatakai://integration/anilist/redirect';

const STARTED_AT_KEY = 'anilist_oauth_started_at';
/** A callback is only accepted this long after the login was started here. */
const PENDING_TTL_MS = 5 * 60 * 1000;

const env = (name: string): string => {
  try {
    return String((import.meta as unknown as { env?: Record<string, unknown> }).env?.[name] ?? '').trim();
  } catch {
    return '';
  }
};

export function getAniListClientId(): string {
  return env('VITE_ANILIST_CLIENT_ID');
}

/** False when this build has no AniList client id: the UI shows "not configured". */
export function isAniListConfigured(): boolean {
  return getAniListClientId() !== '';
}

/**
 * The redirect URL AniList is registered with. Not sent in the implicit-grant
 * request (AniList uses the registered one); exposed for the settings help text.
 */
export function getAniListRedirectUri(): string {
  const first = (value: string) => value.split(/[\s,]+/)[0] ?? '';
  return first(env('VITE_ANILIST_DESKTOP_REDIRECT_URI')) || first(env('VITE_ANILIST_REDIRECT_URI')) || ANILIST_DEFAULT_REDIRECT_URI;
}

export function buildAniListAuthUrl(clientId: string = getAniListClientId()): string {
  if (!clientId) throw new AniListNotConfiguredError();
  const params = new URLSearchParams({ client_id: clientId, response_type: 'token' });
  return `${ANILIST_AUTHORIZE_URL}?${params.toString()}`;
}

export class AniListNotConfiguredError extends Error {
  constructor() {
    super('AniList is not configured in this build (missing VITE_ANILIST_CLIENT_ID).');
    this.name = 'AniListNotConfiguredError';
  }
}

/** AniList is down / blocking requests (403, 429, 5xx, network). Do not retry in a loop. */
export class AniListUnavailableError extends Error {
  constructor(readonly status?: number) {
    super('AniList is unavailable right now. Try again later.');
    this.name = 'AniListUnavailableError';
  }
}

/** The stored token was rejected (401): credentials were cleared; the user must re-link. */
export class AniListAuthExpiredError extends Error {
  constructor() {
    super('Your AniList login expired. Reconnect AniList in Settings → Integrations.');
    this.name = 'AniListAuthExpiredError';
  }
}

// ── Persistence (injectable for tests) ───────────────────────────────────────

export interface AniListTokenPatch {
  anilist_access_token: string | null;
  anilist_token_expires_at: string | null;
  anilist_user_id: string | null;
}

export interface AniListAuthDeps {
  /** Update the signed-in user's own profile row. */
  saveProfile(patch: AniListTokenPatch): Promise<void>;
  now(): number;
}

const defaultDeps: AniListAuthDeps = {
  async saveProfile(patch) {
    // Token + expiry -> user_integration_tokens (owner only); user id -> profiles.
    const { saveIntegrationPatch } = await import('@/lib/integrationTokens');
    await saveIntegrationPatch(patch);
  },
  now: () => Date.now(),
};
let deps: AniListAuthDeps = defaultDeps;

/** Test hook. Pass null to restore the Supabase-backed defaults. */
export function setAniListAuthDeps(next: Partial<AniListAuthDeps> | null): void {
  deps = next ? { ...defaultDeps, ...next } : defaultDeps;
}

// ── Login flow ───────────────────────────────────────────────────────────────

/** Marks a login as started in this app so a stray callback can't link a foreign account. */
export function markAniListLoginStarted(): void {
  try { localStorage.setItem(STARTED_AT_KEY, String(deps.now())); } catch { /* storage unavailable */ }
}

export function isAniListLoginPending(): boolean {
  try {
    const at = Number(localStorage.getItem(STARTED_AT_KEY) || 0);
    return at > 0 && deps.now() - at < PENDING_TTL_MS;
  } catch {
    return false;
  }
}

export function clearAniListPendingMarker(): void {
  try { localStorage.removeItem(STARTED_AT_KEY); } catch { /* ignore */ }
}

/** Build the authorize URL and mark the login in flight. */
export function startAniListLogin(): string {
  const url = buildAniListAuthUrl();
  markAniListLoginStarted();
  return url;
}

export type AniListCallback =
  | { kind: 'token'; accessToken: string; tokenType: string; expiresIn: number | null }
  | { kind: 'error'; error: string; description: string | null };

/**
 * Parse the redirect: a hash string (`#access_token=…`), a query string, or a
 * whole URL. The fragment wins; AniList errors come back the same way
 * (`#error=access_denied&error_description=…`). Null when there is nothing.
 */
export function parseAniListCallback(input: string | null | undefined): AniListCallback | null {
  let raw = String(input ?? '').trim();
  if (!raw) return null;
  const hashAt = raw.indexOf('#');
  if (hashAt >= 0) raw = raw.slice(hashAt + 1);
  else {
    const queryAt = raw.indexOf('?');
    if (queryAt >= 0) raw = raw.slice(queryAt + 1);
  }
  const params = new URLSearchParams(raw.replace(/^[#?]/, ''));
  const error = params.get('error');
  if (error) return { kind: 'error', error, description: params.get('error_description') || params.get('hint') };
  const accessToken = params.get('access_token');
  if (!accessToken) return null;
  const expiresIn = Number(params.get('expires_in'));
  return {
    kind: 'token',
    accessToken,
    tokenType: params.get('token_type') || 'Bearer',
    expiresIn: Number.isFinite(expiresIn) && expiresIn > 0 ? expiresIn : null,
  };
}

interface ViewerResult { Viewer?: { id: number; name: string } | null }

/** Throw the right typed error for an AniList failure (never retries). */
export function toAniListFailure(err: unknown): Error {
  if (err instanceof AniListAuthExpiredError || err instanceof AniListUnavailableError) return err;
  const status = (err as { status?: number } | null)?.status;
  if (status === 401) return new AniListAuthExpiredError();
  // No status = transport failure (offline, DNS, timeout).
  if (status === undefined || status === 403 || status === 429 || status >= 500) return new AniListUnavailableError(status);
  return err instanceof Error ? err : new Error(String(err));
}

export interface AniListLoginResult { userId: number; username: string; expiresAt: string | null }

/** A verified token waiting for the user's "Link AniList account <name>?" confirmation. */
export interface PendingAniListLogin {
  accessToken: string;
  username: string;
  userId: number;
  expiresAt: string | null;
}

/**
 * Step 1 of the implicit grant. Validates the callback against the in-flight
 * marker, which is consumed on the FIRST callback whatever its outcome (a replayed
 * or forged callback therefore finds no marker), then looks up the viewer with
 * the token. Nothing is stored yet: the UI shows the account name and the user
 * confirms with `confirmAniListLogin`.
 */
export async function prepareAniListLogin(callbackInput: string): Promise<PendingAniListLogin> {
  const pending = isAniListLoginPending();
  clearAniListPendingMarker();
  const cb = parseAniListCallback(callbackInput);
  if (!cb) throw new Error('AniList did not return a login token.');
  if (cb.kind === 'error') throw new Error(`AniList: ${cb.description || cb.error}`);
  if (!pending) {
    throw new Error('This AniList login was not started from this app. Start it again from Settings → Integrations.');
  }
  let viewer: ViewerResult['Viewer'];
  try {
    const data = await anilistGql<ViewerResult>('query { Viewer { id name } }', undefined, { token: cb.accessToken, retries: 1 });
    viewer = data?.Viewer;
  } catch (err) {
    throw toAniListFailure(err);
  }
  if (!viewer?.id) throw new Error('AniList did not return your profile.');
  const expiresAt = cb.expiresIn ? new Date(deps.now() + cb.expiresIn * 1000).toISOString() : null;
  return { accessToken: cb.accessToken, username: viewer.name, userId: viewer.id, expiresAt };
}

/** Step 2: the user confirmed; persist token + expiry (private table) and the AniList user id (profile). */
export async function confirmAniListLogin(login: PendingAniListLogin): Promise<AniListLoginResult> {
  await deps.saveProfile({
    anilist_access_token: login.accessToken,
    anilist_token_expires_at: login.expiresAt,
    anilist_user_id: String(login.userId),
  });
  return { userId: login.userId, username: login.username, expiresAt: login.expiresAt };
}

/** Prepare + confirm in one go (for callers that already asked the user). */
export async function completeAniListLogin(callbackInput: string): Promise<AniListLoginResult> {
  return confirmAniListLogin(await prepareAniListLogin(callbackInput));
}

export function isAniListTokenExpired(expiresAt: string | null | undefined, now = Date.now()): boolean {
  if (!expiresAt) return false;
  const at = Date.parse(expiresAt);
  return Number.isFinite(at) && at <= now;
}

/** Clear stored credentials (disconnect, or after a 401). */
export async function clearAniListCredentials(): Promise<void> {
  await deps.saveProfile({
    anilist_access_token: null,
    anilist_token_expires_at: null,
    anilist_user_id: null,
  });
}

/**
 * Run an AniList call with the user's token. A 401 clears the stored login and
 * raises `AniListAuthExpiredError`; 403/429/5xx/network raise
 * `AniListUnavailableError`. Nothing here retries beyond anilistGql's own paced
 * retry on 429/5xx.
 */
export async function withAniListToken<T>(token: string, run: (token: string) => Promise<T>): Promise<T> {
  try {
    return await run(token);
  } catch (err) {
    const failure = toAniListFailure(err);
    if (failure instanceof AniListAuthExpiredError) {
      await clearAniListCredentials().catch(() => undefined);
    }
    throw failure;
  }
}

/** The user's stored AniList access token (own row only), or null when not linked/expired. */
export async function getAniListAccessToken(): Promise<string | null> {
  const { loadIntegrationTokens } = await import('@/lib/integrationTokens');
  const row = await loadIntegrationTokens();
  if (!row?.anilist_access_token || isAniListTokenExpired(row.anilist_token_expires_at, deps.now())) return null;
  return row.anilist_access_token;
}
