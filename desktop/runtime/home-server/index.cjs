'use strict';

const { createHomeServerStore } = require('./store.cjs');
const { createHomeServer, getLanAddresses } = require('./server.cjs');
const pathApi = require('node:path');
const nodeFs = require('node:fs');
const { pathToFileURL } = require('node:url');

const DASHBOARD_PATH = '/server/';

function resolveAppRoot(app) {
  // app.getAppPath() includes app.asar in installed builds. Four parents from
  // this module escapes the archive and incorrectly points at resources/.
  return app?.getAppPath?.() || pathApi.resolve(__dirname, '../../..');
}

/**
 * Roots that may hold the integrated extension server (`server/`). In a packaged
 * build the sources live inside app.asar (Electron's fs and ESM loader read it
 * transparently); if the integrator unpacks the whole `server/` tree instead,
 * the copy under app.asar.unpacked is used.
 */
function candidateRoots(app) {
  const roots = [];
  const add = (root) => {
    if (!root || roots.includes(root)) return;
    roots.push(root);
    const unpacked = root.replace(/app\.asar(?=$|[\\/])/, 'app.asar.unpacked');
    if (unpacked !== root && !roots.includes(unpacked)) roots.push(unpacked);
  };
  add(resolveAppRoot(app));
  // Unpackaged runs (`electron desktop/main.cjs`, used by electron:dev) report
  // desktop/ as the app path, so also try the repository root next to this module.
  add(pathApi.resolve(__dirname, '../../..'));
  return roots;
}

const entryFor = (root) => pathApi.join(root, 'server', 'src', 'desktop.mjs');
const builtFor = (root) => pathApi.join(root, 'server', 'dist', 'interpreter.mjs');

function exists(file) {
  try { return nodeFs.existsSync(file); } catch { return false; }
}

/** `<appRoot>/server/src/desktop.mjs`, preferring a root where server/dist is built. */
function resolveExtensionServerEntry(app) {
  const roots = candidateRoots(app);
  const ready = roots.find((root) => exists(entryFor(root)) && exists(builtFor(root)));
  return entryFor(ready || roots.find((root) => exists(entryFor(root))) || roots[0]);
}

/** Available = the entry point exists and `server:build` produced dist/interpreter.mjs. */
function isExtensionServerBuilt(app) {
  return candidateRoots(app).some((root) => exists(entryFor(root)) && exists(builtFor(root)));
}

function safeOrigin(value) {
  try {
    const url = new URL(String(value || '').trim());
    if (url.protocol !== 'http:' && url.protocol !== 'https:') return null;
    return url.origin;
  } catch {
    return null;
  }
}

const HOST_HEADER_RE = /^[a-z0-9.\-]+(:\d{1,5})?$|^\[[0-9a-f:.]+\](:\d{1,5})?$/i;

/**
 * D4c: media URLs are signed against the origin the client actually used.
 * remoteBaseUrl is only used when the request arrived through the tunnel
 * (its Host, or X-Forwarded-Host, matches the remote host).
 */
function requestBase(req, config) {
  const rawHost = String(req.headers.host || '').trim();
  const host = HOST_HEADER_RE.test(rawHost) ? rawHost : `localhost:${Number(config.port) || 8787}`;
  const remote = safeOrigin(config.remoteBaseUrl);
  if (remote) {
    const remoteUrl = new URL(remote);
    const forwarded = String(req.headers['x-forwarded-host'] || '').split(',')[0].trim().toLowerCase();
    const seen = [host.toLowerCase(), forwarded].filter(Boolean);
    if (seen.some((value) => value === remoteUrl.host.toLowerCase() || value === remoteUrl.hostname.toLowerCase())) return remote;
  }
  return `http://${host}`;
}

/** Hosts the running home server is actually reachable at, given its bind mode. */
function reachableLanHosts(status) {
  const bindHost = status.bindHost;
  if (bindHost === '127.0.0.1') return [];
  if (bindHost === '0.0.0.0') return status.lanAddresses || getLanAddresses();
  return bindHost ? [bindHost] : [];
}

function loopbackHost(status) {
  return status.bindHost && status.bindHost !== '0.0.0.0' ? status.bindHost : '127.0.0.1';
}

