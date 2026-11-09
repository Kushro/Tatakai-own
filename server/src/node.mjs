import http from 'node:http';
import { Readable } from 'node:stream';
import { readFile, access } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { createRequire } from 'node:module';
import { createServer } from './core.mjs';
import { nodeStorage } from './node-storage.mjs';
import { vault } from './security.mjs';
import { createNativeCatalog } from './native-host.mjs';
import { nodeNetwork } from './node-network.mjs';
import { nodeInterpreter } from './node-extension.mjs';
import { resolveNodeSecrets } from './node-secrets.mjs';
import { startupBanner } from './startup-banner.mjs';

/** Dashboard mount point (contract D3a). The Vite build uses the same base. */
export const DASHBOARD_PATH = '/server/';
const serverRoot = fileURLToPath(new URL('../', import.meta.url));
/** <repo>/desktop/runtime/compatibility: the same Java compatibility runtime Electron uses (no copy). */
export const compatibilityRoot = path.resolve(serverRoot, '../desktop/runtime/compatibility');
const DASHBOARD_CSP = "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data: blob:; connect-src 'self'; frame-ancestors 'none'; base-uri 'none'";
const MIME = { '.html': 'text/html; charset=utf-8', '.js': 'text/javascript; charset=utf-8', '.mjs': 'text/javascript; charset=utf-8', '.css': 'text/css; charset=utf-8',
  '.json': 'application/json', '.webmanifest': 'application/manifest+json', '.map': 'application/json', '.svg': 'image/svg+xml', '.png': 'image/png', '.jpg': 'image/jpeg',
  '.jpeg': 'image/jpeg', '.webp': 'image/webp', '.ico': 'image/x-icon', '.woff': 'font/woff', '.woff2': 'font/woff2', '.txt': 'text/plain; charset=utf-8' };

/** Loads the bundled QuickJS interpreter; missing server/dist becomes a clear, catchable error. */
export async function loadInterpreter() {
  try { await access(fileURLToPath(new URL('../dist/interpreter.mjs', import.meta.url))); }
  catch { throw Object.assign(Error('extension server not built'), { code: 'extension_server_not_built' }); }
  return (await import('../dist/interpreter.mjs')).interpreter;
}

/** Serves server/dist/ui under /server/: `/server` -> 301 `/server/`, real files from
 * dist/ui (hashed assets cached immutably), extensionless paths -> index.html (SPA). */
export function createDashboardResponse({ ui = fileURLToPath(new URL('../dist/ui/', import.meta.url)), rootRedirect = false } = {}) {
  const base = path.resolve(ui) + path.sep;
  const text = (status, body, headers = {}) => new Response(body, { status, headers: { 'content-type': 'text/plain; charset=utf-8', 'cache-control': 'no-store', ...headers } });
  const file = async filename => {
    const body = await readFile(filename);
    const immutable = filename.startsWith(base + 'assets' + path.sep);
    return new Response(body, { headers: { 'content-type': MIME[path.extname(filename).toLowerCase()] || 'application/octet-stream',
      'cache-control': immutable ? 'public, max-age=31536000, immutable' : 'no-cache', 'content-security-policy': DASHBOARD_CSP, 'referrer-policy': 'no-referrer' } });
  };
  return async function dashboardResponse(request) {
    if (!['GET', 'HEAD'].includes(request.method)) return text(405, 'Method not allowed', { allow: 'GET, HEAD' });
    const url = new URL(request.url);
    let pathname;
    try { pathname = decodeURIComponent(url.pathname); } catch { return text(400, 'Bad request'); }
    if (pathname === '/' && rootRedirect) return new Response(null, { status: 302, headers: { location: DASHBOARD_PATH, 'cache-control': 'no-store' } });
    if (pathname === DASHBOARD_PATH.slice(0, -1)) return new Response(null, { status: 301, headers: { location: DASHBOARD_PATH + url.search } });
    if (!pathname.startsWith(DASHBOARD_PATH)) return text(404, 'Not found');
    const relative = pathname.slice(DASHBOARD_PATH.length);
    const filename = path.resolve(base, relative || 'index.html');
    if (filename !== base.slice(0, -1) && !filename.startsWith(base)) return text(404, 'Not found');
    try { return await file(filename); }
    catch (error) {
      if (!['ENOENT', 'EISDIR', 'ENOTDIR'].includes(error.code)) throw error;
      // Unknown asset-like paths are real 404s; client-side routes fall back to the SPA shell.
      if (path.extname(relative)) return text(404, 'Not found');
      try { return await file(path.join(base, 'index.html')); }
      catch { return text(503, 'Extension server dashboard is not built. Run npm run server:build.'); }
    }
  };
}

export async function serveNodeRequest(handler, req, res, base) {
  const abort = new AbortController();
  req.on('aborted', () => abort.abort()); res.on('close', () => { if (!res.writableEnded) abort.abort(); });
  try {
    let cancelled = false;
    const body = !['GET', 'HEAD'].includes(req.method) ? new ReadableStream({
      start(controller) {
        req.on('data', chunk => { if (!cancelled) { controller.enqueue(chunk); if (controller.desiredSize <= 0) req.pause(); } });
        req.on('end', () => { if (!cancelled) controller.close(); });
        req.on('error', error => { if (!cancelled) controller.error(error); });
      }, pull() { req.resume(); }, cancel() { cancelled = true; req.resume(); },
    }) : undefined;
    const request = new Request(new URL(req.url, base), { method: req.method, headers: req.headers,
      ...(body ? { body, duplex: 'half' } : {}), signal: abort.signal });
    const response = await handler(request);
    res.writeHead(response.status, Object.fromEntries(response.headers));
    if (response.body) Readable.fromWeb(response.body).on('error', () => res.destroy()).pipe(res); else res.end();
  } catch {
    if (res.headersSent) { res.destroy(); return; }
    res.writeHead(500, { 'content-type': 'application/json', 'cache-control': 'no-store' });
    res.end(JSON.stringify({ success: false, error: { code: 'transport_error', message: 'The server could not complete this request. Retry and check server configuration if it continues.' } }));
  }
}

