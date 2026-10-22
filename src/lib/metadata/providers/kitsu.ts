/**
 * Kitsu metadata provider. JSON:API at kitsu.io/api/edge; includes AniList/MAL id mappings.
 *
 * Implements the subset of MetadataProvider (see ../types.ts) this source can
 * serve: feed, search, media, episodes (anime), genres and ids (anime + manga —
 * this is the router's manga id fallback).
 *
 * Verified live (see tests/kitsu-provider.test.ts fixtures):
 *  - `page[limit]` max is 20; trending ignores `include` (so trending ids are
 *    re-fetched through `filter[id]` to get the AniList mappings).
 *  - `sort=-endDate` combined with a status filter 500s on Kitsu, so
 *    "recently completed" uses `filter[status]=finished&sort=-startDate`.
 *  - `filter[nsfw]` / `filter[ageRating]` are not allowed (manga) or drop
 *    un-rated titles (anime), so adult content is dropped client-side.
 *  - `filter[genres]=A,B` is AND; names are AniList-compatible for the common set.
 *  - `filter[text]` combines with any `sort` and `filter[*]`.
 */

import type { ContentSearchResult, MediaFormat, MediaSeason, MediaStatus, SearchFilters, TatakaiMedia } from '@/core/content/types';
import { loadMapping } from '@/lib/mapping/client';
import {
  KITSU_PAGE_MAX,
  externalIdsOf,
  isKitsuMiss,
  kitsuGet,
  kitsuRelated,
  kitsuRows,
  lookupKitsuByExternal,
  type KitsuDoc,
  type KitsuKind,
  type KitsuParams,
  type KitsuResource,
} from '@/lib/mapping/kitsuApi';
import { rememberIds } from '@/lib/metadata/ids';
import type {
  FeedOptions,
  FeedPage,
  MediaKind,
  MediaRef,
  MetadataEpisode,
  MetadataFeedKind,
  MetadataProvider,
} from '@/lib/metadata/types';

const kindPath = (kind: MediaKind): KitsuKind => (kind === 'ANIME' ? 'anime' : 'manga');

/** Sparse fieldsets for included resources keep list payloads small. */
const LIST_INCLUDE = 'mappings,genres';
const SPARSE: KitsuParams = {
  'fields[mappings]': 'externalSite,externalId',
  'fields[genres]': 'name',
};
const DETAIL_SPARSE: KitsuParams = { ...SPARSE, 'fields[categories]': 'title', 'fields[producers]': 'name', 'fields[animeProductions]': 'role,producer' };

// ── Mapping ───────────────────────────────────────────────────────────────────

const ANIME_FORMATS: Record<string, MediaFormat> = {
  TV: 'TV', movie: 'MOVIE', OVA: 'OVA', ONA: 'ONA', special: 'SPECIAL', music: 'MUSIC',
};

function kitsuStatus(status: unknown): MediaStatus | undefined {
  switch (status) {
    case 'current': return 'RELEASING';
    case 'finished': return 'FINISHED';
    case 'tba': case 'unreleased': case 'upcoming': return 'NOT_YET_RELEASED';
    default: return undefined;
  }
}

function fuzzyDate(value: unknown): TatakaiMedia['startDate'] {
  const m = typeof value === 'string' ? /^(\d{4})(?:-(\d{2}))?(?:-(\d{2}))?/.exec(value) : null;
  if (!m) return undefined;
  const out: Partial<{ year: number; month: number; day: number }> = { year: Number(m[1]) };
  if (m[2]) out.month = Number(m[2]);
  if (m[3]) out.day = Number(m[3]);
  return out;
}

function seasonOfMonth(month: number): MediaSeason {
  return month <= 3 ? 'WINTER' : month <= 6 ? 'SPRING' : month <= 9 ? 'SUMMER' : 'FALL';
}

