import { getActiveStreamingProxySnapshot } from '@/hooks/user/useProxySettings';
import { publicFetch } from '@/lib/net/publicApi';
import { isMobileProxyUrl } from '@/core/extensions/mobile/mobileProxy';
import { downgradePosterForDataSaver, isDataSaverEnabled } from '@/lib/mobile/dataSaver';

const STREAM_PROXY_PASSWORD = String(
  import.meta.env.VITE_STREAM_PROXY_PASSWORD || import.meta.env.VITE_PROXY_PASSWORD || ''
).trim();

// Prefer the configured proxy; the default is the in-app loopback proxy the
// desktop/mobile runtime already provides. There is no hosted proxy in this fork.
const STREAM_PROXY_BASE = String(
  import.meta.env.VITE_STREAM_PROXY_URL ||
    import.meta.env.VITE_PROXY_NODE_URL ||
    import.meta.env.VITE_PROXY_CF_URL ||
    'http://127.0.0.1:8099'
).trim();

function isLocalLike(url: string): boolean {
  return (
    url.startsWith('asset://') ||
    url.startsWith('file://') ||
    url.startsWith('blob:') ||
    url.startsWith('data:') ||
    url.includes('asset.localhost') ||
    url.includes('127.0.0.1') ||
    url.includes('localhost:')
  );
}

function unwrapProxyUrl(rawUrl: string): string {
  let current = String(rawUrl || '').trim();

  for (let index = 0; index < 3; index += 1) {
    const snapshot = readProxyQuerySnapshot(current);
    const nestedUrl = String(snapshot.streamUrl || '').trim();
    if (!nestedUrl) break;
    current = nestedUrl;
  }

  return current;
}

export function getProxiedImageUrl(rawUrl?: string): string {
  const url = String(rawUrl || '').trim();
  if (!url) return '/placeholder.svg';
  return url;
}

/**
 * Get the highest quality image URL available
 * Tries to upgrade to large_image_url if possible
 * Used for trending, spotlight, and hero sections
 */
export function getHighQualityImage(rawUrl?: string, _anilistId?: number): string {
  const url = String(rawUrl || '').trim();
  if (!url) return '/placeholder.svg';

  // Data-saver: serve the medium variant instead of upgrading to large.
  try {
    if (isDataSaverEnabled()) return downgradePosterForDataSaver(url);
  } catch {
    /* storage unavailable — fall through to the upgrade path */
  }

  // Upgrade to larger versions if available
  return url
    .replace('/cover/small/', '/cover/large/')
    .replace('/cover/medium/', '/cover/large/')
    .replace('/banner/small/', '/banner/large/')
    .replace('/banner/medium/', '/banner/large/')
    // Also try to upgrade Jikan URLs to webp large format
    .replace(/\/jpg\/large_image_url/, '/webp/large_image_url')
    .replace(/image_url$/, 'large_image_url');
}

/**
 * Get high-quality poster specifically - prefers large images
 * Used for cards where we show the cover art
 */
export function getHighQualityPoster(rawUrl?: string, anilistId?: number): string {
  const url = String(rawUrl || '').trim();
  if (!url) return '/placeholder.svg';
  try {
    if (isDataSaverEnabled()) return downgradePosterForDataSaver(url);
  } catch {
    /* storage unavailable — fall through to the upgrade path */
  }
  return url
    .replace('/cover/medium/', '/cover/large/')
    .replace('/banner/small/', '/banner/large/')
    .replace('/banner/medium/', '/banner/large/');
}

// Debrid CDN links are IP-pinned/expiring: the proxy's IP != the device IP, so
// wrapping them returns 403 and leaks the token via the proxy logs. Play direct.
const DEBRID_DIRECT_HOST_SUFFIXES = [
  'real-debrid.com',
  'torbox.app',
  'torboxcdn.com',
];

export function isDebridDirectUrl(value: string): boolean {
  try {
    const host = new URL(String(value || '')).hostname.toLowerCase();
    return DEBRID_DIRECT_HOST_SUFFIXES.some((h) => host === h || host.endsWith(`.${h}`));
  } catch {
    return false;
  }
}

