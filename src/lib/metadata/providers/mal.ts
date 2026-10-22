/**
 * MyAnimeList metadata provider. Official API v2 at api.myanimelist.net/v2 with X-MAL-CLIENT-ID (VITE_MAL_CLIENT_ID). No CORS: native builds only.
 *
 * Only a public client id is sent (no user token). Without it MAL answers 403, which
 * we surface as a PublicApiError so the router cools the provider down and falls back.
 *
 * What MAL v2 cannot do (these return an empty page so the router tries the next
 * source instead of tripping the cool-down): genre-filtered feeds, `recently_completed`,
 * manga `upcoming`, filter-only search without text, episode lists, genre lists.
 * Search needs q >= 3 characters.
 */

import type { ContentSearchResult, SearchFilters, TatakaiMedia } from '@/core/content/types';
import { PublicApiError, publicJson } from '@/lib/net/publicApi';
import type { FeedOptions, FeedPage, MediaKind, MediaRef, MetadataFeedKind, MetadataProvider } from '@/lib/metadata/types';
import {
  MAL_ANIME_DETAIL_FIELDS,
  MAL_ANIME_FIELDS,
  MAL_MANGA_DETAIL_FIELDS,
  MAL_MANGA_FIELDS,
  mapMalV2Node,
  seasonOfDate,
  type MalV2Node,
} from './mal-schema';

export const MAL_API_BASE = 'https://api.myanimelist.net/v2';
const DEFAULT_LIMIT = 24;
const MAX_LIMIT = 100;

const clientId = () => String(import.meta.env.VITE_MAL_CLIENT_ID || '').trim();
const path = (kind: MediaKind) => (kind === 'ANIME' ? 'anime' : 'manga');
const limitOf = (n: number | undefined) => Math.max(1, Math.min(MAX_LIMIT, Math.floor(n || DEFAULT_LIMIT)));
const listFields = (kind: MediaKind) => (kind === 'ANIME' ? MAL_ANIME_FIELDS : MAL_MANGA_FIELDS);

interface MalList { data?: Array<{ node: MalV2Node }>; paging?: { next?: string; previous?: string } }

type Params = Record<string, string | number | boolean | undefined>;

async function malGet<T>(endpoint: string, params: Params, signal?: AbortSignal): Promise<T> {
  const id = clientId();
  if (!id) throw new PublicApiError('MyAnimeList client id missing', 403, `${MAL_API_BASE}${endpoint}`);
  const url = new URL(`${MAL_API_BASE}${endpoint}`);
  for (const [k, v] of Object.entries(params)) if (v !== undefined && v !== '') url.searchParams.set(k, String(v));
  return publicJson<T>(url.toString(), { headers: { 'X-MAL-CLIENT-ID': id }, signal });
}

function mapList(res: MalList, kind: MediaKind, adult: boolean): TatakaiMedia[] {
  const seen = new Set<number>();
  const out: TatakaiMedia[] = [];
  for (const row of res.data ?? []) {
    const node = row?.node;
    if (!node || seen.has(node.id)) continue;
    seen.add(node.id);
    let m: TatakaiMedia;
    try { m = mapMalV2Node(node, kind); } catch { continue; } // skip one malformed entry
    if (!adult && m.isAdult) continue;
    out.push(m);
  }
  return out;
}

// ── Feed ─────────────────────────────────────────────────────────────────────

const MANGA_COUNTRY_RANKING: Record<string, string> = { JP: 'manga', KR: 'manhwa', CN: 'manhua', TW: 'manhua' };

function feedRequest(kind: MediaKind, feed: MetadataFeedKind, opts: FeedOptions): { endpoint: string; params: Params } | null {
  if (opts.genre) return null; // no genre filter in the v2 API
  const base: Params = { limit: limitOf(opts.perPage), offset: ((opts.page ?? 1) - 1) * limitOf(opts.perPage), fields: listFields(kind), nsfw: opts.isAdult === true ? true : undefined };
  const k = path(kind);
  const ranking = (type: string) => ({ endpoint: `/${k}/ranking`, params: { ...base, ranking_type: type } });

  if (kind === 'MANGA') {
    const typed = opts.novelsOnly ? 'novels' : opts.countryOfOrigin ? MANGA_COUNTRY_RANKING[opts.countryOfOrigin] : undefined;
    switch (feed) {
      case 'top_rated': return ranking(typed ?? 'all');
      case 'trending': case 'popular': case 'airing': case 'seasonal': return typed ? ranking(typed) : ranking('bypopularity');
      default: return null; // upcoming / recently_completed: not available for manga
    }
  }

  const now = seasonOfDate(new Date());
  const season = (opts.season ?? now.season).toLowerCase();
  const year = opts.seasonYear ?? now.year;
  switch (feed) {
    case 'trending': return ranking('airing');
    case 'popular': return ranking('bypopularity');
    case 'top_rated': return ranking('all');
    case 'upcoming': return ranking('upcoming');
    case 'airing': {
      const cur = seasonOfDate(new Date());
      return { endpoint: `/anime/season/${cur.year}/${cur.season.toLowerCase()}`, params: { ...base, sort: 'anime_num_list_users' } };
    }
    case 'seasonal': return { endpoint: `/anime/season/${year}/${season}`, params: { ...base, sort: 'anime_num_list_users' } };
    default: return null; // recently_completed
  }
}

