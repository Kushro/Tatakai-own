import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync, readdirSync, existsSync } from 'node:fs';
import { createRequire } from 'node:module';
import { inflateRawSync } from 'node:zlib';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import vm from 'node:vm';
import { analyzeBundle } from '../extension-host/compat.mjs';
import { buildExpected } from '../fixtures/metadata/gen-expected.mjs';
import { buildKai, fixtureNames } from '../fixtures/extensions/build.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const contracts = resolve(here, '..');
const repo = resolve(contracts, '..');
const readJson = (...p) => JSON.parse(readFileSync(join(contracts, ...p), 'utf8'));
const text = (p) => readFileSync(p, 'utf8').replaceAll('\r\n', '\n');

// ---------------------------------------------------------------- Link v1 OpenAPI
test('link/openapi.v1.json: M0 scope (E6/E7 trims) and structural integrity', () => {
  const doc = readJson('link', 'openapi.v1.json');
  assert.equal(doc.openapi, '3.1.0');
  const paths = Object.keys(doc.paths);
  for (const p of ['/v1/pairing', '/v1/capabilities', '/v1/library', '/v1/devices/self', '/v1/sessions', '/v1/sessions/{id}', '/v1/sessions/{id}/position',
    '/v1/sessions/{id}/master.m3u8', '/v1/sessions/{id}/media.m3u8', '/v1/sessions/{id}/init.mp4', '/v1/sessions/{id}/seg_{k}.m4s']) {
    assert.ok(paths.includes(p), `missing path ${p}`);
  }
  for (const gone of ['settings', 'events', 'handoff', 'control', 'sync', 'downloads', 'source/', 'subtitles', 'mdns']) {
    assert.ok(!paths.some((p) => p.includes(gone)), `${gone} must not be in the M0 API`);
  }
  // mTLS everywhere except pairing
  for (const [path, item] of Object.entries(doc.paths)) {
    for (const [method, op] of Object.entries(item)) {
      if (path === '/v1/pairing') assert.deepEqual(op.security, [], 'pairing is TLS without client cert');
      else assert.deepEqual(op.security, [{ mutualTls: [] }], `${method} ${path} must require mTLS`);
    }
  }
  // all $refs resolve
  const refs = [];
  JSON.stringify(doc, (k, v) => { if (k === '$ref') refs.push(v); return v; });
  assert.ok(refs.length > 20);
  for (const ref of refs) {
    const target = ref.replace(/^#\//, '').split('/').reduce((o, key) => o?.[key], doc);
    assert.ok(target, `unresolved $ref ${ref}`);
  }
  // error catalog: 23 Temple codes + 3 Link additions, unique, fixed messages
  const codes = doc['x-errors'].codes;
  assert.equal(codes.length, 26);
  assert.equal(new Set(codes.map((c) => c.code)).size, 26);
  for (const added of ['url_source_denied', 'extension_unavailable', 'handoff_rejected']) assert.ok(codes.some((c) => c.code === added));
  assert.equal(codes.find((c) => c.code === 'url_source_denied').http, 403);
  assert.equal(codes.find((c) => c.code === 'extension_unavailable').http, 422);
  assert.equal(codes.find((c) => c.code === 'handoff_rejected').http, 409);
  assert.deepEqual(doc.components.schemas.Error.properties.error.properties.code.enum, codes.map((c) => c.code));
  // constants from 03
  assert.equal(doc['x-transport'].defaultPort, 47321);
  assert.equal(doc['x-pairing-payload'].scheme, 'tatakai-link:v1');
  assert.ok(doc['x-ffmpeg'].protocolWhitelist.startsWith('http,tcp,crypto'));
  assert.deepEqual(doc.components.schemas.Profile.properties.id.enum, ['p-none', 'p-anime4k-1080', 'p-anime4k-1440', 'p-anime4k-2160', 'p-anime4k-1080-fg2x']);
  assert.equal(doc.components.schemas.SessionCreated.properties.heartbeat_interval_secs.const, 4);
  assert.equal(doc.components.schemas.SessionCreate.properties.source.discriminator.propertyName, 'kind');
});

// ---------------------------------------------------------------- extension host contract
test('extension-host/routes.v1.json lists the legacy route families', () => {
  const routes = readJson('extension-host', 'routes.v1.json');
  const ids = routes.routes.map((r) => r.id);
  for (const id of ['health', 'sources', 'stream', 'torrent', 'providers', 'providers/{id}', 'manga/chapters', 'manga/pages', 'novel/search', 'novel/chapters', 'novel/content',
    'custom/home', 'custom/search', 'custom/info', 'custom/watch', 'custom/read']) assert.ok(ids.includes(id), `missing route ${id}`);
  assert.equal(routes.package.bundleMaxBytes, 10485760);
  // every documented bundle method exists in the legacy host source (guards against drift when the contract is edited)
  const host = text(join(repo, 'desktop/runtime/extension-api-host/host-server.cjs'));
  for (const m of ['streamAll', 'torrentAll', 'sourcesAll', 'listProviders', 'runProvider', 'getMangaChapters', 'getMangaPages', 'getNovelChapters', 'getNovelContent',
    'getNovelSearch', 'customHome', 'customSearch', 'customInfo', 'customWatch', 'customRead']) {
    assert.ok(host.includes(`bundle.${m}`), `legacy host no longer calls bundle.${m}`);
  }
});

test('extension-host/globals.v1.md documents the injected globals and require tiers', () => {
  const md = text(join(contracts, 'extension-host', 'globals.v1.md'));
  for (const needle of ['__tatakai_fetch__', '__tatakai_parse_html__', 'Network permission denied', '64 MiB', '10485760', 'network:domain:', '`full`', '`partial`', '`unsupported`']) {
    assert.ok(md.includes(needle), `globals.v1.md must mention ${needle}`);
  }
});

// ---------------------------------------------------------------- fixtures: extensions
const kaiDir = join(contracts, 'fixtures', 'extensions');

function readZipEntries(buf) {
  // minimal central-directory reader (deflate or stored); enough to open our fixtures
  const eocd = buf.lastIndexOf(Buffer.from([0x50, 0x4b, 0x05, 0x06]));
  assert.ok(eocd >= 0, 'not a zip');
  const count = buf.readUInt16LE(eocd + 10);
  let p = buf.readUInt32LE(eocd + 16);
  const out = {};
  for (let i = 0; i < count; i++) {
    assert.equal(buf.readUInt32LE(p), 0x02014b50);
    const method = buf.readUInt16LE(p + 10);
    const csize = buf.readUInt32LE(p + 20);
    const nlen = buf.readUInt16LE(p + 28);
    const elen = buf.readUInt16LE(p + 30);
    const clen = buf.readUInt16LE(p + 32);
    const lho = buf.readUInt32LE(p + 42);
    const name = buf.subarray(p + 46, p + 46 + nlen).toString('utf8');
    const lnlen = buf.readUInt16LE(lho + 26);
    const lelen = buf.readUInt16LE(lho + 28);
    const raw = buf.subarray(lho + 30 + lnlen + lelen, lho + 30 + lnlen + lelen + csize);
    out[name] = method === 8 ? inflateRawSync(raw) : raw;
    p += 46 + nlen + elen + clen;
  }
  return out;
}

test('fixtures/extensions: committed dist/*.kai are current and contain manifest.json + bundle.js', () => {
  for (const name of fixtureNames()) {
    const file = join(kaiDir, 'dist', `${name}.kai`);
    assert.ok(existsSync(file), `missing ${file} - run node contracts/fixtures/extensions/build.mjs`);
    const committed = readFileSync(file);
    assert.ok(committed.equals(buildKai(name)), `${name}.kai is stale - run node contracts/fixtures/extensions/build.mjs`);
    const entries = readZipEntries(committed);
    assert.deepEqual(Object.keys(entries).sort(), ['bundle.js', 'manifest.json']);
    const manifest = JSON.parse(entries['manifest.json'].toString('utf8'));
    for (const f of ['id', 'name', 'version', 'type', 'description', 'permissions']) assert.ok(f in manifest, `${name}: manifest.${f}`);
    assert.equal(manifest.id, name);
    assert.ok(entries['bundle.js'].length < 10 * 1024 * 1024);
  }
});

test('fixtures/extensions: compat analysis (reference implementation)', () => {
  const full = text(join(kaiDir, 'synthetic-full', 'bundle.js'));
  const fs = text(join(kaiDir, 'synthetic-fs', 'bundle.js'));
  assert.equal(analyzeBundle(full).level, 'full');
  const r = analyzeBundle(fs);
  assert.equal(r.level, 'unsupported');
  assert.deepEqual(r.unsupported, ['fs']);
  assert.equal(r.reason, readJson('fixtures', 'extensions', 'synthetic-fs', 'expected.json').reason);
  assert.equal(analyzeBundle("const h = require('https'); h.get").level, 'partial');
  assert.equal(analyzeBundle('const n = "x"; require(n)').level, 'partial');
  assert.equal(analyzeBundle("require('undici')").level, 'unsupported');
  assert.equal(analyzeBundle("require('./local')").level, 'unsupported');
  assert.equal(analyzeBundle("require('node:crypto'); require(`path`)").level, 'full');
});

test('fixtures/extensions: synthetic-full satisfies expected.stream.json when run in a bare vm with the contract globals', async () => {
  const expected = readJson('fixtures', 'extensions', 'synthetic-full', 'expected.stream.json');
  const code = text(join(kaiDir, 'synthetic-full', 'bundle.js'));
  const nodeRequire = createRequire(import.meta.url);
  const allowed = new Set(['crypto']);
  const module = { exports: {} };
  const fetchShim = async () => { throw new Error('Network permission denied'); };
  const fn = vm.runInNewContext(`(function (module, exports, require, __tatakai_fetch__, fetch) {${code}\n})`, { Error, Promise, String });
  fn(module, module.exports, (n) => { if (!allowed.has(n)) throw new Error(`blocked ${n}`); return nodeRequire(n); }, fetchShim, fetchShim);
  const Ctor = module.exports.default || module.exports;
  const ext = new Ctor();
  const plain = (v) => JSON.parse(JSON.stringify(v)); // vm objects live in another realm; compare plain copies

  assert.deepEqual(plain(ext.listProviders()).map((p) => p.name), expected.providers.names);
  const opts = { anilistId: expected.request.params.anilistId, episode: expected.request.params.episode, titles: expected.request.params.titles, resolution: '1080p' };
  const chunks = [];
  await ext.streamAll(opts, (c) => chunks.push(plain(c)));
  assert.deepEqual(chunks.map((c) => ({ provider: c.diagnostic.provider, status: c.diagnostic.status, resultCount: c.diagnostic.resultCount })), expected.providerStatus);
  const results = chunks.flatMap((c) => c.results.map((r) => ({ r, provider: c.provider })));
  results.sort((a, b) => a.r.providerPriority - b.r.providerPriority || Number.parseInt(b.r.quality) - Number.parseInt(a.r.quality));
  assert.equal(results.length, expected.count);
  results.forEach(({ r, provider }, i) => {
    const e = expected.sources[i];
    assert.equal(provider, e.providerName);
    assert.equal(r.sourceType, e.type);
    assert.equal(r.quality, e.quality);
    assert.equal(r.url, e.url);
    assert.equal(r.providerPriority, e.providerPriority);
    assert.equal(r.audioLanguage, e.audioLanguage);
    assert.deepEqual(r.headers, e.headers);
    assert.equal(r.subtitles.length, e.subtitleCount);
  });
  const home = plain(await ext.customHome(expected.customHome.params.sourceId));
  assert.equal(home.sections.length, expected.customHome.sections);
  assert.equal(home.sections[0].items[0].id, expected.customHome.firstItemId);
  assert.equal(await ext.probeNetwork(), expected.networkGate.expectedMessage);
  assert.equal(analyzeBundle(code).level, expected.compat);
});

// ---------------------------------------------------------------- fixtures: metadata
test('fixtures/metadata: expected.json is the current output of the reference mapper', () => {
  const committed = JSON.parse(readFileSync(join(contracts, 'fixtures', 'metadata', 'expected.json'), 'utf8'));
  assert.deepEqual(committed, JSON.parse(JSON.stringify(buildExpected())));
});

test('fixtures/metadata: every fixture is valid JSON and the AniList ones follow the mediaCard fragment', () => {
  const dir = join(contracts, 'fixtures', 'metadata');
  for (const f of readdirSync(dir).filter((n) => n.endsWith('.json'))) JSON.parse(readFileSync(join(dir, f), 'utf8'));
  const trending = readJson('fixtures', 'metadata', 'anilist-trending-page.json');
  assert.equal(trending.data.Page.media.length, 3);
  const expected = readJson('fixtures', 'metadata', 'expected.json');
  const items = expected['anilist-trending-page.json'].items;
  assert.equal(items[0].ref.anilistId, 154587);
  assert.equal(items[0].title, "Frieren: Beyond Journey's End");
  assert.equal(items[2].title, 'Unannounced Sample Title');
  assert.equal(items[2].cover, 'https://s4.anilist.co/file/anilistcdn/media/anime/cover/medium/bx175014-sample.jpg');
  assert.equal(expected['anilist-trending-page.json'].hasNext, true);
  assert.equal(expected['kitsu-anime-list.json'].items[0].ref.anilistId, 16498);
  assert.deepEqual(expected['anizip-frieren.json'].ids, { anilistId: 154587, malId: 52991, kitsuId: 46474 });
  assert.equal(expected['anizip-frieren.json'].episodes[0].title, "The Journey's End");
});

// ---------------------------------------------------------------- Supabase m0 tables (E9)
test('supabase/m0-tables.md copies the watch_history schema literally and documents the id formats', () => {
  const sql = text(join(repo, 'supabase', 'main.sql'));
  const block = /CREATE TABLE public\.watch_history \(([\s\S]*?)\n\);/.exec(sql);
  assert.ok(block, 'watch_history table not found in supabase/main.sql');
  const md = text(join(contracts, 'supabase', 'm0-tables.md'));
  const columnLines = block[1].split('\n').map((l) => l.trim()).filter((l) => l && !l.startsWith('CONSTRAINT'));
  assert.ok(columnLines.length >= 13);
  for (const line of columnLines) assert.ok(md.includes(line), `m0-tables.md is missing the literal column definition: ${line}`);
  for (const needle of ['UNIQUE (user_id, episode_id)', 'on_conflict=user_id,episode_id', 'resolution=merge-duplicates', '`${anime_id}?ep=${episode_number}`', 'String(anilistId)', '>= 0.9']) {
    assert.ok(md.includes(needle), `m0-tables.md must mention ${needle}`);
  }
  // the legacy writer still uses what the doc says
  const hook = text(join(repo, 'src/hooks/user/useWatchHistory.ts'));
  assert.ok(hook.includes("onConflict: 'user_id,episode_id'"));
  assert.ok(hook.includes('progressSeconds / _dur >= 0.9'));
  const watchPage = text(join(repo, 'src/pages/watch/WatchPage.tsx'));
  assert.ok(watchPage.includes('decodedEpisodeId.split("?")[0]'), 'legacy anime_id derivation changed - update m0-tables.md');
});
