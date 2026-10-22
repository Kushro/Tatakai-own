import { afterEach, beforeEach, expect, test } from 'bun:test';
import { readFileSync } from 'node:fs';
import { resetHostLimiter } from '../src/lib/net/hostLimiter';
import { resetTenraiCaches, tenraiProvider as p } from '../src/lib/metadata/providers/tenrai';

const fx = (n: string) => JSON.parse(readFileSync(new URL(`./fixtures/metadata/${n}`, import.meta.url), 'utf8'));
const realFetch = globalThis.fetch;
let calls: URL[] = [];

function stub(routes: Record<string, unknown | (() => Response)>) {
  globalThis.fetch = (async (input: any) => {
    const u = new URL(String(input));
    calls.push(u);
    const hit = Object.keys(routes).find((k) => u.pathname === `/v1${k}`);
    if (!hit) return new Response('{}', { status: 404 });
    const v = routes[hit];
    return typeof v === 'function' ? (v as () => Response)() : new Response(JSON.stringify(v), { status: 200 });
  }) as typeof fetch;
}

beforeEach(() => { calls = []; resetHostLimiter(); resetTenraiCaches(); });
afterEach(() => { globalThis.fetch = realFetch; });

test('feed maps lanes to endpoints, drops adult and duplicate rows', async () => {
  stub({ '/top/anime': fx('tenrai-top-anime.json') });
  const r = await p.feed!('ANIME', 'trending', { perPage: 4 });
  expect(calls[0].searchParams.get('filter')).toBe('airing');
  expect(calls[0].searchParams.get('sfw')).toBe('true');
  expect(r.hasNextPage).toBe(true);
  expect(r.media.some((m) => m.titleRomaji === 'Adult Thing')).toBe(false);
  expect(new Set(r.media.map((m) => m.malId)).size).toBe(r.media.length);
  expect(r.media[0]).toMatchObject({ anilistId: 0, source_api: 'tenrai' });

  const adult = await p.feed!('ANIME', 'popular', { isAdult: true });
  expect(calls[1].searchParams.get('filter')).toBe('bypopularity');
  expect(calls[1].searchParams.has('sfw')).toBe(false);
  expect(adult.media.some((m) => m.isAdult)).toBe(true);
});

test('seasonal, genre and manhwa lanes', async () => {
  stub({
    '/seasons/2026/fall': fx('tenrai-top-anime.json'),
    '/genres/anime': fx('tenrai-genres-anime.json'),
    '/anime': fx('tenrai-top-anime.json'),
    '/manga': fx('tenrai-top-anime.json'),
  });
  await p.feed!('ANIME', 'seasonal', { season: 'FALL', seasonYear: 2026 });
  expect(calls[0].pathname).toBe('/v1/seasons/2026/fall');
  await p.feed!('ANIME', 'popular', { genre: 'Thriller' }); // AniList name -> MAL "Suspense"
  const g = calls.find((c) => c.pathname === '/v1/anime')!;
  expect(g.searchParams.get('genres')).toBe(String(fx('tenrai-genres-anime.json').data.find((x: any) => x.name === 'Suspense').mal_id));
  await p.feed!('MANGA', 'popular', { countryOfOrigin: 'KR' });
  expect(calls.find((c) => c.pathname === '/v1/manga')!.searchParams.get('type')).toBe('manhwa');
});

test('media maps Jikan /full to TatakaiMedia', async () => {
  stub({ '/anime/52991/full': fx('tenrai-anime-full.json') });
  const m = (await p.media!('ANIME', { malId: 52991 }))!;
  expect(m).toMatchObject({
    anilistId: 0, malId: 52991, titleRomaji: 'Sousou no Frieren', titleEnglish: "Frieren: Beyond Journey's End",
    titleNative: '葬送のフリーレン', format: 'TV', status: 'FINISHED', season: 'FALL', seasonYear: 2023, episodes: 28,
    duration: 24, averageScore: 92, rating: 'PG-13', source: 'MANGA', isAdult: false, source_api: 'tenrai',
    trailerUrl: 'https://www.youtube.com/watch?v=ZEkwCGJ3o7M',
  });
  expect(m.startDate).toEqual({ year: 2023, month: 9, day: 29 });
  expect(m.endDate).toEqual({ year: 2024, month: 3, day: 22 });
  expect(m.studios[0].name).toBe('Madhouse');
  expect(m.genres).toContain('Shounen');
  expect(m.description).not.toContain('MAL Rewrite');
  expect(m.relations!.map((r) => r.relationType)).toEqual(['SEQUEL', 'SOURCE', 'SIDE_STORY']);
  expect(m.externalLinks!.map((l) => l.type)).toEqual(['INFO', 'STREAMING']);
});

test('manga media: format, country, null chapters', async () => {
  stub({ '/manga/2/full': fx('tenrai-manga-full.json') });
  const m = (await p.media!('MANGA', { malId: 2 }))!;
  expect(m).toMatchObject({ format: 'MANGA', status: 'RELEASING', countryOfOrigin: 'JP', chapters: undefined });
  expect(m.relations![0].relationType).toBe('ADAPTATION');
});

test('media: null without malId and on 404; throws on 5xx', async () => {
  stub({ '/anime/1/full': () => new Response('{}', { status: 404 }), '/anime/2/full': () => new Response('x', { status: 400 }) });
  expect(await p.media!('ANIME', {})).toBeNull();
  expect(await p.media!('ANIME', { malId: 1 })).toBeNull();
  await expect(p.media!('ANIME', { malId: 2 })).rejects.toMatchObject({ status: 400 });
});

test('search builds Jikan query and returns pageInfo', async () => {
  stub({ '/anime': fx('tenrai-top-anime.json') });
  const r = await p.search!('ANIME', { query: 'frieren', format: ['TV'], year: { min: 2020, max: 2030 }, rating: { min: 70 }, sortBy: 'SCORE_DESC', perPage: 4 });
  const q = calls[0].searchParams;
  expect(q.get('q')).toBe('frieren');
  expect(q.get('type')).toBe('tv');
  expect(q.get('start_date')).toBe('2020-01-01');
  expect(q.has('end_date')).toBe(false);
  expect(q.get('min_score')).toBe('7');
  expect(q.get('order_by')).toBe('score');
  expect(r.pageInfo).toMatchObject({ total: 40, lastPage: 10, hasNextPage: true });
});

test('episodes follow pagination and map fields', async () => {
  const pages = [
    { pagination: { has_next_page: true }, data: fx('tenrai-episodes.json').data.slice(0, 1) },
    { pagination: { has_next_page: false }, data: [{ mal_id: 2, title: 'B', aired: '2023-10-06T00:00:00+00:00', duration: 1500, filler: true, recap: false }] },
  ];
  let i = 0;
  stub({ '/anime/52991/episodes': () => new Response(JSON.stringify(pages[i++]), { status: 200 }) });
  const eps = await p.episodes!({ malId: 52991 });
  expect(eps.map((e) => e.number)).toEqual([1, 2]);
  expect(eps[0].titles).toMatchObject({ en: "The Journey's End", ja: '冒険の終わり' });
  expect(eps[1]).toMatchObject({ isFiller: true, runtimeMin: 25 });
  expect(await p.episodes!({})).toEqual([]);
});

test('genres hide adult unless requested', async () => {
  stub({ '/genres/anime': fx('tenrai-genres-anime.json') });
  const safe = await p.genres!('ANIME');
  expect(safe).toContain('Action');
  expect(safe).not.toContain('Hentai');
  expect(await p.genres!('ANIME', { includeAdult: true })).toContain('Hentai');
  expect(calls.length).toBe(1); // cached
});
