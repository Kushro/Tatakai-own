import { publicJson } from '@/lib/net/publicApi';
import { anilistGql } from '@/lib/anilist/client';
import {
  buildAniListAuthUrl,
  completeAniListLogin,
  isAniListConfigured,
  startAniListLogin,
  withAniListToken,
} from '@/lib/anilist/auth';
import { disconnectMal, exchangeMalCode, getMalAuthUrl } from '@/lib/mal';
import { MAL_API_URL } from '@/lib/mal/auth';

export const ALL_GENRES = [
  "Action", "Adventure", "Comedy", "Drama", "Ecchi", "Fantasy", "Horror", "Mahou Shoujo", 
  "Mecha", "Music", "Mystery", "Psychological", "Romance", "Sci-Fi", "Slice of Life", 
  "Sports", "Supernatural", "Thriller"
];

// ===========================================
// MyAnimeList Integration (REST v2, user token, direct)
// ===========================================

const malBearer = (accessToken: string) => ({ Authorization: `Bearer ${accessToken}` });

// Generate MAL OAuth URL (PKCE; see @/lib/mal/auth)
export function getMALAuthUrl(): string {
  return getMalAuthUrl();
}

// Exchange the callback code for tokens and persist them (client side).
export async function exchangeMALCode(code: string, userId: string, state?: string | null): Promise<boolean> {
  // userId is kept for backward compatibility with existing callers.
  void userId;
  await exchangeMalCode(code, state);
  return true;
}

// Fetch MAL user info
export async function fetchMALUser(accessToken: string): Promise<any> {
  return publicJson(`${MAL_API_URL}/users/@me`, { headers: malBearer(accessToken) });
}

// Fetch MAL anime list (all pages)
export async function fetchMALAnimeList(accessToken: string): Promise<any[]> {
  const out: any[] = [];
  let url: string | undefined = `${MAL_API_URL}/users/@me/animelist?fields=list_status&limit=1000`;
  for (let page = 0; url && page < 50; page++) {
    const data: any = await publicJson(url, { headers: malBearer(accessToken) });
    out.push(...(data.data || []));
    url = data.paging?.next;
  }
  return out;
}

// Update MAL anime status
export async function updateMALAnimeStatus(
  accessToken: string,
  animeId: number,
  status: 'watching' | 'completed' | 'on_hold' | 'dropped' | 'plan_to_watch',
  episodesWatched?: number
): Promise<boolean> {
  const body = new URLSearchParams({ status });
  if (episodesWatched !== undefined) {
    body.append('num_watched_episodes', episodesWatched.toString());
  }
  const { publicFetch } = await import('@/lib/net/publicApi');
  const response = await publicFetch(`${MAL_API_URL}/anime/${animeId}/my_list_status`, {
    method: 'PATCH',
    headers: { ...malBearer(accessToken), 'Content-Type': 'application/x-www-form-urlencoded' },
    body: body.toString(),
  });
  return response.ok;
}

// ===========================================
// AniList Integration (implicit grant, direct GraphQL)
// ===========================================

export { isAniListConfigured };

/**
 * Generate the AniList authorize URL (implicit grant: `response_type=token`) and
 * mark the login in flight. AniList returns to the redirect URL registered on its
 * client (tatakai://integration/anilist/redirect) with the token in the URL
 * fragment. The old redirectUri/state parameters are accepted and ignored.
 */
export function getAniListAuthUrl(_redirectUri?: string, _state?: string): string {
  return startAniListLogin();
}

/** Same URL without marking a login as started (diagnostics only). */
export const previewAniListAuthUrl = buildAniListAuthUrl;

/**
 * Finish the AniList login from the redirect fragment (`#access_token=…`) and
 * persist the token on the signed-in user's profile.
 */
export async function exchangeAniListCode(fragment: string, _userId?: string): Promise<boolean> {
  await completeAniListLogin(fragment);
  return true;
}

