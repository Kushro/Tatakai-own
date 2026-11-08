import { Capacitor, CapacitorHttp } from '@capacitor/core';
import uiI18n from '@/lib/uiI18n';
import { randomUUID } from '../../../lib/uuid';
import { EXTENSION_INSTALLED_EVENT } from '../ecosystems';
import type { WatchExtensionBatch } from '../compatibility/watch-batches';
import type { CustomSourceEntry, CustomHomeData, CustomSearchData, CustomInfoData, CustomWatchData, CustomReadData } from '@/core/content/custom-source-runtime';
import type { MangaReadResponse } from '@/types/manga';
import type { NovelContentResponse } from '@/types/novel';
import { assertAllowedServerUrl, isAllowedServerUrl } from './hostGuard';
import { normalizeServerOrigin, PAIRING_CODE_PATTERN, type ServerLink } from './pairingLink';

export { isAllowedServerUrl, assertAllowedServerUrl, isLocalNetworkHost } from './hostGuard';
export { parseServerLink, parsePairingInput, normalizeServerOrigin, PAIRING_CODE_PATTERN, type ServerLink } from './pairingLink';

/*
 * Client of the extension server built into Tatakai for Windows/Linux (and the
 * standalone `server/` Node build). Contract: docs/redesign/13-integrated-server-contract.md.
 *
 * Transport (D6, §3.9): every request passes the host guard first (http only for
 * local-network hosts). Web and Electron use `fetch`; on Capacitor the WebView
 * origin is https://tatakai.me, so a fetch to http://192.168.… would be blocked
 * mixed content: requests go through the native HTTP client (CapacitorHttp)
 * instead. The native client cannot stream, so `/resolve` asks for the
 * aggregated JSON result there (D6a) while web/Electron read NDJSON snapshots.
 */

const API = '/api/server/v1';
const REQUEST_TIMEOUT_MS = 30000;
// The server caps a resolve at 25 s; the aggregated JSON answer needs a little slack.
const AGGREGATED_RESOLVE_TIMEOUT_MS = 35000;

export type ExtensionServerErrorCode = 'insecure_host' | 'invalid_link' | 'invalid_url' | 'pairing_expired' | 'pairing_required'
  | 'invalid_password' | 'authentication_required' | 'login_throttled' | 'setup_required' | 'incompatible' | 'unreachable' | 'server_error';
const KNOWN_CODES = new Set<string>(['insecure_host', 'invalid_link', 'invalid_url', 'pairing_expired', 'pairing_required', 'invalid_password',
  'authentication_required', 'login_throttled', 'setup_required', 'incompatible', 'unreachable']);

/** Localized copy for an extension-server error code (`extensionServerClient.errors.*`). */
export function describeServerError(code: ExtensionServerErrorCode, details: { host?: string; status?: number } = {}): string {
  switch (code) {
    case 'insecure_host': return uiI18n.t('extensionServerClient.errors.insecureHost', { host: details.host || '?' });
    case 'invalid_link': return uiI18n.t('extensionServerClient.errors.invalidLink');
    case 'invalid_url': return uiI18n.t('extensionServerClient.errors.invalidUrl');
    case 'pairing_expired': return uiI18n.t('extensionServerClient.errors.expiredLink');
    case 'pairing_required': return uiI18n.t('extensionServerClient.errors.pairingRequired');
    case 'invalid_password': return uiI18n.t('extensionServerClient.errors.wrongPassword');
    case 'authentication_required': return uiI18n.t('extensionServerClient.errors.revoked');
    case 'login_throttled': return uiI18n.t('extensionServerClient.errors.throttled');
    case 'setup_required': return uiI18n.t('extensionServerClient.errors.setupRequired');
    case 'incompatible': return uiI18n.t('extensionServerClient.errors.incompatible');
    case 'unreachable': return uiI18n.t('extensionServerClient.errors.unreachable');
    default: return uiI18n.t('extensionServerClient.errors.requestFailed', { status: details.status ?? '?' });
  }
}

