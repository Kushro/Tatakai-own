import uiI18n from '@/lib/uiI18n';
import { useUiTranslation } from '@/hooks/ui/useUiTranslation';
import { useEffect, useState, useCallback } from 'react';
import { QRCodeSVG } from 'qrcode.react';
import { Play, Square, Copy, Users, Shield, Share2, Check, X, ExternalLink, Smartphone, RefreshCw, Puzzle } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Switch } from '@/components/ui/switch';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import {
  SettingRow,
  SettingsBadge,
  SettingsSection,
} from '@/components/settings/SettingsPrimitives';
import { toast } from 'sonner';
import { useIsDesktopApp } from '@/hooks/ui/useIsNativeApp';

type HomeStatus = {
  running?: boolean;
  port?: number;
  bindMode?: string;
  urls?: string[];
  lanAddresses?: string[];
  passwordConfigured?: boolean;
  remoteEnabled?: boolean;
  remoteBaseUrl?: string | null;
  shareCatalog?: boolean;
  requireConsent?: boolean;
  clients?: Array<{ userId?: string; ip?: string; lastSeenAt?: number }>;
};

type ExtensionServerInfo = { name: string; sourceCount: number; ecosystems: string[] } | null;

type ExtensionPairing = { deepLink: string; code: string; expiresIn: number; origin: string };

const ECOSYSTEM_LABELS: Record<string, string> = {
  aniyomi: 'Aniyomi',
  mihon: 'Mihon',
  cloudstream: 'CloudStream',
  tatakai: 'Tatakai (.kai)',
  stremio: 'Stremio',
  seanime: 'Seanime',
  sora: 'Sora',
  mangayomi: 'Mangayomi',
  kodi: 'Kodi',
};

/** IPC rejects with "Error invoking remote method '…': Error: <code>"; keep only the code. */
function ipcErrorCode(err: unknown): string {
  const message = err instanceof Error ? err.message : String(err ?? '');
  const match = message.match(/Error:\s*([^\s:]+)\s*$/);
  return (match?.[1] || message || 'unknown_error').trim();
}

function describeExtensionError(code: string): string {
  switch (code) {
    case 'home_server_stopped':
      return uiI18n.t('extensionServerHost.statusStopped');
    case 'extension_server_unavailable':
      return uiI18n.t('extensionServerHost.statusUnavailable');
    case 'no_reachable_address':
      return uiI18n.t('extensionServerHost.sameNetworkHint');
    default:
      return code;
  }
}

function formatCountdown(ms: number): string {
  const total = Math.max(0, Math.ceil(ms / 1000));
  const minutes = Math.floor(total / 60);
  const seconds = total % 60;
  return `${minutes}:${String(seconds).padStart(2, '0')}`;
}

type ConsentRequest = {
  id: string;
  userId: string;
  label?: string;
  status: 'pending' | 'approved' | 'denied';
  createdAt: number;
};

