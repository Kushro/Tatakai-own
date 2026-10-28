/**
 * AniList metadata provider. GraphQL at graphql.anilist.co via anilistGql (@/lib/anilist/client).
 *
 * Serves feed / search / media / genres / ids for ANIME and MANGA (novels are
 * MANGA-type entries with format NOVEL). AniList has no episode list, so
 * `episodes` is intentionally not implemented.
 *
 * Adult handling: adult titles are excluded unless the caller passes
 * `isAdult: true` explicitly.
 */

import type { ContentSearchResult, MediaSource, SearchFilters, TatakaiMedia } from '@/core/content/types';
import { anilistGql, AniListError } from '@/lib/anilist/client';
import { fetchAniListGenres } from '@/lib/anilist';
import { MEDIA_CARD_FRAGMENT, MEDIA_DETAIL_FRAGMENT, mapAniListMedia, type AniListMediaRaw } from '@/lib/anilist/media';
import type {
  FeedOptions,
  FeedPage,
  MediaKind,
  MediaRef,
  MetadataFeedKind,
  MetadataProvider,
} from '@/lib/metadata/types';

type SeasonName = 'WINTER' | 'SPRING' | 'SUMMER' | 'FALL';

interface PageInfoRaw {
  total?: number | null;
  currentPage?: number | null;
  lastPage?: number | null;
  hasNextPage?: boolean | null;
  perPage?: number | null;
}

interface PageData {
  Page: { pageInfo?: PageInfoRaw | null; media?: AniListMediaRaw[] | null };
}

const PAGE_QUERY = `
query ($page: Int, $perPage: Int, $type: MediaType, $sort: [MediaSort], $search: String,
  $genre_in: [String], $tag_in: [String], $format_in: [MediaFormat], $format_not_in: [MediaFormat],
  $status_in: [MediaStatus], $season: MediaSeason, $seasonYear: Int, $isAdult: Boolean,
  $country: CountryCode, $startDate_greater: FuzzyDateInt, $startDate_lesser: FuzzyDateInt,
  $averageScore_greater: Int, $averageScore_lesser: Int, $episodes_greater: Int, $episodes_lesser: Int) {
  Page(page: $page, perPage: $perPage) {
    pageInfo { total currentPage lastPage hasNextPage perPage }
    media(type: $type, sort: $sort, search: $search, genre_in: $genre_in, tag_in: $tag_in,
      format_in: $format_in, format_not_in: $format_not_in, status_in: $status_in,
      season: $season, seasonYear: $seasonYear, isAdult: $isAdult, countryOfOrigin: $country,
      startDate_greater: $startDate_greater, startDate_lesser: $startDate_lesser,
      averageScore_greater: $averageScore_greater, averageScore_lesser: $averageScore_lesser,
      episodes_greater: $episodes_greater, episodes_lesser: $episodes_lesser) {
      ...mediaCard
    }
  }
}
${MEDIA_CARD_FRAGMENT}`;

const MEDIA_QUERY = `
query ($id: Int, $idMal: Int, $type: MediaType) {
  Media(id: $id, idMal: $idMal, type: $type) { ...mediaDetail }
}
${MEDIA_DETAIL_FRAGMENT}`;

const IDS_QUERY = `
query ($id: Int, $idMal: Int, $type: MediaType) {
  Media(id: $id, idMal: $idMal, type: $type) { id idMal type }
}`;

function currentSeason(): { season: SeasonName; year: number } {
  const now = new Date();
  const month = now.getMonth() + 1;
  const season: SeasonName = month <= 3 ? 'WINTER' : month <= 6 ? 'SPRING' : month <= 9 ? 'SUMMER' : 'FALL';
  return { season, year: now.getFullYear() };
}

const clampPerPage = (n: number | undefined, fallback: number) => Math.min(50, Math.max(1, Math.floor(n || fallback)));

/** Variables shared by feed + search; undefined values are dropped by JSON.stringify. */
type Vars = Record<string, unknown>;

