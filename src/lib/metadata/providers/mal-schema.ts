/**
 * MyAnimeList-shaped data → TatakaiMedia.
 *
 * Two wire formats share the same vocabulary (MAL types, statuses, genres):
 * - Jikan v4 schema, served by Tenrai (`mapJikanEntry`)
 * - Official MAL API v2 (`mapMalV2Node`)
 *
 * Field semantics follow `mapAniListMedia` (src/lib/anilist/media.ts): both
 * mappers return `anilistId: 0` + `malId`; the metadata router fills the
 * AniList id through ani.zip (src/lib/metadata/ids.ts).
 *
 * Notes on fidelity:
 * - Manga formats use the AniList enum names (MANGA / ONE_SHOT / NOVEL). The
 *   `MediaFormat` type in core/content/types.ts only lists anime formats, so
 *   they are cast, the same way AniList manga results already are.
 * - `MediaRelation.id` is the MAL id here (there is no AniList id in the payload).
 * - `rating` is the short MAL age rating ("PG-13", "R", "R+", "Rx").
 */

import type { ExternalLink, MediaRelation, MediaSeason, MediaTag, TatakaiMedia } from '@/core/content/types';
import type { MediaKind } from '@/lib/metadata/types';

export type MalSourceApi = 'tenrai' | 'mal' | 'shikimori';

const opt = <T>(v: T | null | undefined): T | undefined => (v == null || (typeof v === 'string' && v === '') ? undefined : v);

// ── Vocabulary tables ────────────────────────────────────────────────────────

const normKey = (s: string | null | undefined): string => (s || '').toLowerCase().replace(/[^a-z0-9+]/g, '');

/** MAL / Jikan anime `type` (and MAL v2 `media_type`) → AniList format. */
const ANIME_FORMAT: Record<string, TatakaiMedia['format']> = {
  tv: 'TV',
  movie: 'MOVIE',
  ova: 'OVA',
  ona: 'ONA',
  special: 'SPECIAL',
  tvspecial: 'SPECIAL',
  music: 'MUSIC',
  cm: 'SPECIAL',
  pv: 'SPECIAL',
};

/** MAL / Jikan manga `type` (and MAL v2 `media_type`) → AniList format. */
const MANGA_FORMAT: Record<string, string> = {
  manga: 'MANGA',
  manhwa: 'MANGA',
  manhua: 'MANGA',
  doujinshi: 'MANGA',
  doujin: 'MANGA',
  oel: 'MANGA',
  oneshot: 'ONE_SHOT',
  lightnovel: 'NOVEL',
  novel: 'NOVEL',
};

/** Country of origin implied by the MAL manga type (AniList reports it per entry). */
const MANGA_COUNTRY: Record<string, string> = {
  manga: 'JP',
  oneshot: 'JP',
  lightnovel: 'JP',
  novel: 'JP',
  doujinshi: 'JP',
  doujin: 'JP',
  manhwa: 'KR',
  manhua: 'CN',
};

const STATUS: Record<string, TatakaiMedia['status']> = {
  // Jikan / Shikimori-neutral human strings
  finishedairing: 'FINISHED',
  currentlyairing: 'RELEASING',
  notyetaired: 'NOT_YET_RELEASED',
  finished: 'FINISHED',
  publishing: 'RELEASING',
  notyetpublished: 'NOT_YET_RELEASED',
  onhiatus: 'HIATUS',
  discontinued: 'CANCELLED',
  // MAL v2 snake_case (normKey strips underscores)
  currentlypublishing: 'RELEASING',
};

export function mapMalStatus(status: string | null | undefined): TatakaiMedia['status'] {
  return STATUS[normKey(status)];
}

export function mapMalFormat(kind: MediaKind, type: string | null | undefined, durationMin?: number): TatakaiMedia['format'] {
  const k = normKey(type);
  if (!k) return undefined;
  if (kind === 'MANGA') return MANGA_FORMAT[k] as TatakaiMedia['format'];
  const f = ANIME_FORMAT[k];
  // AniList: television series under 15 minutes are TV_SHORT.
  if (f === 'TV' && durationMin != null && durationMin > 0 && durationMin < 15) return 'TV_SHORT';
  return f;
}

export function mapMalCountry(type: string | null | undefined): string | undefined {
  return MANGA_COUNTRY[normKey(type)];
}