/** Error with a stable `code` and a localized, user-facing `message`. */
export class ExtensionServerError extends Error {
  readonly code: ExtensionServerErrorCode;
  constructor(code: ExtensionServerErrorCode, message?: string, details: { host?: string; status?: number } = {}) {
    super(message || describeServerError(code, details));
    this.name = 'ExtensionServerError';
    this.code = code;
  }
}

function hostOf(url?: string): string | undefined {
  try { return url ? new URL(url).host : undefined; } catch { return url; }
}

/** Normalize anything thrown while talking to a server into an {@link ExtensionServerError}. */
export function toServerError(error: unknown, url?: string): ExtensionServerError {
  if (error instanceof ExtensionServerError) return error;
  const code = error instanceof Error ? error.message : '';
  if (KNOWN_CODES.has(code)) return new ExtensionServerError(code as ExtensionServerErrorCode, undefined, { host: hostOf(url) });
  return new ExtensionServerError('unreachable');
}

const KEY = 'tatakai_extension_servers_v1';
export interface ServerConnection { id: string; serverId: string; url: string; name: string; token: string; enabled: boolean; logoUrl?: string }
export interface ServerSource { id: string; extensionId: string; name: string; ecosystem: string; kind: 'watch' | 'read' | 'novel'; enabled: boolean; available: boolean; reason: string; languages: string[] }
export interface ServerHealth { name: string; serverId: string; protocolVersion: number; setupRequired?: boolean; pairingRequired?: boolean; dashboardPath?: string; logoUrl?: string; uptimeSeconds?: number }
export function listServers(): ServerConnection[] {
  try { const rows = JSON.parse(localStorage.getItem(KEY) || '[]'); return Array.isArray(rows) ? rows.filter(row => row && typeof row.url === 'string' && typeof row.token === 'string') : []; } catch { return []; }
}
export function saveServers(rows: ServerConnection[]) { localStorage.setItem(KEY, JSON.stringify(rows)); window.dispatchEvent(new Event(EXTENSION_INSTALLED_EVENT)); }
/** Validate a typed server address and return its origin. Throws {@link ExtensionServerError}. */
export function normalizeServerUrl(value: string): string {
  try { return normalizeServerOrigin(value, { allowDashboardPath: true }); } catch (error) { throw toServerError(error, value); }
}

// ── Transport ────────────────────────────────────────────────────────────────

interface RawResponse { status: number; text: string }
interface RawRequest { method: string; headers: Record<string, string>; body?: unknown; signal?: AbortSignal; timeoutMs: number }
type ElectronResourceFetch = (payload: { url: string; method: string; headers: Record<string, string>; body?: string }) => Promise<{ status: number; body: unknown }>;

function nativeBridge(): boolean {
  try { return Capacitor.isNativePlatform(); } catch { return false; }
}
/**
 * Electron's renderer runs on a secure custom scheme, so plain-http LAN servers
 * would be mixed content there too: use the main-process resource fetch for them.
 */
function electronBridge(url: string): ElectronResourceFetch | null {
  const bridge = (window as Window & { electron?: { fetchExtensionResource?: ElectronResourceFetch } }).electron?.fetchExtensionResource;
  if (typeof bridge !== 'function' || nativeBridge() || location.protocol === 'http:') return null;
  try { return new URL(url).protocol === 'http:' ? bridge : null; } catch { return null; }
}
/** Whether NDJSON progressive results can be read for this server (plain `fetch` only). */
function canStream(url: string): boolean { return !nativeBridge() && !electronBridge(url); }

function deadline(signal: AbortSignal | undefined, timeoutMs: number): AbortSignal {
  return signal ? AbortSignal.any([signal, AbortSignal.timeout(timeoutMs)]) : AbortSignal.timeout(timeoutMs);
}
/** Native clients have no cancellation API: race them against the abort/timeout signal. */
function raceSignal<T>(promise: Promise<T>, signal: AbortSignal): Promise<T> {
  if (signal.aborted) return Promise.reject(signal.reason);
  return new Promise<T>((resolve, reject) => {
    const onAbort = () => reject(signal.reason);
    signal.addEventListener('abort', onAbort, { once: true });
    promise.then(resolve, reject).finally(() => signal.removeEventListener('abort', onAbort));
  });
}

