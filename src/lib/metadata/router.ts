/**
 * Metadata router: calls providers in the user's priority order
 * (`@/lib/metadata/settings`), skipping disabled/unconfigured/unsupported ones
 * and ones in a cool-down after failing, and falls back to the next provider on
 * error or empty result.
 *
 * Consumers use the `metadata` facade at the bottom; they never call a provider
 * directly.
 */

import type { ContentSearchResult, SearchFilters, TatakaiMedia } from '@/core/content/types';
import { AniListError } from '@/lib/anilist/client';
import { PublicApiError } from '@/lib/net/publicApi';
import { ensureAnilistIds, resolveIds, setFallbackIdsResolver, rememberIds } from '@/lib/metadata/ids';
import { getMetadataSources } from '@/lib/metadata/settings';
import {
  providerSupports,
  type FeedOptions,
  type FeedPage,
  type MediaKind,
  type MediaRef,
  type MetadataCapability,
  type MetadataEpisode,
  type MetadataFeedKind,
  type MetadataProvider,
  type ProviderId,
} from '@/lib/metadata/types';

const registry = new Map<ProviderId, MetadataProvider>();

export function registerMetadataProviders(providers: MetadataProvider[]): void {
  for (const p of providers) registry.set(p.id, p);
}

export function getMetadataProvider(id: ProviderId): MetadataProvider | undefined {
  return registry.get(id);
}

export function listMetadataProviders(): MetadataProvider[] {
  return [...registry.values()];
}

// ── Health / cool-down ───────────────────────────────────────────────────────

export interface ProviderHealth {
  downUntil: number;
  lastError?: string;
  lastOkAt?: number;
}
const health = new Map<ProviderId, ProviderHealth>();

/** How long to skip a provider after this error (0 = don't trip). */
function cooldownFor(err: unknown): number {
  const status = err instanceof AniListError || err instanceof PublicApiError ? err.status : undefined;
  if (status === 403 || status === 401) return 15 * 60_000; // API disabled / auth required
  if (status === 429) return 2 * 60_000;
  if (status === undefined || status >= 500) return 3 * 60_000; // network / server
  return 0; // 400/404: the request, not the provider
}

function markFailure(id: ProviderId, err: unknown): void {
  const ms = cooldownFor(err);
  const prev = health.get(id) || { downUntil: 0 };
  health.set(id, { ...prev, downUntil: ms ? Date.now() + ms : prev.downUntil, lastError: err instanceof Error ? err.message : String(err) });
}

function markOk(id: ProviderId): void {
  health.set(id, { downUntil: 0, lastOkAt: Date.now() });
}

export function getProviderHealth(): Record<string, ProviderHealth> {
  return Object.fromEntries(health);
}

/** Clear cool-downs (e.g. after the user reorders sources or retries). */
export function resetProviderHealth(): void {
  health.clear();
}

// ── Core fallback loop ───────────────────────────────────────────────────────

export class NoMetadataProviderError extends Error {
  constructor(readonly capability: MetadataCapability, readonly errors: Array<{ provider: ProviderId; error: string }>) {
    super(errors.length
      ? `All metadata sources failed for ${capability}: ${errors.map((e) => `${e.provider}: ${e.error}`).join('; ')}`
      : `No enabled metadata source supports ${capability}`);
    this.name = 'NoMetadataProviderError';
  }
}

/** Enabled, configured providers that support `cap`, in user priority order. */
export function orderedProviders(cap: MetadataCapability, kind?: MediaKind, includeCoolingDown = false): MetadataProvider[] {
  const now = Date.now();
  return getMetadataSources()
    .filter((s) => s.enabled)
    .map((s) => registry.get(s.id))
    .filter((p): p is MetadataProvider => Boolean(p && p.isConfigured() && providerSupports(p, cap, kind)))
    .filter((p) => includeCoolingDown || (health.get(p.id)?.downUntil ?? 0) <= now);
}

export interface RoutedResult<T> {
  value: T;
  provider: ProviderId;
}

/**
 * Try each provider in order until one returns a non-empty value.
 * When every provider is cooling down, they are retried anyway (last resort).
 */