function kindVars(kind: MediaKind, o: { novelsOnly?: boolean; countryOfOrigin?: string; isAdult?: boolean }): Vars {
  const vars: Vars = {
    type: kind,
    // AniList: `isAdult: false` excludes adult; omitting it returns both.
    isAdult: o.isAdult === true ? undefined : false,
    country: o.countryOfOrigin ? o.countryOfOrigin.toUpperCase() : undefined,
  };
  if (kind === 'MANGA') {
    if (o.novelsOnly) vars.format_in = ['NOVEL'];
    else vars.format_not_in = ['NOVEL'];
  }
  return vars;
}

function feedVars(kind: MediaKind, feed: MetadataFeedKind, opts: FeedOptions): Vars {
  const vars: Vars = {
    ...kindVars(kind, opts),
    page: Math.max(1, opts.page ?? 1),
    perPage: clampPerPage(opts.perPage, 20),
    genre_in: opts.genre ? [opts.genre] : undefined,
  };
  switch (feed) {
    case 'trending':
      vars.sort = ['TRENDING_DESC'];
      break;
    case 'popular':
      vars.sort = ['POPULARITY_DESC'];
      break;
    case 'top_rated':
      vars.sort = ['SCORE_DESC'];
      break;
    case 'airing':
      vars.status_in = ['RELEASING'];
      vars.sort = ['POPULARITY_DESC'];
      break;
    case 'upcoming':
      vars.status_in = ['NOT_YET_RELEASED'];
      vars.sort = ['POPULARITY_DESC'];
      break;
    case 'recently_completed':
      vars.status_in = ['FINISHED'];
      vars.sort = ['END_DATE_DESC'];
      break;
    case 'seasonal': {
      const cur = currentSeason();
      const season = opts.season ?? cur.season;
      const year = opts.seasonYear ?? cur.year;
      if (kind === 'ANIME') {
        vars.season = season;
        vars.seasonYear = year;
      } else {
        // Manga has no seasons: approximate with titles that started that year.
        vars.startDate_greater = year * 10000;
        vars.startDate_lesser = (year + 1) * 10000;
      }
      vars.sort = ['POPULARITY_DESC'];
      break;
    }
  }
  return vars;
}

function searchVars(
  kind: MediaKind,
  f: SearchFilters & { novelsOnly?: boolean; countryOfOrigin?: string },
): Vars {
  const query = f.query?.trim();
  const vars: Vars = {
    ...kindVars(kind, f),
    page: Math.max(1, f.page ?? 1),
    perPage: clampPerPage(f.perPage, 20),
    search: query || undefined,
    genre_in: f.genres?.length ? f.genres : undefined,
    tag_in: f.tags?.length ? f.tags : undefined,
    status_in: f.status?.length ? f.status : undefined,
    sort: [f.sortBy ?? (query ? 'SEARCH_MATCH' : 'POPULARITY_DESC')],
  };
  if (f.format?.length) vars.format_in = f.format;
  // AniList takes a single season; several are post-filtered in `searchAniList`.
  if (f.season?.length === 1) vars.season = f.season[0];
  // FuzzyDateInt is YYYYMMDD; `_greater`/`_lesser` are exclusive.
  if (f.year?.min != null) vars.startDate_greater = f.year.min * 10000;
  if (f.year?.max != null) vars.startDate_lesser = (f.year.max + 1) * 10000;
  if (f.rating?.min != null) vars.averageScore_greater = Math.max(0, Math.round(f.rating.min) - 1);
  if (f.rating?.max != null) vars.averageScore_lesser = Math.round(f.rating.max) + 1;
  if (f.episodes?.min != null) vars.episodes_greater = Math.max(0, Math.round(f.episodes.min) - 1);
  if (f.episodes?.max != null) vars.episodes_lesser = Math.round(f.episodes.max) + 1;
  return vars;
}

async function fetchPage(vars: Vars, signal?: AbortSignal): Promise<{ media: TatakaiMedia[]; pageInfo: PageInfoRaw }> {
  const data = await anilistGql<PageData>(PAGE_QUERY, vars, { signal });
  const page = data?.Page;
  return {
    media: (page?.media ?? []).filter((m): m is AniListMediaRaw => Boolean(m?.id)).map(mapAniListMedia),
    pageInfo: page?.pageInfo ?? {},
  };
}