/** Kitsu synopses end with "(Source: ...)" / "[Written by MAL Rewrite]" credits. */
function cleanSynopsis(text: unknown): string | undefined {
  if (typeof text !== 'string') return undefined;
  let out = text.trim();
  for (let i = 0; i < 2; i++) out = out.replace(/\s*[([]\s*(?:Source|Written by)[^)\]]*[)\]]\s*$/i, '').trim();
  return out || undefined;
}

const str = (v: unknown): string | undefined => (typeof v === 'string' && v.trim() ? v.trim() : undefined);
const optNum = (v: unknown): number | undefined => {
  const n = v == null || v === '' ? NaN : Number(v);
  return Number.isFinite(n) ? n : undefined;
};

/** Map one Kitsu anime/manga resource (plus its included relations) to TatakaiMedia. */
export function mapKitsuMedia(doc: KitsuDoc, res: KitsuResource): TatakaiMedia {
  const kind = res.type === 'manga' ? 'manga' : 'anime';
  const a = res.attributes;
  const ext = externalIdsOf(kitsuRelated(doc, res, 'mappings'), kind);
  const titles = (a.titles ?? {}) as Record<string, string>;
  const romaji = str(titles.en_jp) ?? str(a.canonicalTitle) ?? str(titles.en) ?? '';
  const english = str(titles.en) ?? str(titles.en_us);
  const native = str(titles.ja_jp);
  const main = new Set([romaji, english, native].filter(Boolean));
  const synonyms = [...new Set(((a.abbreviatedTitles ?? []) as unknown[]).map(str).filter((t): t is string => Boolean(t) && !main.has(t)))];

  const genres = kitsuRelated(doc, res, 'genres').map((g) => str(g.attributes.name)).filter((n): n is string => Boolean(n));
  const tags = kitsuRelated(doc, res, 'categories').flatMap((c) => {
    const name = str(c.attributes.title);
    return name ? [{ id: Number(c.id), name, isAdult: false }] : [];
  });

  const studios = (() => {
    const out: TatakaiMedia['studios'] = [];
    const seen = new Set<string>();
    for (const prod of kitsuRelated(doc, res, 'animeProductions')) {
      if (prod.attributes.role !== 'studio') continue;
      const ref = prod.relationships?.producer?.data;
      const producer = ref && !Array.isArray(ref) ? doc.included?.find((r) => r.type === 'producers' && r.id === ref.id) : undefined;
      const name = str(producer?.attributes.name);
      if (!producer || !name || seen.has(producer.id)) continue;
      seen.add(producer.id);
      out.push({ id: Number(producer.id), name, isMain: true });
    }
    return out;
  })();

  const subtype = String(a.subtype ?? '');
  const isManga = kind === 'manga';
  const startDate = fuzzyDate(a.startDate);
  const isTv = !isManga && (subtype === 'TV' || subtype === 'ONA');
  const ageRating = str(a.ageRating);
  const score = optNum(a.averageRating);
  const poster = (a.posterImage ?? {}) as Record<string, string | undefined>;
  const cover = (a.coverImage ?? {}) as Record<string, string | undefined>;
  const youtube = str(a.youtubeVideoId);

  return {
    anilistId: ext.anilistId ?? 0,
    malId: ext.malId,
    kitsuId: Number(res.id),
    titleRomaji: romaji,
    titleEnglish: english,
    titleNative: native,
    synonyms,
    coverImageLarge: str(poster.original) ?? str(poster.large) ?? str(poster.medium),
    coverImageMedium: str(poster.medium) ?? str(poster.small) ?? str(poster.large),
    bannerImage: str(cover.original) ?? str(cover.large),
    format: isManga
      ? ((subtype === 'novel' ? 'NOVEL' : subtype === 'oneshot' ? 'ONE_SHOT' : 'MANGA') as MediaFormat)
      : ANIME_FORMATS[subtype],
    status: kitsuStatus(a.status),
    season: isTv && startDate?.month ? seasonOfMonth(startDate.month) : undefined,
    seasonYear: isTv ? startDate?.year : undefined,
    episodes: optNum(a.episodeCount),
    chapters: optNum(a.chapterCount),
    volumes: optNum(a.volumeCount),
    duration: optNum(a.episodeLength),
    startDate,
    endDate: fuzzyDate(a.endDate),
    description: cleanSynopsis(a.synopsis ?? a.description),
    averageScore: score != null ? Math.round(score) : undefined,
    meanScore: score != null ? Math.round(score) : undefined,
    popularity: optNum(a.userCount),
    favourites: optNum(a.favoritesCount),
    rating: ageRating,
    genres,
    tags,
    isAdult: a.nsfw === true || ageRating === 'R18' || genres.includes('Hentai'),
    countryOfOrigin: isManga ? (subtype === 'manhwa' ? 'KR' : subtype === 'manhua' ? 'CN' : 'JP') : 'JP',
    studios,
    trailerUrl: youtube ? `https://www.youtube.com/watch?v=${youtube}` : undefined,
    source_api: 'kitsu',
    fetchedAt: Date.now(),
  };
}

