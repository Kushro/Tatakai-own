/**
 * Tenrai metadata provider. MyAnimeList data, Jikan v4-compatible REST at api.tenrai.org/v1 (tenraiGet).
 *
 * Feed lanes (anime / manga):
 * - trending           /top/anime?filter=airing  /  /top/manga?filter=publishing   (score-ranked, currently releasing)
 * - popular            /top/*?filter=bypopularity
 * - top_rated          /top/*
 * - airing             /anime?status=airing&order_by=members  /  publishing (members-ranked, currently releasing)
 * - upcoming           /top/*?filter=upcoming
 * - seasonal           /seasons/{year}/{season}  (anime; manga falls back to publishing)
 * - recently_completed /anime?status=complete&order_by=end_date&sort=desc
 * With a `genre` the lanes switch to the filterable `/anime` / `/manga` endpoints.
 *
 * Tenrai honours `limit` up to at least 50 (Jikan capped it at 25); we cap at 50.
 */

import type { ContentSearchResult, MediaFormat, MediaStatus, SearchFilters, TatakaiMedia } from '@/core/content/types';
import { PublicApiError } from '@/lib/net/publicApi';
import { tenraiGet } from '@/lib/tenrai/client';
import type {
  FeedOptions,
  FeedPage,
  MediaKind,
  MediaRef,
  MetadataEpisode,
  MetadataFeedKind,
  MetadataProvider,
} from '@/lib/metadata/types';
import { ADULT_GENRES, mapJikanEntry, seasonOfDate, type JikanEntry } from './mal-schema';

interface JikanPagination {
  last_visible_page?: number;
  has_next_page?: boolean;
  current_page?: number;
  items?: { count?: number; total?: number; per_page?: number };
}
interface JikanList<T = JikanEntry> { data?: T[]; pagination?: JikanPagination }

type Params = Record<string, string | number | boolean | undefined>;

const MAX_PER_PAGE = 50;
const DEFAULT_PER_PAGE = 24;
const EPISODE_PAGE_CAP = 40;
const EPISODE_CAP = 4000;

const path = (kind: MediaKind) => (kind === 'ANIME' ? 'anime' : 'manga');
const clampLimit = (n: number | undefined) => Math.max(1, Math.min(MAX_PER_PAGE, Math.floor(n || DEFAULT_PER_PAGE)));

// ── Genres (name → MAL id), cached per kind ──────────────────────────────────

interface GenreRow { mal_id: number; name: string }
const genreCache = new Map<MediaKind, Promise<GenreRow[]>>();

function loadGenres(kind: MediaKind, signal?: AbortSignal): Promise<GenreRow[]> {
  let p = genreCache.get(kind);
  if (!p) {
    p = tenraiGet<{ data?: GenreRow[] }>(`/genres/${path(kind)}`, undefined, signal).then((r) => r.data ?? []);
    genreCache.set(kind, p);
    p.catch(() => genreCache.delete(kind));
  }
  return p;
}

/** Forget cached genre lists (tests). */
export function resetTenraiCaches(): void {
  genreCache.clear();
}

const GENRE_ALIASES: Record<string, string> = { thriller: 'suspense' };

async function genreIds(kind: MediaKind, names: string[] | undefined, signal?: AbortSignal): Promise<string | undefined> {
  if (!names?.length) return undefined;
  const rows = await loadGenres(kind, signal);
  const byName = new Map(rows.map((g) => [g.name.toLowerCase(), g.mal_id]));
  const ids = names
    .map((n) => byName.get(GENRE_ALIASES[n.toLowerCase()] ?? n.toLowerCase()))
    .filter((id): id is number => id != null);
  return ids.length ? ids.join(',') : undefined;
}

// ── Mapping helpers ──────────────────────────────────────────────────────────

function mapPage(res: JikanList, kind: MediaKind, adult: boolean): TatakaiMedia[] {
  const seen = new Set<number>();
  const out: TatakaiMedia[] = [];
  for (const raw of res.data ?? []) {
    if (!raw || seen.has(raw.mal_id)) continue;
    seen.add(raw.mal_id);
    const m = mapJikanEntry(raw, kind, 'tenrai');
    if (!adult && m.isAdult) continue;
    out.push(m);
  }
  return out;
}

