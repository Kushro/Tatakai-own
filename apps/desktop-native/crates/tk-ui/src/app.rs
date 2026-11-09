//! Root component: services, global UI context, theme/lang application, JS bridge, router and shell.

use std::path::PathBuf;
use std::sync::Arc;

use dioxus::prelude::*;
use tk_player::PlayerHost;

use crate::backend::{BackendRef, RemoteCommandVm};
use crate::components::{NavRail, RailFootItem, Snackbar, SnackbarHost, TitleBar};
use crate::i18n::{self, resolve, t};
use crate::model::{NavAction, NavDest, NavState, Settings, TorrentSection, nav_expanded, reduce_nav};
use crate::platform;
use crate::screens::torrents::{
    debrid::TorrentDebridPage, downloads::TorrentDownloadsPage, files::TorrentFilesPage, layout::TorrentsLayout,
    library::TorrentLibraryPage, overview::TorrentOverviewPage, peers::TorrentPeersPage, settings::TorrentSettingsPage,
    trackers::TorrentTrackersPage,
};
use crate::screens::manga::{details::MangaDetailsPage, home::MangaHomePage, reader::MangaReaderPage};
use crate::screens::novel::{details::NovelDetailsPage, home::NovelHomePage, reader::NovelReaderPage};
use crate::screens::social::{
    activity::ActivityPage, notifications::NotificationsPage, profile::ProfilePage, profile::UserPage,
};
use crate::screens::{
    browse::BrowsePage, calendar::CalendarPage, character::CharacterPage, details::DetailsPage, downloads::DownloadsPage,
    extensions::ExtensionsPage, gallery::GalleryPage, home::HomePage, image_search::ImageSearchPage, library::LibraryPage,
    link::LinkPage, player::PlayerPage, provider::ProviderPage, recs::RecommendationsPage, search::SearchPage,
    server::ServerPage, settings::SettingsPage, studio::StudioPage,
};
use crate::shortcuts::{BRIDGE_JS, JsMsg, Shortcut, shortcut_in};
use crate::ui_thread::UiJobs;

const TOKENS_CSS: &str = include_str!("../assets/tokens.css");
/// Base + shell + components (wave 0), then one stylesheet per area so parallel work never edits the
/// same CSS file (owners in docs/redesign/07 section 5).
const APP_CSS: &[&str] = &[
    include_str!("../assets/app.css"),
    include_str!("../assets/css/media.css"),
    include_str!("../assets/css/sources.css"),
    include_str!("../assets/css/home.css"),
    include_str!("../assets/css/library.css"),
    include_str!("../assets/css/search.css"),
    include_str!("../assets/css/details.css"),
    include_str!("../assets/css/extensions.css"),
    include_str!("../assets/css/link.css"),
    include_str!("../assets/css/settings.css"),
    include_str!("../assets/css/player.css"),
    // wave 3 (docs/redesign/10): torrent client shell + per-torrent pages (C), library / downloads / settings (D)
    include_str!("../assets/css/torrents.css"),
    include_str!("../assets/css/torrents_manage.css"),
    // wave A (docs/redesign/15 section 2): one stylesheet per owner: a, d, f, g, i (e keeps player.css, c details.css)
    include_str!("../assets/css/character.css"),
    include_str!("../assets/css/subs.css"),
    include_str!("../assets/css/system.css"),
    include_str!("../assets/css/lists.css"),
    include_str!("../assets/css/debrid.css"),
    // wave B (docs/redesign/16 section 2): one stylesheet per owner: b (calendar; keeps home.css), h, j, k, l, m, n
    include_str!("../assets/css/calendar.css"),
    include_str!("../assets/css/downloads.css"),
    include_str!("../assets/css/provider.css"),
    include_str!("../assets/css/server.css"),
    include_str!("../assets/css/manga.css"),
    include_str!("../assets/css/novel.css"),
    include_str!("../assets/css/social.css"),
];

/// What tk-app injects (`LaunchBuilder::with_context`).
#[derive(Clone)]
pub struct Services {
    pub backend: BackendRef,
    pub player: Arc<PlayerHost>,
    /// Directory with the bundled `.woff2` files.
    pub fonts_dir: PathBuf,
    /// Jobs app services post to the UI thread (`ui_thread::channel`); drained by `App`.
    pub ui_jobs: Arc<UiJobs>,
    /// Wave A: run once by `App` on its first render, inside the Dioxus / tokio runtime (tk-app spawns its start-up
    /// jobs from here: update notice, `magnet:` link from the command line). `None` = nothing to do.
    pub on_start: Option<Arc<dyn Fn() + Send + Sync>>,
}