/** Resolve Home Server API from electron or tatakaiRuntime (handles stale preload). */
function getHomeServerApi() {
  const w = typeof window !== 'undefined' ? (window as any) : null;
  if (!w) return null;
  const electron = w.electron;
  const runtime = w.tatakaiRuntime;
  const invoke = electron?.invoke || runtime?.invoke;

  const call = (camel: string, channel: string, nested?: string) => {
    return async (...args: unknown[]) => {
      if (electron?.[camel] && typeof electron[camel] === 'function') {
        return electron[camel](...args);
      }
      if (nested && electron?.homeServer?.[nested] && typeof electron.homeServer[nested] === 'function') {
        return electron.homeServer[nested](...args);
      }
      if (runtime?.[camel] && typeof runtime[camel] === 'function') {
        return runtime[camel](...args);
      }
      if (nested && runtime?.homeServer?.[nested] && typeof runtime.homeServer[nested] === 'function') {
        return runtime.homeServer[nested](...args);
      }
      if (typeof invoke === 'function') {
        return invoke(channel, ...args);
      }
      throw new Error(
        uiI18n.t('app.i18n_value_0_is_unavailable_restart_the_tatakai__0eb93bc4', { value0: camel }),
      );
    };
  };

  return {
    getStatus: call('homeServerGetStatus', 'home-server:get-status', 'getStatus'),
    getConfig: call('homeServerGetConfig', 'home-server:get-config', 'getConfig'),
    setConfig: call('homeServerSetConfig', 'home-server:set-config', 'setConfig'),
    start: call('homeServerStart', 'home-server:start', 'start'),
    stop: call('homeServerStop', 'home-server:stop', 'stop'),
    createToken: call('homeServerCreateToken', 'home-server:create-token', 'createToken'),
    upsertUser: call('homeServerUpsertUser', 'home-server:upsert-user', 'upsertUser'),
    listConsent: call('homeServerListConsent', 'home-server:list-consent', 'listConsent'),
    decideConsent: call('homeServerDecideConsent', 'home-server:decide-consent', 'decideConsent'),
    extensionPairing: call('extensionServerPairing', 'extension-server:pairing', 'extensionPairing'),
    extensionInfo: call('extensionServerInfo', 'extension-server:info', 'extensionInfo'),
    openExtensionDashboard: call('extensionServerOpenDashboard', 'extension-server:open-dashboard', 'openExtensionDashboard'),
    onStatus: electron?.onHomeServerStatus || runtime?.onHomeServerStatus,
  };
}

function ExtensionServerD8Info() {
  const list = (prefix: string, keys: string[]) => (
    <ul className="list-disc space-y-1 pl-5 text-xs text-muted-foreground">
      {keys.map((key) => (
        <li key={key}>{uiI18n.t(`extensionServerHost.${prefix}.${key}`)}</li>
      ))}
    </ul>
  );
  return (
    <div className="space-y-3 rounded-lg border border-white/10 bg-black/20 p-3">
      <div className="text-sm font-semibold">{uiI18n.t('extensionServerHost.benefitsTitle')}</div>
      <div className="grid gap-3 md:grid-cols-2">
        <div className="space-y-1">
          <div className="text-xs font-semibold text-success">{uiI18n.t('extensionServerHost.prosTitle')}</div>
          {list('pros', ['otherEcosystems', 'installOnce', 'pcScrapes', 'sharedCache', 'battery'])}
        </div>
        <div className="space-y-1">
          <div className="text-xs font-semibold text-amber">{uiI18n.t('extensionServerHost.consTitle')}</div>
          {list('cons', ['pcOn', 'tunnel', 'videoProxy', 'lanPort', 'latency'])}
        </div>
      </div>
      <div className="space-y-1">
        <div className="text-xs font-semibold">{uiI18n.t('extensionServerHost.extrasTitle')}</div>
        {list('extras', ['qr', 'revocable', 'fallback', 'viewerPassword'])}
      </div>
      <div className="space-y-1">
        <div className="text-xs font-semibold">{uiI18n.t('extensionServerHost.comingSoonTitle')}</div>
        {list('comingSoon', ['desktopNext'])}
      </div>
    </div>
  );
}

