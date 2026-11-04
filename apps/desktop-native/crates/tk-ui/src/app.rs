//! Root component: services, global UI context, theme/lang application, JS bridge, router and shell.

use std::path::PathBuf;
use std::sync::Arc;

use dioxus::prelude::*;
use tk_player::PlayerHost;

use crate::backend::BackendRef;
use crate::components::{NavRail, RailFootItem, Snackbar, SnackbarHost, TitleBar};
use crate::i18n::{self, resolve, t};
use crate::model::{NavAction, NavDest, NavState, Settings, nav_expanded, reduce_nav};
use crate::platform;
use crate::screens::{
    details::DetailsPage, extensions::ExtensionsPage, gallery::GalleryPage, home::HomePage, library::LibraryPage,
    link::LinkPage, player::PlayerPage, search::SearchPage, settings::SettingsPage,
};
use crate::shortcuts::{BRIDGE_JS, JsMsg, Shortcut, shortcut};
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
    pub width: Signal<f64>,
    pub nav: Signal<NavState>,
    pub shortcut: Signal<Option<ShortcutEvt>>,
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
    /// Component gallery; renders nothing in release builds.
    #[route("/gallery")]
    GalleryPage {},
}

impl Route {
    pub fn nav_dest(&self) -> Option<NavDest> {
        match self {
            Self::HomePage {} | Self::DetailsPage { .. } => Some(NavDest::Home),
            Self::SearchPage {} => Some(NavDest::Search),
            Self::LibraryPage {} => Some(NavDest::Library),
            Self::ExtensionsPage {} => Some(NavDest::Extensions),
            Self::LinkPage {} => Some(NavDest::Link),
            Self::SettingsPage {} => Some(NavDest::Settings),
            Self::PlayerPage { .. } | Self::GalleryPage {} => None,
        }
    }

    pub fn is_player(&self) -> bool {
        matches!(self, Self::PlayerPage { .. })
    }

    pub fn for_dest(dest: NavDest) -> Self {
        match dest {
            NavDest::Home => Self::HomePage {},
            NavDest::Search => Self::SearchPage {},
            NavDest::Library => Self::LibraryPage {},
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
    let mut width = use_signal(|| 1280.0_f64);
    let nav = use_signal(NavState::default);
    let mut shortcut_sig = use_signal(|| None::<ShortcutEvt>);
    let mut sys_tag = use_signal(|| "en".to_owned());
    let mut sys_dark = use_signal(|| true);
    use_context_provider(|| Ui { settings, width, nav, shortcut: shortcut_sig });
    use_context_provider(Snackbar::new);

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
                    if let Some(action) = shortcut(&key, ctrl, editable, player) {
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

    let in_player = route.is_player();
    let expanded = nav_expanded(nav_state(), (ui.width)());
    let toggle = move |_| {
        let next = reduce_nav(*nav_state.peek(), NavAction::Toggle);
        nav_state.set(next);
    };

    rsx! {
        div { class: "tk-shell", "data-player": "{in_player}",
            TitleBar { on_toggle_nav: toggle, offline: !(shell_status.online)(), link_port: shell_status.link_port() }
            div { class: "tk-shell__body",
                if !in_player {
                    NavRail {
                        current: route.nav_dest(),
                        expanded,
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
