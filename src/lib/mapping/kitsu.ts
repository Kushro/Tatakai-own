/**
 * Kitsu chapter hierarchy — direct client.
 *
 * Walks Kitsu straight from the renderer (via `@/lib/net/publicApi`):
 *
 *   AniList manga id → Kitsu `/mappings` (anilist/manga) → `/manga/{id}/chapters`
 *
 * MangaBaka is no longer involved. A caller that already has the Kitsu id can pass
 * it and skip the mapping lookup.
 *
 * Kitsu is the only free source in the stack that publishes a **volume number per
 * chapter**, which is what makes the volume tree on the manga page possible at
 * all: scanlation providers give a number and, with luck, a title.
 *
 * Two upstream quirks absorbed here: `meta.count` is not the chapter count (Kitsu
 * reports a flat 5000 for Berserk, which has 401), so `totalChapters` is derived
 * from the rows actually fetched; and rows are frequently sparse — Berserk's
 * chapters have `canonicalTitle`, `published`, `length` and `thumbnail` all null.
 * `page[limit]` is capped at 20, so a long series takes many requests; they run a
 * few at a time and are paced by the Kitsu host limiter.
 */

import { clearMappingCache, loadMapping } from "./client";
import { KITSU_PAGE_MAX, kitsuGet, kitsuRows, lookupKitsuByExternal, type KitsuResource } from "./kitsuApi";

const CACHE_TTL_MS = 6 * 60 * 60 * 1000; // 6 hours

// ── Normalized shapes ─────────────────────────────────────────────────────────

export interface KitsuChapter {
  /** Kitsu's own chapter id — stable, useful as a React key. */
  kitsuId: string;
  /** Chapter number. `null` on rows Kitsu never numbered. */
  number: number | null;
  /** Volume this chapter belongs to, or `null` when unassigned. */
  volumeNumber: number | null;
  /** `canonicalTitle` when present, else `Chapter {number}`. */
  title: string;
  /** True when the title is the generated fallback rather than a real one. */
  isGeneratedTitle: boolean;
  titles: Record<string, string>;
  synopsis: string | null;
  /** `YYYY-MM-DD`. */
  published: string | null;
  /** Page count. */
  pageCount: number | null;
  thumbnail: string | null;
  thumbnailWidth: number | null;
  thumbnailHeight: number | null;
}

export interface KitsuVolume {
  /** `null` groups every chapter Kitsu left unassigned. */
  volumeNumber: number | null;
  label: string;
  chapters: KitsuChapter[];
  firstChapter: number | null;
  lastChapter: number | null;
  /** Earliest and latest `published` among this volume's chapters. */
  publishedStart: string | null;
  publishedEnd: string | null;
}

export interface KitsuChapterHierarchy {
  kitsuMangaId: number;
  /** Flat list, ascending by number. */
  chapters: KitsuChapter[];
  /** Volumes ascending; the unassigned bucket, if any, sorts last. */
  volumes: KitsuVolume[];
  /** Real chapter count — derived from the rows fetched, never from `meta.count`. */
  totalChapters: number;
  /** Volume count, excluding the unassigned bucket. */
  totalVolumes: number;
  /** True when the service's chapter guard cut the list short. */
  truncated: boolean;
}

// ── Normalizing ───────────────────────────────────────────────────────────────

const DEFAULT_MAX_CHAPTERS = 2000;
/** Pages fetched concurrently (each is up to 20 chapters). */
const PAGE_CONCURRENCY = 4;

const numOrNull = (v: unknown): number | null => {
  const n = v == null || v === "" ? NaN : Number(v);
  return Number.isFinite(n) ? n : null;
};

const textOrNull = (v: unknown): string | null => (typeof v === "string" && v.trim() ? v.trim() : null);

/** Normalize one Kitsu `chapters` resource. Pure. */
export function normalizeKitsuChapter(row: KitsuResource): KitsuChapter {
  const a = row.attributes;
  const number = numOrNull(a.number);
  const real = textOrNull(a.canonicalTitle);
  const titles: Record<string, string> = {};
  for (const [lang, value] of Object.entries((a.titles ?? {}) as Record<string, unknown>)) {
    const t = textOrNull(value);
    if (t) titles[lang] = t;
  }
  const thumb = a.thumbnail as { original?: string; large?: string; medium?: string; meta?: { dimensions?: Record<string, { width?: number; height?: number }> } } | null;
  const thumbUrl = thumb ? textOrNull(thumb.original) ?? textOrNull(thumb.large) ?? textOrNull(thumb.medium) : null;
  const dims = thumb?.meta?.dimensions?.large ?? thumb?.meta?.dimensions?.medium;
  const published = textOrNull(a.published);
  return {
    kitsuId: row.id,
    number,
    volumeNumber: numOrNull(a.volumeNumber),
    title: real ?? (number != null ? `Chapter ${number}` : "Chapter"),
    isGeneratedTitle: !real,
    titles,
    synopsis: textOrNull(a.synopsis),
    published: published ? published.slice(0, 10) : null,
    pageCount: numOrNull(a.length),
    thumbnail: thumbUrl,
    thumbnailWidth: thumbUrl ? numOrNull(dims?.width) : null,
    thumbnailHeight: thumbUrl ? numOrNull(dims?.height) : null,
  };
}