export async function tryProviders<T>(
  cap: MetadataCapability,
  kind: MediaKind | undefined,
  call: (p: MetadataProvider) => Promise<T>,
  opts: { isEmpty?: (v: T) => boolean; signal?: AbortSignal } = {},
): Promise<RoutedResult<T>> {
  let candidates = orderedProviders(cap, kind);
  if (!candidates.length) candidates = orderedProviders(cap, kind, true);
  const errors: Array<{ provider: ProviderId; error: string }> = [];
  let empty: RoutedResult<T> | null = null;
  for (const p of candidates) {
    opts.signal?.throwIfAborted();
    try {
      const value = await call(p);
      markOk(p.id);
      if (opts.isEmpty?.(value)) {
        empty ??= { value, provider: p.id };
        continue;
      }
      return { value, provider: p.id };
    } catch (err) {
      if (opts.signal?.aborted) throw err;
      markFailure(p.id, err);
      errors.push({ provider: p.id, error: err instanceof Error ? err.message : String(err) });
    }
  }
  if (empty) return empty;
  throw new NoMetadataProviderError(cap, errors);
}

// Manga (and anime ani.zip misses) resolve ids through providers' ids().
setFallbackIdsResolver(async (kind, ref, signal) => {
  for (const p of orderedProviders('ids', kind)) {
    if (p.id === 'anizip' && kind === 'MANGA') continue;
    try {
      const r = await p.ids!(ref, { signal });
      if (r?.anilistId) return r;
    } catch (err) {
      if (signal?.aborted) throw err;
    }
  }
  return null;
});

// ── Facade used by content clients ───────────────────────────────────────────

async function withIds(kind: MediaKind, ref: MediaRef, signal?: AbortSignal): Promise<MediaRef> {
  if (ref.anilistId && ref.malId && ref.kitsuId) return ref;
  const full = await resolveIds(kind, ref, signal);
  return { ...ref, ...(full || {}) };
}

export const metadata = {
  async feed(kind: MediaKind, feed: MetadataFeedKind, opts: FeedOptions = {}): Promise<RoutedResult<FeedPage>> {
    const r = await tryProviders('feed', kind, (p) => p.feed!(kind, feed, opts), { isEmpty: (v) => !v.media.length, signal: opts.signal });
    return { ...r, value: { ...r.value, media: await ensureAnilistIds(kind, r.value.media, opts.signal) } };
  },

  async search(kind: MediaKind, filters: SearchFilters & { novelsOnly?: boolean; countryOfOrigin?: string; signal?: AbortSignal }): Promise<RoutedResult<ContentSearchResult>> {
    const r = await tryProviders('search', kind, (p) => p.search!(kind, filters), { isEmpty: (v) => !v.media.length, signal: filters.signal });
    return { ...r, value: { ...r.value, media: await ensureAnilistIds(kind, r.value.media, filters.signal) } };
  },

  /** One title; null when no enabled source knows it. */
  async media(kind: MediaKind, ref: MediaRef, opts: { signal?: AbortSignal } = {}): Promise<RoutedResult<TatakaiMedia | null>> {
    const full = await withIds(kind, ref, opts.signal);
    try {
      const r = await tryProviders('media', kind, (p) => p.media!(kind, full, opts), { isEmpty: (v) => !v, signal: opts.signal });
      const m = r.value;
      if (m && !(m.anilistId > 0) && full.anilistId) m.anilistId = full.anilistId;
      if (m?.anilistId) rememberIds(kind, { anilistId: m.anilistId, malId: m.malId ?? full.malId, kitsuId: m.kitsuId ?? full.kitsuId });
      return r;
    } catch (err) {
      if (err instanceof NoMetadataProviderError && !err.errors.length) return { value: null, provider: 'anilist' };
      throw err;
    }
  },

  async episodes(ref: MediaRef, opts: { signal?: AbortSignal } = {}): Promise<RoutedResult<MetadataEpisode[]>> {
    const full = await withIds('ANIME', ref, opts.signal);
    return tryProviders('episodes', 'ANIME', (p) => p.episodes!(full, opts), { isEmpty: (v) => !v.length, signal: opts.signal });
  },

  async genres(kind: MediaKind, opts: { includeAdult?: boolean; signal?: AbortSignal } = {}): Promise<RoutedResult<string[]>> {
    return tryProviders('genres', kind, (p) => p.genres!(kind, opts), { isEmpty: (v) => !v.length, signal: opts.signal });
  },
};
