import type {
  MangaChapterResponse,
  MangaDetailResponse,
  MangaReadResponse,
  MangaSearchItem,
  MangaSearchResult,
  MappedMangaChapter,
  MangaChapterSource,
} from "@/types/manga";
import type {
  MangaFeedSection,
  MangaHomeBundle,
  MediaStatus,
  SearchFilters,
  TatakaiMedia,
} from "./types";
import { metadata, type FeedOptions, type MetadataFeedKind } from "@/lib/metadata";
import { parseMediaId } from "./content-graph";
import {
  readCachedItemsSync,
  hydrateCachedItems,
  writeCachedItems,
  MANGA_CHAPTERS_TTL_MS,
  clearCachedItemsByPrefix,
} from "@/lib/cache/extensionResultCache";
import { isCapacitor } from "@/lib/platform/platform";
import { removeMangaChapterOutliers } from '@/lib/reader/mangaChapterReliability';
import { mangaExtensionContext } from '@/core/extensions/compatibility/title-context';

/** Cache-key builder shared by the client + the query hook's placeholder. */
let chaptersCacheGeneration = 0;
export function mangaChaptersCacheKey(id: string, providers?: string, language?: string): string {
  return `manga-chapters::v3::${String(id || "")}::${providers || ""}::${language || ""}`;
}

export async function clearMangaChaptersCache(id: string): Promise<void> {
  chaptersCacheGeneration++;
  const { clearMangaDexChapterCache } = await import('./mangadex-api');
  clearMangaDexChapterCache(id);
  const { clearMangaUpdatesCache } = await import('./mangaupdates-api');
  clearMangaUpdatesCache(id);
  await clearCachedItemsByPrefix(`manga-chapters::v3::${id}::`);
  await clearCachedItemsByPrefix(`manga-chapters::v2::${id}::`);
  await clearCachedItemsByPrefix(`manga-chapters::${id}::`);
}

/** Stable identity for a mapped chapter: chapter number when present, else its canonical order. */
function chapterIdentity(c: MappedMangaChapter): string {
  return c?.chapterNumber != null ? `n:${c.chapterNumber}` : `o:${c?.canonicalOrder ?? "?"}`;
}

/**
 * Per-source identity within a chapter. Includes the scanlator so distinct
 * scanlation groups of the same provider+language coexist (multi-language AND
 * multi-group sources survive the union merge) — without it, alternate groups
 * collapse into one and the reader loses same-chapter fallback candidates.
 */
function sourceIdentity(s: MangaChapterSource): string {
  return `${s?.provider || ""}::${s?.language || ""}::${s?.scanlator || ""}::${s?.providerChapterId || s?.chapterKey || ""}`;
}

/**
 * Union-merge freshly-merged chapters with a persisted cache, FRESH-AUTHORITATIVE.
 * - A chapter/provider-source present in `fresh` overwrites the cached copy (so a
 *   provider whose source rotated/changed is updated — never the stale one).
 * - Chapters (and per-chapter provider-sources) present ONLY in the cache are
 *   re-added (no-drop guarantee: a provider momentarily down never shrinks the
 *   list). Additive only.
 */
export function unionMappedChapters(
  fresh: MappedMangaChapter[],
  cached: MappedMangaChapter[],
): MappedMangaChapter[] {
  // A responding provider replaces its old catalog. Preserve cache entries
  // only for sources absent from this scan (e.g. temporarily unavailable).
  const freshProviders = new Set(fresh.flatMap(c => (c.sources || []).map(s => s.provider)));
  cached = cached.map(c => ({ ...c, sources: (c.sources || []).filter(s => !freshProviders.has(s.provider)) }))
    .filter(c => c.sources.length > 0);
  const cachedById = new Map<string, MappedMangaChapter>();
  for (const c of cached || []) cachedById.set(chapterIdentity(c), c);

  const seen = new Set<string>();
  const out: MappedMangaChapter[] = [];

  const unionSources = (
    freshSrc: MangaChapterSource[],
    cachedSrc: MangaChapterSource[],
  ): MangaChapterSource[] => {
    const merged: MangaChapterSource[] = [];
    const sSeen = new Set<string>();
    for (const s of freshSrc || []) {
      const k = sourceIdentity(s);
      if (sSeen.has(k)) continue;
      sSeen.add(k);
      merged.push(s);
    }
    for (const s of cachedSrc || []) {
      const k = sourceIdentity(s);
      if (sSeen.has(k)) continue; // fresh wins per provider
      sSeen.add(k);
      merged.push(s);
    }
    return merged;
  };

  for (const f of fresh || []) {
    const id = chapterIdentity(f);
    seen.add(id);
    const prior = cachedById.get(id);
    if (!prior) {
      out.push(f);
      continue;
    }
    // Fresh chapter authoritative; fill only source-providers it lacks + any
    // Kitsu enrichment fields the fresh copy is missing.
    out.push({
      ...prior,
      ...f,
      sources: unionSources(f.sources || [], prior.sources || []),
      published: f.published ?? prior.published ?? null,
      pageCount: f.pageCount ?? prior.pageCount ?? null,
      thumbnail: f.thumbnail ?? prior.thumbnail ?? null,
      synopsis: f.synopsis ?? prior.synopsis ?? null,
      volume: f.volume ?? prior.volume ?? null,
      kitsuId: f.kitsuId ?? prior.kitsuId,
    });
  }

  // Re-add cache-only chapters (no-drop guarantee).
  for (const c of cached || []) {
    const id = chapterIdentity(c);
    if (seen.has(id)) continue;
    seen.add(id);
    out.push(c);
  }

  out.sort((a, b) => {
    const ao = a?.canonicalOrder ?? a?.chapterNumber ?? 0;
    const bo = b?.canonicalOrder ?? b?.chapterNumber ?? 0;
    return ao - bo;
  });
  return out;
}


