import { afterEach, beforeEach, expect, test } from 'bun:test';
import { AniListError } from '../src/lib/anilist/client';
import { resetHostLimiter } from '../src/lib/net/hostLimiter';
import { ensureAnilistIds, rememberIds } from '../src/lib/metadata/ids';
import { metadata, orderedProviders, registerMetadataProviders, resetProviderHealth, NoMetadataProviderError } from '../src/lib/metadata/router';
import { DEFAULT_METADATA_SOURCES, getMetadataSources, setMetadataSources } from '../src/lib/metadata/settings';
import type { MetadataProvider, ProviderId } from '../src/lib/metadata/types';
import type { TatakaiMedia } from '../src/core/content/types';

const store = new Map<string, string>();
(globalThis as any).localStorage = {
  getItem: (k: string) => store.get(k) ?? null,
  setItem: (k: string, v: string) => { store.set(k, v); },
  removeItem: (k: string) => { store.delete(k); },
};

const media = (anilistId: number, extra: Partial<TatakaiMedia> = {}): TatakaiMedia => ({
  anilistId, titleRomaji: `T${anilistId || extra.malId}`, synonyms: [], genres: [], tags: [], studios: [],
  isAdult: false, source_api: 'anilist', fetchedAt: 0, ...extra,
});

function fake(id: ProviderId, impl: Partial<MetadataProvider>): MetadataProvider {
  return { id, label: id, description: '', kinds: ['ANIME', 'MANGA'], isConfigured: () => true, ...impl };
}

const realFetch = globalThis.fetch;
beforeEach(() => {
  store.clear();
  resetProviderHealth();
  resetHostLimiter();
});
afterEach(() => { globalThis.fetch = realFetch; });

test('settings keep user order, drop unknown ids and append new providers', () => {
  setMetadataSources([{ id: 'kitsu', enabled: true }, { id: 'nope' as ProviderId, enabled: true }, { id: 'anilist', enabled: false }]);
  const s = getMetadataSources();
  expect(s.slice(0, 2)).toEqual([{ id: 'kitsu', enabled: true }, { id: 'anilist', enabled: false }]);
  expect(s.map((x) => x.id).sort()).toEqual(DEFAULT_METADATA_SOURCES.map((x) => x.id).sort());
});

test('router falls back past a disabled AniList and cools it down', async () => {
  let anilistCalls = 0;
  registerMetadataProviders([
    fake('anilist', { feed: async () => { anilistCalls++; throw new AniListError('API disabled', 403); } }),
    fake('tenrai', { feed: async () => ({ media: [media(21)], page: 1, hasNextPage: false }) }),
    fake('kitsu', {}), fake('shikimori', {}), fake('mal', {}), fake('anizip', {}),
  ]);
  setMetadataSources(DEFAULT_METADATA_SOURCES);
  const first = await metadata.feed('ANIME', 'trending');
  expect(first.provider).toBe('tenrai');
  expect(first.value.media.map((m) => m.anilistId)).toEqual([21]);
  // AniList is now cooling down: not called again.
  await metadata.feed('ANIME', 'trending');
  expect(anilistCalls).toBe(1);
  expect(orderedProviders('feed', 'ANIME').map((p) => p.id)).not.toContain('anilist');
});

test('user priority decides the primary source', async () => {
  registerMetadataProviders([
    fake('anilist', { search: async () => ({ media: [media(1)], pageInfo: { total: 1, currentPage: 1, lastPage: 1, hasNextPage: false, perPage: 1 } }) }),
    fake('kitsu', { search: async () => ({ media: [media(2)], pageInfo: { total: 1, currentPage: 1, lastPage: 1, hasNextPage: false, perPage: 1 } }) }),
  ]);
  setMetadataSources([{ id: 'kitsu', enabled: true }, { id: 'anilist', enabled: true }]);
  expect((await metadata.search('ANIME', { query: 'x' })).provider).toBe('kitsu');
});

test('empty results fall through; all-failed raises NoMetadataProviderError', async () => {
  registerMetadataProviders([
    fake('anilist', { media: async () => null }),
    fake('tenrai', { media: async () => { throw new Error('boom'); } }),
    fake('kitsu', { media: async () => media(5) }),
    fake('shikimori', {}), fake('mal', {}), fake('anizip', {}),
  ]);
  setMetadataSources(DEFAULT_METADATA_SOURCES);
  rememberIds('ANIME', { anilistId: 5, malId: 50, kitsuId: 500 });
  expect((await metadata.media('ANIME', { anilistId: 5 })).provider).toBe('kitsu');

  // A genuine "not found" from one source plus failures elsewhere = null, not an error.
  registerMetadataProviders([fake('kitsu', { media: async () => { throw new Error('down'); } })]);
  resetProviderHealth();
  expect((await metadata.media('ANIME', { anilistId: 5 })).value).toBeNull();

  // Every source failing is an error the UI can show.
  registerMetadataProviders([fake('anilist', { media: async () => { throw new Error('down'); } })]);
  resetProviderHealth();
  await expect(metadata.media('ANIME', { anilistId: 5 })).rejects.toBeInstanceOf(NoMetadataProviderError);
});

