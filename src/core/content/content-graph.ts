/**
 * src/core/content/content-graph.ts
 *
 * Browsing data for anime. Everything is served by the metadata provider router
 * (`@/lib/metadata`): AniList, Tenrai (MAL), Kitsu, Shikimori, MAL and ani.zip,
 * called directly from the client in the user's priority order. There is no
 * TatakaiAPI backend any more.
 *
 * The public API (method names, signatures, return types) is unchanged, so the
 * pages and hooks that call `contentGraph.*` keep working.
 */

import { queryCache, TTL } from '../cache/query-cache';
import { db, type CachedMedia } from '../db';
import { metadata } from '@/lib/metadata';
import type { FeedOptions, MediaRef, MetadataFeedKind } from '@/lib/metadata';
import { tenraiGet } from '@/lib/tenrai/client';
import type {
  TatakaiMedia,
  SearchFilters,
  ContentSearchResult,
  ContentFeed,
  HomePageBundle,
  MediaSeason,
  FeedType,
} from './types';

/** Dexie `cachedMedia` rows older than this are ignored (and refetched). */
export const CACHED_MEDIA_MAX_AGE_MS = 24 * 60 * 60 * 1000;

const SPOTLIGHT_SIZE = 10;
const TOP10_SIZE = 10;
const HOME_LANE_SIZE = 20;

/**
 * @deprecated The TatakaiAPI content endpoints no longer exist. Kept only so the
 * `@/core` barrel export keeps compiling; always rejects.
 */
export async function apiGet<T>(
  path: string,
  _params?: Record<string, string | number | boolean | undefined>,
): Promise<T> {
  throw new Error(`TatakaiAPI is no longer available (${path}). Use the metadata router instead.`);
}

// ─── Id parsing ───────────────────────────────────────────────────────────────