export type MangaSearchOptions = {
  requiresQuery?: boolean;
  adult?: boolean;
  mode?: MangaSearchMode;
  provider?: MangaSearchProvider;
  genre?: string;
  tags?: string[];
  tagSource?: 'anilist' | 'mangabaka';
  types?: string[];
  mangaType?: string;
  statuses?: string[];
  sort?: string;
  timeWindow?: string;
  origin?: string;
  minYear?: number;
  minScore?: number;
  minChapters?: number;
  maxYear?: number;
  maxScore?: number;
};

// Manga provider types
export type MangaSearchProvider = "all" | "mapped" | "atsu";
export type MangaSearchMode = "search" | "latest" | "added" | "new-chap" | "recent" | "popular" | "foryou" | "recommendation" | "origin" | "random" | "genre" | "category" | "explore";
export type MangaFeedTimeWindow = "day" | "week" | "month" | "all";
export type MangaSortOption = "relevance" | "trending" | "latestUpdate" | "rating" | "popularity" | "chapterCount" | "follows" | "newest";

/**
 * Normalize manga search provider string to valid MangaSearchProvider
 */
export function parseMangaSearchProvider(
  value: string | null | undefined,
  fallback: MangaSearchProvider = "all"
): MangaSearchProvider {
  const normalized = String(value || "").toLowerCase().trim();
  const validProviders: MangaSearchProvider[] = ["all", "mapped", "atsu"];
  
  if (validProviders.includes(normalized as MangaSearchProvider)) {
    return normalized as MangaSearchProvider;
  }
  
  return fallback;
}

/**
 * Get filter presets for Atsu providers
 */
export function getAtsuFilters(): Record<string, string[]> {
  return {
    types: ["manga", "manhwa", "manhua", "comics"],
    statuses: ["ongoing", "completed", "hiatus", "cancelled", "unreleased"],
    origins: ["jp", "kr", "zh"],
  };
}

/**
 * Get manga filter schema
 */
export function getMangaFilterSchema(): Record<string, any> {
  return {
    provider: { type: "select", options: ["all", "mapped", "atsu"] },
    type: { type: "select", options: ["all", "manga", "manhwa", "manhua", "comics"] },
    status: { type: "select", options: ["all", "ongoing", "completed", "hiatus", "cancelled", "unreleased"] },
    sort: { type: "select", options: ["relevance", "trending", "latestUpdate", "rating", "popularity", "chapterCount", "follows", "newest"] },
  };
}

/**
 * Get manga filter counts (for UI display)
 */
export async function getMangaFilterCounts(_query?: string): Promise<Record<string, Record<string, number>>> {
  return {
    type: { manga: 10000, manhwa: 5000, manhua: 3000, comics: 1000 },
    status: { ongoing: 15000, completed: 4000, hiatus: 500, cancelled: 100, unreleased: 50 },
    origin: { jp: 10000, kr: 4000, zh: 3000 },
  };
}

/**
 * Total-chapter reporting went to the TatakaiAPI mirror, which no longer exists.
 * Kept as a no-op so existing call sites keep compiling.
 */
export async function storeMangaTotalChapters(_mangaId: string, _total: number): Promise<void> {
  /* intentionally empty */
}

const STATUS_FILTERS: Record<string, MediaStatus> = {
  ongoing: "RELEASING", completed: "FINISHED", hiatus: "HIATUS",
  cancelled: "CANCELLED", unreleased: "NOT_YET_RELEASED",
};

const ORIGIN_COUNTRY: Record<string, string> = {
  jp: "JP", kr: "KR", cn: "CN", zh: "CN", tw: "TW", us: "US",
  manga: "JP", manhwa: "KR", manhua: "CN", comics: "US",
};

const UPPER_SORTS = new Set<NonNullable<SearchFilters["sortBy"]>>([
  "TRENDING_DESC", "POPULARITY_DESC", "SCORE_DESC", "START_DATE_DESC", "TITLE_ROMAJI",
  "FAVOURITES_DESC", "UPDATED_AT_DESC", "CHAPTERS_DESC", "END_DATE_DESC",
]);

/** Manga/novel UI sort + mode values → router sort (undefined = relevance). */
function toSortBy(sort?: string, mode?: string): SearchFilters["sortBy"] {
  const s = String(sort || "");
  if (UPPER_SORTS.has(s as NonNullable<SearchFilters["sortBy"]>)) return s as SearchFilters["sortBy"];
  switch (s.toLowerCase()) {
    case "trending": return "TRENDING_DESC";
    case "rating": case "score": return "SCORE_DESC";
    case "popularity": return "POPULARITY_DESC";
    case "follows": return "FAVOURITES_DESC";
    case "chaptercount": return "CHAPTERS_DESC";
    case "newest": return "START_DATE_DESC";
    case "latestupdate": return "UPDATED_AT_DESC";
  }
  switch (mode) {
    case "popular": return "POPULARITY_DESC";
    case "latest": case "new-chap": case "recent": return "UPDATED_AT_DESC";
    case "added": return "START_DATE_DESC";
  }
  return undefined;
}