test('relations from MAL-keyed providers are re-keyed to AniList ids; unknown ones dropped', async () => {
  rememberIds('ANIME', { anilistId: 1, malId: 10 });
  rememberIds('ANIME', { anilistId: 2, malId: 20 });
  globalThis.fetch = (async () => new Response('{}', { status: 404 })) as typeof fetch;
  registerMetadataProviders([
    fake('anilist', { media: async () => { throw new AniListError('API disabled', 403); } }),
    fake('tenrai', {
      media: async () => media(0, {
        malId: 10, source_api: 'tenrai',
        relations: [
          { id: 20, relationType: 'SEQUEL', titleRomaji: 'S2', format: 'TV' },
          { id: 999, relationType: 'SIDE_STORY', titleRomaji: 'Unknown', format: 'OVA' },
        ],
      }),
    }),
    fake('kitsu', {}), fake('shikimori', {}), fake('mal', {}), fake('anizip', {}),
  ]);
  setMetadataSources(DEFAULT_METADATA_SOURCES);
  const r = await metadata.media('ANIME', { malId: 10 });
  expect(r.provider).toBe('tenrai');
  expect(r.value?.anilistId).toBe(1);
  expect(r.value?.relations?.map((x) => [x.id, x.titleRomaji])).toEqual([[2, 'S2']]);
});

test('MAL-keyed results get AniList ids from ani.zip; unresolvable items are dropped', async () => {
  globalThis.fetch = (async (url: string) => {
    const u = String(url);
    if (u.includes('mal_id=21')) return new Response(JSON.stringify({ mappings: { anilist_id: 21, mal_id: 21, kitsu_id: 12 } }), { status: 200 });
    return new Response('{}', { status: 404 });
  }) as typeof fetch;
  const out = await ensureAnilistIds('ANIME', [media(0, { malId: 21, source_api: 'tenrai' }), media(0, { malId: 999999, source_api: 'tenrai' }), media(7)]);
  expect(out.map((m) => [m.anilistId, m.kitsuId])).toEqual([[21, 12], [7, undefined]]);
});

test('adult titles are filtered centrally unless isAdult === true', async () => {
  registerMetadataProviders([
    fake('anilist', { feed: async () => ({ media: [media(1), media(2, { isAdult: true })], page: 1, hasNextPage: false }) }),
  ]);
  setMetadataSources([{ id: 'anilist', enabled: true }]);
  expect((await metadata.feed('ANIME', 'trending')).value.media.map((m) => m.anilistId)).toEqual([1]);
  expect((await metadata.feed('ANIME', 'trending', { isAdult: false })).value.media.map((m) => m.anilistId)).toEqual([1]);
  expect((await metadata.feed('ANIME', 'trending', { isAdult: true })).value.media.map((m) => m.anilistId)).toEqual([1, 2]);
});

test('a mapper bug falls back without cooling the provider down', async () => {
  let calls = 0;
  registerMetadataProviders([
    fake('anilist', { feed: async () => { calls++; throw new TypeError("Cannot read properties of null (reading 'jpg')"); } }),
    fake('tenrai', { feed: async () => ({ media: [media(3)], page: 1, hasNextPage: false }) }),
  ]);
  setMetadataSources([{ id: 'anilist', enabled: true }, { id: 'tenrai', enabled: true }]);
  expect((await metadata.feed('ANIME', 'popular')).provider).toBe('tenrai');
  await metadata.feed('ANIME', 'popular');
  expect(calls).toBe(2);
});

test('a page that is empty after id resolution falls through to the next provider', async () => {
  globalThis.fetch = (async () => new Response('{}', { status: 404 })) as typeof fetch;
  registerMetadataProviders([
    fake('tenrai', { feed: async () => ({ media: [media(0, { malId: 123456789, source_api: 'tenrai' })], page: 1, hasNextPage: false }) }),
    fake('kitsu', { feed: async () => ({ media: [media(4)], page: 1, hasNextPage: false }) }),
  ]);
  setMetadataSources([{ id: 'tenrai', enabled: true }, { id: 'kitsu', enabled: true }]);
  const r = await metadata.feed('ANIME', 'upcoming');
  expect(r.provider).toBe('kitsu');
  expect(r.value.media.map((m) => m.anilistId)).toEqual([4]);
});

test('list ids resolve through idsBatch in one call, before per-item lookups', async () => {
  let batchCalls = 0;
  let perItem = 0;
  globalThis.fetch = (async () => { perItem++; return new Response('{}', { status: 404 }); }) as typeof fetch;
  registerMetadataProviders([
    fake('tenrai', { feed: async () => ({ media: [media(0, { malId: 501 }), media(0, { malId: 502 })], page: 1, hasNextPage: false }) }),
    fake('kitsu', {
      idsBatch: async (_kind, refs) => { batchCalls++; return refs.map((r) => (r.malId === 501 ? { anilistId: 9501, malId: 501 } : null)); },
    }),
  ]);
  setMetadataSources([{ id: 'tenrai', enabled: true }, { id: 'kitsu', enabled: true }]);
  const r = await metadata.feed('MANGA', 'popular');
  expect(batchCalls).toBe(1);
  expect(perItem).toBe(0); // manga misses after a bulk pass are not retried one by one
  expect(r.value.media.map((m) => m.anilistId)).toEqual([9501]);
});