const SOURCE: Record<string, TatakaiMedia['source']> = {
  original: 'ORIGINAL',
  manga: 'MANGA',
  '4komamanga': 'MANGA',
  webmanga: 'MANGA',
  digitalmanga: 'MANGA',
  novel: 'LIGHT_NOVEL',
  lightnovel: 'LIGHT_NOVEL',
  webnovel: 'LIGHT_NOVEL',
  visualnovel: 'VISUAL_NOVEL',
  game: 'VIDEO_GAME',
  videogame: 'VIDEO_GAME',
};

export function mapMalSource(source: string | null | undefined): TatakaiMedia['source'] {
  const k = normKey(source);
  if (!k) return undefined;
  return SOURCE[k] ?? 'OTHER';
}

const SEASONS: Record<string, MediaSeason> = { winter: 'WINTER', spring: 'SPRING', summer: 'SUMMER', fall: 'FALL', autumn: 'FALL' };

export function mapMalSeason(season: string | null | undefined): MediaSeason | undefined {
  return SEASONS[(season || '').toLowerCase()];
}

/** Season (Northern-hemisphere anime convention) containing `date`. */
export function seasonOfDate(date: Date): { season: MediaSeason; year: number } {
  const m = date.getUTCMonth();
  const season: MediaSeason = m <= 1 || m === 11 ? 'WINTER' : m <= 4 ? 'SPRING' : m <= 7 ? 'SUMMER' : 'FALL';
  // December belongs to the next year's winter season.
  return { season, year: m === 11 ? date.getUTCFullYear() + 1 : date.getUTCFullYear() };
}

/** "PG-13 - Teens 13 or older" → "PG-13"; MAL v2 "pg_13" → "PG-13". */
export function shortMalRating(rating: string | null | undefined): string | undefined {
  if (!rating) return undefined;
  const r = rating.trim();
  const lead = r.split(' - ')[0].trim();
  const v2: Record<string, string> = { g: 'G', pg: 'PG', pg_13: 'PG-13', r: 'R', 'r+': 'R+', rx: 'Rx' };
  return v2[lead.toLowerCase()] ?? lead;
}

export const ADULT_GENRES = new Set(['hentai', 'erotica']);

export function isAdultMal(rating: string | null | undefined, genres: string[]): boolean {
  const r = shortMalRating(rating)?.toLowerCase();
  return r === 'rx' || genres.some((g) => ADULT_GENRES.has(g.toLowerCase()));
}

const RELATION: Record<string, string> = {
  sequel: 'SEQUEL',
  prequel: 'PREQUEL',
  parentstory: 'PARENT',
  fullstory: 'PARENT',
  sidestory: 'SIDE_STORY',
  spinoff: 'SPIN_OFF',
  adaptation: 'ADAPTATION',
  alternativeversion: 'ALTERNATIVE',
  alternativesetting: 'ALTERNATIVE',
  summary: 'SUMMARY',
  character: 'CHARACTER',
  other: 'OTHER',
};

/** MAL relation name → AniList `relationType(version: 2)`. An anime's "Adaptation" of a manga is AniList's SOURCE. */
export function mapMalRelation(relation: string | null | undefined, ownKind: MediaKind, entryKind: MediaKind): string {
  const mapped = RELATION[normKey(relation)] ?? 'OTHER';
  if (mapped === 'ADAPTATION' && ownKind === 'ANIME' && entryKind === 'MANGA') return 'SOURCE';
  return mapped;
}

// ── Small parsers ────────────────────────────────────────────────────────────

/** "24 min per ep" / "1 hr 55 min" / "45 sec" → minutes. */
export function parseMalDuration(text: string | null | undefined): number | undefined {
  if (!text) return undefined;
  const hr = /(\d+)\s*hr/i.exec(text);
  const min = /(\d+)\s*min/i.exec(text);
  const sec = /(\d+)\s*sec/i.exec(text);
  const total = (hr ? Number(hr[1]) * 60 : 0) + (min ? Number(min[1]) : 0) + (!hr && !min && sec ? Math.max(1, Math.round(Number(sec[1]) / 60)) : 0);
  return total > 0 ? total : undefined;
}

type FuzzyDate = NonNullable<TatakaiMedia['startDate']>;

