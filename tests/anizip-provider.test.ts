import { afterEach, beforeEach, expect, test } from 'bun:test';
import { resetHostLimiter } from '../src/lib/net/hostLimiter';
import { clearMappingCache } from '../src/lib/mapping/client';
import { normalizeAniZip } from '../src/lib/mapping/anizip';
import { anizipProvider } from '../src/lib/metadata/providers/anizip';
import fixture from './fixtures/metadata/anizip-frieren.json';

const realFetch = globalThis.fetch;
let urls: string[] = [];
function serve(status = 200, body: unknown = fixture) {
  globalThis.fetch = (async (input: string) => {
    urls.push(String(input));
    return status === 200 ? Response.json(body) : new Response('{}', { status });
  }) as typeof fetch;
}
beforeEach(() => { urls = []; resetHostLimiter(); clearMappingCache(); });
afterEach(() => { globalThis.fetch = realFetch; });

test('episodes: numeric keys only, sorted, titles cleaned, UTC dates kept', async () => {
  serve();
  const eps = await anizipProvider.episodes!({ anilistId: 154587 });
  expect(urls).toEqual(['https://api.ani.zip/mappings?anilist_id=154587']);
  expect(eps.map((e) => e.number)).toEqual([1, 2]);
  expect(eps[0]).toMatchObject({
    title: "The Journey's End",
    airDate: '2023-09-29T14:00:00Z',
    runtimeMin: 26,
  });
  expect(eps[0].titles).toMatchObject({ en: "The Journey's End", 'x-jat': 'Bouken no Owari', ja: '冒険の終わり' });
  expect(eps[0].image).toStartWith('https://artworks.thetvdb.com/');
  expect(eps[0].overview).toContain('Frieren');
});

test('episodes accept MAL and Kitsu ids, are cached, and return [] on 404', async () => {
  serve();
  await anizipProvider.episodes!({ malId: 52991 });
  await anizipProvider.episodes!({ malId: 52991 });
  await anizipProvider.episodes!({ kitsuId: 46474 });
  expect(urls).toEqual(['https://api.ani.zip/mappings?mal_id=52991', 'https://api.ani.zip/mappings?kitsu_id=46474']);
  serve(404);
  expect(await anizipProvider.episodes!({ anilistId: 99999999 })).toEqual([]);
  expect(await anizipProvider.episodes!({})).toEqual([]);
});

test('server errors throw so the router can fall back, and are not cached', async () => {
  serve(503);
  await expect(anizipProvider.episodes!({ anilistId: 5 })).rejects.toThrow();
  serve();
  expect((await anizipProvider.episodes!({ anilistId: 5 })).length).toBe(2);
});

test('ids returns the full mapping', async () => {
  serve();
  expect(await anizipProvider.ids!({ anilistId: 154587 })).toEqual({ anilistId: 154587, malId: 52991, kitsuId: 46474 });
  serve(404);
  expect(await anizipProvider.ids!({ anilistId: 1234567 })).toBeNull();
});

test('provider is not a catalog', () => {
  expect(anizipProvider.kinds).toEqual([]);
  expect(anizipProvider.feed).toBeUndefined();
});

test('normalizer: specials split out, aliases best-first, artwork and ids', () => {
  const m = normalizeAniZip(fixture as any, 0, Date.parse('2024-01-01T00:00:00Z'));
  expect(m.anilistId).toBe(154587);
  expect(m.episodes.map((e) => e.key)).toEqual(['1', '2']);
  expect(m.specials.map((e) => [e.key, e.number, e.isSpecial])).toEqual([['S1', 1, true]]);
  expect(m.preferredTitle).toBe("Frieren: Beyond Journey's End");
  expect(m.aliases.slice(0, 3)).toEqual(["Frieren: Beyond Journey's End", 'Sousou no Frieren', '葬送のフリーレン']);
  expect(m.artwork.poster).toContain('posters');
  expect(m.artwork.clearLogo).toContain('clearlogo');
  expect(m.ids).toMatchObject({ malId: 52991, kitsuId: 46474, tvdbId: 424536, tmdbId: '209867', imdbId: 'tt22248376' });
  expect(m.episodeCount).toBe(28);
  expect(m.episodes[0]).toMatchObject({ absoluteNumber: 1, seasonNumber: 1, seasonEpisodeNumber: 1, hasAired: true, rating: 8.03 });
});

test('normalizer: future episodes are unaired', () => {
  const m = normalizeAniZip(fixture as any, 0, Date.parse('2023-09-01T00:00:00Z'));
  expect(m.episodes.every((e) => !e.hasAired)).toBe(true);
});
