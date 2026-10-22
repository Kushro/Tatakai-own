/**
 * Cross-provider id resolution. Local extensions and app routes key titles by
 * AniList id, but Tenrai/Shikimori/MAL key them by MAL id and Kitsu by its own
 * id. ani.zip maps anime ids in every direction; manga falls back to providers
 * that implement `ids()` (e.g. Kitsu mappings).
 *
 * Resolved mappings are cached in memory and persisted to localStorage (they
 * never change); misses are cached in memory for an hour.
 */

import type { TatakaiMedia } from '@/core/content/types';
import { publicJson, PublicApiError } from '@/lib/net/publicApi';
import type { MediaKind, MediaRef } from '@/lib/metadata/types';

type FullRef = MediaRef & { anilistId: number };

const STORAGE_KEY = 'tatakai:id-map:v1';
const MAX_ENTRIES = 5000;
const MISS_TTL_MS = 60 * 60 * 1000;

const hits = new Map<string, FullRef>();
const misses = new Map<string, number>();
let loaded = false;

function keysOf(kind: MediaKind, ref: MediaRef): string[] {
  const k = kind === 'ANIME' ? 'a' : 'm';
  const out: string[] = [];
  if (ref.anilistId) out.push(`${k}:anilist:${ref.anilistId}`);
  if (ref.malId) out.push(`${k}:mal:${ref.malId}`);
  if (ref.kitsuId) out.push(`${k}:kitsu:${ref.kitsuId}`);
  return out;
}

function load(): void {
  if (loaded) return;
  loaded = true;
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return;
    for (const [key, ref] of JSON.parse(raw) as Array<[string, FullRef]>) hits.set(key, ref);
  } catch { /* corrupt or unavailable storage: start empty */ }
}

let persistTimer: ReturnType<typeof setTimeout> | undefined;
function persist(): void {
  if (persistTimer) return;
  persistTimer = setTimeout(() => {
    persistTimer = undefined;
    try {
      const entries = [...hits.entries()].slice(-MAX_ENTRIES);
      localStorage.setItem(STORAGE_KEY, JSON.stringify(entries));
    } catch { /* storage full/unavailable: memory cache only */ }
  }, 1000);
}

/** Remember a known mapping (providers that already know both ids call this). */
export function rememberIds(kind: MediaKind, ref: MediaRef): void {
  if (!ref.anilistId) return;
  load();
  const full = { ...ref } as FullRef;
  for (const key of keysOf(kind, ref)) {
    hits.set(key, { ...(hits.get(key) || {}), ...full });
    misses.delete(key);
  }
  persist();
}

export function cachedIds(kind: MediaKind, ref: MediaRef): FullRef | null {
  load();
  for (const key of keysOf(kind, ref)) {
    const hit = hits.get(key);
    if (hit) return hit;
  }
  return null;
}

interface AniZipMappingsOnly {
  mappings?: { anilist_id?: number | null; mal_id?: number | null; kitsu_id?: number | null };
}

async function fromAniZip(ref: MediaRef, signal?: AbortSignal): Promise<FullRef | null> {
  const param = ref.anilistId ? `anilist_id=${ref.anilistId}` : ref.malId ? `mal_id=${ref.malId}` : ref.kitsuId ? `kitsu_id=${ref.kitsuId}` : '';
  if (!param) return null;
  try {
    const j = await publicJson<AniZipMappingsOnly>(`https://api.ani.zip/mappings?${param}`, { signal, retries: 1 });
    const m = j.mappings;
    if (!m?.anilist_id) return null;
    return { anilistId: m.anilist_id, malId: m.mal_id ?? undefined, kitsuId: m.kitsu_id ?? undefined };
  } catch (err) {
    if (err instanceof PublicApiError && err.status === 404) return null;
    throw err;
  }
}

/**
 * Fallback resolvers for kinds/ids ani.zip doesn't cover (manga). Registered by
 * the router with the enabled providers that implement `ids()`.
 */
type IdsResolver = (kind: MediaKind, ref: MediaRef, signal?: AbortSignal) => Promise<FullRef | null>;
let fallbackResolver: IdsResolver | null = null;
export function setFallbackIdsResolver(fn: IdsResolver | null): void {
  fallbackResolver = fn;
}

/** Bulk resolver (providers' `idsBatch`), tried before per-item lookups. */
type BatchIdsResolver = (kind: MediaKind, refs: MediaRef[], signal?: AbortSignal) => Promise<Array<FullRef | null>>;
let batchResolver: BatchIdsResolver | null = null;
export function setBatchIdsResolver(fn: BatchIdsResolver | null): void {
  batchResolver = fn;
}

const inFlight = new Map<string, Promise<FullRef | null>>();

