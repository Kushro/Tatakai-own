import { afterEach, beforeEach, expect, test } from 'bun:test';
import { resetHostLimiter } from '../src/lib/net/hostLimiter';
import {
  MAL_DEFAULT_REDIRECT_URI,
  MalAuthExpiredError,
  MalNotConfiguredError,
  buildMalAuthUrl,
  completeMalLogin,
  generateRandomString,
  getMalRedirectUri,
  getValidMalAccessToken,
  isMalConfigured,
  isMalLoginPending,
  malFetch,
  setMalAuthDeps,
  startMalLogin,
  type MalProfilePatch,
  type MalStoredTokens,
} from '../src/lib/mal/auth';

const store = new Map<string, string>();
(globalThis as any).localStorage = {
  getItem: (k: string) => store.get(k) ?? null,
  setItem: (k: string, v: string) => { store.set(k, v); },
  removeItem: (k: string) => { store.delete(k); },
};

const NOW = Date.parse('2026-10-21T12:00:00Z');
const realFetch = globalThis.fetch;
let calls: Array<{ url: string; method: string; headers: Record<string, string>; body: string }> = [];
let saved: MalProfilePatch[] = [];
let stored: MalStoredTokens | null = null;

function serve(handler: (url: string, headers: Record<string, string>, body: string) => Response) {
  globalThis.fetch = (async (input: string, init: any) => {
    const headers = (init?.headers ?? {}) as Record<string, string>;
    const body = String(init?.body ?? '');
    calls.push({ url: String(input), method: init?.method ?? 'GET', headers, body });
    return handler(String(input), headers, body);
  }) as typeof fetch;
}

const tokenJson = (access: string, refresh = 'refresh-new') =>
  Response.json({ token_type: 'Bearer', expires_in: 2678400, access_token: access, refresh_token: refresh });

beforeEach(() => {
  store.clear();
  calls = [];
  saved = [];
  stored = null;
  resetHostLimiter();
  process.env.VITE_MAL_CLIENT_ID = 'mal-client';
  delete process.env.VITE_MAL_REDIRECT_URI;
  delete process.env.VITE_MAL_DESKTOP_REDIRECT_URI;
  setMalAuthDeps({
    now: () => NOW,
    loadTokens: async () => stored,
    saveProfile: async (patch) => {
      saved.push(patch);
      if (stored && patch.mal_access_token !== undefined) stored = { ...stored, accessToken: patch.mal_access_token ?? null };
      if (patch.mal_access_token === null) stored = null;
    },
  });
});
afterEach(() => {
  globalThis.fetch = realFetch;
  setMalAuthDeps(null);
});

test('PKCE authorize URL: client_id only, plain challenge equals the verifier, tatakai:// redirect', () => {
  const verifier = generateRandomString(128);
  expect(verifier).toHaveLength(128);
  expect(verifier).toMatch(/^[A-Za-z0-9\-._~]+$/);
  const url = new URL(buildMalAuthUrl({ clientId: 'abc', redirectUri: MAL_DEFAULT_REDIRECT_URI, codeVerifier: verifier, state: 'st' }));
  expect(`${url.origin}${url.pathname}`).toBe('https://myanimelist.net/v1/oauth2/authorize');
  expect(Object.fromEntries(url.searchParams)).toEqual({
    response_type: 'code',
    client_id: 'abc',
    code_challenge: verifier,
    code_challenge_method: 'plain',
    state: 'st',
    redirect_uri: 'tatakai://integration/mal/redirect',
  });
  expect([...url.searchParams.keys()]).not.toContain('client_secret');
});

test('redirect URI defaults to tatakai:// and env overrides win', () => {
  expect(getMalRedirectUri()).toBe('tatakai://integration/mal/redirect');
  process.env.VITE_MAL_REDIRECT_URI = 'tatakai://x/mal';
  expect(getMalRedirectUri()).toBe('tatakai://x/mal');
  process.env.VITE_MAL_DESKTOP_REDIRECT_URI = 'tatakai://desktop/mal';
  expect(getMalRedirectUri()).toBe('tatakai://desktop/mal');
});

test('without a client id the build is "not configured" and login cannot start', () => {
  delete process.env.VITE_MAL_CLIENT_ID;
  expect(isMalConfigured()).toBe(false);
  expect(() => startMalLogin()).toThrow(MalNotConfiguredError);
});

