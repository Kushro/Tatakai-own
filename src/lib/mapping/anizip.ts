/**
 * ani.zip mapping — direct client.
 *
 * Calls https://api.ani.zip/mappings straight from the renderer (through
 * `@/lib/net/publicApi`) and normalizes the raw payload here; there is no
 * TatakaiAPI hop any more. `fetchAniZipRaw` + `normalizeAniZip` are shared with
 * the ani.zip metadata provider (`@/lib/metadata/providers/anizip`).
 *
 * What ani.zip gives us, in one request:
 *
 *   1. Per-episode metadata — real episode titles in several languages, synopsis,
 *      a TheTVDB screencap, air date in both local and UTC form, and runtime.
 *      AniList's `streamingEpisodes` has a thumbnail and a title at best, and
 *      nothing at all for unaired episodes.
 *   2. Series title aliases across ~14 languages, which is what the search index
 *      needs in order to match "Ore dake Level Up na Ken" or "나 혼자만 레벨업"
 *      to Solo Leveling.
 *   3. Cross-site ids (MAL, Kitsu, AniDB, TVDB, TMDB, IMDb, AnimePlanet…).
 *
 * Episode keys are strings because specials are keyed `"S1"`, `"S2"`, … alongside
 * the numeric regular episodes — hence `key` plus a parsed `number`/`isSpecial`
 * rather than an array index.
 */

import { publicJson, PublicApiError } from "@/lib/net/publicApi";
import { clearMappingCache, loadMapping } from "./client";
import { applyEpisodeAvailability, episodeAirTime } from '../episodeAvailability';

/** Air dates change rarely but are read constantly. */
const CACHE_TTL_MS = 60 * 60 * 1000; // 1 hour

// ── Normalized shapes ─────────────────────────────────────────────────────────

export interface AniZipEpisode {
  /** Raw key from the payload — `"1"` for regulars, `"S1"` for specials. */
  key: string;
  /** Parsed episode number. For specials this is the number *within* specials. */
  number: number;
  isSpecial: boolean;
  seasonNumber: number | null;
  seasonEpisodeNumber: number | null;
  absoluteNumber: number | null;
  /** Best available title: English → romaji (`x-jat`) → Japanese. */
  title: string | null;
  /** Every localized title, keyed by language tag. */
  titles: Record<string, string>;
  overview: string | null;
  image: string | null;
  /** Local broadcast date, `YYYY-MM-DD`. */
  airDate: string | null;
  /** UTC instant of broadcast, ISO 8601. Sorting and "next episode" use this. */
  airDateUtc: string | null;
  /** Minutes. */
  runtime: number | null;
  rating: number | null;
  tvdbId: number | null;
  anidbEid: number | null;
  /** Undated regular episodes after a known upcoming episode are also unaired. */
  hasAired: boolean;
}

export interface AniZipArtwork {
  banner: string | null;
  poster: string | null;
  fanart: string | null;
  clearLogo: string | null;
}

export interface AniZipIds {
  anilistId: number | null;
  malId: number | null;
  kitsuId: number | null;
  anidbId: number | null;
  tvdbId: number | null;
  tmdbId: string | null;
  imdbId: string | null;
  animePlanetId: string | null;
  aniSearchId: number | null;
  liveChartId: number | null;
  notifyMoeId: string | null;
  type: string | null;
}

export interface AniZipMapping {
  anilistId: number;
  /** Localized series titles keyed by language tag. */
  titles: Record<string, string>;
  /**
   * Distinct series titles, best-first (English → romaji → Japanese → rest).
   * This is the alias list the search index matches against.
   */
  aliases: string[];
  preferredTitle: string | null;
  /** Regular episodes, ascending. */
  episodes: AniZipEpisode[];
  /** Specials (`S1`, `S2`, …), ascending. */
  specials: AniZipEpisode[];
  /** Regular episodes by number, for O(1) enrichment of an existing list. */
  byNumber: Map<number, AniZipEpisode>;
  episodeCount: number | null;
  specialCount: number | null;
  images: Array<{ coverType: string; url: string }>;
  artwork: AniZipArtwork;
  ids: AniZipIds;
  /** Earliest episode whose `airDateUtc` is still in the future. */
  nextEpisode: AniZipEpisode | null;
  /** Latest episode that has already aired. */
  lastAiredEpisode: AniZipEpisode | null;
}

/** What the endpoint actually sends: everything but the `Map`, which JSON can't carry. */
export type AniZipMappingWire = Omit<AniZipMapping, "byNumber">;



// ── Raw payload ───────────────────────────────────────────────────────────────

