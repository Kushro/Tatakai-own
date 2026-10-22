import { afterEach, beforeEach, expect, test } from 'bun:test';
import { resetHostLimiter } from '../src/lib/net/hostLimiter';
import { malProvider as p } from '../src/lib/metadata/providers/mal';

const realFetch = globalThis.fetch;
const realId = process.env.VITE_MAL_CLIENT_ID;
let calls: Array<{ url: URL; headers: Record<string, string> }> = [];

const FRIEREN = {
  id: 52991, title: 'Sousou no Frieren', main_picture: { medium: 'https://cdn/m.jpg', large: 'https://cdn/l.jpg' },
  alternative_titles: { synonyms: ['Frieren at the Funeral'], en: "Frieren: Beyond Journey's End", ja: '葬送のフリーレン' },
  start_date: '2023-09-29', end_date: '2024-03-22', synopsis: 'During their quest.\n\n[Written by MAL Rewrite]', mean: 9.28,
  popularity: 95, num_list_users: 1500000, nsfw: 'white', genres: [{ id: 2, name: 'Adventure' }, { id: 8, name: 'Drama' }],
  media_type: 'tv', status: 'finished_airing', num_episodes: 28, start_season: { year: 2023, season: 'fall' }, source: 'manga',
  average_episode_duration: 1500, rating: 'pg_13', studios: [{ id: 11, name: 'Madhouse' }],
  related_anime: [{ node: { id: 59978, title: 'Frieren 2', main_picture: { large: 'https://cdn/2.jpg' } }, relation_type: 'sequel' }],
  related_manga: [{ node: { id: 126287, title: 'Sousou no Frieren' }, relation_type: 'adaptation' }],
};
const ADULT = { id: 9, title: 'Adult', nsfw: 'black', media_type: 'ova', rating: 'rx', genres: [{ id: 12, name: 'Hentai' }] };
const MANHWA = { id: 121496, title: 'Solo Leveling', media_type: 'manhwa', status: 'finished', num_chapters: 201, num_volumes: 0, mean: 8.6, genres: [] };

function stub(fn: (u: URL) => Response) {
  globalThis.fetch = (async (input: any, init: any) => {
    const url = new URL(String(input));
    calls.push({ url, headers: init?.headers ?? {} });
    return fn(url);
  }) as typeof fetch;
}
const json = (o: unknown, status = 200) => new Response(JSON.stringify(o), { status });

beforeEach(() => { calls = []; resetHostLimiter(); process.env.VITE_MAL_CLIENT_ID = 'test-client-id'; });
afterEach(() => {
  globalThis.fetch = realFetch;
  if (realId === undefined) delete process.env.VITE_MAL_CLIENT_ID; else process.env.VITE_MAL_CLIENT_ID = realId;
});

test('isConfigured follows the client id', () => {
  expect(p.isConfigured()).toBe(true);
  process.env.VITE_MAL_CLIENT_ID = '  ';
  expect(p.isConfigured()).toBe(false);
});

test('without a client id requests fail with 403 and never hit the network', async () => {
  process.env.VITE_MAL_CLIENT_ID = '';
  stub(() => json({}));
  await expect(p.feed!('ANIME', 'popular', {})).rejects.toMatchObject({ status: 403 });
  expect(calls.length).toBe(0);
});

test('a 403 from MAL surfaces as PublicApiError(403) for router cool-down', async () => {
  stub(() => json({ message: 'forbidden' }, 403));
  await expect(p.feed!('ANIME', 'popular', {})).rejects.toMatchObject({ status: 403 });
});

test('feed ranking lanes, header, fields, paging, adult filter', async () => {
  stub(() => json({ data: [{ node: FRIEREN }, { node: ADULT }], paging: { next: 'x' } }));
  const r = await p.feed!('ANIME', 'trending', { page: 2, perPage: 10 });
  const c = calls[0];
  expect(c.headers['X-MAL-CLIENT-ID']).toBe('test-client-id');
  expect(c.url.pathname).toBe('/v2/anime/ranking');
  expect(c.url.searchParams.get('ranking_type')).toBe('airing');
  expect(c.url.searchParams.get('offset')).toBe('10');
  expect(c.url.searchParams.get('fields')).toContain('alternative_titles');
  expect(c.url.searchParams.has('nsfw')).toBe(false);
  expect(r.hasNextPage).toBe(true);
  expect(r.media.map((m) => m.malId)).toEqual([52991]);

  await p.feed!('ANIME', 'seasonal', { season: 'FALL', seasonYear: 2026, isAdult: true });
  expect(calls[1].url.pathname).toBe('/v2/anime/season/2026/fall');
  expect(calls[1].url.searchParams.get('sort')).toBe('anime_num_list_users');
  expect(calls[1].url.searchParams.get('nsfw')).toBe('true');
});

