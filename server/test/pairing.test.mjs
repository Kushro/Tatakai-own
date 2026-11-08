import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createServer, API, defaults, pairingDeepLink } from '../src/core.mjs';
import { memoryStore } from '../src/storage.mjs';
import { vault } from '../src/security.mjs';
// These tests use only the Stremio-free core (no interpreter), so they run without `npm run server:build`.
const secret = 'test-secret-with-at-least-thirty-two-characters';
const bootstrap = 'bootstrap-for-tests';
const viewerPassword = 'viewer-test-password';
async function fixture({ platform = 'node', requireDeviceToken = false, setup = { viewerPassword } } = {}) {
  const store = memoryStore();
  const app = await createServer({ store, artifacts: {}, secret, bootstrapSecret: bootstrap, platform, requireDeviceToken });
  const request = (path, { method = 'GET', data, token, headers = {}, origin = 'http://192.168.1.20:8090' } = {}) => app(new Request(origin + API + path,
    { method, headers: { ...(token ? { authorization: `Bearer ${token}` } : {}), ...headers }, body: data === undefined ? undefined : JSON.stringify(data) }));
  const json = async (path, options) => { const response = await request(path, options); return { status: response.status, ...await response.json() }; };
  let owner;
  if (setup) { const created = await json('/setup', { method: 'POST', headers: { 'x-tatakai-bootstrap': bootstrap }, data: setup }); assert.equal(created.status, 200); owner = created.data.token; }
  return { app, store, request, json, owner };
}
const expectedLink = (origin, code) => `tatakai://extensionserver/connect?server=${encodeURIComponent(origin)}&code=${encodeURIComponent(code)}`;

test('POST /auth/pairing returns code, expiresIn and a tatakai:// deep link (url === deepLink)', async () => {
  const f = await fixture();
  const pair = await f.json('/auth/pairing', { method: 'POST', token: f.owner, data: {} });
  assert.equal(pair.status, 200);
  assert.deepEqual(Object.keys(pair.data).sort(), ['code', 'deepLink', 'expiresIn', 'url']);
  assert.equal(typeof pair.data.code, 'string'); assert.match(pair.data.code, /^[A-Za-z0-9_-]{1,128}$/);
  assert.equal(pair.data.expiresIn, 300);
  assert.equal(pair.data.deepLink, expectedLink('http://192.168.1.20:8090', pair.data.code));
  assert.equal(pair.data.url, pair.data.deepLink);
  assert.ok(!JSON.stringify(pair.data).includes('tatakai.me'));
  assert.equal(pairingDeepLink('https://a.example', 'c d'), 'tatakai://extensionserver/connect?server=https%3A%2F%2Fa.example&code=c%20d');
  const parsed = new URL(pair.data.deepLink);
  assert.equal(parsed.protocol, 'tatakai:'); assert.equal(parsed.host, 'extensionserver'); assert.equal(parsed.pathname, '/connect');
  assert.equal(parsed.searchParams.get('server'), 'http://192.168.1.20:8090'); assert.equal(parsed.searchParams.get('code'), pair.data.code);
});

test('pairing requires owner access', async () => {
  const f = await fixture();
  const viewer = (await f.json('/auth/login', { method: 'POST', data: { password: viewerPassword } })).data.token;
  assert.equal((await f.json('/auth/pairing', { method: 'POST', data: {} })).status, 401);
  assert.equal((await f.json('/auth/pairing', { method: 'POST', token: viewer, data: {} })).status, 403);
});

test('pairing codes are one-use, expire and are capped at 50 live codes', async () => {
  const f = await fixture();
  const pair = (await f.json('/auth/pairing', { method: 'POST', token: f.owner, data: {} })).data;
  const paired = await f.json('/auth/pair', { method: 'POST', data: { code: pair.code, label: 'Phone' } });
  assert.equal(paired.status, 200); assert.equal(paired.data.role, 'viewer'); assert.ok(paired.data.token); assert.ok(paired.data.deviceId);
  const again = await f.json('/auth/pair', { method: 'POST', data: { code: pair.code } });
  assert.equal(again.status, 401); assert.equal(again.error.code, 'pairing_expired');
  assert.equal((await f.json('/auth/pair', { method: 'POST', data: { code: 'unknown' } })).error.code, 'pairing_expired');
  const stale = (await f.json('/auth/pairing', { method: 'POST', token: f.owner, data: {} })).data;
  await f.store.mutate(state => { for (const key of Object.keys(state.pairs)) state.pairs[key] = Date.now() - 1; return { state }; });
  const expired = await f.json('/auth/pair', { method: 'POST', data: { code: stale.code } });
  assert.equal(expired.status, 401); assert.equal(expired.error.code, 'pairing_expired');
  for (let i = 0; i < 50; i++) assert.equal((await f.json('/auth/pairing', { method: 'POST', token: f.owner, data: {} })).status, 200);
  const limited = await f.json('/auth/pairing', { method: 'POST', token: f.owner, data: {} });
  assert.equal(limited.status, 409); assert.equal(limited.error.code, 'pairing_limit');
  await assert.rejects(f.app.createPairing('http://192.168.1.20:8090'), error => error.status === 409 && error.code === 'pairing_limit');
});

