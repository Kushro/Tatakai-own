/**
 * User-configurable metadata source priority (Settings → Metadata sources).
 *
 * The first enabled provider is the primary source; the rest are fallbacks in
 * order. Stored per device in localStorage; changes notify subscribers so the
 * settings screen and the router stay in sync.
 */

import type { ProviderId } from '@/lib/metadata/types';

export interface MetadataSourceSetting {
  id: ProviderId;
  enabled: boolean;
}

/** Default priority agreed for this fork. ani.zip is episodes/ids only. */
export const DEFAULT_METADATA_SOURCES: MetadataSourceSetting[] = [
  { id: 'anilist', enabled: true },
  { id: 'tenrai', enabled: true },
  { id: 'kitsu', enabled: true },
  { id: 'shikimori', enabled: true },
  { id: 'mal', enabled: true },
  { id: 'anizip', enabled: true },
];

const STORAGE_KEY = 'tatakai:metadata-sources:v1';
const listeners = new Set<(s: MetadataSourceSetting[]) => void>();

function normalize(list: unknown): MetadataSourceSetting[] {
  const known = new Set(DEFAULT_METADATA_SOURCES.map((s) => s.id));
  const out: MetadataSourceSetting[] = [];
  if (Array.isArray(list)) {
    for (const item of list) {
      const id = (item as MetadataSourceSetting)?.id;
      if (known.has(id) && !out.some((s) => s.id === id)) out.push({ id, enabled: (item as MetadataSourceSetting).enabled !== false });
    }
  }
  // Providers added after the user saved their order go to the end, enabled.
  for (const d of DEFAULT_METADATA_SOURCES) if (!out.some((s) => s.id === d.id)) out.push({ ...d });
  return out;
}

export function getMetadataSources(): MetadataSourceSetting[] {
  try {
    const raw = typeof localStorage !== 'undefined' ? localStorage.getItem(STORAGE_KEY) : null;
    return normalize(raw ? JSON.parse(raw) : null);
  } catch {
    return normalize(null);
  }
}

export function setMetadataSources(list: MetadataSourceSetting[]): void {
  const next = normalize(list);
  try { localStorage.setItem(STORAGE_KEY, JSON.stringify(next)); } catch { /* storage unavailable: keep in-memory only */ }
  for (const fn of listeners) fn(next);
}

export function resetMetadataSources(): void {
  setMetadataSources(DEFAULT_METADATA_SOURCES);
}

export function subscribeMetadataSources(fn: (s: MetadataSourceSetting[]) => void): () => void {
  listeners.add(fn);
  return () => listeners.delete(fn);
}
