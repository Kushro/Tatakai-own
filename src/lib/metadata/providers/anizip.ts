/**
 * ani.zip metadata provider. api.ani.zip/mappings: episode lists and id mappings (anime only). Not a catalog.
 *
 * Implements the subset of MetadataProvider (see ../types.ts) this source can
 * serve. Owner: agent D.
 */

import type { MetadataProvider } from '@/lib/metadata/types';

export const anizipProvider: MetadataProvider = {
  id: 'anizip',
  label: 'ani.zip',
  description: 'Episodes and id mapping only.',
  kinds: [],
  isConfigured: () => true,
};