/** "2023-09-29" | "2023-09" | "2023" → fuzzy date. */
export function parseMalDate(text: string | null | undefined): FuzzyDate | undefined {
  if (!text) return undefined;
  const [y, m, d] = text.split('-').map(Number);
  if (!Number.isFinite(y)) return undefined;
  const out: FuzzyDate = { year: y };
  if (Number.isFinite(m)) out.month = m;
  if (Number.isFinite(d)) out.day = d;
  return out;
}

function propDate(p: { day?: number | null; month?: number | null; year?: number | null } | null | undefined): FuzzyDate | undefined {
  if (!p) return undefined;
  const out: FuzzyDate = {};
  if (p.year != null) out.year = p.year;
  if (p.month != null) out.month = p.month;
  if (p.day != null) out.day = p.day;
  return Object.keys(out).length ? out : undefined;
}

export function cleanSynopsis(text: string | null | undefined): string | undefined {
  if (!text) return undefined;
  const t = text.replace(/\s*\[Written by MAL Rewrite\]\s*/gi, '\n').trim();
  return t || undefined;
}

function uniq(values: Array<string | null | undefined>): string[] {
  const seen = new Set<string>();
  const out: string[] = [];
  for (const v of values) {
    const s = (v || '').trim();
    if (s && !seen.has(s.toLowerCase())) { seen.add(s.toLowerCase()); out.push(s); }
  }
  return out;
}

// ── Jikan v4 (Tenrai) ────────────────────────────────────────────────────────

interface JikanImages { jpg?: { image_url?: string | null; small_image_url?: string | null; large_image_url?: string | null }; webp?: { image_url?: string | null; large_image_url?: string | null } }
interface JikanNamed { mal_id: number; name: string; url?: string | null }
interface JikanDateProp { from?: { day?: number | null; month?: number | null; year?: number | null }; to?: { day?: number | null; month?: number | null; year?: number | null } }

/** Jikan v4 anime/manga object (list items and `/full`). Everything optional. */
export interface JikanEntry {
  mal_id: number;
  url?: string;
  images?: JikanImages;
  trailer?: { youtube_id?: string | null; url?: string | null } | null;
  titles?: Array<{ type?: string; title?: string }>;
  title?: string;
  title_english?: string | null;
  title_japanese?: string | null;
  title_synonyms?: string[];
  type?: string | null;
  source?: string | null;
  episodes?: number | null;
  chapters?: number | null;
  volumes?: number | null;
  status?: string | null;
  aired?: { prop?: JikanDateProp } | null;
  published?: { prop?: JikanDateProp } | null;
  duration?: string | null;
  rating?: string | null;
  score?: number | null;
  members?: number | null;
  favorites?: number | null;
  synopsis?: string | null;
  season?: string | null;
  year?: number | null;
  studios?: JikanNamed[];
  genres?: JikanNamed[];
  explicit_genres?: JikanNamed[];
  themes?: JikanNamed[];
  demographics?: JikanNamed[];
  relations?: Array<{ relation: string; entry: Array<{ mal_id: number; type: string; name: string; media_type?: string; images?: JikanImages }> }>;
  external?: Array<{ name: string; url: string }>;
  streaming?: Array<{ name: string; url: string }>;
}

const jikanCover = (img: JikanImages | undefined) => ({
  large: opt(img?.jpg?.large_image_url || img?.webp?.large_image_url || img?.jpg?.image_url),
  medium: opt(img?.jpg?.image_url || img?.jpg?.small_image_url || img?.jpg?.large_image_url),
});

function trailerOf(t: JikanEntry['trailer']): string | undefined {
  if (!t) return undefined;
  if (t.youtube_id) return `https://www.youtube.com/watch?v=${t.youtube_id}`;
  if (t.url && /youtu/.test(t.url)) return t.url;
  return undefined;
}

