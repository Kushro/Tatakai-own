<!-- Inventario de código del Tatakai clásico (src/, desktop/, server/) en main @ 1a12f42, generado el 2026-11-08 para docs/redesign/14-parity-plan.md. -->

# Legacy Tatakai app: inventory of screens and features

ROOT = `/media/octavio/DATOS/Gitbutler Repos/Tatakai-own`. Every path below is relative to ROOT. I left out `apps/`, `packages/` and `contracts/`.

## 0. Backend dependency summary (applies throughout)

**Router and shell**
- Router: `src/routes/AppRoutes.tsx`. Shell: `src/App.tsx`.
- Web uses BrowserRouter. Packaged Electron/Capacitor (`app://`, `file://`) uses HashRouter.
- If `isTV()` is true, `src/layouts/TVLayout.tsx` replaces `MainLayout`.

**Catalog and metadata are local/third-party, not tatakai.me**
- The metadata router `src/lib/metadata/router.ts` tries providers in the user's order and falls back on failure: AniList, MAL, Kitsu, Shikimori, ani.zip, Tenrai (`src/lib/metadata/providers/*`).
- Also used: Jikan, MangaDex, MangaUpdates, MangaBaka (`src/core/content/*`).
- `src/lib/api/backendOrigin.ts` says "This fork no longer depends on TatakaiAPI". It returns "" unless `VITE_BACKEND_ORIGIN` / `VITE_TATAKAI_API_URL` is set.
- `.env.web` still sets `VITE_BACKEND_ORIGIN=https://api.tatakai.me` and `VITE_SUPABASE_URL=https://db.tatakai.me`. `.env` points at the user's own `*.supabase.co` project.
- Stream resolution is extension-local. `fetchCombinedSources` in `src/lib/api/api-client.ts` notes "Central dispatch removed".
- The stream proxy defaults to the local server `http://127.0.0.1:8099` (`src/lib/api/proxy-utils.ts`).

**Supabase (hosted)** holds accounts and everything social. Tables are in `src/integrations/supabase/types.ts`:
- Auth and profiles
- watch_history, watchlist, manga_readlist, ratings, reviews
- comments, forum, community groups and events, notifications
- playlists, tier lists
- watch_rooms and their chat, polls and queue
- follows, achievements, badges
- admin tables: popups, maintenance_mode, redirects, update_policies, app_releases, status incidents, reports, suggestions, bans, analytics
- Realtime sync across devices is in `GlobalListeners` (`AppRoutes.tsx`).
- `BackendStatusContext` (`src/contexts/BackendStatusContext.tsx`) tells apart online, no-internet and server-down. Offline-capable routes stay reachable: `/downloads`, `/offline*`, and `/watch|/manga/read|/novel/read` with `?offline=true`.

**Other hosted or third-party dependencies**
- Extension marketplace: `VITE_EXTENSION_API_URL` (extension.tatakai.me), in `src/core/extensions/marketplace-client.ts` and `store-api.ts`.
- tatakai.me links: `/link`, `/download`, the altstore JSON, share links.
- Third-party services:
  - trace.moe (image search)
  - AniSkip
  - Giphy
  - Cloudflare Turnstile
  - GitHub releases (Kushro/Tatakai-own) for mobile updates
  - Sentry, Datadog and GA
  - Discord
  - Translation providers: Google, DeepL/DeepLX, Azure, OpenAI
  - TTS: OpenAI, ElevenLabs
  - Debrid: Real-Debrid, TorBox, AllDebrid, Premiumize

**Feature flags** (`src/core/feature-flags/feature-flags.ts`): content_graph, virtual_grid, blurhash_images, faceted_search, voice_search, tatakai_neural, download_manager, manga_download, ai_subtitle_translation, extension_scraping, warp_tunnel, torrent_engine, anime_calendar, tatakai_wrapped, watch_together, cross_device_sync, character_db, smart_theme_creator, anime_news_feed, tatakai_runtime, web_watch_gated.

## 1. Routes (path → component → purpose)

**Entry, landing, info**
- `/` → `pages/base/HomeGate.tsx` → native/desktop gets the full home `Index`; web guests get `LandingPage`.
- `/browse` → `pages/base/Index.tsx` → the app home (see §2).
- `/welcome` → `pages/base/LandingPage.tsx` → marketing landing. Sections live in `components/landing/*`: FeatureShowcase, Extensions, Integrations, MobileShowcase, Translate, Progression, MoreFeatures.
- `/download` → `pages/base/DownloadPage.tsx` → per-OS downloads: Windows, mac DMG, AppImage/deb, APK, iOS IPA/AltStore source, standalone server. Pulls GitHub stars and the latest release.
- `/discord` → `pages/base/DiscordPage.tsx` → promo page for the Tatakai Discord bot.
- `/faq/seanime-vs-tatakai`, `/faq/extension-support` → `pages/faq/*` → public FAQ pages.
- `/status` → `pages/error/StatusPage.tsx` → service status: health, latency, incidents, proxy pool. Uses `VITE_SERVICE_STATUS_URL` / BetterStack plus Supabase status_incidents.
- `/terms`, `/privacy`, `/dmca`, `/community-guidelines` (alias `/community-rules`) → `pages/legal/*`.

**Auth, onboarding, OAuth**
- `/auth` → `pages/auth/AuthPage.tsx` → Supabase email/password login and register, 6-digit email OTP verification, Turnstile, legal consent.
- `/reset-password`, `/update-password` → `pages/auth/ResetPasswordPage.tsx`, `UpdatePasswordPage.tsx`.
- `/onboarding` → `pages/auth/OnboardingPage.tsx` → after signup: choose an experience preset (ultra/lite, reduce motion), avatar and banner, display name, community guidelines, theme pick. Forced when `tatakai_onboarding_complete` or `theme_selected_v2` is missing.
- `/setup` → `pages/auth/SetupPage.tsx` → native first-run wizard, forced on native until `tatakai_setup_complete`. Steps: storage and download directory, popular extensions, country/region torrent-policy acknowledgement, debrid, Discord RPC, privacy and mobile permissions, terms, Discord/GitHub links.
- `/integration/mal/redirect` (alias `/api/mal/redirect`), `/integration/anilist/redirect`, `/integration/mangabaka/redirect` → `pages/auth/*RedirectPage.tsx` → OAuth callbacks.
- `/mal/:id`, `/anilist/:id` → redirect to `/anime/{mal|anilist}-:id`.

