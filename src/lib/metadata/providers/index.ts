import { anilistProvider } from '@/lib/metadata/providers/anilist';
import { anizipProvider } from '@/lib/metadata/providers/anizip';
import { kitsuProvider } from '@/lib/metadata/providers/kitsu';
import { malProvider } from '@/lib/metadata/providers/mal';
import { shikimoriProvider } from '@/lib/metadata/providers/shikimori';
import { tenraiProvider } from '@/lib/metadata/providers/tenrai';

export const ALL_METADATA_PROVIDERS = [anilistProvider, tenraiProvider, kitsuProvider, shikimoriProvider, malProvider, anizipProvider];
