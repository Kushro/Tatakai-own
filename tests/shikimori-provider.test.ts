import { afterEach, beforeEach, expect, test } from 'bun:test';
import { resetHostLimiter } from '../src/lib/net/hostLimiter';
import { resetShikimoriCaches, shikimoriProvider as p } from '../src/lib/metadata/providers/shikimori';

const realFetch = globalThis.fetch;
let reqs: Array<{ query: string; variables: any; headers: Record<string, string> }> = [];

const FRIEREN = {
  id: '52991', malId: '52991', name: 'Sousou no Frieren', english: "Frieren: Beyond Journey's End", japanese: '葬送のフリーレン',
  synonyms: ['Фрирен', 'Frieren at the Funeral'], kind: 'tv', status: 'released', episodes: 28, episodesAired: 27, duration: 24,
  rating: 'pg_13', score: 9.25, isCensored: false, season: 'fall_2023', origin: 'manga',
  airedOn: { year: 2023, month: 9, day: 29 }, releasedOn: { year: 2024, month: 3, day: 22 },
  poster: { originalUrl: 'https://shikimori.io/o.jpeg', mainUrl: 'https://shikimori.io/m.webp' },
  genres: [{ id: '27', name: 'Shounen', kind: 'demographic' }, { id: '2', name: 'Adventure', kind: 'genre' }, { id: '114', name: 'Award Winning', kind: 'theme' }],
  studios: [{ id: '11', name: 'Madhouse' }],
  description: 'Одержав победу [character=1]Химмеля[/character]',
  videos: [{ url: 'https://youtu.be/abc', kind: 'pv', name: 'PV' }],
  related: [
    { relationKind: 'sequel', anime: { id: '59978', malId: '59978', name: 'Frieren 2', kind: 'tv', status: 'anons', episodes: 0, poster: null, airedOn: { year: 2026 } }, manga: null },
    { relationKind: 'adaptation', anime: null, manga: { id: '126287', malId: '126287', name: 'Sousou no Frieren', kind: 'manga', poster: null, airedOn: { year: 2020 } } },
  ],
};
const HENTAI = { id: '9', malId: '9', name: 'Adult', kind: 'ova', status: 'released', rating: 'rx', genres: [{ id: '12', name: 'Hentai', kind: 'genre' }] };
const BERSERK = { id: '2', malId: '2', name: 'Berserk', kind: 'manga', status: 'ongoing', chapters: 0, volumes: 0, score: 9.46, airedOn: { year: 1989, month: 8, day: 25 }, genres: [{ id: '42', name: 'Seinen', kind: 'demographic' }] };

function stub(fn: (q: { query: string; variables: any }) => unknown) {
  globalThis.fetch = (async (_url: any, init: any) => {
    const body = JSON.parse(init.body);
    reqs.push({ ...body, headers: init.headers });
    return new Response(JSON.stringify({ data: fn(body) }), { status: 200 });
  }) as typeof fetch;
}
const list = (field: string, rows: unknown[]) => ({ [field]: rows });

beforeEach(() => { reqs = []; resetHostLimiter(); resetShikimoriCaches(); });
afterEach(() => { globalThis.fetch = realFetch; });

test('feed sends Shikimori args and UA, filters adult, reports next page', async () => {
  stub(() => list('animes', [FRIEREN, HENTAI]));
  const r = await p.feed!('ANIME', 'upcoming', { perPage: 2 });
  const q = reqs[0];
  expect(q.headers['User-Agent']).toBe('Tatakai-own (desktop)');
  expect(q.variables).toMatchObject({ order: 'popularity', status: 'anons', limit: 2, page: 1, censored: true });
  expect(q.query).toContain('$status: AnimeStatusString');
  expect(r.media.map((m) => m.malId)).toEqual([52991]);
  expect(r.hasNextPage).toBe(true); // raw page was full

  stub(() => list('mangas', [BERSERK]));
  const manga = await p.feed!('MANGA', 'popular', { countryOfOrigin: 'KR', perPage: 5 });
  expect(reqs[1].variables.kind).toBe('manhwa');
  expect(reqs[1].query).toContain('$kind: MangaKindString');
  expect(manga.hasNextPage).toBe(false);
});