**Anime**
- `/anime/:animeId` → `pages/watch/AnimePage.tsx` → anime details (§2).
- `/watch/:episodeId` → `pages/watch/WatchPage.tsx` → episode player page (§2). Supports `?offline=true`.
- `/char/:charname`, `/character/:charname` → `pages/base/CharacterPage.tsx` → character bio (age, birthday, blood type, gender), appearances, favourite toggle.
- `/search`, `/search/producer/:producerName`, `/image-search` → `pages/base/SearchPage.tsx`.
- `/genre`, `/genre/:genre` → `pages/base/GenrePage.tsx` → genre catalogue.
- `/trending` → `pages/base/TrendingPage.tsx` → trending today/week/month/all-time, most-watched on Tatakai (Supabase views), AniList fan favorites, trending manga and light novels.
- `/calendar` → `pages/base/CalendarPage.tsx` → AniList weekly airing schedule, "My shows" pinned via TrackShowButton, jump to date, add/remove to calendar.
- `/recommendations` → `pages/base/RecommendationsPage.tsx` → For You (anime/manga/novels) with match score, why-explanations (genre/studio/era/theme/rating fit, community signal), taste profile, prediction accuracy, not-interested/skip feedback, and the Anime Atlas map (`components/atlas/AnimeAtlasPanel.tsx`).
- `/collections` → `pages/profile/CollectionsPage.tsx` → curated collection tabs: airing, popular, upcoming, season, MAL top.
- `/favorites` → `pages/profile/FavoritesPage.tsx` → watchlist hub by status (watching, planned, completed, on hold, dropped), AI picks, taste-model stats.

**Manga and novels**
- `/manga` → `pages/manga/MangaHomePage.tsx` → manga home with a gated 18+ zone.
- `/manga/discover`, `/manga/genre[/:genre]` → `pages/manga/MangaGenreBrowsePage.tsx` → catalogue browse.
- `/manga/:mangaId` → `pages/manga/MangaPage.tsx` → manga details.
- `/manga/read/:mangaId` → `pages/manga/MangaReaderPage.tsx` → reader.
- `/novel` (also `/novel/comingsoon`) → `pages/novel/NovelHomePage.tsx`. `NovelComingSoon.tsx` is a placeholder.
- `/novel/discover`, `/novel/genre[/:genre]` → `NovelGenreBrowsePage.tsx`.
- `/novel/:novelId` → `NovelPage.tsx`.
- `/novel/read/:novelId` → `NovelReaderPage.tsx`.
- `/translate` → `pages/translate/TranslatePage.tsx` → "Yomi" manga page translation editor, desktop only. Projects, page import, layer inspector, canvas brush, source/target language, provider presets (cloud LLM or offline local model), scope (page, selected pages, whole project), export including PSD. Components in `components/translate/*`; backend in desktop `ipc-translate.cjs` and `runtime/translate/*`.

**Custom sources (extension verticals)**
- `/custom-providers` → `pages/custom/CustomProvidersPage.tsx` → list provider pages from installed extensions; enable and browse.
- `/x/:namespace/:sourceId` → `CustomHomePage.tsx` → that provider's catalogue rows.
- `/x/:ns/:src/info/:id` → `CustomInfoPage.tsx` → title page: episodes/chapters, filter and sort, local bookmark, mark completed, chapter download, CustomCatalogPanel.
- `/x/:ns/:src/watch/:id/:episodeId` → `CustomWatchPage.tsx`.
- `/x/:ns/:src/read/:id/:chapterId` → `CustomReadPage.tsx`.

**Extensions and servers**
- `/extensions` → `pages/base/ExtensionHubPage.tsx` → store: browse and search, filters (ecosystem, compatibility, type: streaming, manga, torrent, utilities), trending, your library, publish link, ExtensionServersSettings, InstalledCompatibilityExtensions, RemoteExtensionsPanel.
- `/extensions/:extensionId` → `ExtensionDetailPage.tsx` → overview, readme, release notes, versions, permissions, capabilities, health, downloads and views, source-code view (desktop), open debug window, install/remove.
- `/extensionserver/*` → `pages/base/ExtensionServerLinkPage.tsx` → confirm pairing with an Extension Server (`tatakai://extensionserver/connect?server=&code=`) or connect with URL plus password.
- `/connect-server` → `ConnectServerPage.tsx` → legacy pairing-link alias.
- `/link` → `pages/tv/TVLinkPage.tsx` → approve a TV login from a phone/web (Supabase OTP).

**Profile and library**
- `/profile`, `/user/:username`, `/@username` (via the catch-all) → `pages/profile/ProfilePage.tsx`. `PublicProfilePage.tsx` also exists.
- `/social/:username` → `FollowConnectionsPage.tsx` → followers and following.
- `/notifications` → `NotificationsPage.tsx` → mark all read.
- `/stats`, `/wrapped` → `WrappedPage.tsx` → Wrapped stats: hours, episodes, best streak, monthly activity, rank badges, RP progress.
- `/downloads` → `MobileOfflinePage.tsx` on Capacitor, otherwise `OfflineLibraryPage.tsx`. `/offline-library` and `/offline` → `OfflineLibraryPage.tsx`.
- `/playlists`, `/playlist/:playlistId` → `PlaylistPage.tsx` (also exports `PlaylistViewPage`). `/p/:shareSlug` → `PublicPlaylistPage.tsx` (public, embeddable).
- `/tierlists`, `/tierlist/:shareCode` → `TierListPage.tsx` / `TierListViewPage`. `/tierlists/edit/:id` → `TierListEditPage.tsx`.

**Community**
- `/community` → `pages/community/CommunityFeedPage.tsx` → social feed.
- `/community/bookmarks` → `BookmarksPage.tsx`.
- `/community/c/:slug` → `CommunitySpacePage.tsx` → join/leave, members and ranks.
- `/community/c/:slug/information` → `CommunityInformationPage.tsx` → about and rules.
- `/community/c/:slug/settings` → `CommunitySettingsPage.tsx` → name, banner, delete.
- `/community/forum/:postId` → `pages/forum/ForumPostPage.tsx` → post view, pin, delete. `ForumNewPostPage.tsx` exists, but `/community/forum/new` redirects to `/community`.