/** Map the manga/novel UI search options onto router filters (shared with novel-client). */
export function toMetadataSearchFilters(
  query: string,
  page: number,
  limit: number,
  options: MangaSearchOptions & { novelsOnly?: boolean },
): SearchFilters & { novelsOnly?: boolean; countryOfOrigin?: string } {
  const mangaType = String(options.mangaType || options.types?.[0] || "").toLowerCase();
  const origin = String(options.origin || "").toLowerCase();
  const countryOfOrigin = ORIGIN_COUNTRY[origin] ?? ORIGIN_COUNTRY[mangaType];
  const statuses = (options.statuses || [])
    .map((s) => STATUS_FILTERS[String(s).toLowerCase()] ?? (String(s).toUpperCase() as MediaStatus))
    .filter(Boolean);
  const year = options.minYear || options.maxYear
    ? { min: options.minYear || undefined, max: options.maxYear || undefined }
    : undefined;
  const rating = options.minScore || options.maxScore
    ? {
        min: options.minScore ? Math.round(options.minScore * 10) : undefined,
        max: options.maxScore ? Math.round(options.maxScore * 10) : undefined,
      }
    : undefined;
  return {
    query: query.trim() || undefined,
    page,
    perPage: limit,
    isAdult: options.adult ? true : false,
    genres: options.genre ? [options.genre] : undefined,
    status: statuses.length ? statuses : undefined,
    sortBy: toSortBy(options.sort, options.mode),
    year,
    rating,
    countryOfOrigin,
    novelsOnly: options.novelsOnly || mangaType === "novel" || undefined,
  };
}

export async function searchManga(
  query: string,
  page = 1,
  limit = 20,
  options: MangaSearchOptions = {},
): Promise<MangaSearchResult> {
  if (options.tags?.length) {
    const { searchMangaTags } = await import('./manga-tag-search');
    return searchMangaTags(query, page, limit, options);
  }
  const { value, provider } = await metadata.search("MANGA", toMetadataSearchFilters(query, page, limit, options));
  let rows: TatakaiMedia[] = value.media;
  if (options.minChapters) rows = rows.filter((m) => (m.chapters ?? 0) >= options.minChapters!);
  const getMangaType = (media: any) => {
    const fmt = String(media?.format || "").toUpperCase();
    if (fmt === "NOVEL" || fmt === "LIGHT_NOVEL") return "novel";
    if (media?.format === "OEL") return "comics";
    if (media?.countryOfOrigin === "KR") return "manhwa";
    if (media?.countryOfOrigin === "CN") return "manhua";
    return "manga";
  };
  const results = rows.map((media) => {
    return {
      id: media?.anilistId
        ? String(media.anilistId)
        : media?.malId
          ? `mal:${media.malId}`
          : "",
      mediaType: "manga" as const,
      type: getMangaType(media),
      // Raw AniList format (MANGA/MANHWA/MANHUA/OEL/ONE_SHOT/NOVEL) so callers can
      // classify the reading track precisely (e.g. tell novels from manga).
      format: media?.format ?? null,
      anilistId: media?.anilistId,
      malId: media?.malId,
      canonicalTitle: media?.titleEnglish || media?.titleRomaji || media?.titleNative || "Untitled",
      title: { romaji: media?.titleRomaji, english: media?.titleEnglish, native: media?.titleNative },
      poster: media?.coverImageLarge || media?.coverImageMedium || null,
      status: media?.status || "unknown",
      year: media?.startDate?.year ?? null,
      score: media?.averageScore ?? null,
      popularity: media?.popularity ?? null,
      providersAvailable: [],
      matchConfidence: 1,
      adult: Boolean(media?.isAdult),
      chapters: media?.chapters ?? null,
      volumes: media?.volumes ?? null,
      originLanguage: media?.countryOfOrigin ?? null,
      readingDirection: "unknown" as const,
    };
  });

  return {
    query,
    page,
    limit,
    partial: false,
    failedProviders: [],
    results,
    currentPage: value.pageInfo.currentPage || page,
    totalPages: value.pageInfo.lastPage,
    total: value.pageInfo.total,
    totalIsApproximate: true,
    hasNextPage: value.pageInfo.hasNextPage,
    source: provider,
  };
}

