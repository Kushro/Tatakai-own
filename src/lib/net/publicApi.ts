/**
 * publicApi.ts — the one transport for public third-party APIs (AniList,
 * Jikan, MAL, MangaDex, ani.zip, Kitsu, MangaUpdates).
 *
 * - Installed apps (Electron, Capacitor) go through native HTTP
 *   (`extensionFetch`: Electron main-process IPC / CapacitorHttp), so hosts
 *   without CORS (MAL, MangaDex, MangaUpdates) work and a 429 without CORS
 *   headers is still readable.
 * - A plain browser uses `fetch`; hosts without CORS will fail there. The fork
 *   targets the desktop and Android builds only.
 * - Every request is paced per host (hostLimiter) and retried on 429/5xx with
 *   Retry-After or exponential backoff.
 *
 * Never send app credentials (Supabase session, etc.) through here: callers
 * pass only what the public API itself needs (e.g. a user's AniList/MAL token).
 */

import { extensionFetch, type ExtensionResponse } from '@/core/extensions/remote/transport';
import { acquireSlot, adoptServerLimit, pauseHost } from '@/lib/net/hostLimiter';

export class PublicApiError extends Error {
  constructor(message: string, readonly status: number, readonly url: string, readonly body?: string) {
    super(message);
    this.name = 'PublicApiError';
  }
}

export interface PublicRequestInit {
  method?: 'GET' | 'POST' | 'PUT' | 'PATCH' | 'DELETE';
  headers?: Record<string, string>;
  /** String body; objects must be serialized by the caller. */
  body?: string;
  signal?: AbortSignal;
  /** Retries after the first attempt on 429/5xx/network errors (default 2). */
  retries?: number;
}

function isNativeApp(): boolean {
  if (typeof window === 'undefined') return false;
  const w = window as unknown as { electron?: { fetchExtensionResource?: unknown }; Capacitor?: { isNativePlatform?: () => boolean } };
  return Boolean(w.electron?.fetchExtensionResource) || Boolean(w.Capacitor?.isNativePlatform?.());
}

async function browserFetch(url: string, init: PublicRequestInit): Promise<ExtensionResponse> {
  const r = await fetch(url, {
    method: init.method || 'GET',
    headers: init.headers,
    body: init.body,
    credentials: 'omit',
    referrerPolicy: 'no-referrer',
    signal: init.signal || AbortSignal.timeout(15_000),
  });
  // Tolerate minimal Response-likes (test doubles returning only ok/status/json).
  const body = typeof r.text === 'function'
    ? await r.text()
    : typeof (r as Partial<Response>).json === 'function' ? JSON.stringify(await r.json()) : '';
  const raw: Record<string, string[]> = {};
  r.headers?.forEach?.((v, k) => { raw[k.toLowerCase()] = [v]; });
  const status = typeof r.status === 'number' ? r.status : r.ok ? 200 : 500;
  return {
    ok: r.ok ?? (status >= 200 && status < 300), status, statusText: r.statusText ?? '', url, rawHeaders: raw, cookies: {},
    headers: { get: (n) => raw[n.toLowerCase()]?.join(', ') ?? null },
    text: async () => body, json: async () => JSON.parse(body),
  };
}

function retryAfterMs(res: ExtensionResponse): number | null {
  const v = res.headers.get('retry-after');
  if (!v) return null;
  const secs = Number(v);
  if (Number.isFinite(secs)) return secs * 1000;
  const at = Date.parse(v);
  return Number.isFinite(at) ? Math.max(0, at - Date.now()) : null;
}

const sleep = (ms: number, signal?: AbortSignal) => new Promise<void>((resolve, reject) => {
  const t = setTimeout(resolve, ms);
  signal?.addEventListener('abort', () => { clearTimeout(t); reject(signal.reason); }, { once: true });
});

/**
 * Paced, retried request to a public API. Resolves with the final response
 * (any status); throws only on network failure after all retries or abort.
 */
export async function publicFetch(url: string, init: PublicRequestInit = {}): Promise<ExtensionResponse> {
  const host = new URL(url).hostname;
  const retries = init.retries ?? 2;
  let lastError: unknown;
  for (let attempt = 0; attempt <= retries; attempt++) {
    init.signal?.throwIfAborted();
    await acquireSlot(host, init.signal);
    let res: ExtensionResponse;
    try {
      res = isNativeApp()
        ? await extensionFetch(url, { method: init.method || 'GET', headers: init.headers, body: init.body, signal: init.signal })
        : await browserFetch(url, init);
    } catch (err) {
      if (init.signal?.aborted) throw err;
      lastError = err;
      if (attempt < retries) await sleep(500 * 2 ** attempt, init.signal);
      continue;
    }
    adoptServerLimit(host, Number(res.headers.get('x-ratelimit-limit')));
    if (res.status === 429 || res.status >= 500) {
      const wait = retryAfterMs(res) ?? 1000 * 2 ** attempt;
      if (res.status === 429) pauseHost(host, wait);
      if (attempt < retries) { await sleep(Math.min(wait, 60_000), init.signal); continue; }
    }
    return res;
  }
  throw lastError instanceof Error ? lastError : new Error(`Request to ${host} failed`);
}

/** `publicFetch` + JSON parse; throws PublicApiError on non-2xx. */
export async function publicJson<T>(url: string, init: PublicRequestInit = {}): Promise<T> {
  const res = await publicFetch(url, {
    ...init,
    headers: { Accept: 'application/json', ...(init.headers || {}) },
  });
  const text = await res.text();
  if (!res.ok) throw new PublicApiError(`${new URL(url).hostname} ${res.status}`, res.status, url, text.slice(0, 500));
  try {
    return JSON.parse(text) as T;
  } catch {
    throw new PublicApiError(`${new URL(url).hostname} returned invalid JSON`, res.status, url, text.slice(0, 200));
  }
}
