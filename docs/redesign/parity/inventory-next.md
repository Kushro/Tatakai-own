<!-- Inventario de código de Tatakai Next (desktop + Android) en main @ 1a12f42, generado el 2026-11-08 para docs/redesign/14-parity-plan.md. -->

## Tatakai Next: inventory of screens and features (desktop-native and android-native)

Paths are relative to `/media/octavio/DATOS/Gitbutler Repos/Tatakai-own/apps/`. Desktop UI = `desktop-native/crates/tk-ui/src/`, desktop wiring = `desktop-native/crates/tk-app/src/`, Android features = `android-native/feature/<x>/src/main/kotlin/app/tatakai/next/feature/<x>/`.

Scope note that applies to both apps: anime only (M0). Manga, novels and the reader do not exist anywhere.

---

### 1. Routes / screens

**Desktop.** The router enum is `Route` in `tk-ui/src/app.rs`. The rail has 7 entries and digits 1–7 jump to them: Home, Search, Library, Torrents, Extensions, Link, Settings.

| Route | Purpose |
|---|---|
| `/` HomePage | Greeting, hero carousel, Continue watching, feed rows (`screens/home.rs`) |
| `/search` | Search with filters, recent searches, master-detail on wide windows (`screens/search.rs`) |
| `/library` | Local Continue watching + History, Sync now (`screens/library.rs`) |
| `/details/:id` | Anime details: tabs, episodes, source panel, send-to-device panel (`screens/details.rs`). Also shown embedded as the right pane of Search |
| `/player/:id/:episode` | Player: embedded mpv, enhanced, or an external-mpv card (`screens/player.rs`, `components/player_controls.rs`) |
| `/extensions` | Install and manage .kai extensions (`screens/extensions.rs`) |
| `/link` | Tatakai Link server: pairing QR/code, devices, firewall, capabilities (`screens/link.rs`) |
| `/settings` | One page, 7 sections with a scroll-spy index (`screens/settings.rs`) |
| `/torrents/` Overview | Per-torrent stats and actions (`screens/torrents/overview.rs`) |
| `/torrents/files` | Per-torrent file table, read-only (`files.rs`) |
| `/torrents/peers` | Peer table + flag legend (`peers.rs`) |
| `/torrents/trackers` | Tracker table (`trackers.rs`) |
| `/torrents/library` | All saved torrents, multi-select delete (`library.rs`) |
| `/torrents/downloads` | Background downloads: warning gate, switch, list (`downloads.rs`) |
| `/torrents/settings` | Torrent engine settings form with a save bar (`settings.rs`) |
| `/gallery` | Component gallery, debug builds only (`screens/gallery.rs`) |

Sheets and dialogs that act as screens on desktop:
- Source picker sheet (`components/sources.rs::SourcePicker`)
- Search filter sheet (`search.rs::FilterSheet`)
- Send-to-device confirm dialog (Details)
- Remove-extension confirm
- Revoke / rename device dialogs (Link)
- Torrent remove/delete confirm, with an "also delete files" option
- Background-download warning (Downloads)
- Torrent switch menu (`torrents/layout.rs`)

There is no onboarding, no tray and no updater. Single-instance is enforced (`tk-app/src/single_instance.rs`). The window is frameless with its own title bar (`components/title_bar.rs`: window controls, offline indicator, "Link :port" chip).

**Android.** Keys are in `android-native/app/src/main/kotlin/app/tatakai/next/nav/Routes.kt`; the shell is `nav/TatakaiApp.kt` (NavigationSuiteScaffold + ListDetailSceneStrategy).
- Compact bottom bar: Home, Search, Library, You.
- Rail (600 dp and wider): Home, Search, Library, Torrents, Extensions, Link, Settings.

