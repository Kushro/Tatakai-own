import { test } from 'node:test';
import assert from 'node:assert/strict';
import { startupBanner } from '../src/startup-banner.mjs';
test('startup branding links to the configured public server, the /server/ dashboard and the actual listener without credentials', () => {
  const hosted = startupBanner('https://sources.example.com', 'http://0.0.0.0:5012');
  assert.match(hosted, /TATAKAI \/ EXTENSION SERVER/);
  assert.ok(hosted.includes('https://sources.example.com/api/server/v1/health'));
  assert.ok(hosted.includes('https://sources.example.com/server/'));
  assert.ok(hosted.includes('http://0.0.0.0:5012'));
  assert.ok(!hosted.includes('tatakai.me'));
  assert.ok(!hosted.includes('localhost:8090'));
  assert.ok(!hosted.includes('tatakai://'));
  const local = startupBanner('http://localhost:8910', 'http://127.0.0.1:8910');
  assert.ok(local.includes('http://localhost:8910/server/'));
  assert.ok(!local.includes('tatakai.me'));
});
test('startup banner prints a one-use pairing deep link when one is provided', () => {
  const deepLink = 'tatakai://extensionserver/connect?server=http%3A%2F%2F192.168.1.20%3A8787&code=abc';
  const banner = startupBanner('http://192.168.1.20:8787', 'http://0.0.0.0:8787', { code: 'abc', expiresIn: 300, deepLink });
  assert.ok(banner.includes(deepLink)); assert.match(banner, /5 min/);
});