export async function getMangaDetail(id: string): Promise<MangaDetailResponse> {
  const normalizedId = String(id || "");
  const parsed = parseMediaId(normalizedId);
  if (!parsed) throw new Error(`Invalid manga ID: ${id}`);
  let found = (await metadata.media("MANGA", parsed.ref)).value;
  if (!found && parsed.alt) found = (await metadata.media("MANGA", parsed.alt)).value;
  if (!found) throw new Error(`Manga not found: ${id}`);
  const row: any = found;

  const media = row?.media ?? row;
  const authors = media?.staff?.filter((s: any) => s.role?.toLowerCase().includes("story")).map((s: any) => s.name) || [];
  const artists = media?.staff?.filter((s: any) => s.role?.toLowerCase().includes("art")).map((s: any) => s.name) || [];
  const publishers = media?.studios?.map((s: any) => s.name) || [];

  return {
    id,
    detail: {
      mediaType: "manga",
      anilistId: media?.anilistId,
      malId: media?.malId,
      canonicalTitle: media?.titleEnglish || media?.titleRomaji || media?.titleNative || "Untitled",
      title: {
        romaji: media?.titleRomaji,
        english: media?.titleEnglish,
        native: media?.titleNative,
        synonyms: media?.synonyms || [],
      },
      status: media?.status || "unknown",
      genres: media?.genres || [],
      themes: [],
      // Raw AniList format (MANGA/MANHWA/MANHUA/OEL/ONE_SHOT/NOVEL) — used to
      // classify the reading track for ranks. Falls back to media.type.
      format: media?.format || media?.type || null,
      origin: media?.countryOfOrigin || null,
      originLanguage: media?.countryOfOrigin || null,
      adult: Boolean(media?.isAdult),
      yearStart: media?.startDate?.year ?? null,
      yearEnd: media?.endDate?.year ?? null,
      score: media?.averageScore ?? null,
      popularity: media?.popularity ?? null,
      coverImage: media?.coverImageLarge || media?.coverImageMedium || null,
      providersAvailable: [],
      synopsis: media?.description || null,
      authors: authors.length ? authors : (media?.authors || []),
      artists: artists.length ? artists : (media?.artists || []),
      publishers: publishers.length ? publishers : (media?.publishers || []),
      serialization: media?.serialization || null,
      totalChapters: media?.chapters || null,
      totalVolumes: media?.volumes || null,
      latestChapter: null,
      lastUpdatedAt: null,
      languagesAvailable: [],
      providerCoverage: { available: [], failed: [] },
      matchConfidence: 1,
      matchedBy: "anilist",
      characters: media?.characters || [],
      staff: media?.staff || [],
      relations: media?.relations || [],
      externalLinks: media?.externalLinks || [],
      // Provenance + raw columns for the admin editor and "in DB" badge.
      tatakaiId: media?.tatakaiId ?? null,
      sourceApi: media?.source_api ?? null,
      inDb: media?.source_api === "tatakai" || Boolean(media?.tatakaiId),
      bannerImage: media?.bannerImage ?? null,
      coverImageMedium: media?.coverImageMedium ?? null,
      meanScore: media?.meanScore ?? null,
      favourites: media?.favourites ?? null,
      rating: media?.rating ?? null,
      trailerUrl: media?.trailerUrl ?? null,
    },
  };
}

export async function getMangaChapters(id: string, _providers?: string, _language?: string, metadata?: MangaDetailResponse | null | Promise<MangaDetailResponse | null>): Promise<MangaChapterResponse> {
  const generation = chaptersCacheGeneration;
  const { fetchMangaDexChapters } = await import('./mangadex-api');
  const { fetchMangaUpdatesCatalog, applyMangaUpdatesCatalog } = await import('./mangaupdates-api');
  const runtime = await import('@/core/content/manga-extension-runtime');
  const anilistId = Number(String(id).replace(/^anilist:/i, ""));
  const [mangaDex, updatesCatalog, extensionPayload] = await Promise.all([
    fetchMangaDexChapters(id),
    fetchMangaUpdatesCatalog(id),
    (async () => runtime.fetchExtensionMangaChapters(mangaExtensionContext(id, metadata === undefined ? await getMangaDetail(id).catch(() => undefined) : await metadata || undefined)))()
      .catch(() => ({chapters:[],providersAvailable:[],failedProviders:[],pending:false})),
  ]);
  const empty: MangaChapterResponse = {anilistId:Number.isFinite(anilistId)?anilistId:0,partial:false,failedProviders:[],chapters:[],mappedChapters:[]};
  // Reader providers always contribute, regardless of which metadata API answers.
  const base = mangaDex || empty;
  const cacheKey = mangaChaptersCacheKey(id, _providers, _language);

  const merged = applyMangaUpdatesCatalog(runtime.mergeChaptersWithExtensions(base, extensionPayload), updatesCatalog);

  // Union-merge with the persisted cache (fresh-authoritative). Guarantees a
  // weaker refetch (a provider momentarily down) never *drops* previously-seen
  // chapters/sources, while a provider that re-responds with a changed source
  // updates in place. Fully non-fatal — any failure returns `merged` unchanged.
  try {
    const cachedFromMem = readCachedItemsSync<MappedMangaChapter>(cacheKey);
    const cached = cachedFromMem ?? (await hydrateCachedItems<MappedMangaChapter>(cacheKey)) ?? [];
    const unioned = removeMangaChapterOutliers(unionMappedChapters(merged.mappedChapters || [], cached))
      .map((chapter, index) => ({ ...chapter, canonicalOrder: index }));

    // Recompute providersAvailable as the union across the merged chapters'
    // sources plus whatever the fresh response already advertised.
    const providers = new Set<string>((merged as any).providersAvailable || []);
    for (const c of unioned) for (const s of c.sources || []) if (s?.provider) providers.add(s.provider);

    if (generation === chaptersCacheGeneration) writeCachedItems(cacheKey, unioned, MANGA_CHAPTERS_TTL_MS);

    return {
      ...merged,
      mappedChapters: unioned,
      ...(providers.size ? ({ providersAvailable: Array.from(providers) } as any) : {}),
    };
  } catch {
    return merged;
  }
}