test('seasonal uses season_year; genre names resolve through genres()', async () => {
  stub((b) => (b.query.includes('genres(entryType') ? { genres: [{ id: '1', name: 'Action', kind: 'genre' }, { id: '12', name: 'Hentai', kind: 'genre' }] } : list('animes', [FRIEREN])));
  await p.feed!('ANIME', 'seasonal', { season: 'FALL', seasonYear: 2026, genre: 'Action' });
  const q = reqs.find((r) => r.variables.season)!;
  expect(q.variables).toMatchObject({ season: 'fall_2026', genre: '1' });
  const unknown = await p.feed!('ANIME', 'popular', { genre: 'Nope' });
  expect(unknown.media).toEqual([]);
  expect(await p.genres!('ANIME')).toEqual(['Action']);
  expect(await p.genres!('ANIME', { includeAdult: true })).toEqual(['Action', 'Hentai']);
});

test('media maps detail: ids, titles, russian text dropped, relations, trailer', async () => {
  stub(() => list('animes', [FRIEREN]));
  const m = (await p.media!('ANIME', { malId: 52991 }))!;
  expect(reqs[0].variables.ids).toBe('52991');
  expect(m).toMatchObject({
    anilistId: 0, malId: 52991, titleRomaji: 'Sousou no Frieren', titleEnglish: "Frieren: Beyond Journey's End", titleNative: '葬送のフリーレン',
    format: 'TV', status: 'FINISHED', season: 'FALL', seasonYear: 2023, episodes: 28, duration: 24, averageScore: 93, rating: 'PG-13',
    source: 'MANGA', isAdult: false, source_api: 'shikimori', trailerUrl: 'https://youtu.be/abc',
    coverImageLarge: 'https://shikimori.io/o.jpeg', coverImageMedium: 'https://shikimori.io/m.webp',
  });
  expect(m.synonyms).toEqual(['Frieren at the Funeral']);
  expect(m.description).toBeUndefined();
  expect(m.genres).toEqual(['Shounen', 'Adventure', 'Award Winning']);
  expect(m.relations!.map((r) => r.relationType)).toEqual(['SEQUEL', 'SOURCE']);
  expect(m.startDate).toEqual({ year: 2023, month: 9, day: 29 });
});

test('malformed rows are skipped; relation kind set; ongoing total stays unknown', async () => {
  const ongoing = { ...FRIEREN, id: '7', malId: '7', status: 'ongoing', episodes: 0, episodesAired: 5, genres: null, poster: null, related: [null, FRIEREN.related[1]] };
  const broken = { id: '8', malId: '8', name: 'Bad', description: 5, genres: [null] };
  stub(() => list('animes', [null, broken, ongoing]));
  const r = await p.feed!('ANIME', 'airing', {});
  expect(r.media.map((m) => m.malId)).toEqual([7]);
  expect(r.media[0].episodes).toBeUndefined();
  stub(() => list('animes', [FRIEREN]));
  const m = (await p.media!('ANIME', { malId: 52991 }))!;
  expect(m.relations!.map((x) => x.kind)).toEqual(['ANIME', 'MANGA']);
});

test('manga media: zero chapters become undefined, no season; missing ids give null', async () => {
  stub(() => list('mangas', [BERSERK]));
  const m = (await p.media!('MANGA', { malId: 2 }))!;
  expect(m).toMatchObject({ format: 'MANGA', status: 'RELEASING', countryOfOrigin: 'JP', chapters: undefined, season: undefined, averageScore: 95 });
  expect(await p.media!('MANGA', {})).toBeNull();
  stub(() => list('mangas', []));
  expect(await p.media!('MANGA', { malId: 5 })).toBeNull();
});

test('search passes text as a variable and maps filters', async () => {
  stub(() => list('animes', [FRIEREN]));
  const r = await p.search!('ANIME', { query: 'frieren"} evil', format: ['TV', 'ONA'], status: ['FINISHED'], year: { min: 2023, max: 2023 }, rating: { min: 80 }, perPage: 10 });
  expect(reqs[0].variables).toMatchObject({ search: 'frieren"} evil', kind: 'tv,ona', status: 'released', season: '2023', score: 8 });
  expect(reqs[0].query).not.toContain('evil');
  expect(r.pageInfo).toMatchObject({ currentPage: 1, hasNextPage: false });
});

test('GraphQL errors throw so the router falls back; no episodes capability', async () => {
  globalThis.fetch = (async () => new Response(JSON.stringify({ errors: [{ message: 'boom' }] }), { status: 200 })) as typeof fetch;
  await expect(p.feed!('ANIME', 'popular', {})).rejects.toThrow('boom');
  expect(p.episodes).toBeUndefined();
});