test('startMalLogin stores verifier/state/redirect and marks the login pending', () => {
  const url = new URL(startMalLogin());
  expect(isMalLoginPending()).toBe(true);
  expect(store.get('mal_code_verifier')).toBe(url.searchParams.get('code_challenge'));
  expect(store.get('mal_oauth_state')).toBe(url.searchParams.get('state'));
  expect(store.get('mal_redirect_uri')).toBe('tatakai://integration/mal/redirect');
});

test('code exchange posts a form-encoded PKCE request to the token endpoint, then stores tokens and the MAL user', async () => {
  const url = new URL(startMalLogin());
  const verifier = url.searchParams.get('code_challenge')!;
  const state = url.searchParams.get('state')!;
  serve((u) => (u.includes('/oauth2/token') ? tokenJson('access-1', 'refresh-1') : Response.json({ id: 777, name: 'octavio' })));

  const result = await completeMalLogin({ code: 'the-code', state });

  const exchange = calls[0];
  expect(exchange.url).toBe('https://myanimelist.net/v1/oauth2/token');
  expect(exchange.method).toBe('POST');
  expect(exchange.headers['Content-Type']).toBe('application/x-www-form-urlencoded');
  expect(Object.fromEntries(new URLSearchParams(exchange.body))).toEqual({
    client_id: 'mal-client',
    grant_type: 'authorization_code',
    code: 'the-code',
    code_verifier: verifier,
    redirect_uri: 'tatakai://integration/mal/redirect',
  });
  expect(calls[1].url).toBe('https://api.myanimelist.net/v2/users/@me');
  expect(calls[1].headers.Authorization).toBe('Bearer access-1');
  expect(saved).toEqual([{
    mal_access_token: 'access-1',
    mal_refresh_token: 'refresh-1',
    mal_token_expires_at: new Date(NOW + 2678400 * 1000).toISOString(),
    mal_user_id: '777',
  }]);
  expect(result).toEqual({ userId: '777', username: 'octavio' });
  expect(store.has('mal_code_verifier')).toBe(false);
});

test('a callback with a wrong or missing state, or no login started here, is rejected without network', async () => {
  serve(() => Response.json({}));
  await expect(completeMalLogin({ code: 'c', state: 's' })).rejects.toThrow('not started from this app');
  startMalLogin();
  await expect(completeMalLogin({ code: 'c', state: 'forged' })).rejects.toThrow('does not match');
  await expect(completeMalLogin({ code: 'c', state: null })).rejects.toThrow('does not match');
  expect(calls).toHaveLength(0);
});

test('an invalid_grant on the code exchange surfaces MAL\'s error and stores nothing', async () => {
  const state = new URL(startMalLogin()).searchParams.get('state')!;
  serve(() => Response.json({ error: 'invalid_grant', message: 'Authorization code has expired' }, { status: 400 }));
  await expect(completeMalLogin({ code: 'old', state })).rejects.toThrow('invalid_grant');
  expect(saved).toHaveLength(0);
});

test('a fresh token is returned as is; one about to expire is refreshed first and persisted (rotated refresh token)', async () => {
  stored = { accessToken: 'fresh', refreshToken: 'r1', expiresAt: new Date(NOW + 3600_000).toISOString() };
  serve(() => tokenJson('unused'));
  expect(await getValidMalAccessToken()).toBe('fresh');
  expect(calls).toHaveLength(0);

  stored = { accessToken: 'old', refreshToken: 'r1', expiresAt: new Date(NOW + 60_000).toISOString() };
  serve(() => tokenJson('renewed', 'r2'));
  expect(await getValidMalAccessToken()).toBe('renewed');
  expect(calls).toHaveLength(1);
  expect(calls[0].url).toBe('https://myanimelist.net/v1/oauth2/token');
  expect(Object.fromEntries(new URLSearchParams(calls[0].body))).toEqual({
    client_id: 'mal-client', grant_type: 'refresh_token', refresh_token: 'r1',
  });
  expect(saved[0]).toMatchObject({ mal_access_token: 'renewed', mal_refresh_token: 'r2' });
});

