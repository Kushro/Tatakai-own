/**
 * Tenrai metadata provider. MyAnimeList data, Jikan v4-compatible REST at api.tenrai.org/v1 (tenraiGet).
 *
 * Implements the subset of MetadataProvider (see ../types.ts) this source can
 * serve. Owner: agent C.
 */

import type { MetadataProvider } from '@/lib/metadata/types';

export const tenraiProvider: MetadataProvider = {
  id: 'tenrai',
  label: 'Tenrai',
  description: 'MyAnimeList data (Jikan successor). Free, no key.',
  kinds: ['ANIME', 'MANGA'],
  isConfigured: () => true,
};