/**
 * GraphQL helper — straight to https://graphql.anilist.co through `anilistGql`
 * (paced and retried by publicApi). `accessToken` is the user's real AniList
 * token and is only sent to AniList. A 401 clears the stored login
 * (AniListAuthExpiredError); 403/429/5xx/offline raise AniListUnavailableError —
 * callers surface "AniList is unavailable right now" and nothing loops.
 */
async function anilistQuery(query: string, variables: Record<string, any>, accessToken?: string): Promise<any> {
  if (!accessToken) return anilistGql<any>(query, variables);
  return withAniListToken(accessToken, (token) => anilistGql<any>(query, variables, { token, retries: 1 }));
}

// ===========================================
// External profile statistics (AniList / MAL)
// ===========================================

export interface ExternalAnimeStats {
  count: number;
  episodesWatched: number;
  daysWatched: number;
  meanScore: number;
  watching?: number;
  completed?: number;
  planning?: number;
  dropped?: number;
  paused?: number;
}

export interface ExternalMangaStats {
  count: number;
  chaptersRead: number;
  volumesRead?: number;
  meanScore: number;
}

export interface ExternalStats {
  provider: 'anilist' | 'mal';
  username?: string | null;
  profileUrl?: string | null;
  avatar?: string | null;
  anime: ExternalAnimeStats;
  manga?: ExternalMangaStats | null;
  topGenres?: { genre: string; count: number }[];
}

// Fetch normalized AniList statistics for the connected viewer.
export async function fetchAniListStatistics(accessToken: string): Promise<ExternalStats> {
  const query = `
    query {
      Viewer {
        id
        name
        siteUrl
        avatar { large medium }
        statistics {
          anime {
            count
            meanScore
            minutesWatched
            episodesWatched
            statuses { status count }
            genres { genre count }
          }
          manga {
            count
            meanScore
            chaptersRead
            volumesRead
          }
        }
      }
    }
  `;
  const data = await anilistQuery(query, {}, accessToken);
  const viewer = data?.Viewer;
  const a = viewer?.statistics?.anime || {};
  const m = viewer?.statistics?.manga || {};

  const statusMap: Record<string, number> = {};
  for (const s of a.statuses || []) {
    statusMap[String(s.status).toUpperCase()] = Number(s.count || 0);
  }

  const topGenres = (a.genres || [])
    .slice()
    .sort((x: any, y: any) => Number(y.count || 0) - Number(x.count || 0))
    .slice(0, 5)
    .map((g: any) => ({ genre: g.genre, count: Number(g.count || 0) }));

  return {
    provider: 'anilist',
    username: viewer?.name || null,
    profileUrl: viewer?.siteUrl || (viewer?.name ? `https://anilist.co/user/${viewer.name}` : null),
    avatar: viewer?.avatar?.large || viewer?.avatar?.medium || null,
    anime: {
      count: Number(a.count || 0),
      episodesWatched: Number(a.episodesWatched || 0),
      daysWatched: Number(((a.minutesWatched || 0) / 1440).toFixed(1)),
      meanScore: Number(a.meanScore || 0),
      watching: statusMap.CURRENT || 0,
      completed: statusMap.COMPLETED || 0,
      planning: statusMap.PLANNING || 0,
      dropped: statusMap.DROPPED || 0,
      paused: statusMap.PAUSED || 0,
    },
    manga: m.count
      ? {
          count: Number(m.count || 0),
          chaptersRead: Number(m.chaptersRead || 0),
          volumesRead: Number(m.volumesRead || 0),
          meanScore: Number(m.meanScore || 0),
        }
      : null,
    topGenres,
  };
}

