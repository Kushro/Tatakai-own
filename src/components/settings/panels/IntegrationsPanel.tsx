import uiI18n from '@/lib/uiI18n';
import { useUiTranslation } from '@/hooks/ui/useUiTranslation';
import { useState, useCallback, useEffect } from 'react';
import { Button } from '@/components/ui/button';
import { Loader2, Link2, Trash2, CheckCircle, RefreshCw, ExternalLink } from 'lucide-react';
import { useAuth } from '@/contexts/AuthContext';
import { getMalAuthUrl, disconnectMal, isMalLoginPendingFresh, clearMalOAuthStorage } from '@/lib/mal';
import { isMalConfigured } from '@/lib/mal/auth';
import { isAniListConfigured } from '@/lib/anilist/auth';
import {
  MANGABAKA_LOGO_URL,
  MANGABAKA_UNAVAILABLE_MESSAGE,
} from '@/lib/mangabaka';
import { getAniListAuthUrl, disconnectAniList } from '@/lib/externalIntegrations';
import { integrationAvailability, isNativeOAuthApp, openOAuthInBrowser, type IntegrationAvailability } from '@/lib/desktopOAuth';
import { toast } from 'sonner';
import { SyncPreviewModal } from '@/components/integrations/SyncPreviewModal';
import { useIntegrationSync, type Integration, type MediaType, type SyncProposalItem } from '@/hooks/user/useIntegrationSync';
import {
  SettingsBadge,
  SettingsEmptyState,
  SettingsSection,
} from '@/components/settings/SettingsPrimitives';

