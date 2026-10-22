import { afterEach, beforeEach, expect, test } from 'bun:test';
import { resetHostLimiter } from '../src/lib/net/hostLimiter';
import { clearMappingCache } from '../src/lib/mapping/client';
import { kitsuProvider, mapKitsuEpisode } from '../src/lib/metadata/providers/kitsu';
import animeList from './fixtures/metadata/kitsu-anime-list.json';
import mangaList from './fixtures/metadata/kitsu-manga-list.json';
import animeDetail from './fixtures/metadata/kitsu-anime-detail.json';
import episodesDoc from './fixtures/metadata/kitsu-episodes.json';

const store = new Map<string, string>();
(globalThis as any).localStorage = {
  getItem: (k: string) => store.get(k) ?? null,
  setItem: (k: string, v: string) => { store.set(k, v); },
  removeItem: (k: string) => { store.delete(k); },
};

const realFetch = globalThis.fetch;
let urls: URL[] = [];
function serve(handler: (u: URL) => unknown | Response) {
  globalThis.fetch = (async (input: string) => {
    const u = new URL(String(input));
    urls.push(u);
    const out = handler(u);
    return out instanceof Response ? out : Response.json(out);
  }) as typeof fetch;
}
beforeEach(() => { urls = []; store.clear(); resetHostLimiter(); clearMappingCache(); });
afterEach(() => { globalThis.fetch = realFetch; });

const withNext = (doc: any) => ({ ...doc, links: { next: 'https://kitsu.io/next' } });

test('popular feed maps AniList/MAL ids from included mappings and reports paging', async () => {
  serve(() => withNext(animeList));
  const page = await kitsuProvider.feed!('ANIME', 'popular', { page: 2, perPage: 2 });
  const q = urls[0].searchParams;
  expect(urls[0].pathname).toBe('/api/edge/anime');
  expect(q.get('sort')).toBe('-userCount');
  expect(q.get('page[limit]')).toBe('2');
  expect(q.get('page[offset]')).toBe('2');
  expect(q.get('include')).toBe('mappings,genres');
  expect(page.hasNextPage).toBe(true);
  const [aot] = page.media;
  expect(aot).toMatchObject({ kitsuId: 7442, anilistId: 16498, malId: 16498, titleEnglish: 'Attack on Titan', titleRomaji: 'Shingeki no Kyojin', titleNative: '進撃の巨人', source_api: 'kitsu', format: 'TV', status: 'FINISHED', countryOfOrigin: 'JP', isAdult: false });
  expect(aot.genres).toContain('Action');
  expect(aot.averageScore).toBe(84);
  expect(aot.startDate).toEqual({ year: 2013, month: 4, day: 7 });
  expect(aot.season).toBe('SPRING');
  expect(aot.description).not.toContain('Source:');
});

test('feed lanes translate to Kitsu filters', async () => {
  serve(() => animeList);
  await kitsuProvider.feed!('ANIME', 'airing', {});
  await kitsuProvider.feed!('ANIME', 'upcoming', {});
  await kitsuProvider.feed!('ANIME', 'top_rated', {});
  await kitsuProvider.feed!('ANIME', 'seasonal', { season: 'FALL', seasonYear: 2025 });
  await kitsuProvider.feed!('ANIME', 'recently_completed', {});
  const [airing, upcoming, top, seasonal, done] = urls.map((u) => u.searchParams);
  expect(airing.get('filter[status]')).toBe('current');
  expect(upcoming.get('filter[status]')).toBe('upcoming,unreleased');
  expect(top.get('sort')).toBe('-averageRating');
  expect([seasonal.get('filter[season]'), seasonal.get('filter[seasonYear]')]).toEqual(['fall', '2025']);
  expect([done.get('filter[status]'), done.get('sort')]).toEqual(['finished', '-startDate']);
});

test('trending uses /trending then re-fetches ids with mappings, keeping trending order; page 2 falls back to popular', async () => {
  serve((u) => {
    if (u.pathname.endsWith('/trending/anime')) return { data: [{ id: '11469', type: 'anime', attributes: {} }, { id: '7442', type: 'anime', attributes: {} }] };
    return animeList;
  });
  const first = await kitsuProvider.feed!('ANIME', 'trending', { perPage: 2 });
  expect(urls[1].searchParams.get('filter[id]')).toBe('11469,7442');
  expect(first.media.map((m) => m.kitsuId)).toEqual([11469, 7442]);
  urls = [];
  await kitsuProvider.feed!('ANIME', 'trending', { page: 2, perPage: 2 });
  expect(urls).toHaveLength(1);
  expect(urls[0].searchParams.get('sort')).toBe('-userCount');
});