**Watch together**
- `/isshoni` → `pages/watch/IsshoNiPage.tsx` → Watch2Together lobby: discover public live rooms, create a room (name, anime, sub/dub, public or password, max viewers 5/10/20/50, start now or scheduled), your active rooms.
- `/isshoni/room/:roomId` → `WatchRoomPage.tsx` → room (§2).

**Admin and status**
- `/admin` → `pages/admin/AdminPage.tsx` → staff dashboard (§2). `/admin/user/:userId` → `AdminUserPage.tsx`.
- `/maintenance`, `/banned`, `/503`, `/error` → `pages/error/*`, guarded. `NoInternetPage` is shown by `OfflineGate`. `NotFound` is the catch-all.

**Other**
- `/settings` → `components/settings/SettingsRouteOpener.tsx` → opens the Settings modal (§3).
- `/:slug` → `CatchAllHandler`: `@username` profile, then an extension-registered page (`extensionRegistry.getPages()`), then the Supabase `redirects` table, then NotFound.

**Global overlays that act as screens** (mounted in `src/App.tsx` and `src/layouts/MainLayout.tsx`)
- Settings modal (`components/settings/SettingsModal.tsx`)
- GlobalSearch Ctrl/Cmd+K palette (`components/layout/GlobalSearch.tsx`, recent searches)
- ExtensionUpdateSheet
- UpdateChangelogSheet
- PopupDisplay (admin popups and banners)
- V6AnnouncementPopup
- ReduceMotionPrompt
- AppDownloadBanner, MobileUpdateBanner, OfflineBanner
- DynamicIsland (desktop titlebar pill showing download/torrent/update/watch activity)
- EasterEggs (Konami code, secret words, tab-away title)
- MediaQuickPeek / PeekSheet (hover/long-press quick peek with add-to-watchlist)
- ImageLightbox
- NotificationSheet / NotificationModal
- LeaderboardSheet
- ReportModal
- ReviewPopup (app rating)
- AdultWarningGate
- BadgeReward / Celebrate
- DevConsole / LogViewer (developer mode; Ctrl+Shift+I log viewer on desktop)
- TitleBar (desktop)

**Navigation**
- Sidebar (`components/layout/Sidebar.tsx`): Home, Search, Manga, Novels, Community, Custom sources (+). The "More" menu has Trending, Calendar, Favorites, and Translate on desktop.
- Header (`components/layout/Header.tsx`): search with suggestions, image search (trace.moe), catalog switcher anime→manga→novel, status link, profile menu, admin link.
- MobileNav (`components/layout/MobileNav.tsx`): Search, Favorites/Trending, Custom, Social, Downloads, with haptics.

**TV layout** (`src/layouts/TVLayout.tsx`, D-pad spatial navigation in `src/core/tv/spatialNavigation.ts`)
- `/` → TVHomePage.
- `/search|/browse|/genre*|/trending` → TVSearchPage. `/tv/shows` and `/tv/movies` filter by format.
- `/tv/iptv` → TVIPTVPage: M3U and Xtream Codes playlists, EPG guide, categories, series episodes (`src/core/iptv/*`).
- `/tv/library` (also `/auth`, `/onboarding`, `/setup`) → TVLibraryPage: My Tatakai, link account, sign out.
- `/anime/:id` → TVAnimePage. `/extensions[/:id]` → TVExtensionsPage. `/downloads` → TVDownloadsPage. `/settings` → TVSettingsPage. Manga and novel routes → TVBooksPage.
- Also: TVCommunityPage, TVSavedPage, TVWatchView.
- TVPairingModal shows a code that expires in five minutes, opened at tatakai.me/link (hosted).
- TV extras: TV appearance, themes and wallpapers, cinematic hero, playback overlay, launcher sync, secure debrid storage (`src/core/tv/*`, `src/components/tv/*`).

## 2. Features by area

### Home (`pages/base/Index.tsx`)
- Hero/spotlight carousel: `components/anime/HeroSection.tsx` and `CinematicHero.tsx` (mobile "refreshed" appearance uses a backdrop).
- Continue Watching from the cloud (`ContinueWatching.tsx`) and from local storage (`LocalContinueWatching.tsx`).
- ExtensionContinue (watch).
- TorrentSessionBanner (`components/home/`).
- Last read manga and novel (`LastReadMangaSection`, `LastReadNovelSection`).
- Followed updates: FollowedAnimeEpisodes, FollowedMangaUpdates, FollowedNovelUpdates.
- LatestEpisodes, TrendingGrid.
- Deferred sections:
  - BecauseYouWatched
  - AIRecommendationBanner
  - TopAnimeSection (today/week/month)
  - UpcomingAnimeSection
  - Most Popular grid
  - Most Favorite grid
  - IndexMangaShowcase
  - GenreCloud
  - CommunitySection (posts, tier lists, playlists, live watch rooms)
  - DiscordSection
- Infinite genre rows: `InfiniteHomeSections` / `MobileInfiniteHomeSections`, data from `hooks/api/useInfiniteHomeSections.ts`.
- Extension slots `home-top` and `home-bottom` (`core/extensions/ExtensionSlot.tsx`).
- Discord RPC "browsing" state. Error and refetch state.
- Data comes from `contentGraph.getHomePage` (metadata router, local).

### Search and discovery (`pages/base/SearchPage.tsx`)
- One search across anime, manga, novels, manhwa and characters, including alternate titles. Recent searches.
- Anime filters: genre, AniList tags (spoiler-tag toggle), format (TV/Movie/OVA/ONA/Special), status, season, year range, episode range, rating range, sort, 18+ toggle.
- Manga filters: origin (JP/KR/CN), MangaBaka tags and tag catalog, status, sort.
- Producer/studio search.
- Image search via trace.moe: scene preview, confidence slider, episode and timestamp match.
- GenrePage (`pages/base/GenrePage.tsx`) uses `components/anime/discover/*` (DiscoverFilterBar, poster grid, ranked rails).
- Manga/Novel GenreBrowse adds filters for length (chapter buckets), origin, rating, year, window (today/week/month), sort (popular, latest, new chapters, random, for-you, recently read), partial-results notice, and a mature gate.

