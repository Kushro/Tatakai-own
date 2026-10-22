import { afterAll, beforeAll, describe, expect, it } from 'bun:test';
import { createRequire } from 'module';
import http from 'node:http';

const require = createRequire(import.meta.url);
const { LocalProxyServer } = require('../desktop/runtime/proxy/local-proxy-server.cjs');
const { ExtensionWorkerPool } = require('../desktop/runtime/extension/extension-worker-pool.cjs');

const logger = { info() {}, error() {} };

// Other test files replace globalThis.fetch without restoring it; these tests
// need the real network stack on both the client and the proxy side.
let previousFetch;
beforeAll(() => { previousFetch = globalThis.fetch; globalThis.fetch = Bun.fetch; });
afterAll(() => { globalThis.fetch = previousFetch; });

async function startUpstream(handler) {
  const server = http.createServer(handler);
  await new Promise((resolve, reject) => {
    server.once('error', reject);
    server.listen(0, '127.0.0.1', resolve);
  });
  const { port } = server.address();
  return { server, origin: `http://127.0.0.1:${port}` };
}

async function closeServer(server) {
  await new Promise((resolve, reject) => server.close((err) => err ? reject(err) : resolve()));
}

describe('extension fetch proxy integration', () => {
  it('forwards an extension request through the dynamically allocated proxy port', async () => {
    let received = null;
    const upstream = await startUpstream(async (req, res) => {
      const chunks = [];
      for await (const chunk of req) chunks.push(Buffer.from(chunk));
      received = {
        method: req.method,
        body: Buffer.concat(chunks).toString('utf8'),
        referer: req.headers.referer,
        marker: req.headers['x-toko-test'],
      };
      res.writeHead(201, { 'content-type': 'application/json' });
      res.end(JSON.stringify({ ok: true }));
    });
    // The fixture upstream listens on 127.0.0.1, which the proxy refuses by default.
    const proxy = new LocalProxyServer(logger, undefined, { allowPrivateTargets: true });

    try {
      const proxyBaseUrl = await proxy.ensureStarted();
      const response = await fetch(`${proxyBaseUrl}/proxy?url=${encodeURIComponent(`${upstream.origin}/search`)}`, {
        method: 'POST',
        headers: { Referer: 'https://provider.test/', 'X-Toko-Test': 'present' },
        body: 'one piece',
      });

      expect(response.status).toBe(201);
      expect(await response.json()).toEqual({ ok: true });
      expect(received).toEqual({
        method: 'POST',
        body: 'one piece',
        referer: 'https://provider.test/',
        marker: 'present',
      });
    } finally {
      await proxy.stop();
      await closeServer(upstream.server);
    }
  });

  it('gives worker-injected fetch the running proxy URL instead of localhost:9001', async () => {
    const upstream = await startUpstream((_req, res) => {
      res.writeHead(200, { 'content-type': 'text/plain' });
      res.end('https://fixture.test/stream.m3u8');
    });
    // The fixture upstream listens on 127.0.0.1, which the proxy refuses by default.
    const proxy = new LocalProxyServer(logger, undefined, { allowPrivateTargets: true });
    const pool = new ExtensionWorkerPool();

    try {
      pool.setFetchProxyBaseUrl(await proxy.ensureStarted());
      const extensionCode = `
        module.exports = class FixtureExtension {
          async single() {
            const response = await __tatakai_fetch__('${upstream.origin}/source');
            if (!response.ok) return [];
            return [{ url: await response.text(), source: 'fixture' }];
          }
        };
      `;
      const manifest = { permissions: ['network:domain:127.0.0.1'] };

      await pool.getOrSpawn('fixture-extension', extensionCode, manifest);
      const result = await pool.invoke('fixture-extension', 'single', [{}]);
      expect(result).toEqual([
        { url: 'https://fixture.test/stream.m3u8', source: 'fixture' },
      ]);
    } finally {
      pool.terminateAll();
      await proxy.stop();
      await closeServer(upstream.server);
    }
  });
});
