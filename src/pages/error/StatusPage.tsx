import uiI18n from '@/lib/uiI18n';
import { useUiTranslation } from '@/hooks/ui/useUiTranslation';
import { useState, useEffect, useCallback, useMemo } from 'react';
import { useNavigate } from 'react-router-dom';
import { Sidebar } from '@/components/layout/Sidebar';
import { MobileNav } from '@/components/layout/MobileNav';
import { GlassPanel } from '@/components/ui/GlassPanel';
import { motion, AnimatePresence } from 'framer-motion';
import {
  ArrowLeft, CheckCircle, XCircle, AlertTriangle, RefreshCw, Server,
  Database, Globe, Clock, Image as ImageIcon, Activity, Gauge, Loader2,
} from 'lucide-react';
import { Button } from '@/components/ui/button';
import { supabase } from '@/integrations/supabase/client';
import { useStatusIncidents } from '@/hooks/admin/useAdminFeatures';
import { formatDistanceToNow } from '@/lib/localeFormat';
import { StatusPageBackground } from '@/components/layout/StatusPageBackground';
import { anilistGql } from '@/lib/anilist/client';
import { publicFetch } from '@/lib/net/publicApi';
import { getProviderHealth, listMetadataProviders, type ProviderId } from '@/lib/metadata';
import { loadStoredProxies } from '@/hooks/user/useProxySettings';
import { useIsDesktopApp } from '@/hooks/ui/useIsNativeApp';

type Health = 'operational' | 'degraded' | 'down' | 'checking';

interface ServiceStatus {
  name: string;
  status: Health;
  latency?: number;
  icon: React.ReactNode;
  description: string;
  url?: string;
  /** Set for metadata-provider rows (health comes from the router). */
  providerId?: ProviderId;
  /** Provider is not configured: shown as such and left out of the health maths. */
  skipped?: boolean;
}

interface ProxyDisplayNode {
  id: string; url: string; type: string;
  status: 'online' | 'degraded' | 'offline'; latencyMs: number; score: number;
}
function buildInitialServices(): ServiceStatus[] {
  const base: ServiceStatus[] = [
    { name: 'Supabase', status: 'checking', icon: <Database className="w-5 h-5" />, get description() { return uiI18n.t('app.database_auth_infrastructure_794b6e51'); } },
    { name: 'Image Delivery', status: 'checking', icon: <ImageIcon className="w-5 h-5" />, get description() { return uiI18n.t('app.posters_banners_thumbnails_054c6616'); }, url: 'https://cdn.myanimelist.net' },
  ];
  const providers: ServiceStatus[] = listMetadataProviders().map((p) => ({
    name: p.label,
    status: 'checking' as Health,
    icon: <Server className="w-5 h-5" />,
    description: p.description,
    providerId: p.id,
    skipped: !p.isConfigured(),
  }));
  return [...base, ...providers];
}

/** One lightweight live request per provider; any 2xx counts as reachable. */
async function probeProvider(id: ProviderId): Promise<{ status: Health; latency: number }> {
  const start = Date.now();
  const grade = (ok: boolean, status: number, slowMs: number) => {
    const latency = Date.now() - start;
    return { status: (ok ? (latency > slowMs ? 'degraded' : 'operational') : (status === 429 ? 'degraded' : 'down')) as Health, latency };
  };
  const signal = AbortSignal.timeout(10000);
  switch (id) {
    case 'anilist':
      await anilistGql('{ Media(id:1){ id } }', undefined, { signal, retries: 0 });
      return grade(true, 200, 1500);
    case 'tenrai': {
      const r = await publicFetch('https://api.tenrai.org/v1/anime/1', { signal, retries: 0 });
      return grade(r.ok, r.status, 1500);
    }
    case 'kitsu': {
      const r = await publicFetch('https://kitsu.io/api/edge/anime/1', { signal, retries: 0, headers: { Accept: 'application/vnd.api+json' } });
      return grade(r.ok, r.status, 1500);
    }
    case 'anizip': {
      const r = await publicFetch('https://api.ani.zip/mappings?anilist_id=1', { signal, retries: 0 });
      return grade(r.ok, r.status, 1500);
    }
    case 'shikimori': {
      const r = await publicFetch('https://shikimori.io/api/graphql', {
        method: 'POST', signal, retries: 0,
        headers: { 'Content-Type': 'application/json', Accept: 'application/json' },
        body: JSON.stringify({ query: '{ animes(ids:"1"){ id } }' }),
      });
      return grade(r.ok, r.status, 1500);
    }
    case 'mal': {
      const r = await publicFetch('https://api.myanimelist.net/v2/anime/1?fields=id', {
        signal, retries: 0,
        headers: { 'X-MAL-CLIENT-ID': String(import.meta.env.VITE_MAL_CLIENT_ID || '').trim() },
      });
      return grade(r.ok, r.status, 1500);
    }
    default:
      return { status: 'down', latency: 0 };
  }
}

const SERVICE_CHECK_FREQ_SECONDS = 30;
const PROXY_POLL_FREQ_SECONDS = 4;
const HISTORY_CAP = 32;
// Proxies the app is actually configured to stream through (from .env). The
// board probes these directly whenever the backend aggregator is unreachable OR
// only reports loopback/dev nodes, so the pool reflects the real public edge.
const ENV_CF_PROXY_URL = String(import.meta.env.VITE_PROXY_CF_URL || '').trim();
const ENV_NODE_PROXY_URL = String(
  import.meta.env.VITE_STREAM_PROXY_URL || import.meta.env.VITE_PROXY_NODE_URL || ''
).trim();
// Host of the configured Cloudflare worker so classifyProxyType can tag it 'cf'
// even behind a custom domain with no cloudflare marker.
const CF_PROXY_HOST = (() => {
  try { return ENV_CF_PROXY_URL ? new URL(ENV_CF_PROXY_URL).hostname.toLowerCase() : ''; }
  catch { return ''; }
})();

