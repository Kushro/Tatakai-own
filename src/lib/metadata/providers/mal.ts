/**
 * MyAnimeList metadata provider. Official API v2 at api.myanimelist.net/v2 with X-MAL-CLIENT-ID (VITE_MAL_CLIENT_ID). No CORS: native builds only.
 *
 * Implements the subset of MetadataProvider (see ../types.ts) this source can
 * serve. Owner: agent C.
 */

import type { MetadataProvider } from '@/lib/metadata/types';

export const malProvider: MetadataProvider = {
  id: 'mal',
  label: 'MyAnimeList',
  description: 'Official API. Needs a MAL client id.',
  kinds: ['ANIME', 'MANGA'],
  isConfigured: () => Boolean(String(import.meta.env.VITE_MAL_CLIENT_ID || '').trim()),
};
