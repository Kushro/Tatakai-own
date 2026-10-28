import { afterEach, beforeEach, expect, test } from 'bun:test';
import { resetHostLimiter } from '../src/lib/net/hostLimiter';
import { clearMappingCache, loadMapping } from '../src/lib/mapping/client';
import { fetchAniZipMapping } from '../src/lib/mapping/anizip';
import { buildKitsuHierarchy, fetchKitsuChapterHierarchy } from '../src/lib/mapping/kitsu';
import aniZip from './fixtures/metadata/anizip-frieren.json';
import chapters from './fixtures/metadata/kitsu-chapters.json';

const realFetch = globalThis.fetch;
let urls: URL[] = [];
function serve(handler: (u: URL) => unknown) {
  globalThis.fetch = (async (input: string) => {
    const u = new URL(String(input));
    urls.push(u);
    const out = handler(u);
    return out instanceof Response ? out : Response.json(out);
  }) as typeof fetch;
}
beforeEach(() => { urls = []; resetHostLimiter(); clearMappingCache(); });
afterEach(() => { globalThis.fetch = realFetch; });

test('fetchAniZipMapping calls ani.zip directly and keeps the AniZipMapping shape', async () => {
  serve(() => aniZip);
  const m = await fetchAniZipMapping(154587);
  expect(urls.map((u) => u.href)).toEqual(['https://api.ani.zip/mappings?anilist_id=154587']);
  expect(m?.episodes).toHaveLength(2);
  expect(m?.specials).toHaveLength(1);
  expect(m?.byNumber.get(2)?.title).toBe("It Didn't Have to Be Magic...");
  expect(m?.lastAiredEpisode).not.toBeNull();
  expect(m?.nextEpisode).toBeNull();
  // second call served from the cache
  await fetchAniZipMapping('154587');
  expect(urls).toHaveLength(1);
});

test('fetchAniZipMapping resolves null on 404, failure and bad ids', async () => {
  serve(() => new Response('{}', { status: 404 }));
  expect(await fetchAniZipMapping(1)).toBeNull();
  serve(() => new Response('x', { status: 503 }));
  expect(await fetchAniZipMapping(2)).toBeNull();
  expect(await fetchAniZipMapping(0)).toBeNull();
  expect(await fetchAniZipMapping(undefined)).toBeNull();
});

test('loadMapping dedupes in-flight loads and rethrows only when asked', async () => {
  let calls = 0;
  const load = async () => { calls++; await new Promise((r) => setTimeout(r, 10)); return 'v'; };
  const [a, b] = await Promise.all([loadMapping('k', load, { ttlMs: 1000 }), loadMapping('k', load, { ttlMs: 1000 })]);
  expect([a, b, calls]).toEqual(['v', 'v', 1]);
  const boom = async () => { throw new Error('x'); };
  expect(await loadMapping('e1', boom, { ttlMs: 1000 })).toBeNull();
  await expect(loadMapping('e2', boom, { ttlMs: 1000, rethrow: true })).rejects.toThrow('x');
});

test('Kitsu chapter hierarchy: AniList id -> Kitsu mapping -> paged /chapters, grouped into volumes', async () => {
  serve((u) => {
    if (u.pathname.endsWith('/mappings')) {
      expect(u.searchParams.get('filter[externalSite]')).toBe('anilist/manga');
      return { data: [{ id: '1', type: 'mappings', attributes: {} }], included: [{ id: '26004', type: 'manga', attributes: { canonicalTitle: 'BNHA' } }] };
    }
    expect(u.pathname).toBe('/api/edge/manga/26004/chapters');
    return chapters; // 3 rows (< page size) => last page
  });
  const h = await fetchKitsuChapterHierarchy(30002);
  expect(h?.kitsuMangaId).toBe(26004);
  expect(h?.totalChapters).toBe(3);
  expect(h?.chapters.map((c) => c.number)).toEqual([1, 2, 3]);
  expect(h?.chapters[0]).toMatchObject({ title: 'Chapter 1', isGeneratedTitle: true, volumeNumber: 1 });
  expect(h?.volumes[0]).toMatchObject({ volumeNumber: 1, firstChapter: 1, lastChapter: 3 });
  expect(h?.totalVolumes).toBe(1);
  expect(h?.truncated).toBe(false);
});

test('Kitsu chapter hierarchy: explicit kitsuId skips the lookup; no mapping/failure resolves null', async () => {
  serve(() => chapters);
  expect(await fetchKitsuChapterHierarchy(null, { kitsuId: 26004 })).not.toBeNull();
  expect(urls.every((u) => u.pathname.includes('/chapters'))).toBe(true);
  serve(() => ({ data: [] }));
  expect(await fetchKitsuChapterHierarchy(5)).toBeNull();
  serve(() => new Response('x', { status: 503 }));
  expect(await fetchKitsuChapterHierarchy(6)).toBeNull();
  expect(await fetchKitsuChapterHierarchy(null)).toBeNull();
});

test('buildKitsuHierarchy puts unassigned chapters in a trailing bucket', () => {
  const row = (id: string, number: number, volumeNumber: number | null) => ({ id, type: 'chapters', attributes: { number, volumeNumber, canonicalTitle: number === 2 ? 'Two' : null, published: '2020-01-02T00:00:00Z' } });
  const h = buildKitsuHierarchy(1, [row('c', 5, null), row('a', 1, 1), row('b', 2, 1), row('a', 1, 1)]);
  expect(h.chapters.map((c) => c.number)).toEqual([1, 2, 5]);
  expect(h.volumes.map((v) => v.volumeNumber)).toEqual([1, null]);
  expect(h.totalVolumes).toBe(1);
  expect(h.chapters[1]).toMatchObject({ title: 'Two', isGeneratedTitle: false, published: '2020-01-02' });
});
