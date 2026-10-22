import { anilistGql, AniListError } from '@/lib/anilist/client';
import type { MangaSearchItem, MangaSearchResult } from '@/types/manga';
import type { MangaSearchOptions } from './manga-client';

export interface MangaTagOption {
  id: number;
  name: string;
  description?: string | null;
  category?: string | null;
  isAdult?: boolean;
  isGeneralSpoiler?: boolean;
}

const tagCache = new Map<string, Promise<MangaTagOption[]>>();

/** Tag browsing is AniList-only; MangaBaka tagging needed the removed backend. */
async function aniList<T>(query: string, variables: Record<string, unknown> = {}): Promise<T> {
  const data = await anilistGql<T>(query, variables);
  if (!data) throw new Error('Catalog returned no data. Please retry.');
  return data;
}

export function getMangaTagOptions(source: 'anilist' | 'mangabaka'): Promise<MangaTagOption[]> {
  const cached = tagCache.get(source);
  if (cached) return cached;
  const request: Promise<MangaTagOption[]> = source === 'anilist'
    ? aniList<{ MediaTagCollection: MangaTagOption[] }>('query { MediaTagCollection { id name description category isAdult isGeneralSpoiler } }')
      .then(data => data.MediaTagCollection)
      // While AniList is unavailable tag browsing degrades to "no tags" (not cached).
      .catch(err => { if (err instanceof AniListError) { tagCache.delete(source); return []; } throw err; })
    : Promise.resolve([]); // MangaBaka tags were served by the removed TatakaiAPI mapping endpoint.
  tagCache.set(source, request);
  void request.catch(() => tagCache.delete(source));
  return request;
}

interface AniListMedia {
  id: number; idMal?: number; title: { romaji?: string; english?: string; native?: string };
  coverImage?: { large?: string }; format?: string; countryOfOrigin?: string;
  status?: string; startDate?: { year?: number }; averageScore?: number; popularity?: number;
  isAdult?: boolean; chapters?: number; volumes?: number;
}

/** AniList being down yields an empty, partial result instead of an error. */
export async function searchMangaTags(query: string, page: number, limit: number, options: MangaSearchOptions): Promise<MangaSearchResult> {
  try {
    return await searchMangaTagsUnchecked(query, page, limit, options);
  } catch (err) {
    if (!(err instanceof AniListError)) throw err;
    return { query, page, currentPage: page, limit, partial: true, failedProviders: ['anilist'], source: 'anilist',
      results: [], total: 0, totalIsApproximate: true, hasNextPage: false };
  }
}

async function searchMangaTagsUnchecked(query: string, page: number, limit: number, options: MangaSearchOptions): Promise<MangaSearchResult> {
  const names = [...new Set(options.tags || [])];
  const normalize = (name: string) => name.trim().toLocaleLowerCase();
  let source = options.tagSource;
  if (!source) {
    const tags = await getMangaTagOptions('anilist');
    source = names.every(name => tags.some(tag => normalize(tag.name) === normalize(name))) ? 'anilist' : 'mangabaka';
  }
  const taxonomy = await getMangaTagOptions(source);
  const tags = names.map(name => {
    const tag = taxonomy.find(entry => normalize(entry.name) === normalize(name));
    if (!tag) throw new Error(`“${name}” is not a ${source === 'anilist' ? 'AniList' : 'MangaBaka'} tag. Choose tags from one catalog.`);
    return tag;
  });
  const base = { query, page, currentPage: page, limit, partial: false, failedProviders: [], source };
  if (source === 'anilist') {
    const country = options.origin || ({ manhwa: 'KR', manhua: 'CN', manga: 'JP', comics: 'US' }[options.mangaType || '']);
    const data = await aniList<{ Page: { pageInfo: { total?: number; hasNextPage: boolean }; media: AniListMedia[] } }>(`
      query ($page: Int, $perPage: Int, $search: String, $tags: [String], $adult: Boolean, $genre: String, $country: CountryCode, $format: MediaFormat) {
        Page(page: $page, perPage: $perPage) {
          pageInfo { total hasNextPage }
          media(type: MANGA, search: $search, tag_in: $tags, isAdult: $adult, genre: $genre, countryOfOrigin: $country, format: $format, sort: [POPULARITY_DESC, ID_DESC]) {
            id idMal title { romaji english native } coverImage { large } format countryOfOrigin status startDate { year } averageScore popularity isAdult chapters volumes
          }
        }
      }`, {
      page, perPage: limit, search: query || undefined, tags: tags.map(tag => tag.name),
      adult: options.adult ? undefined : false, genre: options.genre, country,
      format: options.mangaType === 'novel' ? 'NOVEL' : undefined,
    });
    const results: MangaSearchItem[] = data.Page.media.map(media => ({
      id: String(media.id), mediaType: media.countryOfOrigin === 'KR' ? 'manhwa' : media.countryOfOrigin === 'CN' ? 'manhua' : 'manga',
      anilistId: media.id, malId: media.idMal, canonicalTitle: media.title.english || media.title.romaji || media.title.native || 'Untitled',
      title: media.title, poster: media.coverImage?.large || null, status: media.status || 'unknown',
      year: media.startDate?.year ?? null, score: media.averageScore ?? null, popularity: media.popularity ?? null,
      providersAvailable: [], matchConfidence: 1, adult: Boolean(media.isAdult), chapters: media.chapters ?? null,
      volumes: media.volumes ?? null, originLanguage: media.countryOfOrigin ?? null, readingDirection: 'unknown', providerSource: 'AniList',
    }));
    // AniList documents total as an estimate; hasNextPage is authoritative.
    return { ...base, results, total: data.Page.pageInfo.total, totalIsApproximate: true,
      hasNextPage: data.Page.pageInfo.hasNextPage && page * limit < 5000, resultWindow: 5000 };
  }
  // MangaBaka search was served by the removed TatakaiAPI mapping endpoint.
  return { ...base, results: [], total: 0, totalIsApproximate: false, hasNextPage: false, resultWindow: 0, partial: true, failedProviders: ['mangabaka'] };
}
