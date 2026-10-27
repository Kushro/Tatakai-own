'use strict';

const { createHomeServerStore } = require('./store.cjs');
const { createHomeServer } = require('./server.cjs');
const pathApi = require('node:path');
const nodeFs = require('node:fs');
const { pathToFileURL } = require('node:url');

function resolveExtensionServerEntry(app) {
  // app.getAppPath() includes app.asar in installed builds. Four parents from
  // this module escapes the archive and incorrectly points at resources/.
  const appRoot = app.getAppPath?.() || pathApi.resolve(__dirname, '../../..');
  return pathApi.join(appRoot, 'tatakai-server', 'src', 'desktop.mjs');
}

function createHomeServerRuntime({ app, fs, path, logger, adapters }) {
  const store = createHomeServerStore({ app, fs, path, logger });
  let extensionRuntime;
  // The embedded Tatakai Server (tatakai-server submodule) is not part of this
  // fork. When its entry point is absent the legacy home server still works and
  // the extension-server routes answer 503 instead of crashing start().
  const extensionServerAvailable = () => { try { return nodeFs.existsSync(resolveExtensionServerEntry(app)); } catch { return false; } };
  const ensureExtensionServer = () => extensionRuntime ||= import(pathToFileURL(resolveExtensionServerEntry(app)).href)
    .then(module => module.createDesktopServer({ app, legacyStore: store, adapters }))
    .then(runtime => { app.once?.('before-quit', () => runtime.close()); return runtime; })
    .catch(error => { extensionRuntime = null; throw error; });
  adapters.handleExtensionServer = async (req, res) => {
    if (!extensionServerAvailable()) throw new Error('extension server not bundled');
    const runtime = await ensureExtensionServer();
    const config = store.read();
    const base = config.remoteBaseUrl || `http://${req.headers.host || 'localhost:' + config.port}`;
    return runtime.handle(req, res, base);
  };
  const server = createHomeServer({ store, adapters, logger });

  return {
    store,
    server,
    start: async (overrides) => { if (extensionServerAvailable()) await ensureExtensionServer(); return server.start(overrides); },
    getExtensionServerBootstrap: async () => (extensionServerAvailable() ? (await ensureExtensionServer()).bootstrap : null),
    stop: () => server.stop(),
    getStatus: () => server.getStatus(),
  };
}

module.exports = { createHomeServerRuntime, resolveExtensionServerEntry };