export function mapJikanEntry(m: JikanEntry, kind: MediaKind, sourceApi: MalSourceApi = 'tenrai'): TatakaiMedia {
  const isManga = kind === 'MANGA';
  const cover = jikanCover(m.images);
  const durationMin = isManga ? undefined : parseMalDuration(m.duration);
  const genreObjs = [...(m.genres ?? []), ...(m.explicit_genres ?? []), ...(m.themes ?? []), ...(m.demographics ?? [])];
  const genres = uniq(genreObjs.map((g) => g.name));
  const tags: MediaTag[] = [
    ...(m.themes ?? []).map((t) => ({ id: t.mal_id, name: t.name, category: 'Theme' })),
    ...(m.demographics ?? []).map((t) => ({ id: t.mal_id, name: t.name, category: 'Demographic' })),
  ];
  const dates = (isManga ? m.published : m.aired)?.prop;
  const score = typeof m.score === 'number' && m.score > 0 ? m.score : undefined;
  const synonyms = uniq([
    ...(m.title_synonyms ?? []),
    ...(m.titles ?? []).filter((t) => t.type === 'Synonym').map((t) => t.title),
  ]);

  const relations: MediaRelation[] | undefined = m.relations?.flatMap((r) =>
    r.entry
      .filter((e) => e.type === 'anime' || e.type === 'manga')
      .map((e) => {
        const entryKind: MediaKind = e.type === 'manga' ? 'MANGA' : 'ANIME';
        return {
          id: e.mal_id,
          relationType: mapMalRelation(r.relation, kind, entryKind),
          titleRomaji: e.name,
          coverImage: jikanCover(e.images).large,
          format: mapMalFormat(entryKind, e.media_type),
        };
      }),
  );

  const externalLinks: ExternalLink[] | undefined = m.external || m.streaming
    ? [
        ...(m.external ?? []).map((l) => ({ url: l.url, site: l.name, type: 'INFO' })),
        ...(m.streaming ?? []).map((l) => ({ url: l.url, site: l.name, type: 'STREAMING' })),
      ]
    : undefined;

  return {
    anilistId: 0,
    malId: m.mal_id,
    titleRomaji: m.title || m.title_english || m.title_japanese || '',
    titleEnglish: opt(m.title_english),
    titleNative: opt(m.title_japanese),
    synonyms,
    coverImageLarge: cover.large,
    coverImageMedium: cover.medium,
    bannerImage: undefined,
    format: mapMalFormat(kind, m.type, durationMin),
    status: mapMalStatus(m.status),
    season: isManga ? undefined : mapMalSeason(m.season),
    seasonYear: isManga ? undefined : opt(m.year),
    episodes: isManga ? undefined : opt(m.episodes),
    chapters: isManga ? opt(m.chapters) : undefined,
    volumes: isManga ? opt(m.volumes) : undefined,
    duration: durationMin,
    startDate: propDate(dates?.from),
    endDate: propDate(dates?.to),
    description: cleanSynopsis(m.synopsis),
    averageScore: score != null ? Math.round(score * 10) : undefined,
    meanScore: score != null ? Math.round(score * 10) : undefined,
    popularity: opt(m.members),
    favourites: opt(m.favorites),
    rating: isManga ? undefined : shortMalRating(m.rating),
    genres,
    tags,
    source: isManga ? undefined : mapMalSource(m.source),
    isAdult: isAdultMal(m.rating, genres),
    countryOfOrigin: isManga ? mapMalCountry(m.type) : undefined,
    studios: (m.studios ?? []).map((s) => ({ id: s.mal_id, name: s.name, isMain: true, siteUrl: opt(s.url) })),
    relations,
    trailerUrl: trailerOf(m.trailer),
    externalLinks,
    source_api: sourceApi,
    fetchedAt: Date.now(),
  };
}

// ── Official MAL API v2 ──────────────────────────────────────────────────────

/** Fields requested for anime lists / details (kept in one place so the mapper and the query agree). */
export const MAL_ANIME_FIELDS = [
  'id', 'title', 'main_picture', 'alternative_titles', 'start_date', 'end_date', 'synopsis', 'mean', 'rank',
  'popularity', 'num_list_users', 'nsfw', 'genres', 'media_type', 'status', 'num_episodes', 'start_season',
  'broadcast', 'source', 'average_episode_duration', 'rating', 'studios',
].join(',');
export const MAL_ANIME_DETAIL_FIELDS = `${MAL_ANIME_FIELDS},related_anime,related_manga,pictures,num_scoring_users`;
export const MAL_MANGA_FIELDS = [
  'id', 'title', 'main_picture', 'alternative_titles', 'start_date', 'end_date', 'synopsis', 'mean', 'rank',
  'popularity', 'num_list_users', 'nsfw', 'genres', 'media_type', 'status', 'num_volumes', 'num_chapters',
].join(',');
export const MAL_MANGA_DETAIL_FIELDS = `${MAL_MANGA_FIELDS},related_anime,related_manga,pictures,authors{first_name,last_name}`;