function classifyProxyType(url: string): string {
  const lower = (url || '').toLowerCase();
  if (CF_PROXY_HOST && lower.includes(CF_PROXY_HOST)) return 'cf';
  if (lower.includes('workers.dev') || lower.includes('cloudflare')) return 'cf';
  if (lower.includes('bun')) return 'bun';
  return 'nodejs';
}

/** A loopback / private-range proxy URL is a backend dev-config artifact and is
 *  meaningless on a public status board, so we drop it and probe the real edge. */
function isLoopbackProxyUrl(url: string): boolean {
  try {
    const h = new URL(url).hostname.toLowerCase().replace(/^\[|\]$/g, '');
    return h === 'localhost' || h.endsWith('.localhost') || h === '127.0.0.1' || h === '::1' ||
      /^10\./.test(h) || /^192\.168\./.test(h) || /^172\.(1[6-9]|2\d|3[01])\./.test(h);
  } catch { return false; }
}

type KnownProxyNode = { id: string; url: string; type: string };

/** Remote proxies the user/build configured. Loopback (in-app) proxies are skipped. */
function getKnownProxyNodes(): KnownProxyNode[] {
  const nodes: KnownProxyNode[] = [];
  const seen = new Set<string>();
  const add = (id: string, url: string, type: string) => {
    const key = url.trim().replace(/\/$/, '');
    if (!key || seen.has(key) || isLoopbackProxyUrl(key)) return;
    seen.add(key);
    nodes.push({ id, url: key, type });
  };
  if (ENV_CF_PROXY_URL) add('proxy-cf-edge', ENV_CF_PROXY_URL, 'cf');
  if (ENV_NODE_PROXY_URL) add('proxy-node-env', ENV_NODE_PROXY_URL, classifyProxyType(ENV_NODE_PROXY_URL));
  try {
    for (const entry of loadStoredProxies()) {
      if (entry.enabled && entry.url) add(`proxy-${entry.id}`, entry.url, classifyProxyType(entry.url));
    }
  } catch { /* storage unavailable */ }
  return nodes;
}

function buildProxyProbeUrls(proxyUrl: string, proxyPassword: string): string[] {
  const normalized = String(proxyUrl || '').trim().replace(/\/$/, '');
  if (!normalized) return [];
  const streamEndpoint = /\/api\/v1\/streamingproxy$/i.test(normalized) ? normalized : `${normalized}/api/v1/streamingProxy`;
  const rootEndpoint = normalized.replace(/\/api\/v1\/streamingproxy$/i, '');
  const params = new URLSearchParams({ url: 'https://test-streams.mux.dev/x36xhzz/x36xhzz.m3u8', type: 'video' });
  if (proxyPassword) params.set('password', proxyPassword);
  return Array.from(new Set([`${rootEndpoint}/health`, `${streamEndpoint}?${params.toString()}`]));
}

async function probeKnownProxyNode(node: KnownProxyNode, proxyPassword: string): Promise<ProxyDisplayNode> {
  for (const probeUrl of buildProxyProbeUrls(node.url, proxyPassword)) {
    const start = performance.now();
    try {
      const response = await fetch(probeUrl, { signal: AbortSignal.timeout(5000) });
      const latencyMs = Math.round(performance.now() - start);
      const onlineLike = response.ok || [401, 403, 405].includes(response.status);
      if (onlineLike) return { id: node.id, url: node.url, type: node.type, status: 'online', latencyMs, score: 1 };
      if (response.status >= 400 && response.status < 500)
        return { id: node.id, url: node.url, type: node.type, status: 'degraded', latencyMs, score: 0 };
    } catch { /* try next probe url */ }
  }
  return { id: node.id, url: node.url, type: node.type, status: 'offline', latencyMs: 0, score: 0 };
}     

// Stable, long-lived CDN assets the app actually pulls posters/thumbs from. An
// <img> load isn't CORS-gated the way fetch() is, so this measures real image
// delivery instead of poking an unrelated JSON API (the old api.nekosapi.com
// probe 404'd, which is why Image Delivery always showed "down").
const IMAGE_DELIVERY_PROBES = [
  // Tiny, always-present site assets first (fast + hash-free, so they don't rot
  // like a specific cover path can); the anime cover stays as a real-world check.
  'https://cdn.myanimelist.net/images/favicon.ico',
  'https://cdn.myanimelist.net/img/sp/icon/apple-touch-icon-256.png',
  'https://s4.anilist.co/file/anilistcdn/media/anime/cover/medium/bx1-CXtrrkMpJ8Zq.png',
];

function probeImageLoad(src: string, timeoutMs = 8000): Promise<number> {
  return new Promise((resolve, reject) => {
    const img = new Image();
    const start = performance.now();
    const timer = setTimeout(() => { img.onload = null; img.onerror = null; reject(new Error(uiI18n.t('app.timeout_56d3c949'))); }, timeoutMs);
    img.onload = () => { clearTimeout(timer); resolve(Math.round(performance.now() - start)); };
    img.onerror = () => { clearTimeout(timer); reject(new Error(uiI18n.t('app.load_failed_a78f1164'))); };
    img.src = `${src}${src.includes('?') ? '&' : '?'}_cb=${Date.now()}`;
  });
}

