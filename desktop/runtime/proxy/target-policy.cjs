'use strict';

/**
 * Upstream target policy for LocalProxyServer.
 *
 * Every URL the proxy fetches comes, directly or indirectly, from extension
 * code or from data an extension scraped (stream URLs, `/proxy?url=`, manga
 * page URLs, redirects). Without a check the proxy is an SSRF primitive into
 * the user's LAN, router admin pages and cloud metadata endpoints.
 *
 * Policy: a target must resolve only to public addresses. Loopback is refused
 * too, except for origins the app itself serves (the proxy's own port, the
 * torrent stream server) — the watch-party share path legitimately re-proxies
 * those. Redirects are followed manually so each hop is checked.
 *
 * Limitation: the address check and the connection do separate DNS lookups,
 * so a hostile resolver can still race them (DNS rebinding). Closing that
 * needs a pinned-address dispatcher; this stops the direct cases.
 */

const dns = require('node:dns/promises');
const net = require('node:net');

const BLOCKED_RANGES = new net.BlockList();
for (const [address, prefix] of [
    ['0.0.0.0', 8],        // "this network"
    ['10.0.0.0', 8],       // RFC 1918
    ['100.64.0.0', 10],    // carrier-grade NAT
    ['127.0.0.0', 8],      // loopback
    ['169.254.0.0', 16],   // link-local, cloud metadata
    ['172.16.0.0', 12],    // RFC 1918
    ['192.0.0.0', 24],     // IETF protocol assignments
    ['192.168.0.0', 16],   // RFC 1918
    ['198.18.0.0', 15],    // benchmarking
    ['224.0.0.0', 4],      // multicast
    ['240.0.0.0', 4],      // reserved + broadcast
]) BLOCKED_RANGES.addSubnet(address, prefix, 'ipv4');
for (const [address, prefix] of [
    ['::', 128],           // unspecified
    ['::1', 128],          // loopback
    ['fc00::', 7],         // unique local
    ['fe80::', 10],        // link-local
    ['ff00::', 8],         // multicast
]) BLOCKED_RANGES.addSubnet(address, prefix, 'ipv6');

const REDIRECT_STATUSES = new Set([301, 302, 303, 307, 308]);
const MAX_REDIRECTS = 10;

class BlockedTargetError extends Error {
    constructor(message) {
        super(message);
        this.name = 'BlockedTargetError';
        this.code = 'ETARGETBLOCKED';
    }
}

/** True when `address` (an IP literal) is loopback, private, link-local or reserved. */
function isPrivateAddress(address) {
    let ip = String(address || '').replace(/^\[|\]$/g, '');
    // IPv4-mapped IPv6 (::ffff:10.0.0.1) is judged as the IPv4 it wraps.
    const mapped = ip.match(/^::ffff:(\d+\.\d+\.\d+\.\d+)$/i);
    if (mapped) ip = mapped[1];
    const family = net.isIP(ip);
    if (family === 4) return BLOCKED_RANGES.check(ip, 'ipv4');
    if (family === 6) return BLOCKED_RANGES.check(ip, 'ipv6');
    return false;
}

/**
 * Throw BlockedTargetError unless `url` is http(s) and its host resolves only to
 * public addresses. `trustedOrigins` (exact origins such as
 * `http://127.0.0.1:8888`) skip the address check.
 */
async function assertPublicTarget(url, { trustedOrigins = [], lookup = dns.lookup } = {}) {
    const target = url instanceof URL ? url : new URL(String(url));
    if (target.protocol !== 'http:' && target.protocol !== 'https:') {
        throw new BlockedTargetError(`unsupported target protocol ${target.protocol}`);
    }
    if (trustedOrigins.includes(target.origin)) return;
    const host = target.hostname.replace(/^\[|\]$/g, '').toLowerCase();
    if (host === 'localhost' || host.endsWith('.localhost')) {
        throw new BlockedTargetError(`target ${host} is a local address`);
    }
    if (net.isIP(host)) {
        if (isPrivateAddress(host)) throw new BlockedTargetError(`target ${host} is a private address`);
        return;
    }
    const records = await lookup(host, { all: true, verbatim: true });
    for (const record of records) {
        if (isPrivateAddress(record.address)) {
            throw new BlockedTargetError(`target ${host} resolves to private address ${record.address}`);
        }
    }
}

/**
 * `fetch()` that checks the target — and every redirect hop — with
 * assertPublicTarget. Mirrors fetch's own redirect rules: 303 (and 301/302 on
 * POST) switch to a bodyless GET, and credentials are dropped when a hop
 * changes origin. `allowPrivate` disables the check (tests only).
 */
async function fetchPublicTarget(url, init = {}, { allowPrivate = false, trustedOrigins = [], lookup, fetchImpl = fetch } = {}) {
    let current = new URL(String(url));
    let method = String(init.method || 'GET').toUpperCase();
    let body = init.body;
    let headers = new Headers(init.headers || {});
    const followRedirects = init.redirect !== 'manual';

    for (let hop = 0; ; hop += 1) {
        if (!allowPrivate) await assertPublicTarget(current, { trustedOrigins, lookup });
        const response = await fetchImpl(current, { ...init, method, headers, body, redirect: 'manual' });
        const location = response.headers.get('location');
        if (!followRedirects || !REDIRECT_STATUSES.has(response.status) || !location) return response;
        if (hop >= MAX_REDIRECTS) throw new TypeError('fetch failed: redirect count exceeded');
        try { await response.body?.cancel(); } catch { /* already drained */ }

        const next = new URL(location, current);
        if (response.status === 303 ? method !== 'HEAD' : ((response.status === 301 || response.status === 302) && method === 'POST')) {
            method = 'GET';
            body = undefined;
            headers.delete('content-type');
            headers.delete('content-length');
        }
        if (next.origin !== current.origin) {
            headers = new Headers(headers);
            headers.delete('authorization');
            headers.delete('proxy-authorization');
            headers.delete('cookie');
        }
        current = next;
    }
}

module.exports = { BlockedTargetError, isPrivateAddress, assertPublicTarget, fetchPublicTarget };