const STATUS_PARAM: Record<MediaKind, Partial<Record<MediaStatus, string>>> = {
  ANIME: { FINISHED: 'complete', RELEASING: 'airing', NOT_YET_RELEASED: 'upcoming' },
  MANGA: { FINISHED: 'complete', RELEASING: 'publishing', NOT_YET_RELEASED: 'upcoming', HIATUS: 'hiatus', CANCELLED: 'discontinued' },
};

const TYPE_PARAM: Record<MediaKind, Partial<Record<MediaFormat | 'NOVEL' | 'ONE_SHOT' | 'MANGA', string>>> = {
  ANIME: { TV: 'tv', TV_SHORT: 'tv', MOVIE: 'movie', OVA: 'ova', ONA: 'ona', SPECIAL: 'special', MUSIC: 'music' },
  MANGA: { MANGA: 'manga', ONE_SHOT: 'oneshot', NOVEL: 'lightnovel' },
};

const COUNTRY_TYPE: Record<string, string> = { JP: 'manga', KR: 'manhwa', CN: 'manhua', TW: 'manhua' };

const SORT: Record<NonNullable<SearchFilters['sortBy']>, { order_by: string; sort: 'asc' | 'desc' }> = {
  TRENDING_DESC: { order_by: 'members', sort: 'desc' },
  POPULARITY_DESC: { order_by: 'members', sort: 'desc' },
  SCORE_DESC: { order_by: 'score', sort: 'desc' },
  START_DATE_DESC: { order_by: 'start_date', sort: 'desc' },
  TITLE_ROMAJI: { order_by: 'title', sort: 'asc' },
  FAVOURITES_DESC: { order_by: 'favorites', sort: 'desc' },
  UPDATED_AT_DESC: { order_by: 'start_date', sort: 'desc' }, // no "updated" order in Jikan
  CHAPTERS_DESC: { order_by: 'members', sort: 'desc' }, // no chapter-count order in Jikan
  END_DATE_DESC: { order_by: 'end_date', sort: 'desc' },
};

function pageInfoOf(p: JikanPagination | undefined, page: number, perPage: number, count: number): ContentSearchResult['pageInfo'] {
  const hasNextPage = Boolean(p?.has_next_page);
  return {
    total: p?.items?.total ?? (hasNextPage ? page * perPage + 1 : (page - 1) * perPage + count),
    currentPage: p?.current_page ?? page,
    lastPage: p?.last_visible_page ?? (hasNextPage ? page + 1 : page),
    hasNextPage,
    perPage: p?.items?.per_page ?? perPage,
  };
}

/** Last day of the season's final month, for date-bounded genre lanes. */
function seasonRange(season: string, year: number): { start_date: string; end_date: string } {
  const r = { WINTER: ['01-01', '03-31'], SPRING: ['04-01', '06-30'], SUMMER: ['07-01', '09-30'], FALL: ['10-01', '12-31'] }[season] ?? ['01-01', '12-31'];
  return { start_date: `${year}-${r[0]}`, end_date: `${year}-${r[1]}` };
}

// ── Feed ─────────────────────────────────────────────────────────────────────