| Route | Purpose |
|---|---|
| Home | Search bar, hero, Continue, genre chips, feed rows, extension rows (`home/HomeScreen.kt`) |
| Search / SearchFor(query, feed, genre) | Search, or opened pre-filled from Home "See all", a genre chip or Related (`search/SearchScreen.kt`) |
| Library / LibraryTab(tab) | Tabs: Continue, History, Torrents (`library/LibraryScreen.kt`) |
| Details(kind, anilistId) | Details in the detail pane (`details/DetailsScreen.kt`, `DetailsSections.kt`) |
| Player(args) | Media3, libmpv or external app; also remote Link HLS and offline local files (`player/PlayerScreen.kt`, `PlayerControls.kt`, `ExternalPlayerScreen.kt`) |
| You | Account card + Historial, Descargas, Extensiones, Dispositivo enlazado, Ajustes (`you/YouScreen.kt`) |
| Settings / SettingsSection(section) | One sectioned screen, 8 sections; TORRENTS opens a subpage (`settings/SettingsScreen.kt`, `SettingsSections.kt`) |
| Extensions | Install via SAF and manage (`extensions/ExtensionsScreen.kt`) |
| Link / Linked | Same screen: Link client to the PC (`link/LinkedDeviceScreen.kt`) |
| ExtensionServers | Pair with the PC extension server (TatakaiServer / Electron); also opened by the `tatakai://extensionserver/connect` deep link (`link/ExtensionServersScreen.kt`) |
| Torrents(section, torrentId) | Torrent client: library or picker, detail with tabs Overview/Files/Peers/Trackers, Downloads, Settings (`torrents/TorrentsScreens.kt`, `ui/*`) |

Sheets and dialogs on Android:
- Source picker (`player/sources/SourcePicker.kt`)
- Search filters sheet (`search/SearchFiltersSheet.kt`)
- Subtitle style sheet, Licenses sheet, Clear-cache confirm (Settings)
- Paste-code dialog, QR scanner with CameraX + zxing (`link/QrScanner.kt`), Forget-PC confirm
- Deep-link connect confirm (ExtensionServers)
- Player sheets: Quality, Subtitles, Audio, Speed
- Metered-data confirm before a torrent (`player/PlayerOverlays.kt`)
- Download gate + POST_NOTIFICATIONS rationale (`torrents/DownloadGate.kt`)

Shell extras: offline banner, active-downloads badge on Library (compact) or Torrents (rail), torrent notification taps open `Route.Torrents`. No onboarding.

---

### 2. Features by area

**Home**
- Desktop:
  - Greeting by hour, with provider chips.
  - Hero carousel from the first 5 Trending items.
  - Continue watching opens the player directly; "See all" goes to Library.
  - Rows: Trending, Airing, Seasonal, Popular, Top rated (`tk-app/src/live/mod.rs::home`).
  - Each row fails on its own with Retry and a Kitsu-fallback hint; rows below the first 3 load when scrolled into view.
  - Row "See all" only opens a plain Search.
  - The UI can render extension `customHome` rows (`ext:` prefix), but the live backend never produces them.
- Android (`home/HomeViewModel.kt`):
  - Same 5 feeds, hero, Continue, greeting + provider chips.
  - Genre chips (most frequent genres) open Search by genre.
  - "See all" opens Search on that feed.
  - Lazy extension `custom/home` rows are implemented, but show the extension id instead of its name.
  - Status pills, including Cancelled and Hiatus.

**Search**
- Desktop (`search.rs`):
  - Debounced, with a guard against stale responses; 6 recent searches kept in localStorage.
  - Ctrl+K focuses the field; results are a grid, or list + embedded Details on wide windows.
  - Type chips: Anime is on; Manga and Novel are disabled with the "Coming in M1" tooltip.
  - Origin chips: Metadata plus one per enabled extension. Choosing an extension shows a "not wired" error.
  - Filter sheet: 18 genres, last 7 years, season, status. Only the year is applied (locally, on the subtitle); genre, season and status show a "not available in this build" banner.
  - Only page 1 of results; no Format filter.
- Android (`search/SearchViewModel.kt`):
  - Recent searches (SharedPreferences); type chips with Manga/Novel disabled.
  - Extension chips are visible but disabled (M1).
  - Filters (genre, year, season, status) are applied on the client to the loaded pages.
  - "Load more" pagination; browsing by feed and genre; list-detail on wide windows.