export async function getMangaReadByKey(
  id: string,
  chapterKey: string,
  options?: {
    provider?: string;
    providerChapterId?: string;
    alternatives?: Array<{ provider: string; chapterKey: string; providerChapterId?: string }>;
  },
): Promise<MangaReadResponse> {
  const isNative =
    typeof window !== "undefined" &&
    Boolean(
      (window as any).electron ||
        (window as any).tatakaiRuntime ||
        (window as any).tatakaiMobileExtensions,
    );

  // ── Offline-first: if this chapter (or any of its alternative source keys) has
  // been downloaded, serve the local pages via tatakai-media:// instead of the
  // extension host. This is also the ONLY working path when the app is offline.
  const anilistIdNum = Number(String(id).replace(/^anilist:/i, ""));

  // ── Mobile offline-first: serve downloaded pages from app storage ──────────
  // Capacitor has no `window.electron.manga`; downloaded chapters live in Dexie
  // (offlineChapters) with page images on the filesystem, served via
  // Capacitor.convertFileSrc. This is also the only working path offline.
  if (isCapacitor() && Number.isFinite(anilistIdNum)) {
    const candidateKeys = [
      chapterKey,
      ...(options?.alternatives || []).map((a) => a.chapterKey),
    ].filter(Boolean);
    try {
      const { db } = await import("@/core/db/tatakai-db");
      let matchedKey: string | null = null;
      for (const key of candidateKeys) {
        const row = await db.offlineChapters.get(`${anilistIdNum}:${key}`);
        if (row) {
          matchedKey = key;
          break;
        }
      }
      if (matchedKey) {
        const { getOfflineMangaPagesMobile } = await import(
          "@/core/download/mobile/mobileMangaDownloader"
        );
        const localPages = await getOfflineMangaPagesMobile(anilistIdNum, matchedKey);
        if (localPages.length) {
          return {
            success: true,
            data: {
              pages: localPages.map((p) => ({
                pageNumber: p.pageNumber,
                imageUrl: p.imageUrl,
                proxiedImageUrl: null,
                width: null,
                height: null,
              })),
              chapter: {
                chapterKey: matchedKey,
                anilistId: anilistIdNum,
                provider: "tatakai_offline",
                providerChapterId: matchedKey,
                number: null,
                title: null,
                language: null,
              },
              readMeta: {
                provider: "tatakai_offline",
                fallbackUsed: false,
                fetchedAt: new Date().toISOString(),
              },
            },
          };
        }
      }
    } catch {
      /* offline lookup best-effort; fall through to the extension runtime */
    }
  }

  if (
    isNative &&
    Number.isFinite(anilistIdNum) &&
    (window as any).electron?.manga?.getOfflinePages
  ) {
    const candidateKeys = [
      chapterKey,
      ...(options?.alternatives || []).map((a) => a.chapterKey),
    ].filter(Boolean);
    try {
      const { db } = await import("@/core/db/tatakai-db");
      let matchedKey: string | null = null;
      for (const key of candidateKeys) {
        const row = await db.offlineChapters.get(`${anilistIdNum}:${key}`);
        if (row) {
          matchedKey = key;
          break;
        }
      }
      if (matchedKey) {
        const res = await (window as any).electron.manga.getOfflinePages({
          anilistId: anilistIdNum,
          chapterKey: matchedKey,
        });
        if (res?.success && Array.isArray(res.pages) && res.pages.length) {
          // Serve pages over the local HTTP stream server (the exact path anime
          // downloads use) rather than the tatakai-media:// custom protocol
          // directly. Chromium loads http://127.0.0.1:PORT/stream/… without the
          // Windows drive-letter canonicalization that breaks a raw
          // `tatakai-media:///C:/…` URL in <img src>. Falls back to the raw
          // media URL if the stream server is unavailable.
          const streamLocalFile = (window as any).electron?.streamLocalFile;
          const pages = await Promise.all(
            res.pages.map(async (p: any) => {
              const raw = String(p.imageUrl || "");
              let imageUrl = raw;
              if (streamLocalFile && raw.startsWith("tatakai-media://")) {
                try {
                  const r = await streamLocalFile(raw);
                  if (r?.success && r.url) imageUrl = String(r.url);
                } catch {
                  /* keep the raw media URL */
                }
              }
              return {
                pageNumber: Number(p.pageNumber) || 0,
                imageUrl,
                proxiedImageUrl: null,
                width: null,
                height: null,
              };
            }),
          );
          return {
            success: true,
            data: {
              pages,
              chapter: {
                chapterKey: matchedKey,
                anilistId: anilistIdNum,
                provider: "tatakai_offline",
                providerChapterId: matchedKey,
                number: typeof res.chapterNumber === "number" ? res.chapterNumber : null,
                title: typeof res.title === "string" ? res.title : null,
                language: null,
              },
              readMeta: {
                provider: "tatakai_offline",
                fallbackUsed: false,
                fetchedAt: new Date().toISOString(),
              },
            },
          };
        }
      }
    } catch {
      /* offline lookup is best-effort; fall through to the extension runtime */
    }
  }

  // "If no id then fall back to another scanlator": when the caller has no usable
  // provider/id for this chapter (e.g. a deep link that dropped them), promote the
  // first known source to the primary so the read still reaches the runtime — the
  // remaining sources stay as fallbacks.
  let provider = options?.provider;
  let effKey = chapterKey;
  let providerChapterId = options?.providerChapterId;
  if ((!provider || provider === "") && options?.alternatives?.length) {
    const first = options.alternatives[0];
    provider = first.provider;
    effKey = first.chapterKey || chapterKey;
    providerChapterId = first.providerChapterId || providerChapterId;
  }

  if (provider === 'mangadex' || effKey.startsWith('mangadex:')) {
    const { fetchMangaDexPages } = await import('./mangadex-api');
    const keys = [...new Set([effKey, ...(options?.alternatives || [])
      .filter(source => source.provider === 'mangadex').map(source => source.chapterKey)])];
    let failure: MangaReadResponse = {success:false,message:'MangaDex pages could not be loaded.'};
    for (const [index, key] of keys.entries()) {
      const result = await fetchMangaDexPages(Number.isFinite(anilistIdNum) ? anilistIdNum : 0, key);
      if (result.success) {
        if (result.data?.readMeta) result.data.readMeta.fallbackUsed = index > 0;
        return result;
      }
      failure = result;
    }
    const otherSources = (options?.alternatives || []).filter(source => source.provider !== 'mangadex');
    if (otherSources.length) {
      const { fetchExtensionMangaPages } = await import('./manga-extension-runtime');
      const first = otherSources[0];
      const result = await fetchExtensionMangaPages({extensionId:first.provider,chapterKey:first.chapterKey,
        providerChapterId:first.providerChapterId,anilistId:Number.isFinite(anilistIdNum)?anilistIdNum:undefined,
        alternatives:otherSources.slice(1)});
      if (result.success) {
        if (result.data?.readMeta) result.data.readMeta.fallbackUsed = true;
        return result;
      }
    }
    return failure;
  }
  if ((isNative || provider?.startsWith('server-')) && provider && provider !== "tatakai_media") {
    const { fetchExtensionMangaPages } = await import("@/core/content/manga-extension-runtime");
    const anilistId = Number(String(id).replace(/^anilist:/i, ""));
    return fetchExtensionMangaPages({
      extensionId: provider,
      chapterKey: effKey,
      providerChapterId,
      anilistId: Number.isFinite(anilistId) ? anilistId : undefined,
      alternatives: options?.alternatives,
    });
  }

  return {
    success: false,
    message: "Manga chapter reading moved to extension runtime.",
    guidance: {
      code: "EXTENSION_RUNTIME_REQUIRED",
      message: isNative
        ? "Install a manga extension and open a provider source from the chapter hierarchy."
        : "Connect an extension server in Extensions Hub and select a provider source from the chapter list.",
      retryable: false,
    },
  };
}

