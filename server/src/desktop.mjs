import { mkdir, readFile, writeFile, copyFile } from 'node:fs/promises';
import { randomBytes, scryptSync, timingSafeEqual } from 'node:crypto';
import os from 'node:os';
import path from 'node:path';
import { nodeStorage } from './node-storage.mjs';
import { nodeNetwork } from './node-network.mjs';
import { createServer, defaults } from './core.mjs';
import { capability } from './extensions.mjs';
import { createNativeCatalog } from './native-host.mjs';
import { random, vault } from './security.mjs';
import { serveNodeRequest, createDashboardResponse, loadInterpreter } from './node.mjs';
import { nodeInterpreter } from './node-extension.mjs';

/** Order of `info().ecosystems` (contract §3.4). */
const ECOSYSTEM_ORDER = ['aniyomi', 'mihon', 'cloudstream', 'tatakai', 'stremio', 'seanime', 'sora', 'mangayomi', 'kodi'];
const emptyState = name => ({ schema: 1, id: random(), settings: { ...defaults, name }, admin: null, viewer: null,
  sessions: {}, extensions: [], repositories: [], attempts: {}, pairs: {}, audit: [] });

/** Embeds the extension server in the Electron Home Server (served on its port under
 * /server/ and /api/server/v1). Rejects with Error('extension server not built') when
 * server/dist is missing so Electron can answer a graceful 503. */
export async function createDesktopServer({ app, legacyStore, adapters = {} }) {
  const interpreter = await loadInterpreter();
  const root = path.join(app.getPath('userData'), 'extension-server'); await mkdir(root, { recursive: true });
  const secretPath = path.join(root, 'secrets.json');
  let secrets;
  try { secrets = JSON.parse(await readFile(secretPath, 'utf8')); }
  catch (error) { if (error.code !== 'ENOENT') throw error; secrets = { secret: randomBytes(48).toString('hex'), bootstrap: randomBytes(32).toString('hex') }; await writeFile(secretPath, JSON.stringify(secrets), { mode: 0o600, flag: 'wx' }); }
  const storage = await nodeStorage(root);
  try {
    const legacy = legacyStore?.read?.() || {};
    if (!await storage.store.read()) {
      if (legacy.passwordHash) {
        const backup = path.join(root, 'home-server.before-migration.json');
        try { await copyFile(path.join(app.getPath('userData'), 'home-server.json'), backup, 1); } catch (error) { if (!['EEXIST', 'ENOENT'].includes(error.code)) throw error; }
        await storage.store.mutate(current => ({ state: current || { ...emptyState('Tatakai Desktop Server'),
          admin: { legacyHash: legacy.passwordHash, salt: legacy.passwordSalt }, viewer: { legacyHash: legacy.passwordHash, salt: legacy.passwordSalt }, migratedAt: Date.now() } }));
      } else {
        // D4a: no state and nothing to migrate -> configured from the first run so the PC can pair
        // immediately; owner access to the dashboard uses the bootstrap (setup) secret.
        await storage.store.mutate(current => ({ state: current || emptyState(`Tatakai (${os.hostname()})`.slice(0, 60)) }));
      }
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
    // The Electron broker starts with the extension runtime, possibly after this server: read it lazily.
    const broker = () => adapters.compatibilityBroker?.() || null;
    const catalog = createNativeCatalog({ broker, listExtensions: adapters.listExtensions, invokeExtension: adapters.invokeExtension });
    const originalInvoke = catalog.invoke; catalog.invoke = async (...args) => protect(await originalInvoke(...args));
    const hosts = { catalog, torrent: true };
    for (const family of ['mihon', 'cloudstream', 'aniyomi']) Object.defineProperty(hosts, family, { enumerable: true, get: () => !!broker() });
    const handler = await createServer({ ...storage, secret: secrets.secret, bootstrapSecret: secrets.bootstrap, platform: 'desktop', network, hosts, interpreter, nodeInterpreter,
      // D4b: no anonymous LAN access on the desktop; phones pair (device token) or use the viewer password.
      requireDeviceToken: true,
      legacyVerify: async (password, stored) => { try { return timingSafeEqual(scryptSync(String(password), stored.salt, 64), Buffer.from(stored.legacyHash, 'hex')); } catch { return false; } },
      staticResponse: createDashboardResponse() });
    return {
      bootstrap: secrets.bootstrap,
      handle: (req, res, base) => serveNodeRequest(handler, req, res, base),
      close: async () => { storage.close(); },
      /** Owner-side pairing for the Electron UI (QR payload = deepLink). */
      async createPairing(origin) {
        if (typeof handler.createPairing !== 'function') throw Error('pairing_unavailable');
        const { code, expiresIn, deepLink } = await handler.createPairing(origin);
        return { code, expiresIn, deepLink };
      },
      /** Same count as GET /sources for a viewer (enabled rows plus ~read / ~novel variants). */
      async info() {
        const state = await storage.store.read();
        const rows = [...(state?.extensions || []), ...await catalog.list().catch(() => [])].filter(row => row.enabled);
        let sourceCount = 0; const available = new Set();
        for (const row of rows) {
          const capabilities = row.manifest?.capabilities;
          sourceCount += 1 + (row.kind !== 'read' && capabilities?.includes('manga') ? 1 : 0) + (capabilities?.includes('novel') ? 1 : 0);
          if (capability(row.ecosystem, 'desktop', hosts).available) available.add(row.ecosystem);
        }
        return { name: state?.settings?.name || defaults.name, sourceCount, ecosystems: ECOSYSTEM_ORDER.filter(ecosystem => available.has(ecosystem)) };
      },
    };
  } catch (error) { storage.close(); throw error; }
}