test('isAdult=false drops adult titles; manga country/novel options map to subtype', async () => {
  const adult = JSON.parse(JSON.stringify(animeList));
  adult.data[1].attributes.nsfw = true;
  serve(() => adult);
  expect((await kitsuProvider.feed!('ANIME', 'popular', { isAdult: false })).media).toHaveLength(1);
  // adult titles are excluded unless the caller explicitly opts in
  expect((await kitsuProvider.feed!('ANIME', 'popular', {})).media).toHaveLength(1);
  expect((await kitsuProvider.feed!('ANIME', 'popular', { isAdult: true })).media).toHaveLength(2);
  expect((await kitsuProvider.search!('ANIME', { query: 'x' })).media).toHaveLength(1);
  expect((await kitsuProvider.search!('ANIME', { query: 'x', isAdult: true })).media).toHaveLength(2);
  serve(() => mangaList);
  urls = [];
  await kitsuProvider.feed!('MANGA', 'popular', { countryOfOrigin: 'KR' });
  await kitsuProvider.feed!('MANGA', 'popular', { novelsOnly: true });
  expect(urls.map((u) => u.searchParams.get('filter[subtype]'))).toEqual(['manhwa', 'novel']);
});

test('manga items map chapters/volumes and ids', async () => {
  serve(() => mangaList);
  const { media } = await kitsuProvider.feed!('MANGA', 'popular', {});
  expect(media[0]).toMatchObject({ kitsuId: 26004, format: 'MANGA', chapters: 432, volumes: 42, countryOfOrigin: 'JP' });
  expect(media[0].anilistId).toBeGreaterThan(0);
});

test('search sends text and filters, with paging info', async () => {
  serve(() => ({ ...animeList, meta: { count: 41 } }));
  const r = await kitsuProvider.search!('ANIME', { query: ' frieren ', genres: ['Action'], format: ['MOVIE'], status: ['FINISHED'], year: { min: 2020, max: 2022 }, sortBy: 'SCORE_DESC', page: 1, perPage: 20 });
  const q = urls[0].searchParams;
  expect(q.get('filter[text]')).toBe('frieren');
  expect(q.get('filter[genres]')).toBe('Action');
  expect(q.get('filter[subtype]')).toBe('movie');
  expect(q.get('filter[status]')).toBe('finished');
  expect(q.get('filter[year]')).toBe('2020..2022');
  expect(q.get('sort')).toBe('-averageRating');
  expect(r.pageInfo).toMatchObject({ total: 41, lastPage: 3, currentPage: 1 });
});

test('media resolves the Kitsu id from an AniList id and maps studios/tags', async () => {
  serve((u) => {
    if (u.pathname.endsWith('/mappings')) {
      return { data: [{ id: '1', type: 'mappings', attributes: { externalSite: 'anilist/anime', externalId: '16498' } }], included: [{ id: '7442', type: 'anime', attributes: { canonicalTitle: 'Shingeki no Kyojin' } }] };
    }
    return animeDetail;
  });
  const m = await kitsuProvider.media!('ANIME', { anilistId: 16498 });
  expect(urls[0].searchParams.get('filter[externalSite]')).toBe('anilist/anime');
  expect(urls[1].pathname).toBe('/api/edge/anime/7442');
  expect(m?.kitsuId).toBe(7442);
  expect(m?.anilistId).toBe(16498);
  expect(m?.studios.length).toBeGreaterThan(0);
  expect(m?.tags.length).toBeGreaterThan(0);
});

test('media returns null when Kitsu has no mapping or rejects the id, and throws on server failure', async () => {
  serve(() => ({ data: [] }));
  expect(await kitsuProvider.media!('ANIME', { anilistId: 1 })).toBeNull();
  serve(() => new Response('{}', { status: 400 }));
  expect(await kitsuProvider.media!('ANIME', { kitsuId: 5 })).toBeNull();
  serve(() => new Response('boom', { status: 503 }));
  await expect(kitsuProvider.media!('ANIME', { kitsuId: 6 }, { signal: undefined })).rejects.toThrow();
});

test('episodes page through /episodes and map fields', async () => {
  serve(() => episodesDoc);
  const eps = await kitsuProvider.episodes!({ kitsuId: 7442 });
  expect(urls[0].pathname).toBe('/api/edge/anime/7442/episodes');
  expect(urls[0].searchParams.get('page[limit]')).toBe('20');
  expect(eps.map((e) => e.number)).toEqual([1, 2, 3]);
  expect(eps[0]).toMatchObject({ title: "The Journey's End", airDate: '2023-09-29', runtimeMin: 24 });
  expect(eps[0].image).toStartWith('https://');
  expect(mapKitsuEpisode({ id: '1', type: 'episodes', attributes: {} })).toBeNull();
});

test('genres are paged, cached and hide adult ones by default', async () => {
  serve((u) => {
    const offset = Number(u.searchParams.get('page[offset]'));
    const row = (name: string) => ({ id: name, type: 'genres', attributes: { name } });
    return { data: offset === 0 ? Array.from({ length: 20 }, (_, i) => row(`G${i}`)) : [row('Hentai'), row('Action')] };
  });
  const g = await kitsuProvider.genres!('ANIME');
  expect(g).toContain('Action');
  expect(g).not.toContain('Hentai');
  expect(urls).toHaveLength(2);
  expect(await kitsuProvider.genres!('MANGA', { includeAdult: true })).toContain('Hentai');
  expect(urls).toHaveLength(2);
});

