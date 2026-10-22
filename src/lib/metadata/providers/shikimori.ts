/**
 * Shikimori metadata provider. GraphQL at shikimori.io/api/graphql (MAL ids). 5 req/s, 90 req/min.
 *
 * Shikimori ids are MAL ids (`malId`). Caveats verified live:
 * - `description` and part of `synonyms` are Russian. Descriptions containing Cyrillic are
 *   dropped (the router/UI falls through to other sources for synopsis) and Cyrillic
 *   synonyms are filtered out.
 * - `censored: true` is ignored when an explicit `genre` is given, so adult items are
 *   also dropped client-side (rating rx / Hentai / Erotica genre).
 * - Query depth is limited to 5; `limit` is capped at 50 by the server.
 * - No per-episode data, so no `episodes()`.
 * - No total count: `hasNextPage` = "got a full page".
 */

import type { ContentSearchResult, MediaStatus, SearchFilters, TatakaiMedia } from '@/core/content/types';
import { publicJson } from '@/lib/net/publicApi';
import type { FeedOptions, FeedPage, MediaKind, MediaRef, MetadataFeedKind, MetadataProvider } from '@/lib/metadata/types';
import {
  ADULT_GENRES,
  cleanSynopsis,
  mapMalFormat,
  mapMalCountry,
  mapMalSource,
  seasonOfDate,
} from './mal-schema';

export const SHIKIMORI_GRAPHQL = 'https://shikimori.io/api/graphql';
const USER_AGENT = 'Tatakai-own (desktop)';
const MAX_LIMIT = 50;
const DEFAULT_LIMIT = 24;

// ── Raw shapes ───────────────────────────────────────────────────────────────

interface ShikiDate { year?: number | null; month?: number | null; day?: number | null }
interface ShikiPoster { originalUrl?: string | null; mainUrl?: string | null }
interface ShikiGenre { id?: string; name: string; kind?: string }
interface ShikiRelatedEntry {
  id: string;
  malId?: string | null;
  name: string;
  english?: string | null;
  kind?: string | null;
  status?: string | null;
  episodes?: number | null;
  poster?: ShikiPoster | null;
  airedOn?: ShikiDate | null;
}
export interface ShikiEntry {
  id: string;
  malId?: string | null;
  name: string;
  english?: string | null;
  japanese?: string | null;
  synonyms?: string[] | null;
  kind?: string | null;
  status?: string | null;
  episodes?: number | null;
  episodesAired?: number | null;
  chapters?: number | null;
  volumes?: number | null;
  duration?: number | null;
  rating?: string | null;
  score?: number | null;
  isCensored?: boolean | null;
  season?: string | null;
  origin?: string | null;
  airedOn?: ShikiDate | null;
  releasedOn?: ShikiDate | null;
  poster?: ShikiPoster | null;
  genres?: ShikiGenre[] | null;
  studios?: Array<{ id: string; name: string }> | null;
  description?: string | null;
  videos?: Array<{ url: string; kind?: string | null; name?: string | null }> | null;
  related?: Array<{ relationKind?: string | null; anime?: ShikiRelatedEntry | null; manga?: ShikiRelatedEntry | null }> | null;
}

// ── Queries ──────────────────────────────────────────────────────────────────

const CARD_FIELDS = `id malId name english japanese synonyms kind status score isCensored
  poster { originalUrl mainUrl } airedOn { year month day } releasedOn { year month day }
  genres { id name kind }`;
const ANIME_CARD = `${CARD_FIELDS} rating episodes episodesAired duration season`;
const MANGA_CARD = `${CARD_FIELDS} chapters volumes`;
const RELATED = `related { relationKind
  anime { id malId name english kind status episodes poster { mainUrl originalUrl } airedOn { year month day } }
  manga { id malId name kind status poster { mainUrl originalUrl } airedOn { year month day } } }`;
const ANIME_DETAIL = `${ANIME_CARD} origin description studios { id name } videos { url kind name } ${RELATED}`;
const MANGA_DETAIL = `${MANGA_CARD} description ${RELATED}`;

const fieldName = (kind: MediaKind) => (kind === 'ANIME' ? 'animes' : 'mangas');

async function gql<T>(query: string, variables: Record<string, unknown> | undefined, signal?: AbortSignal): Promise<T> {
  const res = await publicJson<{ data?: T; errors?: Array<{ message: string }> }>(SHIKIMORI_GRAPHQL, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json', 'User-Agent': USER_AGENT },
    body: JSON.stringify({ query, variables }),
    signal,
  });
  if (res.errors?.length && !res.data) throw new Error(`Shikimori GraphQL: ${res.errors[0].message}`);
  return res.data as T;
}