**Details**
- Desktop (`details.rs`):
  - Header: metadata chips, Watch/Continue split button whose chevron opens the source picker.
  - Tabs: Episodes (ranges of 100, Watched/Filler pills, resume progress), Info (synopsis with More/Less, genres, ids). Related is not wired.
  - Side panel "Stream source" (`SourceSummary`); if no extension is installed, a CTA to Extensions.
  - "Send to device" panel listing paired devices; the confirm dialog ends in a not-wired error.
  - Floating toolbar: Watch, Add to list ("Coming in M1"), Send, Download ("Coming in M1").
  - The torrent rows of the picker do offer "Download for later", which queues a background download.
- Android (`details/DetailsViewModel.kt`):
  - Episodes with a per-episode menu ("Download episode"); the Download menu also offers "Download season" (whole batch).
  - Info tab: AniList/MAL/Kitsu ids, other titles, season, status.
  - Related: client-side heuristic (titles from Trending + Popular sharing a genre); tapping opens Search by that genre, not another details page.
  - Source panel showing who picked the source (system or you) and "See all (n)".
  - Send to device works: it starts a Link session on the PC with a web stream and plays the HLS on the phone.
  - No "Add to list" and no Recap pill.

**Source picker / ranking**
- Both:
  - Auto/Manual mode, prefer web or torrent, minimum quality, preferred subtitle and audio language.
  - Ranked rows with reliability, latency, seeders/leechers, size, group, subs/dubs.
  - Release tags: SeaDex best, BD/WEB/REMUX, resolution, codec, bit depth, dual audio, multi-sub, batch (`tk-core/src/release.rs`, `android-native/core/release/ReleaseParser.kt`).
  - Filter bar that never reorders the ranking; failover; per-source reliability stats; picked source remembered per episode.
- Android only: a "Compatible with this device" codec filter and an incompatible-decode warning.

**Player**
- Desktop (`screens/player.rs`, `tk-player/`):
  - Engines (Settings > Player):
    - Embedded mpv (a hole in the web view).
    - Embedded enhanced: mpv-anime-build, with a choice of Tatakai controls or uosc.
    - External mpv: mpv found on PATH or at a set path; the page shows a card with Stop/Back.
    - On Linux, mpv falls back to its own window if DMA-BUF is unavailable.
  - Controls:
    - Transport: play/pause, ±5 s, seek bar with a time preview on hover, volume/mute.
    - Tracks and playback: subtitle and audio track menus, quality (switches source by quality; Auto = system choice), speed, "Smoothing" (interpolation).
    - Upscaling menu with presets Off, Anime4K A, A+A, B, C, FSR; Ctrl+1..6.
    - Window: PiP (small always-on-top window), fullscreen (F).
    - Flow: next episode, 10 s countdown card, source chip that opens the picker.
    - Skip intro/recap/ending button from AniSkip marks (MAL id) for any source.
  - Torrent playback:
    - Plays through a loopback HTTP URL; status line shows speed/peers/%.
    - Seek bar shows downloaded ranges and the ready-to-play range.
    - "Open in the torrent client"; "Next episode from the same torrent" for batches.
  - Progress is recorded to history.
  - Shortcuts (`tk-ui/src/shortcuts.rs`): Space, ←/→, F, Esc, Ctrl+1..6, Ctrl+B (nav), Ctrl+K (search), 1–7 (nav).
- Android (`player/PlayerViewModel.kt`, `PlayerControls.kt`, `core/player`, `core/player-mpv`):
  - Engines:
    - Media3: default; draws its own subtitles with the style settings; ASS shown simplified with a notice.
    - libmpv.
    - External app: chooser, mpv-android or VLC, with headers.
  - Gestures:
    - Tap toggles the controls.
    - Double-tap seeks ±10 s.
    - Horizontal drag scrubs.
    - Vertical drag on the left changes brightness, on the right volume.
  - Chips: Quality (only on adaptive Media3 streams), Subtitles, Audio, Speed, Next.
  - PiP, including auto-enter on Android 12+.
  - Skip intro/recap/ending, plus auto-skip intro when the setting is on; auto-next with a 10 s countdown (setting).
  - Failover with a "Tried" list of failed sources; metered-data confirm before torrents.
  - Plays remote Link sessions (mTLS HLS), offline completed downloads and files of a torrent already in the session.
  - Not present: upscaling menu in the player, fullscreen toggle, screenshot, screen lock, background playback.

