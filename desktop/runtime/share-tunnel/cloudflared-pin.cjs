'use strict';

/**
 * cloudflared-pin.cjs
 *
 * Single source of truth for the cloudflared release we ship and auto-install.
 * Both the build-time fetch (scripts/fetch-cloudflared.cjs) and the runtime
 * auto-install (share-tunnel.cjs) download this exact version and refuse any
 * file whose SHA-256 does not match — never `releases/latest`.
 *
 * Digests are the GitHub release asset digests for the tag below. Windows and
 * Linux also match the checksums published in the release notes.
 *
 * To bump: pick a release, then
 *   gh api repos/cloudflare/cloudflared/releases/tags/<tag> \
 *     --jq '.assets[] | "\(.name) \(.digest)"'
 */

const crypto = require('node:crypto');
const fs = require('node:fs');

const CLOUDFLARED_VERSION = '2026.10.0';
const CLOUDFLARED_RELEASE_BASE =
    `https://github.com/cloudflare/cloudflared/releases/download/${CLOUDFLARED_VERSION}`;

const CLOUDFLARED_SHA256 = Object.freeze({
    'cloudflared-windows-amd64.exe': '86aee4017b26625cee8484c113558f48effa4cd47f7aa05fcf425604e5d2b23c',
    'cloudflared-linux-amd64':       'd33ff2d14475178d2012c2c56beba87389ac5ded27649519f198a7d3134a99db',
    'cloudflared-linux-arm64':       'e6422b9d4f72d3194bc5a38676f13667c06666523217b842a877d72a80b5ac08',
    'cloudflared-darwin-amd64.tgz':  '903845b81828c8cb3c5d13d816a2de71c06a3da5785469df8eb0e1b736d92f9f',
    'cloudflared-darwin-arm64.tgz':  'a2f79ff7b9420aa537d74af239f376da170bbabeb529aec416002adac6a72e70',
});

function sha256File(file) {
    return new Promise((resolve, reject) => {
        const hash = crypto.createHash('sha256');
        fs.createReadStream(file)
            .on('error', reject)
            .on('data', (chunk) => hash.update(chunk))
            .on('end', () => resolve(hash.digest('hex')));
    });
}

/**
 * Throws (and deletes `file`) unless its SHA-256 matches the pinned digest
 * for `asset`. Unknown assets are rejected too.
 */
async function verifyCloudflaredAsset(asset, file) {
    const expected = CLOUDFLARED_SHA256[asset];
    if (!expected) throw new Error(`no pinned SHA-256 for ${asset}`);
    const actual = await sha256File(file);
    if (actual !== expected) {
        try { fs.unlinkSync(file); } catch (_) {}
        throw new Error(`SHA-256 mismatch for ${asset}: expected ${expected}, got ${actual}`);
    }
}

module.exports = {
    CLOUDFLARED_VERSION,
    CLOUDFLARED_RELEASE_BASE,
    CLOUDFLARED_SHA256,
    verifyCloudflaredAsset,
};