impl PartialEq for Services {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.backend, &other.backend) && Arc::ptr_eq(&self.player, &other.player)
    }
}

/// A forwarded shortcut; `seq` makes repeated identical shortcuts distinguishable for effects.
#[derive(Debug, Clone, PartialEq)]
pub struct ShortcutEvt {
    pub seq: u64,
    pub action: Shortcut,
}

/// Global UI state shared through context.
#[derive(Clone, Copy)]
pub struct Ui {
    pub settings: Signal<Settings>,
    /// True once `Backend::load_settings` answered (or failed): until then `settings` holds defaults and the shell
    /// must not act on them (first-run wizard redirect, worker f).
    pub settings_loaded: Signal<bool>,
    pub width: Signal<f64>,
    pub nav: Signal<NavState>,
    pub shortcut: Signal<Option<ShortcutEvt>>,
    /// Torrent the client's per-torrent pages show (`TorrentStatsVm::id`); set by the Library "open", the client's
    /// own picker and the player ("open in the torrent client"). Lives here so it survives leaving `/torrents`.
    pub torrent: Signal<Option<String>>,
}

impl Ui {
    /// Update settings and persist them in the background.
    pub fn commit_settings(mut self, backend: &BackendRef, next: Settings) {
        self.settings.set(next.clone());
        let backend = backend.clone();
        spawn(async move {
            if let Err(e) = backend.save_settings(&next).await {
                tracing::warn!(detail = %e.detail, "saving settings failed");
            }
        });
    }
}

#[derive(Routable, Clone, PartialEq, Debug)]
pub enum Route {
    #[layout(Shell)]
    #[route("/")]
    HomePage {},
    #[route("/search")]
    SearchPage {},
    // Wave A (docs/redesign/15 section 2), worker a: image search, "See all" of a feed, studio and character pages.
    #[route("/search/image")]
    ImageSearchPage {},
    #[route("/browse/:feed")]
    BrowsePage { feed: String },
    #[route("/studio/:name")]
    StudioPage { name: String },
    #[route("/character/:id")]
    CharacterPage { id: String },
    #[route("/library")]
    LibraryPage {},
    #[route("/details/:id")]
    DetailsPage { id: String },
    #[route("/player/:id/:episode")]
    PlayerPage { id: String, episode: u32 },
    #[route("/extensions")]
    ExtensionsPage {},
    #[route("/link")]
    LinkPage {},
    #[route("/settings")]
    SettingsPage {},
    // --- Wave B (docs/redesign/16-parity-wave-b-contract.md section 2). Placeholder screens until each owner lands. ---
    /// Airing calendar with "My shows" and reminders (worker b; under Home, plan 14 D6).
    #[route("/calendar")]
    CalendarPage {},
    /// "For you" recommendations with explanations + the Anime Atlas map (worker b; under Home).
    #[route("/recommendations")]
    RecommendationsPage {},
    /// Downloads: queue, history, local library (worker h; rail entry `NavDest::Downloads`).
    #[route("/downloads")]
    DownloadsPage {},
    /// Custom provider page of an extension (`custom/home` rows + search; worker j; under Extensions). Titles open
    /// `DetailsPage` with an `ext:<ext_id>:<key>` id (`Backend::provider_item`).
    #[route("/provider/:ext_id")]
    ProviderPage { ext_id: String },
    /// Native extension server (`tk-server`): status, pairing, devices, logs, proxy / FlareSolverr (worker k; under Link).
    #[route("/server")]
    ServerPage {},
    /// AniList social (worker n; under Library): the viewer's profile, a public profile, notifications, activity.
    #[route("/profile")]
    ProfilePage {},
    #[route("/user/:name")]
    UserPage { name: String },
    #[route("/notifications")]
    NotificationsPage {},
    #[route("/activity")]
    ActivityPage {},
    /// Manga (worker l; rail entry `NavDest::Manga`). The reader is bare (no rail), like the player.
    #[route("/manga")]
    MangaHomePage {},
    #[route("/manga/:id")]
    MangaDetailsPage { id: String },
    #[route("/manga/:id/read/:ext_id/:chapter_id")]
    MangaReaderPage { id: String, ext_id: String, chapter_id: String },
    /// Novels (worker m; rail entry `NavDest::Novels`). The reader is bare (no rail).
    #[route("/novels")]
    NovelHomePage {},
    #[route("/novels/:id")]
    NovelDetailsPage { id: String },
    #[route("/novels/:id/read/:ext_id/:chapter_id")]
    NovelReaderPage { id: String, ext_id: String, chapter_id: String },
    /// First-run wizard (wave A, worker f; plan 14 section 0.1 #11). The shell redirects here while
    /// `Settings::system.setup_done` is false; Settings > System can open it again.
    #[route("/setup")]
    SetupPage {},
    // Torrent client (wave 3, docs/redesign/10 section 5): side index layout + one page per `TorrentSection`.
    #[nest("/torrents")]
        #[layout(TorrentsLayout)]
            #[route("/")]
            TorrentOverviewPage {},
            #[route("/files")]
            TorrentFilesPage {},
            #[route("/peers")]
            TorrentPeersPage {},
            #[route("/trackers")]
            TorrentTrackersPage {},
            #[route("/library")]
            TorrentLibraryPage {},
            #[route("/downloads")]
            TorrentDownloadsPage {},
            // Wave A, worker i: torrent session history.
            #[route("/history")]
            TorrentHistoryPage {},
            // Wave A, worker i: debrid services as sources.
            #[route("/debrid")]
            TorrentDebridPage {},
            #[route("/settings")]
            TorrentSettingsPage {},
        #[end_layout]
    #[end_nest]
    /// Component gallery; renders nothing in release builds.
    #[route("/gallery")]
    GalleryPage {},
}

