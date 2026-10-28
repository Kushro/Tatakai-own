import { contentGraph, toAnimeCard, toHomeData } from "@/core";
import { applyEpisodeAvailability } from '@/lib/episodeAvailability';
import type { EpisodeData, EpisodeServer, HomeData, StreamingData } from "@/types/anime";
import { getHighQualityImage } from "@/lib/api/proxy-utils";
import { metadata } from "@/lib/metadata";
import { publicJson } from "@/lib/net/publicApi";
import { tenraiGet } from "@/lib/tenrai/client";
import { fetchAniZipMapping } from "@/lib/mapping/anizip";
import { resolveApiV3Base } from "@/lib/api/backendOrigin";
import { withLocaleHeaders } from "@/lib/api/localeHeaders";

// Legacy backend `/api/v3` base, still read by admin/status/sync screens that have
// not been migrated. Catalog data no longer uses it.
export const TATAKAI_API_URL = resolveApiV3Base();

const GENRE_ALIASES: Record<string, string> = {
  "slice-of-life": "Slice of Life",
  "sci-fi": "Sci-Fi",
};

function normalizeGenre(genre: string): string {
  const key = String(genre || "").trim().toLowerCase();
  if (!key) return "";
  if (GENRE_ALIASES[key]) return GENRE_ALIASES[key];
  return key.replace(/-/g, " ").replace(/\b\w/g, (char) => char.toUpperCase());
}

/** Locale headers only; no `X-Tatakai-*` identification is sent to any host. */
export function withClientHeaders(headers: Record<string, string> = {}) {
  return withLocaleHeaders({ ...headers });
}

function resolveExternalApiUrl(baseUrl: string, path: string): string {
  const rawPath = String(path || "").trim();
  if (!rawPath) return String(baseUrl || "").trim();

  if (/^[a-z][a-z0-9+.-]*:\/\//i.test(rawPath)) {
    return rawPath;
  }

  const base = new URL(
    baseUrl,
    typeof window !== "undefined" ? window.location.origin : "http://localhost"
  );
  const basePath = base.pathname.replace(/\/$/, "");
  const pathPart = rawPath.replace(/^\/+/, "");
  const joinedPath = basePath ? `${basePath}/${pathPart}` : `/${pathPart}`;

  return `${base.origin}${joinedPath}`;
}

/**
 * GET a JSON public API (Tenrai/Jikan-compatible, etc.). Goes through the shared
 * public transport (host pacing, retries, native HTTP in the apps); never attaches
 * app credentials or Tatakai identification headers.
 * @param baseUrl - The base URL of the external API (e.g., 'https://api.tenrai.org/v1')
 * @param path - The path to append (e.g., '/anime/1')
 */
export async function externalApiGet<T>(
  baseUrl: string,
  path: string,
  headers: Record<string, string> = {}
): Promise<T> {
  const url = resolveExternalApiUrl(baseUrl, path);
  try {
    return await publicJson<T>(url, { headers });
  } catch (error) {
    console.error(`[externalApiGet] Error fetching ${url}:`, error);
    throw error;
  }
}

/** Legacy adapter for fetchHome */
export async function fetchHome(): Promise<HomeData> {
  const bundle = await contentGraph.getHomePage();
  return toHomeData(bundle);
}

/** Legacy adapter for fetchAnimeInfo */
export async function fetchAnimeInfo(id: string) {
  return contentGraph.getMedia(id);
}

/** Legacy adapter for searchAnime */
export async function searchAnime(query: string, page: number = 1, filters: any = {}) {
  return contentGraph.search({
    query,
    page,
    ...filters,
  });
}

/** Legacy adapter for fetchGenreAnimes */
export async function fetchGenreAnimes(genre: string, page: number = 1) {
  const normalizedGenre = normalizeGenre(genre);
  const result = await contentGraph.search({
    genres: normalizedGenre ? [normalizedGenre] : undefined,
    page,
    perPage: 24,
    sortBy: "TRENDING_DESC",
  });

  return {
    genreName: normalizedGenre || genre,
    animes: result.media.map(toAnimeCard),
    pageInfo: result.pageInfo,
  };
}