// Fetch normalized MyAnimeList statistics for the connected user.
export async function fetchMALStatistics(accessToken: string): Promise<ExternalStats> {
  const data: any = await publicJson(`${MAL_API_URL}/users/@me?fields=anime_statistics`, { headers: malBearer(accessToken) });
  const s = data?.anime_statistics || {};
  const name = data?.name || null;

  return {
    provider: 'mal',
    username: name,
    profileUrl: name ? `https://myanimelist.net/profile/${name}` : null,
    avatar: data?.picture || null,
    anime: {
      count: Number(s.num_items || 0),
      episodesWatched: Number(s.num_episodes || 0),
      daysWatched: Number((s.num_days_watched ?? s.num_days ?? 0)),
      meanScore: Number(s.mean_score || 0),
      watching: Number(s.num_items_watching || 0),
      completed: Number(s.num_items_completed || 0),
      planning: Number(s.num_items_plan_to_watch || 0),
      dropped: Number(s.num_items_dropped || 0),
      paused: Number(s.num_items_on_hold || 0),
    },
    manga: null,
    topGenres: [],
  };
}

// Fetch AniList user info
export async function fetchAniListUser(accessToken: string): Promise<any> {
  const query = `
    query {
      Viewer {
        id
        name
        avatar { medium large }
        bannerImage
        statistics {
          anime {
            count
            minutesWatched
            episodesWatched
          }
        }
      }
    }
  `;
  const data = await anilistQuery(query, {}, accessToken);
  return data.Viewer;
}

/** One AniList list entry as the sync layer consumes it. */
export interface AniListListEntry {
  id: number;
  mediaId: number;
  status: string;
  progress: number | null;
  progressVolumes?: number | null;
  /** 0-100 regardless of the user's score format. */
  score: number | null;
  updatedAt?: number | null;
  media: {
    id: number;
    idMal?: number | null;
    format?: string | null;
    title?: { romaji?: string | null; english?: string | null; native?: string | null } | null;
    coverImage?: { medium?: string | null; large?: string | null } | null;
    episodes?: number | null;
    chapters?: number | null;
    volumes?: number | null;
  } | null;
}

const LIST_ENTRY_FIELDS = `
  id mediaId status progress progressVolumes score(format: POINT_100) updatedAt
  media { id idMal format title { romaji english native } coverImage { medium large } episodes chapters volumes }
`;

/**
 * The whole AniList list (chunked, custom lists de-duplicated). AniList being
 * down raises AniListUnavailableError once; there is no retry loop.
 */
export async function fetchAniListList(accessToken: string, userId: number, type: 'ANIME' | 'MANGA'): Promise<AniListListEntry[]> {
  const query = `
    query ($userId: Int, $type: MediaType, $chunk: Int) {
      MediaListCollection(userId: $userId, type: $type, chunk: $chunk, perChunk: 500, forceSingleCompletedList: true) {
        hasNextChunk
        lists { name status entries { ${LIST_ENTRY_FIELDS} } }
      }
    }
  `;
  const byMedia = new Map<number, AniListListEntry>();
  for (let chunk = 1; chunk <= 40; chunk++) {
    const data = await anilistQuery(query, { userId, type, chunk }, accessToken);
    const collection = data?.MediaListCollection;
    for (const list of collection?.lists || []) {
      for (const entry of list.entries || []) {
        if (entry?.mediaId && !byMedia.has(entry.mediaId)) byMedia.set(entry.mediaId, entry);
      }
    }
    if (!collection?.hasNextChunk) break;
  }
  return [...byMedia.values()];
}

// Fetch AniList anime list
export async function fetchAniListUserList(accessToken: string, userId: number): Promise<any[]> {
  return fetchAniListList(accessToken, userId, 'ANIME');
}

// Fetch AniList manga list (manga and novels)
export async function fetchAniListMangaList(accessToken: string, userId: number): Promise<any[]> {
  return fetchAniListList(accessToken, userId, 'MANGA');
}

const ANILIST_LIST_STATUSES = new Set(['CURRENT', 'PLANNING', 'COMPLETED', 'DROPPED', 'PAUSED', 'REPEATING']);

/** Accepts an AniList status or a Tatakai one (watching, reading, plan_to_*, on_hold...). */
function toAniListListStatus(status: string, manga: boolean): string {
  const upper = String(status || '').trim().toUpperCase();
  if (ANILIST_LIST_STATUSES.has(upper)) return upper;
  return manga ? mapTatakaiMangaStatusToAniList(status) : mapTatakaiStatusToAniList(status);
}