interface AniZipRawEpisode {
  tvdbShowId?: number;
  tvdbId?: number;
  seasonNumber?: number | null;
  episodeNumber?: number | null;
  absoluteEpisodeNumber?: number | null;
  title?: Record<string, string> | null;
  airDate?: string | null;
  airdate?: string | null;
  airDateUtc?: string | null;
  runtime?: number | null;
  length?: number | null;
  overview?: string | null;
  summary?: string | null;
  image?: string | null;
  episode?: string | null;
  anidbEid?: number | null;
  rating?: string | number | null;
}

/** `GET https://api.ani.zip/mappings?...` */
export interface AniZipRaw {
  titles?: Record<string, string> | null;
  episodes?: Record<string, AniZipRawEpisode> | null;
  episodeCount?: number | null;
  specialCount?: number | null;
  images?: Array<{ coverType?: string; url?: string }> | null;
  mappings?: {
    animeplanet_id?: string | null;
    kitsu_id?: number | null;
    mal_id?: number | null;
    type?: string | null;
    anilist_id?: number | null;
    anisearch_id?: number | null;
    anidb_id?: number | null;
    notifymoe_id?: string | null;
    livechart_id?: number | null;
    thetvdb_id?: number | null;
    imdb_id?: string | null;
    themoviedb_id?: string | number | null;
  } | null;
}

export interface AniZipRef {
  anilistId?: number | null;
  malId?: number | null;
  kitsuId?: number | null;
}

const num = (v: unknown): number | null => {
  const n = v == null || (typeof v === "string" && v.trim() === "") ? NaN : Number(v);
  return Number.isFinite(n) ? n : null;
};

/** ani.zip writes apostrophes as backticks ("The Journey`s End"). */
const clean = (v: unknown): string | null => {
  if (typeof v !== "string") return null;
  const t = v.replace(/`/g, "'").trim();
  return t || null;
};

function cleanTitles(raw: Record<string, string> | null | undefined): Record<string, string> {
  const out: Record<string, string> = {};
  for (const [lang, value] of Object.entries(raw ?? {})) {
    const t = clean(value);
    if (t) out[lang] = t;
  }
  return out;
}

/** English → romaji (`x-jat`) → Japanese → anything. */
function bestTitle(titles: Record<string, string>): string | null {
  return titles.en || titles["x-jat"] || titles.ja || Object.values(titles)[0] || null;
}

function normalizeEpisode(key: string, e: AniZipRawEpisode, now: number): AniZipEpisode | null {
  const special = /^S(\d+)$/i.exec(key);
  if (!special && !/^\d+$/.test(key)) return null;
  const number = special ? Number(special[1]) : Number(key);
  const titles = cleanTitles(e.title);
  const airDate = clean(e.airDate) ?? clean(e.airdate);
  const airDateUtc = clean(e.airDateUtc);
  const airTime = episodeAirTime({ airDateUtc, airDate });
  return {
    key,
    number,
    isSpecial: Boolean(special),
    seasonNumber: num(e.seasonNumber),
    seasonEpisodeNumber: num(e.episodeNumber),
    absoluteNumber: num(e.absoluteEpisodeNumber),
    title: bestTitle(titles),
    titles,
    overview: clean(e.overview) ?? clean(e.summary),
    image: clean(e.image),
    airDate,
    airDateUtc,
    runtime: num(e.runtime) ?? num(e.length),
    rating: num(e.rating),
    tvdbId: num(e.tvdbId),
    anidbEid: num(e.anidbEid),
    // Undated episodes are assumed aired; applyEpisodeAvailability corrects the
    // ones that follow a known upcoming episode.
    hasAired: airTime === null ? true : airTime <= now,
  };
}

/**
 * Derive the typed mapping from the raw ani.zip JSON. Pure (modulo `now`), shared
 * by `fetchAniZipMapping` and the ani.zip metadata provider.
 */
export function normalizeAniZip(raw: AniZipRaw, fallbackAnilistId = 0, now = Date.now()): AniZipMappingWire {
  const titles = cleanTitles(raw.titles);
  const aliases: string[] = [];
  const seen = new Set<string>();
  for (const t of [titles.en, titles["x-jat"], titles.ja, ...Object.values(titles)]) {
    if (!t) continue;
    const k = t.toLowerCase();
    if (seen.has(k)) continue;
    seen.add(k);
    aliases.push(t);
  }

  const episodes: AniZipEpisode[] = [];
  const specials: AniZipEpisode[] = [];
  for (const [key, e] of Object.entries(raw.episodes ?? {})) {
    const ep = e ? normalizeEpisode(key, e, now) : null;
    if (ep) (ep.isSpecial ? specials : episodes).push(ep);
  }
  episodes.sort((a, b) => a.number - b.number);
  specials.sort((a, b) => a.number - b.number);

  const images: Array<{ coverType: string; url: string }> = [];
  for (const img of raw.images ?? []) {
    if (img?.url) images.push({ coverType: img.coverType || "", url: img.url });
  }
  const art = (type: string) => images.find(i => i.coverType.toLowerCase() === type)?.url ?? null;

  const m = raw.mappings ?? {};
  return {
    anilistId: num(m.anilist_id) ?? fallbackAnilistId,
    titles,
    aliases,
    preferredTitle: bestTitle(titles),
    episodes,
    specials,
    episodeCount: num(raw.episodeCount),
    specialCount: num(raw.specialCount),
    images,
    artwork: { banner: art("banner"), poster: art("poster"), fanart: art("fanart"), clearLogo: art("clearlogo") },
    ids: {
      anilistId: num(m.anilist_id),
      malId: num(m.mal_id),
      kitsuId: num(m.kitsu_id),
      anidbId: num(m.anidb_id),
      tvdbId: num(m.thetvdb_id),
      tmdbId: m.themoviedb_id != null ? String(m.themoviedb_id) : null,
      imdbId: m.imdb_id ?? null,
      animePlanetId: m.animeplanet_id ?? null,
      aniSearchId: num(m.anisearch_id),
      liveChartId: num(m.livechart_id),
      notifyMoeId: m.notifymoe_id ?? null,
      type: m.type ?? null,
    },
    nextEpisode: null,
    lastAiredEpisode: null,
  };
}

// ── Fetching ──────────────────────────────────────────────────────────────────

function queryOf(ref: AniZipRef): string | null {
  if (ref.anilistId && ref.anilistId > 0) return `anilist_id=${ref.anilistId}`;
  if (ref.malId && ref.malId > 0) return `mal_id=${ref.malId}`;
  if (ref.kitsuId && ref.kitsuId > 0) return `kitsu_id=${ref.kitsuId}`;
  return null;
}

/**
 * Fetch the raw ani.zip payload for any known id. Cached and de-duplicated.
 * Resolves to null when ani.zip has no record (404). Throws on transport failure
 * (never cached), so the metadata router can fall back to another provider.
 */
export async function fetchAniZipRaw(
  ref: AniZipRef,
  options: { signal?: AbortSignal; force?: boolean } = {},
): Promise<AniZipRaw | null> {
  const query = queryOf(ref);
  if (!query) return null;
  return loadMapping<AniZipRaw>(`anizip:${query}`, async (signal) => {
    try {
      return await publicJson<AniZipRaw>(`https://api.ani.zip/mappings?${query}`, { signal });
    } catch (err) {
      if (err instanceof PublicApiError && (err.status === 404 || err.status === 400)) return null;
      throw err;
    }
  }, { ttlMs: CACHE_TTL_MS, signal: options.signal, force: options.force, rethrow: true });
}

