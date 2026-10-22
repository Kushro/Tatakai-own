/**
 * src/hooks/user/useIntegrationSync.ts
 *
 * Sync state and actions for the MyAnimeList and AniList integrations.
 *
 * Everything runs on the device (see `@/lib/listSync`): the preview reads the
 * remote list straight from AniList / MAL with the user's own token and diffs it
 * against the local Supabase lists; `applySync` writes the user-approved items.
 * No TatakaiAPI `/sync/*` call is made and the Supabase session never leaves
 * Supabase.
 *
 *  - Last-sync timestamp lives in localStorage (no DB needed)
 *  - Sync is always previewed first and applied only after user approval
 */

import { useState, useCallback } from 'react';
import { useQueryClient } from '@tanstack/react-query';
import {
  applySync as applyListSync,
  previewAllSync,
  previewSync,
  pushSingleItem,
  type Integration,
  type MediaType,
  type SyncProposalItem,
} from '@/lib/listSync';

export type { Integration, MediaType, SyncProposalItem };

export interface SyncState {
  isLoadingPreview: boolean;
  isApplying: boolean;
  previewItems: SyncProposalItem[];
  lastSyncedAt: string | null;
  error: string | null;
}

const LAST_SYNC_KEY = (integration: string, mediaType: string) =>
  `tatakai:sync:lastAt:${integration}:${mediaType}`;

const messageOf = (err: unknown) => (err instanceof Error ? err.message : String(err));

export function useIntegrationSync() {
  const queryClient = useQueryClient();
  const [state, setState] = useState<SyncState>({
    isLoadingPreview: false,
    isApplying: false,
    previewItems: [],
    lastSyncedAt: null,
    error: null,
  });

  /** Load the last sync timestamp for a given integration + mediaType combo. */
  const getLastSyncedAt = useCallback((integration: Integration, mediaType: MediaType): string | null => {
    try {
      return localStorage.getItem(LAST_SYNC_KEY(integration, mediaType));
    } catch {
      return null;
    }
  }, []);

  const runPreview = useCallback(async (load: () => Promise<SyncProposalItem[]>): Promise<SyncProposalItem[]> => {
    setState(s => ({ ...s, isLoadingPreview: true, error: null, previewItems: [] }));
    try {
      const items = await load();
      setState(s => ({ ...s, isLoadingPreview: false, previewItems: items }));
      return items;
    } catch (err) {
      setState(s => ({ ...s, isLoadingPreview: false, error: messageOf(err) }));
      return [];
    }
  }, []);

  /** Diff for one media type. Results land in state.previewItems for the approval modal. */
  const loadPreview = useCallback(
    (integration: Integration, mediaType: MediaType) => runPreview(() => previewSync(integration, mediaType)),
    [runPreview],
  );

  /** Unified preview: anime, manga and novels for one integration in a single modal. */
  const loadUnifiedPreview = useCallback(
    (integration: Integration) => runPreview(() => previewAllSync(integration)),
    [runPreview],
  );

  /**
   * Apply a set of user-confirmed sync actions.
   * Always called AFTER user approves via the SyncPreviewModal.
   */
  const applySync = useCallback(async (
    integration: Integration,
    actions: SyncProposalItem[],
  ): Promise<{ success: boolean; error?: string }> => {
    setState(s => ({ ...s, isApplying: true, error: null }));
    try {
      const result = await applyListSync(integration, actions);
      await queryClient.invalidateQueries({ predicate: query => /watch|history|readlist|stats/i.test(String(query.queryKey[0])) });
      if (result.applied > 0) {
        const mediaTypes = [...new Set(actions.map(a => a.type))] as MediaType[];
        for (const mt of mediaTypes) {
          try {
            localStorage.setItem(LAST_SYNC_KEY(integration, mt), new Date().toISOString());
          } catch {
            // storage error, ignore
          }
        }
      }
      const failed = result.failed.length;
      const error = failed ? `${failed} of ${actions.length} items could not be synced: ${result.failed[0].error}` : undefined;
      setState(s => ({
        ...s,
        isApplying: false,
        previewItems: [],
        lastSyncedAt: result.applied > 0 ? new Date().toISOString() : s.lastSyncedAt,
        error: error ?? null,
      }));
      return error ? { success: false, error } : { success: true };
    } catch (err) {
      const error = messageOf(err);
      setState(s => ({ ...s, isApplying: false, error }));
      return { success: false, error };
    }
  }, [queryClient]);

  /**
   * Sends a single-item realtime sync (e.g. after episode completion) to every
   * linked tracker that knows the title. Bypasses the preview modal.
   */
  const singleItemSync = useCallback(async (params: {
    type: 'anime' | 'manga';
    malId?: number | null;
    anilistId?: number | null;
    status: string;
    progress: number;
    score?: number;
  }): Promise<void> => {
    try {
      await pushSingleItem(params);
    } catch (err) {
      console.warn('[useIntegrationSync] Single item sync failed:', err);
    }
  }, []);

  return {
    ...state,
    getLastSyncedAt,
    loadPreview,
    loadUnifiedPreview,
    applySync,
    singleItemSync,
  };
}
