/**
 * ani.zip metadata provider. api.ani.zip/mappings: episode lists and id mappings (anime only). Not a catalog.
 *
 * Implements the subset of MetadataProvider (see ../types.ts) this source can
 * serve: `episodes` and `ids`. Fetching and normalizing are shared with the
 * mapping layer (`@/lib/mapping/anizip`), so one request feeds both.
 */

import { fetchAniZipRaw, normalizeAniZip, type AniZipEpisode } from '@/lib/mapping/anizip';
import type { MediaRef, MetadataEpisode, MetadataProvider } from '@/lib/metadata/types';

function toMetadataEpisode(e: AniZipEpisode): MetadataEpisode {
  return {
    // ani.zip keys episodes per AniList entry (1-based), matching `number`.
    number: e.number,
    title: e.title ?? undefined,
    titles: Object.keys(e.titles).length ? e.titles : undefined,
    overview: e.overview ?? undefined,
    image: e.image ?? undefined,
    airDate: e.airDateUtc ?? e.airDate ?? undefined,
    runtimeMin: e.runtime ?? undefined,
  };
}

async function episodes(ref: MediaRef, opts: { signal?: AbortSignal } = {}): Promise<MetadataEpisode[]> {
  if (!ref.anilistId && !ref.malId && !ref.kitsuId) return [];
  const raw = await fetchAniZipRaw(ref, { signal: opts.signal });
  if (!raw) return [];
  // Regular episodes only; specials ("S1") are not part of the numbered list.
  return normalizeAniZip(raw, ref.anilistId ?? 0).episodes.map(toMetadataEpisode);
}

async function ids(ref: MediaRef, opts: { signal?: AbortSignal } = {}): Promise<(Required<Pick<MediaRef, 'anilistId'>> & MediaRef) | null> {
  if (!ref.anilistId && !ref.malId && !ref.kitsuId) return null;
  const raw = await fetchAniZipRaw(ref, { signal: opts.signal });
  const m = raw?.mappings;
  if (!m?.anilist_id) return null;
  return { anilistId: m.anilist_id, malId: m.mal_id ?? undefined, kitsuId: m.kitsu_id ?? undefined };
}

export const anizipProvider: MetadataProvider = {
  id: 'anizip',
  label: 'ani.zip',
  description: 'Episodes and id mapping only.',
  kinds: [],
  isConfigured: () => true,
  episodes,
  ids,
};