async function rawRequest(url: string, init: RawRequest): Promise<RawResponse> {
  assertAllowedServerUrl(url);
  const signal = deadline(init.signal, init.timeoutMs);
  if (nativeBridge()) {
    const response = await raceSignal(CapacitorHttp.request({ url, method: init.method, headers: init.headers, ...(init.body === undefined ? {} : { data: init.body }),
      responseType: 'text', connectTimeout: 8000, readTimeout: init.timeoutMs }), signal);
    return { status: Number(response.status) || 0, text: typeof response.data === 'string' ? response.data : JSON.stringify(response.data ?? '') };
  }
  const bridge = electronBridge(url);
  if (bridge) {
    const response = await raceSignal(bridge({ url, method: init.method, headers: init.headers, body: init.body === undefined ? undefined : JSON.stringify(init.body) }), signal);
    return { status: Number(response.status) || 0, text: typeof response.body === 'string' ? response.body : JSON.stringify(response.body ?? '') };
  }
  const response = await fetch(url, { method: init.method, headers: init.headers, body: init.body === undefined ? undefined : JSON.stringify(init.body),
    signal, credentials: 'omit', referrerPolicy: 'no-referrer' });
  return { status: response.status, text: await response.text() };
}

// Parses a `{ success, data, error }` envelope; tolerates non-JSON bodies.
function readApiResponse(response: RawResponse): { success: boolean; data?: any; error?: { code?: string; message?: string } } {
  try {
    const body = JSON.parse(response.text);
    if (body && typeof body === 'object' && typeof body.success === 'boolean') return body;
    return { success: response.status >= 200 && response.status < 300, data: body };
  } catch {
    return { success: false, error: { message: describeServerError('server_error', { status: response.status }) } };
  }
}

function authHeaders(server: Pick<ServerConnection, 'token'>, accept = 'application/json'): Record<string, string> {
  return { 'content-type': 'application/json', accept, ...(server.token ? { authorization: `Bearer ${server.token}` } : {}) };
}

function failure(path: string, status: number, error?: { code?: string; message?: string }): ExtensionServerError {
  const code = error?.code;
  if (status === 401) {
    if (path === '/auth/login') return new ExtensionServerError(code === 'pairing_required' ? 'pairing_required' : 'invalid_password');
    if (path === '/auth/pair') return new ExtensionServerError('pairing_expired');
    return new ExtensionServerError('authentication_required');
  }
  if (code && ['login_throttled', 'setup_required', 'pairing_expired', 'pairing_required'].includes(code)) return new ExtensionServerError(code as ExtensionServerErrorCode);
  return new ExtensionServerError('server_error', error?.message || undefined, { status });
}

export async function serverRequest<T = any>(server: Pick<ServerConnection, 'url' | 'token'>, path: string, data?: unknown, signal?: AbortSignal,
  options: { timeoutMs?: number } = {}): Promise<T> {
  const url = server.url + API + path;
  let response: RawResponse;
  try {
    response = await rawRequest(url, { method: data === undefined ? 'GET' : 'POST', headers: authHeaders(server), body: data, signal, timeoutMs: options.timeoutMs ?? REQUEST_TIMEOUT_MS });
  } catch (error) {
    if (signal?.aborted) throw error;
    throw toServerError(error, url);
  }
  const result = readApiResponse(response);
  if (response.status < 200 || response.status >= 300 || !result.success) throw failure(path, response.status, result.error);
  return result.data;
}

/** Device label shown in the PC's device list. */
function clientLabel(): string {
  if (nativeBridge()) { try { return Capacitor.getPlatform() === 'ios' ? 'Tatakai (iOS)' : 'Tatakai (Android)'; } catch { return 'Tatakai (mobile)'; } }
  return (window as Window & { electron?: unknown }).electron ? 'Tatakai (desktop)' : 'Tatakai (web)';
}