test('concurrent callers share one refresh request', async () => {
  stored = { accessToken: 'old', refreshToken: 'r1', expiresAt: new Date(NOW - 1000).toISOString() };
  serve(() => tokenJson('renewed'));
  const tokens = await Promise.all([getValidMalAccessToken(), getValidMalAccessToken(), getValidMalAccessToken()]);
  expect(tokens).toEqual(['renewed', 'renewed', 'renewed']);
  expect(calls.filter((c) => c.url.includes('/oauth2/token'))).toHaveLength(1);
});

test('a revoked refresh token clears the stored login and asks to re-link', async () => {
  stored = { accessToken: 'old', refreshToken: 'dead', expiresAt: new Date(NOW - 1000).toISOString() };
  serve(() => Response.json({ error: 'invalid_grant' }, { status: 400 }));
  await expect(getValidMalAccessToken()).rejects.toBeInstanceOf(MalAuthExpiredError);
  expect(saved.at(-1)).toEqual({ mal_access_token: null, mal_refresh_token: null, mal_token_expires_at: null, mal_user_id: null });
});

test('not linked at all raises MalAuthExpiredError without network', async () => {
  stored = null;
  serve(() => Response.json({}));
  await expect(getValidMalAccessToken()).rejects.toBeInstanceOf(MalAuthExpiredError);
  expect(calls).toHaveLength(0);
});

test('API 401 -> refresh once -> retry succeeds with the new bearer token', async () => {
  stored = { accessToken: 'stale', refreshToken: 'r1', expiresAt: new Date(NOW + 3600_000).toISOString() };
  serve((u, headers) => {
    if (u.includes('/oauth2/token')) return tokenJson('renewed', 'r2');
    return headers.Authorization === 'Bearer renewed' ? Response.json({ ok: true }) : new Response('{}', { status: 401 });
  });
  const res = await malFetch('/users/@me');
  expect(res.ok).toBe(true);
  expect(calls.map((c) => [new URL(c.url).hostname, c.headers.Authorization ?? ''])).toEqual([
    ['api.myanimelist.net', 'Bearer stale'],
    ['myanimelist.net', ''],
    ['api.myanimelist.net', 'Bearer renewed'],
  ]);
});

test('API 401 even after a refresh clears the login and raises MalAuthExpiredError', async () => {
  stored = { accessToken: 'stale', refreshToken: 'r1', expiresAt: new Date(NOW + 3600_000).toISOString() };
  serve((u) => (u.includes('/oauth2/token') ? tokenJson('renewed') : new Response('{}', { status: 401 })));
  await expect(malFetch('/users/@me')).rejects.toBeInstanceOf(MalAuthExpiredError);
  expect(saved.at(-1)?.mal_access_token).toBeNull();
});

test('form bodies are url-encoded and the bearer token only goes to api.myanimelist.net', async () => {
  stored = { accessToken: 'tok', refreshToken: 'r1', expiresAt: new Date(NOW + 3600_000).toISOString() };
  serve(() => Response.json({}));
  await malFetch('/anime/52991/my_list_status', { method: 'PATCH', form: { status: 'watching', num_watched_episodes: 3, score: undefined } });
  expect(calls[0].url).toBe('https://api.myanimelist.net/v2/anime/52991/my_list_status');
  expect(calls[0].method).toBe('PATCH');
  expect(calls[0].headers['Content-Type']).toBe('application/x-www-form-urlencoded');
  expect(calls[0].body).toBe('status=watching&num_watched_episodes=3');
  expect(calls[0].headers.Authorization).toBe('Bearer tok');
});

test('list paging never follows a next link off api.myanimelist.net (the bearer token would go along)', async () => {
  const { fetchMALAnimeList } = await import('../src/lib/externalIntegrations');
  serve((u) => {
    if (u.includes('/animelist')) return Response.json({ data: [{ node: { id: 1 } }], paging: { next: 'https://evil.example/steal?x=1' } });
    return Response.json({});
  });
  const rows = await fetchMALAnimeList('tok');
  expect(rows).toHaveLength(1);
  expect(calls.map((c) => new URL(c.url).hostname)).toEqual(['api.myanimelist.net']);

  calls = [];
  let page = 0;
  serve(() => Response.json({ data: [{ node: { id: ++page } }], paging: page === 1 ? { next: 'https://api.myanimelist.net/v2/users/@me/animelist?offset=1' } : {} }));
  expect(await fetchMALAnimeList('tok')).toHaveLength(2);
});
