import { useUiTranslation } from '@/hooks/ui/useUiTranslation';
import { useMutation, useQueryClient } from '@tanstack/react-query';

/**
 * Admin "publish news" mutation.
 *
 * Disabled in this fork: it depended on the upstream TatakaiAPI `/admin/news`
 * route. The mutation rejects with a clear message and never touches a network.
 */

export interface CreateNewsInput {
  title: string;
  content: string;
  excerpt?: string;
  image_url?: string;
  /** Optional source article URL — sets content_type=link and the dedup key. */
  link?: string;
  source?: string;
  tags?: string[];
  published_at?: string;
  is_pinned?: boolean;
}

export function useCreateNews() {
  useUiTranslation();
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: async (_input: CreateNewsInput): Promise<any> => {
      throw new Error('Publishing news is not available in this build.');
    },
    onSuccess: () => {
      // The News tab and For You feed both read forum_posts flair='News'.
      queryClient.invalidateQueries({ queryKey: ['feed'] });
    },
  });
}