/**
 * Fetch the ani.zip mapping for an AniList id.
 *
 * Resolves to `null` when the show is absent upstream or ani.zip is unreachable —
 * every consumer here is enrichment on top of catalog data, so a miss must
 * degrade to "no extra detail", never to a failed page render.
 */
export async function fetchAniZipMapping(
  anilistId: number | string | null | undefined,
  options: { signal?: AbortSignal; force?: boolean } = {},
): Promise<AniZipMapping | null> {
  const id = Number(anilistId);
  if (!Number.isFinite(id) || id <= 0) return null;

  let raw: AniZipRaw | null;
  try {
    raw = await fetchAniZipRaw({ anilistId: id }, options);
  } catch {
    return null;
  }
  if (!raw) return null;
  const wire = normalizeAniZip(raw, id);

  // Refresh cached date-derived flags and correct the API's undated future
  // placeholders without waiting for the upstream mapping cache to expire.
  const now = Date.now();
  const episodes = applyEpisodeAvailability(wire.episodes ?? [], undefined, now);
  const specials = applyEpisodeAvailability(wire.specials ?? [], undefined, now);
  const dated = [...episodes, ...specials].filter(episode => episodeAirTime(episode) !== null)
    .sort((a, b) => episodeAirTime(a)! - episodeAirTime(b)!);
  const aired = dated.filter(episode => episode.hasAired);

  // Rebuilt per call rather than cached: it is an index over `episodes`, which the
  // cache already holds, and building it is a single pass over a few dozen rows.
  const byNumber = new Map<number, AniZipEpisode>();
  for (const episode of episodes) byNumber.set(episode.number, episode);

  return {
    ...wire, episodes, specials, byNumber,
    nextEpisode: dated.find(episode => !episode.hasAired) ?? null,
    lastAiredEpisode: aired.length ? aired[aired.length - 1] : null,
  };
}

/** Drop cached mappings — used by manual refresh paths. */
export function clearAniZipCache(anilistId?: number): void {
  clearMappingCache(typeof anilistId === "number" ? `anizip:anilist_id=${anilistId}` : "anizip:");
}
