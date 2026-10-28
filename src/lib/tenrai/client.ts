/**
 * Tenrai v1 client (https://api.tenrai.org/v1) — MyAnimeList-derived data with
 * a Jikan v4-compatible schema. Jikan's public API shut down on 2026-10-01;
 * Tenrai is its drop-in replacement. Paced by publicApi/hostLimiter.
 *
 * Paths are relative to the v1 base, e.g. `tenraiGet('/anime/1/episodes', { page: 2 })`.
 */

import { publicJson } from '@/lib/net/publicApi';

export const TENRAI_BASE = 'https://api.tenrai.org/v1';

export async function tenraiGet<T>(
  path: string,
  params?: Record<string, string | number | boolean | undefined | null>,
  signal?: AbortSignal,
): Promise<T> {
  const url = new URL(`${TENRAI_BASE}${path.startsWith('/') ? path : `/${path}`}`);
  for (const [k, v] of Object.entries(params ?? {})) {
    if (v !== undefined && v !== null && v !== '') url.searchParams.set(k, String(v));
  }
  return publicJson<T>(url.toString(), { signal });
}