const norm = (s: string) => s.trim().toLowerCase();

async function feed(kind: MediaKind, feedKind: MetadataFeedKind, opts: FeedOptions): Promise<FeedPage> {
  const vars = feedVars(kind, feedKind, opts);
  const { media, pageInfo } = await fetchPage(vars, opts.signal);
  return {
    media,
    page: pageInfo.currentPage ?? (vars.page as number),
    hasNextPage: Boolean(pageInfo.hasNextPage),
  };
}

async function search(
  kind: MediaKind,
  filters: SearchFilters & { novelsOnly?: boolean; countryOfOrigin?: string; signal?: AbortSignal },
): Promise<ContentSearchResult> {
  const vars = searchVars(kind, filters);
  const { media: raw, pageInfo } = await fetchPage(vars, filters.signal);
  let media = raw;
  // Filters AniList's Page query can't express are applied to the returned page.
  if (filters.studio?.length) {
    const wanted = filters.studio.map(norm);
    media = media.filter((m) => m.studios.some((s) => wanted.includes(norm(s.name))));
  }
  if (filters.source?.length) {
    const wanted = new Set<MediaSource>(filters.source);
    media = media.filter((m) => m.source && wanted.has(m.source));
  }
  if (filters.season && filters.season.length > 1) {
    const wanted = new Set(filters.season);
    media = media.filter((m) => m.season && wanted.has(m.season));
  }
  const perPage = (vars.perPage as number) ?? 20;
  return {
    media,
    pageInfo: {
      total: pageInfo.total ?? media.length,
      currentPage: pageInfo.currentPage ?? (vars.page as number),
      lastPage: pageInfo.lastPage ?? (vars.page as number),
      hasNextPage: Boolean(pageInfo.hasNextPage),
      perPage: pageInfo.perPage ?? perPage,
    },
  };
}

async function media(kind: MediaKind, ref: MediaRef, opts?: { signal?: AbortSignal }): Promise<TatakaiMedia | null> {
  if (!ref.anilistId && !ref.malId) return null;
  try {
    const data = await anilistGql<{ Media?: AniListMediaRaw | null }>(
      MEDIA_QUERY,
      ref.anilistId ? { id: ref.anilistId, type: kind } : { idMal: ref.malId, type: kind },
      { signal: opts?.signal },
    );
    return data?.Media?.id ? mapAniListMedia(data.Media) : null;
  } catch (err) {
    if (err instanceof AniListError && err.isNotFound) return null;
    throw err;
  }
}

async function ids(
  ref: MediaRef,
  opts?: { signal?: AbortSignal; kind?: MediaKind },
): Promise<(Required<Pick<MediaRef, 'anilistId'>> & MediaRef) | null> {
  // AniList ids are unique across anime/manga; MAL ids are not, so a MAL-only
  // lookup needs the kind (the router may omit it, in which case we skip).
  if (!ref.anilistId && !(ref.malId && opts?.kind)) return null;
  try {
    const data = await anilistGql<{ Media?: { id: number; idMal?: number | null } | null }>(
      IDS_QUERY,
      ref.anilistId ? { id: ref.anilistId } : { idMal: ref.malId, type: opts!.kind },
      { signal: opts?.signal },
    );
    const m = data?.Media;
    if (!m?.id) return null;
    return { ...ref, anilistId: m.id, malId: m.idMal ?? ref.malId };
  } catch (err) {
    if (err instanceof AniListError && err.isNotFound) return null;
    throw err;
  }
}

export const anilistProvider: MetadataProvider = {
  id: 'anilist',
  label: 'AniList',
  description: 'Most complete source. Temporarily disabled by AniList (403) as of Oct 2026.',
  kinds: ['ANIME', 'MANGA'],
  isConfigured: () => true,
  feed,
  search,
  media,
  genres: (_kind, opts) => fetchAniListGenres(Boolean(opts?.includeAdult)),
  ids,
};