// ── Genres (English name → Shikimori id), cached ─────────────────────────────

interface GenreRow { id: string; name: string; kind?: string }
const genreCache = new Map<MediaKind, Promise<GenreRow[]>>();

function loadGenres(kind: MediaKind, signal?: AbortSignal): Promise<GenreRow[]> {
  let p = genreCache.get(kind);
  if (!p) {
    p = gql<{ genres?: GenreRow[] }>(
      `query($t: GenreEntryTypeEnum!) { genres(entryType: $t) { id name kind } }`,
      { t: kind === 'ANIME' ? 'Anime' : 'Manga' },
      signal,
    ).then((r) => r.genres ?? []);
    genreCache.set(kind, p);
    p.catch(() => genreCache.delete(kind));
  }
  return p;
}

/** Forget cached genre lists (tests). */
export function resetShikimoriCaches(): void {
  genreCache.clear();
}

const GENRE_ALIASES: Record<string, string> = { thriller: 'suspense' };

async function genreIds(kind: MediaKind, names: string[] | undefined, signal?: AbortSignal): Promise<string | undefined> {
  if (!names?.length) return undefined;
  const rows = await loadGenres(kind, signal);
  const byName = new Map(rows.map((g) => [g.name.toLowerCase(), g.id]));
  const ids = names.map((n) => byName.get(GENRE_ALIASES[n.toLowerCase()] ?? n.toLowerCase())).filter((x): x is string => Boolean(x));
  return ids.length ? ids.join(',') : undefined;
}

// ── Mapping ──────────────────────────────────────────────────────────────────

const STATUS: Record<string, MediaStatus> = {
  anons: 'NOT_YET_RELEASED',
  ongoing: 'RELEASING',
  released: 'FINISHED',
  paused: 'HIATUS',
  discontinued: 'CANCELLED',
};

const RATING: Record<string, string> = { g: 'G', pg: 'PG', pg_13: 'PG-13', r: 'R', r_plus: 'R+', rx: 'Rx' };

const RELATION: Record<string, string> = {
  sequel: 'SEQUEL', prequel: 'PREQUEL', side_story: 'SIDE_STORY', spin_off: 'SPIN_OFF', summary: 'SUMMARY',
  parent_story: 'PARENT', full_story: 'PARENT', alternative_version: 'ALTERNATIVE', alternative_setting: 'ALTERNATIVE',
  character: 'CHARACTER', adaptation: 'ADAPTATION', other: 'OTHER',
};

const CYRILLIC = /[Ѐ-ӿ]/;
const opt = <T>(v: T | null | undefined): T | undefined => (v == null || v === '' ? undefined : v);
const num = (v: string | null | undefined): number | undefined => {
  const n = Number(v);
  return Number.isFinite(n) && n > 0 ? n : undefined;
};

function fuzzy(d: ShikiDate | null | undefined): TatakaiMedia['startDate'] {
  if (!d) return undefined;
  const out: NonNullable<TatakaiMedia['startDate']> = {};
  if (d.year != null) out.year = d.year;
  if (d.month != null) out.month = d.month;
  if (d.day != null) out.day = d.day;
  return Object.keys(out).length ? out : undefined;
}

const poster = (p: ShikiPoster | null | undefined) => ({
  large: opt(p?.originalUrl || p?.mainUrl),
  medium: opt(p?.mainUrl || p?.originalUrl),
});

function youtubeUrl(videos: ShikiEntry['videos']): string | undefined {
  const v = videos?.find((x) => x.kind === 'pv' && /youtu/.test(x.url)) ?? videos?.find((x) => x.kind === 'cm' && /youtu/.test(x.url));
  return v?.url;
}