function ExtensionServerPanel({ status, bootstrap }: { status: HomeStatus; bootstrap: string }) {
  const [info, setInfo] = useState<ExtensionServerInfo>(null);
  const [infoLoaded, setInfoLoaded] = useState(false);
  const [pairing, setPairing] = useState<ExtensionPairing | null>(null);
  const [expiresAt, setExpiresAt] = useState(0);
  const [now, setNow] = useState(() => Date.now());
  const [pairingBusy, setPairingBusy] = useState(false);
  const [pairingError, setPairingError] = useState('');
  const running = Boolean(status.running);
  const remoteOrigin = (() => {
    try {
      // Saved config (status), not the unsaved form fields: it is what pairing uses.
      const url = new URL(status.remoteBaseUrl || '');
      return status.remoteEnabled && url.protocol === 'https:' ? url.origin : '';
    } catch {
      return '';
    }
  })();

  const loadInfo = useCallback(async () => {
    try {
      const api = getHomeServerApi();
      const next = (await api?.extensionInfo()) as ExtensionServerInfo;
      setInfo(next && typeof next === 'object' ? next : null);
    } catch {
      setInfo(null);
    } finally {
      setInfoLoaded(true);
    }
  }, []);

  useEffect(() => {
    void loadInfo();
    const timer = window.setInterval(() => void loadInfo(), 15000);
    return () => window.clearInterval(timer);
  }, [loadInfo, running]);

  useEffect(() => {
    if (!pairing) return;
    const timer = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(timer);
  }, [pairing]);

  useEffect(() => {
    if (!running) setPairing(null);
  }, [running]);

  const generatePairing = async (prefer: 'lan' | 'remote' = 'lan') => {
    setPairingBusy(true);
    setPairingError('');
    try {
      const api = getHomeServerApi();
      if (!api) throw new Error('extension_server_unavailable');
      const next = (await api.extensionPairing({ prefer })) as ExtensionPairing;
      if (!next?.deepLink) throw new Error('extension_server_unavailable');
      setPairing(next);
      setExpiresAt(Date.now() + (Number(next.expiresIn) || 300) * 1000);
      setNow(Date.now());
    } catch (err) {
      const message = uiI18n.t('extensionServerHost.pairingFailed', { error: describeExtensionError(ipcErrorCode(err)) });
      setPairing(null);
      setPairingError(message);
      toast.error(message);
    } finally {
      setPairingBusy(false);
    }
  };

  const openDashboard = async () => {
    try {
      const api = getHomeServerApi();
      if (!api) throw new Error('extension_server_unavailable');
      await api.openExtensionDashboard();
    } catch (err) {
      toast.error(describeExtensionError(ipcErrorCode(err)));
    }
  };

  const copy = (value: string, message: string) => {
    void navigator.clipboard.writeText(value).then(() => toast.success(message));
  };

  const remaining = pairing ? expiresAt - now : 0;
  const expired = Boolean(pairing) && remaining <= 0;
  const available = Boolean(info);
  const lanAddress = pairing && !pairing.origin.startsWith('https://') ? pairing.origin : status.urls?.[0] || '';
  const ecosystems = (info?.ecosystems || []).map((id) => ECOSYSTEM_LABELS[id] || id).join(', ');

  return (
    <div className="rounded-xl border border-white/10 bg-white/[0.03] p-4 space-y-4">
      <div className="flex flex-wrap items-start justify-between gap-3">
        <div className="min-w-0">
          <div className="flex items-center gap-2 text-sm font-semibold">
            <Puzzle className="h-4 w-4" />
            {uiI18n.t('extensionServerHost.title')}
          </div>
          <p className="mt-1 text-xs text-muted-foreground">{uiI18n.t('extensionServerHost.description')}</p>
        </div>
        <SettingsBadge tone={!available && infoLoaded ? 'warning' : running ? 'success' : 'muted'}>
          {!available && infoLoaded
            ? uiI18n.t('extensionServerHost.statusUnavailable')
            : running
              ? uiI18n.t('extensionServerHost.statusRunning')
              : uiI18n.t('extensionServerHost.statusStopped')}
        </SettingsBadge>
      </div>

      {info ? (
        <div className="grid gap-1 text-xs text-muted-foreground">
          {info.name ? (
            <div>
              {uiI18n.t('extensionServerHost.serverName')}: <span className="text-foreground">{info.name}</span>
            </div>
          ) : null}
          <div>{uiI18n.t('extensionServerHost.sourceCount', { count: info.sourceCount })}</div>
          {ecosystems ? <div>{uiI18n.t('extensionServerHost.ecosystems', { list: ecosystems })}</div> : null}
        </div>
      ) : null}

      <div className="grid gap-2 text-xs">
        <div className="flex flex-wrap items-center gap-2">
          <span className="text-muted-foreground">{uiI18n.t('extensionServerHost.lanAddress')}:</span>
          {lanAddress ? <code className="break-all rounded bg-black/30 px-2 py-0.5">{lanAddress}</code> : <span>—</span>}
        </div>
        <div className="flex flex-wrap items-center gap-2">
          <span className="text-muted-foreground">{uiI18n.t('extensionServerHost.remoteAddress')}:</span>
          {remoteOrigin ? (
            <code className="break-all rounded bg-black/30 px-2 py-0.5">{remoteOrigin}</code>
          ) : (
            <span className="text-muted-foreground">{uiI18n.t('extensionServerHost.noRemoteAddress')}</span>
          )}
        </div>
      </div>

      <div className="flex flex-wrap gap-2">
        <Button variant="outline" size="sm" className="gap-1" disabled={!running || !available} onClick={() => void openDashboard()}>
          <ExternalLink className="h-3.5 w-3.5" />
          {uiI18n.t('extensionServerHost.openDashboard')}
        </Button>
        <Button
          variant="outline"
          size="sm"
          className="gap-1"
          disabled={!bootstrap}
          onClick={() => copy(bootstrap, uiI18n.t('extensionServerHost.setupSecretCopied'))}
        >
          <Copy className="h-3.5 w-3.5" />
          {uiI18n.t('extensionServerHost.copySetupSecret')}
        </Button>
      </div>
      <p className="text-xs text-muted-foreground">{uiI18n.t('extensionServerHost.setupSecretHint')}</p>

      <div className="space-y-3 rounded-lg border border-white/10 bg-black/20 p-3">
        <div className="flex items-center gap-2 text-sm font-semibold">
          <Smartphone className="h-4 w-4" />
          {uiI18n.t('extensionServerHost.linkPhone')}
        </div>
        <p className="text-xs text-muted-foreground">{uiI18n.t('extensionServerHost.linkPhoneDescription')}</p>
        <p className="text-xs text-muted-foreground">{uiI18n.t('extensionServerHost.sameNetworkHint')}</p>

        {pairing && !expired ? (
          <div className="flex flex-col gap-4 sm:flex-row sm:items-center">
            <div className="w-fit rounded-lg bg-white p-3">
              <QRCodeSVG value={pairing.deepLink} size={176} level="M" />
            </div>
            <div className="min-w-0 space-y-2 text-sm">
              <div>
                <span className="text-muted-foreground">{uiI18n.t('extensionServerHost.pairingCode')}: </span>
                <code className="rounded bg-black/30 px-2 py-0.5 font-mono tracking-wider">{pairing.code}</code>
              </div>
              <div className="text-xs text-muted-foreground">
                {uiI18n.t('extensionServerHost.expiresIn', { time: formatCountdown(remaining) })}
              </div>
              <code className="block max-w-full break-all text-xs text-muted-foreground">{pairing.origin}</code>
              <Button
                size="sm"
                variant="outline"
                className="gap-1"
                onClick={() => copy(pairing.deepLink, uiI18n.t('extensionServerHost.linkCopied'))}
              >
                <Copy className="h-3.5 w-3.5" />
                {uiI18n.t('extensionServerHost.copyLink')}
              </Button>
            </div>
          </div>
        ) : expired ? (
          <p className="text-xs text-amber">{uiI18n.t('extensionServerHost.expired')}</p>
        ) : null}

        {pairingError ? <p className="text-xs text-destructive">{pairingError}</p> : null}

        <div className="flex flex-wrap gap-2">
          <Button
            size="sm"
            variant="secondary"
            className="gap-1"
            disabled={!running || !available || pairingBusy}
            onClick={() => void generatePairing('lan')}
          >
            <RefreshCw className={`h-3.5 w-3.5${pairingBusy ? ' animate-spin' : ''}`} />
            {uiI18n.t('extensionServerHost.generateCode')}
          </Button>
          {remoteOrigin ? (
            <Button
              size="sm"
              variant="outline"
              className="gap-1"
              disabled={!running || !available || pairingBusy}
              onClick={() => void generatePairing('remote')}
            >
              <RefreshCw className="h-3.5 w-3.5" />
              {uiI18n.t('extensionServerHost.generateCode')} · {uiI18n.t('extensionServerHost.remoteAddress')}
            </Button>
          ) : null}
        </div>
      </div>

      <ExtensionServerD8Info />
    </div>
  );
}

