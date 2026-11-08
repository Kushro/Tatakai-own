import { mkdir, readFile, writeFile, copyFile } from 'node:fs/promises';
import { randomBytes, scryptSync, timingSafeEqual } from 'node:crypto';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { nodeStorage } from './node-storage.mjs';
import { nodeNetwork } from './node-network.mjs';
import { createServer, defaults } from './core.mjs';
import { createNativeCatalog } from './native-host.mjs';
import { random, vault } from './security.mjs';
import { serveNodeRequest } from './node.mjs';
import { nodeInterpreter } from './node-extension.mjs';

export async function createDesktopServer({ app, legacyStore, adapters }) {
  const root = path.join(app.getPath('userData'), 'extension-server'); await mkdir(root, { recursive: true });
  const secretPath = path.join(root, 'secrets.json');
  let secrets;
  try { secrets = JSON.parse(await readFile(secretPath, 'utf8')); }
  catch (error) { if (error.code !== 'ENOENT') throw error; secrets = { secret: randomBytes(48).toString('hex'), bootstrap: randomBytes(32).toString('hex') }; await writeFile(secretPath, JSON.stringify(secrets), { mode: 0o600, flag: 'wx' }); }
  const storage = await nodeStorage(root);
  const legacy = legacyStore.read();
  if (!await storage.store.read() && legacy.passwordHash) {
    const backup = path.join(root, 'home-server.before-migration.json');
    try { await copyFile(path.join(app.getPath('userData'), 'home-server.json'), backup, 1); } catch (error) { if (error.code !== 'EEXIST') throw error; }
    await storage.store.mutate(current => ({ state: current || { schema: 1, id: random(), settings: { ...defaults, name: 'Tatakai Desktop Server' },
      admin: { legacyHash: legacy.passwordHash, salt: legacy.passwordSalt }, viewer: { legacyHash: legacy.passwordHash, salt: legacy.passwordSalt },
      sessions: {}, extensions: [], repositories: [], attempts: {}, pairs: {}, audit: [], migratedAt: Date.now() } }));
  }
  const encryption = await vault(secrets.secret), publicNetwork = nodeNetwork();
  // Existing desktop proxy URLs are fetched only through this host-owned callback.
  const internal = new Map();
  const network = async (url, init) => {
    if (!url.startsWith('tatakai-native:')) return publicNetwork(url, init);
    const entry = await encryption.open(url.slice(15));
    const resource = internal.get(entry.id); if (!resource || resource.expires < Date.now()) throw Error('Native resource expired');
    const response = await fetch(resource.url, { ...init, headers: { ...resource.headers, ...init.headers }, redirect: 'error' });
    return { response, url: resource.url, headers: resource.headers, resourceUrl: async target => protect(target) };
  };
  async function protect(value) {
    if (typeof value === 'string' && adapters.isLocalMediaUrl?.(value)) {
      for (const [id, entry] of internal) if (entry.expires < Date.now()) internal.delete(id);
      if (internal.size > 10000) throw Error('Native resource limit reached');
      const id = random(); internal.set(id, { url: value, headers: {}, expires: Date.now() + 6 * 3600000 });
      return 'tatakai-native:' + await encryption.seal({ id });
    }
    if (Array.isArray(value)) return Promise.all(value.map(protect));
    if (value && typeof value === 'object') return Object.fromEntries(await Promise.all(Object.entries(value).map(async ([key, item]) => [key, await protect(item)])));
    return value;
  }
  const catalog = createNativeCatalog({ broker: adapters.compatibilityBroker?.(), listExtensions: adapters.listExtensions, invokeExtension: adapters.invokeExtension });
  const originalInvoke = catalog.invoke; catalog.invoke = async (...args) => protect(await originalInvoke(...args));
  const hosts = { catalog, torrent: true };
  for (const family of ['mihon', 'cloudstream', 'aniyomi']) hosts[family] = !!adapters.compatibilityBroker?.();
  const { interpreter } = await import('../dist/interpreter.mjs');
  const ui = fileURLToPath(new URL('../dist/ui/', import.meta.url));
  const handler = await createServer({ ...storage, secret: secrets.secret, bootstrapSecret: secrets.bootstrap, platform: 'desktop', network, hosts, interpreter, nodeInterpreter,
    legacyVerify: async (password, stored) => { try { return timingSafeEqual(scryptSync(String(password), stored.salt, 64), Buffer.from(stored.legacyHash, 'hex')); } catch { return false; } },
    staticResponse: async request => {
      const pathname = new URL(request.url).pathname, filename = path.resolve(ui, pathname.startsWith('/assets/') ? pathname.slice(1) : 'index.html');
      if (!filename.startsWith(ui)) return new Response('Not found', { status: 404 });
      try { return new Response(await readFile(filename), { headers: { 'content-type': filename.endsWith('.js') ? 'text/javascript' : filename.endsWith('.css') ? 'text/css' : 'text/html', 'referrer-policy': 'no-referrer' } }); } catch { return new Response('Build Tatakai Server before packaging desktop.', { status: 503 }); }
    } });
  return { bootstrap: secrets.bootstrap, handle: (req, res, base) => serveNodeRequest(handler, req, res, base), close: storage.close };
}
