import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createServer, API } from '../src/core.mjs';
import { memoryStore } from '../src/storage.mjs';
import { createNetwork, publicUrl, vault, digest } from '../src/security.mjs';
import { interpreter } from '../dist/interpreter.mjs';
import { nodeInterpreter } from '../src/node-extension.mjs';
const secret = 'test-secret-with-at-least-thirty-two-characters';
const bootstrap = 'bootstrap-for-tests';
const password = 'viewer-test-password', adminPassword = 'owner-test-password';
const manifest = { id: 'fixture', name: 'Fixture', version: '1.0', resources: ['stream', 'catalog', 'meta', 'subtitles'], types: ['series'], catalogs: [{ id: 'fixture', type: 'series' }] };
async function fixture(platform = 'node') {
  const store = memoryStore(), files = new Map(), calls = [];
  const artifacts = { put: async (key, value) => files.set(key, value), get: async key => files.get(key), remove: async key => files.delete(key) };
  const network = createNetwork({ fetch: async (url, init) => {
    calls.push({ url, init });
    if (url.endsWith('/manifest.json')) return Response.json(manifest);
    if (url.includes('/catalog/')) return Response.json({ metas: [{ id: 'fixture:show', name: 'Exact title' }] });
    if (url.includes('/meta/')) return Response.json({ meta: { name: 'Exact title', videos: [{ id: 'fixture:episode:1', episode: 1, season: 1 }] } });
    if (url.includes('/stream/')) return Response.json({ streams: [{ name: 'Fixture stream', url: 'https://cdn.example/master.m3u8', behaviorHints: { proxyHeaders: { request: { referer: 'https://addon.example/', cookie: 'upstream-secret' } } } }] });
    if (url.includes('/subtitles/')) return Response.json({ subtitles: [{ url: 'https://cdn.example/sub.vtt', lang: 'en' }] });
    if (url.endsWith('/master.m3u8')) return new Response('#EXTM3U\n#EXT-X-KEY:METHOD=AES-128,URI="key.bin"\n#EXT-X-MAP:URI="init.mp4"\n#EXT-X-MEDIA:TYPE=SUBTITLES,URI="sub.m3u8"\nsegment.ts\nhttps://other.example/segment.ts', { headers: { 'content-type': 'application/vnd.apple.mpegurl' } });
    if (url.includes('redirect.example')) return new Response(null, { status: 302, headers: { location: 'http://127.0.0.1/private' } });
    return new Response('media bytes', { status: init.headers.range ? 206 : 200, headers: { 'content-type': 'video/mp2t', 'content-length': '11', ...(init.headers.range ? { 'content-range': 'bytes 0-10/11' } : {}) } });
  } });
  const options = { store, artifacts, secret, bootstrapSecret: bootstrap, platform, network, interpreter, nodeInterpreter: ['node','desktop'].includes(platform) ? nodeInterpreter : undefined };
  let app = await createServer(options);
  async function request(path, { method = 'GET', data, token, headers = {} } = {}) {
    return app(new Request('https://server.example' + API + path, { method, headers: { ...(token ? { authorization: `Bearer ${token}` } : {}), ...headers }, body: data === undefined ? undefined : JSON.stringify(data) }));
  }
  const json = async (path, options) => { const response = await request(path, options); const value = await response.json(); return { status: response.status, ...value }; };
  const setup = await json('/setup', { method: 'POST', headers: { 'x-tatakai-bootstrap': bootstrap }, data: { adminPassword, viewerPassword: password } });
  assert.equal(setup.status, 200); const owner = setup.data.token;
  const login = await json('/auth/login', { method: 'POST', data: { password, label: 'Test viewer' } }); const viewer = login.data.token;
  return { store, calls, request, json, owner, viewer, files, handler: request => app(request), coldStart: async () => { app = await createServer(options); } };
}
test('public health advertises validated owner artwork and runtime uptime without credentials', async () => {
  const f = await fixture();
  const changed = await f.json('/settings', {method:'PATCH', token:f.owner, data:{name:'Community Source',logoUrl:'https://images.example/logo.png'}});
  assert.equal(changed.status,200);
  const health = (await f.json('/health')).data;
  assert.equal(health.name,'Community Source');
  assert.equal(health.logoUrl,'https://images.example/logo.png');
  assert.ok(Number.isInteger(health.uptimeSeconds) && health.uptimeSeconds >= 0);
  assert.equal(health.token,undefined);
  assert.equal((await f.json('/settings',{method:'PATCH',token:f.owner,data:{logoUrl:'https://user:password@images.example/logo.png'}})).status,400);
  assert.equal((await f.json('/settings',{method:'PATCH',token:f.viewer,data:{logoUrl:'https://images.example/logo.png'}})).status,403);
  const serverless=await fixture('workers');
  assert.equal((await serverless.json('/health')).data.uptimeSeconds,undefined);
});
for (const platform of ['node', 'vercel', 'workers', 'desktop']) test(`${platform}: setup, auth, addon, proxy, cold start, and revocation contract`, async () => {
  const f = await fixture(platform);
  assert.equal((await f.json('/settings', { token: f.viewer })).status, 403);
  assert.equal((await f.json('/settings')).status, 401);
  assert.equal((await f.json('/setup', { method: 'POST', data: {} })).status, 409);
  const installed = await f.json('/extensions', { method: 'POST', token: f.owner, data: { url: 'https://addon.example/manifest.json' } });
  assert.equal(installed.status, 200); assert.equal(installed.data.origin, undefined);
  const sources = await f.json('/sources', { token: f.viewer }); assert.equal(sources.data.length, 1);
  const result = await f.json('/resolve', { method: 'POST', token: f.viewer, data: { title: 'Exact title', episode: 1 } });
  assert.equal(result.data.sources.length, 1); assert.equal(result.data.sources[0].headers, undefined);
  assert.equal(result.data.sources[0].extensionName, 'Fixture');
  assert.equal(result.data.sources[0].extensionServerName, 'Tatakai Server');
  assert.ok(result.data.sources[0].sourceName);
  const media = result.data.sources[0].url;
  assert.match(media, /^https:\/\/server.example\/api\/server\/v1\/media\//);
  assert.ok(!media.includes('upstream-secret')); assert.ok(!media.includes('cdn.example'));
  await f.coldStart();
  const response = await f.request('/media/' + media.split('/media/')[1]); assert.equal(response.status, 200);
  const playlist = await response.text(); assert.ok(playlist.includes('URI="https://server.example/')); assert.ok(!playlist.includes('cdn.example')); assert.ok(!playlist.includes('other.example'));
  const segmentUrl = playlist.split('\n').find(line => line.startsWith('https://'));
  const segment = await f.request('/media/' + segmentUrl.split('/media/')[1], { headers: { range: 'bytes=0-10' } });
  assert.equal(segment.status, 206); assert.equal(segment.headers.get('content-range'), 'bytes 0-10/11'); await segment.text();
  const crossUrl = playlist.trim().split('\n').at(-1); const cross = await f.request('/media/' + crossUrl.split('/media/')[1]); await cross.text();
  assert.equal(f.calls.at(-1).init.headers.cookie, undefined);
  const devices = await f.json('/devices', { token: f.owner }); const device = devices.data.find(row => row.role === 'viewer');
  await f.json('/devices/' + device.id, { method: 'DELETE', token: f.owner });
  assert.equal((await f.request('/media/' + media.split('/media/')[1])).status, 401);
  assert.equal((await f.json('/sources', { token: f.viewer })).status, 401);
});
test('setup requires bootstrap; racing setup creates one owner', async () => {
  const store = memoryStore(), options = { store, artifacts: {}, secret, bootstrapSecret: bootstrap };
  const app = await createServer(options);
  const req = headers => new Request('https://server.example' + API + '/setup', { method: 'POST', headers, body: JSON.stringify({ adminPassword, viewerPassword: password }) });
  assert.equal((await app(req({}))).status, 403);
  const results = await Promise.all([app(req({ 'x-tatakai-bootstrap': bootstrap })), app(req({ 'x-tatakai-bootstrap': bootstrap }))]);
  assert.deepEqual(results.map(row => row.status).sort(), [200, 409]);
});
test('preferences are encrypted; bad update preserves previous install', async () => {
  const f = await fixture();
  const installed = await f.json('/extensions', { method: 'POST', token: f.owner, data: { url: 'https://addon.example/manifest.json', preferences: { secret: 'private-provider-value' } } });
  assert.ok(!JSON.stringify(await f.store.read()).includes('private-provider-value'));
  const update = await f.json('/extensions/' + installed.data.id + '/update', { method: 'POST', token: f.owner, data: { url: 'http://127.0.0.1/manifest.json' } });
  assert.equal(update.status, 403); assert.equal((await f.json('/extensions', { token: f.owner })).data.length, 1);
  const exported = await f.json('/export', { token: f.owner }); assert.ok(!JSON.stringify(exported).includes('private-provider-value'));
});
test('pairing is one-use, password change revokes viewers, and origins apply to errors', async () => {
  const f = await fixture();
  const pair = await f.json('/auth/pairing', { method: 'POST', token: f.owner, data: {} });
  assert.equal((await f.json('/auth/pair', { method: 'POST', data: { code: pair.data.code } })).status, 200);
  assert.equal((await f.json('/auth/pair', { method: 'POST', data: { code: pair.data.code } })).status, 401);
  assert.equal((await f.json('/auth/password', { method: 'POST', token: f.owner, data: { role: 'viewer', currentPassword: adminPassword, password: 'changed-viewer-password' } })).status, 200);
  assert.equal((await f.json('/sources', { token: f.viewer })).status, 401);
  const error = await f.request('/settings', { headers: { origin: 'https://tatakai.me' } }); assert.equal(error.headers.get('access-control-allow-origin'), 'https://tatakai.me');
  assert.equal((await f.request('/health', { headers: { origin: 'https://evil.example' } })).status, 403);
  for (const origin of ['app://tatakai.me', 'capacitor://localhost', 'http://localhost', 'https://localhost']) {
    const native = await f.request('/health', { headers: { origin } });
    assert.equal(native.status, 200);
    assert.equal(native.headers.get('access-control-allow-origin'), origin);
    assert.equal((await f.request('/settings', { headers: { origin } })).status, 401);
  }
  assert.equal((await f.request('/health', { headers: { origin: 'null' } })).status, 403);
});
test('CAS mutations preserve concurrent edits', async () => {
  const store = memoryStore(); await store.mutate(() => ({ state: { count: 0 } }));
  await Promise.all(Array.from({ length: 8 }, () => store.mutate(state => { state.count++; return { state }; })));
  assert.equal((await store.read()).count, 8);
});
test('SSRF rejects encoded IPv4, IPv6, metadata, private DNS and redirect targets', async () => {
  for (const url of ['http://127.0.0.1/', 'http://2130706433/', 'http://0x7f000001/', 'http://[::1]/', 'http://[::ffff:127.0.0.1]/', 'http://169.254.169.254/', 'http://10.0.0.1/', 'http://metadata.google.internal/']) assert.throws(() => publicUrl(url));
  const network = createNetwork({ resolveHost: async () => ['192.168.1.1'], fetch: () => { throw Error('Must not fetch'); } });
  await assert.rejects(network('https://public.example/'), /private_upstream/);
  const redirect = createNetwork({ fetch: async () => new Response(null, { status: 302, headers: { location: 'http://localhost/private' } }) });
  await assert.rejects(redirect('https://public.example/'), /private_upstream/);
});
test('serverless reports native families unavailable', async () => {
  const f = await fixture('workers'); const health = await f.json('/health');
  for (const family of ['mihon', 'cloudstream', 'aniyomi', 'kodi']) assert.equal(health.data.capabilities.find(row => row.ecosystem === family).available, false);
});
test('Tatakai script executes inside interpreter and host modules are rejected', async () => {
  const f = await fixture();
  const installed = await f.json('/extensions', { method: 'POST', token: f.owner, data: { manifest: { id: 'script', name: 'Script', version: '1' }, code: 'module.exports = { async single(options) { return [{url:"https://cdn.example/video.mp4",quality:options.resolution}]; } }' } });
  assert.equal(installed.status, 200, JSON.stringify(installed));
  const result = await f.json('/resolve', { method: 'POST', token: f.viewer, data: { title: 'Anything', episode: 1 } }); assert.equal(result.data.sources.length, 1);
  const bad = await f.json('/extensions', { method: 'POST', token: f.owner, data: { manifest: { id: 'bad', name: 'Bad', version: '1' }, code: 'module.exports=require("node:fs")' } }); assert.equal(bad.status, 422);
});
test('forged and expired media capabilities are rejected', async () => {
  const f = await fixture(), encryption = await vault(secret);
  assert.equal((await f.request('/media/forged')).status, 401);
  const ticket = await encryption.seal({ expires: 0, url: 'https://cdn.example/file' });
  assert.equal((await f.request('/media/' + ticket)).status, 410);
});

for (const platform of ['node', 'vercel', 'workers', 'desktop']) test(`${platform}: novel discovery and chapter text use the declared novel capability`, async () => {
  const f = await fixture(platform);
  const installed = await f.json('/extensions', { method: 'POST', token: f.owner, data: {
    manifest: { id: 'novel-fixture', name: 'Novel Fixture', version: '1', capabilities: ['novel'] },
    code: 'module.exports = { getNovelChapters: async () => [{number:1,title:"Opening",sources:[{provider:"translator",chapterKey:"translator:chapter-1",providerChapterId:"chapter-1"}]}], getNovelContent: async (args) => { if(args.provider!=="translator" || args.chapterKey!=="translator:chapter-1") throw Error("Wrong chapter"); return {title:"Opening",number:1,paragraphs:[{index:0,text:"Fixture chapter text."}]}; } }',
  } });
  assert.equal(installed.status, 200, JSON.stringify(installed));
  const sources = (await f.json('/sources', { token: f.viewer })).data;
  const novel = sources.find(source => source.kind === 'novel'); assert.ok(novel); assert.match(novel.id, /~novel$/);
  const chapters = await f.json('/invoke', { method: 'POST', token: f.viewer, data: { sourceId: novel.id, op: 'novelChapters', args: { anilistId: 98361 } } });
  assert.equal(chapters.status, 200, JSON.stringify(chapters)); assert.equal(chapters.data[0].number, 1);
  await f.coldStart();
  const content = await f.json('/invoke', { method: 'POST', token: f.viewer, data: { sourceId: novel.id, op: 'novelContent', args: chapters.data[0].sources[0] } });
  assert.equal(content.status, 200, JSON.stringify(content)); assert.equal(content.data.paragraphs[0].text, 'Fixture chapter text.');
  const noNovel = await f.json('/extensions', { method: 'POST', token: f.owner, data: { manifest: { id: 'watch-only', name: 'Watch', version: '1' }, code: 'module.exports={single:async()=>[]}' } });
  const rejected = await f.json('/invoke', { method: 'POST', token: f.viewer, data: { sourceId: noNovel.data.id, op: 'novelContent', args: {} } });
  assert.equal(rejected.status, 422);
});
test('lookup cache survives cold starts, is encrypted, and clears from owner controls', async () => {
  const f = await fixture();
  await f.json('/extensions', { method: 'POST', token: f.owner, data: { url: 'https://addon.example/manifest.json' } });
  const source = (await f.json('/sources', { token: f.viewer })).data[0];
  const invoke = () => f.json('/invoke', { method: 'POST', token: f.viewer, data: { sourceId: source.id, op: 'search', args: { query: 'Exact title' } } });
  await invoke(); await f.coldStart(); await invoke();
  assert.equal(f.calls.filter(call => call.url.includes('/catalog/')).length, 1);
  assert.ok(!JSON.stringify(await f.store.read()).includes('Exact title'));
  assert.equal((await f.json('/cache', { token: f.viewer })).status, 403);
  assert.equal((await f.json('/cache', { token: f.owner })).data.lookupEntries, 1);
  await f.json('/cache', { method: 'DELETE', token: f.owner }); await invoke();
  assert.equal(f.calls.filter(call => call.url.includes('/catalog/')).length, 2);
});
test('media cache isolates credentials and checks revocation before cache hits', async () => {
  const f = await fixture(), encryption = await vault(secret);
  const ticket = async cookie => encryption.seal({ url: 'https://cdn.example/image.png', headers: { cookie }, sessionId: await digest(f.viewer), expires: Date.now() + 60000 });
  const first = await ticket('first-cookie'), other = await ticket('other-cookie');
  const initial = await f.request('/media/' + first); await initial.text(); assert.equal(initial.headers.get('x-tatakai-cache'), 'MISS');
  await f.coldStart();
  const hit = await f.request('/media/' + first); await hit.text(); assert.equal(hit.headers.get('x-tatakai-cache'), 'HIT');
  const isolated = await f.request('/media/' + other); await isolated.text(); assert.equal(isolated.headers.get('x-tatakai-cache'), 'MISS');
  assert.equal(f.calls.filter(call => call.url.endsWith('/image.png')).length, 2);
  assert.ok([...f.files.values()].every(value => !value.includes('media bytes')));
  const stats = await f.json('/cache', { token: f.owner }); assert.equal(stats.data.mediaEntries, 2);
  const devices = await f.json('/devices', { token: f.owner });
  await f.json('/devices/' + devices.data.find(device => device.role === 'viewer').id, { method: 'DELETE', token: f.owner });
  assert.equal((await f.request('/media/' + first)).status, 401);
  await f.json('/cache', { method: 'DELETE', token: f.owner }); assert.equal((await f.json('/cache', { token: f.owner })).data.mediaBytes, 0);
});
for (const family of ['sora', 'mangayomi', 'seanime']) test(`${family}: original script adapter resolves a title and proxies playback`, async () => {
  const f = await fixture();
  const code = family === 'sora' ? `
    async function searchResults(){return [{title:'Exact title',href:'https://source.example/title'}]}
    async function extractDetails(){return {title:'Exact title'}}
    async function extractEpisodes(){return [{title:'Episode 1',href:'https://source.example/ep1',number:1}]}
    async function extractStreamUrl(){return 'https://cdn.example/video.mp4'}
  ` : family === 'mangayomi' ? `class DefaultExtension {
    getHeaders(){return {Referer:'https://source.example/'}}
    search(){return {list:[{name:'Exact title',link:'https://source.example/title'}]}}
    getDetail(){return {name:'Exact title',chapters:[{name:'Episode 1',url:'https://source.example/ep1',number:1}]}}
    getVideoList(){return [{url:'https://cdn.example/video.mp4'}]}
  }` : `class Provider {
    search(options: {query: string}){return [{title:'Exact title',id:'title1'}]}
    findEpisodes(id: string){return [{id:'episode1',number:1}]}
    getSettings(){return {episodeServers:['fixture']}}
    findEpisodeServer(){return {videoSources:[{url:'https://cdn.example/video.mp4',quality:'720p',type:'mp4'}]}}
  }`;
  const install = await f.json('/extensions', { method: 'POST', token: f.owner, data: { ecosystem: family, manifest: { id: family, name: family, version: '1', language: family === 'seanime' ? 'typescript' : 'javascript' }, code } });
  assert.equal(install.status, 200, JSON.stringify(install));
  const resolved = await f.json('/resolve', { method: 'POST', token: f.viewer, data: { title: 'Exact title', episode: 1 } });
  assert.equal(resolved.data.sources.length, 1, JSON.stringify(resolved));
  assert.match(resolved.data.sources[0].url, /\/media\//);
});
test('viewer invocation cannot change preferences or read provider secrets', async () => {
  const f = await fixture();
  for (const op of ['preferences', 'setPreference']) assert.equal((await f.json('/invoke', { method: 'POST', token: f.viewer, data: { op, sourceId: 'anything' } })).status, 403);
});

test('live logs require owner access, redact inputs, support cursors, and survive cold starts', async () => {
  const f = await fixture('workers');
  assert.equal((await f.json('/logs', { token: f.viewer })).status, 403);
  assert.equal((await f.json('/logs')).status, 401);
  await f.json('/auth/login', { method: 'POST', data: { password: 'private-login-secret?token=private-token' } });
  await f.json('/invoke', { method: 'POST', token: f.viewer, data: { sourceId: 'missing-private-source', op: 'search', args: { query: 'private-search-value' } } });
  const first = await f.json('/logs', { token: f.owner });
  assert.equal(first.status, 200); assert.ok(first.data.entries.length > 0);
  assert.ok(!JSON.stringify(first).includes('private-'));
  assert.ok(first.data.entries.some(row => row.status === 401 && row.route === '/auth/login'));
  await f.coldStart();
  const after = await f.json('/logs?after=' + first.data.cursor, { token: f.owner });
  assert.deepEqual(after.data.entries, []); assert.equal(after.data.cursor, first.data.cursor);
  const persisted = await f.json('/logs', { token: f.owner });
  assert.deepEqual(persisted.data, first.data);
});
test('large Node bundles require explicit owner selection and retain source methods', async () => {
  const f = await fixture();
  const code = 'module.exports={single(){if(process.env.SERVER_SECRET)throw Error("Deployment secret leaked");return [{url:"https://cdn.example/video.mp4"}];}};/*' + 'x'.repeat(3 * 1024 * 1024) + '*/';
  const input = { manifest: { id: 'large-native', name: 'Large native fixture', version: '1' }, code };
  assert.equal((await f.json('/extensions', { method: 'POST', token: f.owner, data: input })).status, 400);
  const installed = await f.json('/extensions', { method: 'POST', token: f.owner, data: { ...input, runtime: 'node' } });
  assert.equal(installed.status, 200, JSON.stringify(installed)); assert.equal(installed.data.runtime, 'node');
  const resolved = await f.json('/resolve', { method: 'POST', token: f.viewer, data: { title: 'Fixture' } });
  assert.equal(resolved.data.sources.length, 1, JSON.stringify(resolved));
  const worker = await fixture('workers');
  assert.equal((await worker.json('/extensions', { method: 'POST', token: worker.owner, data: { ...input, runtime: 'node' } })).status, 422);
});
test('native worker cancellation terminates hung source code', async () => {
  const started = Date.now();
  await assert.rejects(nodeInterpreter({ code: 'module.exports={single(){while(true){}}}', method: 'single', signal: AbortSignal.timeout(150), network: async () => { throw Error('Unexpected network'); } }));
  assert.ok(Date.now() - started < 3000);
  const probe = await nodeInterpreter({ code: 'module.exports={single(){return []}}', probe: true, network: async () => {} });
  assert.equal(probe.valid, true);
});

test('Node streams playable sources before extension completion and deduplicates the final batch', async () => {
  const f = await fixture();
  const installed = await f.json('/extensions', { method: 'POST', token: f.owner, data: { runtime: 'node', manifest: { id: 'progressive', name: 'Progressive', version: '1' },
    code: 'module.exports={async streamAll(args,onChunk){const first={url:"https://cdn.example/first.m3u8",source:"First provider",language:"English",sourceType:"hls"};onChunk({results:[first],providerPriority:8});await new Promise(resolve=>setTimeout(resolve,500));return[first,{url:"https://cdn.example/second.mp4",source:"Second provider",language:"Japanese",sourceType:"mp4",providerPriority:0}];}}' } });
  assert.equal(installed.status, 200, JSON.stringify(installed));
  const response = await f.request('/resolve', { method: 'POST', token: f.viewer, data: { title: 'Fixture' }, headers: { accept: 'application/x-ndjson' } });
  const reader = response.body.getReader(), decoder = new TextDecoder(); let pending = '', first, final;
  while (true) {
    const { value, done } = await reader.read(); pending += decoder.decode(value, { stream: !done });
    const lines = pending.split('\n'); pending = lines.pop();
    for (const line of lines.filter(Boolean)) {
      const batch = JSON.parse(line); final = batch;
      if (!first && batch.sources.length) first = batch;
    }
    if (done) break;
  }
  assert.equal(first.sources.length, 1); assert.equal(first.statuses[0].status, 'loading');
  assert.match(first.sources[0].url, /\/media\//); assert.equal(first.sources[0].sourceName, 'First provider');
  assert.equal(first.sources[0].providerPriority, 8);
  assert.equal(final.sources.length, 2); assert.equal(final.statuses[0].status, 'ok');
  assert.equal(final.sources[0].sourceName, 'Second provider'); assert.equal(final.sources[0].providerPriority, 0);
});
test('oversized HTTP uploads return JSON without resetting the connection', async () => {
  const { createServer: httpServer } = await import('node:http'); const { serveNodeRequest } = await import('../src/node.mjs');
  const f = await fixture();
  const server = httpServer((req,res) => serveNodeRequest(f.handler, req, res, 'http://127.0.0.1:' + server.address().port));
  await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
  try {
    const base = 'http://127.0.0.1:' + server.address().port;
    const response = await fetch(base + API + '/extensions', { method: 'POST', headers: { authorization: 'Bearer ' + f.owner, 'content-type': 'application/json' }, body: JSON.stringify({ code: 'x'.repeat(15 * 1024 * 1024) }) });
    assert.equal(response.status, 413); assert.equal((await response.json()).success, false);
    assert.equal((await fetch(base + API + '/health')).status, 200);
  } finally { server.closeAllConnections(); await new Promise(resolve => server.close(resolve)); }
});
test('public access needs no password while management still requires owner access', async () => {
  const store = memoryStore();
  const app = await createServer({ store, artifacts: {}, secret, bootstrapSecret: bootstrap });
  const request = (path, data, token, key) => app(new Request('https://public.example' + API + path, { method: data === undefined ? 'GET' : 'POST', headers: { ...(token ? { authorization: 'Bearer ' + token } : {}), ...(key ? { 'x-tatakai-bootstrap': key } : {}) }, body: data === undefined ? undefined : JSON.stringify(data) }));
  const setup = await request('/setup', {}, undefined, bootstrap); assert.equal(setup.status, 200); const owner = (await setup.json()).data.token;
  const health = (await (await request('/health')).json()).data; assert.equal(health.accessPasswordRequired, false); assert.equal(health.adminPasswordRequired, false);
  assert.equal((await request('/sources')).status, 200);
  assert.equal((await request('/extensions')).status, 401);
  assert.equal((await request('/auth/login', { role: 'viewer' })).status, 200);
  assert.equal((await request('/auth/login', { role: 'admin' })).status, 403);
  assert.equal((await request('/auth/login', { role: 'admin' }, undefined, bootstrap)).status, 200);
  assert.equal((await request('/auth/password', { role: 'viewer', password: 'new-viewer-password' }, owner)).status, 200);
  assert.equal((await request('/sources')).status, 401);
  assert.equal((await request('/auth/login', { role: 'viewer' })).status, 401);
  assert.equal((await request('/auth/password', { role: 'viewer', password: '' }, owner)).status, 200);
  assert.equal((await request('/sources')).status, 200);
});

test('hosted chapter discovery publishes early sources and retains later providers in one scan',async()=>{
 const f=await fixture('node');
 const code=`module.exports={getMangaChapters:async()=>[],streamMangaChapters:async(args,publish)=>{
  await fetch('https://addon.example/catalog/series/reading.json');
  publish([{number:1,sources:[{provider:'fast',chapterKey:'fast:1'}]}]);
  await new Promise(resolve=>setTimeout(resolve,1500));
  return [{number:1,sources:[{provider:'fast',chapterKey:'fast:1'}]},{number:2,sources:[{provider:'late',chapterKey:'late:2'}]}];
 }}`;
 const installed=await f.json('/extensions',{method:'POST',token:f.owner,data:{runtime:'node',manifest:{id:'reading',name:'Reading',version:'1',capabilities:['manga']},code}});
 assert.equal(installed.status,200,JSON.stringify(installed));
 const data={sourceId:installed.data.id,op:'chapters',args:{title:'Fixture'}};
 const first=await f.json('/invoke',{method:'POST',token:f.viewer,data:{...data,progressive:true}});
 assert.equal(first.status,200);assert.equal(first.data.partial,true);assert.ok(Array.isArray(first.data.chapters));
 const complete=await f.json('/invoke',{method:'POST',token:f.viewer,data});
 assert.equal(complete.data.length,2);
 const cached=await f.json('/invoke',{method:'POST',token:f.viewer,data:{...data,progressive:true}});
 assert.equal(cached.data.partial,false);assert.equal(cached.data.chapters[1].number,2);
 assert.equal(f.calls.filter(call=>call.url.endsWith('/reading.json')).length,1);
});