test('unsupported lanes return an empty page without a request', async () => {
  stub(() => json({ data: [] }));
  expect((await p.feed!('ANIME', 'recently_completed', {})).media).toEqual([]);
  expect((await p.feed!('MANGA', 'upcoming', {})).media).toEqual([]);
  expect((await p.feed!('ANIME', 'popular', { genre: 'Action' })).media).toEqual([]);
  expect(calls.length).toBe(0);
});

test('manga ranking types and mapping', async () => {
  stub(() => json({ data: [{ node: MANHWA }], paging: {} }));
  const r = await p.feed!('MANGA', 'popular', { countryOfOrigin: 'KR' });
  expect(calls[0].url.searchParams.get('ranking_type')).toBe('manhwa');
  expect(r.media[0]).toMatchObject({ format: 'MANGA', countryOfOrigin: 'KR', status: 'FINISHED', chapters: 201, volumes: undefined, averageScore: 86 });
});

test('search needs 3+ chars and applies client-side filters', async () => {
  stub(() => json({ data: [{ node: FRIEREN }, { node: { ...FRIEREN, id: 2, media_type: 'movie' } }], paging: {} }));
  expect((await p.search!('ANIME', { query: 'ab' })).media).toEqual([]);
  expect(calls.length).toBe(0);
  const r = await p.search!('ANIME', { query: 'frieren', format: ['TV'] });
  expect(calls[0].url.searchParams.get('q')).toBe('frieren');
  expect(r.media.map((m) => m.malId)).toEqual([52991]);
});

test('media maps the v2 detail node', async () => {
  stub(() => json(FRIEREN));
  const m = (await p.media!('ANIME', { malId: 52991 }))!;
  expect(calls[0].url.pathname).toBe('/v2/anime/52991');
  expect(m).toMatchObject({
    anilistId: 0, malId: 52991, titleRomaji: 'Sousou no Frieren', titleEnglish: "Frieren: Beyond Journey's End", titleNative: '葬送のフリーレン',
    synonyms: ['Frieren at the Funeral'], format: 'TV', status: 'FINISHED', season: 'FALL', seasonYear: 2023, episodes: 28, duration: 25,
    averageScore: 93, popularity: 1500000, rating: 'PG-13', source: 'MANGA', isAdult: false, source_api: 'mal',
    coverImageLarge: 'https://cdn/l.jpg', coverImageMedium: 'https://cdn/m.jpg',
  });
  expect(m.startDate).toEqual({ year: 2023, month: 9, day: 29 });
  expect(m.description).toBe('During their quest.');
  expect(m.relations!.map((r) => r.relationType)).toEqual(['SEQUEL', 'SOURCE']);
  expect(await p.media!('ANIME', {})).toBeNull();
});

test('malformed list rows are skipped; relation kind follows related_anime/related_manga', async () => {
  const sparse = { id: 5, title: 'Sparse', genres: null, studios: null, alternative_titles: null, main_picture: null };
  const broken = { id: 6, title: 'Broken', genres: 5 };
  stub(() => json({ data: [null, { node: null }, { node: broken }, { node: sparse }], paging: {} }));
  const r = await p.feed!('ANIME', 'popular', {});
  expect(r.media.map((m) => m.malId)).toEqual([5]);
  stub(() => json({ ...FRIEREN, related_anime: [null, ...FRIEREN.related_anime] }));
  const m = (await p.media!('ANIME', { malId: 52991 }))!;
  expect(m.relations!.map((x) => x.kind)).toEqual(['ANIME', 'MANGA']);
});

test('media: 404 gives null, other errors throw', async () => {
  stub(() => json({}, 404));
  expect(await p.media!('MANGA', { malId: 1 })).toBeNull();
  stub(() => json({}, 400));
  await expect(p.media!('MANGA', { malId: 1 })).rejects.toMatchObject({ status: 400 });
  expect(p.episodes).toBeUndefined();
  expect(p.genres).toBeUndefined();
});
