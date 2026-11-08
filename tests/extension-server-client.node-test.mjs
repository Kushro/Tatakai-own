// `node --test tests/extension-server-client.node-test.mjs` (Node >= 22.18 strips TS types;
// bun is not required). Mirrors tests/extension-server-client.test.ts.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { registerHooks } from 'node:module';

// src/ uses extensionless relative imports (bundler resolution): retry them as `.ts`.
registerHooks({
  resolve(specifier, context, next) {
    try { return next(specifier, context); } catch (error) {
      if (/^\.\.?\//.test(specifier) && !/\.\w+$/.test(specifier)) return next(specifier + '.ts', context);
      throw error;
    }
  },
});
const { isAllowedServerUrl, assertAllowedServerUrl } = await import('../src/core/extensions/server/hostGuard.ts');
const { normalizeServerOrigin, parsePairingInput, parseServerLink } = await import('../src/core/extensions/server/pairingLink.ts');

test('host guard: https anywhere, http only on the local network', () => {
  for (const url of ['https://example.com', 'https://8.8.8.8:8443', 'http://127.0.0.1:8090', 'http://localhost', 'http://pc.localhost', 'http://[::1]:8090',
    'http://10.0.0.5', 'http://172.16.0.1', 'http://172.31.255.255', 'http://192.168.1.20:8090', 'http://169.254.10.1', 'http://[fe80::1]',
    'http://100.64.0.1', 'http://100.127.255.254', 'http://[fd7a:115c:a1e0::1]:8090', 'http://[fc00::1]', 'http://mipc.local:8090']) assert.equal(isAllowedServerUrl(url), true, url);
  for (const url of ['http://example.com', 'http://8.8.8.8', 'http://172.32.0.1', 'http://172.15.0.1', 'http://100.128.0.1', 'http://100.63.0.1', 'http://mipc',
    'http://[2001:db8::1]', 'http://local', 'http://user:pass@192.168.1.2', 'https://user@example.com', 'ftp://192.168.1.2', 'tatakai://extensionserver/connect', 'not a url', '']) assert.equal(isAllowedServerUrl(url), false, url);
  assert.throws(() => assertAllowedServerUrl('http://example.com'), /insecure_host/);
});

test('pairing links: canonical, compatibility formats and validation', () => {
  const server = 'http://192.168.1.20:8090';
  const deepLink = `tatakai://extensionserver/connect?server=${encodeURIComponent(server)}&code=Ab_9-x`;
  assert.deepEqual(parseServerLink(deepLink), { server, code: 'Ab_9-x' });
  assert.deepEqual(parseServerLink(`/extensionserver/connect?server=${encodeURIComponent(server)}&code=Ab_9-x`), { server, code: 'Ab_9-x' });
  assert.deepEqual(parseServerLink(`https://tatakai.me/connect-server?server=${encodeURIComponent(server)}&code=abc`), { server, code: 'abc' });
  assert.deepEqual(parseServerLink(`/connect-server?server=${encodeURIComponent(server)}`), { server });
  assert.deepEqual(parseServerLink(`/extensionserver/${encodeURIComponent(server)}?pass=secret`), { server, password: 'secret' });
  assert.deepEqual(parseServerLink(`${server}/server/`), { server });
  assert.throws(() => parseServerLink(`tatakai://extensionserver/connect?server=${encodeURIComponent(server)}`), /invalid_link/);
  assert.throws(() => parseServerLink(`tatakai://extensionserver/connect?server=${encodeURIComponent(server)}&code=bad%20code`), /invalid_link/);
  assert.throws(() => parseServerLink(`tatakai://extensionserver/connect?server=${encodeURIComponent(server + '/path')}&code=abc`), /invalid_link/);
  assert.throws(() => parseServerLink(`tatakai://extensionserver/connect?server=${encodeURIComponent('http://example.com')}&code=abc`), /insecure_host/);
  assert.throws(() => parseServerLink('https://evil.example/connect-server?server=x&code=y'), /invalid_link/);
  assert.deepEqual(parsePairingInput('Ab_9-x', '192.168.1.20:8090'), { server, code: 'Ab_9-x' });
  assert.throws(() => parsePairingInput('Ab_9-x', ''), /invalid_link/);
  assert.equal(normalizeServerOrigin('192.168.1.20:8090'), server);
  assert.equal(normalizeServerOrigin('pc.example.com'), 'https://pc.example.com');
  assert.throws(() => normalizeServerOrigin('http://pc.example.com'), /insecure_host/);
  assert.throws(() => normalizeServerOrigin('http://user:pw@192.168.1.20'), /invalid_url/);
});