const SEVERITY_COLORS: Record<string, string> = {
  minor: 'bg-yellow-500/20 text-yellow-500 border-yellow-500/50',
  major: 'bg-orange-500/20 text-orange-500 border-orange-500/50',
  critical: 'bg-red-500/20 text-red-500 border-red-500/50',
};

function dotClass(status: Health | ProxyDisplayNode['status']): string {
  if (status === 'operational' || status === 'online') return 'bg-emerald-400';
  if (status === 'degraded') return 'bg-amber-400';
  if (status === 'checking') return 'bg-primary animate-pulse';
  return 'bg-red-500';
}

function StatusGlyph({ status, className = 'w-5 h-5' }: { status: Health; className?: string }) {
  useUiTranslation();
  if (status === 'operational') return <CheckCircle className={`${className} text-emerald-400`} />;
  if (status === 'degraded') return <AlertTriangle className={`${className} text-amber-400`} />;
  if (status === 'down') return <XCircle className={`${className} text-red-500`} />;
  return <RefreshCw className={`${className} text-primary animate-spin`} />;
}

/** Rolling per-service history rendered as a betterstack-style tick strip. */
function UptimeBars({ history }: { history: Health[] }) {
  useUiTranslation();
  const recent = history.slice(-HISTORY_CAP);
  const padded: (Health | null)[] = [
    ...Array.from({ length: Math.max(0, HISTORY_CAP - recent.length) }, () => null),
    ...recent,
  ];
  return (
    <div className="flex items-end gap-[3px] h-8" aria-hidden>
      {padded.map((h, i) => {
        const color = h === 'operational' ? 'bg-emerald-400/80'
          : h === 'degraded' ? 'bg-amber-400/80'
          : h === 'down' ? 'bg-red-500/80' : 'bg-white/10';
        const height = h === 'operational' ? 'h-full' : h === 'degraded' ? 'h-2/3'
          : h === 'down' ? 'h-1/2' : 'h-1/5';
        return <div key={i} className={`flex-1 rounded-full min-w-[2px] ${color} ${height}`} />;
      })}
    </div>
  );
}