async function feedRequest(kind: MediaKind, feed: MetadataFeedKind, opts: FeedOptions): Promise<{ url: string; params: Params }> {
  const adult = opts.isAdult === true;
  const params: Params = { page: opts.page ?? 1, limit: clampLimit(opts.perPage), ...(adult ? {} : { sfw: true }) };
  const k = path(kind);
  const genres = opts.genre ? await genreIds(kind, [opts.genre], opts.signal) : undefined;
  const now = seasonOfDate(new Date());
  const season = opts.season ?? now.season;
  const seasonYear = opts.seasonYear ?? now.year;

  // Manga-only narrowing: always goes through the filterable /manga endpoint.
  const mangaType = kind === 'MANGA'
    ? (opts.novelsOnly ? 'lightnovel' : opts.countryOfOrigin ? COUNTRY_TYPE[opts.countryOfOrigin] : undefined)
    : undefined;

  if (genres || mangaType) {
    const q: Params = { ...params, genres, type: mangaType };
    switch (feed) {
      case 'top_rated': Object.assign(q, { order_by: 'score', sort: 'desc', min_score: 6 }); break;
      case 'airing': case 'trending': Object.assign(q, { status: STATUS_PARAM[kind].RELEASING, order_by: 'members', sort: 'desc' }); break;
      case 'upcoming': Object.assign(q, { status: 'upcoming', order_by: 'members', sort: 'desc' }); break;
      case 'recently_completed': Object.assign(q, { status: 'complete', order_by: 'end_date', sort: 'desc' }); break;
      case 'seasonal': Object.assign(q, kind === 'ANIME' ? seasonRange(season, seasonYear) : { status: 'publishing' }, { order_by: 'members', sort: 'desc' }); break;
      default: Object.assign(q, { order_by: 'members', sort: 'desc' });
    }
    return { url: `/${k}`, params: q };
  }

  switch (feed) {
    case 'trending': return { url: `/top/${k}`, params: { ...params, filter: kind === 'ANIME' ? 'airing' : 'publishing' } };
    case 'popular': return { url: `/top/${k}`, params: { ...params, filter: 'bypopularity' } };
    case 'top_rated': return { url: `/top/${k}`, params };
    case 'airing': return { url: `/${k}`, params: { ...params, status: STATUS_PARAM[kind].RELEASING, order_by: 'members', sort: 'desc' } };
    case 'upcoming': return { url: `/top/${k}`, params: { ...params, filter: 'upcoming' } };
    case 'seasonal':
      return kind === 'ANIME'
        ? { url: `/seasons/${seasonYear}/${season.toLowerCase()}`, params }
        : { url: `/top/manga`, params: { ...params, filter: 'publishing' } };
    case 'recently_completed': return { url: `/${k}`, params: { ...params, status: 'complete', order_by: 'end_date', sort: 'desc' } };
  }
}

async function feed(kind: MediaKind, feedKind: MetadataFeedKind, opts: FeedOptions): Promise<FeedPage> {
  const { url, params } = await feedRequest(kind, feedKind, opts);
  const res = await tenraiGet<JikanList>(url, params, opts.signal);
  return {
    media: mapPage(res, kind, opts.isAdult === true),
    page: res.pagination?.current_page ?? Number(params.page),
    hasNextPage: Boolean(res.pagination?.has_next_page),
  };
}

// ── Search ───────────────────────────────────────────────────────────────────

async function search(
  kind: MediaKind,
  f: SearchFilters & { novelsOnly?: boolean; countryOfOrigin?: string; signal?: AbortSignal },
): Promise<ContentSearchResult> {
  const adult = f.isAdult === true;
  const page = f.page ?? 1;
  const perPage = clampLimit(f.perPage);
  const params: Params = { page, limit: perPage, ...(adult ? {} : { sfw: true }) };

  const q = f.query?.trim();
  if (q) params.q = q;

  const genres = await genreIds(kind, f.genres, f.signal);
  if (genres) params.genres = genres;

  // Jikan takes a single `type` / `status`; extra values are filtered client-side below.
  const formats = f.format ?? [];
  const typeMap = TYPE_PARAM[kind] as Record<string, string | undefined>;
  const types = formats.map((x) => typeMap[x]).filter((x): x is string => Boolean(x));
  if (f.novelsOnly) params.type = 'lightnovel';
  else if (f.countryOfOrigin && kind === 'MANGA' && COUNTRY_TYPE[f.countryOfOrigin]) params.type = COUNTRY_TYPE[f.countryOfOrigin];
  else if (new Set(types).size === 1) params.type = types[0];

  const statuses = (f.status ?? []).map((s) => STATUS_PARAM[kind][s]).filter((s): s is string => Boolean(s));
  if (statuses.length === 1) params.status = statuses[0];

  // Jikan's end_date bounds the END of a run (it would drop shows that started in range but
  // finished later), so only the lower bound is sent; the upper bound is applied below.
  if (f.year?.min) params.start_date = `${f.year.min}-01-01`;
  if (f.rating?.min) params.min_score = Math.max(0, Math.min(10, f.rating.min / 10));
  if (f.rating?.max) params.max_score = Math.max(0, Math.min(10, f.rating.max / 10));

  if (f.sortBy) Object.assign(params, SORT[f.sortBy]);
  else if (!q) Object.assign(params, SORT.POPULARITY_DESC);

  const res = await tenraiGet<JikanList>(`/${path(kind)}`, params, f.signal);
  let media = mapPage(res, kind, adult);
  if (formats.length > 1) {
    const allowed = new Set(formats as string[]);
    media = media.filter((m) => !m.format || allowed.has(m.format));
  }
  if (statuses.length > 1) {
    const allowed = new Set(f.status);
    media = media.filter((m) => !m.status || allowed.has(m.status));
  }
  if (f.year?.max) media = media.filter((m) => m.startDate?.year == null || m.startDate.year <= f.year!.max!);
  if (f.episodes?.min != null || f.episodes?.max != null) {
    media = media.filter((m) => {
      if (m.episodes == null) return true;
      return (f.episodes!.min == null || m.episodes >= f.episodes!.min) && (f.episodes!.max == null || m.episodes <= f.episodes!.max);
    });
  }
  return { media, pageInfo: pageInfoOf(res.pagination, page, perPage, media.length) };
}

