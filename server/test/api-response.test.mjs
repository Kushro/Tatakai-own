import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readApiResponse } from '../src/api-response.mjs';
import { serveNodeRequest } from '../src/node.mjs';
import { createServer } from 'node:http';

test('API clients identify empty, truncated and non-JSON responses without leaking their bodies', async () => {
  for (const [body, status] of [['', 200], ['', 502], ['{"success":', 200], ['<html>private-proxy-value</html>', 503]]) {
    await assert.rejects(readApiResponse(new Response(body, { status })), error => {
      assert.match(error.message, new RegExp(`HTTP ${status}`));
      assert.ok(!error.message.includes('private-proxy-value'));
      assert.ok(!error.message.includes('Unexpected end of JSON'));
      return true;
    });
  }
  await assert.rejects(readApiResponse(Response.json({ unrelated: true })), /invalid API response/);
  assert.deepEqual(await readApiResponse(Response.json({ success: true, data: [] })), { success: true, data: [] });
  assert.deepEqual(await readApiResponse(Response.json({ success: false, error: { code: 'invalid_password' } }, { status: 401 })), { success: false, error: { code: 'invalid_password' } });
});

test('Node transport failures return a JSON error instead of an empty HTTP 500', async () => {
  const server = createServer((req, res) => serveNodeRequest(async () => { throw Error('private-deployment-value'); }, req, res, 'http://127.0.0.1'));
  await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
  try {
    const response = await fetch(`http://127.0.0.1:${server.address().port}/api/server/v1/health`);
    assert.equal(response.status, 500);
    const result = await readApiResponse(response);
    assert.equal(result.error.code, 'transport_error');
    assert.ok(!JSON.stringify(result).includes('private-deployment-value'));
  } finally { server.closeAllConnections(); await new Promise(resolve => server.close(resolve)); }
});