export function HomeServerSettings() {
  useUiTranslation();
  const isDesktop = useIsDesktopApp();
  const [status, setStatus] = useState<HomeStatus>({});
  const [port, setPort] = useState(8787);
  const [bindMode, setBindMode] = useState<'localhost' | 'lan' | 'all'>('lan');
  const [password, setPassword] = useState('');
  const [remoteEnabled, setRemoteEnabled] = useState(false);
  const [remoteBaseUrl, setRemoteBaseUrl] = useState('');
  const [shareCatalog, setShareCatalog] = useState(true);
  const [requireConsent, setRequireConsent] = useState(true);
  const [viewerName, setViewerName] = useState('viewer');
  const [viewerPassword, setViewerPassword] = useState('');
  const [extensionBootstrap, setExtensionBootstrap] = useState('');
  const [busy, setBusy] = useState(false);
  const [deviceToken, setDeviceToken] = useState('');
  const [consentRequests, setConsentRequests] = useState<ConsentRequest[]>([]);

  const refresh = useCallback(async () => {
    const api = getHomeServerApi();
    if (!api) return;
    const [nextStatus, config, consent] = await Promise.all([
      api.getStatus(),
      api.getConfig(),
      api.listConsent().catch(() => ({ requests: [] })),
    ]);
    setStatus(nextStatus || {});
    if (config?.port) setPort(config.port);
    if (config?.bindMode) setBindMode(config.bindMode);
    if (typeof config?.remoteEnabled === 'boolean') setRemoteEnabled(config.remoteEnabled);
    if (config?.remoteBaseUrl) setRemoteBaseUrl(config.remoteBaseUrl);
    if (typeof config?.shareCatalog === 'boolean') setShareCatalog(config.shareCatalog);
    if (typeof config?.requireConsent === 'boolean') setRequireConsent(config.requireConsent);
    setExtensionBootstrap(config?.extensionServerBootstrap || '');
    setConsentRequests(Array.isArray(consent?.requests) ? consent.requests : []);
  }, []);

  useEffect(() => {
    if (!isDesktop) return;
    void refresh();
    const api = getHomeServerApi();
    const unsub = api?.onStatus?.((data: HomeStatus) => setStatus(data));
    const timer = window.setInterval(() => void refresh(), 8000);
    return () => {
      unsub?.();
      window.clearInterval(timer);
    };
  }, [isDesktop, refresh]);

  if (!isDesktop) return null;

  const saveConfig = async () => {
    setBusy(true);
    try {
      const api = getHomeServerApi();
      if (!api) throw new Error(uiI18n.t('app.home_server_bridge_missing_restart_the_app_0df6c41d'));
      await api.setConfig({
        port,
        bindMode,
        remoteEnabled,
        remoteBaseUrl,
        shareCatalog,
        requireConsent,
        password: password || undefined,
      });
      setPassword('');
      toast.success(uiI18n.t('app.home_server_settings_saved_6072945f'));
      await refresh();
    } catch (err) {
      toast.error(err instanceof Error ? err.message : uiI18n.t('app.failed_to_save_23778b5f'));
    } finally {
      setBusy(false);
    }
  };

  const start = async () => {
    setBusy(true);
    try {
      const api = getHomeServerApi();
      if (!api) throw new Error(uiI18n.t('app.home_server_bridge_missing_restart_the_app_0df6c41d'));
      await api.setConfig({
        port,
        bindMode,
        remoteEnabled,
        remoteBaseUrl,
        shareCatalog,
        requireConsent,
        password: password || undefined,
      });
      const res = await api.start({ port, bindMode });
      if (res && res.success === false) throw new Error(res.error || 'Start failed');
      toast.success(uiI18n.t('app.home_server_started_a3b53fbb'));
      await refresh();
    } catch (err) {
      toast.error(err instanceof Error ? err.message : uiI18n.t('app.start_failed_ef434c64'));
    } finally {
      setBusy(false);
    }
  };

  const stop = async () => {
    setBusy(true);
    try {
      const api = getHomeServerApi();
      await api?.stop();
      toast.success(uiI18n.t('app.home_server_stopped_bed7163a'));
      await refresh();
    } catch (err) {
      toast.error(err instanceof Error ? err.message : uiI18n.t('app.stop_failed_3dfcbc0f'));
    } finally {
      setBusy(false);
    }
  };

  const createToken = async () => {
    const api = getHomeServerApi();
    const res = await api?.createToken({ userId: 'admin', get label() { return uiI18n.t('app.phone_f6be6ca9'); } });
    if (res?.token) {
      setDeviceToken(res.token);
      toast.success(uiI18n.t('app.device_token_created_07db7bbd'));
    }
  };

  const upsertViewer = async () => {
    const api = getHomeServerApi();
    const res = await api?.upsertUser({
      id: viewerName || 'viewer',
      name: viewerName || 'Viewer',
      role: 'viewer',
      permissions: ['library', 'streams', 'downloads', 'catalog'],
      password: viewerPassword || undefined,
    });
    if (res?.success) {
      toast.success(uiI18n.t('app.household_user_saved_0e48dc1c'));
      setViewerPassword('');
    } else {
      toast.error(res?.error || uiI18n.t('app.failed_to_save_user_f0ec5737'));
    }
  };

  const decideConsent = async (id: string, approve: boolean) => {
    const api = getHomeServerApi();
    await api?.decideConsent({ id, approve });
    toast.success(approve ? uiI18n.t('app.access_approved_3b89815a') : uiI18n.t('app.access_denied_64343a83'));
    await refresh();
  };

  const primaryUrl = status.urls?.[0] || `http://127.0.0.1:${port}`;

  return (
    <SettingsSection
      title="Tatakai Extension Server"
      description="Host extension sources and a media proxy, with your existing library and household sharing."
      bodyClassName="space-y-6"
      action={
        <div className="flex items-center gap-2">
          <SettingsBadge tone={status.running ? 'success' : 'muted'}>
            {status.running ? uiI18n.t('app.running_73989d9c') : uiI18n.t('app.stopped_51e9111b')}
          </SettingsBadge>
          {status.running ? (
            <Button variant="outline" size="sm" disabled={busy} onClick={stop} className="gap-2">
              <Square className="h-3.5 w-3.5" />
              {uiI18n.t('app.stop_9e253470')}
            </Button>
          ) : (
            <Button size="sm" disabled={busy} onClick={start} className="gap-2">
              <Play className="h-3.5 w-3.5" />
              {uiI18n.t('app.start_server_201c97df')}
            </Button>
          )}
        </div>
      }
    >
      <div className="flex flex-wrap gap-2">
        <Button variant="outline" disabled={!status.running} asChild><a href={`${primaryUrl}/home`} target="_blank" rel="noreferrer">Open shared library</a></Button>
      </div>
      <ExtensionServerPanel status={status} bootstrap={extensionBootstrap} />
      <div className="flex flex-col">

        <SettingRow
          title={uiI18n.t('app.port_fe035157')}
          description={uiI18n.t('app.tcp_port_the_home_server_listens_on_8978a2d8')}
          control={
            <Input
              type="number"
              value={port}
              onChange={(e) => setPort(Number(e.target.value) || 8787)}
              className="w-28 bg-white/5 border-white/10"
            />
          }
        />
        <SettingRow
          title={uiI18n.t('app.bind_mode_bedd6fca')}
          description={uiI18n.t('app.which_network_interfaces_the_server_accepts_conn_eba2fa0f')}
          control={(ids) => (
            <Select value={bindMode} onValueChange={(v) => setBindMode(v as 'localhost' | 'lan' | 'all')}>
              <SelectTrigger aria-labelledby={ids.labelId} aria-describedby={ids.descriptionId} className="h-9 w-52">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="localhost">{uiI18n.t('app.localhost_only_07a01419')}</SelectItem>
                <SelectItem value="lan">{uiI18n.t('app.lan_recommended_c34c15ba')}</SelectItem>
                <SelectItem value="all">{uiI18n.t('app.all_interfaces_explicit_acfc71b0')}</SelectItem>
              </SelectContent>
            </Select>
          )}
        />
        <SettingRow
          title={uiI18n.t('app.admin_password_b2cee170')}
          icon={Shield}
          description={status.passwordConfigured ? uiI18n.t('app.a_password_is_set_leave_blank_to_keep_it_26b8e655') : uiI18n.t('app.set_a_password_before_sharing_d9f0b5a0')}
        >
          <Input
            type="password"
            placeholder={status.passwordConfigured ? uiI18n.t('app.leave_blank_to_keep_a2bf4757') : uiI18n.t('app.set_a_password_before_sharing_6bfa5f5f')}
            value={password}
            onChange={(e) => setPassword(e.target.value)}
            className="bg-white/5 border-white/10"
          />
        </SettingRow>

        <SettingRow
          tone="accent"
          title={uiI18n.t('app.share_download_catalog_b05abf3f')}
          icon={Share2}
          description={uiI18n.t('app.other_authenticated_users_can_list_your_offline__e0b1ecf0')}
          control={<Switch checked={shareCatalog} onCheckedChange={setShareCatalog} aria-label={uiI18n.t('app.share_download_catalog_b05abf3f')} />}
        />
        <SettingRow
          title={uiI18n.t('app.require_host_consent_780d2b08')}
          description={uiI18n.t('app.viewers_must_request_access_you_approve_before_t_e452d02c')}
          control={<Switch checked={requireConsent} onCheckedChange={setRequireConsent} disabled={!shareCatalog} aria-label={uiI18n.t('app.require_host_consent_780d2b08')} />}
        />
        <div className="pt-2">
          <Button variant="outline" size="sm" disabled={busy} onClick={saveConfig}>
            {uiI18n.t('app.save_settings_913aba9f')}
          </Button>
        </div>
      </div>

      {consentRequests.filter((r) => r.status === 'pending').length > 0 ? (
        <div className="rounded-xl border border-amber/30 bg-amber/5 p-4 space-y-3">
          <div className="text-sm font-semibold">{uiI18n.t('app.pending_consent_requests_1ee3dabf')}</div>
          {consentRequests
            .filter((r) => r.status === 'pending')
            .map((req) => (
              <div key={req.id} className="flex items-center justify-between gap-3 text-sm">
                <div>
                  <div className="font-medium">{req.userId}</div>
                  <div className="text-xs text-muted-foreground">{req.label || uiI18n.t('app.device_f3a929b3')} · {new Date(req.createdAt).toLocaleString(uiI18n.language)}</div>
                </div>
                <div className="flex gap-2">
                  <Button size="sm" variant="outline" className="gap-1" onClick={() => decideConsent(req.id, true)}>
                    <Check className="h-3.5 w-3.5" /> {uiI18n.t('app.approve_7b2c7f14')}
                  </Button>
                  <Button size="sm" variant="ghost" className="gap-1" onClick={() => decideConsent(req.id, false)}>
                    <X className="h-3.5 w-3.5" /> {uiI18n.t('app.deny_53577bb5')}
                  </Button>
                </div>
              </div>
            ))}
        </div>
      ) : null}

      <div className="rounded-xl border border-white/10 bg-white/[0.03] p-4 space-y-3">
        <div className="text-sm font-semibold">{uiI18n.t('app.connect_from_phone_tv_or_browser_54248ecc')}</div>
        <div className="flex flex-wrap items-center gap-2 text-sm">
          <code className="min-w-0 max-w-full break-all rounded bg-black/30 px-2 py-1">{primaryUrl}</code>
          <Button
            size="sm"
            variant="outline"
            className="gap-1"
            onClick={() => {
              void navigator.clipboard.writeText(primaryUrl);
              toast.success(uiI18n.t('app.url_copied_73a9becc'));
            }}
          >
            <Copy className="h-3.5 w-3.5" />
            {uiI18n.t('app.copy_af74f7c5')}
          </Button>
          <Button
            size="sm"
            variant="secondary"
            className="gap-1"
            disabled={!status.running}
            onClick={() => window.open(primaryUrl, '_blank', 'noopener,noreferrer')}
          >
            <ExternalLink className="h-3.5 w-3.5" />
            {uiI18n.t('app.open_home_ui_ac6ed5ce')}
          </Button>
        </div>
        <p className="text-xs text-muted-foreground">
          {uiI18n.t('app.opens_the_tatakai_branded_web_app_at_that_url_br_6fa14a5d')} <strong>{uiI18n.t('app.share_download_catalog_b05abf3f')}</strong> {uiI18n.t('app.above_so_remote_devices_can_watch_4de50a43')}
        </p>
        <Button size="sm" variant="secondary" onClick={createToken}>
          {uiI18n.t('app.create_device_token_2a635111')}
        </Button>
        {deviceToken ? (
          <code className="block text-xs break-all rounded bg-black/30 p-2">{deviceToken}</code>
        ) : null}
      </div>

      <div className="rounded-xl border border-white/10 bg-white/[0.03] p-4 space-y-4">
        <div className="flex items-center justify-between gap-3">
          <div>
            <div className="text-sm font-semibold">{uiI18n.t('app.remote_access_d78a6045')}</div>
            <p className="text-xs text-muted-foreground mt-1">
              {uiI18n.t('app.enable_only_behind_a_reverse_proxy_caddy_nginx_w_0d7dab1b')}
            </p>
          </div>
          <Switch checked={remoteEnabled} onCheckedChange={setRemoteEnabled} aria-label={uiI18n.t('app.enable_remote_access_11d88d53')} />
        </div>
        <Input
          placeholder="https://home.example.com"
          value={remoteBaseUrl}
          disabled={!remoteEnabled}
          onChange={(e) => setRemoteBaseUrl(e.target.value)}
        />
      </div>

      <div className="rounded-xl border border-white/10 bg-white/[0.03] p-4 space-y-3">
        <div className="flex items-center gap-2 text-sm font-semibold">
          <Users className="h-4 w-4" />
          {uiI18n.t('app.household_user_viewer_1fce5046')}
        </div>
        <div className="grid gap-3 md:grid-cols-2">
          <Input placeholder={uiI18n.t('app.username_84c29015')} value={viewerName} onChange={(e) => setViewerName(e.target.value)} />
          <Input
            type="password"
            placeholder={uiI18n.t('app.password_8be3c943')}
            value={viewerPassword}
            onChange={(e) => setViewerPassword(e.target.value)}
          />
        </div>
        <Button size="sm" variant="outline" onClick={upsertViewer}>
          {uiI18n.t('app.save_viewer_62f09c77')}
        </Button>
      </div>
    </SettingsSection>
  );
}