// ── Detail / episodes / genres ───────────────────────────────────────────────

async function media(kind: MediaKind, ref: MediaRef, opts: { signal?: AbortSignal } = {}): Promise<TatakaiMedia | null> {
  if (!ref.malId) return null;
  try {
    const res = await tenraiGet<{ data?: JikanEntry }>(`/${path(kind)}/${ref.malId}/full`, undefined, opts.signal);
    return res.data ? mapJikanEntry(res.data, kind, 'tenrai') : null;
  } catch (err) {
    if (err instanceof PublicApiError && err.status === 404) return null;
    throw err;
  }
}

interface JikanEpisodeRow {
  mal_id: number;
  title?: string | null;
  title_japanese?: string | null;
  title_romanji?: string | null;
  aired?: string | null;
  duration?: number | null;
  filler?: boolean | null;
  recap?: boolean | null;
  synopsis?: string | null;
}

async function episodes(ref: MediaRef, opts: { signal?: AbortSignal } = {}): Promise<MetadataEpisode[]> {
  if (!ref.malId) return [];
  const out: MetadataEpisode[] = [];
  for (let page = 1; page <= EPISODE_PAGE_CAP && out.length < EPISODE_CAP; page++) {
    let res: JikanList<JikanEpisodeRow>;
    try {
      res = await tenraiGet<JikanList<JikanEpisodeRow>>(`/anime/${ref.malId}/episodes`, { page }, opts.signal);
    } catch (err) {
      if (err instanceof PublicApiError && err.status === 404 && page === 1) return [];
      throw err;
    }
    for (const e of res.data ?? []) {
      const titles: Record<string, string> = {};
      if (e.title) titles.en = e.title;
      if (e.title_romanji) titles['x-jat'] = e.title_romanji;
      if (e.title_japanese) titles.ja = e.title_japanese;
      out.push({
        number: e.mal_id,
        title: e.title || e.title_romanji || e.title_japanese || undefined,
        titles: Object.keys(titles).length ? titles : undefined,
        overview: e.synopsis || undefined,
        airDate: e.aired || undefined,
        runtimeMin: e.duration ? Math.max(1, Math.round(e.duration / 60)) : undefined,
        isFiller: Boolean(e.filler),
        isRecap: Boolean(e.recap),
      });
    }
    if (!res.pagination?.has_next_page) break;
  }
  return out.slice(0, EPISODE_CAP);
}

async function genres(kind: MediaKind, opts: { includeAdult?: boolean; signal?: AbortSignal } = {}): Promise<string[]> {
  const rows = await loadGenres(kind, opts.signal);
  return rows.map((g) => g.name).filter((n) => opts.includeAdult || !ADULT_GENRES.has(n.toLowerCase()));
}

export const tenraiProvider: MetadataProvider = {
  id: 'tenrai',
  label: 'Tenrai',
  description: 'MyAnimeList data (Jikan successor). Free, no key.',
  kinds: ['ANIME', 'MANGA'],
  isConfigured: () => true,
  feed,
  search,
  media,
  episodes,
  genres,
};
