import { beforeEach, expect, test } from 'bun:test';
import { resetHostLimiter } from '../src/lib/net/hostLimiter';
import { registerMetadataProviders, resetProviderHealth } from '../src/lib/metadata/router';
import { DEFAULT_METADATA_SOURCES, setMetadataSources } from '../src/lib/metadata/settings';
import type { MetadataProvider, ProviderId, MetadataFeedKind } from '../src/lib/metadata/types';
import type { TatakaiMedia } from '../src/core/content/types';

console.warn = () => {}; // Dexie/IndexedDB is absent under bun; the cache layer logs and carries on
const store = new Map<string, string>();
(globalThis as any).localStorage = {
  getItem: (k: string) => store.get(k) ?? null,
  setItem: (k: string, v: string) => { store.set(k, v); },
  removeItem: (k: string) => { store.delete(k); },
};

const { contentGraph, parseMediaId } = await import('../src/core/content/content-graph');

const media = (anilistId: number, extra: Partial<TatakaiMedia> = {}): TatakaiMedia => ({
  anilistId, titleRomaji: `T${anilistId}`, synonyms: [], genres: [], tags: [], studios: [],
  isAdult: false, source_api: 'anilist', fetchedAt: 0, ...extra,
});
const fake = (id: ProviderId, impl: Partial<MetadataProvider>): MetadataProvider =>
  ({ id, label: id, description: '', kinds: ['ANIME', 'MANGA'], isConfigured: () => true, ...impl });

const feedCalls: MetadataFeedKind[] = [];
const UUID = '3f2b8c1e-1a2b-4c3d-8e4f-123456789abc';

beforeEach(() => {
  store.clear();
  feedCalls.length = 0;
  resetProviderHealth();
  resetHostLimiter();
  contentGraph.invalidateAll();
  registerMetadataProviders([
    fake('anilist', {
      feed: async (_k, feed, opts) => {
        feedCalls.push(feed);
        const base = feed === 'trending' ? 100 : feed === 'airing' ? 200 : feed === 'popular' ? 300 : feed === 'upcoming' ? 400 : 500;
        return { media: [media(base, { bannerImage: 'b.jpg' }), media(base + 1)], page: opts.page ?? 1, hasNextPage: false };
      },
      search: async (_k, f) => ({
        media: [media(7, { titleRomaji: f.query ?? '' })],
        pageInfo: { total: 1, currentPage: 1, lastPage: 1, hasNextPage: false, perPage: 8 },
      }),
      media: async (_k, ref) =>
        (ref.anilistId === 42 || ref.malId === 4200 ? media(42, { malId: 4200, status: 'RELEASING', episodes: 12 }) : null),
      genres: async () => ['Action', 'Drama'],
      episodes: async () => [{ number: 1, title: 'One', isFiller: false }, { number: 2 }],
    }),
    fake('tenrai', {}), fake('kitsu', {}), fake('shikimori', {}), fake('mal', {}), fake('anizip', {}),
  ]);
  setMetadataSources(DEFAULT_METADATA_SOURCES);
});

test('parseMediaId understands the id forms and rejects legacy UUIDs', () => {
  expect(parseMediaId('21')?.ref).toEqual({ anilistId: 21 });
  expect(parseMediaId('mal:21')?.ref).toEqual({ malId: 21 });
  expect(parseMediaId('anilist-21')?.ref).toEqual({ anilistId: 21 });
  expect(parseMediaId(UUID)).toBeNull();
});

test('getHomePage composes the bundle from router feeds', async () => {
  const home = await contentGraph.getHomePage();
  expect(home.trending.map((m) => m.anilistId)).toEqual([100, 101]);
  expect(home.topAiring[0].anilistId).toBe(200);
  expect(home.popular[0].anilistId).toBe(300);
  expect(home.topUpcoming[0].anilistId).toBe(400);
  expect(home.latestCompleted[0].anilistId).toBe(500);
  expect(home.spotlight[0].bannerImage).toBe('b.jpg');
  expect(home.top10.today.length).toBe(2);
  expect(home.genres).toEqual(['Action', 'Drama']);
});

test('search and getMedia go through the router', async () => {
  expect((await contentGraph.search({ query: 'abc' })).media[0].titleRomaji).toBe('abc');
  expect((await contentGraph.getMedia('42')).malId).toBe(4200);
  expect((await contentGraph.getMedia('mal:4200')).anilistId).toBe(42);
  await expect(contentGraph.getMedia(UUID)).rejects.toThrow();
  await expect(contentGraph.getMedia('999')).rejects.toThrow();
});

test('episodes and next-airing use the router', async () => {
  expect(await contentGraph.getDbEpisodes('42')).toEqual([
    { episodeNumber: 1, title: 'One', isFiller: false },
    { episodeNumber: 2, title: null, isFiller: false },
  ]);
  expect((await contentGraph.getMalEpisodesPage(4200)).hasNextPage).toBe(false);
  expect(await contentGraph.getNextAiring(42)).toMatchObject({ status: 'RELEASING', episodes: 12 });
});
