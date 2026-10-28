// Tatakai native extension host - bundle loader and route dispatcher (routes.v1.json). Evaluated after prelude.js.
(function (G) {
  'use strict';
  const state = { instance: null, manifest: null, cache: new Map(), result: null };

  class HostError extends Error {
    constructor(status, message) { super(message); this.status = status; }
  }

  G.__tk_load = function (manifestJson, source) {
    state.manifest = JSON.parse(manifestJson);
    const module = { exports: {} };
    const params = ['module', 'exports', 'require', '__tatakai_fetch__', '__tatakai_anitomy__', '__tatakai_parse_html__', 'Buffer', 'process', 'global', 'setImmediate', 'clearImmediate', 'fetch'];
    const fn = new Function(...params, source);
    fn(module, module.exports, G.__tk_require, G.__tatakai_fetch__, G.__tatakai_anitomy__, G.__tatakai_parse_html__, G.Buffer, G.process, G, G.setImmediate, G.clearImmediate, G.fetch);
    const exp = module.exports && module.exports.default ? module.exports.default : module.exports;
    let inst = exp;
    if (typeof exp === 'function') inst = new exp();
    if (!inst || (typeof inst !== 'object' && typeof inst !== 'function')) throw new Error('Extension bundle produced no usable export');
    state.instance = inst;
  };

  const has = (name) => state.instance && typeof state.instance[name] === 'function';
  const call = (name, ...args) => {
    if (!has(name)) throw new HostError(501, "Extension does not implement '" + name + "'");
    return state.instance[name](...args);
  };

  // ---- source normalisation -----------------------------------------------------------------------------------------
  const asArray = (v) => (Array.isArray(v) ? v : v === undefined || v === null ? [] : [v]);
  const num = (v, d) => { const n = Number(v); return Number.isFinite(n) ? n : d; };

  function parseOpts(p) {
    p = p || {};
    const titles = asArray(p.titles !== undefined ? p.titles : p['titles[]']).map(String).filter(Boolean);
    const anilistId = num(p.anilistId, 0) > 0 ? num(p.anilistId, 0) : 0;
    const episode = num(p.episode, 1) > 0 ? num(p.episode, 1) : 1;
    const opts = { anilistId, titles, episode, resolution: p.resolution ? String(p.resolution) : '1080p' };
    const pl = p.preferredLanguages !== undefined ? p.preferredLanguages : p.preferredLanguage;
    if (pl !== undefined) opts.preferredLanguages = (Array.isArray(pl) ? pl : String(pl).split(',')).map((s) => String(s).trim()).filter(Boolean);
    const po = {};
    for (const k of ['maxConcurrency', 'maxRetries', 'retryDelayMs', 'timeoutMs']) if (p[k] !== undefined) po[k] = num(p[k], undefined);
    if (po.timeoutMs === undefined) po.timeoutMs = 20000;
    opts.providerOptions = po;
    for (const k of ['episodeCount', 'tatakaiId', 'countryCode', 'exclusions']) if (p[k] !== undefined) opts[k] = p[k];
    return opts;
  }

  const typeOf = (r) => {
    const t = String(r.sourceType || '').toLowerCase();
    if (t) return t;
    const u = String(r.url || '');
    if (/\.m3u8(\?|#|$)/i.test(u)) return 'hls';
    if (/\.mp4(\?|#|$)/i.test(u)) return 'mp4';
    if (/\.mpd(\?|#|$)/i.test(u)) return 'dash';
    if (/^magnet:/i.test(u) || /\.torrent(\?|$)/i.test(u)) return 'torrent';
    return 'embed';
  };

  function normalize(r, provider, defaultPriority) {
    const type = typeOf(r);
    const audio = r.audioLanguage || r.language || undefined;
    return Object.assign({}, r, {
      providerName: provider,
      providerKey: String(provider).toLowerCase(),
      server: r.server || '',
      providerPriority: Number.isFinite(r.providerPriority) ? r.providerPriority : defaultPriority,
      audioLanguage: audio,
      language: audio,
      languageLabel: r.languageLabel || audio,
      isDub: audio !== undefined && audio !== 'ja',
      type,
      sourceType: type,
      fileFormat: r.fileFormat || (type === 'hls' ? 'm3u8' : type === 'mp4' ? 'mp4' : type === 'dash' ? 'mpd' : type),
      isM3U8: type === 'hls',
      isEmbed: type === 'embed' || type === 'custom',
      isTorrent: type === 'torrent',
      headers: r.headers || {},
      subtitles: r.subtitles || [],
    });
  }

  const qualityRank = (q) => { const m = /(\d{3,4})/.exec(String(q || '')); return m ? Number(m[1]) : 0; };
  function sortSources(list, prefs) {
    const langRank = (s) => { if (!prefs || !prefs.length) return 0; const i = prefs.indexOf(s.audioLanguage); return i < 0 ? prefs.length : i; };
    return list
      .map((s, i) => [s, i])
      .sort((a, b) => a[0].providerPriority - b[0].providerPriority || qualityRank(b[0].quality) - qualityRank(a[0].quality) || langRank(a[0]) - langRank(b[0]) || a[1] - b[1])
      .map((e) => e[0]);
  }

  const count = (list, key) => list.reduce((acc, s) => { const k = s[key] === undefined ? 'unknown' : String(s[key]); acc[k] = (acc[k] || 0) + 1; return acc; }, {});

  async function collect(method, fallback, opts, onlyType) {
    const chunks = [];
    if (has(method)) {
      await state.instance[method](opts, (chunk) => { if (chunk) chunks.push(chunk); });
    } else if (has(fallback)) {
      const results = asArray(await state.instance[fallback](opts));
      chunks.push({ provider: state.instance.name || state.manifest.id, results, diagnostic: { provider: state.instance.name || state.manifest.id, status: results.length ? 'ok' : 'empty', resultCount: results.length, durationMs: 0, attempts: 1 } });
    } else {
      throw new HostError(501, "Extension does not implement '" + method + "'");
    }
    let sources = [];
    const providerStatus = [];
    chunks.forEach((c, i) => {
      const provider = String(c.provider || (c.diagnostic && c.diagnostic.provider) || state.manifest.id);
      sources = sources.concat(asArray(c.results).map((r) => normalize(r, provider, i)));
      if (c.diagnostic) providerStatus.push(c.diagnostic);
    });
    if (onlyType) sources = sources.filter((s) => s.type === onlyType);
    sources = sortSources(sources, opts.preferredLanguages);
    return { sources, byLanguage: count(sources, 'audioLanguage'), byType: count(sources, 'type'), providerStatus, count: sources.length, cached: false, fetchedAt: new Date().toISOString() };
  }

  async function sourceRoute(route, p) {
    const opts = parseOpts(p);
    if (opts.anilistId === 0 && opts.titles.length === 0) throw new HostError(400, 'anilistId or titles is required');
    const bypass = p && (String(p.refresh) === '1' || String(p.nocache) === '1');
    const key = route + ':' + JSON.stringify(opts);
    const ttl = route === 'torrent' ? 1800000 : 600000;
    const hit = state.cache.get(key);
    if (!bypass && hit && Date.now() - hit.at < ttl) return Object.assign({}, hit.value, { cached: true });
    const value = route === 'stream' ? await collect('streamAll', 'single', opts)
      : route === 'torrent' ? await collect('torrentAll', 'batch', opts, 'torrent')
      : await collect('sourcesAll', 'batch', opts);
    state.cache.set(key, { at: Date.now(), value });
    return value;
  }

  async function providerRoute(name, p) {
    const opts = parseOpts(p);
    let chunk;
    try {
      chunk = await call('runProvider', name, opts);
    } catch (e) {
      if (e instanceof HostError) throw e;
      if (/unknown provider/i.test(String(e && e.message))) throw new HostError(404, String(e.message));
      throw e;
    }
    if (!chunk) throw new HostError(404, 'Unknown provider: ' + name);
    const provider = String(chunk.provider || name);
    const diag = chunk.diagnostic || {};
    let sources = asArray(chunk.results).map((r) => normalize(r, provider, 0));
    sources = sortSources(sources, opts.preferredLanguages);
    return {
      provider, kind: chunk.kind || null, status: diag.status || (sources.length ? 'ok' : 'empty'), durationMs: diag.durationMs === undefined ? null : diag.durationMs,
      attempts: diag.attempts === undefined ? null : diag.attempts, error: diag.error || null, count: sources.length, sources,
      byLanguage: count(sources, 'audioLanguage'), byType: count(sources, 'type'), fetchedAt: new Date().toISOString(),
    };
  }

  const withNs = (v) => (v && typeof v === 'object' && !Array.isArray(v) ? Object.assign({ namespace: state.manifest.apiServer && state.manifest.apiServer.namespace || state.manifest.id }, v) : v);

  async function route(name, p) {
    p = p || {};
    const m = state.manifest;
    const ns = (m.apiServer && m.apiServer.namespace) || m.id;
    switch (name) {
      case 'health': return { ok: true, namespace: ns, extensionId: m.id };
      case 'sources': case 'stream': case 'torrent': return sourceRoute(name, p);
      case 'providers': {
        const providers = asArray(await call('listProviders'));
        const byKind = providers.reduce((a, x) => { const k = x.kind || 'unknown'; a[k] = (a[k] || 0) + 1; return a; }, {});
        return { namespace: ns, count: providers.length, byKind, providers, fetchedAt: new Date().toISOString() };
      }
      case 'manga/chapters': {
        const r = has('debugMangaChapters') ? await call('debugMangaChapters', p) : await call('getMangaChapters', p);
        return withNs(r);
      }
      case 'manga/pages': { if (!p.chapterKey) throw new HostError(400, 'chapterKey is required'); const r = await call('getMangaPages', p); return Array.isArray(r) ? { namespace: ns, pages: r, count: r.length } : withNs(r); }
      case 'novel/search': { const r = await call('getNovelSearch', { query: p.query, provider: p.provider }); return Array.isArray(r) ? { namespace: ns, results: r } : withNs(r); }
      case 'novel/chapters': { const r = has('debugNovelChapters') ? await call('debugNovelChapters', p) : await call('getNovelChapters', p); return withNs(r); }
      case 'novel/content': { if (!p.chapterKey) throw new HostError(400, 'chapterKey is required'); return withNs(await call('getNovelContent', p)); }
      case 'custom/home': return call('customHome', p.sourceId);
      case 'custom/search': return call('customSearch', p.sourceId, p.query, num(p.page, 1), p.filters || {});
      case 'custom/info': return call('customInfo', p.sourceId, p.id);
      case 'custom/watch': return call('customWatch', p.sourceId, p.id, p.episodeId);
      case 'custom/read': return call('customRead', p.sourceId, p.id, p.chapterId);
      default:
        if (name.startsWith('providers/')) return providerRoute(decodeURIComponent(name.slice('providers/'.length)), p);
        throw new HostError(404, "Unknown route '" + name + "'");
    }
  }

  function settle(promise) {
    state.result = null;
    promise.then(
      (v) => { state.result = { ok: true, value: v === undefined ? null : v }; },
      (e) => { state.result = { ok: false, status: (e && e.status) || 502, message: String((e && e.message) || e) }; }
    );
  }

  // kind: 'route' | 'method'; payload: JSON ({route params} object, or args array for 'method')
  G.__tk_dispatch = function (kind, name, payloadJson) {
    state.result = null;
    let p;
    try {
      p = Promise.resolve().then(() => {
        const payload = payloadJson ? JSON.parse(payloadJson) : undefined;
        if (kind === 'method') {
          if (!has(name)) throw new HostError(501, "Extension does not implement '" + name + "'");
          return state.instance[name](...(Array.isArray(payload) ? payload : []));
        }
        return route(name, payload);
      });
    } catch (e) {
      p = Promise.reject(e);
    }
    settle(p);
  };

  // One round trip per host-loop iteration: JSON {done, envelope?, timer, fetches}
  G.__tk_status = function () {
    if (state.result) {
      let env;
      try { env = JSON.stringify(state.result); } catch (e) { env = JSON.stringify({ ok: false, status: 502, message: 'result is not JSON serialisable' }); }
      state.result = null;
      return JSON.stringify({ done: true, envelope: env });
    }
    return JSON.stringify({ done: false, timer: G.__tk_next_timer(), fetches: G.__tk_pending_fetches() });
  };
})(globalThis);