### Anime details (`pages/watch/AnimePage.tsx`, `components/anime/AnimeDetailLayout.tsx`)
- Hero with optional trailer video background (`VideoBackground`).
- Facts: aired, status, duration, studios, producers, source, country, mean score, popularity, favourites, tags, also-known-as.
- Episode list: search by number or title, thumbnail or compact view, filler and upcoming badges, "not yet aired", official streams.
- Seasons and series tree / watch order (`AnimeSeasons`).
- Related anime and manga. Recommendations (AniList plus local engine). Characters and staff with voice actors.
- Next-episode countdown (`NextEpisodeSchedule`).
- Actions: WatchlistButton (statuses), AddToPlaylist, Share, TrackShowButton (calendar pin), Start watch-together, Open in app, Download season (`SeasonDownloadModal`: pick episodes, language/server, auto-download for new episodes; desktop and mobile).
- Ratings and reviews (`RatingsSection`), episode/anime comments (`EpisodeComments`), community posts about the anime (`AnimeCommunityPosts`).
- View count (`TatakaiAnimeViews`). Adult warning gate. Staff ContentEditSheet. Extension slots. Discord RPC "viewing". Mobile quick actions.

### Watch page (`pages/watch/WatchPage.tsx`)
- Players chosen by platform: VideoPlayer (desktop/web), MobileVideoPlayer, NativeTorrentPlayer, EmbedPlayer (iframe, ad-blocked on desktop), TVWatchView, and an Android native PlayerActivity (`android/.../player/NativePlayerPlugin.java`).
- Source panel (`components/video/SourcePanel.tsx`):
  - Grouped by language (SourceLanguageTabs: sub, dub, multi).
  - Type badges: Embed, HLS, Torrent.
  - Friendly server names. Fetch more servers / refresh, verified badge.
  - Torrent sources (`TorrentSourcePanel`) show seeders, leechers and debrid cached/uncached status.
  - TorrentSessionPanel (peers and stats). TorrentSwitchDialog.
- ExtensionRequired notice when no source extension is installed. SourceDebug (developer mode).
- Episode navigation: prev/next, jump to episode, unaired guards.
- Report issue (ReportModal). External-link warning dialog. View tracking. Watch history and progress sync (Supabase plus local continue-watching).
- Debrid orchestration (`core/providers/debrid-orchestrator.ts`). Torrent preload of the next episode (`core/player/TorrentPreloadController.ts`). Review popup.

### Player (`components/video/*`, `core/player/*`)
- **Controls:** PlaybackControls, Seekbar with preview, VolumeControl, SpeedSelector, TimeDisplay, FullscreenButton, PiPButton, ScreenshotButton, DownloadButton, ExternalPlayerButton, SkipButton, AudioTrackSelector, SubtitleSelector, PlayerSeekInput.
- **Skip intro/outro/recap:** AniSkip (`hooks/media/useAniskip.ts`) merged with extension segments (`core/player/skip-segments.ts`). Auto-skip-intro toggle.
- **Up next:** UpNextOverlay countdown with configurable seconds, autoplay, auto-next.
- **Upscaling:** Anime4K WebGL shaders with modes off/light/standard/high (`lib/video/anime4k/Anime4KRenderer.ts`, mpv shader parser).
- **Enhancements** (`hooks/usePlayerEnhancements.ts`): theater mode, ambient mode, stable volume, sleep timer (including end of episode), loop, default quality, smart quality and data saver on mobile.
- **Subtitles:**
  - Internal tracks, upload of SRT/ASS/SSA/VTT, custom fonts (TTF/OTF/WOFF).
  - Style: size, font, color, background opacity, outline, vertical position. Sync offset.
  - Secondary subtitle overlay.
  - Live and pre-translation (`useSubtitleTranslation`, `useSubtitlePretranslation`).
- **Audio:** track switching, including MKV extraction via desktop ffmpeg (`ipc-media.cjs`). Media probe.
- **Keyboard shortcuts** (`lib/video/keybindings.ts`): rebindable, with a shortcut overlay. Defaults: Space play, ←/→ seek, F fullscreen, M mute, P PiP, J/K previous/next episode.
- **Mobile gestures:** DoubleTapSeek, LongPressSpeed (2x), SwipeGestures. Also orientation lock, keep-awake, haptics.
- **Adapters** (`core/player/*Adapter.ts`): HLS, DASH, Direct, Torrent, Debrid, Offline. PlaybackCoordinator/EventBus.
- **External player** handoff to mpv, VLC, MPC-HC/BE, with auto-launch (`components/settings/ExternalPlayerSettings.tsx`, desktop `services/external-player.cjs`).
- **Discord RPC** watching and paused states (`lib/discordRpc.ts`).
- **Other:** JWPlayer optional, view-count badge.

### Watch together (`components/watch/room/*`, `hooks/media/useWatchRoom.ts`, `useP2PWatchRoom.ts`, `core/watch-party/*`)
- Room state, chat, polls, queue, participants and invites all live in Supabase (hosted).
- Top bar: share/copy link, export chat, settings, poll, leave, admin force-delete.
- Panels: ChatPanel, PollsPanel, QueuePanel, ViewersPanel (transfer host). Dialogs: PasswordDialog, PollComposer, LibraryPickerDialog (stream from the offline library).
- Ready check and countdown/start timer. Host controls. Sync is disabled for embed sources.
- Room settings: provider selection, custom m3u8/mp4 URL, manual subtitle URL, local file or offline-library streaming.
- P2P WebRTC media relay with drift correction (`core/watch-party/sync.ts`).
- Desktop share tunnel via cloudflared (`desktop/runtime/share-tunnel/*`).
- Watch-together entry points also appear in playlists, tier lists and community widgets.

