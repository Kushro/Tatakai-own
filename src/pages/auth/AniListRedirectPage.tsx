import { useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useLocation, useNavigate } from 'react-router-dom';
import { useSettingsModal } from '@/contexts/SettingsModalContext';
import { exchangeAniListCode } from '@/lib/externalIntegrations';
import { parseAniListCallback } from '@/lib/anilist/auth';
import { useAuth } from '@/contexts/AuthContext';
import { toast } from 'sonner';
import { Loader2, CheckCircle2, AlertCircle } from 'lucide-react';
import { GlassPanel } from '@/components/ui/GlassPanel';

/**
 * AniList implicit-grant callback. The app is reached through the
 * `tatakai://integration/anilist/redirect#access_token=…` deep link, so the token
 * is in the URL FRAGMENT (`location.hash`), never in the query string.
 */
export default function AniListRedirectPage() {
    const { t } = useTranslation();
    const location = useLocation();
    const navigate = useNavigate();
    const { openSettings } = useSettingsModal();
    const { user, isLoading: authLoading, refreshProfile } = useAuth();
    const [status, setStatus] = useState<'loading' | 'success' | 'error'>('loading');
    const [error, setError] = useState<string | null>(null);

    const hasRun = useRef(false);

    useEffect(() => {
        // Wait until auth context has finished resolving before doing anything
        if (authLoading || hasRun.current) return;

        const fragment = location.hash || location.search;
        const callback = parseAniListCallback(fragment);

        if (callback?.kind === 'error') {
            hasRun.current = true;
            const message = callback.description || callback.error;
            setStatus('error');
            setError(message);
            toast.error(t('auth.anilistError', { error: message }));
            return;
        }

        if (!callback) {
            hasRun.current = true;
            setStatus('error');
            setError(t('auth.noAuthCode'));
            return;
        }

        if (!user) {
            hasRun.current = true;
            setStatus('error');
            setError(t('auth.mustLoginToLinkAnilist'));
            return;
        }

        hasRun.current = true;
        // Drop the token from the address bar / history right away.
        try { window.history.replaceState(window.history.state, '', location.pathname); } catch { /* ignore */ }

        const completeAuth = async () => {
            try {
                await exchangeAniListCode(fragment);
                await refreshProfile();
                setStatus('success');
                toast.success(t('auth.anilistLinked'));
                setTimeout(() => { navigate('/', { replace: true }); openSettings('integrations'); }, 2000);
            } catch (err: any) {
                console.error('AniList login error:', err);
                setStatus('error');
                setError(err.message || t('auth.exchangeCodeFailed'));
                toast.error(t('auth.anilistLinkFailed'));
            }
        };

        void completeAuth();
    }, [location.hash, location.search, location.pathname, navigate, user, authLoading, refreshProfile, openSettings, t]);

    return (
        <div className="min-h-screen bg-background flex items-center justify-center p-4">
            <GlassPanel className="max-w-md w-full p-8 text-center space-y-6">
                {status === 'loading' && (
                    <>
                        <Loader2 className="w-12 h-12 text-[#02A9FF] animate-spin mx-auto" />
                        <h1 className="text-2xl font-bold">{t('auth.linkingAnilist')}</h1>
                        <p className="text-muted-foreground">{t('auth.linkingWait')}</p>
                    </>
                )}

                {status === 'success' && (
                    <>
                        <CheckCircle2 className="w-12 h-12 text-green-500 mx-auto" />
                        <h1 className="text-2xl font-bold">{t('auth.successTitle')}</h1>
                        <p className="text-muted-foreground">{t('auth.anilistLinkedRedirect')}</p>
                    </>
                )}

                {status === 'error' && (
                    <>
                        <AlertCircle className="w-12 h-12 text-destructive mx-auto" />
                        <h1 className="text-2xl font-bold">{t('auth.authFailed')}</h1>
                        <p className="text-destructive font-medium">{error}</p>
                        <button
                            onClick={() => { navigate('/', { replace: true }); openSettings('integrations'); }}
                            className="mt-4 px-6 py-2 bg-primary text-primary-foreground rounded-lg hover:bg-primary/90 transition-colors"
                        >
                            {t('auth.backToSettings')}
                        </button>
                    </>
                )}
            </GlassPanel>
        </div>
    );
}