export function mapShikiEntry(e: ShikiEntry, kind: MediaKind): TatakaiMedia {
  const isManga = kind === 'MANGA';
  const genres = [...new Set((e.genres ?? []).map((g) => g.name))];
  const cover = poster(e.poster);
  const score = typeof e.score === 'number' && e.score > 0 ? e.score : undefined;
  const rating = e.rating ? RATING[e.rating] : undefined;
  const durationMin = !isManga && e.duration ? e.duration : undefined;
  const aired = e.airedOn?.year != null ? e.airedOn : undefined;
  const seasonMatch = !isManga && e.season ? /^(winter|spring|summer|fall)_(\d{4})$/.exec(e.season) : null;
  const seasonUpper = seasonMatch ? (seasonMatch[1].toUpperCase() as NonNullable<TatakaiMedia['season']>) : undefined;
  const derived = !isManga && !seasonMatch && aired?.year && aired.month ? seasonOfDate(new Date(Date.UTC(aired.year, aired.month - 1, 1))) : undefined;
  const adult = e.rating === 'rx' || genres.some((g) => ADULT_GENRES.has(g.toLowerCase()));
  const description = e.description && !CYRILLIC.test(e.description) ? cleanSynopsis(e.description.replace(/\[\/?[a-z_]+(?:=[^\]]*)?\]/gi, '')) : undefined;

  const relations = e.related
    ?.map((r) => {
      const target = r.anime ?? r.manga;
      if (!target) return null;
      const entryKind: MediaKind = r.anime ? 'ANIME' : 'MANGA';
      let relationType = RELATION[r.relationKind || 'other'] ?? 'OTHER';
      if (relationType === 'ADAPTATION' && kind === 'ANIME' && entryKind === 'MANGA') relationType = 'SOURCE';
      return {
        id: num(target.malId) ?? Number(target.id),
        relationType,
        titleRomaji: target.name,
        titleEnglish: opt(target.english),
        coverImage: poster(target.poster).large,
        format: mapMalFormat(entryKind, target.kind) as TatakaiMedia['format'],
        status: target.status ? STATUS[target.status] : undefined,
        startDate: fuzzy(target.airedOn),
        seasonYear: opt(target.airedOn?.year),
        episodes: opt(target.episodes) || undefined,
      };
    })
    .filter((r): r is NonNullable<typeof r> => r !== null);

  return {
    anilistId: 0,
    malId: num(e.malId) ?? num(e.id),
    titleRomaji: e.name || e.english || e.japanese || '',
    titleEnglish: opt(e.english),
    titleNative: opt(e.japanese),
    synonyms: (e.synonyms ?? []).filter((s) => s && !CYRILLIC.test(s)),
    coverImageLarge: cover.large,
    coverImageMedium: cover.medium,
    bannerImage: undefined,
    format: mapMalFormat(kind, e.kind === 'tv_special' ? 'tvspecial' : e.kind, durationMin),
    status: e.status ? STATUS[e.status] : undefined,
    season: seasonUpper ?? derived?.season,
    seasonYear: seasonMatch ? Number(seasonMatch[2]) : derived?.year,
    episodes: isManga ? undefined : (e.episodes || e.episodesAired || undefined),
    chapters: isManga ? e.chapters || undefined : undefined,
    volumes: isManga ? e.volumes || undefined : undefined,
    duration: durationMin,
    startDate: fuzzy(e.airedOn),
    endDate: fuzzy(e.releasedOn),
    description,
    averageScore: score != null ? Math.round(score * 10) : undefined,
    meanScore: score != null ? Math.round(score * 10) : undefined,
    rating: isManga ? undefined : rating,
    genres,
    tags: (e.genres ?? [])
      .filter((g) => g.kind === 'theme' || g.kind === 'demographic')
      .map((g) => ({ id: Number(g.id) || 0, name: g.name, category: g.kind === 'theme' ? 'Theme' : 'Demographic' })),
    source: isManga ? undefined : mapMalSource(e.origin?.replace(/_/g, ' ')),
    isAdult: adult,
    countryOfOrigin: isManga ? mapMalCountry(e.kind === 'light_novel' ? 'lightnovel' : e.kind === 'one_shot' ? 'oneshot' : e.kind) : undefined,
    studios: (e.studios ?? []).map((s) => ({ id: Number(s.id) || 0, name: s.name, isMain: true })),
    relations,
    trailerUrl: youtubeUrl(e.videos),
    source_api: 'shikimori',
    fetchedAt: Date.now(),
  };
}

function mapList(list: ShikiEntry[] | undefined, kind: MediaKind, adult: boolean): TatakaiMedia[] {
  const out: TatakaiMedia[] = [];
  const seen = new Set<string>();
  for (const e of list ?? []) {
    if (!e || seen.has(e.id)) continue;
    seen.add(e.id);
    const m = mapShikiEntry(e, kind);
    if (!m.malId) continue;
    if (!adult && m.isAdult) continue;
    out.push(m);
  }
  return out;
}

