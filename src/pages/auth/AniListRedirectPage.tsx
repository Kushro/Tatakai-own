import { useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useLocation, useNavigate } from 'react-router-dom';
import { useSettingsModal } from '@/contexts/SettingsModalContext';
import { confirmAniListLogin, parseAniListCallback, prepareAniListLogin, type PendingAniListLogin } from '@/lib/anilist/auth';
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
    const [status, setStatus] = useState<'loading' | 'confirm' | 'success' | 'error'>('loading');
    const [pendingLogin, setPendingLogin] = useState<PendingAniListLogin | null>(null);
    const [isLinking, setIsLinking] = useState(false);
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

        // Verify the token and look up the account, but store nothing until the
        // user confirms it is the account they meant to link.
        const verify = async () => {
            try {
                const login = await prepareAniListLogin(fragment);
                setPendingLogin(login);
                setStatus('confirm');
            } catch (err: any) {
                console.error('AniList login error:', err);
                setStatus('error');
                setError(err.message || t('auth.exchangeCodeFailed'));
                toast.error(t('auth.anilistLinkFailed'));
            }
        };

        void verify();
    }, [location.hash, location.search, location.pathname, navigate, user, authLoading, refreshProfile, openSettings, t]);

    const handleConfirm = async () => {
        if (!pendingLogin || isLinking) return;
        setIsLinking(true);
        try {
            await confirmAniListLogin(pendingLogin);
            await refreshProfile();
            setPendingLogin(null);
            setStatus('success');
            toast.success(t('auth.anilistLinked'));
            setTimeout(() => { navigate('/', { replace: true }); openSettings('integrations'); }, 2000);
        } catch (err: any) {
            setStatus('error');
            setError(err.message || t('auth.exchangeCodeFailed'));
            toast.error(t('auth.anilistLinkFailed'));
        } finally {
            setIsLinking(false);
        }
    };

    const handleCancel = () => {
        setPendingLogin(null);
        navigate('/', { replace: true });
        openSettings('integrations');
    };

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

                {status === 'confirm' && pendingLogin && (
                    <>
                        <AlertCircle className="w-12 h-12 text-[#02A9FF] mx-auto" />
                        <h1 className="text-2xl font-bold">Link AniList account {pendingLogin.username}?</h1>
                        <p className="text-muted-foreground">
                            Only continue if you just started this from Settings and this is your AniList account.
                        </p>
                        <div className="mt-4 flex flex-col sm:flex-row items-center justify-center gap-3">
                            <button
                                onClick={() => void handleConfirm()}
                                disabled={isLinking}
                                className="px-6 py-2 bg-primary text-primary-foreground rounded-lg hover:bg-primary/90 transition-colors disabled:opacity-50"
                            >
                                {isLinking ? <Loader2 className="w-4 h-4 animate-spin" /> : 'Link account'}
                            </button>
                            <button
                                onClick={handleCancel}
                                disabled={isLinking}
                                className="px-6 py-2 rounded-lg border border-white/10 bg-white/5 text-muted-foreground hover:text-foreground hover:bg-white/10 transition-colors"
                            >
                                Cancel
                            </button>
                        </div>
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
