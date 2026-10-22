import { afterEach, beforeEach, expect, test } from 'bun:test';
import { AniListError } from '../src/lib/anilist/client';
import { resetHostLimiter } from '../src/lib/net/hostLimiter';
import { anilistProvider } from '../src/lib/metadata/providers/anilist';

const realFetch = globalThis.fetch;
let calls: Array<{ query: string; variables: Record<string, any> }> = [];

function mockAniList(handler: (variables: Record<string, any>) => { status?: number; body: unknown }) {
  globalThis.fetch = (async (_url: string, init: RequestInit) => {
    const payload = JSON.parse(String(init.body));
    calls.push(payload);
    const { status = 200, body } = handler(payload.variables ?? {});
    return new Response(JSON.stringify(body), { status, headers: { 'content-type': 'application/json' } });
  }) as unknown as typeof fetch;
}

const raw = (id: number, extra: Record<string, unknown> = {}) => ({
  id, idMal: id + 1000, type: 'ANIME', format: 'TV', status: 'RELEASING',
  title: { romaji: `R${id}`, english: `E${id}`, native: `N${id}` },
  coverImage: { extraLarge: `https://img/${id}.jpg` }, genres: ['Action'], isAdult: false,
  studios: { edges: [{ isMain: true, node: { id: 1, name: 'MAPPA' } }] }, source: 'MANGA', ...extra,
});
const page = (media: unknown[], hasNextPage = false) => ({
  data: { Page: { pageInfo: { total: media.length, currentPage: 1, lastPage: 1, hasNextPage, perPage: 20 }, media } },
});

beforeEach(() => { calls = []; resetHostLimiter(); });
afterEach(() => { globalThis.fetch = realFetch; });

test('feed maps kinds to AniList sort/status variables and TatakaiMedia', async () => {
  mockAniList(() => ({ body: page([raw(1)], true) }));
  const r = await anilistProvider.feed!('ANIME', 'trending', { page: 2, perPage: 10 });
  expect(calls[0].variables).toMatchObject({ type: 'ANIME', sort: ['TRENDING_DESC'], page: 2, perPage: 10, isAdult: false });
  expect(r.hasNextPage).toBe(true);
  expect(r.media[0]).toMatchObject({ anilistId: 1, malId: 1001, titleEnglish: 'E1', source_api: 'anilist', coverImageLarge: 'https://img/1.jpg' });

  await anilistProvider.feed!('ANIME', 'airing', {});
  expect(calls[1].variables).toMatchObject({ status_in: ['RELEASING'], sort: ['POPULARITY_DESC'] });
  await anilistProvider.feed!('ANIME', 'recently_completed', {});
  expect(calls[2].variables).toMatchObject({ status_in: ['FINISHED'], sort: ['END_DATE_DESC'] });
  await anilistProvider.feed!('ANIME', 'seasonal', { season: 'FALL', seasonYear: 2026 });
  expect(calls[3].variables).toMatchObject({ season: 'FALL', seasonYear: 2026 });
});

test('isAdult is only lifted when explicitly true', async () => {
  mockAniList(() => ({ body: page([]) }));
  await anilistProvider.feed!('ANIME', 'popular', {});
  await anilistProvider.feed!('ANIME', 'popular', { isAdult: true });
  expect(calls[0].variables.isAdult).toBe(false);
  expect('isAdult' in calls[1].variables).toBe(false);
});

test('manga feeds honor countryOfOrigin, novelsOnly and exclude novels otherwise', async () => {
  mockAniList(() => ({ body: page([]) }));
  await anilistProvider.feed!('MANGA', 'popular', { countryOfOrigin: 'KR' });
  await anilistProvider.feed!('MANGA', 'popular', { novelsOnly: true });
  expect(calls[0].variables).toMatchObject({ type: 'MANGA', country: 'KR', format_not_in: ['NOVEL'] });
  expect(calls[1].variables).toMatchObject({ format_in: ['NOVEL'] });
  expect('format_not_in' in calls[1].variables).toBe(false);
});

test('search builds filter variables and post-filters studio/source', async () => {
  mockAniList(() => ({
    body: page([raw(1), raw(2, { studios: { edges: [{ isMain: true, node: { id: 2, name: 'Bones' } }] }, source: 'ORIGINAL' })]),
  }));
  const r = await anilistProvider.search!('ANIME', {
    query: ' naruto ', genres: ['Action'], tags: ['Ninja'], year: { min: 2000, max: 2010 }, rating: { min: 70, max: 90 },
    episodes: { min: 12 }, format: ['TV'], status: ['FINISHED'], season: ['FALL'], sortBy: 'SCORE_DESC', page: 3, perPage: 5,
    studio: ['mappa'],
  });
  expect(calls[0].variables).toMatchObject({
    search: 'naruto', genre_in: ['Action'], tag_in: ['Ninja'], startDate_greater: 20000000, startDate_lesser: 20110000,
    averageScore_greater: 69, averageScore_lesser: 91, episodes_greater: 11, format_in: ['TV'], status_in: ['FINISHED'],
    season: 'FALL', sort: ['SCORE_DESC'], page: 3, perPage: 5,
  });
  expect(r.media.map((m) => m.anilistId)).toEqual([1]);
  expect(r.pageInfo).toMatchObject({ currentPage: 1, hasNextPage: false, perPage: 20 });
});

test('media returns null for not found and maps detail', async () => {
  mockAniList((v) => v.id === 9
    ? { status: 404, body: { errors: [{ message: 'Not Found.', status: 404 }] } }
    : { body: { data: { Media: raw(5) } } });
  expect(await anilistProvider.media!('ANIME', { anilistId: 9 })).toBeNull();
  expect((await anilistProvider.media!('ANIME', { malId: 1005 }))?.anilistId).toBe(5);
  expect(calls[1].variables).toMatchObject({ idMal: 1005, type: 'ANIME' });
});

test('a 403 (API disabled) surfaces as AniListError so the router falls back', async () => {
  mockAniList(() => ({ status: 403, body: { errors: [{ message: 'temporarily disabled', status: 403 }] } }));
  const err = await anilistProvider.feed!('ANIME', 'trending', { retries: 0 } as any).catch((e) => e);
  expect(err).toBeInstanceOf(AniListError);
  expect((err as AniListError).status).toBe(403);
});

test('ids resolves by anilist id, and by MAL id only when the kind is known', async () => {
  mockAniList(() => ({ body: { data: { Media: { id: 7, idMal: 77 } } } }));
  expect(await anilistProvider.ids!({ malId: 77 })).toBeNull();
  expect(calls.length).toBe(0);
  expect(await (anilistProvider.ids as any)({ malId: 77 }, { kind: 'MANGA' })).toMatchObject({ anilistId: 7, malId: 77 });
  expect(calls[0].variables).toMatchObject({ idMal: 77, type: 'MANGA' });
});