/** Connect with a one-time pairing `code`, or with the viewer `password` (empty when the server allows it). */
export async function connectServer(urlInput: string, password: string, code?: string) {
  const url = normalizeServerUrl(urlInput), provisional = { url, token: '' };
  if (code && !PAIRING_CODE_PATTERN.test(code)) throw new ExtensionServerError('invalid_link');
  const health = await serverRequest<ServerHealth>(provisional, '/health');
  if (health?.protocolVersion !== 1) throw new ExtensionServerError('incompatible');
  if (health.setupRequired) throw new ExtensionServerError('setup_required');
  const label = clientLabel();
  const auth = await serverRequest(provisional, code ? '/auth/pair' : '/auth/login', code ? { code, label } : { password, role: 'viewer', label });
  const previous = listServers(), existing = previous.find(row => row.serverId === health.serverId && row.url === url);
  const connection: ServerConnection = { id: existing?.id || randomUUID(), serverId: health.serverId, name: existing?.name || health.name, url, token: auth.token, enabled: true, logoUrl: health.logoUrl || undefined };
  saveServers([...previous.filter(row => row.id !== connection.id), connection]);
  return connection;
}
/** Connect from a parsed pairing / connection link (see `parseServerLink`). */
export function connectServerLink(link: ServerLink, password = '') {
  return connectServer(link.server, link.password ?? password, link.code);
}
export function setServerEnabled(id: string, enabled: boolean) {
  const rows = listServers().map(row => row.id === id ? { ...row, enabled } : row); saveServers(rows); return rows;
}
/** Forget a server on this device; the token is revoked on the PC when it answers (best effort). */
export async function removeServer(id: string) {
  const server = listServers().find(row => row.id === id);
  if (server) await serverRequest(server, '/auth/logout', {}, undefined, { timeoutMs: 5000 }).catch(() => {});
  const rows = listServers().filter(row => row.id !== id); saveServers(rows); return rows;
}
export function connectionFor(namespace: string) {
  const server = listServers().find(row => `server-${row.id}` === namespace && row.enabled);
  if (!server) throw Error('This server is disabled or disconnected'); return server;
}
export function serverInvoke(namespace: string, sourceId: string, op: string, args: Record<string, unknown>, signal?: AbortSignal) {
  return serverRequest(connectionFor(namespace), '/invoke', { sourceId, op, args, progressive: ['chapters','novelChapters'].includes(op) }, signal);
}

// ── Media guard ──────────────────────────────────────────────────────────────

/**
 * Media URLs returned by a server must pass the host guard; plain-http ones must
 * also share the saved server's origin (its `/api/server/v1/media/*` proxy).
 * Non-http values (magnet links, data URIs, empty) are left to the player.
 */
export function isAllowedServerMediaUrl(server: Pick<ServerConnection, 'url'>, value: unknown): boolean {
  if (typeof value !== 'string' || !/^https?:/i.test(value)) return true;
  try {
    const url = new URL(value);
    if (url.protocol === 'https:') return true;
    return url.origin === new URL(server.url).origin && isAllowedServerUrl(url.href);
  } catch { return false; }
}
function guardSources<T extends Record<string, any>>(server: Pick<ServerConnection, 'url'>, rows: T[] | undefined): T[] {
  return (Array.isArray(rows) ? rows : []).filter(row => row && isAllowedServerMediaUrl(server, row.url ?? row.file));
}

