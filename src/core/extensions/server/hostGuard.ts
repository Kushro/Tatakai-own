/**
 * Host guard for extension servers (D6, contract §3.8). Same rule as the
 * Kotlin `HostGuard` of Tatakai Next:
 *
 *  - `https:` is allowed for any host.
 *  - `http:` is allowed only for hosts on the user's own network, judged from
 *    the literal host (no DNS): loopback (127/8, ::1, localhost, *.localhost),
 *    RFC1918 (10/8, 172.16/12, 192.168/16), link-local (169.254/16, fe80::/10),
 *    CGNAT / Tailscale (100.64/10), private IPv6 (fc00::/7) and mDNS `*.local`.
 *  - URLs with embedded credentials (`user:pass@host`) are rejected.
 *  - Anything else (other schemes, dotless names, public hosts over http) is
 *    blocked with `insecure_host`.
 *
 * Pure module (no DOM, no Capacitor) so it can be unit tested on its own.
 */

function ipv4(host: string): number[] | null {
  const match = host.match(/^(\d{1,3})\.(\d{1,3})\.(\d{1,3})\.(\d{1,3})$/);
  if (!match) return null;
  const parts = match.slice(1).map(Number);
  return parts.every(part => part <= 255) ? parts : null;
}

function privateIpv4([a, b]: number[]): boolean {
  return a === 127 || a === 10 || (a === 172 && b >= 16 && b <= 31) || (a === 192 && b === 168)
    || (a === 169 && b === 254) || (a === 100 && b >= 64 && b <= 127);
}

function privateIpv6(host: string): boolean {
  if (!host.includes(':')) return false;
  const value = host.toLowerCase();
  if (value === '::1') return true;
  const first = value.startsWith('::') ? 0 : parseInt(value.split(':')[0], 16);
  if (!Number.isFinite(first)) return false;
  // fe80::/10 link-local, fc00::/7 unique local (Tailscale uses fd7a:115c:a1e0::/48).
  return (first & 0xffc0) === 0xfe80 || (first & 0xfe00) === 0xfc00;
}

/** True when `host` (URL hostname, IPv6 with or without brackets) is a local-network host. */
export function isLocalNetworkHost(rawHost: string): boolean {
  const host = String(rawHost || '').trim().toLowerCase().replace(/^\[|\]$/g, '').replace(/\.$/, '');
  if (!host) return false;
  if (host === 'localhost' || host.endsWith('.localhost')) return true;
  const v4 = ipv4(host);
  if (v4) return privateIpv4(v4);
  if (host.includes(':')) return privateIpv6(host);
  // mDNS names need at least one label before `.local`.
  return /^[a-z0-9-]+(\.[a-z0-9-]+)*\.local$/.test(host);
}

/** Whether a server (or media) URL may be used by the extension-server client. */
export function isAllowedServerUrl(url: string): boolean {
  let parsed: URL;
  try { parsed = new URL(url); } catch { return false; }
  // Same as the Kotlin HostGuard: embedded credentials (user:pass@) are never accepted.
  if (parsed.username || parsed.password) return false;
  if (parsed.protocol === 'https:') return Boolean(parsed.hostname);
  if (parsed.protocol === 'http:') return isLocalNetworkHost(parsed.hostname);
  return false;
}

/** Throws `Error('insecure_host')` when {@link isAllowedServerUrl} rejects `url`. */
export function assertAllowedServerUrl(url: string): void {
  if (!isAllowedServerUrl(url)) throw new Error('insecure_host');
}