test('handle.createPairing creates a one-use code for the hosting process', async () => {
  const f = await fixture();
  assert.equal(typeof f.app.createPairing, 'function');
  const created = await f.app.createPairing('http://10.0.0.5:8090');
  assert.deepEqual(Object.keys(created).sort(), ['code', 'deepLink', 'expiresIn']);
  assert.equal(created.expiresIn, 300);
  assert.equal(created.deepLink, expectedLink('http://10.0.0.5:8090', created.code));
  const paired = await f.json('/auth/pair', { method: 'POST', data: { code: created.code } });
  assert.equal(paired.status, 200);
  assert.equal((await f.json('/sources', { token: paired.data.token })).status, 200);
  assert.equal((await f.json('/auth/pair', { method: 'POST', data: { code: created.code } })).status, 401);
  const remote = await f.app.createPairing('https://tunnel.example.com');
  assert.equal(new URL(remote.deepLink).searchParams.get('server'), 'https://tunnel.example.com');
});

test('handle.createPairing validates the origin and requires setup', async () => {
  const f = await fixture();
  for (const origin of ['http://10.0.0.5:8090/', 'http://10.0.0.5:8090/path', 'not a url', '', undefined, 42, 'ftp://10.0.0.5', 'app://tatakai.me', 'http://user:pass@10.0.0.5'])
    await assert.rejects(f.app.createPairing(origin), error => error.status === 400 && error.code === 'invalid_origin', String(origin));
  const fresh = await fixture({ setup: null });
  await assert.rejects(fresh.app.createPairing('http://10.0.0.5:8090'), error => error.status === 409 && error.code === 'setup_required');
});

test('requireDeviceToken disables anonymous access until a device pairs', async () => {
  const f = await fixture({ platform: 'desktop', requireDeviceToken: true, setup: {} });
  const health = (await f.json('/health')).data;
  assert.equal(health.pairingRequired, true); assert.equal(health.accessPasswordRequired, false);
  assert.equal(health.dashboardPath, '/server/'); assert.equal(health.desktop, true); assert.equal(health.protocolVersion, 1);
  const anonymous = await f.json('/sources');
  assert.equal(anonymous.status, 401); assert.equal(anonymous.error.code, 'authentication_required');
  const login = await f.json('/auth/login', { method: 'POST', data: { role: 'viewer' } });
  assert.equal(login.status, 401); assert.equal(login.error.code, 'pairing_required');
  // Public media tickets minted by a previous, open configuration are not honoured.
  const state = await f.store.read(), encryption = await vault(secret);
  const ticket = await encryption.seal({ url: 'https://cdn.example/file.mp4', headers: {}, sessionId: `public:${state.publicEpoch || state.id}`, expires: Date.now() + 60000 });
  assert.equal((await f.request('/media/' + ticket)).status, 401);
  const { code } = await f.app.createPairing('http://192.168.1.20:8090');
  const paired = await f.json('/auth/pair', { method: 'POST', data: { code, label: 'Phone' } });
  assert.equal(paired.status, 200);
  const sources = await f.json('/sources', { token: paired.data.token });
  assert.equal(sources.status, 200); assert.deepEqual(sources.data, []);
  // A viewer password keeps working as an alternative to pairing.
  assert.equal((await f.json('/auth/password', { method: 'POST', token: f.owner, data: { role: 'viewer', password: viewerPassword } })).status, 200);
  assert.equal((await f.json('/health')).data.pairingRequired, false);
  assert.equal((await f.json('/auth/login', { method: 'POST', data: { role: 'viewer', password: viewerPassword } })).status, 200);
  assert.equal((await f.json('/sources')).status, 401);
});

test('without requireDeviceToken the standalone server keeps public viewer access', async () => {
  const f = await fixture({ setup: {} });
  assert.equal((await f.json('/health')).data.pairingRequired, false);
  assert.equal((await f.json('/sources')).status, 200);
  assert.equal((await f.json('/auth/login', { method: 'POST', data: { role: 'viewer' } })).status, 200);
});

test('default allowed origins keep the APK WebView, Electron and native shells and drop www', async () => {
  for (const origin of ['https://tatakai.me', 'app://tatakai.me', 'capacitor://localhost', 'http://localhost', 'https://localhost']) assert.ok(defaults.allowedOrigins.includes(origin), origin);
  assert.ok(!defaults.allowedOrigins.includes('https://www.tatakai.me'));
  const f = await fixture();
  for (const origin of defaults.allowedOrigins) {
    const response = await f.request('/health', { headers: { origin } });
    assert.equal(response.status, 200, origin); assert.equal(response.headers.get('access-control-allow-origin'), origin);
  }
  for (const origin of ['https://www.tatakai.me', 'https://evil.example', 'null']) assert.equal((await f.request('/health', { headers: { origin } })).status, 403, origin);
  // Same-origin requests (the dashboard under /server/) never need an allow-list entry.
  assert.equal((await f.request('/health', { headers: { origin: 'http://192.168.1.20:8090' } })).status, 200);
  assert.deepEqual((await f.json('/settings', { token: f.owner })).data.allowedOrigins, defaults.allowedOrigins);
});

test('only node and desktop platforms are supported', async () => {
  for (const platform of ['vercel', 'workers']) await assert.rejects(createServer({ store: memoryStore(), artifacts: {}, secret, platform }), /Unsupported server platform/);
  const f = await fixture();
  const health = (await f.json('/health')).data;
  assert.equal(health.platform, 'node'); assert.equal(health.proxy.maxResponseBytes, null);
  assert.ok(Number.isInteger(health.uptimeSeconds)); assert.equal(typeof health.startedAt, 'string');
});
