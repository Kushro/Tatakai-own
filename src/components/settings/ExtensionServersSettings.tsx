import uiI18n from '@/lib/uiI18n';
import { useUiTranslation } from '@/hooks/ui/useUiTranslation';
import { useState } from 'react';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { SettingsSection } from './SettingsPrimitives';
import {
  connectServer,
  connectServerLink,
  listServers,
  parsePairingInput,
  removeServer,
  saveServers,
  serverRequest,
  setServerEnabled,
  toServerError,
} from '@/core/extensions/server/client';

function InfoList({ title, keys }: { title: string; keys: string[] }) {
  return <div className="flex flex-col gap-1">
    <h4 className="text-sm font-semibold text-foreground">{title}</h4>
    <ul className="list-disc space-y-1 pl-5 text-sm text-muted-foreground">{keys.map(key => <li key={key}>{uiI18n.t(key)}</li>)}</ul>
  </div>;
}

/** D8: what linking this device to the Windows/Linux PC gives and costs (collapsible). */
export function ExtensionServerBenefits({ defaultOpen = false }: { defaultOpen?: boolean }) {
  useUiTranslation();
  const k = 'extensionServerClient.';
  return <details className="rounded-lg border border-border p-4" open={defaultOpen}>
    <summary className="cursor-pointer text-sm font-semibold">{uiI18n.t(k + 'benefitsTitle')}</summary>
    <div className="mt-3 flex flex-col gap-4">
      <InfoList title={uiI18n.t(k + 'prosTitle')} keys={['otherEcosystems', 'installOnce', 'pcScrapes', 'sharedCache', 'battery'].map(key => `${k}pros.${key}`)} />
      <InfoList title={uiI18n.t(k + 'consTitle')} keys={['pcOn', 'tunnel', 'videoProxy', 'lanPort', 'latency'].map(key => `${k}cons.${key}`)} />
      <InfoList title={uiI18n.t(k + 'extrasTitle')} keys={['qr', 'revocable', 'fallback', 'viewerPassword'].map(key => `${k}extras.${key}`)} />
      <InfoList title={uiI18n.t(k + 'comingSoonTitle')} keys={[`${k}comingSoon.desktopNext`]} />
    </div>
  </details>;
}