async function feed(kind: MediaKind, feedKind: MetadataFeedKind, opts: FeedOptions): Promise<FeedPage> {
  const page = opts.page ?? 1;
  const req = feedRequest(kind, feedKind, opts);
  if (!req) return { media: [], page, hasNextPage: false };
  const res = await malGet<MalList>(req.endpoint, req.params, opts.signal);
  return { media: mapList(res, kind, opts.isAdult === true), page, hasNextPage: Boolean(res.paging?.next) };
}

// ── Search ───────────────────────────────────────────────────────────────────

async function search(
  kind: MediaKind,
  f: SearchFilters & { novelsOnly?: boolean; countryOfOrigin?: string; signal?: AbortSignal },
): Promise<ContentSearchResult> {
  const page = f.page ?? 1;
  const limit = limitOf(f.perPage);
  const empty: ContentSearchResult = { media: [], pageInfo: { total: 0, currentPage: page, lastPage: page, hasNextPage: false, perPage: limit } };
  const q = f.query?.trim();
  if (!q || q.length < 3) return empty;

  const adult = f.isAdult === true;
  const res = await malGet<MalList>(`/${path(kind)}`, {
    q, limit, offset: (page - 1) * limit, fields: listFields(kind), nsfw: adult ? true : undefined,
  }, f.signal);
  const hasNextPage = Boolean(res.paging?.next);

  // The v2 search endpoint takes only text; apply the remaining filters to the page.
  let media = mapList(res, kind, adult);
  if (f.format?.length) { const a = new Set<string>(f.format); media = media.filter((m) => !m.format || a.has(m.format)); }
  if (f.status?.length) { const a = new Set<string>(f.status); media = media.filter((m) => !m.status || a.has(m.status)); }
  if (f.genres?.length) {
    const want = f.genres.map((g) => g.toLowerCase());
    media = media.filter((m) => want.every((g) => m.genres.some((x) => x.toLowerCase() === g || (g === 'thriller' && x === 'Suspense'))));
  }
  if (f.year?.min || f.year?.max) {
    media = media.filter((m) => {
      const y = m.startDate?.year;
      return y == null || ((!f.year!.min || y >= f.year!.min) && (!f.year!.max || y <= f.year!.max));
    });
  }
  if (f.rating?.min) media = media.filter((m) => m.averageScore == null || m.averageScore >= f.rating!.min!);
  if (kind === 'MANGA' && f.novelsOnly) media = media.filter((m) => (m.format as string) === 'NOVEL');
  if (kind === 'MANGA' && f.countryOfOrigin) media = media.filter((m) => !m.countryOfOrigin || m.countryOfOrigin === f.countryOfOrigin);

  return {
    media,
    pageInfo: { total: (page - 1) * limit + res.data!.length + (hasNextPage ? 1 : 0), currentPage: page, lastPage: hasNextPage ? page + 1 : page, hasNextPage, perPage: limit },
  };
}

// ── Detail ───────────────────────────────────────────────────────────────────

async function media(kind: MediaKind, ref: MediaRef, opts: { signal?: AbortSignal } = {}): Promise<TatakaiMedia | null> {
  if (!ref.malId) return null;
  try {
    const node = await malGet<MalV2Node>(`/${path(kind)}/${ref.malId}`, {
      fields: kind === 'ANIME' ? MAL_ANIME_DETAIL_FIELDS : MAL_MANGA_DETAIL_FIELDS,
    }, opts.signal);
    return node?.id ? mapMalV2Node(node, kind) : null;
  } catch (err) {
    if (err instanceof PublicApiError && err.status === 404) return null;
    throw err;
  }
}

export const malProvider: MetadataProvider = {
  id: 'mal',
  label: 'MyAnimeList',
  description: 'Official API. Needs a MAL client id.',
  kinds: ['ANIME', 'MANGA'],
  isConfigured: () => Boolean(clientId()),
  feed,
  search,
  media,
};
