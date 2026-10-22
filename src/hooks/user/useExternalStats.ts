import { useQuery } from '@tanstack/react-query';
import { fetchAniListStatistics, fetchMALStatistics, type ExternalStats } from '@/lib/externalIntegrations';
import { getValidMalAccessToken } from '@/lib/mal/auth';

interface ProfileTokens {
  anilist_access_token?: string | null;
  mal_access_token?: string | null;
  anilist_user_id?: string | null;
  mal_user_id?: string | null;
}

/**
 * Fetches normalized AniList / MyAnimeList statistics for the connected user,
 * straight from AniList (GraphQL, the user's own token) and MAL (REST v2, token
 * refreshed on demand). A provider that is down or has an expired login is
 * skipped quietly; the other one still renders. Only the profile owner's linked
 * accounts are read, so pass `enabled: false` when viewing others.
 */
export function useExternalStats(profile: ProfileTokens | null | undefined, enabled = true) {
  const hasAnilist = !!(profile?.anilist_access_token || profile?.anilist_user_id);
  const hasMal = !!(profile?.mal_access_token || profile?.mal_user_id);
  const anyLinked = hasAnilist || hasMal;
  const anilistToken = profile?.anilist_access_token ?? null;

  const { data, isLoading } = useQuery<ExternalStats[]>({
    queryKey: ['external-stats', hasAnilist, hasMal, !!anilistToken],
    enabled: enabled && anyLinked,
    staleTime: 5 * 60 * 1000,
    retry: false,
    queryFn: async () => {
      const jobs: Array<Promise<ExternalStats>> = [];
      if (hasAnilist && anilistToken) jobs.push(fetchAniListStatistics(anilistToken));
      if (hasMal) jobs.push(getValidMalAccessToken().then(fetchMALStatistics));
      const settled = await Promise.allSettled(jobs);
      return settled.flatMap((r) => (r.status === 'fulfilled' ? [r.value] : []));
    },
  });

  return {
    stats: data ?? [],
    hasAnilist,
    hasMal,
    isLoading: enabled && anyLinked && isLoading,
  };
}
