import { Worker } from 'node:worker_threads';
import { readFile } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import { limitedBytes, base64, cleanHeaders, fail } from './security.mjs';

/** Native extensions are trusted owner-installed programs. A worker contains
 * crashes/timeouts, but is not an OS security sandbox. No deployment secrets
 * are passed through its environment. */
let active = 0;
let bootstrap;
const waiting = [];
async function acquire(signal) {
  signal?.throwIfAborted();
  if (active < 4) { active++; return; }
  if (waiting.length >= 32) fail(503, 'extension_queue_full');
  await new Promise((resolve, reject) => {
    const task = { resolve: () => { signal?.removeEventListener('abort', abort); resolve(); } };
    const abort = () => { const index = waiting.indexOf(task); if (index >= 0) waiting.splice(index, 1); reject(signal.reason || Error('Cancelled')); };
    signal?.addEventListener('abort', abort, { once: true }); waiting.push(task);
  });
}
export async function nodeInterpreter(options) {
  const bounded = { ...options, signal: options.signal ? AbortSignal.any([options.signal, AbortSignal.timeout(25000)]) : AbortSignal.timeout(25000) };
  await acquire(bounded.signal);
  try {
    bootstrap ||= readFile(new URL('../dist/node-extension-worker.cjs', import.meta.url), 'utf8');
    return await execute({ ...bounded, bootstrap: await bootstrap });
  }
  finally { if (waiting.length) waiting.shift().resolve(); else active--; }
}
function execute({ code, probe, method, args = {}, preferences = {}, manifest = {}, network, signal, bootstrap, onProgress }) {
  const worker = new Worker(bootstrap, { eval: true,
    workerData: { code, probe, method, args, preferences, requireBase: fileURLToPath(new URL('./node-extension-worker.cjs', import.meta.url)) }, env: { NODE_ENV: 'production' },
    resourceLimits: { maxOldGenerationSizeMb: 192, stackSizeMb: 4 },
  });
  return new Promise((resolve, reject) => {
    const requests = new Map(), partial = []; let settled = false;
    const finish = (error, value) => {
      if (settled) return; settled = true; clearTimeout(timer); signal?.removeEventListener('abort', abort);
      for (const controller of requests.values()) controller.abort(); requests.clear();
      void worker.terminate(); error ? reject(error) : resolve(value);
    };
    const abort = () => signal?.reason?.name === 'TimeoutError' && partial.length ? finish(null, partial) : finish(signal?.reason || Error('Extension cancelled'));
    const timer = setTimeout(() => partial.length ? finish(null, partial) : finish(Object.assign(Error('Native extension timed out'), { status: 504, code: 'extension_timeout' })), 25000);
    signal?.addEventListener('abort', abort, { once: true }); if (signal?.aborted) abort();
    worker.on('error', error => finish(error)); worker.on('exit', () => { if (!settled) finish(Error('Native extension exited without a result')); });
    worker.on('message', async message => {
      if (settled) return;
      if (message.type === 'chunk') {
        if (Array.isArray(message.results)) {
          const items = message.results.slice(0, Math.max(0, (['getMangaChapters','getNovelChapters'].includes(method) ? 20000 : 500) - partial.length));
          partial.push(...items); if (items.length) onProgress?.(items);
        }
        return;
      }
      if (message.type === 'result') { finish(null, message.value); return; }
      if (message.type === 'error') { finish(Object.assign(Error(String(message.error).replace(/https?:\/\/\S+/g, '[URL]').slice(0, 500)), { status: 422, code: 'native_extension_error' })); return; }
      if (message.type === 'cancel') { requests.get(message.id)?.abort(); return; }
      if (message.type !== 'fetch') return;
      if (requests.size >= 24) { worker.postMessage({ type: 'fetchResult', id: message.id, error: 'Too many extension requests' }); return; }
      const controller = new AbortController(); requests.set(message.id, controller);
      try {
        const target = new URL(message.url);
        if (Array.isArray(manifest.permissions)) {
          const domains = manifest.permissions.filter(value => value.startsWith('network:domain:')).map(value => value.slice(15).toLowerCase());
          if (!domains.some(domain => domain === '*' || domain === target.hostname || domain.startsWith('*.') && target.hostname.endsWith(domain.slice(1)))) fail(403, 'network_permission_denied');
        }
        const init = message.init || {};
        const { response, url } = await network(target.href, { method: init.method || 'GET', body: init.body, headers: cleanHeaders(init.headers), signal: AbortSignal.any([controller.signal, AbortSignal.timeout(15000)]) });
        const body = base64(await limitedBytes(response, 8 * 1024 * 1024));
        if (!settled) worker.postMessage({ type: 'fetchResult', id: message.id, status: response.status, headers: Object.fromEntries(response.headers), body, url });
      } catch (error) { if (!settled) worker.postMessage({ type: 'fetchResult', id: message.id, error: error.code || 'Source request failed' }); }
      finally { requests.delete(message.id); }
    });
  });
}
