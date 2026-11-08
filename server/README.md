# Tatakai Extension Server

**Your sources, connected.** Install extensions once on your PC (or a home
server) and use their anime, manga and novel sources from the Tatakai app on
your phone, web and other devices.

```text
Sources → Extension runtime → Built-in media proxy → Tatakai clients
```

This directory is part of the Tatakai repository (it used to be the separate
`TatakaiServer` repo / `tatakai-server` submodule). There is no
`server/package.json`: dependencies live in the root `package.json` and the
single root lockfile, and the server bundles the app's own code in place
(`src/core/extensions/**`, `src/components/ui/**`,
`desktop/runtime/compatibility/**`, `shared/aniyomi/**`). Nothing is copied.

It runs in two ways:

| Mode | Entry | Who uses it |
| --- | --- | --- |
| **Embedded in Electron** (main mode) | `server/src/desktop.mjs` | Tatakai desktop for Windows / Linux, on the Home Server port |
| **Standalone Node** | `server/src/node.mjs` (or the Docker image) | VPS / homelab |

Serverless adapters (Cloudflare Workers, D1, Postgres/Neon, Vercel, Render,
Heroku) were removed. The API stays `/api/server/v1` with `protocolVersion: 1`.

## Layout

```text
server/
  src/        runtime (core API, extensions, proxy, storage, Node/desktop hosts)
  ui/         dashboard (React + Vite + Tailwind), served under /server/
  test/       node --test suites
  scripts/    build.mjs, dev.mjs
  dist/       build output (git-ignored)
    interpreter.mjs            QuickJS adapters (Sora / Mangayomi / Seanime / Tatakai scripts)
    node-extension-worker.cjs  worker bootstrap for trusted Node bundles (Toko)
    quickjs.wasm
    ui/                        dashboard, base /server/
```

## Build

Requires **Node.js 22.16+** (`node:sqlite`) and a `node_modules` installed for
the current OS from the root lockfile. From the repository root:

```sh
npm ci --ignore-scripts      # or your normal npm install
npm run server:build         # node server/scripts/build.mjs
npm run server:test          # node --test "server/test/*.test.mjs"
```

`server:build` wipes and recreates `server/dist`. esbuild resolves packages
from `<repo>/node_modules` and maps `@` to `<repo>/src`; the dashboard is built
with Vite using `server/ui/vite.config.ts` and `base: '/server/'`.

The Java compatibility runtime (Aniyomi / Mihon / CloudStream) is **not** part
of `dist`. Both hosts load it from `desktop/runtime/compatibility/` in place,
and `aniyomi.cjs` checks its bootstrap sources against
`aniyomi-compiled.json` at `<repo>/desktop/runtime/compatibility/aniyomi-java/**`
and `<repo>/shared/aniyomi/**`. Keep that layout wherever the server runs (the
Docker image keeps it).

## Embedded in Tatakai desktop (Windows / Linux)

`desktop/runtime/home-server/index.cjs` imports `<appRoot>/server/src/desktop.mjs`
and calls:

```js
const runtime = await createDesktopServer({ app, legacyStore, adapters });
// runtime = { bootstrap, handle(req, res, base), close(), createPairing(origin), info() }
```

- `adapters` come from `desktop/ipc/ipc-home-server.cjs`: `compatibilityBroker()`
  (read lazily, because the broker starts with the extension runtime),
  `isLocalMediaUrl(url)`, `listExtensions()` and `invokeExtension(...)`. The
  desktop server publishes the Electron compatibility broker (Aniyomi, Mihon,
  CloudStream) and the installed `.kai` extensions as sources, next to the
  interpreted extensions installed in the dashboard.
- State lives in `<userData>/extension-server/` (`server.sqlite`, `artifacts/`,
  `secrets.json`). A legacy Home Server password is migrated once (a backup is
  written to `home-server.before-migration.json`). With nothing to migrate, the
  server is configured on first run as `Tatakai (<hostname>)` so the PC can pair
  immediately; owner access uses the **setup secret** (`bootstrap`, shown in
  Electron as "Copy setup secret").
- The desktop server runs with `requireDeviceToken`: no anonymous LAN access.
  Phones pair with a one-time code, or sign in with the optional viewer password.
- If `server/dist` is missing, `createDesktopServer` rejects with
  `Error('extension server not built')` and Electron answers
  `503 extension_server_unavailable` on `/server/*` and `/api/server/*`. The
  rest of the Home Server keeps working.
- `info()` returns `{ name, sourceCount, ecosystems }` for the Electron settings
  screen. `createPairing(origin)` returns `{ code, expiresIn, deepLink }`.

Routes on the Home Server port:

| Path | Served by |
| --- | --- |
| `/`, `/home`, `/ui/*`, anything else | legacy remote UI (unchanged) |
| `/api/home/*` | legacy Home Server API (unchanged) |
| `/server` → `301 /server/`; `/server/*` | extension server dashboard (SPA fallback for paths without an extension) |
| `/api/server/v1/*` (including `/media/*`) | extension server API |

Packaging (electron-builder, root `package.json`): `files` must include
`server/src/**/*` and `server/dist/**/*`; `asarUnpack` must include
`server/dist/quickjs.wasm` and `server/dist/node-extension-worker.cjs`; run
`npm run server:build` before electron-builder.

## Linking a phone

The owner creates a one-time pairing code (five minutes, single use) from
Electron settings or the dashboard. The QR code and the "copy link" button both
carry the same **deep link**:

```text
tatakai://extensionserver/connect?server=<encodeURIComponent(origin)>&code=<encodeURIComponent(code)>
```

