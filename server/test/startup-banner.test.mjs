import { test } from 'node:test';
import assert from 'node:assert/strict';
import { startupBanner } from '../src/startup-banner.mjs';
test('startup branding links to the configured public server and actual listener without credentials', () => {
  const hosted = startupBanner('https://sources.example.com', 'http://0.0.0.0:5012');
  assert.match(hosted, /TATAKAI \/ EXTENSION SERVER/);
  assert.ok(hosted.includes('https://sources.example.com/api/server/v1/health'));
  assert.ok(hosted.includes('http://0.0.0.0:5012'));
  assert.ok(hosted.includes('https://tatakai.me/extensionserver/https%3A%2F%2Fsources.example.com'));
  const local = startupBanner('http://localhost:8910', 'http://127.0.0.1:8910');
  assert.ok(local.includes('http://localhost:8090/extensionserver/http%3A%2F%2Flocalhost%3A8910'));
});