async function saveAniListEntry(accessToken: string, mediaId: number, status: string, progress: number | undefined, scoreRaw: number | undefined, manga: boolean): Promise<boolean> {
  const mutation = `
    mutation ($mediaId: Int, $status: MediaListStatus, $progress: Int, $scoreRaw: Int) {
      SaveMediaListEntry(mediaId: $mediaId, status: $status, progress: $progress, scoreRaw: $scoreRaw) { id status progress }
    }
  `;
  const data = await anilistQuery(mutation, {
    mediaId,
    status: toAniListListStatus(status, manga),
    progress: progress !== undefined && Number.isFinite(progress) ? Math.max(0, Math.trunc(progress)) : undefined,
    scoreRaw: scoreRaw !== undefined && scoreRaw > 0 ? Math.min(100, Math.round(scoreRaw)) : undefined,
  }, accessToken);
  return Boolean(data?.SaveMediaListEntry?.id);
}

// Update AniList anime status
export async function updateAniListAnimeStatus(
  accessToken: string,
  mediaId: number,
  status: 'CURRENT' | 'COMPLETED' | 'PAUSED' | 'DROPPED' | 'PLANNING' | string,
  progress?: number,
  scoreRaw?: number
): Promise<boolean> {
  return saveAniListEntry(accessToken, mediaId, status, progress, scoreRaw, false);
}

// Update AniList manga status (also used for novels)
export async function updateAniListMangaStatus(
  accessToken: string,
  mediaId: number,
  status: 'CURRENT' | 'COMPLETED' | 'PAUSED' | 'DROPPED' | 'PLANNING' | string,
  progress?: number,
  scoreRaw?: number
): Promise<boolean> {
  return saveAniListEntry(accessToken, mediaId, status, progress, scoreRaw, true);
}

/** Remove a title from the user's AniList (anime or manga). Missing entries are fine. */
export async function deleteAniListEntry(accessToken: string, mediaId: number): Promise<boolean> {
  const lookup = await anilistQuery('query ($id: Int) { Media(id: $id) { mediaListEntry { id } } }', { id: mediaId }, accessToken);
  const entryId = lookup?.Media?.mediaListEntry?.id;
  if (!entryId) return false;
  const data = await anilistQuery('mutation ($id: Int) { DeleteMediaListEntry(id: $id) { deleted } }', { id: entryId }, accessToken);
  return Boolean(data?.DeleteMediaListEntry?.deleted);
}

export type AniListSort =
  | 'POPULARITY_DESC'
  | 'SCORE_DESC'
  | 'TRENDING_DESC'
  | 'START_DATE_DESC'
  | 'FAVOURITES_DESC';

export type AniListSearchFilters = {
  page?: number;
  perPage?: number;
  format?: 'TV' | 'TV_SHORT' | 'MOVIE' | 'SPECIAL' | 'OVA' | 'ONA' | 'MUSIC';
  status?: 'FINISHED' | 'RELEASING' | 'NOT_YET_RELEASED' | 'CANCELLED' | 'HIATUS';
  season?: 'WINTER' | 'SPRING' | 'SUMMER' | 'FALL';
  seasonYear?: number;
  countryOfOrigin?: 'JP' | 'KR' | 'CN' | 'TW' | 'US';
  genres?: string[];
  sort?: AniListSort;
};

export interface AniListMedia {
  id: number;
  idMal?: number | null;
  bannerImage?: string | null;
  format?: string | null;
  title?: {
    romaji?: string | null;
    english?: string | null;
    native?: string | null;
  };
  coverImage?: {
    medium?: string | null;
    large?: string | null;
  };
  episodes?: number | null;
  chapters?: number | null;
  volumes?: number | null;
  status?: string | null;
  seasonYear?: number | null;
  season?: string | null;
  genres?: string[];
  averageScore?: number | null;
  popularity?: number | null;
  favourites?: number | null;
  trending?: number | null;
  countryOfOrigin?: string | null;
  startDate?: { year?: number | null; month?: number | null; day?: number | null };
  studios?: { nodes?: Array<{ name?: string | null } | null> | null } | null;
}