/** Legacy adapter for producer studio pages */
export async function fetchProducerAnimes(producer: string, page: number = 1) {
  const result = await contentGraph.search({
    studio: [String(producer || "").trim()],
    page,
    perPage: 24,
    sortBy: "POPULARITY_DESC",
  });

  return {
    producerName: producer,
    animes: result.media.map(toAnimeCard),
    top10Animes: { today: [], week: [], month: [] },
    topAiringAnimes: [],
    currentPage: result.pageInfo.currentPage,
    totalPages: result.pageInfo.lastPage,
    hasNextPage: result.pageInfo.hasNextPage,
  };
}

/** Legacy helper for obtaining a larger poster image when MAL ID is known */
export async function fetchJikanCover(malId?: number | null, fallbackPoster = ""): Promise<string> {
  if (!malId) return fallbackPoster;

  try {
    const json = await tenraiGet<{
      data?: { images?: { webp?: { large_image_url?: string }; jpg?: { large_image_url?: string; image_url?: string } } };
    }>(`/anime/${malId}`);
    const imageUrl =
      json?.data?.images?.webp?.large_image_url ||
      json?.data?.images?.jpg?.large_image_url ||
      json?.data?.images?.jpg?.image_url;
    return String(imageUrl || fallbackPoster || "");
  } catch {
    return fallbackPoster;
  }
}

/**
 * Known future dates and episode-order/schedule evidence set `hasAired` false.
 *
 * Episodes from the router/count fallbacks carry no air-date signal
 * (`hasAired` undefined) remain playable unless a known upcoming episode puts
 * them beyond the current airing boundary. Never invert this to `!== true`: that would
 * brick playback for every fallback-tier episode.
 */
export function isEpisodeUnaired(
  episode: { hasAired?: boolean } | null | undefined,
): boolean {
  return !!episode && episode.hasAired === false;
}

/** Legacy adapter for fetchEpisodes */
export async function fetchEpisodes(animeId: string) {
  // In v6, episodes are usually part of the media object or fetched via content graph
  const media = await contentGraph.getMedia(animeId);
  const baseId = media.tatakaiId || animeId;
  let episodes: EpisodeData[] = [];

  // ani.zip leads for the same reason it does in `useEpisodes`: AniList's
  // `streamingEpisodes` is regularly a neighbouring season's Crunchyroll listing
  // stored newest-first, so its array order is not this season's episode order.
  const mapping = await fetchAniZipMapping(media.anilistId).catch(() => null);
  if (mapping) {
    episodes = mapping.episodes.map((entry) => ({
      episodeId: `${baseId}?ep=${entry.number}`,
      number: entry.number,
      title: entry.title || `Episode ${entry.number}`,
      isFiller: false,
      overview: entry.overview ?? undefined,
      image: entry.image ?? undefined,
      airDate: entry.airDate ?? undefined,
      airDateUtc: entry.airDateUtc ?? undefined,
      runtime: entry.runtime ?? undefined,
      hasAired: entry.hasAired,
    }));
  }

  if (episodes.length === 0 && (media.anilistId || media.malId)) {
    try {
      const { value } = await metadata.episodes({
        anilistId: media.anilistId > 0 ? media.anilistId : undefined,
        malId: media.malId,
        kitsuId: media.kitsuId,
      });
      for (const row of value) {
        if (!Number.isFinite(row.number) || row.number <= 0) continue;
        episodes.push({
          episodeId: `${baseId}?ep=${row.number}`,
          number: row.number,
          title: row.title || `Episode ${row.number}`,
          isFiller: Boolean(row.isFiller),
          overview: row.overview,
          image: row.image,
          airDate: row.airDate,
          runtime: row.runtimeMin,
        });
      }
    } catch {
      /* no source has an episode list: fall back to count placeholders */
    }
  }

  if (episodes.length === 0) {
    const count = Math.min(media.episodes ?? 0, 2000);
    episodes = Array.from({ length: count }, (_, i) => ({
      episodeId: `${baseId}?ep=${i + 1}`,
      number: i + 1,
      title: `Episode ${i + 1}`,
      isFiller: false,
    }));
  }

  return {
    episodes: applyEpisodeAvailability(episodes, media.nextAiringEpisode),
    totalEpisodes: mapping?.episodeCount ?? media.episodeSubCount ?? media.episodes ?? episodes.length,
  };
}