### Library, lists and history
- Watchlist statuses: plan, watching, completed, on hold, dropped (`hooks/user/useWatchlist.ts`).
- Watch history (`useWatchHistory.ts`). Continue watching from local and cloud.
- Manga and novel readlists (`useMangaReadlist`, `useNovelReadlist`).
- Favorites page. Character favorites. Tracked shows. Ratings.
- Playlists (`pages/profile/PlaylistPage.tsx`, `components/playlist/*`): create/edit, drag reorder, public/private, discover community playlists (trending, newest, most liked), collaborators (owner/editor/viewer), share slug, embed code, discussion comments, launch a watch party.
- Tier lists (`TierListPage.tsx`, `components/tierlist/TierListEditor.tsx`): anime, manga and character entries, public/private, likes, views, collaborators, share code, comments, community browse.
- Saved content (settings): bookmarks, tier lists, playlists, uploaded media with a 25 MB quota.

### Downloads, offline and torrents
- **Desktop offline library** (`pages/profile/OfflineLibraryPage.tsx`):
  - Tabs for anime, manga/comics, light novels, active downloads, history.
  - Search, ETA and progress, cancel/retry.
  - Resume interrupted downloads. Import video files (`ImportAnimeModal`). Repair anime. Open folder. Sync library.
  - Default server and preferred audio language.
  - AutomationProfiles: auto-download rules (best/balanced 1080p/data saver 720p), simulate, check now, preload next episode.
- **Mobile offline** (`pages/error/MobileOfflinePage.tsx`, `components/mobile/MobileDownloadsUI.tsx`): per-episode and chapter delete, re-download, free watched episodes (auto-evict), broken-file detection, HLS or torrent, local notifications. Android native downloads use `TatakaiDownloadsPlugin` / `TatakaiDownloadService`.
- **Download engines:** anime (`core/download/download-monitor.ts`, desktop `ipc-download-manager.cjs` with HLS checkpoints and recovery), manga (`manga-download-monitor.ts`, `MangaDownloadModal`), novel (`novel-download-monitor.ts`, `novelOffline.ts` in Dexie, works on web too).
- **Auto-downloader:** `AutomationScheduler.tsx`, `automation-engine.ts`, candidate scorer, desktop `services/auto-downloader.cjs` (polls every 30 minutes).
- **Torrents:**
  - Desktop WebTorrent engine (`desktop/runtime/torrent/*`): session manager, piece prioritizer, next-episode preloader, HLS remux, release-name parser, batch detector, episode matcher. Android `TatakaiTorrentPlugin`.
  - Magnet download modal (`TorrentDownloadModal`). Magnet/torrent-file import and alignment (`MagnetAlignmentModal`, confidence mapping).
  - Torrent settings (`components/settings/TorrentSettings.tsx`): storage path, max connections, UPnP, background behavior, DL/UL limits, bandwidth schedule (standard/night owl/gaming), custom trackers, max cache GB, free space automatically, preload next episode.
  - Session history. Country torrent-legality policy (informational; `core/country-policy/*`, Supabase country_torrent_policies).
  - Ctrl+Shift+T restarts the torrent service.
- **Debrid** (`components/settings/DebridSettingsPanel.tsx`, `core/providers/*`): Real-Debrid, TorBox, AllDebrid, Premiumize.

### Extensions (`src/core/extensions/*`, `components/extensions/*`)
- Ecosystems: tatakai (`.kai` bundle), stremio, seanime, mihon, aniyomi, cloudstream, sora, mangayomi, kodi (external only) (`ecosystems.ts`).
- Install from the store, sideload a `.kai` file or a manifest/package URL, or configure a Stremio addon (`SideloadExtensionModal.tsx`).
- Desktop sandbox and worker pool (`desktop/runtime/extension/*`). Kill switch and audit log. Renderer bundles that need trust (`rendererLoader.ts`).
- Remote (Stremio/Seanime) extensions panel: enable/disable, preferences, update, remove, reset title matches (`RemoteExtensionsPanel.tsx`).
- Compatibility (Aniyomi/Mihon/CloudStream repos), bulk install, preferences: desktop Java runtime (`desktop/runtime/compatibility/*`) and Android `compatibility/*Engine.java` (`CompatibilityExtensionsPanel`, `CompatibilitySettings`, `InstalledCompatibilityExtensions`).
- Sora and Mangayomi QuickJS VMs (`core/extensions/sora`, `mangayomi`).
- Update sheet on launch (`ExtensionUpdateSheet`). Extension debug window. ExtensionSlot contributions: themes, analytics, custom sources, pages.
- Extension host port setting (default 8099) (`ExtensionHostPortSetting.tsx`).
- Extension servers, client side (`ExtensionServersSettings.tsx`, `core/extensions/server/*`): scan QR, paste link, manual URL+password, enable/disable/remove/test, status, offline fallback to local extensions.
- The store catalogue depends on the hosted extension API.

### Settings modal (`components/settings/settingsCategories.tsx`)
Every category has keyword search and deep-link sections.

**User group**
- **Account** (`panels/AccountPanel.tsx`): username, display name, avatar and banner, email change (two-step OTP), password change or reset email, sessions, log out, log out of all devices.
- **Profile Store** (`panels/ProfileStorePanel.tsx`): profile themes (gothic-empress, komorebi, midnight-lofi, sekaipop, uchiha in `components/profile/themes/*`), canvas editor, widgets, wallpapers, notes, presets, cosmetics. Access is by ownership or Booster/Pro/Ultra/Admin tiers.
- **Saved content** (`panels/SavedContentPanel.tsx`).
- **Privacy** (`panels/PrivacyPanel.tsx`): mature content (show everywhere, blur in search, warn before), public/private profile, visibility of info, watchlist, history and tier lists, clear watch history, clear search history.
- **Integrations** (`panels/IntegrationsPanel.tsx`): connect/disconnect AniList, MAL and MangaBaka. Sync preview with import/export/conflict choices (`components/integrations/SyncPreviewModal.tsx`, `lib/listSync.ts`). Auto-sync of episode and chapter progress (`hooks/user/useIntegrationSync.ts`).

**App group**
- **Appearance** (`panels/AppearancePanel.tsx`):
  - Media card style overlay vs poster.
  - Mobile appearance classic vs refreshed.
  - About 27 app themes (`hooks/ui/useTheme.ts`): midnight, cherry-blossom, neon-tokyo, aurora-borealis, deep-ocean, cyberpunk, zen-garden, light-minimal, light-sakura, brutalism-dark/plus, obsidian, solar, caffeine, dark-matter, royal-gold, emerald-forest, blood-moon, frostbite, synthwave, vampire, matcha-light, ocean-breeze, brutalist-concrete, ultra-lite, lite, amoled.
  - Extension-provided themes.