// Search anime on AniList
export async function searchAniListAnime(title: string, filters: AniListSearchFilters = {}): Promise<AniListMedia[]> {
  const query = `
    query (
      $page: Int,
      $search: String,
      $perPage: Int,
      $format: MediaFormat,
      $status: MediaStatus,
      $season: MediaSeason,
      $seasonYear: Int,
      $countryOfOrigin: CountryCode,
      $genreIn: [String],
      $sort: [MediaSort]
    ) {
      Page(page: $page, perPage: $perPage) {
        media(
          search: $search,
          type: ANIME,
          format: $format,
          status: $status,
          season: $season,
          seasonYear: $seasonYear,
          countryOfOrigin: $countryOfOrigin,
          genre_in: $genreIn,
          sort: $sort
        ) {
          id
          idMal
          bannerImage
          format
          title { romaji english native }
          coverImage { medium large }
          episodes
          status
          seasonYear
          season
          genres
          averageScore
          popularity
          favourites
          trending
          countryOfOrigin
          startDate { year }
        }
      }
    }
  `;
  const variables = {
    page: filters.page || 1,
    search: title,
    perPage: filters.perPage || 10,
    format: filters.format,
    status: filters.status,
    season: filters.season,
    seasonYear: filters.seasonYear,
    countryOfOrigin: filters.countryOfOrigin,
    genreIn: filters.genres,
    sort: filters.sort ? [filters.sort] : undefined,
  };

  const data = await anilistQuery(query, variables);
  return data.Page?.media || [];
}

export async function searchAniListManga(title: string): Promise<AniListMedia[]> {
  const query = `
    query ($search: String, $page: Int, $perPage: Int) {
      Page(page: $page, perPage: $perPage) {
        media(search: $search, type: MANGA, sort: [POPULARITY_DESC]) {
          id
          idMal
          bannerImage
          format
          title { romaji english native }
          coverImage { medium large }
          chapters
          volumes
          status
          seasonYear
          genres
          averageScore
          popularity
          favourites
          trending
          countryOfOrigin
          startDate { year }
        }
      }
    }
  `;

  const data = await anilistQuery(query, {
    search: title,
    page: 1,
    perPage: 10,
  });

  return data.Page?.media || [];
}

export async function fetchAniListDiscover(filters: AniListSearchFilters = {}): Promise<AniListMedia[]> {
  const query = `
    query (
      $page: Int,
      $perPage: Int,
      $format: MediaFormat,
      $status: MediaStatus,
      $season: MediaSeason,
      $seasonYear: Int,
      $countryOfOrigin: CountryCode,
      $genreIn: [String],
      $sort: [MediaSort]
    ) {
      Page(page: $page, perPage: $perPage) {
        media(
          type: ANIME,
          format: $format,
          status: $status,
          season: $season,
          seasonYear: $seasonYear,
          countryOfOrigin: $countryOfOrigin,
          genre_in: $genreIn,
          sort: $sort
        ) {
          id
          idMal
          bannerImage
          format
          title { romaji english native }
          coverImage { medium large }
          episodes
          status
          seasonYear
          season
          genres
          averageScore
          popularity
          favourites
          trending
          countryOfOrigin
          startDate { year }
        }
      }
    }
  `;

  const variables = {
    page: filters.page || 1,
    perPage: filters.perPage || 20,
    format: filters.format,
    status: filters.status,
    season: filters.season,
    seasonYear: filters.seasonYear,
    countryOfOrigin: filters.countryOfOrigin,
    genreIn: filters.genres,
    sort: filters.sort ? [filters.sort] : ['TRENDING_DESC'],
  };

  const data = await anilistQuery(query, variables);
  return data.Page?.media || [];
}

