'use strict'

/**
 * @file fetch-cloudflared.cjs
 * Ensures the per-platform `cloudflared` binary exists in resources/bin/ so
 * electron-builder bundles it (host-hosted watch-party tunnels). Pulled from
 * the pinned cloudflare/cloudflared release in
 * desktop/runtime/share-tunnel/cloudflared-pin.cjs and SHA-256 verified — never
 * committed to git (the 55 MB binary stays out of the repo).
 *
 * Skips when a binary matching the pinned digest is already present; set
 * FORCE=1 to re-download.
 */

const fs   = require('fs')
const os   = require('os')
const path = require('path')
const { execFileSync } = require('child_process')
const axios = require('axios')
const {
  CLOUDFLARED_VERSION,
  CLOUDFLARED_RELEASE_BASE: BASE,
  verifyCloudflaredAsset,
} = require('../desktop/runtime/share-tunnel/cloudflared-pin.cjs')

const ROOT    = path.join(__dirname, '..')
const BIN_DIR = path.join(ROOT, 'resources', 'bin')

function targetFor(platform, arch) {
  if (platform === 'win32') return { asset: 'cloudflared-windows-amd64.exe', out: 'cloudflared.exe', tgz: false }
  if (platform === 'linux') return { asset: 'cloudflared-linux-amd64',       out: 'cloudflared',     tgz: false }
  if (platform === 'darwin') {
    const a = arch === 'arm64' ? 'arm64' : 'amd64'
    return { asset: `cloudflared-darwin-${a}.tgz`, out: 'cloudflared', tgz: true }
  }
  throw new Error(`Unsupported platform: ${platform}`)
}

async function download(url, dest) {
  const res = await axios.get(url, { responseType: 'stream', maxRedirects: 5, timeout: 180000 })
  await new Promise((resolve, reject) => {
    const ws = fs.createWriteStream(dest)
    res.data.pipe(ws)
    ws.on('finish', resolve)
    ws.on('error', reject)
    res.data.on('error', reject)
  })
}

async function main() {
  const { asset, out, tgz } = targetFor(process.platform, process.arch)
  if (!fs.existsSync(BIN_DIR)) fs.mkdirSync(BIN_DIR, { recursive: true })
  const outPath = path.join(BIN_DIR, out)

  // The darwin digest covers the .tgz, not the extracted binary, so an existing
  // mac binary can't be re-checked here — always refetch there.
  if (!process.env.FORCE && !tgz && fs.existsSync(outPath)) {
    try {
      await verifyCloudflaredAsset(asset, outPath)
      console.log(`[cloudflared] ${CLOUDFLARED_VERSION} already present → ${path.relative(ROOT, outPath)} (FORCE=1 to re-download)`)
      return
    } catch (e) {
      console.log(`[cloudflared] existing binary rejected (${e.message}) — re-downloading`)
    }
  }

  const url = `${BASE}/${asset}`
  console.log(`[cloudflared] downloading ${url}`)
  if (tgz) {
    const tmp = path.join(os.tmpdir(), asset)
    await download(url, tmp)
    await verifyCloudflaredAsset(asset, tmp)
    execFileSync('tar', ['-xzf', tmp, '-C', BIN_DIR], { stdio: 'inherit' })
    fs.rmSync(tmp, { force: true })
  } else {
    const tmp = `${outPath}.download`
    await download(url, tmp)
    await verifyCloudflaredAsset(asset, tmp)
    fs.renameSync(tmp, outPath)
  }
  console.log(`[cloudflared] SHA-256 verified (${CLOUDFLARED_VERSION}/${asset})`)
  if (process.platform !== 'win32') { try { fs.chmodSync(outPath, 0o755) } catch (_) {} }
  const mb = (fs.statSync(outPath).size / 1e6).toFixed(1)
  console.log(`[cloudflared] ready → ${path.relative(ROOT, outPath)} (${mb} MB)`)
}

main().catch((e) => { console.error('[cloudflared] FAILED:', e.message); process.exit(1) })