impl Route {
    pub fn nav_dest(&self) -> Option<NavDest> {
        match self {
            Self::HomePage {} | Self::DetailsPage { .. } | Self::CalendarPage {} | Self::RecommendationsPage {} => Some(NavDest::Home),
            Self::SearchPage {} | Self::ImageSearchPage {} | Self::BrowsePage { .. } | Self::StudioPage { .. } | Self::CharacterPage { .. } => {
                Some(NavDest::Search)
            }
            Self::LibraryPage {} | Self::ProfilePage {} | Self::UserPage { .. } | Self::NotificationsPage {} | Self::ActivityPage {} => {
                Some(NavDest::Library)
            }
            Self::DownloadsPage {} => Some(NavDest::Downloads),
            Self::MangaHomePage {} | Self::MangaDetailsPage { .. } | Self::MangaReaderPage { .. } => Some(NavDest::Manga),
            Self::NovelHomePage {} | Self::NovelDetailsPage { .. } | Self::NovelReaderPage { .. } => Some(NavDest::Novels),
            Self::ExtensionsPage {} | Self::ProviderPage { .. } => Some(NavDest::Extensions),
            Self::LinkPage {} | Self::ServerPage {} => Some(NavDest::Link),
            Self::SettingsPage {} => Some(NavDest::Settings),
            Self::PlayerPage { .. } | Self::SetupPage {} | Self::GalleryPage {} => None,
            _ => self.torrent_section().map(|_| NavDest::Torrents),
        }
    }

    /// The torrent client page this route shows, if any.
    pub fn torrent_section(&self) -> Option<TorrentSection> {
        match self {
            Self::TorrentOverviewPage {} => Some(TorrentSection::Overview),
            Self::TorrentFilesPage {} => Some(TorrentSection::Files),
            Self::TorrentPeersPage {} => Some(TorrentSection::Peers),
            Self::TorrentTrackersPage {} => Some(TorrentSection::Trackers),
            Self::TorrentLibraryPage {} => Some(TorrentSection::Library),
            Self::TorrentDownloadsPage {} => Some(TorrentSection::Downloads),
            Self::TorrentHistoryPage {} => Some(TorrentSection::History),
            Self::TorrentDebridPage {} => Some(TorrentSection::Debrid),
            Self::TorrentSettingsPage {} => Some(TorrentSection::Settings),
            _ => None,
        }
    }

    pub fn for_torrent_section(section: TorrentSection) -> Self {
        match section {
            TorrentSection::Overview => Self::TorrentOverviewPage {},
            TorrentSection::Files => Self::TorrentFilesPage {},
            TorrentSection::Peers => Self::TorrentPeersPage {},
            TorrentSection::Trackers => Self::TorrentTrackersPage {},
            TorrentSection::Library => Self::TorrentLibraryPage {},
            TorrentSection::Downloads => Self::TorrentDownloadsPage {},
            TorrentSection::History => Self::TorrentHistoryPage {},
            TorrentSection::Debrid => Self::TorrentDebridPage {},
            TorrentSection::Settings => Self::TorrentSettingsPage {},
        }
    }

    pub fn is_player(&self) -> bool {
        matches!(self, Self::PlayerPage { .. })
    }

    /// Routes drawn without the navigation rail (player, first-run wizard, manga / novel readers).
    pub fn is_bare(&self) -> bool {
        matches!(self, Self::PlayerPage { .. } | Self::SetupPage {} | Self::MangaReaderPage { .. } | Self::NovelReaderPage { .. })
    }

