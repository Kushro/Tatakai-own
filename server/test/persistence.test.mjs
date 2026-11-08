import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtemp, writeFile, readFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import path from 'node:path';
import http from 'node:http';
import { scryptSync } from 'node:crypto';
import { nodeStorage } from '../src/node-storage.mjs';
import { createDesktopServer } from '../src/desktop.mjs';
const prefix = path.resolve(tmpdir(), 'tatakai-server-test-');
async function cleanup(directory) { if (!path.resolve(directory).startsWith(prefix)) throw Error('Unexpected test directory'); await rm(directory, { recursive: true, force: true }); }
test('SQLite state, CAS, and artifacts survive reopening from a second process adapter', async () => {
  const root = await mkdtemp(prefix), first = await nodeStorage(root), second = await nodeStorage(root);
  try {
    await first.store.mutate(() => ({ state: { count: 0 } }));
    await Promise.all([first.store.mutate(state => ({ state: { count: state.count + 1 } })), second.store.mutate(state => ({ state: { count: state.count + 1 } }))]);
    assert.equal((await second.store.read()).count, 2);
    await first.artifacts.put('fixture/code', 'module.exports={}'); assert.equal(await second.artifacts.get('fixture/code'), 'module.exports={}');
    await second.artifacts.remove('fixture/code'); await assert.rejects(first.artifacts.get('fixture/code'));
  } finally { first.close(); second.close(); await cleanup(root); }
});
test('desktop migration backs up and preserves legacy users/tokens, and upgrades old passwords', async () => {
  const root = await mkdtemp(prefix), password = 'old-pass', salt = 'legacy-salt';
  const legacy = { passwordHash: scryptSync(password, salt, 64).toString('hex'), passwordSalt: salt,
    users: [{ id: 'child', permissions: ['library'] }], tokens: { existing: { userId: 'child' } }, enabled: false };
  const filename = path.join(root, 'home-server.json'); await writeFile(filename, JSON.stringify(legacy));
  const runtime = await createDesktopServer({ app: { getPath: () => root }, legacyStore: { read: () => structuredClone(legacy) }, adapters: { listExtensions: async () => [] } });
  const server = http.createServer((req,res) => runtime.handle(req, res, `http://127.0.0.1:${server.address().port}`));
  await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
  try {
    const base = `http://127.0.0.1:${server.address().port}/api/server/v1`;
    assert.equal(JSON.parse(await readFile(filename, 'utf8')).tokens.existing.userId, 'child');
    assert.deepEqual(JSON.parse(await readFile(path.join(root, 'extension-server/home-server.before-migration.json'), 'utf8')), legacy);
    let response = await fetch(base + '/auth/login', { method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify({ role: 'admin', password }) });
    const login = await response.json(); assert.equal(response.status, 200, JSON.stringify(login));
    response = await fetch(base + '/auth/password', { method: 'POST', headers: { 'content-type': 'application/json', authorization: 'Bearer ' + login.data.token }, body: JSON.stringify({ currentPassword: password, password: 'new-owner-password' }) });
    assert.equal(response.status, 200);
    response = await fetch(base + '/auth/login', { method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify({ role: 'admin', password: 'new-owner-password' }) }); assert.equal(response.status, 200);
    assert.equal(JSON.parse(await readFile(filename, 'utf8')).enabled, false);
  } finally { server.closeAllConnections(); await new Promise(resolve => server.close(resolve)); runtime.close(); await cleanup(root); }
});
