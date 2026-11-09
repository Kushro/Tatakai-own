import { random, fail } from './security.mjs';
const management = new Set(['status', 'prepare', 'sources', 'repositories', 'addRepository', 'removeRepository', 'packages', 'installedPackages', 'install', 'remove', 'setEnabled', 'preferences', 'setPreference']);
/** Exposes a host compatibility broker (Aniyomi / Mihon / CloudStream) and the host's
 * installed .kai extensions as server sources. `broker` may be the broker itself or a
 * getter (Electron creates its broker lazily, after the extension runtime starts). */
export function createNativeCatalog({ broker, listExtensions, invokeExtension }) {
  const current = () => (typeof broker === 'function' ? broker() : broker) || null;
  async function request(payload, signal) {
    const active = current(); if (!active) fail(422, 'native_runtime_unavailable');
    const requestId = random();
    const abort = () => { void active.request({ op: 'cancel', target: requestId, requestId: random() }).catch(() => {}); };
    signal?.throwIfAborted(); signal?.addEventListener('abort', abort, { once: true });
    try { return await active.request({ ...payload, requestId }); }
    finally { signal?.removeEventListener('abort', abort); }
  }
  return {
    async list() {
      const native = current() ? await request({ op: 'sources' }).catch(() => []) : [];
      const installed = listExtensions ? await Promise.resolve().then(listExtensions).catch(() => []) : [];
      return [...(Array.isArray(native) ? native : []).map(source => ({ id: `native-${source.id}`, nativeId: source.id, external: true, ecosystem: source.ecosystem,
        manifest: { name: source.name, version: 'host' }, enabled: source.enabled, kind: source.kind, languages: [source.lang].filter(Boolean), priority: 0 })),
      ...(Array.isArray(installed) ? installed : []).map(source => ({ id: `local-${source.id}`, localId: source.id, external: true, ecosystem: 'tatakai', manifest: { name: source.name, version: source.version || 'host' }, enabled: true, kind: source.type === 'manga' ? 'read' : 'watch', languages: [], priority: 0 }))];
    },
    async manage(payload, signal) {
      if (!current()) fail(422, 'native_runtime_unavailable');
      if (!management.has(payload.op)) fail(400, 'unsupported_native_operation');
      return request(payload, signal);
    },
    async invoke(row, op, args, signal) {
      if (row.localId) {
        const methods = { search: 'search', browse: 'getHome', info: 'getInfo', episodes: 'getEpisodes', resolve: 'single', chapters: 'getMangaChapters', pages: 'getMangaPages' };
        if (!methods[op] || !invokeExtension) fail(422, 'unsupported_operation');
        return invokeExtension(row.localId, methods[op], { ...args, titles: args.titles || [args.title].filter(Boolean), resolution: args.resolution || '1080p' });
      }
      const payload = { ...args, op, sourceId: row.nativeId, ecosystem: row.ecosystem };
      if (op === 'resolve' && !args.id && args.title) {
        const result = await request({ ...payload, op: 'search', query: args.title }, signal);
        const normalize = value => String(value).toLowerCase().replace(/[^\p{L}\p{N}]/gu, '');
        const matches = (Array.isArray(result) ? result : result.results || []).filter(item => normalize(item.title || item.name) === normalize(args.title));
        if (matches.length !== 1) fail(422, 'title_match_required');
        const info = await request({ ...payload, op: 'info', id: matches[0].id }, signal);
        const entry = info.entries?.find(item => Number(item.number) === Number(args.episode || 1));
        if (!entry) fail(404, 'episode_not_found');
        payload.id = matches[0].id; payload.episodeId = entry.id;
      }
      return request(payload, signal);
    },
  };
}
