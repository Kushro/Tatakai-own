import { test } from 'node:test';
import assert from 'node:assert/strict';
import { memoryStore } from '../src/storage.mjs';
import { appendServerLog, logRoute } from '../src/logs.mjs';
test('activity logs retain only the latest 200 events and never serialize unapproved fields', async () => {
  const store = memoryStore(); await store.mutate(() => ({ state: {} }));
  for (let index = 0; index < 205; index++) await appendServerLog(store, { event: 'request_completed', route: '/extensions', status: 200, password: 'private-password', token: 'private-token', url: 'https://private.example', body: 'private-body' });
  const state = await store.read(); assert.equal(state.logs.length, 200); assert.equal(state.logSequence, 205); assert.equal(state.logs[0].id, 6);
  assert.ok(!JSON.stringify(state).includes('private-')); assert.ok(!JSON.stringify(state).includes('private.example'));
  assert.equal(logRoute('/api/server/v1/extensions/private-token', '/api/server/v1'), '/extensions');
  assert.equal(logRoute('/api/server/v1/private-path', '/api/server/v1'), '/unknown');
});
