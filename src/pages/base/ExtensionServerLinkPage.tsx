import uiI18n from '@/lib/uiI18n';
import { useUiTranslation } from '@/hooks/ui/useUiTranslation';
import { useEffect, useState } from 'react';
import { useLocation, useNavigate } from 'react-router-dom';
import { Dialog, DialogContent, DialogHeader, DialogTitle, DialogDescription, DialogFooter } from '@/components/ui/dialog';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { ExtensionServerBenefits } from '@/components/settings/ExtensionServersSettings';
import { connectServer, parseServerLink, toServerError } from '@/core/extensions/server/client';

type CapturedLink = { pathname: string; search: string };

/**
 * Opens extension-server links (contract §3.7): the pairing deep link
 * `tatakai://extensionserver/connect?server=…&code=…` (routed here as
 * `/extensionserver/connect?…` by the Capacitor / Electron deep-link bridges),
 * `/connect-server?server=…&code=…`, and the old `/extensionserver/<url>[?pass=…]`.
 * The head script of index.html moves credentials out of the address bar before
 * anything else loads; this page reads them from `window.__tatakaiServerLink`.
 */
export default function ExtensionServerLinkPage() {
  useUiTranslation();
  const location = useLocation(), navigate = useNavigate();
  const [input] = useState(() => {
    const captured: CapturedLink = (window as Window & { __tatakaiServerLink?: CapturedLink }).__tatakaiServerLink || location;
    if (/^\/extensionserver\/confirm\/?$/.test(captured.pathname)) return { url: '', password: '', code: '', error: uiI18n.t('extensionServerClient.linkRefreshed') };
    try {
      const link = parseServerLink(captured.pathname + captured.search);
      return { url: link.server, password: link.password || '', code: link.code || '', error: '' };
    } catch (error) {
      return { url: '', password: '', code: '', error: toServerError(error).message };
    }
  });
  const [url, setUrl] = useState(input.url), [password, setPassword] = useState(input.password), [error, setError] = useState(input.error), [busy, setBusy] = useState(false);
  const [code, setCode] = useState(input.code);
  useEffect(() => { delete (window as Window & { __tatakaiServerLink?: unknown }).__tatakaiServerLink; window.history.replaceState(null, '', '/extensionserver/confirm'); }, []);
  async function connect(event: React.FormEvent) {
    event.preventDefault(); setBusy(true); setError('');
    try { await connectServer(url, password, code || undefined); setPassword(''); navigate('/settings?tab=extensions', { replace: true }); }
    catch (failure) {
      const reason = toServerError(failure);
      // A used/expired code cannot be retried: fall back to the address + password form.
      if (reason.code === 'pairing_expired') setCode('');
      setError(reason.message);
    }
    finally { setBusy(false); }
  }
  let host = url;
  try { host = new URL(url).host; } catch { /* Shown as typed. */ }
  const close = () => navigate('/', { replace: true });
  return <Dialog open onOpenChange={open => { if (!open && !busy) close(); }}><DialogContent className="max-h-[90vh] overflow-y-auto">
    <DialogHeader>
      <DialogTitle>{code && host ? uiI18n.t('extensionServerClient.pairWith', { host }) : uiI18n.t('extensionServerClient.title')}</DialogTitle>
      <DialogDescription>{uiI18n.t('extensionServerClient.trustWarning')}</DialogDescription>
    </DialogHeader>
    <form className="flex flex-col gap-4" onSubmit={connect}>
      {code ? <p className="break-all text-sm text-muted-foreground">{url}</p> : <>
        <div className="flex flex-col gap-2"><Label htmlFor="link-server-url">{uiI18n.t('extensionServerClient.serverUrl')}</Label><Input id="link-server-url" type="text" inputMode="url" autoCapitalize="off" spellCheck={false} required value={url} onChange={event => setUrl(event.target.value)} placeholder={uiI18n.t('extensionServerClient.serverUrlPlaceholder')} /></div>
        <div className="flex flex-col gap-2"><Label htmlFor="link-server-password">{uiI18n.t('extensionServerClient.password')}</Label><Input id="link-server-password" type="password" autoComplete="off" value={password} onChange={event => setPassword(event.target.value)} /></div>
        <p className="text-xs text-muted-foreground">{uiI18n.t('extensionServerClient.lanOnlyHttp')}</p>
      </>}
      <ExtensionServerBenefits />
      {error && <p role="alert" className="text-sm text-destructive">{error}</p>}
      <DialogFooter>
        <Button type="button" variant="outline" disabled={busy} onClick={close}>{uiI18n.t('app.cancel_77dfd213')}</Button>
        <Button disabled={busy || !url}>{busy ? uiI18n.t('extensionServerClient.connecting') : code ? uiI18n.t('extensionServerClient.pair') : uiI18n.t('extensionServerClient.connect')}</Button>
      </DialogFooter>
    </form>
  </DialogContent></Dialog>;
}