export async function resolveServerSources(options: Record<string, unknown>, signal?: AbortSignal, progress?: (batch: WatchExtensionBatch) => void): Promise<WatchExtensionBatch> {
  const batches = new Map<string, WatchExtensionBatch>();
  const merged = (): WatchExtensionBatch => ({ sources: [...batches.values()].flatMap(row => row.sources), subtitles: [...batches.values()].flatMap(row => row.subtitles), statuses: [...batches.values()].flatMap(row => row.statuses) });
  await Promise.all(listServers().filter(row => row.enabled).map(async server => {
    // Each NDJSON line (and the aggregated JSON answer) is a full snapshot, not a delta.
    const publish = (batch: WatchExtensionBatch) => {
      batches.set(server.id, {
        sources: guardSources(server, batch?.sources).map(source => ({ ...source, extensionServerName: server.name,
          sourceName: source.sourceName || source.source || source.server || source.providerName,
          extensionName: source.extensionName || source.providerName })),
        subtitles: guardSources(server, batch?.subtitles),
        statuses: Array.isArray(batch?.statuses) ? batch.statuses : [],
      });
      progress?.(merged());
    };
    try {
      if (!canStream(server.url)) {
        // Capacitor / Electron bridge: aggregated JSON result (D6a).
        publish(await serverRequest<WatchExtensionBatch>(server, '/resolve', options, signal, { timeoutMs: AGGREGATED_RESOLVE_TIMEOUT_MS }));
        return;
      }
      const url = server.url + API + '/resolve';
      try { assertAllowedServerUrl(url); } catch (error) { throw toServerError(error, url); }
      let response: Response;
      try {
        response = await fetch(url, { method: 'POST', headers: authHeaders(server, 'application/x-ndjson'), body: JSON.stringify(options), signal: deadline(signal, REQUEST_TIMEOUT_MS), credentials: 'omit', referrerPolicy: 'no-referrer' });
      } catch (error) { if (signal?.aborted) throw error; throw toServerError(error, url); }
      if (!response.ok) throw failure('/resolve', response.status, readApiResponse({ status: response.status, text: await response.text().catch(() => '') }).error);
      if (!response.body) { publish(readApiResponse({ status: response.status, text: await response.text() }).data); return; }
      const reader = response.body.getReader(), decoder = new TextDecoder(); let pending = '';
      try { while (true) { const { value, done } = await reader.read(); pending += decoder.decode(value, { stream: !done }); if (pending.length > 8 * 1024 * 1024) throw Error('Server response exceeded limit'); const lines = pending.split('\n'); pending = lines.pop()!; for (const line of lines) if (line) {
        publish(JSON.parse(line) as WatchExtensionBatch);
      } if (done) break; } }
      finally { await reader.cancel().catch(() => {}); reader.releaseLock(); }
    } catch (error) {
      if (signal?.aborted) return;
      // An unreachable PC never blocks playback: local extensions keep resolving.
      const failureCode = error instanceof ExtensionServerError ? error.code : 'unreachable';
      const message = failureCode === 'unreachable' ? uiI18n.t('extensionServerClient.offlineFallback') : error instanceof Error ? error.message : uiI18n.t('extensionServerClient.offlineFallback');
      const previous = batches.get(server.id) || { sources: [], subtitles: [], statuses: [] };
      batches.set(server.id, { ...previous, statuses: [...previous.statuses, { provider: 'server-' + server.id, name: server.name, ecosystem: 'tatakai', kind: 'anime', status: 'error', count: 0, durationMs: 0, error: message }] });
      progress?.(merged());
    }
  }));
  signal?.throwIfAborted(); return merged();
}
export async function serverCustomSources(): Promise<CustomSourceEntry[]> {
  const results = await Promise.allSettled(listServers().filter(row => row.enabled).map(async server => {
    const sources = await serverRequest<ServerSource[]>(server, '/sources');
    return sources.filter(source => source.available && source.kind !== 'novel').map(source => ({ namespace: `server-${server.id}`, id: source.id,
      extensionId: source.extensionId, name: `${source.name} · ${server.name}`, kind: source.kind, description: source.ecosystem, enabled: true } as CustomSourceEntry));
  }));
  return results.flatMap(result => result.status === 'fulfilled' ? result.value : []);
}
const cards = (items: any[]) => items.map(item => ({ ...item, id: String(item.id || item.url), title: item.title || item.name, image: item.image || item.poster }));
export async function serverCustomSearch(namespace: string, sourceId: string, query: string, page: number, signal?: AbortSignal): Promise<CustomSearchData> {
  const result = await serverInvoke(namespace, sourceId, query ? 'search' : 'browse', { query, page, skip: (page - 1) * 100 }, signal);
  return { results: cards(Array.isArray(result) ? result : result.results || result.metas || []), hasNextPage: !!result.hasNextPage };
}
export async function serverCustomHome(namespace: string, sourceId: string, page: number, signal?: AbortSignal): Promise<CustomHomeData> {
  const result = await serverCustomSearch(namespace, sourceId, '', page, signal); return { sections: [{ title: 'Browse', items: result.results }] };
}
export async function serverCustomInfo(namespace: string, sourceId: string, id: string, signal?: AbortSignal): Promise<CustomInfoData | null> {
  const result = await serverInvoke(namespace, sourceId, 'info', { id, url: id }, signal);
  if (!result) return null;
  return { ...result, id: String(result.id || id), title: result.title || result.name, image: result.image || result.poster,
    entries: (result.entries || result.videos || result.episodes || result.chapters || []).map((entry: any) => ({ ...entry, id: String(entry.id || entry.url), title: entry.title || entry.name, label: entry.label || entry.title || entry.name || `Entry ${entry.number || entry.episode || ''}`, number: entry.number ?? entry.episode })) };
}
export async function serverCustomWatch(namespace: string, sourceId: string, id: string, episodeId: string): Promise<CustomWatchData> {
  const result = await serverInvoke(namespace, sourceId, 'resolve', { id: episodeId || id, url: episodeId || id });
  return { sources: guardSources(connectionFor(namespace), Array.isArray(result) ? result : result?.sources) };
}
export async function serverCustomRead(namespace: string, sourceId: string, chapterId: string): Promise<CustomReadData> {
  let detail: Record<string, unknown> = {};
  try { const decoded = JSON.parse(chapterId); if (decoded && typeof decoded.chapterKey === 'string') detail = decoded; } catch { /* Ordinary source chapter IDs stay unchanged. */ }
  const result = await serverInvoke(namespace, sourceId, 'pages', { id: chapterId, chapterId, url: chapterId, ...detail });
  const server = connectionFor(namespace);
  return { pages: (Array.isArray(result) ? result : result.pages || []).map((page: any) => typeof page === 'string' ? page : page.imageUrl || page.url)
    .filter((imageUrl: unknown) => isAllowedServerMediaUrl(server, imageUrl)).map((imageUrl: string, index: number) => ({ pageNumber: index + 1, imageUrl })) };
}
export async function serverMappedChapters(params: { anilistId?: number; title?: string; titles?: string[] }) {
  const sources = (await serverCustomSources()).filter(source => source.kind === 'read');
  const chapters: any[] = [], failedProviders: string[] = []; let pending = false;
  await Promise.all(sources.map(async source => {
    const provider = `${source.namespace}|${source.id}`;
    try {
      let result: any;
      if (source.description === 'tatakai') result = await serverInvoke(source.namespace, source.id, 'chapters', params);
      else {
      const search = await serverCustomSearch(source.namespace, source.id, params.title || params.titles?.[0] || '', 1);
      const normalize = (value: string) => value.toLowerCase().replace(/[^\p{L}\p{N}]/gu, '');
      const titles = [params.title, ...(params.titles || [])].filter(Boolean).map(value => normalize(value!));
      const matches = search.results.filter(item => titles.includes(normalize(item.title)));
      if (matches.length !== 1) throw Error('Select the title from the server catalog to read');
      result = await serverInvoke(source.namespace, source.id, 'chapters', { id: matches[0].id, url: matches[0].id });
      }
      pending ||= result?.partial === true;
      for (const chapter of Array.isArray(result) ? result : result.entries || result.chapters || []) {
        const number = Number(chapter.number ?? chapter.chapter), id = String(chapter.id || chapter.url || '');
        if (!Number.isFinite(number)) continue;
        if (Array.isArray(chapter.sources) && chapter.sources.length) {
          chapters.push({ number, title: chapter.title, sources: chapter.sources.map((entry: any) => {
            const chapterId = JSON.stringify({ chapterKey: entry.chapterKey, provider: entry.provider, providerChapterId: entry.providerChapterId, anilistId: params.anilistId });
            // Keep each bundled provider selectable while routing pages through
            // the same server source (the first two pipe-separated segments).
            const sourceProvider = entry.provider ? `${provider}|${encodeURIComponent(entry.provider)}` : provider;
            return { ...entry, provider: sourceProvider, providerName: `${entry.providerName || entry.provider || source.name} · ${source.name}`, chapterKey: `${sourceProvider}:${chapterId}`, providerChapterId: chapterId };
          }) }); continue;
        }
        if (!id) continue;
        chapters.push({ number, title: chapter.label || chapter.title || chapter.name, sources: [{ provider, providerName: source.name, chapterKey: `${provider}:${id}`, providerChapterId: id, language: null, scanlator: null, releaseDate: null }] });
      }
    } catch { failedProviders.push(provider); }
  }));
  return { chapters, failedProviders, pending, providersAvailable: sources.map(source => `${source.namespace}|${source.id}`) };
}
export async function serverMappedPages(params: { extensionId: string; chapterKey: string; providerChapterId?: string; anilistId?: number }): Promise<MangaReadResponse> {
  const [namespace, sourceId] = params.extensionId.split('|');
  const chapterId = params.providerChapterId || params.chapterKey.slice(params.extensionId.length + 1);
  const result = await serverCustomRead(namespace, sourceId, chapterId);
  return { success: result.pages.length > 0, data: { pages: result.pages.map(page => ({ ...page, proxiedImageUrl: page.imageUrl, width: null, height: null })), chapter: { chapterKey: params.chapterKey, anilistId: params.anilistId || 0, provider: params.extensionId, providerChapterId: chapterId, number: null, title: null, language: null } } };
}

