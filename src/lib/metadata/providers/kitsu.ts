/**
 * Kitsu metadata provider. JSON:API at kitsu.io/api/edge; includes AniList/MAL id mappings.
 *
 * Implements the subset of MetadataProvider (see ../types.ts) this source can
 * serve. Owner: agent D.
 */

import type { MetadataProvider } from '@/lib/metadata/types';

export const kitsuProvider: MetadataProvider = {
  id: 'kitsu',
  label: 'Kitsu',
  description: 'Free, no key. Trending updates slowly.',
  kinds: ['ANIME', 'MANGA'],
  isConfigured: () => true,
};
