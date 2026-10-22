import { useUiTranslation } from '@/hooks/ui/useUiTranslation';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';

/**
 * Admin news management — list / refresh / pin / delete of the bot-authored
 * News posts. These all go through the JWT-guarded TatakaiAPI `/admin/news`
 * routes because bot-owned forum_posts rows can't be read-for-management,
 * edited, or deleted from the browser under RLS. Auth = the admin's live
 * Supabase access token as a Bearer header (no client-shipped secret).
 */

export interface AdminNewsPost {
  id: string;
  title: string;
  content: string;
  image_url: string | null;
  is_pinned: boolean;
  created_at: string;
  metadata: {
    source?: string;
    news_url?: string | null;
    news_link?: string;
    tags?: string[];
    excerpt?: string | null;
    manual?: boolean;
    published_at?: string;
  } | null;
}

export interface NewsRefreshResult {
  aggregated: number;
  inserted: number;
  existing: number;
  purged: number;
}

/** Disabled: the `/admin/news` routes belonged to the upstream TatakaiAPI backend. */
async function adminNewsFetch<T>(_path: string, _init: RequestInit = {}): Promise<T> {
  throw new Error('News administration is not available in this build.');
}

/** Recent bot-authored News posts (pinned first, then newest). */
export function useNewsPosts(page = 1, limit = 30) {
  useUiTranslation();
  return useQuery({
    queryKey: ['admin-news', page, limit],
    queryFn: async (): Promise<AdminNewsPost[]> => [],
  });
}

/** Run one ingestion pass now (pull upstream sources → insert new stories). */
export function useRefreshNews() {
  useUiTranslation();
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: () => adminNewsFetch<NewsRefreshResult>('/refresh', { method: 'POST' }),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['admin-news'] });
      queryClient.invalidateQueries({ queryKey: ['feed'] });
    },
  });
}

/** Pin / unpin a News post. */
export function useSetNewsPinned() {
  useUiTranslation();
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (args: { id: string; is_pinned: boolean }) =>
      adminNewsFetch<{ id: string; is_pinned: boolean }>(`/${args.id}`, {
        method: 'PATCH',
        body: JSON.stringify({ is_pinned: args.is_pinned }),
      }),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['admin-news'] });
      queryClient.invalidateQueries({ queryKey: ['feed'] });
    },
  });
}

/** Delete a News post. */
export function useDeleteNews() {
  useUiTranslation();
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (id: string) => adminNewsFetch<{ id: string }>(`/${id}`, { method: 'DELETE' }),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['admin-news'] });
      queryClient.invalidateQueries({ queryKey: ['feed'] });
    },
  });
}