/** Group a chapter list into volumes. Pure. */
export function buildKitsuHierarchy(
  kitsuMangaId: number,
  rows: KitsuResource[],
  truncated = false,
): KitsuChapterHierarchy {
  const seen = new Set<string>();
  const chapters = rows
    .filter((r) => !seen.has(r.id) && seen.add(r.id))
    .map(normalizeKitsuChapter)
    .sort((a, b) => (a.number ?? Infinity) - (b.number ?? Infinity));

  const groups = new Map<number | null, KitsuChapter[]>();
  for (const c of chapters) {
    const list = groups.get(c.volumeNumber) ?? [];
    list.push(c);
    groups.set(c.volumeNumber, list);
  }

  const volumes: KitsuVolume[] = [...groups.entries()]
    .sort(([a], [b]) => (a ?? Infinity) - (b ?? Infinity))
    .map(([volumeNumber, list]) => {
      const numbers = list.map((c) => c.number).filter((n): n is number => n != null);
      const dates = list.map((c) => c.published).filter((d): d is string => Boolean(d)).sort();
      return {
        volumeNumber,
        label: volumeNumber != null ? `Volume ${volumeNumber}` : "No volume",
        chapters: list,
        firstChapter: numbers.length ? Math.min(...numbers) : null,
        lastChapter: numbers.length ? Math.max(...numbers) : null,
        publishedStart: dates[0] ?? null,
        publishedEnd: dates[dates.length - 1] ?? null,
      };
    });

  return {
    kitsuMangaId,
    chapters,
    volumes,
    totalChapters: chapters.length,
    totalVolumes: volumes.filter((v) => v.volumeNumber != null).length,
    truncated,
  };
}

// ── Fetching ──────────────────────────────────────────────────────────────────

async function walkChapters(kitsuId: number, cap: number, signal?: AbortSignal): Promise<{ rows: KitsuResource[]; truncated: boolean }> {
  const rows: KitsuResource[] = [];
  let offset = 0;
  let lastFull = false;
  while (offset < cap) {
    const offsets = Array.from({ length: PAGE_CONCURRENCY }, (_, i) => offset + i * KITSU_PAGE_MAX).filter((o) => o < cap);
    const pages = await Promise.all(offsets.map((o) =>
      kitsuGet(`/manga/${kitsuId}/chapters`, { "page[limit]": KITSU_PAGE_MAX, "page[offset]": o, sort: "number" }, signal)));
    let ended = false;
    for (const page of pages) {
      const data = kitsuRows(page);
      rows.push(...data);
      lastFull = data.length >= KITSU_PAGE_MAX;
      if (!lastFull) { ended = true; break; }
    }
    if (ended) break;
    offset += offsets.length * KITSU_PAGE_MAX;
  }
  return { rows: rows.slice(0, cap), truncated: lastFull && rows.length >= cap };
}

/**
 * Fetch the volume → chapter hierarchy for a manga.
 *
 * Pass an AniList id (the usual case) and Kitsu's mappings resolve the Kitsu id.
 * Pass `kitsuId` to skip that hop when it is already known.
 *
 * Returns `null` when the manga maps to no Kitsu record, Kitsu lists no chapters,
 * or Kitsu is unreachable, so callers fall back to their provider-derived list.
 */
export async function fetchKitsuChapterHierarchy(
  anilistId: number | string | null | undefined,
  options: {
    signal?: AbortSignal;
    force?: boolean;
    maxChapters?: number;
    /** Skip the mapping lookup when the Kitsu manga id is already known. */
    kitsuId?: number | string | null;
  } = {},
): Promise<KitsuChapterHierarchy | null> {
  const id = Number(anilistId);
  const givenKitsuId = Number(options.kitsuId);
  const hasKitsuId = Number.isInteger(givenKitsuId) && givenKitsuId > 0;
  if (!hasKitsuId && !(Number.isInteger(id) && id > 0)) return null;

  const cap = Math.max(KITSU_PAGE_MAX, Math.min(options.maxChapters ?? DEFAULT_MAX_CHAPTERS, 10_000));
  const key = hasKitsuId ? `kitsu:chapters:k${givenKitsuId}:${cap}` : `kitsu:chapters:a${id}:${cap}`;

  return loadMapping<KitsuChapterHierarchy>(key, async (signal) => {
    const kitsuId = hasKitsuId ? givenKitsuId : (await lookupKitsuByExternal("manga", "anilist", id, signal))?.kitsuId;
    if (!kitsuId) return null;
    const { rows, truncated } = await walkChapters(kitsuId, cap, signal);
    return rows.length ? buildKitsuHierarchy(kitsuId, rows, truncated) : null;
  }, { ttlMs: CACHE_TTL_MS, signal: options.signal, force: options.force });
}

/** Drop cached hierarchies — used by manual refresh paths. */
export function clearKitsuChapterCache(anilistId?: number): void {
  clearMappingCache(typeof anilistId === "number" ? `kitsu:chapters:a${anilistId}:` : "kitsu:chapters:");
}