function createHomeServerRuntime({ app, fs, path, logger, adapters }) {
  const store = createHomeServerStore({ app, fs, path, logger });
  let extensionRuntime;
  // The embedded Tatakai extension server lives in <appRoot>/server. When it has
  // not been built (no server/dist) the legacy home server still works and the
  // extension-server routes answer 503 instead of crashing start().
  const extensionServerAvailable = () => isExtensionServerBuilt(app);
  const ensureExtensionServer = () => extensionRuntime ||= import(pathToFileURL(resolveExtensionServerEntry(app)).href)
    .then(module => module.createDesktopServer({ app, legacyStore: store, adapters }))
    .then(runtime => { app.once?.('before-quit', () => runtime.close()); return runtime; })
    .catch(error => { extensionRuntime = null; throw error; });
  adapters.handleExtensionServer = async (req, res) => {
    if (!extensionServerAvailable()) throw new Error('extension server not built');
    const runtime = await ensureExtensionServer();
    return runtime.handle(req, res, requestBase(req, store.read()));
  };
  const server = createHomeServer({ store, adapters, logger });

  function reachableOrigins() {
    const status = server.getStatus();
    const port = Number(status.port) || 8787;
    const lan = reachableLanHosts(status).map((ip) => `http://${ip}:${port}`);
    const remoteOrigin = safeOrigin(status.remoteBaseUrl);
    // Clients only accept plain HTTP for LAN hosts, so a remote address must be HTTPS.
    const remote = status.remoteEnabled && remoteOrigin?.startsWith('https://') ? remoteOrigin : null;
    return { status, lan, remote };
  }

  async function getExtensionServerPairing({ prefer = 'lan', origin } = {}) {
    const { status, lan, remote } = reachableOrigins();
    if (!status.running) throw new Error('home_server_stopped');
    if (!extensionServerAvailable()) throw new Error('extension_server_unavailable');
    const runtime = await ensureExtensionServer().catch((error) => {
      logger?.warn?.('[HomeServer] extension server failed to load', error);
      throw new Error('extension_server_unavailable');
    });
    if (typeof runtime.createPairing !== 'function') throw new Error('extension_server_unavailable');
    const known = [...lan, ...(remote ? [remote] : [])];
    const explicit = safeOrigin(origin);
    let chosen = explicit && known.includes(explicit) ? explicit : null;
    if (!chosen && prefer === 'remote' && remote) chosen = remote;
    if (!chosen) chosen = lan[0] || remote;
    if (!chosen) throw new Error('no_reachable_address');
    const pairing = await runtime.createPairing(chosen);
    return { deepLink: pairing.deepLink, code: pairing.code, expiresIn: pairing.expiresIn, origin: chosen };
  }

  async function getExtensionServerInfo() {
    if (!extensionServerAvailable()) return null;
    try {
      const runtime = await ensureExtensionServer();
      if (typeof runtime.info !== 'function') return null;
      const info = await runtime.info();
      return {
        name: String(info?.name || ''),
        sourceCount: Number(info?.sourceCount) || 0,
        ecosystems: Array.isArray(info?.ecosystems) ? info.ecosystems.map(String) : [],
      };
    } catch (error) {
      logger?.warn?.('[HomeServer] extension server info failed', error);
      return null;
    }
  }

  function getExtensionServerDashboardUrl() {
    const status = server.getStatus();
    if (!status.running) throw new Error('home_server_stopped');
    if (!extensionServerAvailable()) throw new Error('extension_server_unavailable');
    return `http://${loopbackHost(status)}:${Number(status.port) || 8787}${DASHBOARD_PATH}`;
  }

  return {
    store,
    server,
    start: async (overrides) => {
      if (extensionServerAvailable()) {
        // A broken extension server must never keep the legacy home server down.
        await ensureExtensionServer().catch((error) => logger?.warn?.('[HomeServer] extension server failed to start', error));
      }
      return server.start(overrides);
    },
    getExtensionServerBootstrap: async () => {
      if (!extensionServerAvailable()) return null;
      try { return (await ensureExtensionServer()).bootstrap; } catch { return null; }
    },
    getExtensionServerPairing,
    getExtensionServerInfo,
    getExtensionServerDashboardUrl,
    isExtensionServerAvailable: extensionServerAvailable,
    stop: () => server.stop(),
    getStatus: () => server.getStatus(),
  };
}

module.exports = { createHomeServerRuntime, resolveExtensionServerEntry, isExtensionServerBuilt, requestBase, DASHBOARD_PATH };