/**
 * Resolve the full id set (anilistId required) for one title, or null when no
 * source knows it. Never throws for "unknown"; throws only on abort.
 */
export async function resolveIds(kind: MediaKind, ref: MediaRef, signal?: AbortSignal): Promise<FullRef | null> {
  const cached = cachedIds(kind, ref);
  if (cached && (cached.malId || !ref.malId)) return cached;
  const missKey = keysOf(kind, ref).join('|');
  const missAt = misses.get(missKey);
  if (missAt && Date.now() - missAt < MISS_TTL_MS) return cached;

  // Lanes overlap (trending/airing/seasonal): share one lookup per title.
  const pending = inFlight.get(missKey);
  if (pending) return pending;
  const run = lookupIds(kind, ref, missKey, cached, signal).finally(() => inFlight.delete(missKey));
  inFlight.set(missKey, run);
  return run;
}

async function lookupIds(kind: MediaKind, ref: MediaRef, missKey: string, cached: FullRef | null, signal?: AbortSignal): Promise<FullRef | null> {
  let found: FullRef | null = null;
  if (kind === 'ANIME') {
    try { found = await fromAniZip(ref, signal); } catch (err) { if (signal?.aborted) throw err; }
  }
  if (!found && fallbackResolver) {
    try { found = await fallbackResolver(kind, ref, signal); } catch (err) { if (signal?.aborted) throw err; }
  }
  if (found) {
    rememberIds(kind, { ...ref, ...found });
    return { ...ref, ...found };
  }
  misses.set(missKey, Date.now());
  return cached;
}

async function mapLimit<T, R>(items: T[], limit: number, fn: (item: T) => Promise<R>): Promise<R[]> {
  const out = new Array<R>(items.length);
  let next = 0;
  await Promise.all(Array.from({ length: Math.min(limit, items.length) }, async () => {
    while (next < items.length) {
      const i = next++;
      out[i] = await fn(items[i]);
    }
  }));
  return out;
}

/**
 * Fill `anilistId` on every item (resolving from malId/kitsuId) and drop items
 * that can't be resolved — app routes and local extensions need the AniList id.
 */
export async function ensureAnilistIds(kind: MediaKind, items: TatakaiMedia[], signal?: AbortSignal): Promise<TatakaiMedia[]> {
  const withCache = items.map((m) => {
    if (m.anilistId > 0) {
      if (m.malId || m.kitsuId) rememberIds(kind, { anilistId: m.anilistId, malId: m.malId, kitsuId: m.kitsuId });
      return m;
    }
    const hit = cachedIds(kind, { malId: m.malId, kitsuId: m.kitsuId });
    return hit ? { ...m, anilistId: hit.anilistId, malId: m.malId ?? hit.malId, kitsuId: m.kitsuId ?? hit.kitsuId } : m;
  });

  // One bulk lookup for the whole list before falling back to per-item calls.
  const fresh = withCache.filter((m) => !(m.anilistId > 0) && (m.malId || m.kitsuId)
    && !misses.has(keysOf(kind, { malId: m.malId, kitsuId: m.kitsuId }).join('|')));
  let batchCovered = false;
  if (fresh.length && batchResolver) {
    try {
      const found = await batchResolver(kind, fresh.map((m) => ({ malId: m.malId, kitsuId: m.kitsuId })), signal);
      batchCovered = true;
      fresh.forEach((m, i) => {
        const r = found[i];
        if (r?.anilistId) rememberIds(kind, { ...r, malId: m.malId ?? r.malId, kitsuId: m.kitsuId ?? r.kitsuId });
      });
    } catch (err) {
      if (signal?.aborted) throw err;
    }
  }

  const resolved = await mapLimit(withCache, 6, async (m) => {
    if (m.anilistId > 0) return m;
    const ref = { malId: m.malId, kitsuId: m.kitsuId };
    const hit = cachedIds(kind, ref);
    if (hit) return { ...m, anilistId: hit.anilistId, malId: m.malId ?? hit.malId, kitsuId: m.kitsuId ?? hit.kitsuId };
    // After a successful bulk pass, only anime gets a per-item retry (ani.zip
    // is anime-only and cheap); manga misses would just repeat slow lookups.
    if (batchCovered && kind === 'MANGA') {
      misses.set(keysOf(kind, ref).join('|'), Date.now());
      return m;
    }
    const ids = await resolveIds(kind, ref, signal);
    return ids?.anilistId ? { ...m, anilistId: ids.anilistId, malId: m.malId ?? ids.malId, kitsuId: m.kitsuId ?? ids.kitsuId } : m;
  });
  return resolved.filter((m) => m.anilistId > 0);
}