**Library / history / continue / lists**
- Desktop (`library.rs`):
  - Continue grid (40) + history list; "Sync now".
  - Banner: "AniList/MAL lists arrive in M1". No lists.
- Android (`library/LibraryScreen.kt`):
  - Tabs Continue, History, Torrents; sync state "synced at …".
  - No AniList/MAL lists.
  - You > Historial opens the History tab.

**Torrents**
- Desktop (`tk-torrent` on librqbit; `tk-app/src/live/torrent*.rs`; `screens/torrents/*`):
  - Overview: pause, resume, open folder, remove (optionally with files), copy info-hash, protocol pills.
  - Files: searchable and sortable table, read-only.
  - Peers and Trackers tables.
  - Library: sortable table, keep-after-watching toggle, open in client, open folder, bulk delete.
  - Downloads: warning gate before enabling background downloads; list with pause/resume/remove.
  - Settings: download/upload limits, listen port, UPnP, max peers, DHT/PEX/LSD, download folder, after-watch keep/delete, background switch.
- Android (libtorrent4j; `core/torrent`, `feature/torrents`):
  - Same sections.
  - Files tab: play this file, download only this one, save to Downloads.
  - Share magnet link.
  - Foreground service with notifications (pause all, resume, watch, episode ready).
  - Android-only settings: Wi-Fi only, pause on low battery, upload on mobile data, seed after completion, storage in app folder or SAF folder (completed downloads move there), storage usage.
- Neither app lets you add a magnet or .torrent by hand.

**Extensions**
- Desktop (`extensions.rs`, `tk-ext`, rquickjs):
  - Install .kai by drag-and-drop, file chooser or pasted path.
  - Enable switch, remove with confirm.
  - Compatibility pill (Full/Partial/Unsupported) with an explanatory banner.
  - Permission chips (+n), SHA-256, domains, last-updated date, collapsible per-extension log.
  - Hidden-webview browser runtime for Cloudflare-protected sources (`tk-app/src/live/browser.rs`).
- Android (`extensions/ExtensionsScreen.kt`, `core/extensions*`, QuickJS):
  - Install via SAF; same management features.
  - Also shows kind (Torrent, Streaming, Custom), routes (streams/torrents), browser domains.
  - Hidden WebView runtime.
  - PC extension server client (`core/extension-server`, `ExtensionServersScreen.kt`):
    - Pair by QR, pasted link, or manual URL + viewer password.
    - Server list with enable/remove/refresh and status.
    - Host guard: plain http only on LAN or Tailscale addresses.
    - Deep link with a confirm step; benefits panel.
    - Server results merge into the source list.
- Neither app has extension repositories/catalog, per-extension settings or extension updates.

**Settings**
- Desktop (`settings.rs`), 7 sections:
  - Appearance: 10 palettes; mode System/Light/Dark/AMOLED; contrast Standard/Medium/High; language System/EN/ES; reduce motion.
  - Player: engine (3 modes, with a per-mode line); enhanced controls (ours/uosc); mpv-anime-build status + Prepare; external mpv path + detect again; default upscaling preset; source mode, prefer, minimum quality, subtitle language, audio language.
  - Metadata: provider priority (AniList, Kitsu, ani.zip) with up/down; cache row is text only.
  - Account: Supabase email/password sign in/out, sync now.
  - Link: port field (restarts the server), "Manage pairing".
  - Data & storage: telemetry "None"; clear cache, open folder and export history are disabled.
  - About: version, Link v1, license; Check for updates and Open logs disabled; Licenses only shows a snackbar with a filename.
  - There is no Torrents section here; it lives in Torrents > Settings.
- Android (`SettingsSections.kt`), 8 sections:
  - Appearance: palettes; dynamic color; mode System/Light/Dark; separate AMOLED switch; contrast; language (per-app locale); reduce motion.
  - Player: engine Media3/libmpv/External; external app; preset (with a battery note); source preferences; auto-play next; skip intro; subtitle style sheet (size, background, font Outfit/Space Grotesk/System, bold, outline, preview); link to Extensions.
  - Metadata: priority, cache size, TTL selector.
  - Account: sign in/out, sync now, error variants (invalid, suspended, email not confirmed).
  - Link: opens Linked device.
  - Torrents: subpage.
  - Data: storage bar (cache/torrents/free), clear cache with confirm (works), telemetry.
  - About: component versions, Licenses sheet (works), Open logs (M1 snackbar).