test('ids resolves manga AniList <-> MAL <-> Kitsu through mappings', async () => {
  serve((u) => {
    if (u.pathname.endsWith('/mappings')) {
      return { data: [{ id: '1', type: 'mappings', attributes: {} }], included: [{ id: '54139', type: 'manga', attributes: { canonicalTitle: 'X' } }] };
    }
    return {
      data: { id: '54139', type: 'manga', attributes: {}, relationships: { mappings: { data: [{ id: 'a', type: 'mappings' }, { id: 'b', type: 'mappings' }] } } },
      included: [
        { id: 'a', type: 'mappings', attributes: { externalSite: 'anilist/manga', externalId: '105778' } },
        { id: 'b', type: 'mappings', attributes: { externalSite: 'myanimelist/manga', externalId: '108556' } },
      ],
    };
  });
  expect(await kitsuProvider.ids!({ anilistId: 105778 })).toEqual({ anilistId: 105778, malId: 108556, kitsuId: 54139 });
  expect(urls[0].searchParams.get('filter[externalSite]')).toBe('anilist/manga');
  clearMappingCache();
  expect(await kitsuProvider.ids!({ malId: 108556 })).toMatchObject({ anilistId: 105778 });
});

const mappingRow = (id: string, site: string, externalId: number, itemId: number, kind = 'manga') => ({
  id, type: 'mappings', attributes: { externalSite: site, externalId: String(externalId) },
  relationships: { item: { data: { type: kind, id: String(itemId) } } },
});
const itemDoc = (kind: string, ids: Array<[number, number, number]>) => {
  const mappings: any[] = [];
  const data = ids.map(([kitsu, al, mal]) => {
    mappings.push({ id: `a${kitsu}`, type: 'mappings', attributes: { externalSite: `anilist/${kind}`, externalId: String(al) } });
    mappings.push({ id: `m${kitsu}`, type: 'mappings', attributes: { externalSite: `myanimelist/${kind}`, externalId: String(mal) } });
    return { id: String(kitsu), type: kind, attributes: {}, relationships: { mappings: { data: [{ type: 'mappings', id: `a${kitsu}` }, { type: 'mappings', id: `m${kitsu}` }] } } };
  });
  return { data, included: mappings };
};

test('idsBatch resolves MAL ids in bulk (2 requests for 3 titles), keeps input order, null for unknown', async () => {
  serve((u) => {
    if (u.pathname.endsWith('/mappings')) {
      expect(u.searchParams.get('filter[externalSite]')).toBe('myanimelist/manga');
      expect(u.searchParams.get('filter[externalId]')).toBe('13,999,2');
      return { data: [mappingRow('1', 'myanimelist/manga', 2, 8), mappingRow('2', 'myanimelist/manga', 13, 38)] };
    }
    expect(u.pathname).toBe('/api/edge/manga');
    expect(u.searchParams.get('filter[id]')).toBe('38,8');
    expect(u.searchParams.get('include')).toBe('mappings');
    return itemDoc('manga', [[8, 30002, 2], [38, 30013, 13]]);
  });
  const out = await kitsuProvider.idsBatch!('MANGA', [{ malId: 13 }, { malId: 999 }, { malId: 2 }]);
  expect(urls).toHaveLength(2);
  expect(out).toEqual([
    { anilistId: 30013, malId: 13, kitsuId: 38 },
    null,
    { anilistId: 30002, malId: 2, kitsuId: 8 },
  ]);
});

test('idsBatch handles AniList-only and kitsuId-only refs and chunks 20 per request', async () => {
  const refs = Array.from({ length: 25 }, (_, i) => ({ anilistId: 1000 + i }));
  serve((u) => {
    if (u.pathname.endsWith('/mappings')) {
      expect(u.searchParams.get('filter[externalSite]')).toBe('anilist/anime');
      const ext = u.searchParams.get('filter[externalId]')!.split(',').map(Number);
      expect(ext.length).toBeLessThanOrEqual(20);
      return { data: ext.map((e) => mappingRow(String(e), 'anilist/anime', e, e - 500, 'anime')) };
    }
    const ids = u.searchParams.get('filter[id]')!.split(',').map(Number);
    expect(ids.length).toBeLessThanOrEqual(20);
    return itemDoc('anime', ids.map((k) => [k, k + 500, k + 9000] as [number, number, number]));
  });
  const out = await kitsuProvider.idsBatch!('ANIME', [...refs, { kitsuId: 7 }]);
  expect(out).toHaveLength(26);
  expect(out[0]).toEqual({ anilistId: 1000, malId: 9500, kitsuId: 500 });
  expect(out[24]).toEqual({ anilistId: 1024, malId: 9524, kitsuId: 524 });
  expect(out[25]).toEqual({ anilistId: 507, malId: 9007, kitsuId: 7 });
  expect(urls.filter((u) => u.pathname.endsWith('/mappings'))).toHaveLength(2);
  expect(urls.filter((u) => u.pathname.endsWith('/anime'))).toHaveLength(2);
});
