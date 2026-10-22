/**
 * Public entry point for catalog metadata. Importing it registers every
 * provider; consumers then call `metadata.feed/search/media/episodes/genres`.
 */

import { ALL_METADATA_PROVIDERS } from '@/lib/metadata/providers';
import { registerMetadataProviders } from '@/lib/metadata/router';

registerMetadataProviders(ALL_METADATA_PROVIDERS);

export { metadata, getProviderHealth, resetProviderHealth, listMetadataProviders, getMetadataProvider, NoMetadataProviderError } from '@/lib/metadata/router';
export type { RoutedResult, ProviderHealth } from '@/lib/metadata/router';
export * from '@/lib/metadata/settings';
export * from '@/lib/metadata/types';
export { resolveIds, ensureAnilistIds, rememberIds } from '@/lib/metadata/ids';