interface MalV2Related {
  node: { id: number; title: string; main_picture?: { medium?: string; large?: string } };
  relation_type?: string;
  relation_type_formatted?: string;
}

/** Official MAL API v2 anime/manga node. */
export interface MalV2Node {
  id: number;
  title: string;
  main_picture?: { medium?: string; large?: string };
  alternative_titles?: { synonyms?: string[]; en?: string; ja?: string };
  start_date?: string;
  end_date?: string;
  synopsis?: string;
  mean?: number;
  popularity?: number;
  num_list_users?: number;
  nsfw?: 'white' | 'gray' | 'black' | string;
  genres?: Array<{ id: number; name: string }>;
  media_type?: string;
  status?: string;
  num_episodes?: number;
  num_chapters?: number;
  num_volumes?: number;
  start_season?: { year?: number; season?: string };
  source?: string;
  average_episode_duration?: number;
  rating?: string;
  studios?: Array<{ id: number; name: string }>;
  related_anime?: MalV2Related[];
  related_manga?: MalV2Related[];
}

export function mapMalV2Node(m: MalV2Node, kind: MediaKind): TatakaiMedia {
  const isManga = kind === 'MANGA';
  const durationMin = m.average_episode_duration ? Math.max(1, Math.round(m.average_episode_duration / 60)) : undefined;
  const genres = uniq((m.genres ?? []).map((g) => g.name));
  const score = typeof m.mean === 'number' && m.mean > 0 ? m.mean : undefined;
  const adult = m.nsfw === 'black' || isAdultMal(m.rating, genres);

  const rel = (list: MalV2Related[] | undefined, entryKind: MediaKind): MediaRelation[] =>
    (list ?? []).map((r) => ({
      id: r.node.id,
      relationType: mapMalRelation(r.relation_type, kind, entryKind),
      titleRomaji: r.node.title,
      coverImage: opt(r.node.main_picture?.large || r.node.main_picture?.medium),
    }));
  const relations = m.related_anime || m.related_manga ? [...rel(m.related_anime, 'ANIME'), ...rel(m.related_manga, 'MANGA')] : undefined;

  return {
    anilistId: 0,
    malId: m.id,
    titleRomaji: m.title || m.alternative_titles?.en || m.alternative_titles?.ja || '',
    titleEnglish: opt(m.alternative_titles?.en),
    titleNative: opt(m.alternative_titles?.ja),
    synonyms: uniq(m.alternative_titles?.synonyms ?? []),
    coverImageLarge: opt(m.main_picture?.large || m.main_picture?.medium),
    coverImageMedium: opt(m.main_picture?.medium || m.main_picture?.large),
    bannerImage: undefined,
    format: mapMalFormat(kind, m.media_type, durationMin),
    status: mapMalStatus(m.status),
    season: isManga ? undefined : mapMalSeason(m.start_season?.season),
    seasonYear: isManga ? undefined : opt(m.start_season?.year),
    episodes: isManga ? undefined : opt(m.num_episodes) || undefined,
    chapters: isManga ? opt(m.num_chapters) || undefined : undefined,
    volumes: isManga ? opt(m.num_volumes) || undefined : undefined,
    duration: isManga ? undefined : durationMin,
    startDate: parseMalDate(m.start_date),
    endDate: parseMalDate(m.end_date),
    description: cleanSynopsis(m.synopsis),
    averageScore: score != null ? Math.round(score * 10) : undefined,
    meanScore: score != null ? Math.round(score * 10) : undefined,
    popularity: opt(m.num_list_users),
    // The v2 API exposes no favourites count.
    favourites: undefined,
    rating: isManga ? undefined : shortMalRating(m.rating),
    genres,
    tags: [],
    source: isManga ? undefined : mapMalSource(m.source),
    isAdult: adult,
    countryOfOrigin: isManga ? mapMalCountry(m.media_type) : undefined,
    studios: (m.studios ?? []).map((s) => ({ id: s.id, name: s.name, isMain: true })),
    relations,
    source_api: 'mal',
    fetchedAt: Date.now(),
  };
}
