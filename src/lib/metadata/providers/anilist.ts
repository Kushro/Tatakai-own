/**
 * AniList metadata provider. GraphQL at graphql.anilist.co via anilistGql (@/lib/anilist/client).
 *
 * Implements the subset of MetadataProvider (see ../types.ts) this source can
 * serve. Owner: agent B.
 */

import type { MetadataProvider } from '@/lib/metadata/types';

export const anilistProvider: MetadataProvider = {
  id: 'anilist',
  label: 'AniList',
  description: 'Most complete source. Temporarily disabled by AniList (403) as of Oct 2026.',
  kinds: ['ANIME', 'MANGA'],
  isConfigured: () => true,
};