function buildProxyUrl(rawUrl: string, referer?: string, userAgent?: string, type: 'video' | 'subtitle' = 'video'): string {
  const url = unwrapProxyUrl(rawUrl);
  // Already the in-app proxy (mobile token, desktop loopback): wrapping it in
  // the remote proxy would strip the replay headers that make it work.
  if (isMobileProxyUrl(url)) return url;
  if (!url || isLocalLike(url) || !/^https?:/i.test(url)) return url;
  // Debrid direct links must never go through the shared proxy.
  if (type === 'video' && isDebridDirectUrl(url)) return url;

  const snapshot = getActiveStreamingProxySnapshot();
  const base = snapshot.url || STREAM_PROXY_BASE;
  const password = snapshot.password !== undefined ? snapshot.password : STREAM_PROXY_PASSWORD;

  const params = new URLSearchParams({ url, type });
  if (referer) params.set('referer', referer);
  if (userAgent) params.set('userAgent', userAgent);
  if (password) params.set('password', password);
  return `${base}?${params.toString()}`;
}

export function getProxiedVideoUrl(rawUrl: string, referer?: string, userAgent?: string): string {
  return buildProxyUrl(rawUrl, referer, userAgent, 'video');
}

export function getProxiedSubtitleUrl(rawUrl: string, referer?: string, userAgent?: string): string {
  const url = unwrapProxyUrl(rawUrl);
  // In-app proxy tracks resolve natively with header replay (see
  // MobileVideoPlayer); routing them through the backend would lose the token.
  if (isMobileProxyUrl(url)) return url;
  if (!url || isLocalLike(url) || !/^https?:/i.test(url)) return url;

  // No hosted subtitle proxy: native shells fetch subtitles through the
  // native HTTP stack (fetchViaChain), the web build uses the plain URL.
  return url;
}

/**
 * Public JSON APIs (waifu.im / nekosia.cat) are fetched directly. There is no
 * hosted passthrough in this fork; `fetchJsonWithFallback` goes through the
 * paced native transport, so this just returns the URL unchanged.
 */
export function getProxiedJsonUrl(rawUrl: string): string {
  return String(rawUrl || '').trim();
}

/**
 * Hosts that send `Access-Control-Allow-Origin: *` (verified: nekosia.cat,
 * nekos.best). The shared backend IP is rate-limited by these upstreams
 * (all users share one IP → frequent upstream 403s through /api/proxy/json),
 * while a direct browser fetch carries the user's own IP and succeeds.
 * Try direct first, fall back to the proxy (e.g. desktop renderer CORS edge).
 */
const DIRECT_FIRST_HOSTS = new Set(['api.nekosia.cat', 'nekos.best']);

export function shouldFetchJsonDirectFirst(rawUrl: string): boolean {
  try {
    return DIRECT_FIRST_HOSTS.has(new URL(String(rawUrl || '')).hostname.toLowerCase());
  } catch {
    return false;
  }
}

/**
 * GET a public JSON API. Uses the native HTTP transport on Electron/Capacitor
 * (no CORS) and a plain fetch in the browser, paced per host.
 */
export async function fetchJsonWithFallback(rawUrl: string, init?: RequestInit): Promise<Response> {
  const url = String(rawUrl || '').trim();
  const headers: Record<string, string> = { Accept: 'application/json' };
  if (init?.headers) new Headers(init.headers).forEach((v, k) => { headers[k] = v; });
  const res = await publicFetch(url, { headers, signal: init?.signal ?? undefined });
  const body = await res.text();
  return new Response(body, { status: res.status, statusText: res.statusText });
}

export function readProxyQuerySnapshot(rawUrl: string): {
  streamUrl?: string;
  referer?: string;
  userAgent?: string;
  password?: string;
} {
  try {
    const parsed = new URL(rawUrl, window.location.origin);
    return {
      streamUrl: parsed.searchParams.get('url') || undefined,
      referer: parsed.searchParams.get('referer') || undefined,
      userAgent: parsed.searchParams.get('userAgent') || undefined,
      password: parsed.searchParams.get('password') || undefined,
    };
  } catch {
    return {};
  }
}