    /// The manga / novel readers (wave B): the shell hides the rail and the readers draw their own chrome.
    pub fn is_reader(&self) -> bool {
        matches!(self, Self::MangaReaderPage { .. } | Self::NovelReaderPage { .. })
    }

    pub fn for_dest(dest: NavDest) -> Self {
        match dest {
            NavDest::Home => Self::HomePage {},
            NavDest::Search => Self::SearchPage {},
            NavDest::Library => Self::LibraryPage {},
            NavDest::Downloads => Self::DownloadsPage {},
            NavDest::Manga => Self::MangaHomePage {},
            NavDest::Novels => Self::NovelHomePage {},
            NavDest::Torrents => Self::TorrentOverviewPage {},
            NavDest::Extensions => Self::ExtensionsPage {},
            NavDest::Link => Self::LinkPage {},
            NavDest::Settings => Self::SettingsPage {},
        }
    }
}

#[component]
pub fn App() -> Element {
    let services = use_context::<Services>();
    let mut settings = use_signal(Settings::default);
    let mut settings_loaded = use_signal(|| false);
    let mut width = use_signal(|| 1280.0_f64);
    let nav = use_signal(NavState::default);
    let mut shortcut_sig = use_signal(|| None::<ShortcutEvt>);
    let torrent = use_signal(|| None::<String>);
    let mut sys_tag = use_signal(|| "en".to_owned());
    let mut sys_dark = use_signal(|| true);
    use_context_provider(|| Ui { settings, settings_loaded, width, nav, shortcut: shortcut_sig, torrent });
    use_context_provider(Snackbar::new);

    // Wave A: start-up jobs of the app (update notice, pending magnet link) and the tray icon (worker f, `tray.rs`).
    use_hook({
        let on_start = services.on_start.clone();
        move || {
            if let Some(f) = on_start {
                f();
            }
        }
    });
    crate::tray::use_tray();

    let fonts_css = use_hook({
        let dir = services.fonts_dir.clone();
        move || crate::fonts::font_face_css(&dir)
    });

    // Load persisted settings once.
    let backend = services.backend.clone();
    use_future(move || {
        let backend = backend.clone();
        async move {
            match backend.load_settings().await {
                Ok(s) => settings.set(s.sanitized()),
                Err(e) => tracing::warn!(detail = %e.detail, "loading settings failed; using defaults"),
            }
            settings_loaded.set(true);
        }
    });

    // Jobs from app services that need the UI thread (docs/redesign/09 section 6).
    let jobs = services.ui_jobs.clone();
    use_future(move || {
        let jobs = jobs.clone();
        async move {
            let Some(mut rx) = jobs.take() else { return };
            while let Some(job) = rx.recv().await {
                job();
            }
        }
    });

    // Web view bridge: system language / dark mode / window width and keyboard shortcuts.
    use_future(move || async move {
        let mut eval = document::eval(BRIDGE_JS);
        let mut seq = 0_u64;
        while let Ok(msg) = eval.recv::<JsMsg>().await {
            match msg {
                JsMsg::Env { tag, dark, w } => {
                    if *sys_tag.peek() != tag {
                        sys_tag.set(tag);
                    }
                    if *sys_dark.peek() != dark {
                        sys_dark.set(dark);
                    }
                    if (*width.peek() - w).abs() > 0.5 {
                        width.set(w);
                    }
                }
                JsMsg::Key { key, ctrl, editable, player, .. } => {
                    // Wave A (worker e): the user's rebinds (`Settings::flow.shortcuts`) apply here.
                    let action = shortcut_in(&settings.peek().flow.shortcuts, &key, ctrl, editable, player);
                    if let Some(action) = action {
                        seq += 1;
                        shortcut_sig.set(Some(ShortcutEvt { seq, action }));
                    }
                }
            }
        }
    });

    // Apply theme attributes and language whenever settings or the system change.
    use_effect(move || {
        let s = settings.read();
        let attrs = s.theme_attrs(sys_dark());
        platform::apply_theme(&attrs.palette, attrs.mode, attrs.contrast, s.reduce_motion);
        i18n::set_lang(resolve(s.language, &sys_tag.read()));
    });

    rsx! {
        style { dangerous_inner_html: "{TOKENS_CSS}" }
        style { dangerous_inner_html: "{fonts_css}" }
        for (i , css) in APP_CSS.iter().enumerate() {
            style { key: "{i}", dangerous_inner_html: "{css}" }
        }
        Router::<Route> {}
    }
}