- **Display** (`panels/DisplayPanel.tsx`): app language (en/es/ja + auto), preferred title language (account-synced), manga title language, reduce motion and high contrast. Admin-only feature toggles: content graph, virtual grid, blurhash, runtime.
- **Translation** (`panels/TranslationPanel.tsx`):
  - Engine: auto, Google, DeepL, DeepLX, Azure, OpenAI-compatible, or on-device. URL, key, model and region fields, plus a test button.
  - Target language, cache clear.
  - Toggles for posts (auto), comments, device translation, subtitles (with pretranslate) and novels.
  - TranslationStatus shows on-device engine lite/full download state.
- **Video Player:** the embedded `VideoSettingsPanel.tsx`, covering every player option above plus keybind editing.
- **Reader** (`panels/ReaderPanel.tsx`):
  - Layout: direction LTR/RTL, paged/vertical/webtoon continuous, double page, fit width/height/both/natural, max page width, gap, background color, brightness, panel crop.
  - Loading: eager or lazy, preload count, infinite chapter scroll.
  - Behavior: click to turn pages, auto-scroll with speed, Ctrl+wheel zoom, fullscreen on open, hide chrome, keep awake, progress indicator, capture button, comments, rebindable keybinds.
- **Novel Reader** (`panels/NovelReaderPanel.tsx`):
  - Typography: fonts (default sans, Georgia, Lora, dyslexic, custom upload or installed), size, line and paragraph spacing, text width, background and text colors.
  - Flow: continuous scroll, progress bar, percent read, chapter number, fullscreen, keep awake, paragraph comments.
  - TTS narrator: system voice, KittenTTS (experimental local), OpenAI, ElevenLabs, local OpenAI-compatible server. Rate, voice gender preference, preview. Files: `components/novel/TtsIntegrationSettings.tsx`, `ExperimentalKittenSettings.tsx`, `lib/novel/*`.
- **App Settings**, native/desktop only (`panels/AppSettingsPanel.tsx` → `DesktopSettings.tsx` etc.):
  - Launch at startup, download location, external player, debrid, torrent engine.
  - Home Server (`HomeServerSettings.tsx`).
  - FlareSolverr, external or embedded (`FlareSolverrSettings.tsx`).
  - Country policy with IP reveal (`CountryPolicyPanel.tsx`).
  - Discord Rich Presence, app version and updates, system info (CPU, RAM).
  - Developer mode, export logs, runtime diagnostics, extension sideload audit, WARP routing log, reset app (danger zone).
  - Display/window: fullscreen F11, hide title bar.
  - Storage: quota 5–200 GB or unlimited, torrent cache directory, clear app/media cache, disk breakdown.
  - Resume interrupted downloads.
  - Performance: memory profile low/balanced/unlimited, free memory now, memory metrics graph.
  - Metadata sources: order and enable AniList, Tenrai, Kitsu, Shikimori, MAL, ani.zip, with status and cool-down (`MetadataSourcesPanel.tsx`).
  - Proxy servers: local app proxy vs external streaming proxies, primary/active selection, test (`ProxySettingsPanel.tsx`).
  - WARP tunnel (`WarpSettingsPanel.tsx`). Extension host port. Torrent session history. Desktop log viewer. Changelog.
- **Mobile App Settings**, Capacitor only (`panels/MobileAppSettingsPanel.tsx`): haptics, keep awake, status-bar icon color, orientation and landscape-in-player, keyboard resize mode, autoplay, auto-download next, auto-evict watched, data saver, quality cap, debrid, country policy, FlareSolverr URL, download notifications (progress and complete), clear cache, developer mode, device info, app version, update check (Android in-app install via `TatakaiUpdaterPlugin`; iOS opens the IPA).
- **Extensions** (`panels/ExtensionsPanel.tsx`): installed list (loaded or not), open Extension Hub, ExtensionServersSettings, RemoteExtensionsPanel.

**Info group**
- **About** (`panels/AboutPanel.tsx`): version, legal links, send feedback (to `/suggestions`), DiagnosticsSettings (export diagnostics in `core/diagnostics/*`).
- **Changelog** (`panels/ChangelogPanel.tsx`), from the Supabase `changelog` table and GitHub releases.

### Account, auth and sync
- Supabase Auth with email/password and OTP: `src/contexts/AuthContext.tsx`, `hooks/auth/useActiveSession.ts`.
- Desktop OAuth via the `tatakai://` deep link (`lib/desktopOAuth.ts`).
- AniList/MAL/MangaBaka tokens (`lib/integrationTokens.ts`, Supabase `user_integration_tokens`).
- Progress sync across devices through Supabase realtime (`AppRoutes.tsx` GlobalListeners).
- Bans: user, IP and device bans lead to `/banned`. Maintenance mode leads to `/maintenance`. Roles: admin, moderator.

### Profiles and social (all Supabase-backed)
- **Profile page** (`pages/profile/ProfilePage.tsx`) tabs: Overview, Watchlist, History, Manga readlist, Novel readlist, Vault, Streaks.
- **Overview widgets** (`components/profile/overview/*`): hero stats, activity heatmap, genre, taste, rating distribution, library distribution, manga analytics, watching habits, growth trends, interactive charts, smart insights, achievements, favorite characters, reviews, social connections, community analytics, external (AniList/MAL) stats, calendar saves and integration.
- **Vault tab:** your posts, comments, playlists and tier lists.
- **Customization:** banner and cover change, avatar picker (gallery, AniList character or anime art, GIF reactions, upload, URL; `AvatarPickerSheet.tsx`), avatar decorations and cosmetic effects (`ProfileCosmeticsPicker`, `IdentityCosmetics`), profile themes and editors.
- **Profile settings sheet:** bio, username, public toggle, show history, watchlist and calendar. Social links editor: AniList, MAL, Discord, X, Instagram, YouTube, Twitch.
- **Follow, followers and following.** Profile preview hover card (reputation, watch time). Badges and ranks (`lib/badges.ts`, `lib/rankUtils.ts`). Watch streaks. Wrapped stats. Leaderboards (most watched, comments, followers, rated, streak, active).
- **Comments system** (`components/comments/*`, `components/video/EpisodeComments.tsx`):
  - Scope: per episode, anime, series, playlist and tier list. Global or episode filter.
  - Rich composer with emoji, GIF picker (Giphy, saved GIFs), image attachments, embeds, polls, quotes, spoilers.
  - Replies, likes, pin, staff delete with tombstone, paused comments, translated content.
  - Manga chapter comments. Novel paragraph comments (`components/novel/ParagraphComments.tsx`) and selection comments.