// ─── Shared card type used by trending/recommendation UI ─────────────────────
export type MangaCard = {
  id: string;
  title: string;
  poster: string | null;
  score: number | null;
  popularity: number | null;
  status: string;
  chapters: number | null;
  genres: string[];
  /** Originating source for deduplication */
  anilistId?: number;
  malId?: number;
  /** 'manga' | 'manhwa' | 'manhua' */
  format?: string;
};

function rowToMangaCard(row: any): MangaCard {
  const media = row?.media ?? row;
  return {
    id: media?.anilistId
      ? String(media.anilistId)
      : media?.malId
        ? `mal:${media.malId}`
        : String(media?.id || ""),
    title: media?.titleEnglish || media?.titleRomaji || media?.titleNative || "Untitled",
    poster: media?.coverImageLarge || media?.coverImageMedium || null,
    score: media?.averageScore ?? null,
    popularity: media?.popularity ?? null,
    status: media?.status || "unknown",
    chapters: media?.chapters ?? null,
    genres: media?.genres || [],
    anilistId: media?.anilistId,
    malId: media?.malId,
    format: (media?.format || media?.type || "manga").toLowerCase(),
  };
}

async function feedCards(feed: MetadataFeedKind, limit: number, opts: Partial<FeedOptions> = {}): Promise<MangaCard[]> {
  try {
    const { value } = await metadata.feed("MANGA", feed, { perPage: limit, ...opts });
    return value.media.map(rowToMangaCard).filter((m) => m.id);
  } catch {
    return [];
  }
}

/** Trending Japanese manga (AniList trending, falling back through the router). */
export async function getTrendingManga(limit = 18): Promise<MangaCard[]> {
  return feedCards("trending", limit, { countryOfOrigin: "JP" });
}

/** Popular manhwa (Korean). */
export async function getTrendingManhwa(limit = 12): Promise<MangaCard[]> {
  return feedCards("popular", limit, { countryOfOrigin: "KR" });
}

/** Popular manhua (Chinese). */
export async function getTrendingManhua(limit = 12): Promise<MangaCard[]> {
  return feedCards("popular", limit, { countryOfOrigin: "CN" });
}

/**
 * Fetch manga recommendations based on a genre tag.
 * Used in the discovery / recommendation sections.
 */
export async function getMangaByGenre(genre: string, limit = 12): Promise<MangaCard[]> {
  return feedCards("popular", limit, { genre });
}

/**
 * Currently-publishing manga (the closest public-API proxy for "new chapters").
 */
export async function getLatestMangaUpdates(limit = 18): Promise<MangaCard[]> {
  return feedCards("airing", limit);
}

// ─── Hub feeds, assembled client-side from the metadata router ───────────────
// The shelf list below replaces the TatakaiAPI manga home and sections endpoints.

/**
 * `TatakaiMedia` → `MangaSearchItem`.
 *
 * The hub's cards are all built from `MangaSearchItem` (`toUnifiedMangaCard`,
 * `IndexMangaShowcase.toMangaCard`, `inferMangaAdultFlag`), so translating at the
 * boundary means none of those call sites change.
 */
