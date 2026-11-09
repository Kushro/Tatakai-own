import { describe, expect, it } from 'bun:test';
import { assertAllowedServerUrl, isAllowedServerUrl } from '../src/core/extensions/server/hostGuard';
import { normalizeServerOrigin, parsePairingInput, parseServerLink } from '../src/core/extensions/server/pairingLink';

// Contract §3.8: same rule as the Kotlin HostGuard of Tatakai Next.
describe('extension server host guard', () => {
  it('allows https for any host', () => {
    for (const url of ['https://example.com', 'https://abc.trycloudflare.com/api/server/v1/health', 'https://8.8.8.8:8443']) expect(isAllowedServerUrl(url)).toBe(true);
  });
  it('allows http only for local-network hosts', () => {
    for (const url of ['http://127.0.0.1:8090', 'http://localhost:8090', 'http://pc.localhost', 'http://[::1]:8090', 'http://10.0.0.5', 'http://172.16.0.1',
      'http://172.31.255.255', 'http://192.168.1.20:8090', 'http://169.254.10.1', 'http://[fe80::1]', 'http://100.64.0.1', 'http://100.127.255.254',
      'http://[fd7a:115c:a1e0::1]:8090', 'http://[fc00::1]', 'http://mipc.local:8090']) expect(isAllowedServerUrl(url)).toBe(true);
  });
  it('blocks http for public, dotless and out-of-range hosts and other schemes', () => {
    for (const url of ['http://example.com', 'http://8.8.8.8', 'http://172.32.0.1', 'http://172.15.0.1', 'http://100.128.0.1', 'http://100.63.0.1', 'http://mipc',
      'http://[2001:db8::1]', 'http://local', 'http://user:pass@192.168.1.2', 'https://user@example.com', 'ftp://192.168.1.2', 'tatakai://extensionserver/connect', 'not a url', '']) expect(isAllowedServerUrl(url)).toBe(false);
    expect(() => assertAllowedServerUrl('http://example.com')).toThrow('insecure_host');
  });
});

describe('extension server pairing links', () => {
  const server = 'http://192.168.1.20:8090';
  const deepLink = `tatakai://extensionserver/connect?server=${encodeURIComponent(server)}&code=${encodeURIComponent('Ab_9-x')}`;
  it('parses the canonical deep link and its in-app route', () => {
    expect(parseServerLink(deepLink)).toEqual({ server, code: 'Ab_9-x' });
    expect(parseServerLink(`/extensionserver/connect?server=${encodeURIComponent(server)}&code=Ab_9-x`)).toEqual({ server, code: 'Ab_9-x' });
  });
  it('accepts the compatibility formats', () => {
    expect(parseServerLink(`https://tatakai.me/connect-server?server=${encodeURIComponent(server)}&code=abc`)).toEqual({ server, code: 'abc' });
    expect(parseServerLink(`/connect-server?server=${encodeURIComponent(server)}`)).toEqual({ server });
    expect(parseServerLink(`/extensionserver/${encodeURIComponent(server)}?pass=secret`)).toEqual({ server, password: 'secret' });
    expect(parseServerLink(`${server}/server/`)).toEqual({ server });
  });
  it('rejects malformed links and insecure hosts', () => {
    expect(() => parseServerLink('tatakai://extensionserver/connect?server=http%3A%2F%2F192.168.1.20%3A8090')).toThrow('invalid_link');
    expect(() => parseServerLink(`tatakai://extensionserver/connect?server=${encodeURIComponent(server)}&code=bad%20code`)).toThrow('invalid_link');
    expect(() => parseServerLink(`tatakai://extensionserver/connect?server=${encodeURIComponent(server + '/path')}&code=abc`)).toThrow('invalid_link');
    expect(() => parseServerLink(`tatakai://extensionserver/connect?server=${encodeURIComponent('http://example.com')}&code=abc`)).toThrow('insecure_host');
    expect(() => parseServerLink('https://evil.example/connect-server?server=x&code=y')).toThrow('invalid_link');
  });
  it('pairs a pasted bare code with the address field', () => {
    expect(parsePairingInput('Ab_9-x', '192.168.1.20:8090')).toEqual({ server, code: 'Ab_9-x' });
    expect(() => parsePairingInput('Ab_9-x', '')).toThrow('invalid_link');
    expect(parsePairingInput(deepLink)).toEqual({ server, code: 'Ab_9-x' });
  });
  it('normalizes typed addresses', () => {
    expect(normalizeServerOrigin('192.168.1.20:8090')).toBe(server);
    expect(normalizeServerOrigin('pc.example.com')).toBe('https://pc.example.com');
    expect(() => normalizeServerOrigin('http://pc.example.com')).toThrow('insecure_host');
    expect(() => normalizeServerOrigin('http://user:pw@192.168.1.20')).toThrow('invalid_url');
  });
});