function mapDoc(kind: MediaKind, doc: KitsuDoc, opts: { isAdult?: boolean } = {}): TatakaiMedia[] {
  const items = kitsuRows(doc).map((r) => mapKitsuMedia(doc, r));
  for (const m of items) {
    if (m.anilistId > 0) rememberIds(kind, { anilistId: m.anilistId, malId: m.malId, kitsuId: m.kitsuId });
  }
  return opts.isAdult === false ? items.filter((m) => !m.isAdult) : items;
}

// ── Query building ────────────────────────────────────────────────────────────

const lower = (s: string) => s.toLowerCase();

function currentSeason(now = new Date()): { season: MediaSeason; year: number } {
  return { season: seasonOfMonth(now.getMonth() + 1), year: now.getFullYear() };
}

/** `subtype` filter for the manga-only options (country of origin / novels). */
function mangaSubtype(opts: { countryOfOrigin?: string; novelsOnly?: boolean }): string | undefined {
  if (opts.novelsOnly) return 'novel';
  switch (opts.countryOfOrigin) {
    case 'KR': return 'manhwa';
    case 'CN': case 'TW': return 'manhua';
    case 'JP': return 'manga,oneshot,doujin';
    default: return undefined;
  }
}

const SORT_BY: Partial<Record<NonNullable<SearchFilters['sortBy']>, string>> = {
  FAVOURITES_DESC: '-favoritesCount',
  UPDATED_AT_DESC: '-updatedAt',
  CHAPTERS_DESC: '-chapterCount',
  TRENDING_DESC: '-userCount',
  POPULARITY_DESC: '-userCount',
  SCORE_DESC: '-averageRating',
  START_DATE_DESC: '-startDate',
  TITLE_ROMAJI: 'slug',
};

const FORMAT_TO_SUBTYPE: Partial<Record<MediaFormat, string>> = {
  TV: 'TV', TV_SHORT: 'TV', MOVIE: 'movie', OVA: 'OVA', ONA: 'ONA', SPECIAL: 'special', MUSIC: 'music',
};
const STATUS_TO_KITSU: Record<MediaStatus, string | undefined> = {
  RELEASING: 'current', FINISHED: 'finished', NOT_YET_RELEASED: 'upcoming,unreleased,tba', CANCELLED: undefined, HIATUS: undefined,
};

function range(r?: { min?: number; max?: number }): string | undefined {
  if (!r || (r.min == null && r.max == null)) return undefined;
  return `${r.min ?? ''}..${r.max ?? ''}`;
}

function pageOf(opts: { page?: number; perPage?: number }): { page: number; perPage: number; offset: number } {
  const page = Math.max(1, Math.floor(opts.page ?? 1));
  const perPage = Math.min(KITSU_PAGE_MAX, Math.max(1, Math.floor(opts.perPage ?? KITSU_PAGE_MAX)));
  return { page, perPage, offset: (page - 1) * perPage };
}

// ── Provider ──────────────────────────────────────────────────────────────────