export function mediaToMangaSearchItem(m: TatakaiMedia): MangaSearchItem {
  const origin = String(m.countryOfOrigin || "").toUpperCase();
  const mediaType: MangaSearchItem["mediaType"] =
    origin === "KR" ? "manhwa" : origin === "CN" || origin === "TW" ? "manhua" : "manga";

  return {
    id: m.anilistId ? String(m.anilistId) : m.malId ? `mal:${m.malId}` : "",
    mediaType,
    format: m.format ?? null,
    anilistId: m.anilistId,
    malId: m.malId,
    canonicalTitle: m.titleEnglish || m.titleRomaji || m.titleNative || "Untitled",
    title: { romaji: m.titleRomaji, english: m.titleEnglish, native: m.titleNative },
    poster: m.coverImageLarge || m.coverImageMedium || null,
    banner: m.bannerImage,
    logo: m.logo,
    titleLogo: m.titleLogo,
    description: m.description,
    status: m.status || "unknown",
    year: m.startDate?.year ?? null,
    score: m.averageScore ?? null,
    popularity: m.popularity ?? null,
    providersAvailable: [],
    matchConfidence: 1,
    adult: Boolean(m.isAdult),
    chapters: m.chapters ?? null,
    volumes: m.volumes ?? null,
    originLanguage: m.countryOfOrigin ?? null,
    readingDirection: "unknown",
  };
}

function mapLane(lane: TatakaiMedia[] | undefined): MangaSearchItem[] {
  return (Array.isArray(lane) ? lane : []).map(mediaToMangaSearchItem).filter((item) => item.id);
}

/** Every lane of `MangaHomeBundle`, already in `MangaSearchItem` shape. */
export type MangaHomeFeed = {
  [K in Exclude<keyof MangaHomeBundle, "genres" | "fetchedAt">]: MangaSearchItem[];
} & {
  genres: string[];
  fetchedAt: number;
};

/** One shelf of the endless feed, with the icon still a lucide *name*. */
export type MangaFeedShelf = Omit<MangaFeedSection, "items"> & { items: MangaSearchItem[] };

export type MangaSectionsFeed = {
  page: number;
  sections: MangaFeedShelf[];
  hasNextPage: boolean;
  totalPages: number;
};

const EMPTY_HOME_FEED: MangaHomeFeed = {
  spotlight: [], trending: [], popular: [], topRated: [], newChapters: [],
  manhwa: [], manhua: [], adultLatest: [], adultPopular: [],
  genres: [], fetchedAt: 0,
};

/** Fetch one lane; a failing lane is empty rather than failing the whole hub. */
async function lane(feed: MetadataFeedKind, perPage: number, opts: Partial<FeedOptions> = {}): Promise<TatakaiMedia[]> {
  try {
    return (await metadata.feed("MANGA", feed, { perPage, ...opts })).value.media;
  } catch {
    return [];
  }
}

/**
 * The top of the manga hub, composed from parallel router feeds.
 *
 * - spotlight: trending titles with a banner (topped up with the rest)
 * - newChapters: currently publishing (no per-chapter feed exists publicly)
 * - manhwa / manhua: popular titles by country of origin (KR / CN)
 * - adultLatest / adultPopular: only fetched when `adult` is true; titles in the
 *   Hentai genre
 * Returns an empty feed only when every lane is empty.
 */
export async function getMangaHome(adult = false): Promise<MangaHomeFeed> {
  const [trending, popular, topRated, newChapters, manhwa, manhua, adultLatest, adultPopular, genres] =
    await Promise.all([
      lane("trending", 20),
      lane("popular", 20),
      lane("top_rated", 20),
      lane("airing", 20),
      lane("popular", 20, { countryOfOrigin: "KR" }),
      lane("popular", 20, { countryOfOrigin: "CN" }),
      adult ? lane("airing", 20, { isAdult: true, genre: "Hentai" }) : Promise.resolve([] as TatakaiMedia[]),
      adult ? lane("popular", 20, { isAdult: true, genre: "Hentai" }) : Promise.resolve([] as TatakaiMedia[]),
      metadata.genres("MANGA", { includeAdult: adult }).then((r) => r.value).catch(() => [] as string[]),
    ]);

  const lanes = [trending, popular, topRated, newChapters, manhwa, manhua];
  if (lanes.every((l) => l.length === 0)) return { ...EMPTY_HOME_FEED, fetchedAt: Date.now() };

  const hero = trending.length ? trending : popular;
  const banners = hero.filter((m) => m.bannerImage);
  const spotlight = [...banners, ...hero.filter((m) => !m.bannerImage)].slice(0, 10);

  return {
    spotlight: mapLane(spotlight),
    trending: mapLane(trending),
    popular: mapLane(popular),
    topRated: mapLane(topRated),
    newChapters: mapLane(newChapters),
    manhwa: mapLane(manhwa),
    manhua: mapLane(manhua),
    adultLatest: mapLane(adultLatest),
    adultPopular: mapLane(adultPopular),
    genres,
    fetchedAt: Date.now(),
  };
}

interface ShelfDef {
  id: string;
  title: string;
  genre: string;
  layout: string;
  /** lucide icon name; the hook owns the name -> component map. */
  icon: string;
  feed: MetadataFeedKind;
  opts: Partial<FeedOptions>;
}

const LAYOUT_CYCLE = ["carousel", "grid", "featured", "compact", "masonry"] as const;

