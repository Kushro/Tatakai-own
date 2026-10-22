/**
 * Cache + in-flight dedupe for the client-side mapping layer.
 *
 * ani.zip and Kitsu are called straight from the renderer through
 * `@/lib/net/publicApi` (paced, retried, native HTTP in the apps). MangaBaka is
 * disabled in this fork. The responses are read on nearly every page mount, so a
 * short-lived same-process cache sits in front of them.
 *
 * `loadMapping` resolves to `null` instead of throwing by default. All sources
 * are enrichment on top of catalog data, so a miss must degrade to "no extra
 * detail", never to a failed page render. Pass `rethrow` when the caller (a
 * metadata provider) must distinguish "failed" from "no record" so the router
 * can fall back; failures are then never cached.
 */

import { evictToCap, sweepExpired } from "@/lib/cache/boundedMap";
import { getProfileKnobs } from "@/lib/memoryProfile";

/** How long a transport failure is remembered (so a down host isn't hammered). */
const ERROR_TTL_MS = 60_000;

interface CacheEntry {
  at: number;
  ttlMs: number;
  value: unknown;
}

const cache = new Map<string, CacheEntry>();
const inFlight = new Map<string, Promise<unknown>>();

/**
 * Store a cache entry and enforce the memory-profile entry cap. Every write path
 * (hit, miss, error) funnels through here so the map can never outgrow the cap.
 */
function setCache(key: string, entry: CacheEntry): void {
  cache.set(key, entry);
  evictToCap(cache, getProfileKnobs().cacheCaps.mapping);
}

export interface MappingLoadOptions {
  ttlMs: number;
  signal?: AbortSignal;
  /** Bypass the cache and any in-flight request. */
  force?: boolean;
  /** Propagate loader errors (uncached) instead of resolving to null. */
  rethrow?: boolean;
}

/**
 * Run `load` once per `key` within the TTL. `load` returns the value, or null
 * when the upstream has no record (cached like any other answer so an unmapped
 * title doesn't re-request on every mount).
 */
export async function loadMapping<T>(
  key: string,
  load: (signal: AbortSignal | undefined) => Promise<T | null>,
  options: MappingLoadOptions,
): Promise<T | null> {
  const settle = (p: Promise<T | null>) => (options.rethrow ? p : p.catch(() => null));

  if (!options.force) {
    const hit = cache.get(key);
    if (hit && Date.now() - hit.at < hit.ttlMs) return hit.value as T | null;
    const pending = inFlight.get(key);
    if (pending) return settle(pending as Promise<T | null>);
  }

  const request = (async (): Promise<T | null> => {
    try {
      // Deferred so `inFlight.set` below always runs before the `finally`.
      const value = (await Promise.resolve().then(() => load(options.signal))) ?? null;
      setCache(key, { at: Date.now(), ttlMs: options.ttlMs, value });
      return value;
    } catch (err) {
      if (options.rethrow) throw err;
      if (!options.signal?.aborted) setCache(key, { at: Date.now(), ttlMs: ERROR_TTL_MS, value: null });
      return null;
    } finally {
      inFlight.delete(key);
    }
  })();

  inFlight.set(key, request);
  return settle(request);
}

/** Drop cached mapping responses whose key contains `match`, or all of them. */
export function clearMappingCache(match?: string): void {
  if (!match) {
    cache.clear();
    return;
  }
  for (const key of [...cache.keys()]) {
    if (key.includes(match)) cache.delete(key);
  }
}

/** Remove expired mapping entries; called from the idle sweep. Returns count. */
export function sweepMappingCache(now: number = Date.now()): number {
  try {
    return sweepExpired(cache, (e) => e.at + e.ttlMs, now);
  } catch {
    return 0;
  }
}