// ── Argument builders ────────────────────────────────────────────────────────

interface QueryArgs {
  order?: string;
  status?: string;
  season?: string;
  kind?: string;
  genre?: string;
  search?: string;
  score?: number;
}

const limitOf = (n: number | undefined) => Math.max(1, Math.min(MAX_LIMIT, Math.floor(n || DEFAULT_LIMIT)));

interface ListResult { media: TatakaiMedia[]; rawCount: number }

async function runList(kind: MediaKind, args: QueryArgs, page: number, limit: number, adult: boolean, signal?: AbortSignal): Promise<ListResult> {
  const f = fieldName(kind);
  const A = kind === 'ANIME';
  const sel = kind === 'ANIME' ? ANIME_CARD : MANGA_CARD;
  // GraphQL variables keep user-supplied search text out of the query string.
  const data = await gql<Record<string, ShikiEntry[]>>(
    `query($page: Int, $limit: Int, $order: OrderEnum, $status: ${A ? 'AnimeStatusString' : 'MangaStatusString'}, $season: SeasonString, $kind: ${A ? 'AnimeKindString' : 'MangaKindString'}, $genre: String, $search: String, $score: Int, $censored: Boolean) {
      ${f}(page: $page, limit: $limit, order: $order, status: $status, season: $season, kind: $kind, genre: $genre, search: $search, score: $score, censored: $censored) { ${sel} }
    }`,
    { page, limit, censored: !adult, ...args },
    signal,
  );
  return { media: mapList(data?.[f], kind, adult), rawCount: data?.[f]?.length ?? 0 };
}

function feedArgs(kind: MediaKind, feed: MetadataFeedKind, opts: FeedOptions): QueryArgs {
  const now = seasonOfDate(new Date());
  const season = (opts.season ?? now.season).toLowerCase();
  const year = opts.seasonYear ?? now.year;
  const base: QueryArgs = {};
  if (kind === 'MANGA') {
    if (opts.novelsOnly) base.kind = 'light_novel,novel';
    else if (opts.countryOfOrigin) base.kind = { JP: 'manga,one_shot', KR: 'manhwa', CN: 'manhua', TW: 'manhua' }[opts.countryOfOrigin];
  }
  switch (feed) {
    case 'trending': return { ...base, order: 'ranked_shiki', status: 'ongoing' };
    case 'popular': return { ...base, order: 'popularity' };
    case 'top_rated': return { ...base, order: 'ranked' };
    case 'airing': return { ...base, order: 'popularity', status: 'ongoing' };
    case 'upcoming': return { ...base, order: 'popularity', status: 'anons' };
    // Manga has no seasons: show what is publishing.
    case 'seasonal': return kind === 'ANIME' ? { ...base, order: 'popularity', season: `${season}_${year}` } : { ...base, order: 'popularity', status: 'ongoing' };
    case 'recently_completed': return { ...base, order: 'aired_on', status: 'released' };
  }
}

async function feed(kind: MediaKind, feedKind: MetadataFeedKind, opts: FeedOptions): Promise<FeedPage> {
  const page = opts.page ?? 1;
  const limit = limitOf(opts.perPage);
  const args = feedArgs(kind, feedKind, opts);
  if (opts.genre) {
    const genre = await genreIds(kind, [opts.genre], opts.signal);
    // Unknown genre for this vocabulary: nothing to show rather than an unfiltered list.
    if (!genre) return { media: [], page, hasNextPage: false };
    args.genre = genre;
  }
  const adult = opts.isAdult === true;
  const { media, rawCount } = await runList(kind, args, page, limit, adult, opts.signal);
  return { media, page, hasNextPage: rawCount >= limit };
}

const SORT_ORDER: Record<NonNullable<SearchFilters['sortBy']>, string> = {
  TRENDING_DESC: 'popularity',
  POPULARITY_DESC: 'popularity',
  SCORE_DESC: 'ranked',
  START_DATE_DESC: 'aired_on',
  TITLE_ROMAJI: 'name',
  FAVOURITES_DESC: 'popularity',
  UPDATED_AT_DESC: 'updated_at_desc',
  CHAPTERS_DESC: 'popularity',
  END_DATE_DESC: 'aired_on',
};