**Account / sync**
- Both: Supabase GoTrue password login + `watch_history` upsert and select only (`tk-core/src/supabase.rs`; `android-native/core/supabase`, `core/data/HistorySync.kt`). No sign-up, no OAuth, no AniList/MAL.
- Desktop: the session lives in memory only and is lost on restart. Android: SharedPreferences.

**Link / pairing**
- Desktop is the server (`tk-link`):
  - Pairing window: QR + 10-character code, 5 min, single use, with countdown and "Open a new window".
  - Server card: status, address, CA fingerprint, start, restart on another port, readable port-in-use error.
  - Paired devices: rename, revoke, online / last seen.
  - Windows: firewall rule button. Linux/macOS: firewall hint card.
  - Capabilities: GPU, encoder, upscaling, profiles.
  - mTLS sessions doing FFmpeg transcode + Anime4K upscaling to HLS.
- Android is the client (`core/link`, `LinkedDeviceScreen.kt`):
  - Pair by QR or pasted payload/code.
  - Status card, capabilities, enhance-profile selection.
  - "Enhance a stream" by URL, Forget PC.
  - Send-to-device from Details.

**Themes / i18n**
- Both: 10 palettes, contrast levels, reduce motion; English + Spanish only (`tk-ui/i18n/{en,es}.json`; Android `values` / `values-es` per module).
- Android also has dynamic color.

**Tray / updater**
- Not present in either app.

---

### 3. Stubbed, placeholder, disabled or "coming soon" (explicit)

**Desktop**
- Search:
  - Manga/Novel chips disabled ("Coming in M1", `search.rs`).
  - Search inside an extension shows `not_wired("search by extension")`.
  - Genre, season and status filters are not applied (banner `error.not_wired`); only page 1 of results.
- Details:
  - Related tab is `not_wired("related")`.
  - Send to device confirm ends in `not_wired("send_to_device")`.
  - Add to list and header/toolbar Download show the "Coming in M1" snackbar (`details.rs` lines 233, 235, 444, 446).
- Player:
  - Screenshot button only flashes `error.not_wired`.
  - Audio menu shows a disabled "Auto" item when there are no tracks.
  - The `playback_source` path returns `ErrorKind::TorrentM2` ("Torrent arrives in M2") for torrent sources. The player uses `torrent_play` instead, so this text is a leftover.
- Settings:
  - Data: Clear cache, Open folder, Export history are `disabled: true` (`settings.rs` 402–425).
  - About: Check for updates and Open logs are disabled; Licenses only shows a snackbar with "THIRD_PARTY_NOTICES.md".
  - Metadata cache row has no control.
  - Account shows "AniList and MAL lists arrive in M1"; Library shows the matching `lib.m1` banner.
- Home: extension rows are supported by the UI but the backend never returns them.
- Build without the `live` feature: every call returns NotWired (`tk-app/src/offline.rs`; defaults in `tk-ui/src/backend.rs` 695–837).
- Extension compatibility:
  - Node modules that are stubbed, giving "Partial": os, http, https, stream, zlib, string_decoder, assert, timers, readline, perf_hooks, async_hooks, module, tty.
  - Node modules that are unsupported, giving "Unsupported": fs, child_process, net, tls, dgram, cluster, worker_threads, vm, dns, http2, inspector, v8, repl.
  - Lists in `tk-ext/src/compat.rs`.
- i18n keys defined but never used:
  - Player: `player.prev`, `player.subtitle_style`, `player.enhance_menu`.
  - Settings: `settings.player.decoder*`, `settings.player.autonext*`, `settings.player.skip*` (no such toggles on desktop), `settings.data.cleared`, `settings.about.uptodate`.
  - Account/Link: `account.pending`, `link.session`, `link.stop` (no stop-server button).
  - Details/Search: `details.studio/season/format/duration/min/in_list/recap`, `search.format`, `search.source`, `search.select_prompt`.
  - Feeds: `feed.upcoming`, `feed.recently_completed`.
  - Sources: `src.use`, `src.using`.
  - Misc: `offline.banner`, `common.shortcuts/done/example/optional`, `ext.install.manifest`, `torrent.placeholder` ("This part of the torrent client is being built.").

