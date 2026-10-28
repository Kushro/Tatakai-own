import { afterAll, beforeAll, describe, expect, it } from 'bun:test';
import { createRequire } from 'module';
import http from 'node:http';
import { mkdtempSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

const require = createRequire(import.meta.url);
const { isPrivateAddress, assertPublicTarget, fetchPublicTarget } = require('../desktop/runtime/proxy/target-policy.cjs');
const { isHostAllowed } = require('../desktop/runtime/extension/extension-sandbox.cjs');
const { LocalProxyServer } = require('../desktop/runtime/proxy/local-proxy-server.cjs');
const { ExtensionWorkerPool } = require('../desktop/runtime/extension/extension-worker-pool.cjs');
const { createHostApp } = require('../desktop/runtime/extension-api-host/host-server.cjs');

const logger = { info() {}, warn() {}, error() {} };

// Other test files replace globalThis.fetch without restoring it; these tests
// need the real network stack on both the client and the proxy side.
let previousFetch;
beforeAll(() => { previousFetch = globalThis.fetch; globalThis.fetch = Bun.fetch; });
afterAll(() => { globalThis.fetch = previousFetch; });

async function startServer(handler) {
  const server = http.createServer(handler);
  await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
  return { server, origin: `http://127.0.0.1:${server.address().port}` };
}

const closeServer = (server) => new Promise((resolve) => server.close(() => resolve()));

describe('isHostAllowed', () => {
  it('matches exact hosts, *.subdomain patterns and the * wildcard', () => {
    expect(isHostAllowed('nyaa.si', ['nyaa.si'])).toBe(true);
    expect(isHostAllowed('NYAA.SI', ['nyaa.si'])).toBe(true);
    expect(isHostAllowed('sub.nyaa.si', ['nyaa.si'])).toBe(false);
    expect(isHostAllowed('sub.nyaa.si', ['*.nyaa.si'])).toBe(true);
    expect(isHostAllowed('nyaa.si', ['*.nyaa.si'])).toBe(false);
    expect(isHostAllowed('evilnyaa.si', ['*.nyaa.si'])).toBe(false);
    expect(isHostAllowed('anything.test', ['*'])).toBe(true);
    expect(isHostAllowed('nyaa.si', [])).toBe(false);
    expect(isHostAllowed('', ['*'])).toBe(false);
  });
});

describe('target policy', () => {
  it('classifies private, loopback, link-local and mapped addresses', () => {
    for (const ip of ['127.0.0.1', '10.1.2.3', '172.20.0.1', '192.168.1.1', '169.254.169.254', '100.64.0.1', '0.0.0.0', '::1', 'fd00::1', 'fe80::1', '::ffff:10.0.0.1', '[::1]']) {
      expect(isPrivateAddress(ip)).toBe(true);
    }
    for (const ip of ['1.1.1.1', '8.8.8.8', '2606:4700:4700::1111', '::ffff:1.1.1.1']) {
      expect(isPrivateAddress(ip)).toBe(false);
    }
  });

  it('refuses hostnames that resolve to a private address and allows trusted origins', async () => {
    const lookup = async (host) => [{ address: host === 'rebind.test' ? '10.0.0.5' : '93.184.216.34', family: 4 }];
    await expect(assertPublicTarget('https://rebind.test/x', { lookup })).rejects.toThrow('private address');
    await expect(assertPublicTarget('https://public.test/x', { lookup })).resolves.toBeUndefined();
    await expect(assertPublicTarget('http://localhost:8080/', { lookup })).rejects.toThrow('local address');
    await expect(assertPublicTarget('file:///etc/passwd', { lookup })).rejects.toThrow('protocol');
    await expect(assertPublicTarget('http://127.0.0.1:8888/webtorrent/x', { lookup, trustedOrigins: ['http://127.0.0.1:8888'] })).resolves.toBeUndefined();
  });

  it('checks every redirect hop and strips credentials across origins', async () => {
    const lookup = async () => [{ address: '93.184.216.34', family: 4 }];
    const seen = [];
    const fetchImpl = async (url, init) => {
      seen.push({ url: String(url), cookie: init.headers.get('cookie'), method: init.method });
      if (String(url).startsWith('https://a.test/')) return new Response(null, { status: 302, headers: { location: 'https://b.test/next' } });
      if (String(url).startsWith('https://b.test/')) return new Response(null, { status: 303, headers: { location: 'http://169.254.169.254/latest' } });
      return new Response('metadata');
    };
    await expect(fetchPublicTarget('https://a.test/start', { method: 'POST', body: 'x', headers: { cookie: 'sid=1' } }, { lookup, fetchImpl })).rejects.toThrow('private address');
    expect(seen).toEqual([
      { url: 'https://a.test/start', cookie: 'sid=1', method: 'POST' },
      { url: 'https://b.test/next', cookie: null, method: 'GET' },
    ]);
  });
});

describe('LocalProxyServer private-target policy', () => {
  it('refuses /proxy and /stream targets on loopback without contacting them', async () => {
    let hits = 0;
    const upstream = await startServer((_req, res) => { hits += 1; res.end('secret'); });
    const proxy = new LocalProxyServer(logger);
    try {
      const base = await proxy.ensureStarted();
      const direct = await fetch(`${base}/proxy?url=${encodeURIComponent(`${upstream.origin}/admin`)}`);
      expect(direct.status).toBe(403);
      const streamUrl = proxy.registerSource({ url: `${upstream.origin}/video.mp4`, headers: {} });
      expect((await fetch(streamUrl)).status).toBe(403);
      expect(hits).toBe(0);
    } finally {
      await proxy.stop();
      await closeServer(upstream.server);
    }
  });

  it('still serves app-owned loopback origins passed as trusted', async () => {
    const upstream = await startServer((_req, res) => res.end('torrent bytes'));
    const proxy = new LocalProxyServer(logger, undefined, { trustedLocalOrigins: [upstream.origin] });
    try {
      await proxy.ensureStarted();
      const streamUrl = proxy.registerSource({ url: `${upstream.origin}/webtorrent/x.mp4`, headers: {} });
      const response = await fetch(streamUrl);
      expect(response.status).toBe(200);
      expect(await response.text()).toBe('torrent bytes');
    } finally {
      await proxy.stop();
      await closeServer(upstream.server);
    }
  });
});

describe('worker network allowlist', () => {
  it('gates plain fetch() as well as __tatakai_fetch__', async () => {
    const upstream = await startServer((_req, res) => res.end('ok'));
    const proxy = new LocalProxyServer(logger, undefined, { allowPrivateTargets: true });
    const pool = new ExtensionWorkerPool();
    try {
      pool.setFetchProxyBaseUrl(await proxy.ensureStarted());
      const extensionCode = `
        module.exports = class {
          async probe(url) {
            try { const r = await fetch(url); return 'ok:' + (await r.text()); }
            catch (e) { return e.name; }
          }
        };
      `;
      await pool.getOrSpawn('net-policy', extensionCode, { permissions: ['network:domain:127.0.0.1'] });
      expect(await pool.invoke('net-policy', 'probe', [`${upstream.origin}/x`])).toBe('ok:ok');
      expect(await pool.invoke('net-policy', 'probe', ['https://undeclared.test/'])).toBe('PermissionDeniedError');
    } finally {
      pool.terminateAll();
      await proxy.stop();
      await closeServer(upstream.server);
    }
  });
});

describe('in-process host network allowlist', () => {
  it("applies the calling extension's permissions to __tatakai_fetch__", async () => {
    // The shim installs once per process; start from a clean global so this
    // test exercises the host's version.
    delete globalThis.__tatakai_fetch__;
    const dir = mkdtempSync(join(tmpdir(), 'host-net-policy-'));
    const bundlePath = join(dir, 'bundle.cjs');
    writeFileSync(bundlePath, `module.exports = {
      getNovelChapters: async () => [],
      getNovelContent: async ({ chapterKey }) => {
        const url = decodeURIComponent(chapterKey.slice('fixture:'.length));
        try { await globalThis.__tatakai_fetch__(url); return { paragraphs: [{ index: 0, text: 'fetched' }] }; }
        catch (e) { return { paragraphs: [{ index: 0, text: e.name }] }; }
      },
    };`);
    const entry = { bundlePath, manifest: { id: 'netpolicy', permissions: ['network:domain:127.0.0.1'], apiServer: { namespace: 'netpolicy', routes: ['novel/content'] }, capabilities: ['novel'] } };
    const host = createHostApp({ registry: { listLoaded: () => ['netpolicy'], lookup: () => entry }, logger });
    const server = http.createServer(host.handler);
    await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
    const base = `http://127.0.0.1:${server.address().port}/api/v3/netpolicy/novel/content?chapterKey=`;
    const probe = async (url) => (await (await fetch(base + encodeURIComponent(`fixture:${encodeURIComponent(url)}`))).json()).paragraphs[0].text;
    try {
      expect(await probe('https://undeclared.test/')).toBe('PermissionDeniedError');
      // Declared, but loopback: the allowlist passes and the private-address policy refuses it.
      expect(await probe('http://127.0.0.1:9/')).toBe('BlockedTargetError');
      // Outside any bundle scope there are no permissions to grant.
      await expect(globalThis.__tatakai_fetch__('https://undeclared.test/')).rejects.toThrow('PermissionDeniedError');
    } finally {
      await closeServer(server);
    }
  });
});