async function resolveKitsuId(kind: MediaKind, ref: MediaRef, signal?: AbortSignal): Promise<number | null> {
  if (ref.kitsuId) return ref.kitsuId;
  const k = kindPath(kind);
  if (ref.anilistId) {
    const hit = await lookupKitsuByExternal(k, 'anilist', ref.anilistId, signal);
    if (hit) return hit.kitsuId;
  }
  if (ref.malId) {
    const hit = await lookupKitsuByExternal(k, 'myanimelist', ref.malId, signal);
    if (hit) return hit.kitsuId;
  }
  return null;
}

async function feed(kind: MediaKind, feedKind: MetadataFeedKind, opts: FeedOptions): Promise<FeedPage> {
  const path = kindPath(kind);
  const { page, perPage, offset } = pageOf(opts);
  const subtype = kind === 'MANGA' ? mangaSubtype(opts) : undefined;
  const filtered = Boolean(opts.genre || subtype);

  // Kitsu's trending lane has no paging/filters/includes: first page only, then
  // fall back to the popularity lane (also used when filters are set).
  if (feedKind === 'trending' && page === 1 && !filtered) {
    const trending = await kitsuGet(`/trending/${path}`, { limit: perPage }, opts.signal);
    const ids = kitsuRows(trending).map((r) => r.id);
    if (!ids.length) return { media: [], page, hasNextPage: false };
    const doc = await kitsuGet(`/${path}`, {
      'filter[id]': ids.join(','), include: LIST_INCLUDE, 'page[limit]': KITSU_PAGE_MAX, ...SPARSE,
    }, opts.signal);
    const order = new Map(ids.map((id, i) => [id, i]));
    const rows = kitsuRows(doc).sort((a, b) => (order.get(a.id) ?? 0) - (order.get(b.id) ?? 0));
    const media = mapDoc(kind, { ...doc, data: rows }, opts);
    return { media, page, hasNextPage: true };
  }

  const params: KitsuParams = {
    include: LIST_INCLUDE, 'page[limit]': perPage, 'page[offset]': offset, 'filter[genres]': opts.genre, 'filter[subtype]': subtype, ...SPARSE,
  };
  switch (feedKind) {
    case 'trending': case 'popular':
      params.sort = '-userCount'; break;
    case 'top_rated':
      params.sort = '-averageRating'; break;
    case 'airing':
      params['filter[status]'] = 'current'; params.sort = '-userCount'; break;
    case 'upcoming':
      params['filter[status]'] = 'upcoming,unreleased'; params.sort = '-userCount'; break;
    case 'seasonal': {
      const cur = currentSeason();
      if (kind === 'ANIME') {
        params['filter[season]'] = lower(opts.season ?? cur.season);
        params['filter[seasonYear]'] = opts.seasonYear ?? cur.year;
      } else {
        params['filter[status]'] = 'current';
      }
      params.sort = '-userCount';
      break;
    }
    case 'recently_completed':
      // `-endDate` + a status filter 500s on Kitsu; `-startDate` is the stable proxy.
      params['filter[status]'] = 'finished'; params.sort = '-startDate'; break;
  }
  const doc = await kitsuGet(`/${path}`, params, opts.signal);
  return { media: mapDoc(kind, doc, opts), page, hasNextPage: Boolean(doc.links?.next) };
}

