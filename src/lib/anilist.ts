/**
 * anilist.ts - Genre helpers on top of the direct AniList client
 * (`@/lib/anilist/client`), which paces requests and retries 429s.
 *
 * Genre notes:
 *   - `GenreCollection` is a plain `[String]` list. `GenreCollection { genre }`
 *     — the shape used in `TatakaiAPI/src/providers/anilist/queries.ts` — is
 *     rejected by the API, so a client built on that query silently returns [].
 *   - It is NOT filtered by type: the same 19 genres cover anime and manga.
 *   - AniList's vocabulary also includes `Hentai`, which is mature content.
 *     `fetchAniListGenres` drops it unless `includeAdult` is set.
 */

import { ANILIST_URL, anilistGql } from '@/lib/anilist/client';

export const ANILIST_GRAPHQL_URL = ANILIST_URL;

/** The mature genre AniList exposes; excluded from the genre list by default. */
export const ADULT_GENRE = 'Hentai';

export interface AniListQueryOptions {
  variables?: Record<string, unknown>;
  /** Bypass the in-module memory cache (default: cache for 12h). */
  forceRefresh?: boolean;
  signal?: AbortSignal;
}

async function gql<TData>(query: string, options: AniListQueryOptions = {}): Promise<TData> {
  return anilistGql<TData>(query, options.variables, { signal: options.signal });
}

/** 12h: AniList only adds a genre when it promotes a tag, which is rare. */
const GENRES_TTL_MS = 12 * 60 * 60 * 1000;

let genresCache: { at: number; includeAdult: boolean; genres: string[] } | null = null;

/**
 * All genres AniList currently knows about, deduplicated and in AniList's own
 * order (Action, Adventure, Comedy, ... Thriller) — stable enough to index the
 * GenreCloud accent palette against.
 *
 * `includeAdult` is the honest way to opt into `Hentai`; callers derive it from
 * the user's content-safety setting rather than passing a literal.
 */
export async function fetchAniListGenres(includeAdult = false): Promise<string[]> {
  if (!includeAdult && genresCache?.includeAdult === false) {
    const age = Date.now() - genresCache.at;
    if (age < GENRES_TTL_MS) return genresCache.genres;
  }

  const data = await gql<{ GenreCollection: string[] }>(
    'query { GenreCollection }',
  );

  const raw = Array.isArray(data.GenreCollection) ? data.GenreCollection : [];
  const genres = [...new Set(raw.map((g) => g.trim()).filter(Boolean))];
  if (!includeAdult) {
    const adultIndex = genres.indexOf(ADULT_GENRE);
    if (adultIndex >= 0) genres.splice(adultIndex, 1);
  }

  if (!includeAdult) {
    genresCache = { at: Date.now(), includeAdult, genres };
  }
  return genres;
}