#[component]
fn Shell() -> Element {
    let ui = use_context::<Ui>();
    let route = use_route::<Route>();
    let navigator = use_navigator();
    let mut nav_state = ui.nav;
    let mut shortcut_seen = use_signal(|| 0_u64);
    let shell_status = crate::components::title_bar::use_shell_status();
    // Wave A: Discord presence while browsing / viewing (worker e) and magnets handed over by a second launch (worker i).
    crate::screens::player::presence::use_route_presence(route.clone());
    crate::screens::torrents::library::use_pending_magnet_watch();
    // Wave B (worker k, `link.remote-control` / `link.handoff`): commands a paired phone sends over Tatakai Link.
    // `Open` navigates to the player; the transport commands become shortcuts the player already handles. The loop
    // ends on `NotWired` (default `Backend`) and backs off on any other error.
    {
        let services = use_context::<Services>();
        let mut shortcut_sig = ui.shortcut;
        use_future(move || {
            let backend = services.backend.clone();
            async move {
                let mut seq = 1_000_000_u64;
                loop {
                    match backend.wait_remote_command().await {
                        // Hand-off (`link.handoff`): the phone pushed its history row first (`POST /v1/history`), so
                        // the player resumes at `position_secs` through the normal `PlaybackVm::start_secs` path.
                        Ok(RemoteCommandVm::Open { media_id, episode, position_secs: _ }) => {
                            navigator.push(Route::PlayerPage { id: media_id, episode });
                        }
                        Ok(cmd) => {
                            if let Some(action) = crate::screens::player::remote::shortcut_for(&cmd) {
                                seq += 1;
                                shortcut_sig.set(Some(ShortcutEvt { seq, action }));
                            }
                        }
                        Err(e) if e.kind == crate::model::ErrorKind::NotWired => break,
                        Err(e) => {
                            tracing::debug!(detail = %e.detail, "remote command wait failed; retrying");
                            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                        }
                    }
                }
            }
        });
    }

    // Ctrl+B / Ctrl+K are global; everything else is handled by the screen that owns it.
    use_effect(move || {
        let Some(evt) = (ui.shortcut)() else { return };
        if *shortcut_seen.peek() == evt.seq {
            return;
        }
        shortcut_seen.set(evt.seq);
        match evt.action {
            Shortcut::ToggleNav => {
                let next = reduce_nav(*nav_state.peek(), NavAction::Toggle);
                nav_state.set(next);
            }
            Shortcut::FocusSearch => {
                navigator.push(Route::SearchPage {});
            }
            Shortcut::NavTo(n) => {
                if let Some(dest) = NavDest::ALL.get(usize::from(n).saturating_sub(1)) {
                    navigator.push(Route::for_dest(*dest));
                }
            }
            _ => {}
        }
    });

    // Wave A (worker f): first-run wizard. Once the saved settings are known, a fresh profile goes to `/setup`;
    // the wizard sets `Settings::system.setup_done` when finished or skipped. Never fires while settings are defaults.
    {
        let route = route.clone();
        use_effect(move || {
            let loaded = (ui.settings_loaded)();
            let done = ui.settings.read().system.setup_done;
            if loaded && !done && !matches!(route, Route::SetupPage {} | Route::GalleryPage {}) {
                navigator.replace(Route::SetupPage {});
            }
        });
    }

    let in_player = route.is_player();
    let bare = route.is_bare();
    let expanded = nav_expanded(nav_state(), (ui.width)());
    let toggle = move |_| {
        let next = reduce_nav(*nav_state.peek(), NavAction::Toggle);
        nav_state.set(next);
    };

    rsx! {
        div { class: "tk-shell", "data-player": "{in_player}",
            TitleBar { on_toggle_nav: toggle, offline: !(shell_status.online)(), link_port: shell_status.link_port() }
            div { class: "tk-shell__body",
                if !bare {
                    NavRail {
                        current: route.nav_dest(),
                        expanded,
                        // Wave B: rail badges from their owners (`use_rail_badges` collects them; each returns 0 until
                        // wired): n AniList unread (Library), j extension updates (Extensions), h active downloads.
                        badges: crate::components::rail_badges::use_rail_badges(),
                        on_navigate: move |d| {
                            navigator.push(Route::for_dest(d));
                        },
                        on_toggle: toggle,
                        on_search: move |_| {
                            navigator.push(Route::SearchPage {});
                        },
                        if cfg!(debug_assertions) {
                            RailFootItem {
                                icon: "grid".to_string(),
                                label: t("gallery.title"),
                                onclick: move |_| {
                                    navigator.push(Route::GalleryPage {});
                                },
                            }
                        }
                    }
                }
                main { class: "tk-main", Outlet::<Route> {} }
            }
            SnackbarHost {}
        }
    }
}
