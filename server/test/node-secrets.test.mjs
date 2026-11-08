import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtemp, rm, readFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { resolveNodeSecrets } from '../src/node-secrets.mjs';
import { vault } from '../src/security.mjs';
import { startNode } from '../src/node.mjs';

test('diskless deployments reject a missing database instead of using temporary SQLite', async () => {
  const previousRequired = process.env.REQUIRE_PERSISTENT_STORAGE, previousDatabase = process.env.DATABASE_URL;
  process.env.REQUIRE_PERSISTENT_STORAGE = '1'; delete process.env.DATABASE_URL;
  try { await assert.rejects(startNode({ port: 0 }), /Set DATABASE_URL/); }
  finally {
    if (previousRequired === undefined) delete process.env.REQUIRE_PERSISTENT_STORAGE; else process.env.REQUIRE_PERSISTENT_STORAGE = previousRequired;
    if (previousDatabase === undefined) delete process.env.DATABASE_URL; else process.env.DATABASE_URL = previousDatabase;
  }
});

test('Node generates persistent random defaults, warns, and preserves encrypted data', async () => {
  const root = await mkdtemp(path.join(tmpdir(), 'tatakai-secrets-'));
  try {
    const warnings = [], options = { warn: message => warnings.push(message) };
    const [first, concurrent] = await Promise.all([resolveNodeSecrets(root, options), resolveNodeSecrets(root, options)]);
    assert.deepEqual(first, concurrent); assert.ok(first.secret.length >= 32);
    assert.ok(warnings.some(message => message.includes('WARNING') && message.includes('SERVER_SECRET')));
    assert.ok(!warnings.some(message => message.includes(first.secret)));
    const encrypted = await (await vault(first.secret)).seal({ credential: 'private-fixture' });
    const restarted = await resolveNodeSecrets(root, { ...options, hasState: true });
    assert.deepEqual(restarted, first);
    assert.deepEqual(await (await vault(restarted.secret)).open(encrypted), { credential: 'private-fixture' });
    assert.deepEqual(JSON.parse(await readFile(path.join(root, 'node-secrets.json'), 'utf8')), first);
  } finally { await rm(root, { recursive: true, force: true }); }
});

test('missing original encryption key cannot silently replace an existing installation key', async () => {
  const root = await mkdtemp(path.join(tmpdir(), 'tatakai-existing-secret-'));
  try {
    await assert.rejects(resolveNodeSecrets(root, { hasState: true, warn: () => {} }), /original SERVER_SECRET/);
    const configured = { secret: 'explicit-fixture-key-with-at-least-32-characters', bootstrapSecret: 'owner-fixture' };
    assert.deepEqual(await resolveNodeSecrets(root, configured), configured);
  } finally { await rm(root, { recursive: true, force: true }); }
});

test('Node starts without secret environment variables and accepts the saved bootstrap key', async () => {
  const root = await mkdtemp(path.join(tmpdir(), 'tatakai-start-defaults-'));
  const oldSecret = process.env.SERVER_SECRET, oldBootstrap = process.env.BOOTSTRAP_SECRET;
  delete process.env.SERVER_SECRET; delete process.env.BOOTSTRAP_SECRET;
  let runtime;
  try {
    runtime = await startNode({ root, port: 0, publicUrl: 'http://localhost' });
    const base = `http://127.0.0.1:${runtime.server.address().port}/api/server/v1`;
    const health = await (await fetch(base + '/health')).json(); assert.equal(health.data.setupRequired, true);
    const saved = JSON.parse(await readFile(path.join(root, 'node-secrets.json'), 'utf8'));
    const setup = await fetch(base + '/setup', { method: 'POST', headers: { 'x-tatakai-bootstrap': saved.bootstrapSecret }, body: JSON.stringify({ adminPassword: '', viewerPassword: '' }) });
    assert.equal(setup.status, 200); assert.ok((await setup.json()).data.token);
  } finally {
    await runtime?.close();
    if (oldSecret === undefined) delete process.env.SERVER_SECRET; else process.env.SERVER_SECRET = oldSecret;
    if (oldBootstrap === undefined) delete process.env.BOOTSTRAP_SECRET; else process.env.BOOTSTRAP_SECRET = oldBootstrap;
    await rm(root, { recursive: true, force: true });
  }
});