// Upcoming anime on AniList (NOT_YET_RELEASED by popularity). Routed through
// the TatakaiAPI GraphQL proxy like every other AniList call — never directly
// from the browser (avoids CORS + shared rate budget). Replaces the retired
// direct-Jikan seasons/upcoming fetch.
export async function fetchUpcomingAniListAnime(
  page = 1,
  perPage = 12,
): Promise<AniListMedia[]> {
  const query = `
    query ($page: Int, $perPage: Int) {
      Page(page: $page, perPage: $perPage) {
        media(type: ANIME, status: NOT_YET_RELEASED, sort: [POPULARITY_DESC]) {
          id
          idMal
          format
          title { romaji english native }
          coverImage { medium large }
          episodes
          status
          seasonYear
          season
          genres
          averageScore
          popularity
          countryOfOrigin
          startDate { year month day }
          studios { nodes { name } }
        }
      }
    }
  `;

  const upcoming = await anilistQuery(query, { page, perPage });
  return upcoming.Page?.media || [];
}

export async function fetchAniListMediaById(ids: {
  anilistId?: number | null;
  malId?: number | null;
}): Promise<AniListMedia | null> {
  const anilistId = Number(ids?.anilistId);
  const malId = Number(ids?.malId);

  const hasAniListId = Number.isFinite(anilistId) && anilistId > 0;
  const hasMalId = Number.isFinite(malId) && malId > 0;
  if (!hasAniListId && !hasMalId) return null;

  const query = `
    query ($id: Int, $idMal: Int) {
      Media(id: $id, idMal: $idMal, type: ANIME) {
        id
        idMal
        bannerImage
        format
        title { romaji english native }
        coverImage { medium large }
        episodes
        status
        seasonYear
        season
        genres
        averageScore
        popularity
        favourites
        trending
        countryOfOrigin
        startDate { year }
      }
    }
  `;

  const variables = {
    id: hasAniListId ? anilistId : undefined,
    idMal: !hasAniListId && hasMalId ? malId : undefined,
  };

  const data = await anilistQuery(query, variables);
  return data?.Media || null;
}

// Disconnect integrations
export async function disconnectMAL(userId: string): Promise<void> {
  await disconnectMal(userId);
}

export async function disconnectAniList(userId: string): Promise<void> {
  const { supabase } = await import('@/integrations/supabase/client');
  const { error } = await supabase
    .from('profiles')
    .update({
      anilist_user_id: null,
      anilist_access_token: null,
      anilist_refresh_token: null,
      anilist_token_expires_at: null,
    })
    .eq('user_id', userId);

  if (error) throw error;
}

export function mapTatakaiStatusToAniList(status: string) {
  const map: Record<string, any> = {
    'watching': 'CURRENT',
    'completed': 'COMPLETED',
    'plan_to_watch': 'PLANNING',
    'dropped': 'DROPPED',
    'on_hold': 'PAUSED'
  };
  return map[status] || 'PLANNING';
}

export function mapAniListAnimeStatusToTatakai(status: string) {
  const map: Record<string, string> = {
    'CURRENT': 'watching',
    'REPEATING': 'watching',
    'COMPLETED': 'completed',
    'PLANNING': 'plan_to_watch',
    'DROPPED': 'dropped',
    'PAUSED': 'on_hold'
  };
  return map[String(status || '').toUpperCase()] || 'plan_to_watch';
}

export function mapAniListMangaStatusToTatakai(status: string) {
  const map: Record<string, string> = {
    'CURRENT': 'reading',
    'COMPLETED': 'completed',
    'PLANNING': 'plan_to_read',
    'DROPPED': 'dropped',
    'PAUSED': 'on_hold',
    'REPEATING': 'reading'
  };
  return map[String(status || '').toUpperCase()] || 'plan_to_read';
}

export function mapTatakaiMangaStatusToAniList(status: string) {
  const map: Record<string, any> = {
    'reading': 'CURRENT',
    'completed': 'COMPLETED',
    'plan_to_read': 'PLANNING',
    'dropped': 'DROPPED',
    'on_hold': 'PAUSED'
  };
  return map[String(status || '').toLowerCase()] || 'PLANNING';
}