export async function serverNovelChapters(params: { anilistId?: number; title?: string; titles?: string[]; fullScan?: boolean }) {
  const results = await Promise.all(listServers().filter(row => row.enabled).map(async server => {
    const chapters: any[] = [], providersAvailable: string[] = [], failedProviders: string[] = []; let pending = false;
    try {
      const sources = await serverRequest<ServerSource[]>(server, '/sources');
      await Promise.all(sources.filter(source => source.available && source.kind === 'novel').map(async source => {
        const provider = `server-${server.id}|${source.id}`;
        try {
          const result = await serverInvoke(`server-${server.id}`, source.id, 'novelChapters', params);
          pending ||= result?.partial === true;
          providersAvailable.push(provider);
          for (const chapter of Array.isArray(result) ? result : result.chapters || []) {
            const entries = Array.isArray(chapter.sources) && chapter.sources.length ? chapter.sources : [chapter];
            chapters.push({ ...chapter, sources: entries.map((entry: any) => {
              const detail = JSON.stringify({ chapterKey: entry.chapterKey, provider: entry.provider, providerChapterId: entry.providerChapterId });
              return { ...entry, provider, providerName: `${entry.providerName || entry.provider || source.name} · ${server.name}`,
                chapterKey: `${provider}:${detail}`, providerChapterId: detail };
            }) });
          }
        } catch { failedProviders.push(provider); }
      }));
    } catch { failedProviders.push(`server-${server.id}`); }
    return { chapters, providersAvailable, failedProviders, pending };
  }));
  return { pending: results.some(row => row.pending), chapters: results.flatMap(row => row.chapters), providersAvailable: results.flatMap(row => row.providersAvailable), failedProviders: results.flatMap(row => row.failedProviders) };
}

export async function serverNovelContent(params: { extensionId: string; chapterKey: string; providerChapterId?: string; anilistId?: number }): Promise<NovelContentResponse> {
  const [namespace, sourceId] = params.extensionId.split('|');
  const encoded = params.providerChapterId || params.chapterKey.slice(params.extensionId.length + 1);
  const detail = JSON.parse(encoded);
  if (typeof detail.chapterKey !== 'string') throw Error('This chapter has no source key. Reload the novel’s chapter list.');
  const result = await serverInvoke(namespace, sourceId, 'novelContent', { ...detail, anilistId: params.anilistId });
  const paragraphs = (result.paragraphs || []).map((paragraph: any, index: number) => ({ index, text: String(typeof paragraph === 'string' ? paragraph : paragraph.text || '').trim() })).filter((paragraph: any) => paragraph.text);
  return { success: paragraphs.length > 0, data: { paragraphs, chapter: { chapterKey: params.chapterKey, anilistId: params.anilistId || 0,
    provider: params.extensionId, providerChapterId: encoded, number: result.number ?? null, title: result.title ?? null, language: result.language ?? null },
    readMeta: { provider: params.extensionId, fetchedAt: new Date().toISOString(), fallbackUsed: false } } };
}
