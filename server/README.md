# Tatakai Extension Server

[![Server checks](https://github.com/Snozxyx/TatakaiServer/actions/workflows/ci.yml/badge.svg)](https://github.com/Snozxyx/TatakaiServer/actions/workflows/ci.yml)

**Your sources, connected.** Install extensions once and use their anime, manga
and novel sources from Tatakai web, desktop and mobile.

```text
Sources → Extension runtime → Built-in media proxy → Tatakai
```

[![Set extension API](https://img.shields.io/badge/Tatakai-Set_extension_API-e84a7f)](https://tatakai.me/connect-server)
[![Vercel](https://vercel.com/button)](https://vercel.com/new/clone?repository-url=https%3A%2F%2Fgithub.com%2FSnozxyx%2FTatakaiServer&project-name=tatakai-extension-server&env=SERVER_SECRET,BOOTSTRAP_SECRET,DATABASE_URL,BLOB_READ_WRITE_TOKEN,PUBLIC_URL)
[![Cloudflare](https://deploy.workers.cloudflare.com/button)](https://deploy.workers.cloudflare.com/?url=https%3A%2F%2Fgithub.com%2FSnozxyx%2FTatakaiServer)
[![Heroku](https://www.herokucdn.com/deploy/button.svg)](https://heroku.com/deploy?template=https%3A%2F%2Fgithub.com%2FSnozxyx%2FTatakaiServer)
[![Render](https://render.com/images/deploy-to-render-button.svg)](https://render.com/deploy?repo=https%3A%2F%2Fgithub.com%2FSnozxyx%2FTatakaiServer)

The buttons use [Snozxyx/TatakaiServer](https://github.com/Snozxyx/TatakaiServer)
and open each provider's configuration flow. Render leaves plan selection to you;
Cloudflare defaults to Workers + D1 without an R2 subscription. Templates cannot
bypass provider account verification or billing rules. Users choose their storage
and optional upgrades. Heroku still provisions a paid database; Vercel requires
user-configured storage.

## Start locally

Requires **Node.js 22.16+**. This repository is independently buildable; its shared
adapters and UI primitives are included in `shared/tatakai` (see `THIRD_PARTY.md`).

```powershell
git clone https://github.com/Snozxyx/TatakaiServer.git
cd TatakaiServer
npm ci
npm run build
npm start
```

Open [the dashboard](http://localhost:8787). The startup banner prints dashboard,
API, health and connection URLs. No `.env` is required locally. Missing
`SERVER_SECRET` and `BOOTSTRAP_SECRET` values are generated once in
`data/node-secrets.json`, with a warning. Copy **bootstrapSecret** from this file
into initial setup. Keep it private and back it up with the database/artifacts.
The same encryption key must survive restarts; an installation previously using
an external key still needs its original key.

Setup: owner password → server name/viewer password → install extensions →
preferences → test a source → connect Tatakai. Admin/viewer passwords are separate
and optional. An owner without an admin password signs in with the setup key;
management is never anonymous. The encryption secret is separate from passwords.

To configure explicitly, copy `.env.example` to `.env` and replace placeholders.
`npm start` and `npm run dev` load it; environment variables take precedence.

## Set the extension API in Tatakai

1. Open **Settings → Extension Hub → Extension Servers**.
2. Set the extension API to the server **origin**, e.g. `http://localhost:8787`
   or `https://sources.example.com`. Tatakai adds `/api/server/v1` itself.
3. Enter the **viewer** password, or leave it blank for public access.
4. Confirm. Multiple servers can coexist with local extensions.


For public servers, use [the connection page](https://tatakai.me/connect-server)
or a share link with the full server origin URL-encoded:

```text
https://tatakai.me/extensionserver/https%3A%2F%2Fsources.example.com
```

Optional `?pass=VIEWER_PASSWORD` pre-fills the confirmation form and is removed
from the address bar. Prefer **Devices & Pairing → Generate Pairing QR** for a
single-use five-minute link without a password. Never share an admin password or
deployment secret. The dashboard also has **Set extension API in Tatakai**.

`tatakai.me` requires an **HTTPS** server. Test HTTP localhost using local Tatakai,
or use an HTTPS reverse proxy/deployment. Add custom Tatakai website origins in
Advanced settings. Authenticated clients use revocable bearer device tokens.

## Development

```powershell
npm run dev
```

This builds the runtime and starts both API **8787** and dashboard
[localhost:5173](http://localhost:5173), with UI hot reload. Keep this command
running. `PORT` changes the API port; `DEV_UI_PORT` changes the dashboard port.
Restart after server code/environment changes. Stop any older dashboard-only
Vite command with Ctrl+C first; the updated command fixes its missing-API proxy.

## Deploy

### Vercel

Use the button to import **Snozxyx/TatakaiServer**. Leave **Root Directory** at
the repository root and choose framework **Other**. The package's `vercel.json` builds the dashboard and
routes `/api/server/*` to its function.

Provide `SERVER_SECRET` (32+ random characters), `BOOTSTRAP_SECRET`, `PUBLIC_URL`
(your HTTPS origin), a **Neon Postgres** `DATABASE_URL` and a private Vercel Blob
store's `BLOB_READ_WRITE_TOKEN`. Apply `migrations/0001_state.sql` to Postgres
before setup. Keep storage and secrets attached across redeployments.

This adapter runs compatible HTTP addons/interpreted scripts, not trusted Node
bundles such as Toko. Proxy responses are deliberately bounded to independently
requested HLS resources and validated Range requests up to **4 MiB/25 seconds**,
within the configured function duration. Long/large responses fail explicitly;
function duration includes streaming. See [monorepo configuration](https://vercel.com/docs/monorepos)
and [function limits](https://vercel.com/docs/functions/limitations).

### Cloudflare Workers

Use the button with **Snozxyx/TatakaiServer**. Root `wrangler.jsonc` builds the
server and declares `DB` (D1), static assets, required secrets and observability.
It does not provision R2 or select a paid Workers plan. Provide `SERVER_SECRET` and
`BOOTSTRAP_SECRET` in the deployment flow. After resource provisioning, apply the
D1 schema **from the repository root**, then open the Workers HTTPS URL:

```powershell
npx wrangler d1 migrations apply DB --remote --config wrangler.jsonc
```

D1 stores configuration, logs, extension artifacts and encrypted cache bodies.
Artifacts are chunked below D1 row limits and written transactionally (up to
4 MiB per artifact). Apply both SQL migrations. Serverless memory is not durable
storage. HTTP addons and bounded scripts are supported; native
Java/browser/torrent adapters are unavailable. For manual CLI deployment,
set both secrets, provision the declared resources and apply migrations. See [Cloudflare's deployment flow](https://developers.cloudflare.com/workers/platform/deploy-buttons/).

R2 is optional: use `wrangler.r2.jsonc` if you choose to enable it. Existing R2
deployments should retain that binding/configuration until their artifacts are
migrated; changing storage does not copy existing artifacts automatically.

### Heroku

The button uses root `app.json`, `heroku.yml` and `Dockerfile`, configuring
a container web process, generated secrets and a **paid Postgres add-on**.
Set `PUBLIC_URL` to your app's HTTPS origin. Copy `BOOTSTRAP_SECRET` from Config
Vars into initial setup.

When `DATABASE_URL` exists, Node stores both state and artifacts in Postgres,
creating tables on startup. Dyno files are not durable state. The template uses
Heroku's documented `PGSSLMODE=no-verify`: the database connection is encrypted
but its certificate is not verified. For another provider, use its verified TLS
settings. Keep environment secrets stable. See [container setup](https://devcenter.heroku.com/articles/build-docker-images-heroku-yml)
and [Postgres settings](https://devcenter.heroku.com/articles/connecting-heroku-postgres).

### Render

The button uses root `render.yaml`: **no preset compute plan**, no paid disk or automatically
created database, generated secrets and health checks. Supply a Postgres
`DATABASE_URL` of your choice (for example, an external free-tier database).
State, artifacts, logs and cache are stored there. The service refuses to fall
back to temporary local files if the database URL is missing.

Automatic upstream redeploys are disabled. Public URLs use `RENDER_EXTERNAL_URL`;
set `PUBLIC_URL` for a custom domain. Select **Free** in Render's configuration
if you want free compute; Render may otherwise preselect its own paid default.
Retrieve `BOOTSTRAP_SECRET` from Environment
for setup. Users handle provider account checks, billing prompts and optional
upgrades themselves. Free services can sleep and cannot attach persistent disks;
see [Render's free-service limits](https://render.com/docs/free) and
[deployment-button flow](https://render.com/docs/deploy-to-render).

### Docker / self-hosted Node

Build from the **repository root**:

```powershell
docker build -t tatakai-server .
docker run --rm -p 8787:8787 -v tatakai-data:/data -e PUBLIC_URL=http://localhost:8787 tatakai-server
```

Use HTTPS for public connections. Containers use `HOST=0.0.0.0`; standalone Node
defaults to `127.0.0.1`. Optional Java/browser adapters need extra runtime packages;
base images do not install them.

## Dashboard and live logs

- **Sources:** installation, updates, enable/disable, preferences, repositories.
- **Playground:** source tests before connecting clients.
- **Devices & Pairing:** connection URL/QR and viewer-token revocation.
- **System & Settings:** passwords, origins, timeout/concurrency, languages/order,
  cache limits and configuration export/import.
- **Live Logs:** owner-only, refreshes every two seconds while open, with
  pause/resume, empty and error states. Last **200** structured request/source
  events persist across restarts/cold starts. Passwords, tokens, request bodies,
  query strings, upstream URLs and provider credentials are excluded. This is
  activity logging, not raw provider console output or shell access.

## Capability matrix

| Capability | Node / desktop | Render / Heroku Node | Vercel | Workers |
| --- | --- | --- | --- | --- |
| Stremio HTTP addons | Yes | Yes | Yes | Yes |
| Compatible Tatakai/Seanime/Sora/Mangayomi scripts | Bounded interpreter | Bounded interpreter | Bounded interpreter | Bounded interpreter |
| Trusted Node bundles (e.g. Toko) | Owner opt-in | Owner opt-in | No | No |
| Mihon/CloudStream/Aniyomi | Optional Java runtime | Extra runtime installation | No | No |
| Browser-dependent sources | Desktop / optional Playwright | Extra browser installation | No | No |
| Kodi | No execution adapter | No execution adapter | No | No |
| HLS, images, subtitles, Range/HEAD proxy | Yes | Yes | Bounded | Host limits apply |
| Library/downloads/household/consent/torrent/sharing | Desktop compatibility layer | No | No | No |

Actual support depends on each extension's APIs and provider availability;
ecosystem names are not universal compatibility claims. Themes/renderer UI remain
client features. Native code is owner-trusted software with host file/network
access, not a security sandbox.

## Validation

```powershell
npm test
npm run build
```

Tests cover API/auth separation, setup/revocation/redaction, persistence/CAS,
invalid packages/update rollback, URL protection, proxy/cache, progressive results
and provider priority. Shared-core platform tests do not constitute hosted
deployment tests. Local Workerd/D1/R2 and representative script/Toko executions
were exercised during development. Vercel, Heroku, Render and Docker still need
deployment verification in actual provider accounts.

## Used from TatakaiV6

This service is a Git submodule at `tatakai-server/` in the main Tatakai checkout.
Clone Tatakai with `git clone --recurse-submodules`, or run
`git submodule update --init --recursive` in an existing checkout. The website
connects through the API; the service can also be deployed independently.
