import { afterEach, beforeEach, expect, test } from 'bun:test';
import { createRequire } from 'node:module';
import { resetHostLimiter } from '../src/lib/net/hostLimiter';
import {
  AniListAuthExpiredError,
  AniListUnavailableError,
  ANILIST_DEFAULT_REDIRECT_URI,
  buildAniListAuthUrl,
  completeAniListLogin,
  confirmAniListLogin,
  isAniListLoginPending,
  prepareAniListLogin,
  getAniListRedirectUri,
  isAniListConfigured,
  isAniListTokenExpired,
  markAniListLoginStarted,
  parseAniListCallback,
  setAniListAuthDeps,
  withAniListToken,
  type AniListTokenPatch,
} from '../src/lib/anilist/auth';
import { anilistGql } from '../src/lib/anilist/client';

const store = new Map<string, string>();
(globalThis as any).localStorage = {
  getItem: (k: string) => store.get(k) ?? null,
  setItem: (k: string, v: string) => { store.set(k, v); },
  removeItem: (k: string) => { store.delete(k); },
};

const realFetch = globalThis.fetch;
let calls: Array<{ url: string; init: any }> = [];
let saved: AniListTokenPatch[] = [];
const NOW = Date.parse('2026-10-21T12:00:00Z');

function serve(handler: (url: string, init: any) => Response) {
  globalThis.fetch = (async (input: string, init: any) => {
    calls.push({ url: String(input), init });
    return handler(String(input), init);
  }) as typeof fetch;
}

beforeEach(() => {
  store.clear();
  calls = [];
  saved = [];
  resetHostLimiter();
  setAniListAuthDeps({ now: () => NOW, saveProfile: async (patch) => { saved.push(patch); } });
  delete process.env.VITE_ANILIST_CLIENT_ID;
  delete process.env.VITE_ANILIST_REDIRECT_URI;
  delete process.env.VITE_ANILIST_DESKTOP_REDIRECT_URI;
});
afterEach(() => {
  globalThis.fetch = realFetch;
  setAniListAuthDeps(null);
});

test('authorize URL is the implicit grant with only a client id', () => {
  expect(isAniListConfigured()).toBe(false);
  expect(() => buildAniListAuthUrl()).toThrow('not configured');
  process.env.VITE_ANILIST_CLIENT_ID = '12345';
  expect(isAniListConfigured()).toBe(true);
  const url = new URL(buildAniListAuthUrl());
  expect(`${url.origin}${url.pathname}`).toBe('https://anilist.co/api/v2/oauth/authorize');
  expect(Object.fromEntries(url.searchParams)).toEqual({ client_id: '12345', response_type: 'token' });
});

test('redirect URI defaults to the tatakai:// deep link and honours env overrides', () => {
  expect(getAniListRedirectUri()).toBe('tatakai://integration/anilist/redirect');
  expect(ANILIST_DEFAULT_REDIRECT_URI).toBe('tatakai://integration/anilist/redirect');
  process.env.VITE_ANILIST_REDIRECT_URI = 'tatakai://custom/anilist';
  expect(getAniListRedirectUri()).toBe('tatakai://custom/anilist');
  process.env.VITE_ANILIST_DESKTOP_REDIRECT_URI = 'tatakai://desktop/anilist';
  expect(getAniListRedirectUri()).toBe('tatakai://desktop/anilist');
});

test('parses the token from the URL fragment (hash, bare fragment or full deep link)', () => {
  const frag = 'access_token=aa.bb-cc_dd&token_type=Bearer&expires_in=31536000';
  const expected = { kind: 'token', accessToken: 'aa.bb-cc_dd', tokenType: 'Bearer', expiresIn: 31536000 };
  expect(parseAniListCallback(`#${frag}`)).toEqual(expected as any);
  expect(parseAniListCallback(frag)).toEqual(expected as any);
  expect(parseAniListCallback(`tatakai://integration/anilist/redirect#${frag}`)).toEqual(expected as any);
  expect(parseAniListCallback('')).toBeNull();
  expect(parseAniListCallback('#state=x')).toBeNull();
});

test('parses an access_denied fragment as an error', () => {
  expect(parseAniListCallback('#error=access_denied&error_description=The+user+denied')).toEqual({
    kind: 'error', error: 'access_denied', description: 'The user denied',
  });
});

test('completing the login looks up the viewer with the token and persists token, expiry and user id', async () => {
  serve(() => Response.json({ data: { Viewer: { id: 4242, name: 'octavio' } } }));
  markAniListLoginStarted();
  const result = await completeAniListLogin('#access_token=tok123&token_type=Bearer&expires_in=3600');

  expect(calls).toHaveLength(1);
  expect(calls[0].url).toBe('https://graphql.anilist.co');
  expect(calls[0].init.headers.Authorization).toBe('Bearer tok123');
  expect(String(calls[0].init.body)).toContain('Viewer');
  expect(saved[0]).not.toHaveProperty('anilist_refresh_token');
  expect(saved).toEqual([{
    anilist_access_token: 'tok123',
    anilist_token_expires_at: new Date(NOW + 3600_000).toISOString(),
    anilist_user_id: '4242',
  }]);
  expect(result).toMatchObject({ userId: 4242, username: 'octavio' });
  // the in-flight marker is consumed: a replayed callback is rejected
  await expect(completeAniListLogin('#access_token=tok123')).rejects.toThrow('not started from this app');
});

