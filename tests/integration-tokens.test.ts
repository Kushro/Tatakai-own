import { beforeEach, expect, mock, test } from 'bun:test';

type Op = { table: string; op: string; payload?: any; filters: Array<[string, unknown]>; opts?: any };
let ops: Op[] = [];
let selectRows: Record<string, any> = {};

function builder(table: string) {
  const state: Op = { table, op: 'select', filters: [] };
  const finish = () => { ops.push(state); return { error: null, data: null }; };
  const chain: any = {
    select: () => { state.op = 'select'; return chain; },
    upsert: (payload: any, opts: any) => { state.op = 'upsert'; state.payload = payload; state.opts = opts; return Object.assign(Promise.resolve(finish()), chain); },
    update: (payload: any) => { state.op = 'update'; state.payload = payload; return chain; },
    eq: (col: string, val: unknown) => { state.filters.push([col, val]); return chain; },
    maybeSingle: () => { ops.push(state); return Promise.resolve({ data: selectRows[table] ?? null, error: null }); },
    then: (resolve: any) => resolve(finish()),
  };
  return chain;
}

mock.module('../src/integrations/supabase/client', () => ({
  supabase: {
    auth: { getSession: async () => ({ data: { session: { user: { id: 'user-1' } } } }) },
    from: (table: string) => builder(table),
  },
}));

const { setAniListAuthDeps, confirmAniListLogin, clearAniListCredentials, getAniListAccessToken } = await import('../src/lib/anilist/auth');
const { setMalAuthDeps, clearMalCredentials, getValidMalAccessToken } = await import('../src/lib/mal/auth');
const { defaultSyncIO } = await import('../src/lib/listSync');

beforeEach(() => {
  ops = [];
  selectRows = {};
  setAniListAuthDeps(null);
  setMalAuthDeps(null);
});

const TOKEN_KEYS = ['anilist_access_token', 'anilist_token_expires_at', 'anilist_refresh_token', 'mal_access_token', 'mal_refresh_token', 'mal_token_expires_at'];

test('AniList login: token + expiry go to user_integration_tokens, only the user id goes to profiles', async () => {
  await confirmAniListLogin({ accessToken: 'tok', username: 'u', userId: 5, expiresAt: '2027-01-01T00:00:00.000Z' });
  const tokens = ops.find((o) => o.table === 'user_integration_tokens')!;
  const profile = ops.find((o) => o.table === 'profiles')!;
  expect(tokens.op).toBe('upsert');
  expect(tokens.payload).toMatchObject({ user_id: 'user-1', anilist_access_token: 'tok', anilist_token_expires_at: '2027-01-01T00:00:00.000Z' });
  expect(tokens.opts).toEqual({ onConflict: 'user_id' });
  expect(profile.payload).toEqual({ anilist_user_id: '5' });
  expect(profile.filters).toEqual([['user_id', 'user-1']]);
});

test('no code path writes a token column to profiles (AniList or MAL, set or clear)', async () => {
  await confirmAniListLogin({ accessToken: 'tok', username: 'u', userId: 5, expiresAt: null });
  await clearAniListCredentials();
  await clearMalCredentials();
  const profileWrites = ops.filter((o) => o.table === 'profiles');
  expect(profileWrites.length).toBeGreaterThan(0);
  for (const w of profileWrites) {
    for (const key of TOKEN_KEYS) expect(Object.keys(w.payload)).not.toContain(key);
  }
  const tokenWrites = ops.filter((o) => o.table === 'user_integration_tokens');
  expect(tokenWrites.some((w) => w.payload.mal_access_token === null && w.payload.mal_refresh_token === null)).toBe(true);
});

test('MAL tokens are read from the owner-only table, never from profiles', async () => {
  selectRows.user_integration_tokens = { mal_access_token: 'mal-tok', mal_refresh_token: 'r', mal_token_expires_at: new Date(Date.now() + 3600_000).toISOString() };
  expect(await getValidMalAccessToken()).toBe('mal-tok');
  const reads = ops.filter((o) => o.op === 'select');
  expect(reads.map((o) => o.table)).toEqual(['user_integration_tokens']);
  expect(reads[0].filters).toEqual([['user_id', 'user-1']]);
});

test('AniList token reads come from the owner-only table and respect expiry', async () => {
  selectRows.user_integration_tokens = { anilist_access_token: 'al', anilist_token_expires_at: new Date(Date.now() + 3600_000).toISOString() };
  expect(await getAniListAccessToken()).toBe('al');
  selectRows.user_integration_tokens = { anilist_access_token: 'al', anilist_token_expires_at: new Date(Date.now() - 1000).toISOString() };
  expect(await getAniListAccessToken()).toBeNull();
});

const item = (extra: Record<string, unknown>) => ({
  id: 'i', mediaId: 'x', title: 'MAL Title', poster: 'mal-poster', type: 'anime', direction: 'import',
  actionText: '', ...extra,
}) as any;

test('importing onto an existing anime row patches status (and ids it has) without touching name, poster or stored ids', async () => {
  await defaultSyncIO.writeLocal(item({ localMediaIds: ['154587'], remoteStatus: 'completed', malId: 52991, anilistId: null, remoteProgress: 0, localProgress: 0 }));
  const update = ops.find((o) => o.table === 'watchlist')!;
  expect(update.op).toBe('update');
  expect(update.payload).toMatchObject({ status: 'completed', mal_id: 52991 });
  for (const key of ['anime_name', 'anime_poster', 'anilist_id']) expect(Object.keys(update.payload)).not.toContain(key);
  expect(update.filters).toEqual([['user_id', 'user-1'], ['anime_id', '154587']]);
});

test('a progress import records the episode so the item stops reappearing', async () => {
  await defaultSyncIO.writeLocal(item({ localMediaIds: ['154587'], remoteStatus: 'watching', anilistId: 154587, remoteProgress: 8, localProgress: 3 }));
  const history = ops.find((o) => o.table === 'watch_history')!;
  expect(history.op).toBe('upsert');
  expect(history.opts).toEqual({ onConflict: 'user_id,episode_id' });
  expect(history.payload).toMatchObject({ user_id: 'user-1', anime_id: '154587', episode_number: 8, completed: true, anilist_id: 154587 });
  ops = [];
  // no progress change -> no history write
  await defaultSyncIO.writeLocal(item({ localMediaIds: ['154587'], remoteStatus: 'watching', remoteProgress: 3, localProgress: 3 }));
  expect(ops.some((o) => o.table === 'watch_history')).toBe(false);
});

test('a brand new import creates the row with name, poster and both ids', async () => {
  await defaultSyncIO.writeLocal(item({ remoteStatus: 'plan_to_watch', anilistId: 5, malId: 6, remoteProgress: 0 }));
  const row = ops.find((o) => o.table === 'watchlist')!;
  expect(row.op).toBe('upsert');
  expect(row.payload).toMatchObject({ anime_id: '5', anime_name: 'MAL Title', anime_poster: 'mal-poster', mal_id: 6, anilist_id: 5, status: 'plan_to_watch' });
});