type PlaybackSourceOptions = {
  animeName?: string;
  anilistId?: number | string | null;
  currentUserId?: string;
  knownAnilistId?: number | string | null;
  knownMalId?: number | string | null;
};

function groupEpisodeServers(providerServers: EpisodeServer[] | undefined | null) {
  const grouped = {
    sub: [] as EpisodeServer[],
    dub: [] as EpisodeServer[],
  };

  for (const server of providerServers || []) {
    const serverLabel = [server.language, server.displayName, server.serverName, server.providerName]
      .filter(Boolean)
      .join(" ")
      .toLowerCase();
    const isDub = Boolean(server.isDub) || serverLabel.includes("dub");
    grouped[isDub ? "dub" : "sub"].push(server);
  }

  return grouped;
}

/** Legacy adapter for fetchEpisodeServers */
export async function fetchEpisodeServers(episodeId: string) {
  const combined = await fetchCombinedSources(episodeId, undefined, undefined, "hd-1", "sub");
  return groupEpisodeServers(combined.providerServers);
}

/** Legacy adapter for fetchStreamingSources */
export async function fetchStreamingSources(
  episodeId: string,
  server: string = "hd-2",
  category: string = "sub",
  options: PlaybackSourceOptions = {}
) {
  return fetchCombinedSources(
    episodeId,
    options.animeName,
    undefined,
    server,
    category,
    options.currentUserId,
    options.knownAnilistId ?? options.anilistId,
    options.knownMalId
  );
}

/** Legacy adapter for fetchTatakaiEpisodeSources */
export async function fetchTatakaiEpisodeSources(
  episodeId: string,
  server: string = "hd-2",
  category: string = "sub",
  options: PlaybackSourceOptions = {}
) {
  return fetchCombinedSources(
    episodeId,
    options.animeName,
    undefined,
    server,
    category,
    options.currentUserId,
    options.knownAnilistId ?? options.anilistId,
    options.knownMalId
  );
}

/** Legacy adapter for fetchNextEpisodeSchedule */
export async function fetchNextEpisodeSchedule(animeId: string) {
  return null;
}

/** Central dispatch removed: source resolution is extension-local now (desktop
 *  runtime, mobile in-WebView runtime). This stays as the shared shape so all
 *  callers keep compiling; it resolves nothing over the network. */
export async function fetchCombinedSources(
  episodeId: string | undefined,
  animeName: string | undefined,
  episodeNumber: number | undefined,
  server: string = "hd-2",
  category: string = "sub",
  currentUserId?: string,
  knownAnilistId?: number | string | null,
  knownMalId?: number | string | null
): Promise<StreamingData> {
  void episodeId;
  void animeName;
  void episodeNumber;
  void server;
  void category;
  void currentUserId;

  const anilistID =
    typeof knownAnilistId === "string" ? Number(knownAnilistId) || null :
    typeof knownAnilistId === "number" ? knownAnilistId : null;
  const malID =
    typeof knownMalId === "string" ? Number(knownMalId) || null :
    typeof knownMalId === "number" ? knownMalId : null;

  return {
    headers: { Referer: "", "User-Agent": "" },
    sources: [],
    subtitles: [],
    tracks: [],
    providerServers: [],
    anilistID,
    malID,
  };
}

/**
 * Unwrap a TatakaiAPI envelope.
 *
 * The envelope is `{ success, data, meta?, error? }` (see `TatakaiAPI/src/lib/envelope.ts`).
 * This used to test `res.status === "ok"`, a field the API has never sent, so it
 * threw "API Error" on every response — successful ones included.
 */
export function unwrapApiData<T>(res: any): T {
  if (res && typeof res === "object") {
    if (res.success === true) return res.data as T;
    if (res.success === false) throw new Error(res.error || res.message || "API Error");
    // Bare payload (some legacy endpoints return data with no envelope).
    if (!("success" in res)) return res as T;
  }
  throw new Error("API Error");
}

// Re-export image quality helpers for convenience
export { getHighQualityImage, getHighQualityPoster, getProxiedImageUrl } from "@/lib/api/proxy-utils";