type ShelfSeed = Omit<ShelfDef, "layout">;
const genreShelf = (genre: string, icon: string, country?: FeedOptions["countryOfOrigin"]): ShelfSeed => {
  const prefix = country === "KR" ? "Manhwa: " : country === "CN" ? "Manhua: " : "";
  const slug = genre.toLowerCase().replace(/[^a-z0-9]+/g, "-");
  return {
    id: country ? `${country === "KR" ? "manhwa" : "manhua"}-${slug}` : slug,
    title: `${prefix}${genre}`,
    genre,
    icon,
    feed: "popular",
    opts: { genre, countryOfOrigin: country },
  };
};

const baseShelves: ShelfSeed[] = [
  { id: "trending", title: "Trending Manga", genre: "trending", icon: "Flame", feed: "trending", opts: { countryOfOrigin: "JP" } },
  { id: "popular-manhwa", title: "Popular Manhwa", genre: "manhwa", icon: "Sparkles", feed: "popular", opts: { countryOfOrigin: "KR" } },
  { id: "popular-manhua", title: "Popular Manhua", genre: "manhua", icon: "Zap", feed: "popular", opts: { countryOfOrigin: "CN" } },
  { id: "top-rated", title: "Top Rated", genre: "top rated", icon: "Sparkles", feed: "top_rated", opts: {} },
  { id: "publishing-now", title: "Publishing Now", genre: "ongoing", icon: "Compass", feed: "airing", opts: {} },
  { id: "upcoming", title: "Coming Soon", genre: "upcoming", icon: "Rocket", feed: "upcoming", opts: {} },
  { id: "recently-completed", title: "Recently Completed", genre: "completed", icon: "Brain", feed: "recently_completed", opts: {} },
  genreShelf("Action", "Sword"),
  genreShelf("Romance", "Heart"),
  genreShelf("Comedy", "Laugh"),
  genreShelf("Fantasy", "Sparkles"),
  genreShelf("Sci-Fi", "Rocket"),
  genreShelf("Horror", "Ghost"),
  genreShelf("Slice of Life", "Flower2"),
  genreShelf("Adventure", "Compass"),
  genreShelf("Mystery", "Search"),
  genreShelf("Psychological", "Brain"),
  genreShelf("Supernatural", "Ghost"),
  genreShelf("Sports", "Zap"),
  genreShelf("Drama", "Heart"),
  genreShelf("Thriller", "Flame"),
  genreShelf("Mahou Shoujo", "Sparkles"),
  genreShelf("Action", "Sword", "KR"),
  genreShelf("Romance", "Heart", "KR"),
  genreShelf("Fantasy", "Sparkles", "KR"),
  genreShelf("Action", "Sword", "CN"),
  genreShelf("Fantasy", "Sparkles", "CN"),
  genreShelf("Romance", "Heart", "CN"),
];

const SHELVES: ShelfDef[] = baseShelves.map((shelf, index) => ({
  ...shelf,
  layout: LAYOUT_CYCLE[index % LAYOUT_CYCLE.length],
}));

const SHELVES_PER_PAGE = 4;
/** Cards fetched per shelf; the hook renders 5 and needs slack for the adult filter. */
const SHELF_PREVIEW_ITEMS = 7;

/**
 * One scroll page of the endless feed: four shelves from the local shelf list,
 * fetched in parallel. Shelves whose fetch fails or comes back empty are dropped.
 */
export async function getMangaSections(page = 1, adult = false): Promise<MangaSectionsFeed> {
  const totalPages = Math.ceil(SHELVES.length / SHELVES_PER_PAGE);
  const slice = SHELVES.slice((page - 1) * SHELVES_PER_PAGE, page * SHELVES_PER_PAGE);
  const filled = await Promise.all(
    slice.map(async (shelf): Promise<MangaFeedShelf> => ({
      id: shelf.id,
      title: shelf.title,
      genre: shelf.genre,
      layout: shelf.layout,
      icon: shelf.icon,
      items: mapLane(await lane(shelf.feed, SHELF_PREVIEW_ITEMS, { ...shelf.opts, isAdult: adult })),
    })),
  );

  return {
    page,
    sections: filled.filter((section) => section.items.length > 0),
    hasNextPage: page < totalPages,
    totalPages,
  };
}

export type MangaSectionItemsPage = {
  id: string;
  title: string;
  layout: string;
  icon: string;
  genre: string;
  page: number;
  perPage: number;
  items: MangaSearchItem[];
  hasNextPage: boolean;
  totalPages: number;
};

/** Fetch per-section items for infinite scroll within a shelf. */
export async function getMangaSectionItems(
  sectionId: string,
  page = 1,
  options: { adult?: boolean; perPage?: number } = {},
): Promise<MangaSectionItemsPage | null> {
  const shelf = SHELVES.find((s) => s.id === sectionId);
  if (!shelf) return null;
  const perPage = options.perPage ?? 24;
  const { value } = await metadata.feed("MANGA", shelf.feed, {
    ...shelf.opts,
    page,
    perPage,
    isAdult: options.adult,
  });

  return {
    id: shelf.id,
    title: shelf.title,
    layout: shelf.layout || "grid",
    icon: shelf.icon,
    genre: shelf.genre,
    page,
    perPage,
    items: mapLane(value.media),
    hasNextPage: value.hasNextPage,
    totalPages: value.hasNextPage ? page + 1 : page,
  };
}
