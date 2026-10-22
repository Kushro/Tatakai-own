import { useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useNavigate, useSearchParams } from 'react-router-dom';
import { exchangeMalCode, clearMalPendingMarker, clearMalOAuthStorage, getMalAuthUrl } from '@/lib/mal';
import { isNativeOAuthApp, openOAuthInBrowser } from '@/lib/desktopOAuth';
import { readMalRedirectUri } from '@/lib/mal/auth';
import { useAuth } from '@/contexts/AuthContext';
import { toast } from 'sonner';
import { Loader2, CheckCircle2, AlertCircle } from 'lucide-react';
import { GlassPanel } from '@/components/ui/GlassPanel';

/**
 * MAL PKCE callback. Reached through the `tatakai://integration/mal/redirect?code=…&state=…`
 * deep link; the code is exchanged client side with the verifier stored when the
 * login started (no backend, no client secret).
 */
export default function MalRedirectPage() {
    const { t } = useTranslation();
    const [searchParams] = useSearchParams();
    const navigate = useNavigate();
    const { refreshProfile, isLoading: authLoading, user } = useAuth();
    const [status, setStatus] = useState<'loading' | 'success' | 'error'>('loading');
    const [error, setError] = useState<string | null>(null);
    const [isRetrying, setIsRetrying] = useState(false);

    const hasRun = useRef(false);

    const handleReconnect = async () => {
        if (isRetrying) return;
        if (!user) {
            navigate('/profile');
            return;
        }
        setIsRetrying(true);
        try {
            // The old flow is dead (codes are single use), so mint a fresh verifier.
            clearMalOAuthStorage();
            const url = getMalAuthUrl();
            if (isNativeOAuthApp() && (await openOAuthInBrowser(url))) {
                toast.info('Complete the MyAnimeList login in your browser.');
                return;
            }
            window.location.href = url;
        } catch (e) {
            toast.error(e instanceof Error ? e.message : 'Could not restart MyAnimeList login.');
        } finally {
            setIsRetrying(false);
        }
    };

    useEffect(() => {
        if (hasRun.current || authLoading) return;
        hasRun.current = true;

        const code = searchParams.get('code');
        const state = searchParams.get('state');
        const errorParam = searchParams.get('error');

        if (errorParam) {
            const sent = readMalRedirectUri();
            const hint = /invalid_client|redirect/i.test(errorParam)
                ? `${errorParam} - the MAL app must list the redirect URL ${sent || 'tatakai://integration/mal/redirect'} exactly.`
                : errorParam;
            setStatus('error');
            setError(hint);
            toast.error(t('auth.malError', { error: errorParam }));
            return;
        }

        if (!code) {
            setStatus('error');
            setError(t('auth.noAuthCode'));
            return;
        }

        const completeAuth = async () => {
            try {
                if (!user) throw new Error('Sign in to Tatakai before linking MyAnimeList.');
                // Authorization codes are single-use: a reload or double-run would burn
                // the code twice and the retry dies with `invalid_grant`.
                const usedKey = `mal_exchange_used:${code}`;
                try {
                    if (sessionStorage.getItem(usedKey) === '1') {
                        throw new Error('This MyAnimeList login link was already used. Start the login again.');
                    }
                    sessionStorage.setItem(usedKey, '1');
                } catch (e) {
                    if (e instanceof Error && e.message.startsWith('This MyAnimeList login link')) throw e;
                    /* storage unavailable: proceed without the guard */
                }
                // Drop the code from the address bar / history.
                try { window.history.replaceState(window.history.state, '', window.location.pathname); } catch { /* ignore */ }
                await exchangeMalCode(code, state);
                await refreshProfile();
                setStatus('success');
                toast.success(t('auth.malLinked'));
                setTimeout(() => navigate('/profile'), 2000);
            } catch (err: any) {
                console.error('MAL Exchange Error:', err);
                setStatus('error');
                clearMalPendingMarker();
                const raw = err.message || t('auth.exchangeCodeFailed');
                const sent = readMalRedirectUri();
                setError(/invalid_client|redirect_uri|redirect/i.test(raw)
                    ? `${raw} - the MAL app's redirect URL must be exactly ${sent || 'tatakai://integration/mal/redirect'}.`
                    : raw);
                toast.error(t('auth.malLinkFailed'));
            }
        };

        void completeAuth();
    }, [searchParams, navigate, refreshProfile, authLoading, user, t]);

    return (
        <div className="min-h-screen bg-background flex items-center justify-center p-4">
            <GlassPanel className="max-w-md w-full p-8 text-center space-y-6">
                {status === 'loading' && (
                    <>
                        <Loader2 className="w-12 h-12 text-primary animate-spin mx-auto" />
                        <h1 className="text-2xl font-bold">{t('auth.linkingMal')}</h1>
                        <p className="text-muted-foreground">{t('auth.linkingWait')}</p>
                    </>
                )}

                {status === 'success' && (
                    <>
                        <CheckCircle2 className="w-12 h-12 text-green-500 mx-auto" />
                        <h1 className="text-2xl font-bold">{t('auth.successTitle')}</h1>
                        <p className="text-muted-foreground">{t('auth.malLinkedRedirect')}</p>
                    </>
                )}

                {status === 'error' && (
                    <>
                        <AlertCircle className="w-12 h-12 text-destructive mx-auto" />
                        <h1 className="text-2xl font-bold">{t('auth.authFailed')}</h1>
                        <p className="text-destructive font-medium">{error}</p>
                        <div className="mt-4 flex flex-col sm:flex-row items-center justify-center gap-3">
                            <button
                                onClick={() => void handleReconnect()}
                                disabled={isRetrying}
                                className="px-6 py-2 bg-primary text-primary-foreground rounded-lg hover:bg-primary/90 transition-colors disabled:opacity-50 disabled:cursor-wait"
                            >
                                {isRetrying ? t('migration.restarting') : t('migration.reconnectMal')}
                            </button>
                            <button
                                onClick={() => navigate('/profile')}
                                className="px-6 py-2 rounded-lg border border-white/10 bg-white/5 text-muted-foreground hover:text-foreground hover:bg-white/10 transition-colors"
                            >
                                {t('auth.backToProfile')}
                            </button>
                        </div>
                    </>
                )}
            </GlassPanel>
        </div>
    );
}
