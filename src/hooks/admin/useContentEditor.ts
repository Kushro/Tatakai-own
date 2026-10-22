import { useUiTranslation } from '@/hooks/ui/useUiTranslation';
import { useMutation, useQueryClient } from '@tanstack/react-query';
import { contentGraph } from '@/core/content/content-graph';
import type { TatakaiMedia } from '@/core/content/types';

/**
 * Admin content-editor mutation.
 *
 * Disabled in this fork: it depended on the upstream TatakaiAPI
 * `/admin/content/:id` route. The mutation rejects with a clear message.
 */

export interface UpdateContentInput {
  /** AniList numeric id (as string) or tatakai uuid. */
  id: string;
  /** content_items columns → new values (only whitelisted keys are applied). */
  fields: Record<string, unknown>;
  reason?: string;
}

export function useUpdateContent() {
  useUiTranslation();
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: async (_input: UpdateContentInput): Promise<TatakaiMedia> => {
      throw new Error('Content editing is not available in this build.');
    },
    onSuccess: async (_data, variables) => {
      await contentGraph.purgeMedia(variables.id);
      // Refetch the info pages / feeds that may show the edited item.
      queryClient.invalidateQueries();
    },
  });
}