const UUID_RE = /^[0-9a-f]{8}-[0-9a-f]{4}-[1-5][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i;

/**
 * Turn the id forms the app uses into a router ref.
 * - `123`, `anilist:123`, `anilist-123`, `anilist_123` → AniList id
 * - `mal:123`, `mal-123` → MAL id
 * - UUID (legacy TatakaiAPI ids) → null (cannot be resolved without the backend)
 */
export function parseMediaId(rawId: string): { ref: MediaRef; alt?: MediaRef } | null {
  const id = String(rawId ?? '').trim();
  if (!id || UUID_RE.test(id)) return null;
  const prefixed = id.match(/^(mal|anilist)[-_:]?(\d+)$/i);
  if (prefixed) {
    const n = Number(prefixed[2]);
    if (!(n > 0)) return null;
    return prefixed[1].toLowerCase() === 'mal' ? { ref: { malId: n } } : { ref: { anilistId: n } };
  }
  const n = Number(id);
  if (Number.isInteger(n) && n > 0) {
    // Bare numbers are AniList ids; a MAL id is the last-resort interpretation.
    return { ref: { anilistId: n }, alt: { malId: n } };
  }
  return null;
}

// ─── Content Graph ────────────────────────────────────────────────────────────

class ContentGraph {

  // ── Internal cache wrapper ────────────────────────────────────────────────

  private async cached<T>(
    key: string,
    ttlMs: number,
    fetcher: () => Promise<T>,
  ): Promise<T> {
    const hit = queryCache.get<T>(key);
    if (hit !== null) return hit;

    const result = await fetcher();
    queryCache.set(key, result, ttlMs);

    // Background task: If the result contains media, save to local DB
    if (result && typeof result === 'object') {
      if ('spotlight' in result && 'trending' in result) {
        // It's a HomePageBundle
        const bundle = result as unknown as HomePageBundle;
        this.saveManyToDb([
          ...(bundle.spotlight || []),
          ...(bundle.trending || []),
          ...(bundle.topAiring || []),
          ...(bundle.popular || []),
          ...(bundle.topUpcoming || []),
          ...(bundle.latestCompleted || []),
          ...(bundle.top10?.today || []),
          ...(bundle.top10?.week || []),
          ...(bundle.top10?.month || []),
        ]);
      } else if ('media' in result && Array.isArray((result as { media: unknown }).media)) {
        this.saveManyToDb((result as { media: TatakaiMedia[] }).media);
      } else if (Array.isArray(result)) {
        this.saveManyToDb((result as unknown[]).filter(isMedia));
      } else if (isMedia(result)) {
        this.saveToDb(result);
      }
    }

    return result;
  }

  private async saveToDb(media: TatakaiMedia) {
    if (!media?.anilistId) return;
    try {
      const cached: CachedMedia = {
        id: String(media.anilistId),
        anilistId: media.anilistId,
        malId: media.malId,
        title: media.titleEnglish || media.titleRomaji,
        titleJp: media.titleNative,
        poster: media.coverImageLarge,
        genres: media.genres,
        format: media.format,
        status: media.status,
        season: media.season,
        seasonYear: media.seasonYear,
        cachedAt: new Date().toISOString(),
        raw: JSON.stringify(media),
      };
      await db.cachedMedia.put(cached);
    } catch (err) {
      console.warn('[ContentGraph] Failed to save media to DB:', err);
    }
  }

  private async saveManyToDb(medias: TatakaiMedia[]) {
    for (const m of medias) {
      this.saveToDb(m);
    }
  }

  // ── Router helpers ────────────────────────────────────────────────────────

  private async feedPage(
    feed: MetadataFeedKind,
    page: number,
    perPage: number,
    extra: Partial<FeedOptions> = {},
  ): Promise<TatakaiMedia[]> {
    const { value } = await metadata.feed('ANIME', feed, { page, perPage, ...extra });
    return value.media;
  }

  // ── Home page (composed from parallel feeds) ──────────────────────────────

  /**
   * Builds the home bundle from the router's feeds, fetched in parallel.
   * - spotlight: first trending titles that have a banner (topped up with the
   *   rest of trending when there are too few banners)
   * - top10.today / week / month are approximations (no per-period ranking is
   *   available from public APIs): trending / airing / popular
   * A lane that fails comes back empty; the call only fails when every lane does.
   */
  async getHomePage(): Promise<HomePageBundle> {
    return this.cached('home:bundle', TTL.HOT, async () => {
      const lane = (feed: MetadataFeedKind) => this.feedPage(feed, 1, HOME_LANE_SIZE);
      const results = await Promise.allSettled([
        lane('trending'),
        lane('airing'),
        lane('popular'),
        lane('upcoming'),
        lane('recently_completed'),
        metadata.genres('ANIME'),
      ]);
      const [trendingR, airingR, popularR, upcomingR, completedR, genresR] = results;
      const ok = <T>(r: PromiseSettledResult<T>, fallback: T): T => (r.status === 'fulfilled' ? r.value : fallback);

      const trending = ok(trendingR, []);
      const topAiring = ok(airingR, []);
      const popular = ok(popularR, []);
      const topUpcoming = ok(upcomingR, []);
      const latestCompleted = ok(completedR, []);
      const lanes = [trending, topAiring, popular, topUpcoming, latestCompleted];

      if (lanes.every((l) => l.length === 0)) {
        const failed = results.find((r): r is PromiseRejectedResult => r.status === 'rejected');
        throw failed?.reason instanceof Error ? failed.reason : new Error('Home feeds returned no results');
      }

      // If trending is unavailable, spotlight/top-10 fall back to the next best lane.
      const hero = trending.length ? trending : topAiring.length ? topAiring : popular;
      const withBanner = hero.filter((m) => m.bannerImage);
      const spotlight = (withBanner.length >= SPOTLIGHT_SIZE
        ? withBanner
        : [...withBanner, ...hero.filter((m) => !m.bannerImage)]
      ).slice(0, SPOTLIGHT_SIZE);

      return {
        spotlight,
        trending,
        topAiring,
        popular,
        topUpcoming,
        latestCompleted,
        top10: {
          today: hero.slice(0, TOP10_SIZE),
          week: (topAiring.length ? topAiring : hero).slice(0, TOP10_SIZE),
          month: (popular.length ? popular : hero).slice(0, TOP10_SIZE),
        },
        genres: ok(genresR, { value: [] as string[], provider: 'anilist' as const }).value,
        fetchedAt: Date.now(),
      };
    });
  }

  // ── Feeds ─────────────────────────────────────────────────────────────────

  async getTrending(page = 1, perPage = 20): Promise<ContentFeed> {
    return this.cached(`feed:trending:${page}:${perPage}`, TTL.HOT, async () => ({
      type: 'trending' as const, media: await this.feedPage('trending', page, perPage), fetchedAt: Date.now(),
    }));
  }

  async getPopular(page = 1, perPage = 20): Promise<ContentFeed> {
    return this.cached(`feed:popular:${page}:${perPage}`, TTL.WARM, async () => ({
      type: 'popular' as const, media: await this.feedPage('popular', page, perPage), fetchedAt: Date.now(),
    }));
  }

  async getTopRated(page = 1, perPage = 20): Promise<ContentFeed> {
    return this.cached(`feed:top_rated:${page}:${perPage}`, TTL.WARM, async () => ({
      type: 'top_rated' as const, media: await this.feedPage('top_rated', page, perPage), fetchedAt: Date.now(),
    }));
  }

  async getSeasonal(
    season?: MediaSeason,
    year?: number,
    page = 1,
    perPage = 20,
  ): Promise<ContentFeed> {
    const s = season ?? getCurrentSeason().season;
    const y = year ?? getCurrentSeason().year;
    return this.cached(`feed:seasonal:${s}:${y}:${page}:${perPage}`, TTL.HOT, async () => ({
      type: 'seasonal' as const,
      media: await this.feedPage('seasonal', page, perPage, { season: s, seasonYear: y }),
      season: s,
      seasonYear: y,
      fetchedAt: Date.now(),
    }));
  }

  async getUpcoming(page = 1, perPage = 20): Promise<ContentFeed> {
    return this.cached(`feed:upcoming:${page}:${perPage}`, TTL.HOT, async () => ({
      type: 'upcoming' as const, media: await this.feedPage('upcoming', page, perPage), fetchedAt: Date.now(),
    }));
  }

  async getFeed(type: FeedType, page = 1, perPage = 20): Promise<ContentFeed> {
    switch (type) {
      case 'trending':        return this.getTrending(page, perPage);
      case 'popular':         return this.getPopular(page, perPage);
      case 'top_rated':       return this.getTopRated(page, perPage);
      case 'seasonal':        return this.getSeasonal(undefined, undefined, page, perPage);
      case 'upcoming':        return this.getUpcoming(page, perPage);
      case 'recently_updated': return this.getTrending(page, perPage); // fallback
      default:                return this.getTrending(page, perPage);
    }
  }

  // ── Search ────────────────────────────────────────────────────────────────

  async search(filters: SearchFilters): Promise<ContentSearchResult> {
    const cacheKey = `search:${JSON.stringify(filters)}`;
    return this.cached(cacheKey, TTL.HOT, async () => {
      const { value } = await metadata.search('ANIME', filters);
      return value;
    });
  }

  // ── Single item lookup ────────────────────────────────────────────────────

  async getMedia(id: string): Promise<TatakaiMedia> {
    const rawId = String(id ?? '').trim();
    if (!rawId) throw new Error(`Invalid media ID: ${id}`);

    // Verbose tracing: `localStorage.tatakaiMediaDebug = '1'`.
    const debugOn = (() => {
      try { return localStorage.getItem('tatakaiMediaDebug') === '1'; } catch { return false; }
    })();
    const diag = (branch: string, extra?: Record<string, unknown>) => {
      if (debugOn) console.debug('[getMedia]', { rawId, branch, ...extra });
    };

    // 1. Memory cache
    const memHit = queryCache.get<TatakaiMedia>(`media:resolve:${rawId}`);
    if (memHit) { diag('mem-cache-hit'); return memHit; }

    const parsed = parseMediaId(rawId);
    if (!parsed) {
      // Legacy TatakaiAPI UUIDs (and junk) can't be resolved without the backend.
      diag('unresolvable');
      throw new Error(`Invalid media ID: ${id}`);
    }

    // 2. Local DB (Dexie), ignoring rows older than CACHED_MEDIA_MAX_AGE_MS
    try {
      const local = parsed.ref.anilistId
        ? await db.cachedMedia.get(String(parsed.ref.anilistId))
        : await db.cachedMedia.where('malId').equals(parsed.ref.malId!).first();
      const age = local?.cachedAt ? Date.now() - Date.parse(local.cachedAt) : Infinity;
      if (local?.raw && age < CACHED_MEDIA_MAX_AGE_MS) {
        const media = JSON.parse(local.raw) as TatakaiMedia;
        queryCache.set(`media:resolve:${rawId}`, media, TTL.WARM);
        diag('dexie-hit');
        return media;
      }
    } catch (err) {
      console.warn('[ContentGraph] DB lookup failed:', err);
    }

    // 3. Router
    return this.cached(`media:resolve:${rawId}`, TTL.COLD, async () => {
      let found = (await metadata.media('ANIME', parsed.ref)).value;
      if (!found && parsed.alt) found = (await metadata.media('ANIME', parsed.alt)).value;
      if (!found) throw new Error(`Media not found: ${id}`);
      diag('router', { anilistId: found.anilistId });
      return found;
    });
  }

  // ── Episodes (via the metadata router: ani.zip / Tenrai / Kitsu / Shikimori) ──

  async getDbEpisodes(id: string): Promise<Array<{
    episodeNumber: number;
    title?: string | null;
    isFiller?: boolean;
  }>> {
    const parsed = parseMediaId(id);
    if (!parsed) return [];
    try {
      const { value } = await metadata.episodes(parsed.ref);
      return value
        .map((e) => ({ episodeNumber: Number(e.number), title: e.title ?? null, isFiller: Boolean(e.isFiller) }))
        .filter((row) => row.episodeNumber > 0);
    } catch {
      return [];
    }
  }

  /**
   * Episodes for a MAL id in the old paged-Jikan shape. The router returns the
   * whole list at once, so page 1 carries everything and `hasNextPage` is false.
   */
  async getMalEpisodesPage(malId: number, page = 1): Promise<{
    episodes: Array<{ mal_id?: number; title?: string | null; filler?: boolean }>;
    hasNextPage: boolean;
  }> {
    if (page > 1) return { episodes: [], hasNextPage: false };
    const { value } = await metadata.episodes({ malId });
    return {
      episodes: value.map((e) => ({ mal_id: e.number, title: e.title ?? null, filler: Boolean(e.isFiller) })),
      hasNextPage: false,
    };
  }

  /** MAL broadcast schedule (Jikan-compatible `/anime/{id}/full` from Tenrai). */
  async getMalAnimeFull(malId: number): Promise<{
    broadcast?: {
      day?: string | null;
      time?: string | null;
      timezone?: string | null;
    } | null;
  } | null> {
    try {
      const res = await tenraiGet<{ data?: { broadcast?: { day?: string | null; time?: string | null; timezone?: string | null } | null } }>(
        `/anime/${malId}/full`,
      );
      return { broadcast: res?.data?.broadcast ?? null };
    } catch {
      return null;
    }
  }

  // ── Genres ────────────────────────────────────────────────────────────────

  async getGenres(): Promise<string[]> {
    return this.cached('genres', TTL.COLD, async () => (await metadata.genres('ANIME')).value);
  }

  // ── Live next-airing ──────────────────────────────────────────────────────

  /**
   * Live next-episode schedule for an anime, straight from the router (never the
   * local cache). `nextAiringEpisode` is only present when the answering source
   * (AniList) knows it. Returns null for unknown titles or on error.
   */
  async getNextAiring(anilistId: number): Promise<{
    status: string | null;
    episodes: number | null;
    nextAiringEpisode: { episode: number; airingAt: number; timeUntilAiring: number } | null;
  } | null> {
    if (!Number.isFinite(anilistId) || anilistId <= 0) return null;
    try {
      const { value } = await metadata.media('ANIME', { anilistId });
      if (!value) return null;
      return {
        status: value.status ?? null,
        episodes: value.episodes ?? null,
        nextAiringEpisode: value.nextAiringEpisode ?? null,
      };
    } catch {
      return null;
    }
  }

  // ── Cache management ──────────────────────────────────────────────────────

  invalidateAll() {
    queryCache.clear();
  }

  invalidateFeed(type: FeedType) {
    queryCache.invalidatePrefix(`feed:${type}:`);
  }

  invalidateMedia(anilistId: number) {
    queryCache.invalidatePrefix('media:');
    void anilistId;
  }

  invalidateHome() {
    queryCache.delete('home:bundle');
  }

  /**
   * Drop every cached copy of a single media item so the next getMedia()
   * refetches it. Coarse on the memory cache (clears all `media:` resolves).
   */
  async purgeMedia(id: string) {
    queryCache.invalidatePrefix('media:');
    try {
      await db.cachedMedia.delete(id);
      const numeric = Number(id);
      if (Number.isFinite(numeric) && numeric > 0) {
        await db.cachedMedia.where('anilistId').equals(numeric).delete();
      }
    } catch (err) {
      console.warn('[ContentGraph] purgeMedia failed:', err);
    }
  }
}

function isMedia(v: unknown): v is TatakaiMedia {
  return Boolean(v) && typeof v === 'object' && typeof (v as TatakaiMedia).anilistId === 'number' && 'titleRomaji' in (v as object);
}

// ─── Singleton ────────────────────────────────────────────────────────────────

export const contentGraph = new ContentGraph();

// ─── Utility ─────────────────────────────────────────────────────────────────

export function getCurrentSeason(): { season: MediaSeason; year: number } {
  const now = new Date();
  const month = now.getMonth() + 1;
  const year = now.getFullYear();
  const season: MediaSeason =
    month <= 3 ? 'WINTER' : month <= 6 ? 'SPRING' : month <= 9 ? 'SUMMER' : 'FALL';
  return { season, year };
}