export function IntegrationsPanel() {
  useUiTranslation();
  const { user, profile, refreshProfile } = useAuth();

  // MangaBaka is disabled in this fork (no backend for its token exchange).
  const [mangabakaLogoFailed, setMangabakaLogoFailed] = useState(false);

  // Whether Connect can work in this build: needs a client id, and the
  // tatakai:// deep link only exists in the desktop and Android apps.
  const malAvailability: IntegrationAvailability = integrationAvailability(isMalConfigured());
  const anilistAvailability: IntegrationAvailability = integrationAvailability(isAniListConfigured());
  const availabilityNote = (a: IntegrationAvailability): string | null =>
    a === 'not_configured'
      ? 'Not configured in this build (the client id was not set when the app was built).'
      : a === 'native_only'
        ? 'Account linking works in the desktop and Android apps.'
        : null;

  // Smart Sync (approval-modal) State
  const integrationSync = useIntegrationSync();
  const [syncModalOpen, setSyncModalOpen] = useState(false);
  const [syncModalIntegration, setSyncModalIntegration] = useState<Integration>('mal');
  const [syncModalMediaType, setSyncModalMediaType] = useState<MediaType | undefined>(undefined);

  const openLoginUrl = async (url: string, providerName: string) => {
    if (isNativeOAuthApp() && (await openOAuthInBrowser(url))) {
      toast.info(`Complete the ${providerName} login in your browser. The app opens again when you finish.`);
      return;
    }
    toast.error(`Could not open your browser for ${providerName} login.`, {
      action: {
        label: 'Copy link',
        onClick: () => {
          void navigator.clipboard?.writeText(url).then(
            () => toast.success('Login link copied. Paste it into your browser.'),
            () => toast.error('Copy failed. Please try again.'),
          );
        },
      },
    });
  };

  const handleMALConnect = async (force = false) => {
    try {
      // Each Connect click mints a fresh PKCE verifier, invalidating any MAL
      // tab opened by a previous click, so restarting is made explicit.
      if (!force && isMalLoginPendingFresh()) {
        toast.warning('A MyAnimeList login is already in progress', {
          description: 'Only the newest MyAnimeList tab can complete. Finish it, or discard it and restart.',
          duration: 10000,
          action: {
            label: 'Discard and restart',
            onClick: () => {
              clearMalOAuthStorage();
              void handleMALConnect(true);
            },
          },
        });
        return;
      }
      await openLoginUrl(getMalAuthUrl(), 'MyAnimeList');
    } catch (err) {
      toast.error(err instanceof Error ? err.message : 'Could not open MyAnimeList login.');
    }
  };

  const handleAniListConnect = async () => {
    try {
      await openLoginUrl(getAniListAuthUrl(), 'AniList');
    } catch (err) {
      toast.error(err instanceof Error ? err.message : 'Could not open AniList login. Please try again.');
    }
  };

  const handleMALDisconnect = async () => {
    if (!user) return;
    try {
      await disconnectMal(user.id);
      await refreshProfile();
      toast.success(uiI18n.t('app.myanimelist_disconnected_44575c90'));
    } catch {
      toast.error(uiI18n.t('app.failed_to_disconnect_2ec12e67'));
    }
  };

  const handleAniListDisconnect = async () => {
    if (!user) return;
    try {
      await disconnectAniList(user.id);
      await refreshProfile();
      toast.success(uiI18n.t('app.anilist_disconnected_86c071cc'));
    } catch {
      toast.error(uiI18n.t('app.failed_to_disconnect_2ec12e67'));
    }
  };

  // Use the non-sensitive identity columns to detect a linked account. The
  // access-token columns are not always returned to the client (column-level
  // grants), so keying "Connected" off the token left the badge stuck on
  // "Connect" even after a successful link. user_id/username are always cleared
  // on disconnect, so they're a reliable connection signal.
  const hasMAL = !!(profile?.mal_user_id || (profile as any)?.mal_access_token);
  const hasAniList = !!(
    profile?.anilist_user_id ||
    (profile as any)?.anilist_username ||
    (profile as any)?.anilist_access_token
  );

  /**
   * Per-service full sync: watchlist + watch history + readlist diffed against
   * the chosen tracker (import, export, and conflicts) in one approval modal.
   * MangaBaka syncs the manga readlist (it is a manga-only tracker).
   */
  const handleOpenSync = useCallback(async (integration: Integration) => {
    setSyncModalIntegration(integration);
    setSyncModalMediaType(undefined);
    setSyncModalOpen(true);
    await integrationSync.loadUnifiedPreview(integration);
  }, [integrationSync]);

  /**
   * Called when the user confirms selected items in the SyncPreviewModal.
   */
  const handleApplySyncModal = useCallback(async (selectedItems: SyncProposalItem[]) => {
    const result = await integrationSync.applySync(syncModalIntegration, selectedItems);
    if (result.success) {
      toast.success(uiI18n.t('app.sync_applied_successfully_i18n_value_0_changei18_427a6c68', { value0: selectedItems.length, value1: selectedItems.length !== 1 ? 's' : '' }));
      setSyncModalOpen(false);
    } else {
      toast.error(uiI18n.t('app.sync_failed_i18n_value_0_633109a4', { value0: result.error }));
    }
  }, [integrationSync, syncModalIntegration]);

  return (
    <>
      <div className="space-y-6">
        {user ? (
          <>
            {/* MyAnimeList */}
            <div className="group relative overflow-hidden rounded-xl border border-white/10 bg-white/[0.02] p-5 sm:p-6">
              <div className="absolute top-0 right-0 p-8 -mr-8 -mt-8 bg-[#2E51A2]/10 rounded-full blur-3xl group-hover:bg-[#2E51A2]/20 transition-all duration-500" />

              <div className="relative flex flex-col md:flex-row md:items-center justify-between gap-6">
                <div className="flex items-center gap-5">
                  <div className="w-14 h-14 rounded-2xl bg-[#2E51A2] flex items-center justify-center text-white font-black text-xl shadow-lg shadow-[#2E51A2]/20">
                    {uiI18n.t('app.mal_308b46c0')}
                  </div>
                  <div>
                    <h3 className="text-lg font-bold flex items-center gap-2">
                      {uiI18n.t('app.myanimelist_be786904')}
                      {hasMAL && <SettingsBadge tone="success">{uiI18n.t('app.connected_c2f9b7b4')}</SettingsBadge>}
                    </h3>
                    <p className="text-sm text-muted-foreground mt-0.5">
                      {hasMAL
                        ? uiI18n.t('app.linked_to_mal_account_i18n_value_0_a45b9d07', { value0: profile?.mal_user_id || 'Active' })
                        : uiI18n.t('app.sync_your_anime_and_manga_lists_and_ratings_auto_01ecf4b9')}
                    </p>
                  </div>
                </div>

                <div className="flex flex-wrap items-center gap-3">
                  {hasMAL ? (
                    <Button
                      variant="outline"
                      size="sm"
                      onClick={handleMALDisconnect}
                      className="gap-2 border-destructive/20 text-destructive hover:bg-destructive/10 hover:border-destructive/40 transition-all w-full sm:w-auto"
                    >
                      <Trash2 className="w-4 h-4" />
                      {uiI18n.t('app.disconnect_ed28e068')}
                    </Button>
                  ) : (
                    <Button
                      size="default"
                      onClick={() => void handleMALConnect()}
                      disabled={malAvailability !== 'ready'}
                      className="gap-2 bg-[#2E51A2] hover:bg-[#2E51A2]/90 px-6 shadow-lg shadow-[#2E51A2]/20"
                    >
                      <ExternalLink className="w-4 h-4" />
                      {uiI18n.t('app.connect_myanimelist_dde6b79a')}
                    </Button>
                  )}
                </div>
              </div>

              {!hasMAL && availabilityNote(malAvailability) && (
                <p className="relative mt-4 text-xs text-amber-400">{availabilityNote(malAvailability)}</p>
              )}
              {hasMAL && (
                <div className="relative mt-5 pt-5 border-t border-white/5">
                  <div className="flex items-center gap-2 text-xs text-muted-foreground">
                    <CheckCircle className="w-3.5 h-3.5 text-success" />
                    {uiI18n.t('app.auto_sync_is_active_episode_chapter_updates_push_7def4c40')}
                  </div>
                </div>
              )}
            </div>

            {/* AniList */}
            <div className="group relative overflow-hidden rounded-xl border border-white/10 bg-white/[0.02] p-5 sm:p-6">
              <div className="absolute top-0 right-0 p-8 -mr-8 -mt-8 bg-[#02A9FF]/10 rounded-full blur-3xl group-hover:bg-[#02A9FF]/20 transition-all duration-500" />

              <div className="relative flex flex-col md:flex-row md:items-center justify-between gap-6">
                <div className="flex items-center gap-5">
                  <div className="w-14 h-14 rounded-2xl bg-[#02A9FF] flex items-center justify-center shadow-lg shadow-[#02A9FF]/20">
                    <svg viewBox="0 0 24 24" className="w-9 h-9 fill-white">
                      <path d="M6.361 2.943 0 21.056h4.942l1.077-3.133H11.4l1.052 3.133H22.9c.71 0 1.1-.392 1.1-1.101V17.53c0-.71-.39-1.101-1.1-1.101h-6.483V4.045c0-.71-.392-1.102-1.101-1.102h-2.422c-.71 0-1.101.392-1.101 1.102v1.064l-.758-2.166zm2.324 5.948 1.688 5.018H7.144z" />
                    </svg>
                  </div>
                  <div>
                    <h3 className="text-lg font-bold flex items-center gap-2">
                      {uiI18n.t('app.anilist_c03a6787')}
                      {hasAniList && <SettingsBadge tone="success">{uiI18n.t('app.connected_c2f9b7b4')}</SettingsBadge>}
                    </h3>
                    <p className="text-sm text-muted-foreground mt-0.5">
                      {hasAniList
                        ? uiI18n.t('app.linked_to_anilist_account_i18n_value_0_f22e4d10', { value0: profile?.anilist_user_id || 'Active' })
                        : uiI18n.t('app.sync_anime_watch_progress_and_manga_chapter_prog_7a0e838a')}
                    </p>
                  </div>
                </div>

                <div className="flex flex-wrap items-center gap-3">
                  {hasAniList ? (
                    <Button
                      variant="outline"
                      size="sm"
                      onClick={handleAniListDisconnect}
                      className="gap-2 border-destructive/20 text-destructive hover:bg-destructive/10 hover:border-destructive/40 transition-all w-full sm:w-auto"
                    >
                      <Trash2 className="w-4 h-4" />
                      {uiI18n.t('app.disconnect_ed28e068')}
                    </Button>
                  ) : (
                    <Button
                      size="default"
                      onClick={handleAniListConnect}
                      disabled={anilistAvailability !== 'ready'}
                      className="gap-2 bg-[#02A9FF] hover:bg-[#02A9FF]/90 px-6 shadow-lg shadow-[#02A9FF]/20"
                    >
                      <ExternalLink className="w-4 h-4" />
                      {uiI18n.t('app.connect_anilist_1f9f781d')}
                    </Button>
                  )}
                </div>
              </div>

              {!hasAniList && availabilityNote(anilistAvailability) && (
                <p className="relative mt-4 text-xs text-amber-400">{availabilityNote(anilistAvailability)}</p>
              )}
              {hasAniList && (
                <div className="relative mt-5 pt-5 border-t border-white/5">
                  <div className="flex items-center gap-2 text-xs text-muted-foreground">
                    <CheckCircle className="w-3.5 h-3.5 text-success" />
                    {uiI18n.t('app.auto_sync_is_active_chapter_and_episode_updates__99b47c3f')}
                  </div>
                </div>
              )}
            </div>

            {/* MangaBaka — disabled in this fork */}
            <div className="relative overflow-hidden rounded-xl border border-white/10 bg-white/[0.02] p-5 sm:p-6 opacity-70">
              <div className="relative flex items-center gap-5">
                <div className="w-14 h-14 rounded-2xl bg-[#1a1a1e] border border-white/10 flex items-center justify-center overflow-hidden">
                  {!mangabakaLogoFailed ? (
                    <img
                      src={MANGABAKA_LOGO_URL}
                      alt="MangaBaka"
                      className="w-10 h-10 object-contain grayscale"
                      loading="lazy"
                      onError={() => setMangabakaLogoFailed(true)}
                    />
                  ) : (
                    <span className="text-[#e30047] font-black text-lg">MB</span>
                  )}
                </div>
                <div>
                  <h3 className="text-lg font-bold flex items-center gap-2">
                    MangaBaka
                    <SettingsBadge tone="muted">Unavailable</SettingsBadge>
                  </h3>
                  <p className="text-sm text-muted-foreground mt-0.5">{MANGABAKA_UNAVAILABLE_MESSAGE}</p>
                </div>
              </div>
            </div>

            {/* Per-service full sync — one button per connected tracker.
                Watchlist + watch history + readlist are diffed against the
                tracker (import, export, conflicts) in one approval modal.
                MangaBaka covers the manga readlist (manga-only tracker). */}
            {(hasMAL || hasAniList) && (
              <SettingsSection
                icon={RefreshCw}
                eyebrow="Sync"
                title={uiI18n.t('app.sync_905f6309')}
                description={uiI18n.t('app.review_changes_per_service_before_applying_episo_4e0f03b5')}
              >
                <div className="flex flex-col sm:flex-row flex-wrap gap-2">
                  {hasMAL && (
                    <Button
                      onClick={() => void handleOpenSync('mal')}
                      disabled={integrationSync.isLoadingPreview}
                      className="gap-2 text-white shadow-md w-full sm:w-auto"
                      style={{ backgroundColor: '#2E51A2' }}
                    >
                      {integrationSync.isLoadingPreview ? (
                        <Loader2 className="w-4 h-4 animate-spin" />
                      ) : (
                        <RefreshCw className="w-4 h-4" />
                      )}
                      {uiI18n.t('app.sync_with_myanimelist_0bea8439')}
                    </Button>
                  )}
                  {hasAniList && (
                    <Button
                      onClick={() => void handleOpenSync('anilist')}
                      disabled={integrationSync.isLoadingPreview}
                      className="gap-2 text-white shadow-md w-full sm:w-auto"
                      style={{ backgroundColor: '#02A9FF' }}
                    >
                      {integrationSync.isLoadingPreview ? (
                        <Loader2 className="w-4 h-4 animate-spin" />
                      ) : (
                        <RefreshCw className="w-4 h-4" />
                      )}
                      {uiI18n.t('app.sync_with_anilist_912fbedb')}
                    </Button>
                  )}
                </div>
              </SettingsSection>
            )}

            <SettingsSection
              title={uiI18n.t('app.what_syncs_2a52ac56')}
              description={uiI18n.t('app.watchlist_watch_history_and_manga_readlist_with__6b6bb7f2')}
            />
          </>
        ) : (
          <SettingsEmptyState
            icon={Link2}
            title={uiI18n.t('app.sign_in_to_connect_external_services_7c2fd1d5')}
            description={uiI18n.t('app.link_myanimelist_anilist_or_mangabaka_to_sync_yo_75596d3d')}
          />
        )}
      </div>

      {/* Smart Sync approval modal — always shown with manual confirmation required */}
      <SyncPreviewModal
        isOpen={syncModalOpen}
        onClose={() => setSyncModalOpen(false)}
        onApply={handleApplySyncModal}
        items={integrationSync.previewItems}
        integration={syncModalIntegration}
        mediaType={syncModalMediaType}
        isLoading={integrationSync.isLoadingPreview}
        isApplying={integrationSync.isApplying}
      />
    </>
  );
}