/** Animated circular gauge for the aggregate health index. */
function HealthRing({ value, color }: { value: number; color: string }) {
  useUiTranslation();
  const r = 42;
  const circumference = 2 * Math.PI * r;
  return (
    <div className="relative w-28 h-28 shrink-0">
      <svg viewBox="0 0 100 100" className="w-full h-full -rotate-90">
        <circle cx="50" cy="50" r={r} fill="none" stroke="rgba(255,255,255,0.08)" strokeWidth="8" />
        <motion.circle
          cx="50" cy="50" r={r} fill="none" stroke={color} strokeWidth="8" strokeLinecap="round"
          strokeDasharray={circumference}
          initial={{ strokeDashoffset: circumference }}
          animate={{ strokeDashoffset: circumference - (circumference * value) / 100 }}
          transition={{ duration: 1.2, ease: 'easeOut' }}
        />
      </svg>
      <div className="absolute inset-0 flex flex-col items-center justify-center">
        <span className="text-2xl font-black font-mono">{value.toFixed(0)}%</span>
        <span className="text-[9px] uppercase tracking-[0.2em] text-muted-foreground">{uiI18n.t('app.health_834b34f1')}</span>
      </div>
    </div>
  );
}
export default function StatusPage() {
  useUiTranslation();
  const navigate = useNavigate();
  const isDesktopApp = useIsDesktopApp();
  const { data: incidents = [], isLoading: loadingIncidents } = useStatusIncidents(false);

  const [proxies, setProxies] = useState<ProxyDisplayNode[]>([]);
  const [proxiesLoaded, setProxiesLoaded] = useState(false);
  const [services, setServices] = useState<ServiceStatus[]>(buildInitialServices);
  const [uptimeHistory, setUptimeHistory] = useState<Record<string, Health[]>>({});
  const [lastChecked, setLastChecked] = useState<Date>(new Date());
  const [isRefreshing, setIsRefreshing] = useState(false);
  const [nextCheckIn, setNextCheckIn] = useState(SERVICE_CHECK_FREQ_SECONDS);

  const sharedProxyPassword = String(
    import.meta.env.VITE_STREAM_PROXY_PASSWORD || import.meta.env.VITE_PROXY_PASSWORD || ''
  ).trim();
  const loadProxyStatus = useCallback(async () => {
    const nodes = getKnownProxyNodes();
    const probed = await Promise.all(nodes.map((n) => probeKnownProxyNode(n, sharedProxyPassword)));
    setProxies(probed);
    setProxiesLoaded(true);
    return probed;
  }, [sharedProxyPassword]);
  // All service probes fire in parallel so the board fills in one pass instead
  // of staggering through six sequential awaits (the old flow left AniList stuck
  // on "checking" forever because it was never probed at all).
  const runChecks = useCallback(async () => {
    setIsRefreshing(true);
    setNextCheckIn(SERVICE_CHECK_FREQ_SECONDS);
    setServices((prev) => prev.map((s) => ({ ...s, status: 'checking', latency: undefined })));

    const check = async (
      name: string,
      fn: () => Promise<{ status: Health; latency: number }>
    ): Promise<{ name: string; status: Health; latency: number }> => {
      try { return { name, ...(await fn()) }; }
      catch { return { name, status: 'down', latency: 0 }; }
    };
    const providerChecks = listMetadataProviders().map((p) =>
      p.isConfigured()
        ? check(p.label, () => probeProvider(p.id))
        : Promise.resolve({ name: p.label, status: 'checking' as Health, latency: 0, skipped: true })
    );
    const results = await Promise.all([
      check('Supabase', async () => {
        const start = Date.now();
        const { error } = await supabase.from('profiles').select('count').limit(1);
        const latency = Date.now() - start;
        return { status: error ? 'down' : (latency > 500 ? 'degraded' : 'operational'), latency };
      }),
      check('Image Delivery', async () => {
        for (const src of IMAGE_DELIVERY_PROBES) {
          try {
            const latency = await probeImageLoad(src);
            return { status: latency > 1500 ? 'degraded' : 'operational', latency };
          } catch { /* try next CDN before giving up */ }
        }
        return { status: 'down', latency: 0 };
      }),
      ...providerChecks,
    ]) as Array<{ name: string; status: Health; latency: number; skipped?: boolean }>;
    setServices((prev) => prev.map((s) => {
      const r = results.find((x) => x.name === s.name);
      return r ? { ...s, status: r.status, latency: r.latency, skipped: r.skipped ?? false } : s;
    }));
    setUptimeHistory((prev) => {
      const next = { ...prev };
      for (const r of results) {
        if (r.skipped) continue;
        next[r.name] = [...(next[r.name] || []), r.status].slice(-HISTORY_CAP);
      }
      return next;
    });

    setLastChecked(new Date());
    setIsRefreshing(false);
  }, [loadProxyStatus]);

  useEffect(() => {
    runChecks();
    loadProxyStatus().catch(() => { /* proxies unreachable on first paint */ });

    const proxyInterval = setInterval(() => { loadProxyStatus().catch(() => {}); }, PROXY_POLL_FREQ_SECONDS * 1000);
    const serviceInterval = setInterval(() => { runChecks(); }, SERVICE_CHECK_FREQ_SECONDS * 1000);
    const countdownInterval = setInterval(
      () => setNextCheckIn((v) => (v <= 1 ? SERVICE_CHECK_FREQ_SECONDS : v - 1)),
      1000
    );
    return () => {
      clearInterval(proxyInterval);
      clearInterval(serviceInterval);
      clearInterval(countdownInterval);
    };
  }, [runChecks, loadProxyStatus]);
  const healthIndex = useMemo(() => {
    const svc = services.filter((s) => !s.skipped && s.status !== 'checking').map((s) => (s.status === 'operational' ? 1 : s.status === 'degraded' ? 0.6 : 0));
    const prx = proxies.map((p) => (p.status === 'online' ? 1 : p.status === 'degraded' ? 0.6 : 0));
    const all = [...svc, ...prx];
    if (!all.length) return 0;
    return Number(((all.reduce((a, b) => a + b, 0) / all.length) * 100).toFixed(1));
  }, [services, proxies]);

  const uptimeSamples = useMemo(() => Object.values(uptimeHistory).flat(), [uptimeHistory]);
  const uptimePct = useMemo(() => {
    if (!uptimeSamples.length) return null;
    const up = uptimeSamples.filter((s) => s === 'operational').length;
    return Number(((up / uptimeSamples.length) * 100).toFixed(2));
  }, [uptimeSamples]);

  const mttrMinutes = useMemo(() => {
    const resolved = incidents.filter((i: any) => !i.is_active && i.created_at && i.resolved_at);
    const durations = resolved
      .map((i: any) => {
        const start = new Date(i.created_at).getTime();
        const end = new Date(i.resolved_at).getTime();
        return end > start ? (end - start) / 60000 : null;
      })
      .filter((v: number | null): v is number => v !== null);
    if (!durations.length) return null;
    return Math.round(durations.reduce((a, b) => a + b, 0) / durations.length);
  }, [incidents]);

  const activeIncidents = incidents.filter((i: any) => i.is_active).length;
  const activeServices = services.filter((s) => !s.skipped);
  const anyDown = activeServices.some((s) => s.status === 'down') || proxies.some((p) => p.status === 'offline');
  const anyDegraded = activeServices.some((s) => s.status === 'degraded') || proxies.some((p) => p.status === 'degraded');
  const anyChecking = activeServices.some((s) => s.status === 'checking') || isRefreshing;

  const overall: Health = anyDown ? 'down' : anyDegraded ? 'degraded' : anyChecking ? 'checking' : 'operational';
  const overallLabel = {
    down: 'Major Outage',
    degraded: 'Partial Degradation',
    checking: 'Analyzing Infrastructure',
    operational: 'All Systems Operational',
  }[overall];
  const overallGradient = overall === 'down' ? 'from-red-500/20 via-red-900/30 to-red-500/10'
    : overall === 'degraded' ? 'from-amber-500/20 via-amber-900/30 to-amber-500/10'
    : overall === 'checking' ? 'from-primary/15 via-primary/10 to-primary/5'
    : 'from-emerald-500/20 via-emerald-900/30 to-emerald-500/10';
  const ringColor = overall === 'down' ? 'rgb(239 68 68)'
    : overall === 'degraded' ? 'rgb(251 191 36)'
    : overall === 'checking' ? 'rgb(236 72 153)'
    : 'rgb(52 211 153)';

  const kpis = [
    { get label() { return uiI18n.t('app.health_index_576bc73c'); }, value: `${healthIndex.toFixed(1)}%`, get detail() { return uiI18n.t('app.i18n_value_0_i18n_value_1_services_up_06e178fa', { value0: activeServices.filter((s) => s.status === 'operational').length, value1: activeServices.length }); }, icon: <Gauge className="w-4 h-4" /> },
    { get label() { return uiI18n.t('app.observed_uptime_b7bcc5a8'); }, value: uptimePct !== null ? `${uptimePct}%` : '--', get detail() { return uiI18n.t('app.i18n_value_0_samples_e420e8b0', { value0: uptimeSamples.length }); }, icon: <Activity className="w-4 h-4" /> },
    { get label() { return uiI18n.t('app.proxy_pool_66df8d9e'); }, value: `${proxies.filter((p) => p.status === 'online').length}/${proxies.length}`, get detail() { return uiI18n.t('app.i18n_value_0_degraded_353cccde', { value0: proxies.filter((p) => p.status === 'degraded').length }); }, icon: <Server className="w-4 h-4" /> },
    { get label() { return uiI18n.t('app.incidents_5e326b17'); }, value: `${activeIncidents}`, detail: mttrMinutes !== null ? `MTTR ${mttrMinutes}m` : 'no history', icon: <AlertTriangle className="w-4 h-4" /> },
  ];
  return (
    <div className="min-h-screen bg-background text-foreground overflow-x-hidden relative">
      <StatusPageBackground overlayColor="from-background/95 via-background/80 to-background/95" />
      <Sidebar />

      <div className="fixed inset-0 pointer-events-none overflow-hidden z-0">
        <motion.div
          className="absolute top-[15%] right-[5%] text-9xl font-bold text-primary/5 select-none"
          animate={{ y: [0, -30, 0], opacity: [0.03, 0.08, 0.03] }}
          transition={{ duration: 12, repeat: Infinity, ease: 'easeInOut' }}
        >状態</motion.div>
        <motion.div
          className="absolute bottom-[20%] left-[8%] text-8xl font-bold text-accent/5 select-none"
          animate={{ y: [0, 20, 0], opacity: [0.03, 0.06, 0.03] }}
          transition={{ duration: 10, repeat: Infinity, ease: 'easeInOut', delay: 2 }}
        >稼働</motion.div>
        <div className="absolute top-0 right-1/3 w-px h-full bg-gradient-to-b from-transparent via-primary/10 to-transparent" />
        <div className="absolute top-0 left-1/4 w-px h-full bg-gradient-to-b from-transparent via-accent/10 to-transparent" />
      </div>

      <main className={`relative z-10 ${isDesktopApp ? 'pl-6' : 'pl-6 md:pl-32'} pr-6 py-6 max-w-[1400px] mx-auto pb-24 md:pb-6`}>
        <div className="flex flex-wrap items-center justify-between gap-4 mb-8">
          <button
            onClick={() => navigate(-1)}
            className="group flex items-center gap-2 p-2 rounded-full hover:bg-white/5 transition-all"
          >
            <ArrowLeft className="w-5 h-5 text-muted-foreground group-hover:text-foreground group-hover:-translate-x-1 transition-all" />
            <span className="text-sm font-medium text-muted-foreground group-hover:text-foreground">{uiI18n.t('app.return_24f096b2')}</span>
          </button>
          <div className="flex items-center gap-3">
            <div className="hidden sm:flex items-center gap-2 text-[11px] text-muted-foreground font-mono bg-white/5 px-3 py-1.5 rounded-full border border-white/10">
              <RefreshCw className={`w-3 h-3 ${isRefreshing ? 'animate-spin text-primary' : ''}`} />
              {isRefreshing ? uiI18n.t('app.checking_820d6004') : uiI18n.t('app.next_check_in_i18n_value_0s_5ecb13de', { value0: nextCheckIn })}
            </div>
            <Button
              onClick={runChecks}
              disabled={isRefreshing}
              variant="outline"
              className="gap-2 bg-white/5 border-white/10 hover:bg-white/10 h-10 px-4 rounded-xl transition-all"
            >
              <RefreshCw className={`w-4 h-4 ${isRefreshing ? 'animate-spin' : ''}`} />
              {isRefreshing ? uiI18n.t('app.running_73989d9c') : uiI18n.t('app.refresh_56e3badc')}
            </Button>
          </div>
        </div>
        <motion.div initial={{ opacity: 0, y: 12 }} animate={{ opacity: 1, y: 0 }} transition={{ duration: 0.5 }}>
          <GlassPanel className={`relative overflow-hidden border border-white/10 bg-gradient-to-br ${overallGradient} p-6 md:p-8 mb-6`}>
            <motion.div
              className="absolute inset-0 bg-gradient-to-r from-transparent via-white/5 to-transparent -skew-x-12"
              animate={{ x: ['-120%', '120%'] }}
              transition={{ duration: 4, repeat: Infinity, ease: 'linear' }}
            />
            <div className="relative z-10 flex flex-col md:flex-row items-center gap-6 md:gap-10">
              <HealthRing value={healthIndex} color={ringColor} />
              <div className="flex-1 text-center md:text-left">
                <div className="flex items-center justify-center md:justify-start gap-3 mb-2">
                  <StatusGlyph status={overall} className="w-7 h-7" />
                  <h1 className="font-display text-2xl md:text-4xl font-bold tracking-tight">{overallLabel}</h1>
                </div>
                <p className="text-muted-foreground flex items-center justify-center md:justify-start gap-2 text-sm">
                  <Activity className="w-4 h-4 text-primary" />
                  {uiI18n.t('app.tatakai_network_infrastructure_updated_c5762cba')} {formatDistanceToNow(lastChecked, { addSuffix: true })}
                </p>
                <div className="mt-4 flex flex-wrap items-center justify-center md:justify-start gap-2">
                  {(['operational', 'degraded', 'down'] as const).map((k) => (
                    <span key={k} className="inline-flex items-center gap-1.5 text-xs font-medium px-2.5 py-1 rounded-full bg-black/20 border border-white/10 capitalize">
                      <span className={`w-2 h-2 rounded-full ${dotClass(k)}`} />
                      {activeServices.filter((s) => s.status === k).length} {uiI18n.t(({"operational":"app.operational_7a65affc","degraded":"app.degraded_6dc24469","down":"app.down_77346d04"})[k])}
                    </span>
                  ))}
                </div>
              </div>
            </div>
          </GlassPanel>
        </motion.div>

        <div className="grid grid-cols-2 md:grid-cols-3 xl:grid-cols-5 gap-3 mb-10">
          {kpis.map((k) => (
            <GlassPanel key={k.label} className="p-4 border border-white/10 bg-white/5">
              <div className="flex items-center justify-between mb-2">
                <p className="text-[10px] uppercase tracking-[0.2em] text-muted-foreground">{k.label}</p>
                <span className="text-primary/60">{k.icon}</span>
              </div>
              <div className="text-2xl font-black font-mono tracking-tight">{k.value}</div>
              <p className="mt-1 text-[11px] text-muted-foreground truncate">{k.detail}</p>
            </GlassPanel>
          ))}
        </div>
        <div className="mb-12">
          <div className="flex items-center justify-between mb-6">
            <h2 className="font-display text-2xl font-bold flex items-center gap-3">
              <span className="p-2 rounded-lg bg-primary/10 text-primary"><Server className="w-6 h-6" /></span>
              {uiI18n.t('app.core_services_562880c0')}
            </h2>
            <span className="text-xs text-muted-foreground font-mono bg-white/5 px-3 py-1.5 rounded-full border border-white/10">
              {activeServices.filter((s) => s.status === 'operational').length}/{activeServices.length} {uiI18n.t('app.operational_7a65affc')}
            </span>
          </div>
          <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-5">
            {services.map((service, index) => (
              <motion.div
                key={service.name}
                initial={{ opacity: 0, y: 16 }}
                animate={{ opacity: 1, y: 0 }}
                transition={{ duration: 0.3, delay: index * 0.06 }}
              >
                <GlassPanel className="group p-5 border border-white/10 bg-white/5 hover:border-white/20 hover:-translate-y-1 transition-all duration-300 h-full">
                  <div className="flex items-start justify-between mb-4">
                    <div className="flex items-center gap-3">
                      <span className="p-2.5 rounded-xl bg-white/5 border border-white/10 text-primary">{service.icon}</span>
                      <div>
                        <h3 className="font-semibold">{service.name}</h3>
                        <p className="text-[11px] text-muted-foreground leading-snug">{service.description}</p>
                      </div>
                    </div>
                    {service.skipped ? <span className="text-xs text-muted-foreground">--</span> : <StatusGlyph status={service.status} />}
                  </div>
                  {service.providerId && (() => {
                    const h = getProviderHealth()[service.providerId];
                    if (!h || !h.downUntil || h.downUntil <= Date.now()) return null;
                    return (
                      <p className="mb-3 text-[11px] text-amber-400 leading-snug">
                        Skipped by the router until {new Date(h.downUntil).toLocaleTimeString(uiI18n.language, { hour: '2-digit', minute: '2-digit' })}
                        {h.lastError ? ` - ${h.lastError}` : ''}
                      </p>
                    );
                  })()}
                  <div className="mb-3"><UptimeBars history={uptimeHistory[service.name] || []} /></div>
                  <div className="flex items-center justify-between text-xs">
                    <span className="flex items-center gap-2">
                      <span className={`w-2 h-2 rounded-full ${service.skipped ? 'bg-white/20' : dotClass(service.status)}`} />
                      <span className="capitalize font-medium">{service.skipped ? 'Not configured' : service.status}</span>
                    </span>
                    <span className={`font-mono ${service.latency && service.latency > 500 ? 'text-amber-400' : 'text-emerald-400'}`}>
                      {service.latency !== undefined && service.latency > 0 ? uiI18n.t('app.i18n_value_0ms_0f3f2d06', { value0: service.latency }) : '--'}
                    </span>
                  </div>
                </GlassPanel>
              </motion.div>
            ))}
          </div>
        </div>
        <div className="mb-12">
          <div className="flex items-center justify-between mb-6">
            <h2 className="font-display text-2xl font-bold flex items-center gap-3">
              <span className="p-2 rounded-lg bg-primary/10 text-primary"><Globe className="w-6 h-6" /></span>
              {uiI18n.t('app.edge_routing_d1d75b9e')}
            </h2>
            <span className="text-xs text-muted-foreground font-mono bg-white/5 px-3 py-1.5 rounded-full border border-white/10">
              {proxies.length} {uiI18n.t('app.node_f8e966d1')}{proxies.length === 1 ? '' : 's'}
            </span>
          </div>
          <GlassPanel className="p-0 overflow-hidden border border-white/10 rounded-3xl">
            <div className="overflow-x-auto">
              <table className="w-full text-sm text-left border-collapse">
                <thead>
                  <tr className="bg-white/5 border-b border-white/10 text-[10px] uppercase tracking-wider text-muted-foreground">
                    <th className="px-6 py-4 font-bold">{uiI18n.t('app.node_260f7a8c')}</th>
                    <th className="px-6 py-4 font-bold">{uiI18n.t('app.type_3deb7456')}</th>
                    <th className="px-6 py-4 font-bold">{uiI18n.t('app.status_bae7d5be')}</th>
                    <th className="px-6 py-4 font-bold">{uiI18n.t('app.latency_3e399725')}</th>
                    <th className="px-6 py-4 font-bold">{uiI18n.t('app.weight_69c0b815')}</th>
                  </tr>
                </thead>
                <tbody className="divide-y divide-white/5">
                  <AnimatePresence>
                    {proxies.map((proxy) => (
                      <motion.tr key={proxy.id} initial={{ opacity: 0 }} animate={{ opacity: 1 }} exit={{ opacity: 0 }} className="hover:bg-white/5 transition-colors">
                        <td className="px-6 py-4 font-mono text-xs text-primary/80">{proxy.id}</td>
                        <td className="px-6 py-4"><span className="inline-flex px-2.5 py-1 rounded-lg text-[10px] font-bold bg-white/5 border border-white/10 uppercase">{proxy.type}</span></td>
                        <td className="px-6 py-4">
                          <span className="flex items-center gap-2">
                            <span className={`w-2 h-2 rounded-full ${dotClass(proxy.status)}`} />
                            <span className={`font-bold text-xs uppercase tracking-wide ${proxy.status === 'online' ? 'text-emerald-400' : proxy.status === 'degraded' ? 'text-amber-400' : 'text-red-500'}`}>{proxy.status}</span>
                          </span>
                        </td>
                        <td className="px-6 py-4 font-mono text-xs text-muted-foreground">{proxy.latencyMs > 0 ? uiI18n.t('app.i18n_value_0ms_0f3f2d06', { value0: proxy.latencyMs }) : '—'}</td>
                        <td className="px-6 py-4 font-mono text-xs text-muted-foreground">{proxy.score}</td>
                      </motion.tr>
                    ))}
                  </AnimatePresence>
                  {proxies.length === 0 && (
                    <tr>
                      <td colSpan={5} className="px-6 py-12 text-center text-sm text-muted-foreground">
                        {proxiesLoaded ? (
                          <span>No remote proxy configured. Streams use the in-app proxy.</span>
                        ) : (
                          <span className="inline-flex items-center gap-2">
                            <Loader2 className="w-4 h-4 animate-spin" />
                            {uiI18n.t('app.initializing_node_pool_f7b0ba32')}
                          </span>
                        )}
                      </td>
                    </tr>
                  )}
                </tbody>
              </table>
            </div>
          </GlassPanel>
        </div>
        <div className="mb-12">
        </div>
        <div className="grid grid-cols-1 lg:grid-cols-3 gap-6">
          <div className="lg:col-span-2">
            <div className="flex items-center justify-between mb-6">
              <h2 className="font-display text-2xl font-bold flex items-center gap-3">
                <span className="p-2 rounded-lg bg-primary/10 text-primary"><AlertTriangle className="w-6 h-6" /></span>
                {uiI18n.t('app.incidents_amp_maintenance_5b76da29')}
              </h2>
              {activeIncidents > 0 && (
                <span className="text-xs font-mono px-3 py-1.5 rounded-full border bg-red-500/10 border-red-500/30 text-red-400">
                  {activeIncidents} {uiI18n.t('app.active_2bb6b986')}
                </span>
              )}
            </div>
            {loadingIncidents ? (
              <GlassPanel className="p-12 border border-white/10 rounded-3xl flex items-center justify-center">
                <span className="inline-flex items-center gap-2 text-sm text-muted-foreground">
                  <Loader2 className="w-4 h-4 animate-spin" /> {uiI18n.t('app.loading_incident_feed_26f9e5a4')}
                </span>
              </GlassPanel>
            ) : incidents.length === 0 ? (
              <GlassPanel className="p-12 border border-white/10 rounded-3xl flex flex-col items-center justify-center text-center gap-3">
                <CheckCircle className="w-10 h-10 text-emerald-400" />
                <div>
                  <p className="font-semibold">{uiI18n.t('app.no_incidents_reported_3d09db9f')}</p>
                  <p className="text-sm text-muted-foreground">{uiI18n.t('app.all_infrastructure_has_been_running_smoothly_4f361b33')}</p>
                </div>
              </GlassPanel>
            ) : (
              <div className="space-y-4">
                {incidents.map((incident: any) => (
                  <motion.div key={incident.id} initial={{ opacity: 0, y: 12 }} animate={{ opacity: 1, y: 0 }}>
                    <GlassPanel className={`p-6 border rounded-3xl ${incident.is_active ? 'border-red-500/30 bg-red-500/[0.03]' : 'border-white/10'}`}>
                      <div className="flex flex-wrap items-center gap-2 mb-3">
                        <span className={`text-[10px] font-bold uppercase tracking-wider px-2.5 py-1 rounded-lg border ${SEVERITY_COLORS[incident.severity] || SEVERITY_COLORS.minor}`}>
                          {incident.severity || uiI18n.t('app.minor_fee63d97')}
                        </span>
                        <span className="text-[10px] font-medium uppercase tracking-wider px-2.5 py-1 rounded-lg bg-white/5 border border-white/10 text-muted-foreground capitalize">
                          {String(incident.status || 'investigating').replace(/_/g, ' ')}
                        </span>
                        {incident.is_active && (
                          <span className="inline-flex items-center gap-1.5 text-[10px] font-bold uppercase tracking-wider px-2.5 py-1 rounded-lg bg-red-500/15 border border-red-500/40 text-red-400">
                            <span className="w-1.5 h-1.5 rounded-full bg-red-400 animate-pulse" /> {uiI18n.t('app.live_65c821a5')}
                          </span>
                        )}
                      </div>
                      <h3 className="font-display text-lg font-bold mb-1">{incident.title}</h3>
                      {incident.description && (
                        <p className="text-sm text-muted-foreground leading-relaxed mb-4">{incident.description}</p>
                      )}
                      {Array.isArray(incident.affected_services) && incident.affected_services.length > 0 && (
                        <div className="flex flex-wrap gap-2 mb-4">
                          {incident.affected_services.map((svc: string) => (
                            <span key={svc} className="text-[11px] px-2.5 py-1 rounded-full bg-white/5 border border-white/10 text-muted-foreground">{svc}</span>
                          ))}
                        </div>
                      )}
                      {Array.isArray(incident.updates) && incident.updates.length > 0 && (
                        <div className="mt-4 pl-4 border-l-2 border-white/10 space-y-4">
                          {incident.updates.map((update: any) => (
                            <div key={update.id} className="relative">
                              <span className="absolute -left-[21px] top-1.5 w-2.5 h-2.5 rounded-full bg-primary border-2 border-background" />
                              <p className="text-xs font-semibold capitalize text-primary">{String(update.status || '').replace(/_/g, ' ')}</p>
                              <p className="text-sm text-muted-foreground leading-relaxed">{update.message}</p>
                              {update.created_at && (
                                <p className="text-[10px] text-muted-foreground/60 font-mono mt-0.5">
                                  {formatDistanceToNow(new Date(update.created_at), { addSuffix: true })}
                                </p>
                              )}
                            </div>
                          ))}
                        </div>
                      )}
                      <div className="mt-4 pt-4 border-t border-white/5 flex items-center gap-2 text-[11px] text-muted-foreground/60 font-mono">
                        <Clock className="w-3 h-3" />
                        {incident.is_active ? uiI18n.t('app.opened_c4f1f5b1') : uiI18n.t('app.resolved_d999aeb0')}{' '}
                        {formatDistanceToNow(new Date(incident.resolved_at || incident.created_at), { addSuffix: true })}
                      </div>
                    </GlassPanel>
                  </motion.div>
                ))}
              </div>
            )}
          </div>
          <div className="space-y-4">
            <h2 className="font-display text-2xl font-bold flex items-center gap-3 mb-2">
              <span className="p-2 rounded-lg bg-primary/10 text-primary"><Gauge className="w-6 h-6" /></span>
              {uiI18n.t('app.reliability_6780c750')}
            </h2>
            <GlassPanel className="p-5 border border-white/10 rounded-2xl">
              <div className="flex items-center justify-between mb-2">
                <p className="text-[11px] uppercase tracking-[0.2em] text-muted-foreground">{uiI18n.t('app.observed_uptime_b7bcc5a8')}</p>
                <Activity className="w-4 h-4 text-primary/60" />
              </div>
              <div className="text-3xl font-black font-mono mb-3">{uptimePct !== null ? uiI18n.t('app.i18n_value_0_cac4fa79', { value0: uptimePct }) : '--'}</div>
              <div className="h-2 rounded-full bg-white/10 overflow-hidden">
                <motion.div
                  className="h-full rounded-full bg-gradient-to-r from-emerald-400 to-primary"
                  initial={{ width: 0 }}
                  animate={{ width: `${uptimePct ?? 0}%` }}
                  transition={{ duration: 1, ease: 'easeOut' }}
                />
              </div>
              <p className="mt-2 text-[11px] text-muted-foreground">{uptimeSamples.length} {uiI18n.t('app.samples_this_session_7e8a0649')}</p>
            </GlassPanel>
            <div className="grid grid-cols-2 gap-4">
              <GlassPanel className="p-5 border border-white/10 rounded-2xl">
                <p className="text-[11px] uppercase tracking-[0.2em] text-muted-foreground mb-2">{uiI18n.t('app.mttr_b97d3367')}</p>
                <div className="text-2xl font-black font-mono">{mttrMinutes !== null ? uiI18n.t('app.i18n_value_0m_a5643c66', { value0: mttrMinutes }) : '--'}</div>
                <p className="mt-1 text-[11px] text-muted-foreground">{uiI18n.t('app.mean_recovery_d59ff579')}</p>
              </GlassPanel>
              <GlassPanel className="p-5 border border-white/10 rounded-2xl">
                <p className="text-[11px] uppercase tracking-[0.2em] text-muted-foreground mb-2">{uiI18n.t('app.check_freq_1071171a')}</p>
                <div className="text-2xl font-black font-mono">{SERVICE_CHECK_FREQ_SECONDS}s</div>
                <p className="mt-1 text-[11px] text-muted-foreground">{uiI18n.t('app.proxy_d799bae6')} {PROXY_POLL_FREQ_SECONDS}s</p>
              </GlassPanel>
            </div>
            <GlassPanel className="p-5 border border-white/10 rounded-2xl">
              <div className="flex items-center justify-between">
                <div>
                  <p className="text-[11px] uppercase tracking-[0.2em] text-muted-foreground mb-1">{uiI18n.t('app.last_pulse_6b3c273e')}</p>
                  <div className="text-lg font-bold font-mono">{lastChecked.toLocaleTimeString(uiI18n.language)}</div>
                </div>
                <Clock className={`w-8 h-8 ${isRefreshing ? 'text-primary animate-pulse' : 'text-primary/40'}`} />
              </div>
            </GlassPanel>
          </div>
        </div>
      </main>
      <MobileNav />
    </div>
  );
}
