'use strict';
const { parentPort, workerData } = require('node:worker_threads');
const { createRequire } = require('node:module');
const { load } = require('cheerio');
const moduleRequire = createRequire(workerData.requireBase || __filename);
const pending = new Map(); let serial = 0;
parentPort.on('message', message => {
  if (message.type !== 'fetchResult') return;
  const request = pending.get(message.id); if (!request) return; pending.delete(message.id); request.cleanup();
  if (message.error) request.reject(Error(message.error));
  else {
    const response = new Response([204, 205, 304].includes(message.status) ? null : Buffer.from(message.body.replaceAll('-', '+').replaceAll('_', '/'), 'base64'), { status: message.status, headers: message.headers });
    Object.defineProperty(response, 'url', { value: message.url }); request.resolve(response);
  }
});
const hostFetch = (url, init = {}) => new Promise((resolve, reject) => {
  const id = ++serial, abort = () => { parentPort.postMessage({ type: 'cancel', id }); pending.delete(id); reject(Error('Source request cancelled')); };
  if (init.signal?.aborted) return reject(Error('Source request cancelled'));
  init.signal?.addEventListener('abort', abort, { once: true });
  pending.set(id, { resolve, reject, cleanup: () => init.signal?.removeEventListener('abort', abort) });
  const headers = Object.fromEntries(new Headers(init.headers || {}));
  const body = init.body instanceof URLSearchParams ? init.body.toString() : init.body;
  parentPort.postMessage({ type: 'fetch', id, url: String(url), init: { method: init.method || 'GET', headers, body } });
});
function parseHtml(html) {
  if (typeof html !== 'string' || html.length > 8 * 1024 * 1024) throw Error('Invalid source HTML');
  const $ = load(html);
  $.find = selector => {
    const selection = $(selector), each = selection.each.bind(selection);
    selection.each = callback => { each((index, node) => callback(index, $(node))); return selection; };
    return selection;
  };
  return $;
}
globalThis.fetch = hostFetch; globalThis.__tatakai_fetch__ = hostFetch; globalThis.__tatakai_parse_html__ = parseHtml;
globalThis.__tatakai_anitomy__ = title => ({ anime_title: title });
console.log = console.warn = console.error = console.debug = () => {};
(async () => {
  const mod = { exports: {} };
  const execute = new Function('module', 'exports', 'require', '__tatakai_fetch__', '__tatakai_parse_html__', '__tatakai_anitomy__', workerData.code);
  execute(mod, mod.exports, moduleRequire, hostFetch, parseHtml, globalThis.__tatakai_anitomy__);
  const exported = mod.exports.default || mod.exports;
  const provider = typeof exported === 'function' ? new exported() : exported;
  if (workerData.probe) {
    const methods = [...new Set([...Object.keys(provider), ...Object.getOwnPropertyNames(Object.getPrototypeOf(provider) || {})])].filter(key => key !== 'constructor' && typeof provider[key] === 'function');
    if (!methods.length) throw Error('No source methods exported');
    parentPort.postMessage({ type: 'result', value: { valid: true, methods } }); return;
  }
  const args = { ...workerData.args, preferences: workerData.preferences, titles: workerData.args.titles || [workerData.args.title].filter(Boolean), resolution: workerData.args.resolution || '1080p', providerOptions: { maxRetries: 0, maxConcurrency: 8, timeoutMs: 8000, ...workerData.args.providerOptions } };
  let method = workerData.method;
  const chunk = value => parentPort.postMessage({ type: 'chunk', results: (value.results || []).map(source => ({ ...source,
    source: source.source || value.provider,
    providerPriority: Number.isFinite(source.providerPriority) ? source.providerPriority : Number.isFinite(value.providerPriority) ? value.providerPriority : undefined,
  })) });
  let value;
  if (method === 'single' && typeof provider.streamAll === 'function') value = await provider.streamAll(args, chunk);
  else if (method === 'single' && typeof provider.sourcesAll === 'function') value = await provider.sourcesAll(args, chunk);
  else if (['getMangaChapters','getNovelChapters'].includes(method) && typeof provider[method === 'getMangaChapters' ? 'streamMangaChapters' : 'streamNovelChapters'] === 'function') {
    value = await provider[method === 'getMangaChapters' ? 'streamMangaChapters' : 'streamNovelChapters'](args, results => parentPort.postMessage({type:'chunk',results}));
  }
  else {
    if (typeof provider[method] !== 'function') throw Error('Unsupported extension method: ' + method);
    value = await provider[method](args);
  }
  if (JSON.stringify(value ?? null).length > 8 * 1024 * 1024) throw Error('Extension result exceeded 8 MB');
  parentPort.postMessage({ type: 'result', value });
})().catch(error => parentPort.postMessage({ type: 'error', error: String(error.message || error) }));