**Android**
- Search: Manga/Novel chips and extension source chips are disabled with "Coming in M1" (`SearchScreen.kt` 258–289).
- Link:
  - "Continue on the PC" shows the M1 snackbar.
  - Remote control: Prev/Play/Next buttons are `enabled = false` with the "arrives in M1" hint.
  - Hint: "Finding PCs on your network automatically arrives in M1"; `LinkRepository.discover()` always returns empty.
  - Capabilities card has no GPU/encoder/upscaler fields.
- Settings:
  - About > Open logs shows the M1 snackbar.
  - Account: "AniList and MAL lists arrive in M1"; no pending-changes counter.
- Details: "No torrent source for this episode" appears as a disabled menu item.
- ExtensionServers: "Coming soon: Server mode for the Tatakai Next desktop app".
- Shader presets are saved, but no Anime4K/FSR shaders are bundled, so `MpvEngine.applyPreset` is a no-op and Media3 ignores them. The "turns off below 20% battery" hint has no matching code.
- String `library_m1` is defined but unused.

---

### 4. Planned or deferred in the design docs and not implemented

Sources: `docs/redesign/02-ux-flows.md`, `04-implementation-plan.md` §1 and §8, `03-pairing-upscale.md`, `13-integrated-server-contract.md`, `KNOWN-ISSUES.md`.

**M1**
- Manga/novel details + reader (pinch zoom, prefetch).
- AniList/MAL lists in Library (Watching, Planned, Completed, Paused, Dropped), read and write, with OAuth linking; extra accounts in Settings.
- Extension search and origin tabs.
- Link: mDNS + TOFU discovery, `/v1/settings`, remote PC library, handoff ("Continue on…"), remote control, events.
- MAL/Shikimori/Tenrai providers in the router.
- Baseline profile and macrobenchmark.
- Android Anime4K local (shaders not bundled).

**M2**
- Watch rooms, debrid, library/download sync over Link, `phone` source, TV, Stremio compatibility, HEVC.
- Public profile page.
- Isolated `:extensions` process.

**M3**
- On-device translation, community, stats/Wrapped, IPTV, RIFE/frame generation, image upscaling in the reader.
- Mangayomi/Seanime compatibility.

**Never in scope (cut or merged)**
- Cut: DynamicIsland, promo popups, marketplace, renderer/theme extensions.
- Merged into other screens: calendar, genre pages, favorites, collections, playlists, offline library.

**Spec'd in 02 but missing**
- Details: Characters tab, poster shared-element transition.
- Player: thumbnail seek previews, screen lock (Android), decoder setting (desktop).
- Search: Format filter.
- Both: a Settings Accounts section for AniList/MAL.

**Desktop server mode for the extension server**: out of scope, marked "Próximamente" (13 §1).

**KNOWN-ISSUES highlights**
- Desktop:
  - No updater or installer signing.
  - Supabase session not persisted.
  - `tk-link` does not record device platform or last seen.
  - LinkHost does not expose the local library.
  - Torrent Files page is read-only: when the episode file is not found, nothing lets you pick or play a specific file.
  - Trackers always show Unknown; PEX cannot be turned off (librqbit); no per-peer progress or country.
  - Fonts (Space Grotesk, Outfit) are not bundled.
- Android:
  - No MediaSessionService, so no background playback.
  - No preload of the next episode, no fullscreen toggle, no screenshot.
  - Quality selection only on adaptive Media3 streams.
  - Torrent notification "Watch" only opens the client.
  - No GeoIP; no D-pad/TV support.
  - Room uses destructive migration, so unsynced local history is lost on update.

---

### i18n cross-check
- Desktop `en.json` and `es.json` both have 763 lines and the same key set.
- The Android `values` / `values-es` pairs match across modules; small line-count differences come from plurals.
- Strings that point to missing features are the M1/M2 markers and the unused keys listed in section 3.