/** Standalone Node server (VPS / homelab / Docker).
 * Env: PORT (8787), HOST (127.0.0.1), DATA_DIR (<repo>/server/data), PUBLIC_URL, SERVER_SECRET,
 * BOOTSTRAP_SECRET, ENABLE_NATIVE_EXTENSIONS=1, ENABLE_BROWSER_RUNTIME=1, REQUIRE_PERSISTENT_STORAGE=1. */
export async function startNode(options = {}) {
  const explicitRoot = options.root || process.env.DATA_DIR;
  if (process.env.DATABASE_URL && !options.root) throw Error('DATABASE_URL is no longer supported: the Postgres storage adapter was removed. Mount a persistent volume and set DATA_DIR instead.');
  if (process.env.REQUIRE_PERSISTENT_STORAGE === '1' && !explicitRoot) throw Error('Set DATA_DIR to a persistent volume. This deployment requires persistent storage; temporary local files cannot preserve your server configuration.');
  const root = path.resolve(explicitRoot || path.join(serverRoot, 'data'));
  const storage = await nodeStorage(root);
  let broker;
  try {
    const secrets = await resolveNodeSecrets(root, { secret: options.secret || process.env.SERVER_SECRET,
      bootstrapSecret: options.bootstrapSecret || process.env.BOOTSTRAP_SECRET, hasState: !!await storage.store.read() });
    const interpreter = await loadInterpreter();
    const publicNetwork = nodeNetwork();
    const secret = secrets.secret, encryption = await vault(secret);
    const network = async (url, init) => {
      if (url.startsWith('tatakai-native:')) {
        const entry = await encryption.open(url.slice(15));
        if (!broker || new URL(entry.url).origin !== broker.host.base) throw Error('Internal native resource is unavailable');
        const response = await fetch(entry.url, { ...init, headers: { ...entry.headers, ...init.headers }, redirect: 'error' });
        return { response, url: entry.url, headers: init.headers, resourceUrl: async target => new URL(target).origin === new URL(entry.url).origin ? 'tatakai-native:' + await encryption.seal({ url: target, headers: entry.headers }) : target };
      }
      return publicNetwork(url, init);
    };
    const hosts = { ...(options.hosts || {}) };
    if ((options.nativeExtensions ?? process.env.ENABLE_NATIVE_EXTENSIONS === '1')) {
      // Same broker as Electron, required in place. aniyomi.cjs verifies its bootstrap sources at
      // <repo>/desktop/runtime/compatibility/aniyomi-java/** and <repo>/shared/aniyomi/**, so that layout must exist.
      const require = createRequire(import.meta.url), { CompatibilityBroker } = require(path.join(compatibilityRoot, 'broker.cjs'));
      let browser;
      if (process.env.ENABLE_BROWSER_RUNTIME === '1') {
        const { chromium } = await import('playwright'); const { PlaywrightBrowser } = await import('./playwright-browser.mjs'); browser = new PlaywrightBrowser(chromium);
      }
      broker = new CompatibilityBroker({ root: path.resolve(root, 'compatibility/mihon'), browser, proxy: async (url, headers) => {
        if (new URL(url).origin === broker.host.base) return 'tatakai-native:' + await encryption.seal({ url, headers });
        return url;
      } });
      hosts.catalog = createNativeCatalog({ broker });
      for (const family of ['mihon', 'cloudstream', 'aniyomi']) hosts[family] = true;
    }
    const dashboard = createDashboardResponse({ rootRedirect: true });
    const handler = await createServer({ ...storage, platform: 'node', interpreter, nodeInterpreter, network,
      secret, bootstrapSecret: secrets.bootstrapSecret, hosts, staticResponse: dashboard });
    const port = Number(options.port ?? process.env.PORT ?? 8787), bind = options.bind || process.env.HOST || '127.0.0.1';
    let origin = options.publicUrl || process.env.PUBLIC_URL || `http://localhost:${port}`;
    const server = http.createServer((req, res) => serveNodeRequest(handler, req, res, origin));
    await new Promise((resolve, reject) => { server.once('error', reject); server.listen(port, bind, resolve); });
    if (!options.publicUrl && !process.env.PUBLIC_URL && port === 0) origin = `http://localhost:${server.address().port}`;
    const listenHost = bind.includes(':') ? `[${bind}]` : bind;
    return { server, handler, root, origin: new URL(origin).origin, dashboardUrl: new URL(DASHBOARD_PATH, origin).href, listenUrl: `http://${listenHost}:${server.address().port}`,
      close: async () => { broker?.stop(); await new Promise(resolve => server.close(resolve)); await storage.close(); } };
  } catch (error) { broker?.stop(); await storage.close(); throw error; }
}
if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try { process.loadEnvFile?.(); } catch (error) { if (error.code !== 'ENOENT') throw error; }
  startNode().then(runtime => {
    console.log(startupBanner(runtime.origin, runtime.listenUrl));
    for (const signal of ['SIGINT', 'SIGTERM']) process.once(signal, () => runtime.close().then(() => process.exit(0)));
  }).catch(error => { console.error(error.message); process.exitCode = 1; });
}