- **Community feed** (`pages/community/CommunityFeedPage.tsx`, `components/community/feed/*`):
  - Tabs: For You, Following, News, Schedule. Filters: all, tier lists, playlists, watchrooms.
  - Composer: title, text, images, GIF, polls with image choices, share anime/manga, attach playlist or tier list, watch-together lobby, mention (@), hashtag (#), spoiler, target community, drafts, approval queue for image posts.
  - PostCard: like, comment, repost, quote, bookmark, share, edit, delete, pin, translated content.
  - Widgets: communities (create, join), events (create/delete), hashtags, trending rail, airing schedule, anime news strip, leaderboard, online presence, suggested users, members showcase, watch-together, my playlists, tier lists.
  - Moderation: ModerationMenu, BanUserDialog, community rules gate, general-language warning.
- **Suggestions** (`pages/base/SuggestionsPage.tsx`): feature, bug, content, improvement, other. Image attachment, captcha, admin responses.
- **Reports:** ReportModal with Turnstile.

### Notifications
- Supabase `notifications` (mentions, replies, announcements, system).
- Airing reminders for calendar-pinned shows (NotificationSheet "next episode" and "upcoming").
- Capacitor LocalNotifications for downloads (`hooks/community/useNotifications.ts`, `core/download/mobile/mobileDownloadService.ts`). Notification sound toggle.
- Push token tables exist, plus a Supabase function `supabase/functions/send-notification`.

### Manga (`pages/manga/*`, `components/manga/*`, `core/content/manga-*`)
- **Home:** hero, trending, picks, smart discover, discovery lanes and quick filters, genre slider, recommendation grid, followed updates, last read, extra lanes, gated 18+ zone.
- **Details:** synopsis, metadata (AniList, MAL, Kitsu, Shikimori, MangaUpdates, ANN), cross-site ratings, tags, publishers, serialization, series tree, characters, recommendations, anime coverage.
- **Chapters:** per-provider rows, preferred language and source, auto best source per chapter, sort, search, volume grouping, refresh and clear cache, download all.
- **Readlist statuses.**
- **Reader** (`MangaReaderPage.tsx`, `components/reader/*`): ReaderChrome, page rail, provider switch per chapter, prefetch next/previous chapter, chapter navigator, comments, fullscreen, capture page, offline reading (`?offline=true`), and "Translate this downloaded chapter with Yomi" (`components/translate/TranslateChapterButton.tsx`).

### Novels (`pages/novel/*`, `components/novel/*`)
- Home, browse, details: translator filter, check all sources, download chapters, library statuses.
- Reader: settings sheet, narration (read aloud with pause/resume/stop), paragraph comments, offline mode, chapter discussion. Also NovelDiagnostics and NovelDiscussCard.

### Recommendations
- Client-side taste profile and scoring (`src/core/recommendations/*`, `hooks/api/useRecommendationEngine.ts`, `useMLRecommendations.ts`).
- Feedback stored in Supabase `recommendation_feedback`.
- Atlas embedding map (`components/atlas/*`).
- MangaBaka enrichment.

### Admin (`pages/admin/AdminPage.tsx`, `components/admin/*`, sections in `adminSections.ts`)
- **Insight:** analytics, user stats, streaming analytics, performance insights, world map.
- **Queues:** reports, suggestions, pending forum posts.
- **Moderation:** comments, content moderation, ban management (IP and device bans, templates), ban audit, moderation logs, admin logs.
- **Community:** users, watch rooms, achievements, badges, notifications/broadcasts, news.
- **Catalogue:** catalog entries (content overrides), popups (PopupBuilder).
- **System:** settings (maintenance, community approval), incidents, changelog, API admin, app updates (AppReleaseManager, AppVersionManager, UpdateManagementPanel, GitHubReleasesPanel), crash reports, desktop logs, redirects manager.
- Per-user admin page with tabs (`AdminUserPage.tsx`). All of this is Supabase-backed.

### i18n and themes
- i18next via `src/lib/i18n.ts` and `src/lib/localeBackend.ts`. Locales: `src/locales/en.json`, `es.json`, `ja.json` (7,244 lines each).
- Top-level keys to cross-check against: extensionWorkspace, landingHero, extensionUpdate, common, nav, auth, anime, player, header, settings, display, privacy, watchlist, sidebar, wrapped, leaderboard, download, social, manga, reader, ui, animeDetail, comments, errors, playlist, watch, home, tierlist, moderation, discover, layout, torrent, migration, community, translation, translationStatus, communityControls, badgeReward, releaseSheet, insights, developerConsole, extensions, extensionRequired, metadataSources, extensionServerHost, extensionServerClient.
- `app.*` is a flat bucket of about 5,646 hashed keys. I mined it per file to build this list.

### Discord
- Desktop Rich Presence (`desktop/services/discord-rpc.cjs`, `src/lib/discordRpc.ts`) with browsing, viewing, watching and paused states, plus settings.
- Discord Embedded Activity SDK support (`src/lib/discordActivity.ts`).
- `/discord` bot page. Discord home section.

### Misc
- Turnstile captcha (`components/security/TurnstileWidget.tsx`). SEO component (`components/seo/Seo.tsx`). Analytics and engagement tracking (`core/analytics`, `hooks/api/useAnalytics.ts`). Sentry (`lib/sentry.ts`).
- Mobile: pull-to-refresh, back-button handler, deep-link bridge, app review prompt, data saver.
- Android ad and click-hijack blocking: `AntiHijackWebViewClient`, `AdHosts`, `CloudflareBypass`.
- Android local media proxy and cache (`TatakaiLocalProxyPlugin`). Mux plugin.

## 3. Desktop-only (Electron, `desktop/`)

- **Main process** `desktop/main.cjs`: splash window (`window/splash-window.cjs`, `splash.html`, themed via `services/theme-bridge.cjs`), `offline.html`, window factory with state.
  - `app://` secure scheme for the packaged renderer, needed by Turnstile (`window/app-scheme.cjs`).
  - CORS bridge for HLS hosts (`window/cors-bridge.cjs`).
  - Ad/popunder blocker that surfaces toasts in the renderer (`security/ad-blocker.cjs`). IPC guard (`security/ipc-guard.cjs`).
- **Tray** (`window/tray.cjs`): Show App, Quit, click to restore.
- **Shortcuts** (`window/shortcuts.cjs`, window-scoped): F11 fullscreen, F12 DevTools, Ctrl+Shift+I log viewer, Ctrl+R reload, Ctrl+Shift+R hard reload, Ctrl+Shift+T restart torrent.
- **Deep links** (`window/deep-link.cjs`): `tatakai://` routes, `magnet:` handling, `tatakai://extensionserver/connect`, OAuth token returns.
- **Preload bridges** (`preload.cjs`): `window.electron` and `window.tatakaiRuntime`.
- **IPC services** (`desktop/ipc/*`):
  - **system:** window controls, fullscreen, platform and system info, client ID, prefs, export logs, auto-launch, reset app data, relaunch, storage info and clear by category, memory profile and reclaim memory.
  - **library:** pick a folder, scan the local library, offline library sync, import videos (analyze and finalize), parse release names, repair/delete anime, stream a local file.
  - **download-manager:** enqueue, pause, resume, cancel, list, recovery, resume interrupted, profiles, simulate.
  - **manga-download-manager** and **manga-library:** offline manga, read pages, delete chapter or series.
  - **media:** ffprobe probe, extract subtitle and audio tracks, read a local file.
  - **torrent:** start, preview, stop, stream, stats, peers, preload next and preload source, sessions, magnet/torrent-file import, resolve metadata, storage path, reannounce, restart service, update settings, diagnostics.
  - **debrid:** request proxy.
  - **compatibility:** Aniyomi, Mihon, CloudStream, Skystream and Suwayomi brokers on a Java runtime (`runtime/compatibility/*`).
  - **runtime:**
    - Extension load, unload, invoke, sideload, install manifest, enable, remove, kill switch, audit log, `.kai` metadata, source code, renderer bundle.
    - Extension API host port (`runtime/extension-api-host/*`).
    - Local proxy and preview proxy (`runtime/proxy/local-proxy-server.cjs`).
    - Share tunnel start/stop/publish via cloudflared (`runtime/share-tunnel/*`).
    - WARP status, toggle, mode and routing log (`runtime/warp/warp-tunnel.cjs`).
    - Cloudflare bypass and FlareSolverr, external or embedded (`runtime/proxy/*`).
  - **home-server:** status, config, start/stop, pairing, device tokens, household users, consent approve/deny, open dashboard (`ipc-home-server.cjs`, `runtime/home-server/*`).
  - **translate:** Yomi engine request, export (including PSD), fonts, backup/restore, chapter, offline model setup/status/start/stop with a llama runtime and model download (`runtime/translate/*`).
  - **theme:** persist the theme for splash and native surfaces.
- **Updater** (`services/update-manager.cjs`): electron-updater check, download, install, rollback and history, plus a Supabase `update_policies` channel policy. The renderer side is `core/update/*` and the DynamicIsland update states.
- **Other services:**
  - Discord RPC (`services/discord-rpc.cjs`)
  - KittenTTS local neural TTS through a Python daemon (`services/kitten-tts.cjs`, `kitten-tts-daemon.py`), with model download, delete and synthesize
  - External player detect and launch (`services/external-player.cjs`)
  - Crash reporter and breadcrumbs (`crash-service.cjs`)
  - Sentry error tracker (`error-tracker.cjs`)
  - Perf monitor for memory/CPU history (`perf-monitor.cjs`)
  - Log service tail (`log-service.cjs`)
  - Persistent device CID (`cid.cjs`)
  - Auto-downloader (`auto-downloader.cjs`, `auto-download-sources.cjs`)
  - ffmpeg paths, branding init, platform adapter
- **Local server hosting:**
  - **Legacy Home Server** (`desktop/runtime/home-server/server.cjs` + `ui/index.html`): a remote web UI (Browse, Downloads, Extensions, player) and `/api/home/*` (auth, catalog, search, library, downloads, extensions invoke, playback resolve, torrents search/start, consent, users).
  - **Integrated Extension Server** (`server/`, embedded through `server/src/desktop.mjs` on the Home Server port; also runs standalone with `server/src/node.mjs` or Docker):
    - API `/api/server/v1/*`: health, setup, auth (login, logout, pair, pairing, password), devices, extensions, repositories, sources, resolve, invoke, media proxy and refresh, cache, settings, logs, diagnostics, import/export, native.
    - Dashboard SPA at `/server/` (`server/ui/main.tsx`) with tabs Sources, Playground, Devices, Settings, Logs.
    - Interpreted extensions via QuickJS (Sora, Mangayomi, Seanime, Tatakai scripts). Optional Java broker and Playwright browser runtime.
    - Phone pairing: one-time five-minute code, QR, deep link. Revocable device tokens. Optional viewer password. The renderer settings are `HomeServerSettings.tsx` (host side) and `ExtensionServersSettings.tsx` / `ExtensionServerLinkPage.tsx` (client side).

## Gaps and caveats

- Docs list AllDebrid and Premiumize. The settings panel and the cloud-debrid client both reference them, so I treat them as present.
- The feature-flag list includes `voice_search`, but I found no SpeechRecognition use, so it probably isn't implemented.
- Even with `VITE_BACKEND_ORIGIN` set, routes that call api.tatakai.me appear to be legacy-only.
- `/community/forum/new` redirects away, so `ForumNewPostPage` is unreachable.
- Docs: `docs/reference/features.md`, `docs/features/*.md` and `docs/redesign/*` are useful references for the diff.