const FORMAT_KIND: Record<string, string> = {
  TV: 'tv', TV_SHORT: 'tv', MOVIE: 'movie', OVA: 'ova', ONA: 'ona', SPECIAL: 'special,tv_special', MUSIC: 'music',
  MANGA: 'manga', ONE_SHOT: 'one_shot', NOVEL: 'light_novel,novel',
};
const STATUS_ARG: Record<MediaKind, Partial<Record<MediaStatus, string>>> = {
  ANIME: { FINISHED: 'released', RELEASING: 'ongoing', NOT_YET_RELEASED: 'anons' },
  MANGA: { FINISHED: 'released', RELEASING: 'ongoing', NOT_YET_RELEASED: 'anons', HIATUS: 'paused', CANCELLED: 'discontinued' },
};

async function search(
  kind: MediaKind,
  f: SearchFilters & { novelsOnly?: boolean; countryOfOrigin?: string; signal?: AbortSignal },
): Promise<ContentSearchResult> {
  const page = f.page ?? 1;
  const limit = limitOf(f.perPage);
  const adult = f.isAdult === true;
  const args: QueryArgs = {};
  const q = f.query?.trim();
  if (q) args.search = q;
  const genre = await genreIds(kind, f.genres, f.signal);
  if (f.genres?.length && !genre) return { media: [], pageInfo: { total: 0, currentPage: page, lastPage: page, hasNextPage: false, perPage: limit } };
  if (genre) args.genre = genre;

  const kinds = new Set<string>();
  for (const fmt of f.format ?? []) FORMAT_KIND[fmt]?.split(',').forEach((k) => kinds.add(k));
  if (kind === 'MANGA') {
    if (f.novelsOnly) { kinds.clear(); kinds.add('light_novel'); kinds.add('novel'); }
    else if (f.countryOfOrigin) {
      kinds.clear();
      ({ JP: ['manga', 'one_shot'], KR: ['manhwa'], CN: ['manhua'], TW: ['manhua'] } as Record<string, string[]>)[f.countryOfOrigin]?.forEach((k) => kinds.add(k));
    }
  }
  if (kinds.size) args.kind = [...kinds].join(',');

  const statuses = [...new Set((f.status ?? []).map((s) => STATUS_ARG[kind][s]).filter((s): s is string => Boolean(s)))];
  if (statuses.length) args.status = statuses.join(',');
  if (f.rating?.min) args.score = Math.round(f.rating.min / 10);
  // Shikimori `season`: "summer_2017", "2017", or a range "2014_2016".
  const { min: y0, max: y1 } = f.year ?? {};
  if (y0 && y0 === y1) args.season = f.season?.length === 1 ? `${f.season[0].toLowerCase()}_${y0}` : String(y0);
  else if (y0 || y1) args.season = `${y0 ?? 1960}_${y1 ?? new Date().getUTCFullYear() + 1}`;
  if (f.sortBy) args.order = SORT_ORDER[f.sortBy];
  else if (!q) args.order = 'popularity';

  const { media, rawCount } = await runList(kind, args, page, limit, adult, f.signal);
  const hasNextPage = rawCount >= limit;
  return {
    media,
    pageInfo: { total: (page - 1) * limit + media.length + (hasNextPage ? 1 : 0), currentPage: page, lastPage: hasNextPage ? page + 1 : page, hasNextPage, perPage: limit },
  };
}

async function media(kind: MediaKind, ref: MediaRef, opts: { signal?: AbortSignal } = {}): Promise<TatakaiMedia | null> {
  if (!ref.malId) return null;
  const f = fieldName(kind);
  const data = await gql<Record<string, ShikiEntry[]>>(
    `query($ids: String) { ${f}(ids: $ids, limit: 1) { ${kind === 'ANIME' ? ANIME_DETAIL : MANGA_DETAIL} } }`,
    { ids: String(ref.malId) },
    opts.signal,
  );
  const row = data?.[f]?.[0];
  return row ? mapShikiEntry(row, kind) : null;
}

async function genres(kind: MediaKind, opts: { includeAdult?: boolean; signal?: AbortSignal } = {}): Promise<string[]> {
  const rows = await loadGenres(kind, opts.signal);
  return [...new Set(rows.map((g) => g.name))].filter((n) => opts.includeAdult || !ADULT_GENRES.has(n.toLowerCase()));
}

export const shikimoriProvider: MetadataProvider = {
  id: 'shikimori',
  label: 'Shikimori',
  description: 'MyAnimeList-based GraphQL. Free, no key.',
  kinds: ['ANIME', 'MANGA'],
  isConfigured: () => true,
  feed,
  search,
  media,
  genres,
};