test('a callback without a login started here is rejected before any network call', async () => {
  serve(() => Response.json({}));
  await expect(completeAniListLogin('#access_token=evil&expires_in=10')).rejects.toThrow('not started from this app');
  expect(calls).toHaveLength(0);
  expect(saved).toHaveLength(0);
});

test('an AniList error callback and an empty callback fail clearly', async () => {
  markAniListLoginStarted();
  await expect(completeAniListLogin('#error=access_denied&error_description=nope')).rejects.toThrow('nope');
  await expect(completeAniListLogin('#')).rejects.toThrow('did not return');
});

test('AniList down (403) during login surfaces "unavailable" with one request and nothing saved', async () => {
  serve(() => new Response('{"errors":[{"message":"API disabled","status":403}]}', { status: 403 }));
  markAniListLoginStarted();
  await expect(completeAniListLogin('#access_token=tok&expires_in=60')).rejects.toBeInstanceOf(AniListUnavailableError);
  expect(calls).toHaveLength(1);
  expect(saved).toHaveLength(0);
});

test('a 401 on a user call clears the stored login and asks to re-link; it does not retry', async () => {
  serve(() => new Response('{"errors":[{"message":"Invalid token","status":401}]}', { status: 401 }));
  await expect(withAniListToken('stale', (token) => anilistGql('query { Viewer { id } }', undefined, { token })))
    .rejects.toBeInstanceOf(AniListAuthExpiredError);
  expect(calls).toHaveLength(1);
  expect(saved).toEqual([{
    anilist_access_token: null, anilist_token_expires_at: null, anilist_user_id: null,
  }]);
});

test('a 403 on a user call raises "unavailable" and keeps the login', async () => {
  serve(() => new Response('{"errors":[{"message":"disabled","status":403}]}', { status: 403 }));
  await expect(withAniListToken('tok', (token) => anilistGql('query { Viewer { id } }', undefined, { token })))
    .rejects.toBeInstanceOf(AniListUnavailableError);
  expect(saved).toHaveLength(0);
});

test('token expiry helper', () => {
  expect(isAniListTokenExpired(null)).toBe(false);
  expect(isAniListTokenExpired(new Date(NOW - 1).toISOString(), NOW)).toBe(true);
  expect(isAniListTokenExpired(new Date(NOW + 1000).toISOString(), NOW)).toBe(false);
});

test('desktop deep-link handler keeps the OAuth fragment and still sanitizes everything else', () => {
  const { handleDeepLink } = createRequire(import.meta.url)('../desktop/window/deep-link.cjs');
  const sent: Array<[string, string]> = [];
  const win = { webContents: { send: (channel: string, route: string) => sent.push([channel, route]) } };
  handleDeepLink('tatakai://integration/anilist/redirect#access_token=aa.bb-cc_dd&token_type=Bearer&expires_in=3600', () => win);
  handleDeepLink('tatakai://integration/mal/redirect?code=abc-1&state=xyz', () => win);
  handleDeepLink('tatakai://anime/12#access_token=leak', () => win);
  handleDeepLink('tatakai://integration/anilist/redirect#access_token=a<script>b', () => win);
  expect(sent).toEqual([
    ['navigate', '/integration/anilist/redirect#access_token=aa.bb-cc_dd&token_type=Bearer&expires_in=3600'],
    ['navigate', '/integration/mal/redirect?code=abc-1&state=xyz'],
    ['navigate', '/anime/12'],
    ['navigate', '/integration/anilist/redirect#access_token=ascriptb'],
  ]);
});

test('the pending marker is consumed by the FIRST callback, success or failure', async () => {
  serve(() => Response.json({ data: { Viewer: { id: 1, name: 'a' } } }));
  markAniListLoginStarted();
  await expect(prepareAniListLogin('#error=access_denied')).rejects.toThrow('AniList');
  expect(isAniListLoginPending()).toBe(false);
  // a token arriving afterwards (forged / replayed) finds no marker
  await expect(prepareAniListLogin('#access_token=evil&expires_in=10')).rejects.toThrow('not started from this app');
  expect(calls).toHaveLength(0);
});

test('the pending marker expires after 5 minutes', async () => {
  let now = NOW;
  setAniListAuthDeps({ now: () => now, saveProfile: async (patch) => { saved.push(patch); } });
  markAniListLoginStarted();
  now = NOW + 4 * 60_000;
  expect(isAniListLoginPending()).toBe(true);
  now = NOW + 5 * 60_000 + 1;
  expect(isAniListLoginPending()).toBe(false);
  await expect(prepareAniListLogin('#access_token=late&expires_in=10')).rejects.toThrow('not started from this app');
});

test('prepare verifies the account but stores nothing until the user confirms it', async () => {
  serve(() => Response.json({ data: { Viewer: { id: 99, name: 'someone' } } }));
  markAniListLoginStarted();
  const pending = await prepareAniListLogin('#access_token=tok&expires_in=3600');
  expect(pending).toMatchObject({ username: 'someone', userId: 99, accessToken: 'tok' });
  expect(saved).toHaveLength(0);
  await confirmAniListLogin(pending);
  expect(saved).toEqual([{
    anilist_access_token: 'tok',
    anilist_token_expires_at: new Date(NOW + 3600_000).toISOString(),
    anilist_user_id: '99',
  }]);
});