export function ExtensionServersSettings({ initialUrl = '', pairingCode = '', initialLink = '' }: { initialUrl?: string; pairingCode?: string; initialLink?: string }) {
  useUiTranslation();
  const [servers, setServers] = useState(listServers), [url, setUrl] = useState(initialUrl), [password, setPassword] = useState('');
  const [link, setLink] = useState(initialLink), [code, setCode] = useState(pairingCode);
  const [busy, setBusy] = useState(false), [message, setMessage] = useState(''), [failed, setFailed] = useState(false);
  async function run(fn: () => Promise<string | void>) {
    setBusy(true); setMessage(''); setFailed(false);
    try { const done = await fn(); if (done) setMessage(done); setServers(listServers()); }
    catch (error) { setFailed(true); setMessage(toServerError(error).message); }
    finally { setBusy(false); }
  }
  const pasteLink = () => void run(async () => {
    const parsed = parsePairingInput(link, url);
    if (!parsed.code && parsed.password === undefined) {
      // A bare address: move it to the manual form and ask for the password.
      setUrl(parsed.server); setLink('');
      return uiI18n.t('extensionServerClient.reconnectHint');
    }
    const server = await connectServerLink(parsed, password);
    setLink(''); setPassword(''); setCode('');
    return uiI18n.t('extensionServerClient.connected', { name: server.name });
  });
  const connectManual = () => void run(async () => {
    const server = await connectServer(url, password, code || undefined);
    setPassword(''); setCode('');
    return uiI18n.t('extensionServerClient.connected', { name: server.name });
  });
  return <SettingsSection title={uiI18n.t('extensionServerClient.title')} description={uiI18n.t('extensionServerClient.description')}>
    <div className="flex flex-col gap-4">
      <ExtensionServerBenefits />
      <form className="flex flex-col gap-2" onSubmit={event => { event.preventDefault(); pasteLink(); }}>
        <Label htmlFor="extension-server-link">{uiI18n.t('extensionServerClient.pasteLink')}</Label>
        <div className="flex flex-wrap gap-2">
          <Input id="extension-server-link" className="min-w-0 flex-1" value={link} autoComplete="off" spellCheck={false} onChange={event => setLink(event.target.value)}
            placeholder={uiI18n.t('extensionServerClient.pasteLinkPlaceholder')} required />
          <Button disabled={busy || !link.trim()}>{busy ? uiI18n.t('extensionServerClient.connecting') : uiI18n.t('extensionServerClient.pair')}</Button>
        </div>
        <p className="text-xs text-muted-foreground">{uiI18n.t('extensionServerClient.pasteLinkHint')}</p>
      </form>
      <form className="flex flex-col gap-3" onSubmit={event => { event.preventDefault(); connectManual(); }}>
        <h3 className="text-sm font-semibold">{uiI18n.t('extensionServerClient.manualTitle')}</h3>
        <div className="flex flex-col gap-2"><Label htmlFor="extension-server-url">{uiI18n.t('extensionServerClient.serverUrl')}</Label><Input id="extension-server-url" type="text" inputMode="url" autoCapitalize="off" spellCheck={false} value={url} onChange={event => setUrl(event.target.value)} placeholder={uiI18n.t('extensionServerClient.serverUrlPlaceholder')} required /></div>
        {!code && <div className="flex flex-col gap-2"><Label htmlFor="extension-server-password">{uiI18n.t('extensionServerClient.password')}</Label><Input id="extension-server-password" type="password" autoComplete="current-password" value={password} onChange={event => setPassword(event.target.value)} /></div>}
        <p className="text-xs text-muted-foreground">{uiI18n.t('extensionServerClient.lanOnlyHttp')} {uiI18n.t('extensionServerClient.awayFromHome')}</p>
        <Button className="self-start" disabled={busy}>{busy ? uiI18n.t('extensionServerClient.connecting') : code ? uiI18n.t('extensionServerClient.pair') : uiI18n.t('extensionServerClient.connect')}</Button>
      </form>
      {message && <p role={failed ? 'alert' : 'status'} className={failed ? 'text-sm text-destructive' : 'text-sm'}>{message}</p>}
      <h3 className="text-sm font-semibold">{uiI18n.t('extensionServerClient.serversTitle')}</h3>
      {!servers.length && <p className="text-sm text-muted-foreground">{uiI18n.t('extensionServerClient.noServers')}</p>}
      {servers.map(server => <div key={server.id} className="flex flex-col gap-3 rounded-lg border p-4">
        <div className="flex flex-col gap-2"><Label htmlFor={`server-name-${server.id}`}>{uiI18n.t('extensionServerClient.serverName')}</Label><Input id={`server-name-${server.id}`} value={server.name} onChange={event => { const rows = listServers().map(row => row.id === server.id ? { ...row, name: event.target.value } : row); saveServers(rows); setServers(rows); }} /></div>
        <p className="break-all text-sm text-muted-foreground">{server.url}</p>
        <div className="flex flex-wrap gap-2">
          <Button variant="outline" size="sm" disabled={busy} onClick={() => setServers(setServerEnabled(server.id, !server.enabled))}>{server.enabled ? uiI18n.t('extensionServerClient.disable') : uiI18n.t('extensionServerClient.enable')}</Button>
          <Button variant="outline" size="sm" disabled={busy} onClick={() => void run(async () => { await serverRequest(server, '/sources'); return uiI18n.t('extensionServerClient.testOk', { name: server.name }); })}>{uiI18n.t('extensionServerClient.test')}</Button>
          <Button variant="outline" size="sm" onClick={() => { setUrl(server.url); setPassword(''); setCode(''); setFailed(false); setMessage(uiI18n.t('extensionServerClient.reconnectHint')); }}>{uiI18n.t('extensionServerClient.reconnect')}</Button>
          <Button variant="outline" size="sm" disabled={busy} onClick={() => { if (!window.confirm(uiI18n.t('extensionServerClient.removeConfirm', { name: server.name }))) return; void run(async () => { await removeServer(server.id); return uiI18n.t('extensionServerClient.removed'); }); }}>{uiI18n.t('extensionServerClient.remove')}</Button>
        </div>
      </div>)}
    </div>
  </SettingsSection>;
}
