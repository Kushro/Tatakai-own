# Upstream sync: Snozxyx/Tatakai 6.3.0 -> 6.4.2

Merge of `upstream/main` (32 commits, merge base `80bfc19`, about 930 files) into
the hardened fork. Rule: the fork wins on anything that talks to upstream
infrastructure; everything else is taken. Package version is `6.4.2-own`
(Android `versionName` too; `versionCode` follows upstream, 642).

## Commit table

| Upstream commit | Subject | Result | Notes |
|---|---|---|---|
| e7bdb5a | Add TatakaiServer submodule + server docs | Discarded | Submodule and `.gitmodules` removed; `server:*` npm scripts, `tatakai-server/**` in electron-builder `files`, `verify-extension-server-package` removed. Client code kept (see below). |
| 0863f02 | Update server submodule | Discarded | Same. |
| 71b073e | Release 6.3.0 (extension runtimes, translation workspace) | Integrated | Aniyomi/Mihon/CloudStream/Sora/Mangayomi/Suwayomi runtimes, translate-engine, Yomi editor. Aniyomi bridge artifacts are now built from source in `Kushro/TatakaiServer-own` (see Risks). |
| 24d903b, a12ecd5, 612206f, ba977d8 | Translation preview, landing media, release checks | Partially | Code/assets taken; platform release checks that depend on upstream CI dropped with the workflows. |
| a41550d | Extract inference runtime links safely | Integrated | |
| 677d18b | Preserve community-server media proxy tickets | Integrated | Only touches user-run Tatakai servers. |
| c61d3c8 | Deliver manually requested website builds | Discarded | Upstream Vercel/website deploy workflow. |
| d1eaf2e | Keep video settings and community connections in browser | Integrated | |
| 690c6d3, 359d2e8 | Android TV status docs | Integrated | Docs only (readme itself kept as the fork's). |
| 5c8ca13, 5f64106, dd3f574 | Dependency bumps (tokio, npm groups, markdown-it) | Integrated | vite 8, electron 42, tiptap 3, zod 4, sharp 0.35, dompurify, etc. Lockfile regenerated with `npm install`. |
| 0549cef, 1eb8974 | Security: desktop IPC, local services, RLS, OAuth (+ review regressions) | Partially | Desktop IPC guards, extension worker permission-model isolation, home-server origin guards, app-URL parsing, `send-notification` auth, RLS migration `20261026000007` taken. The `tatakai.me` origin allowances were removed. Upstream's AniList code-exchange + `state` flow NOT taken: the fork keeps its client-side implicit grant with single-use pending marker and user confirmation, and `scripts/check-security-baseline.mjs` AUTH-1 was rewritten to assert that design. `SECURITY.md` kept, report URL pointed at `Kushro/Tatakai-own`. |
| d785860 | Repair featured badge selection / watch-save trigger | Integrated | Migrations `20261026000001`, `20261019000100`. |
| 3bb6730 | Novel homepage through one cached API bundle | Discarded | Depends on TatakaiAPI `/api/v3/novel/home`. Fork's router-based `useNovelHome` kept. |
| ea387d6 | Load reading sources progressively, connect catalog servers on web | Partially | Progressive source loading taken; server-backed bits only work against user-run servers. Chapter fetch now passes manga detail as extension title context (`getMangaChapters(..., metadata)`). |
| 70e8a51 | Profile themes, peer watch rooms, mobile reader improvements | Integrated | Profile themes/cosmetics/widgets, P2P watch rooms (WebRTC over Supabase realtime, Cloudflare STUN, user-supplied TURN), reading views, pinch zoom, themed selects. |
| 755b50b, 004c98b, 7cd222d, c3ec6f4 | Merged PRs 23-26 into release/6.4.0 | Integrated | |
| ef9fb6d | Release 6.4.0 | Partially | Everything but upstream release plumbing. Contains the Discord-webhook changelog script/workflow (discarded). |
| 135fc4c, d049731, e9ede46 | Build/release CI fixes (manualChunks, release upload retries, makensis) | Partially | `vite.config.ts` chunking fixes taken; `.github/workflows/build.yml`, `deploy-server-b.yml`, `macos-rebuild.yml` kept at the fork's version (publish to `Kushro/Tatakai-own`). `changelog-discord.yml` removed. `security.yml` and `translate-engine.yml` (verification only, no publishing) kept. |
| ebb3b28 | Release 6.4.1 | Integrated | |
| 95a62b6 | Release 6.4.2 (mobile playback recovery, opt-in cloud proxy) | Partially | Mobile playback recovery, SystemBars/inset handling, TV focus, reader fixes taken. Opt-in cloud proxy (`hoko.tatakai.me`) removed: `mobile-cloud-proxy.ts`, its settings section and locale keys gone; `useMobileCloudProxy` is an inert stub. |

## Discarded items and why

| Item | Why |
|---|---|
| `tatakai-server` submodule, `.gitmodules`, `server:*` scripts, packaging checks | External repo owned by upstream; fork does not run their server. `desktop/runtime/home-server` now degrades gracefully when the embedded extension server is absent (legacy home server still starts, extension-server routes answer 503). `src/core/extensions/server/client.ts` got a local `readApiResponse` instead of importing from the submodule, so connecting to a user-run Tatakai server still works. |
| `hoko.tatakai.me` cloud proxy | Routes video URLs through upstream infra. |
| `scripts/write-client-env.mjs` + test | Hard-codes `db.tatakai.me`, `api.tatakai.me` defaults for client builds. |
| `src/hooks/media/useCustomTracker.ts` behaviour | Posted the Supabase JWT to TatakaiAPI `/sync/extension-progress` and read tokens from `profiles` (moved to an owner-only table). Now a no-op with the same signature. |
| `ContentCatalogManager` (admin) | TatakaiAPI `/admin/content`. |
| `getNovelHome` | TatakaiAPI bundle. |
| BetterStack status fetch (`src/lib/betterStackStatus.ts`) | Pointed at `api.tatakai.me/service-status.json`; now only fetches if `VITE_SERVICE_STATUS_URL` is set. |
| Hosted iOS Turnstile page | `https://tatakai.me/security/turnstile.html`; now `VITE_TURNSTILE_HOSTED_URL` (empty = Turnstile disabled on packaged iOS, as before). |
| `wsrv.nl` image rewriting (`src/lib/media/imageUrl.ts`) | Third-party service would see every image URL and viewer IP; passthrough unless `VITE_IMAGE_PROXY=wsrv`. `tests/media-optimization.test.ts` removed. |
| `BRAND_ASSET_BASE` on tatakai.me | Always local `assets/brand`. |
| Upstream AniList OAuth code flow, `tests/anilist-oauth-state.test.ts`, `.env.example` upstream URLs | See security row above. |
| Extension marketplace/store routes, `extensions.tatakai.me` default, `/api/proxy/extension`, Publish button | Extension store not used (local `.kai` sideload). The new sideload modal (file picker, native repository tab) was taken. |
| `public/altstore/apps.json`, readme, GitHub links to `Snozxyx` | Branding/distribution of upstream. |
| `StatusPage` | Fork's router-based status page kept whole. |
| Tests that import `../TatakaiAPI/**`, `changelog-discord`, `ios-turnstile`, `betterstack-status`, `profile-theme-repair` (needs a file upstream never published) | Targets removed or unshippable. |

## Migrations

Fourteen new upstream migrations (`20261019000000` .. `20261026000007`) were
added unchanged. None re-adds `profiles.anilist_*`/`mal_*` token columns or any
table the fork dropped. `20261026000007` (RLS hardening) is guarded with
`IF EXISTS`/`to_regclass` and creates `security_is_staff()`/`security_is_admin()`.
`npm run check:migrations`: 166/166 valid. The static check does not execute
SQL; apply them to a scratch Supabase project before pushing.

Ordering note: `20261019*`/`20261020*`/`20261023*` sort before the fork's
`20261022000000_user_integration_tokens.sql`. A project that already applied
later fork migrations needs `supabase db push --include-all`.

## Risks to review

- Aniyomi bridge jar/apk (`desktop/runtime/compatibility/aniyomi-artifacts.json`) are downloaded from `github.com/Snozxyx/TatakaiServer` releases, verified by SHA-256 and byte length, but you are trusting that pin. Android's `preBuild` runs `prepare-compatibility-assets.mjs` which fetches them.
- Rich embeds (`ExternalLinkEmbeds`) call the `discord.com` invite API for user-posted invite links; nekos.best and Cloudflare STUN are used by the home section and P2P rooms. None are upstream infra.
- `useCustomTracker` is disabled, so extension-source progress is not pushed to AniList/MAL.
- Supabase generated types are stale for several new tables/columns, as they were before the merge.
- Package license field is `MPL-2.0` and `LICENSE` is MPL (upstream relicensed).

## Verification

- `npm run type-check`: 132 errors, all pre-existing (137 at fork head); no new ones.
- `npm test`: 1154 pass / 79 fail / 23 errors (fork head: 864 / 45 / 25 on fewer files). Every failing file passes in isolation except five that also fail at fork head; the rest is `mock.module` cross-file pollution from the added test files.
- `npm run check:migrations`: 166/166. `npm run check:i18n`: pass. `node scripts/check-security-baseline.mjs`: pass.
