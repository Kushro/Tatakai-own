/**
 * Shikimori metadata provider. GraphQL at shikimori.io/api/graphql (MAL ids). 5 req/s, 90 req/min.
 *
 * Implements the subset of MetadataProvider (see ../types.ts) this source can
 * serve. Owner: agent C.
 */

import type { MetadataProvider } from '@/lib/metadata/types';

export const shikimoriProvider: MetadataProvider = {
  id: 'shikimori',
  label: 'Shikimori',
  description: 'MyAnimeList-based GraphQL. Free, no key.',
  kinds: ['ANIME', 'MANGA'],
  isConfigured: () => true,
};
