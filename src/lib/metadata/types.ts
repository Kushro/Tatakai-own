/**
 * Metadata provider contract.
 *
 * The app no longer depends on TatakaiAPI for catalog data. Each public source
 * (AniList, Tenrai, Kitsu, Shikimori, MyAnimeList, ani.zip) implements the
 * subset of `MetadataProvider` it can serve, and the router
 * (`@/lib/metadata/router`) calls them in the user's priority order, falling
 * back to the next provider on failure.
 *
 * Rules every provider must follow:
 * - Return `TatakaiMedia` (src/core/content/types.ts). Set `source_api` to the
 *   provider id and fill `malId` / `kitsuId` when known. Set `anilistId` when
 *   the source knows it, otherwise 0 — the router resolves it via ani.zip.
 * - Throw on transport/HTTP failure (so the router falls back). Return `null`
 *   (media) or an empty list only when the source genuinely has no result.
 * - Respect `opts.isAdult` (false = exclude adult/hentai content).
 * - Only use `publicJson`/`publicFetch` (`@/lib/net/publicApi`) or the AniList
 *   client — never `fetch` directly, never send the Supabase session.
 */

import type { ContentSearchResult, SearchFilters, TatakaiMedia } from '@/core/content/types';

export type ProviderId = 'anilist' | 'tenrai' | 'kitsu' | 'shikimori' | 'mal' | 'anizip';

export type MediaKind = 'ANIME' | 'MANGA';

/**
 * Feed lanes. `airing` = currently releasing by popularity; `recently_completed`
 * = finished most recently; `seasonal` uses opts.season/seasonYear (defaults to
 * the current season).
 */
export type MetadataFeedKind =
  | 'trending'
  | 'popular'
  | 'top_rated'
  | 'airing'
  | 'upcoming'
  | 'seasonal'
  | 'recently_completed';

export type MetadataCapability = 'feed' | 'search' | 'media' | 'episodes' | 'genres' | 'ids';

export interface FeedOptions {
  page?: number;
  perPage?: number;
  isAdult?: boolean;
  season?: 'WINTER' | 'SPRING' | 'SUMMER' | 'FALL';
  seasonYear?: number;
  /** Manga only: ISO country of origin filter (JP manga, KR manhwa, CN manhua). */
  countryOfOrigin?: 'JP' | 'KR' | 'CN' | 'TW';
  /** Manga only: restrict to novels (LIGHT_NOVEL / NOVEL formats). */
  novelsOnly?: boolean;
  /** Restrict to one genre (AniList genre vocabulary, e.g. "Action"). */
  genre?: string;
  signal?: AbortSignal;
}

export interface FeedPage {
  media: TatakaiMedia[];
  page: number;
  hasNextPage: boolean;
}

/** Identifies one title. At least one id must be set. */
export interface MediaRef {
  anilistId?: number;
  malId?: number;
  kitsuId?: number;
}

export interface MetadataEpisode {
  /** 1-based episode number (absolute within the AniList entry). */
  number: number;
  title?: string;
  titles?: Record<string, string>;
  overview?: string;
  image?: string;
  /** ISO date or datetime. */
  airDate?: string;
  runtimeMin?: number;
  isFiller?: boolean;
  isRecap?: boolean;
}

export interface MetadataProvider {
  id: ProviderId;
  /** Human label for the settings screen. */
  label: string;
  /** Short note shown in settings (limits, requirements). */
  description: string;
  /** Kinds this provider can serve for feed/search/media. */
  kinds: MediaKind[];
  /** False when the provider can't run (e.g. MAL without a client id). */
  isConfigured(): boolean;

  feed?(kind: MediaKind, feed: MetadataFeedKind, opts: FeedOptions): Promise<FeedPage>;
  search?(kind: MediaKind, filters: SearchFilters & { novelsOnly?: boolean; countryOfOrigin?: string; signal?: AbortSignal }): Promise<ContentSearchResult>;
  media?(kind: MediaKind, ref: MediaRef, opts?: { signal?: AbortSignal }): Promise<TatakaiMedia | null>;
  episodes?(ref: MediaRef, opts?: { signal?: AbortSignal }): Promise<MetadataEpisode[]>;
  genres?(kind: MediaKind, opts?: { includeAdult?: boolean; signal?: AbortSignal }): Promise<string[]>;
  /** Cross-reference ids (ani.zip, Kitsu mappings). Return null when unknown. */
  ids?(ref: MediaRef, opts?: { signal?: AbortSignal }): Promise<Required<Pick<MediaRef, 'anilistId'>> & MediaRef | null>;
}

/** The capability a provider method implements. */
export function providerSupports(p: MetadataProvider, cap: MetadataCapability, kind?: MediaKind): boolean {
  if (kind && cap !== 'episodes' && cap !== 'ids' && !p.kinds.includes(kind)) return false;
  return typeof (p as unknown as Record<string, unknown>)[cap] === 'function';
}