async function search(
  kind: MediaKind,
  filters: SearchFilters & { novelsOnly?: boolean; countryOfOrigin?: string; signal?: AbortSignal },
): Promise<ContentSearchResult> {
  const path = kindPath(kind);
  const { page, perPage, offset } = pageOf(filters);
  const query = filters.query?.trim();
  const params: KitsuParams = {
    include: LIST_INCLUDE, 'page[limit]': perPage, 'page[offset]': offset, ...SPARSE,
    'filter[text]': query || undefined,
    'filter[genres]': filters.genres?.length ? filters.genres.join(',') : undefined,
    'filter[year]': range(filters.year),
    'filter[averageRating]': range(filters.rating),
  };
  const statuses = [...new Set((filters.status ?? []).flatMap((s) => (STATUS_TO_KITSU[s] ?? '').split(',')).filter(Boolean))];
  if (statuses.length) params['filter[status]'] = statuses.join(',');

  if (kind === 'ANIME') {
    const subtypes = [...new Set((filters.format ?? []).map((f) => FORMAT_TO_SUBTYPE[f]).filter(Boolean))];
    if (subtypes.length) params['filter[subtype]'] = subtypes.join(',');
    if (filters.season?.length) params['filter[season]'] = filters.season.map(lower).join(',');
    params['filter[episodeCount]'] = range(filters.episodes);
  } else {
    params['filter[subtype]'] = mangaSubtype(filters);
  }

  const sort = filters.sortBy ? SORT_BY[filters.sortBy] : undefined;
  // Text search keeps Kitsu's relevance order unless the caller asks for a sort.
  params.sort = sort ?? (query ? undefined : '-userCount');

  const doc = await kitsuGet(`/${path}`, params, filters.signal);
  const total = doc.meta?.count ?? 0;
  const hasNextPage = Boolean(doc.links?.next);
  return {
    media: mapDoc(kind, doc, filters),
    pageInfo: { total, currentPage: page, lastPage: Math.max(page, Math.ceil(total / perPage)), hasNextPage, perPage },
  };
}

async function media(kind: MediaKind, ref: MediaRef, opts: { signal?: AbortSignal } = {}): Promise<TatakaiMedia | null> {
  const kitsuId = await resolveKitsuId(kind, ref, opts.signal);
  if (!kitsuId) return null;
  const path = kindPath(kind);
  let doc: KitsuDoc;
  try {
    doc = await kitsuGet(`/${path}/${kitsuId}`, {
      include: kind === 'ANIME' ? 'mappings,genres,categories,animeProductions.producer' : 'mappings,genres,categories',
      ...DETAIL_SPARSE,
    }, opts.signal);
  } catch (err) {
    if (isKitsuMiss(err)) return null;
    throw err;
  }
  const res = kitsuRows(doc)[0];
  if (!res) return null;
  const mapped = mapKitsuMedia(doc, res);
  if (!mapped.anilistId && ref.anilistId) mapped.anilistId = ref.anilistId;
  if (!mapped.malId && ref.malId) mapped.malId = ref.malId;
  if (mapped.anilistId > 0) rememberIds(kind, { anilistId: mapped.anilistId, malId: mapped.malId, kitsuId: mapped.kitsuId });
  return mapped;
}

/** Map one Kitsu `episodes` resource. Pure. */
export function mapKitsuEpisode(row: KitsuResource): MetadataEpisode | null {
  const a = row.attributes;
  const number = optNum(a.number);
  if (number == null || number < 0) return null;
  const rawTitles = (a.titles ?? {}) as Record<string, string>;
  const titles: Record<string, string> = {};
  if (str(rawTitles.en) ?? str(rawTitles.en_us)) titles.en = (str(rawTitles.en) ?? str(rawTitles.en_us))!;
  if (str(rawTitles.en_jp)) titles['x-jat'] = str(rawTitles.en_jp)!;
  if (str(rawTitles.ja_jp)) titles.ja = str(rawTitles.ja_jp)!;
  const thumb = (a.thumbnail ?? {}) as Record<string, string | undefined>;
  return {
    number,
    title: str(a.canonicalTitle) ?? titles.en ?? titles['x-jat'] ?? titles.ja,
    titles: Object.keys(titles).length ? titles : undefined,
    overview: cleanSynopsis(a.synopsis ?? a.description),
    image: str(thumb.original) ?? str(thumb.large) ?? str(thumb.medium),
    airDate: str(a.airdate),
    runtimeMin: optNum(a.length),
  };
}

const MAX_EPISODES = 2000;
const EPISODE_CONCURRENCY = 4;