`origin` is the address the phone can reach: the PC's LAN address
(`http://192.168.x.y:<port>`), its Tailscale address, or the HTTPS tunnel URL
when away from home. Clients accept plain `http://` only for loopback, private
(RFC1918), link-local, CGNAT / Tailscale (`100.64.0.0/10`, `fc00::/7`) and
`*.local` hosts; anything else must be HTTPS. Pairing returns a revocable device
token (Devices in the dashboard). Clients can also connect with the server URL
and the viewer password.

What you gain: Aniyomi, Mihon and CloudStream sources (PC only) on the phone;
extensions installed and configured once for every device; scraping and
Cloudflare bypass done by the PC's browser; shared cache and progressive
results; less battery and CPU used on the phone. What it costs: the PC must be
on with Tatakai open; away from home you need the HTTPS tunnel (or Tailscale);
video goes through the PC's proxy (its upload bandwidth when you are away); a
port is open on your LAN; source lookups have a little more latency. If the PC
does not answer, the apps keep using their local extensions.

## Standalone Node (VPS / homelab)

```sh
npm run server:build
npm run server:start         # node server/src/node.mjs
```

Open `http://localhost:8787/server/` (`GET /` redirects there). The startup
banner prints the dashboard, API and health URLs.

| Variable | Default | Meaning |
| --- | --- | --- |
| `PORT` | `8787` | listen port |
| `HOST` | `127.0.0.1` | bind address (`0.0.0.0` to accept LAN / container traffic) |
| `DATA_DIR` | `<repo>/server/data` | SQLite state, artifacts, `node-secrets.json` |
| `PUBLIC_URL` | `http://localhost:<PORT>` | origin used for minted media URLs (set it behind a reverse proxy) |
| `SERVER_SECRET` | generated | encryption key (32+ chars); generated once into `node-secrets.json` if unset |
| `BOOTSTRAP_SECRET` | generated | setup key; generated once into `node-secrets.json` if unset |
| `ENABLE_NATIVE_EXTENSIONS` | off | `1` loads the Java compatibility broker (Aniyomi / Mihon / CloudStream); it downloads its pinned runtime into `DATA_DIR/compatibility` when you prepare it in the dashboard |
| `ENABLE_BROWSER_RUNTIME` | off | `1` enables the headless Playwright bridge for browser-dependent sources (needs `npx playwright install chromium`) |
| `REQUIRE_PERSISTENT_STORAGE` | off | `1` refuses to start without an explicit `DATA_DIR` |

`server/.env` is loaded if present (`process.loadEnvFile`); real environment
variables win. `DATABASE_URL` is rejected: the Postgres adapter is gone.

First run: copy `bootstrapSecret` from `DATA_DIR/node-secrets.json` into the
dashboard setup. Keep that file private and back it up with the data directory:
the encryption key must survive restarts. In standalone mode, a server without a
viewer password is public (as before); set one for anything reachable from the
internet, and use HTTPS for public connections.

### Development

```sh
npm run server:dev
```

Builds the runtime, starts the API on `PORT` (8787) and serves the dashboard
with hot reload at `http://localhost:5173/server/` (`DEV_UI_PORT`). Vite proxies
`/api` to the API.

### Docker (Linux)

The image builds from the **repository root** (it needs the root lockfile and
the app sources the server bundles):

```sh
docker build -f server/Dockerfile -t tatakai-server .
docker run --rm -p 8787:8787 -v tatakai-data:/data \
  -e PUBLIC_URL=http://192.168.1.10:8787 tatakai-server
```

The build stage runs `npm ci --ignore-scripts` and `npm run server:build`; the
runtime stage installs production dependencies only and copies `server/src`,
`server/dist`, `desktop/runtime/compatibility` and `shared/aniyomi`. It runs as
`node` with `HOST=0.0.0.0`, `DATA_DIR=/data` (volume) and a health check on
`/api/server/v1/health`. Java and Playwright browsers are not installed;
add them in a derived image if you enable those adapters.

## Capability matrix

| Capability | Desktop (Electron) | Standalone Node / Docker |
| --- | --- | --- |
| Stremio HTTP addons | Yes | Yes |
| Tatakai / Seanime / Sora / Mangayomi scripts | Bounded QuickJS interpreter | Bounded QuickJS interpreter |
| Trusted Node bundles (e.g. Toko) | Owner opt-in (worker thread) | Owner opt-in (worker thread) |
| Installed `.kai` extensions of the app | Yes (via Electron) | No |
| Aniyomi / Mihon / CloudStream | Electron compatibility broker | `ENABLE_NATIVE_EXTENSIONS=1` (downloads its Java runtime) |
| Browser-dependent sources / Cloudflare | Electron browser bridge | `ENABLE_BROWSER_RUNTIME=1` (headless Playwright, no interactive sign-in) |
| Kodi | No execution adapter | No execution adapter |
| HLS, images, subtitles, Range/HEAD proxy | Yes | Yes |
| Anonymous access without viewer password | No (pairing or viewer password) | Yes (set a viewer password) |
| Pairing deep link / QR | Yes | Yes |

Actual support depends on each extension's APIs and provider availability.
Native code is owner-trusted software with host file and network access; a
worker thread contains crashes and timeouts but is not a security sandbox.

## Dashboard

- **Sources:** install, update, enable/disable, preferences, repositories.
- **Playground:** test a source before connecting clients.
- **Devices & Pairing:** one-time pairing QR / deep link and device-token revocation.
- **System & Settings:** passwords, allowed origins, timeout/concurrency,
  languages/order, cache limits, configuration export/import.
- **Live Logs:** owner-only structured request/source events (the last 200
  persist). Passwords, tokens, request bodies, query strings, upstream URLs and
  provider credentials are never logged. No telemetry is sent anywhere.

## Coming soon

Server mode for Tatakai Next desktop (`apps/desktop-native`), optional and
configurable.