async function episodes(ref: MediaRef, opts: { signal?: AbortSignal } = {}): Promise<MetadataEpisode[]> {
  const kitsuId = await resolveKitsuId('ANIME', ref, opts.signal);
  if (!kitsuId) return [];
  const rows: KitsuResource[] = [];
  let offset = 0;
  let ended = false;
  while (!ended && offset < MAX_EPISODES) {
    const offsets = Array.from({ length: EPISODE_CONCURRENCY }, (_, i) => offset + i * KITSU_PAGE_MAX).filter((o) => o < MAX_EPISODES);
    const pages = await Promise.all(offsets.map((o) =>
      kitsuGet(`/anime/${kitsuId}/episodes`, { 'page[limit]': KITSU_PAGE_MAX, 'page[offset]': o, sort: 'number' }, opts.signal)));
    for (const page of pages) {
      const data = kitsuRows(page);
      rows.push(...data);
      if (data.length < KITSU_PAGE_MAX) { ended = true; break; }
    }
    offset += offsets.length * KITSU_PAGE_MAX;
  }
  const seen = new Set<number>();
  return rows
    .map(mapKitsuEpisode)
    .filter((e): e is MetadataEpisode => e != null && e.number > 0 && !seen.has(e.number) && (seen.add(e.number), true))
    .sort((x, y) => x.number - y.number);
}

const GENRES_TTL_MS = 24 * 60 * 60 * 1000;
const ADULT_GENRES = new Set(['Hentai']);

async function genres(_kind: MediaKind, opts: { includeAdult?: boolean; signal?: AbortSignal } = {}): Promise<string[]> {
  // Kitsu has one genre vocabulary shared by anime and manga.
  const all = await loadMapping<string[]>('kitsu:genres', async (signal) => {
    const names: string[] = [];
    for (let offset = 0; offset < 200; offset += KITSU_PAGE_MAX) {
      const rows = kitsuRows(await kitsuGet('/genres', { 'page[limit]': KITSU_PAGE_MAX, 'page[offset]': offset }, signal));
      names.push(...rows.map((g) => str(g.attributes.name)).filter((n): n is string => Boolean(n)));
      if (rows.length < KITSU_PAGE_MAX) break;
    }
    return names.length ? names : null;
  }, { ttlMs: GENRES_TTL_MS, signal: opts.signal, rethrow: true });
  return (all ?? []).filter((n) => opts.includeAdult || !ADULT_GENRES.has(n)).sort((a, b) => a.localeCompare(b));
}

async function ids(
  ref: MediaRef,
  opts: { signal?: AbortSignal; kind?: MediaKind } = {},
): Promise<(Required<Pick<MediaRef, 'anilistId'>> & MediaRef) | null> {
  // The router's fallback has no kind in its signature; manga is the case that
  // needs this (ani.zip covers anime), so that is the default.
  const kind: MediaKind = opts.kind ?? 'MANGA';
  const k = kindPath(kind);
  const kitsuId = await resolveKitsuId(kind, ref, opts.signal);
  if (!kitsuId) return null;
  let doc: KitsuDoc;
  try {
    doc = await kitsuGet(`/${k}/${kitsuId}`, { include: 'mappings', ...SPARSE }, opts.signal);
  } catch (err) {
    if (isKitsuMiss(err)) return null;
    throw err;
  }
  const res = kitsuRows(doc)[0];
  if (!res) return null;
  const ext = externalIdsOf(kitsuRelated(doc, res, 'mappings'), k);
  const anilistId = ext.anilistId ?? ref.anilistId;
  if (!anilistId) return null;
  const out = { anilistId, malId: ext.malId ?? ref.malId, kitsuId };
  rememberIds(kind, out);
  return out;
}

export const kitsuProvider: MetadataProvider = {
  id: 'kitsu',
  label: 'Kitsu',
  description: 'Free, no key. Trending updates slowly.',
  kinds: ['ANIME', 'MANGA'],
  isConfigured: () => true,
  feed,
  search,
  media,
  episodes,
  genres,
  ids,
};
