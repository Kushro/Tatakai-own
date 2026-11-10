//! Extensions: install `.kai` (drop zone, file chooser, path or URL), enable/disable, remove, compatibility
//! pill with an explanatory banner, permission chips, hash / domains / date from
//! `Backend::extension_detail` and a collapsible per-extension log (`Backend::extension_logs`).
//!
//! Wave B (worker j; docs/redesign/16 section 4; classic `ExtensionServersSettings.tsx`, `RemoteExtensionsPanel.tsx`,
//! `ExtensionUpdateSheet.tsx`, `SideloadExtensionModal.tsx`, `ExtensionHubPage.tsx`, `CustomProvidersPage.tsx`):
//! tabs Installed · Servers · Stremio · Repositories · Providers.
//! * Installed: per-extension preferences form (`ext.preferences`, `[data-ext-pref]`, secrets masked), install by URL
//!   (`ext.install-url`; a Stremio manifest is added as an add-on), "Disable all" / "Enable all"
//!   (`ext.kill-switch`, `[data-kill-switch]`, rows `[data-enabled]`), the updates sheet (`ext.updates.*`,
//!   `[data-update-id]`, opened at launch when `Settings::ext.notify_updates`).
//! * Servers: paired extension servers (`ext.server-client.*`, `[data-server-id][data-server-online]`), pairing sheet
//!   (deep link / address + code / password), the sources each one offers (Aniyomi, Mihon, CloudStream, Sora,
//!   Mangayomi, Seanime... through the server: `ext.ecosystems.via-server`), and the confirmation of a
//!   `tatakai://extensionserver/connect` link the OS handed to the app (`ext.deep-link.server`,
//!   [`use_pending_server_link_watch`]).
//! * Stremio: add-ons by manifest URL (`ext.stremio*`, `[data-stremio-id]`).
//! * Repositories: indexes by URL with their catalogue and one-click install (`ext.repositories`, `[data-repo-id]`).
//! * Providers: extensions with custom provider pages (`ext.custom-providers.list`) and the saved provider titles.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use dioxus::prelude::*;

use crate::app::{Route, Services, Ui};
use crate::backend::{
    CustomProviderVm, ExtLogLevel, ExtPrefFieldVm, ExtPrefKindVm, ExtRepoVm, ExtServerPairVm, ExtServerVm, ExtUpdateVm,
    ExtensionVm, StremioAddonVm,
};
use crate::components::{
    Banner, Button, ButtonSize, ButtonVariant, ConfirmDialog, EmptyState, ErrorState, IconButton, InfoChip,
    LoadingIndicator, MediaCard, Pill, Segmented, Sheet, SkeletonLines, Switch, TextField, Tone, use_snackbar,
};
use crate::i18n::{t, tf, tn};
use crate::icons::Icon;
use crate::model::ext::{
    CHECK_INTERVALS, ExtPrefs, kill_switch_on, link_server, provider_id_parts, server_error_key,
};
use crate::model::{CompatLevel, ErrorKind, PlatformVm, SettingsAction, UiError, is_kai_path, reduce_settings};

/// Permission chips shown before collapsing the rest into `+n`.
const PERMS_SHOWN: usize = 6;
const FILE_INPUT_ID: &str = "tk-kai-file";
/// How often a `tatakai://` link handed to the app is looked for.
const PENDING_POLL_SECS: u64 = 2;
/// Error detail `install_extension_url` answers for a Stremio manifest (tk-app `ext_catalog::STREMIO_MANIFEST_DETAIL`).
const STREMIO_MANIFEST_DETAIL: &str = "stremio_manifest";

/// A pairing link waiting for its confirmation (set by [`use_pending_server_link_watch`] or the page itself).
static PENDING_LINK: GlobalSignal<Option<String>> = Signal::global(|| None);
/// The updates sheet opens by itself once per run.
static UPDATES_SHOWN: AtomicBool = AtomicBool::new(false);

/// Tabs of the page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExtTab {
    Installed,
    Servers,
    Stremio,
    Repos,
    Providers,
}

impl ExtTab {
    pub const ALL: [ExtTab; 5] = [Self::Installed, Self::Servers, Self::Stremio, Self::Repos, Self::Providers];

    pub fn id(self) -> &'static str {
        match self {
            Self::Installed => "installed",
            Self::Servers => "servers",
            Self::Stremio => "stremio",
            Self::Repos => "repos",
            Self::Providers => "providers",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|t| t.id() == id)
    }
}

fn tone(c: CompatLevel) -> Tone {
    match c {
        CompatLevel::Full => Tone::Ok,
        CompatLevel::Partial => Tone::Warn,
        CompatLevel::Unsupported => Tone::Err,
    }
}

/// Icon of a permission chip by its namespace (`network:domain:x`, `storage:*` ...).
pub fn perm_icon(perm: &str) -> &'static str {
    if perm.starts_with("network") {
        "language"
    } else if perm.starts_with("storage") {
        "storage"
    } else {
        "warning"
    }
}

/// `(i18n key, n)` for an age of `diff_secs` (negative ages count as now).
pub fn rel_bucket(diff_secs: i64) -> (&'static str, i64) {
    let d = diff_secs.max(0);
    match d {
        0..60 => ("time.now", 0),
        60..3_600 => ("time.min_ago", d / 60),
        3_600..86_400 => ("time.h_ago", d / 3_600),
        86_400..172_800 => ("time.yesterday", 1),
        _ => ("time.d_ago", d / 86_400),
    }
}

/// "5 min ago", "yesterday" ... (also used by Details for device last-seen).
pub fn rel_time(now_unix: i64, then_unix: i64) -> String {
    let (key, n) = rel_bucket(now_unix - then_unix);
    tf(key, &[("n", &n.to_string())])
}

fn now_unix() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

/// Strip spaces and the quotes Windows adds with "Copy as path" (or a shell adds when pasting a path).
fn clean_path(p: &str) -> String {
    p.trim().trim_matches(['"', '\'']).to_owned()
}

/// Example path for the install field, in the OS's own form.
fn path_placeholder() -> &'static str {
    match PlatformVm::current() {
        PlatformVm::Windows => "C:\\...\\extension.kai",
        PlatformVm::Linux => "/home/…/extension.kai",
        PlatformVm::Mac => "/Users/…/extension.kai",
    }
}

/// `http(s)://` text typed in a URL field.
fn looks_like_url(s: &str) -> bool {
    let s = s.trim().to_ascii_lowercase();
    (s.starts_with("https://") || s.starts_with("http://") || s.starts_with("stremio://")) && s.len() > 10
}

/// The localized cause of an extension-server error (pairing refusals have their own texts).
fn server_cause(e: &UiError) -> String {
    server_error_key(&e.detail).map(|k| t(&k)).unwrap_or_default()
}

/// Commit a new `Settings::ext` (saved at once, like the Settings sections).
fn use_commit_ext() -> Callback<ExtPrefs> {
    let services = use_context::<Services>();
    let ui = use_context::<Ui>();
    use_callback(move |p: ExtPrefs| {
        let next = reduce_settings(&ui.settings.peek(), SettingsAction::SetExt(p));
        ui.commit_settings(&services.backend, next);
    })
}

/// Mounted once by the app shell (integrator, next to `use_pending_magnet_watch`): polls
/// `Backend::take_pending_server_link` and, when a `tatakai://extensionserver/connect` link arrives, brings the window
/// forward and opens Extensions > Servers with the confirmation.
pub fn use_pending_server_link_watch() {
    let services = use_context::<Services>();
    let ui = use_context::<Ui>();
    let navigator = use_navigator();
    use_future(move || {
        let backend = services.backend.clone();
        async move {
            loop {
                let ready = *ui.settings_loaded.peek() && ui.settings.peek().system.setup_done;
                if ready && let Ok(Some(link)) = backend.take_pending_server_link().await {
                    *PENDING_LINK.write() = Some(link);
                    let window = dioxus::desktop::window();
                    window.window.set_visible(true);
                    window.window.set_minimized(false);
                    window.window.set_focus();
                    navigator.push(Route::ExtensionsPage {});
                }
                tokio::time::sleep(Duration::from_secs(PENDING_POLL_SECS)).await;
            }
        }
    });
}

#[component]
pub fn ExtensionsPage() -> Element {
    let services = use_context::<Services>();
    let ui = use_context::<Ui>();
    let snackbar = use_snackbar();
    let backend = services.backend.clone();
    let mut tab = use_signal(|| ExtTab::Installed);
    // UX review #7: the installer (drop zone + path + URL) folds away; the installed list comes first.
    let mut install_open = use_signal(|| false);
    let mut list = use_resource({
        let backend = backend.clone();
        move || {
            let backend = backend.clone();
            async move { backend.extensions().await }
        }
    });
    let mut updates = use_resource({
        let backend = backend.clone();
        move || {
            let backend = backend.clone();
            async move { backend.ext_updates_check().await }
        }
    });
    let mut show_updates = use_signal(|| false);
    let mut kill_busy = use_signal(|| false);
    let mut pending_remove = use_signal(|| None::<ExtensionVm>);
    // Toggle / remove / kill switch failures, shown inline above the list.
    let mut action_error = use_signal(|| None::<UiError>);

    // A pairing link the OS handed to the app: cold start (still in the backend) and while this page is open.
    use_future({
        let backend = backend.clone();
        move || {
            let backend = backend.clone();
            async move {
                loop {
                    if let Ok(Some(link)) = backend.take_pending_server_link().await {
                        *PENDING_LINK.write() = Some(link);
                    }
                    tokio::time::sleep(Duration::from_secs(PENDING_POLL_SECS)).await;
                }
            }
        }
    });
    use_effect(move || {
        if PENDING_LINK.read().is_some() && *tab.peek() != ExtTab::Servers {
            tab.set(ExtTab::Servers);
        }
    });
    // The updates sheet opens by itself once per run when there is something to update.
    use_effect(move || {
        let n = match &*updates.read() {
            Some(Ok(v)) => v.len(),
            _ => 0,
        };
        if n > 0 && ui.settings.peek().ext.notify_updates && !UPDATES_SHOWN.swap(true, Ordering::Relaxed) {
            show_updates.set(true);
        }
    });

    let installed = match &*list.read() {
        Some(Ok(items)) => Some(items.len()),
        _ => None,
    };
    let enabled_flags: Vec<bool> = match &*list.read() {
        Some(Ok(items)) => items.iter().map(|e| e.enabled).collect(),
        _ => Vec::new(),
    };
    let kill_on = kill_switch_on(&enabled_flags);
    let update_count = match &*updates.read() {
        Some(Ok(v)) => v.len(),
        _ => 0,
    };

    let kill = {
        let backend = backend.clone();
        move |_: MouseEvent| {
            if kill_busy() {
                return;
            }
            let enable = kill_on;
            let backend = backend.clone();
            kill_busy.set(true);
            action_error.set(None);
            spawn(async move {
                match backend.set_all_extensions_enabled(enable).await {
                    Ok(n) => {
                        let key = if enable { "ext.kill_switch.enabled" } else { "ext.kill_switch.disabled" };
                        snackbar.show(tn(key, n as usize));
                    }
                    Err(e) => action_error.set(Some(e)),
                }
                kill_busy.set(false);
                list.restart();
            });
        }
    };

    let body = match &*list.read() {
        None => rsx! { SkeletonLines { widths: vec![100, 100, 100] } },
        Some(Err(e)) => rsx! {
            ErrorState { error: e.clone(), on_retry: move |_| list.restart() }
        },
        Some(Ok(items)) if items.is_empty() => rsx! {
            EmptyState {
                icon: "extensions".to_string(),
                title: t("ext.empty.title"),
                body: t("ext.empty.body"),
            }
        },
        Some(Ok(items)) => rsx! {
            ul { class: "tk-extlist",
                for ext in items.clone() {
                    ExtensionRow {
                        key: "{ext.id}",
                        ext: ext.clone(),
                        on_toggle: {
                            let backend = backend.clone();
                            let id = ext.id.clone();
                            move |enabled: bool| {
                                let (backend, id) = (backend.clone(), id.clone());
                                action_error.set(None);
                                spawn(async move {
                                    if let Err(e) = backend.set_extension_enabled(&id, enabled).await {
                                        action_error.set(Some(e));
                                    }
                                    list.restart();
                                });
                            }
                        },
                        on_remove: move |e: ExtensionVm| pending_remove.set(Some(e)),
                    }
                }
            }
        },
    };

    let tabs: Vec<(String, String)> = ExtTab::ALL.iter().map(|x| (x.id().to_owned(), t(&format!("ext.tab.{}", x.id())))).collect();
    let kill_label = if kill_on { t("ext.kill_switch.enable_all") } else { t("ext.kill_switch.disable_all") };
    let updates_label = if update_count > 0 { tf("ext.updates.button_n", &[("n", &update_count.to_string())]) } else { t("ext.updates.button") };
    let current = tab();
    let kill_attr = if kill_on { "on" } else { "off" };
    let tab_body = match current {
        ExtTab::Installed => rsx! {
            {body}
            if install_open() {
                InstallPanel {
                    on_installed: move |_| {
                        install_open.set(false);
                        list.restart();
                        updates.restart();
                    },
                }
            } else {
                div { class: "tk-ext-install-cta", "data-install-open": "false",
                    Button {
                        label: t("ext.install.open"),
                        icon: "plus".to_string(),
                        variant: ButtonVariant::Tonal,
                        onclick: move |_| install_open.set(true),
                    }
                    Button {
                        label: t("ext.repos.add_cta"),
                        icon: "link".to_string(),
                        variant: ButtonVariant::Outlined,
                        onclick: move |_| tab.set(ExtTab::Repos),
                    }
                }
            }
        },
        ExtTab::Servers => rsx! { ServersPanel {} },
        ExtTab::Stremio => rsx! { StremioPanel {} },
        ExtTab::Repos => rsx! { ReposPanel { on_installed: move |_| { list.restart(); updates.restart(); } } },
        ExtTab::Providers => rsx! { ProvidersPanel {} },
    };

    rsx! {
        div { class: "tk-page tk-page--ext", "data-ext-tab": "{current.id()}",
            div { class: "tk-page__head",
                div {
                    h1 { class: "tk-page__title", {t("ext.title")} }
                    p { class: "tk-page__intro tk-muted", {t("ext.intro")} }
                }
                if let Some(n) = installed {
                    Pill { text: tn("ext.installed", n) }
                }
            }
            div { class: "tk-ext-toolbar", "data-kill-switch": "{kill_attr}",
                Segmented {
                    options: tabs,
                    selected: current.id().to_string(),
                    label: t("ext.tabs"),
                    on_select: move |id: String| {
                        if let Some(x) = ExtTab::from_id(&id) {
                            tab.set(x);
                        }
                    },
                }
                div { class: "tk-ext-toolbar__actions",
                    Button {
                        label: updates_label,
                        icon: "update".to_string(),
                        variant: if update_count > 0 { ButtonVariant::Tonal } else { ButtonVariant::Outlined },
                        size: ButtonSize::Sm,
                        onclick: move |_| show_updates.set(true),
                    }
                    Button {
                        label: kill_label,
                        icon: if kill_on { "play".to_string() } else { "pause".to_string() },
                        variant: ButtonVariant::Outlined,
                        size: ButtonSize::Sm,
                        disabled: kill_busy() || installed.unwrap_or(0) == 0,
                        onclick: kill,
                    }
                }
            }
            if kill_on && current == ExtTab::Installed {
                Banner { text: t("ext.kill_switch.banner"), icon: "warning".to_string(), tone: Tone::Warn }
            }
            if let Some(err) = action_error() {
                ErrorState { error: err.clone(), compact: true, on_retry: move |_| {
                    action_error.set(None);
                    list.restart();
                } }
            }
            {tab_body}
            if show_updates() {
                UpdatesSheet {
                    on_close: move |_| show_updates.set(false),
                    on_changed: move |_| {
                        list.restart();
                        updates.restart();
                    },
                }
            }
            if let Some(ext) = pending_remove() {
                ConfirmDialog {
                    title: t("ext.remove.title"),
                    body: tf("ext.remove.body", &[("name", &ext.name)]),
                    confirm_label: t("common.remove"),
                    icon: "trash".to_string(),
                    on_cancel: move |_| pending_remove.set(None),
                    on_confirm: {
                        let backend = backend.clone();
                        move |_| {
                            let (backend, id, name) = (backend.clone(), ext.id.clone(), ext.name.clone());
                            pending_remove.set(None);
                            action_error.set(None);
                            spawn(async move {
                                match backend.remove_extension(&id).await {
                                    Ok(()) => snackbar.show(tf("ext.removed", &[("name", &name)])),
                                    Err(e) => action_error.set(Some(e)),
                                }
                                list.restart();
                            });
                        }
                    },
                }
            }
        }
    }
}

/// Install a `.kai`: drop zone / file chooser / path, and by URL (`ext.install-url`).
#[component]
fn InstallPanel(#[props(default)] on_installed: EventHandler<()>) -> Element {
    let services = use_context::<Services>();
    let snackbar = use_snackbar();
    let backend = services.backend.clone();
    let mut path = use_signal(String::new);
    let mut url = use_signal(String::new);
    let mut install_error = use_signal(|| None::<UiError>);
    let mut url_error = use_signal(|| None::<UiError>);
    let mut invalid = use_signal(|| false);
    let mut url_invalid = use_signal(|| false);
    let mut busy = use_signal(|| false);
    let mut over = use_signal(|| false);

    let install = use_callback({
        let backend = backend.clone();
        move |p: String| {
            let p = clean_path(&p);
            if !is_kai_path(&p) {
                invalid.set(true);
                return;
            }
            invalid.set(false);
            install_error.set(None);
            busy.set(true);
            let backend = backend.clone();
            spawn(async move {
                match backend.install_extension(&p).await {
                    Ok(ext) => {
                        path.set(String::new());
                        snackbar.show(tf("ext.install.ok", &[("name", &ext.name)]));
                        on_installed.call(());
                    }
                    Err(e) => install_error.set(Some(e)),
                }
                busy.set(false);
            });
        }
    });

    let install_url = use_callback({
        let backend = backend.clone();
        move |u: String| {
            let u = u.trim().to_owned();
            if !looks_like_url(&u) {
                url_invalid.set(true);
                return;
            }
            url_invalid.set(false);
            url_error.set(None);
            busy.set(true);
            let backend = backend.clone();
            spawn(async move {
                match backend.install_extension_url(&u).await {
                    Ok(ext) => {
                        url.set(String::new());
                        snackbar.show(tf("ext.install.ok", &[("name", &ext.name)]));
                        on_installed.call(());
                    }
                    // A Stremio manifest: added as an add-on instead.
                    Err(e) if e.detail == STREMIO_MANIFEST_DETAIL => match backend.stremio_add(&u).await {
                        Ok(a) => {
                            url.set(String::new());
                            snackbar.show(tf("stremio.added", &[("name", &a.name)]));
                        }
                        Err(e) => url_error.set(Some(e)),
                    },
                    Err(e) => url_error.set(Some(e)),
                }
                busy.set(false);
            });
        }
    });

    let drop_label = t("ext.install");
    rsx! {
        section { class: "tk-panel tk-install",
            label {
                class: "tk-drop",
                r#for: "{FILE_INPUT_ID}",
                role: "button",
                tabindex: "0",
                "aria-label": "{drop_label}",
                "aria-disabled": "{busy()}",
                "data-over": "{over()}",
                ondragenter: move |e| {
                    e.prevent_default();
                    over.set(true);
                },
                ondragover: move |e| {
                    e.prevent_default();
                    if !*over.peek() {
                        over.set(true);
                    }
                },
                ondragleave: move |_| over.set(false),
                ondrop: move |e| {
                    e.prevent_default();
                    over.set(false);
                    if busy() {
                        return;
                    }
                    // Desktop drops carry the real file path.
                    match e.data_transfer().files().first() {
                        Some(f) => install.call(f.path().to_string_lossy().into_owned()),
                        None => invalid.set(true),
                    }
                },
                onkeydown: move |e| {
                    let activate = match e.key() {
                        Key::Enter => true,
                        Key::Character(c) => c == " ",
                        _ => false,
                    };
                    if activate {
                        e.prevent_default();
                        let _ = document::eval(&format!("const i=document.getElementById('{FILE_INPUT_ID}');if(i){{i.click();}}"));
                    }
                },
                if busy() {
                    LoadingIndicator { small: true }
                } else {
                    Icon { name: "download".to_string(), size: 32 }
                }
                b { "{drop_label}" }
                span { class: "tk-drop__hint", {t("ext.install.drop")} }
                input {
                    id: "{FILE_INPUT_ID}",
                    r#type: "file",
                    accept: ".kai",
                    hidden: true,
                    disabled: busy(),
                    onchange: move |e| {
                        if let Some(f) = e.files().first() {
                            install.call(f.path().to_string_lossy().into_owned());
                        }
                    },
                }
            }
            div { class: "tk-install__or", {t("ext.install.or")} }
            div { class: "tk-install__path",
                TextField {
                    label: t("ext.install.path"),
                    value: path(),
                    icon: "folder".to_string(),
                    mono: true,
                    error: invalid(),
                    hint: if invalid() { t("ext.install.invalid") } else { String::new() },
                    placeholder: path_placeholder().to_string(),
                    oninput: move |v: String| {
                        invalid.set(false);
                        path.set(v);
                    },
                    onsubmit: move |_| install.call(path.peek().clone()),
                }
                Button {
                    label: t("ext.install.go"),
                    variant: ButtonVariant::Tonal,
                    disabled: busy() || path().trim().is_empty(),
                    onclick: move |_| install.call(path.peek().clone()),
                }
            }
            if invalid() && path().trim().is_empty() {
                p { class: "tk-form__error", role: "alert", {t("ext.install.invalid")} }
            }
            if let Some(err) = install_error() {
                ErrorState { error: err.clone(), compact: true, on_retry: move |_| install.call(path.peek().clone()) }
            }
            div { class: "tk-install__path", "data-install-url": "true",
                TextField {
                    label: t("ext.install_url.label"),
                    value: url(),
                    icon: "link".to_string(),
                    mono: true,
                    error: url_invalid(),
                    hint: if url_invalid() { t("ext.install_url.invalid") } else { t("ext.install_url.hint") },
                    placeholder: "https://…/extension.kai".to_string(),
                    oninput: move |v: String| {
                        url_invalid.set(false);
                        url.set(v);
                    },
                    onsubmit: move |_| install_url.call(url.peek().clone()),
                }
                Button {
                    label: t("ext.install_url.go"),
                    variant: ButtonVariant::Tonal,
                    disabled: busy() || url().trim().is_empty(),
                    onclick: move |_| install_url.call(url.peek().clone()),
                }
            }
            if let Some(err) = url_error() {
                ErrorState { error: err.clone(), compact: true, on_retry: move |_| install_url.call(url.peek().clone()) }
            }
        }
    }
}

#[component]
fn ExtensionRow(
    ext: ExtensionVm,
    #[props(default)] on_toggle: EventHandler<bool>,
    #[props(default)] on_remove: EventHandler<ExtensionVm>,
) -> Element {
    let services = use_context::<Services>();
    let mut detail = use_resource({
        let (backend, id) = (services.backend.clone(), ext.id.clone());
        move || {
            let (backend, id) = (backend.clone(), id.clone());
            async move { backend.extension_detail(&id).await }
        }
    });
    let mut prefs = use_resource({
        let (backend, id) = (services.backend.clone(), ext.id.clone());
        move || {
            let (backend, id) = (backend.clone(), id.clone());
            async move { backend.ext_preferences(&id).await }
        }
    });
    let mut show_log = use_signal(|| false);
    let mut show_prefs = use_signal(|| false);

    let compat_label = t(&format!("compat.{}", ext.compat.id()));
    let can_enable = ext.compat.can_enable();
    let for_remove = ext.clone();
    let initial = ext.name.chars().next().map(|c| c.to_uppercase().to_string()).unwrap_or_default();
    let version = tf("ext.version", &[("v", &ext.version)]);
    let log_label = if show_log() { t("ext.hide_logs") } else { t("ext.logs") };
    let now = now_unix();
    let hash_label = t("ext.hash");
    let pref_count = match &*prefs.read() {
        Some(Ok(v)) => v.len(),
        _ => 0,
    };
    let prefs_label = if show_prefs() { t("ext.prefs.hide") } else { tn("ext.prefs.show", pref_count) };
    let prefs_failed = matches!(&*prefs.read(), Some(Err(e)) if e.kind != ErrorKind::NotWired);
    let pref_fields: Vec<ExtPrefFieldVm> = match &*prefs.read() {
        Some(Ok(v)) => v.clone(),
        _ => Vec::new(),
    };

    let foot = match &*detail.read() {
        None => rsx! {
            span { class: "tk-skel tk-skel--line tk-ext__footskel", "aria-hidden": "true" }
        },
        Some(Err(e)) => {
            let retry = e.kind != ErrorKind::NotWired;
            rsx! {
                ErrorState { error: e.clone(), compact: true, retry, on_retry: move |_| detail.restart() }
            }
        }
        Some(Ok(d)) => {
            let domains = d.domains.join(", ");
            rsx! {
                if let Some(ts) = d.updated_unix {
                    span { {tf("ext.updated", &[("when", &rel_time(now, ts))])} }
                }
                if !d.hash.is_empty() {
                    span { "{hash_label} " code { class: "tk-mono", "{d.hash}" } }
                }
                span { title: "{domains}", {tn("ext.domains", d.domains.len())} }
                if !d.kind.is_empty() {
                    span { class: "tk-mono", "{d.kind}" }
                }
            }
        }
    };

    rsx! {
        li { class: "tk-ext", "data-compat": "{ext.compat.id()}", "data-enabled": "{ext.enabled}", "data-ext-id": "{ext.id}",
            div { class: "tk-ext__head",
                div { class: "tk-ext__ico", "aria-hidden": "true", "{initial}" }
                div { class: "tk-ext__name",
                    b {
                        span { "{ext.name}" }
                        Pill { text: compat_label, tone: tone(ext.compat) }
                    }
                    span { class: "tk-ext__ver", "{version} · {ext.id}" }
                }
                div { class: "tk-ext__ctl",
                    span { class: "tk-ext__ctllabel", {t("ext.enabled")} }
                    Switch {
                        checked: ext.enabled && can_enable,
                        label: t("ext.enabled"),
                        disabled: !can_enable,
                        on_toggle,
                    }
                    IconButton {
                        icon: "trash".to_string(),
                        label: t("ext.remove"),
                        onclick: move |_| on_remove.call(for_remove.clone()),
                    }
                }
            }
            div { class: "tk-ext__perms",
                span { class: "tk-ext__permlabel", {t("ext.permissions")} }
                if ext.permissions.is_empty() {
                    InfoChip { text: t("ext.no_permissions") }
                }
                for p in ext.permissions.iter().take(PERMS_SHOWN) {
                    InfoChip { key: "{p}", text: p.clone(), icon: perm_icon(p).to_string() }
                }
                if ext.permissions.len() > PERMS_SHOWN {
                    InfoChip { text: format!("+{}", ext.permissions.len() - PERMS_SHOWN) }
                }
            }
            if ext.compat == CompatLevel::Unsupported {
                Banner { text: t("ext.cannot_enable"), icon: "warning".to_string(), tone: Tone::Warn }
            }
            if ext.compat == CompatLevel::Partial {
                Banner { text: t("ext.compat.partial_hint"), icon: "info".to_string() }
            }
            div { class: "tk-ext__foot",
                {foot}
                if pref_count > 0 {
                    Button {
                        label: prefs_label,
                        icon: "tune".to_string(),
                        variant: ButtonVariant::Text,
                        size: ButtonSize::Xs,
                        onclick: move |_| show_prefs.set(!show_prefs()),
                    }
                }
                if prefs_failed {
                    Button {
                        label: t("ext.prefs.retry"),
                        icon: "refresh".to_string(),
                        variant: ButtonVariant::Text,
                        size: ButtonSize::Xs,
                        onclick: move |_| prefs.restart(),
                    }
                }
                Button {
                    label: log_label,
                    icon: "terminal".to_string(),
                    variant: ButtonVariant::Text,
                    size: ButtonSize::Xs,
                    class: "tk-ext__logbtn".to_string(),
                    onclick: move |_| show_log.set(!show_log()),
                }
            }
            if show_prefs() && !pref_fields.is_empty() {
                PrefsForm { id: ext.id.clone(), fields: pref_fields.clone(), on_saved: move |_| prefs.restart() }
            }
            if show_log() {
                ExtLog { id: ext.id.clone() }
            }
        }
    }
}

/// The preferences an extension declares (`ext.preferences`). Toggles and selects save at once; text and secret
/// fields save with their button (secrets are masked and never echoed back in the log).
#[component]
fn PrefsForm(id: String, fields: Vec<ExtPrefFieldVm>, #[props(default)] on_saved: EventHandler<()>) -> Element {
    let services = use_context::<Services>();
    let snackbar = use_snackbar();
    let mut error = use_signal(|| None::<UiError>);
    let mut saving = use_signal(|| false);

    let save = use_callback({
        let (backend, id) = (services.backend.clone(), id.clone());
        move |(key, value): (String, String)| {
            let (backend, id) = (backend.clone(), id.clone());
            saving.set(true);
            error.set(None);
            spawn(async move {
                match backend.set_ext_preference(&id, &key, &value).await {
                    Ok(()) => {
                        snackbar.show(t("ext.prefs.saved"));
                        on_saved.call(());
                    }
                    Err(e) => error.set(Some(e)),
                }
                saving.set(false);
            });
        }
    });

    rsx! {
        div { class: "tk-extprefs", "data-ext-prefs": "{id}",
            for f in fields {
                PrefField { key: "{f.key}", field: f.clone(), disabled: saving(), on_save: move |kv: (String, String)| save.call(kv) }
            }
            if let Some(e) = error() {
                ErrorState { error: e, compact: true, retry: false }
            }
        }
    }
}

#[component]
fn PrefField(field: ExtPrefFieldVm, #[props(default)] disabled: bool, #[props(default)] on_save: EventHandler<(String, String)>) -> Element {
    let mut draft = use_signal(|| field.value.clone());
    let kind = match field.kind {
        ExtPrefKindVm::Text => "text",
        ExtPrefKindVm::Secret => "secret",
        ExtPrefKindVm::Toggle => "toggle",
        ExtPrefKindVm::Select => "select",
    };
    let key = field.key.clone();
    let hint = field.hint.clone().unwrap_or_default();
    let control = match field.kind {
        ExtPrefKindVm::Toggle => {
            let key = key.clone();
            rsx! {
                Switch {
                    checked: field.value == "true",
                    label: field.label.clone(),
                    disabled,
                    on_toggle: move |on: bool| on_save.call((key.clone(), on.to_string())),
                }
            }
        }
        ExtPrefKindVm::Select => {
            let key = key.clone();
            let label = field.label.clone();
            let current = field.value.clone();
            rsx! {
                select {
                    class: "tk-extprefs__select",
                    "aria-label": "{label}",
                    value: "{current}",
                    disabled: disabled,
                    onchange: move |e: FormEvent| on_save.call((key.clone(), e.value())),
                    for (v , l) in field.options.clone() {
                        option { key: "{v}", value: "{v}", selected: v == current, "{l}" }
                    }
                }
            }
        }
        ExtPrefKindVm::Text | ExtPrefKindVm::Secret => {
            let secret = field.kind == ExtPrefKindVm::Secret;
            let (k1, k2) = (key.clone(), key.clone());
            let changed = draft() != field.value;
            rsx! {
                div { class: "tk-install__path",
                    TextField {
                        label: field.label.clone(),
                        value: draft(),
                        kind: if secret { "password".to_string() } else { "text".to_string() },
                        mono: secret,
                        disabled,
                        hint: hint.clone(),
                        oninput: move |v: String| draft.set(v),
                        onsubmit: move |_| on_save.call((k1.clone(), draft.peek().clone())),
                    }
                    Button {
                        label: t("common.save"),
                        variant: ButtonVariant::Tonal,
                        size: ButtonSize::Sm,
                        disabled: disabled || !changed,
                        onclick: move |_| on_save.call((k2.clone(), draft.peek().clone())),
                    }
                }
            }
        }
    };
    let show_label = !matches!(field.kind, ExtPrefKindVm::Text | ExtPrefKindVm::Secret);
    rsx! {
        div { class: "tk-extprefs__row", "data-ext-pref": "{key}", "data-pref-kind": "{kind}",
            if show_label {
                div { class: "tk-extprefs__text",
                    span { class: "tk-extprefs__label", "{field.label}" }
                    if !hint.is_empty() {
                        span { class: "tk-extprefs__hint tk-muted", "{hint}" }
                    }
                }
            }
            div { class: "tk-extprefs__ctl", {control} }
        }
    }
}

/// The extension's log, newest first. Mounted only while expanded, so it loads on open.
#[component]
fn ExtLog(id: String) -> Element {
    let services = use_context::<Services>();
    let mut logs = use_resource({
        let (backend, id) = (services.backend.clone(), id.clone());
        move || {
            let (backend, id) = (backend.clone(), id.clone());
            async move { backend.extension_logs(&id).await }
        }
    });
    let now = now_unix();
    let view = match &*logs.read() {
        None => rsx! { SkeletonLines { widths: vec![90, 70, 80] } },
        Some(Err(e)) => {
            let retry = e.kind != ErrorKind::NotWired;
            rsx! {
                ErrorState { error: e.clone(), compact: true, retry, on_retry: move |_| logs.restart() }
            }
        }
        Some(Ok(lines)) if lines.is_empty() => rsx! {
            p { class: "tk-muted tk-log__empty", {t("ext.logs_empty")} }
        },
        Some(Ok(lines)) => rsx! {
            pre { class: "tk-log",
                for (i , l) in lines.iter().enumerate() {
                    {
                        let level = match l.level {
                            ExtLogLevel::Ok => "ok",
                            ExtLogLevel::Warn => "warn",
                            ExtLogLevel::Err => "err",
                        };
                        let when = l.at_unix.map(|ts| rel_time(now, ts)).unwrap_or_default();
                        rsx! {
                            div { key: "{i}", class: "tk-log__line", "data-level": "{level}",
                                span { class: "tk-log__lvl", "[{level}] " }
                                "{l.op}: {l.message}"
                                if !when.is_empty() {
                                    span { class: "tk-log__at", " · {when}" }
                                }
                            }
                        }
                    }
                }
            }
        },
    };
    view
}

// ---------------------------------------------------------------------------------------------
// Updates sheet
// ---------------------------------------------------------------------------------------------

/// Available updates with their notes, "Update" per row and "Update all" (`ext.updates.sheet`), plus the update
/// preferences (`Settings::ext`).
#[component]
fn UpdatesSheet(#[props(default)] on_close: EventHandler<()>, #[props(default)] on_changed: EventHandler<()>) -> Element {
    let services = use_context::<Services>();
    let ui = use_context::<Ui>();
    let snackbar = use_snackbar();
    let commit = use_commit_ext();
    let backend = services.backend.clone();
    let mut updates = use_resource({
        let backend = backend.clone();
        move || {
            let backend = backend.clone();
            async move { backend.ext_updates_check().await }
        }
    });
    let mut busy = use_signal(|| None::<String>);
    let mut error = use_signal(|| None::<UiError>);
    let prefs = ui.settings.read().ext.clone();

    let update_one = use_callback({
        let backend = backend.clone();
        move |id: String| {
            let backend = backend.clone();
            busy.set(Some(id.clone()));
            error.set(None);
            spawn(async move {
                match backend.ext_update(&id).await {
                    Ok(ext) => {
                        snackbar.show(tf("ext.updates.done_one", &[("name", &ext.name), ("v", &ext.version)]));
                        on_changed.call(());
                    }
                    Err(e) => error.set(Some(e)),
                }
                busy.set(None);
                updates.restart();
            });
        }
    });
    let update_all = {
        let backend = backend.clone();
        move |_: MouseEvent| {
            let backend = backend.clone();
            busy.set(Some(String::new()));
            error.set(None);
            spawn(async move {
                match backend.ext_update_all().await {
                    Ok(n) => {
                        snackbar.show(tn("ext.updates.done_all", n as usize));
                        on_changed.call(());
                    }
                    Err(e) => error.set(Some(e)),
                }
                busy.set(None);
                updates.restart();
            });
        }
    };

    let count = match &*updates.read() {
        Some(Ok(v)) => v.len(),
        _ => 0,
    };
    let body = match &*updates.read() {
        None => rsx! { SkeletonLines { widths: vec![100, 80, 90] } },
        Some(Err(e)) => rsx! { ErrorState { error: e.clone(), compact: true, on_retry: move |_| updates.restart() } },
        Some(Ok(list)) if list.is_empty() => rsx! {
            EmptyState { icon: "check".to_string(), title: t("ext.updates.none.title"), body: t("ext.updates.none.body") }
        },
        Some(Ok(list)) => rsx! {
            ul { class: "tk-extupd",
                for u in list.clone() {
                    UpdateRow {
                        key: "{u.id}",
                        update: u.clone(),
                        busy: busy().is_some(),
                        working: busy().as_deref() == Some(u.id.as_str()),
                        on_update: move |id: String| update_one.call(id),
                    }
                }
            }
        },
    };
    let interval_opts: Vec<(String, String)> =
        CHECK_INTERVALS.iter().map(|h| (h.to_string(), tf("ext.updates.every_h", &[("n", &h.to_string())]))).collect();
    let footer = rsx! {
        Button {
            label: t("ext.updates.all"),
            icon: "update".to_string(),
            disabled: busy().is_some() || count == 0,
            onclick: update_all,
        }
    };

    rsx! {
        Sheet { title: t("ext.updates.title"), wide: true, on_close: move |_| on_close.call(()), footer,
            div { class: "tk-extupd__body", "data-updates": "{count}",
                {body}
                if let Some(e) = error() {
                    ErrorState { error: e, compact: true, retry: false }
                }
                div { class: "tk-extopts",
                    OptRow { label: t("ext.updates.auto"), hint: t("ext.updates.auto_hint"),
                        Switch {
                            checked: prefs.auto_check_updates,
                            label: t("ext.updates.auto"),
                            on_toggle: {
                                let p = prefs.clone();
                                move |on: bool| commit.call(ExtPrefs { auto_check_updates: on, ..p.clone() })
                            },
                        }
                    }
                    OptRow { label: t("ext.updates.interval"), stack: true,
                        Segmented {
                            options: interval_opts,
                            selected: prefs.check_interval_hours.to_string(),
                            label: t("ext.updates.interval"),
                            small: true,
                            on_select: {
                                let p = prefs.clone();
                                move |id: String| {
                                    if let Ok(h) = id.parse::<u8>() {
                                        commit.call(ExtPrefs { check_interval_hours: h, ..p.clone() });
                                    }
                                }
                            },
                        }
                    }
                    OptRow { label: t("ext.updates.notify"), hint: t("ext.updates.notify_hint"),
                        Switch {
                            checked: prefs.notify_updates,
                            label: t("ext.updates.notify"),
                            on_toggle: {
                                let p = prefs.clone();
                                move |on: bool| commit.call(ExtPrefs { notify_updates: on, ..p.clone() })
                            },
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn UpdateRow(update: ExtUpdateVm, busy: bool, working: bool, #[props(default)] on_update: EventHandler<String>) -> Element {
    let id = update.id.clone();
    rsx! {
        li { class: "tk-extupd__row", "data-update-id": "{update.id}",
            div { class: "tk-extupd__text",
                b { "{update.name}" }
                span { class: "tk-mono tk-muted", {tf("ext.updates.versions", &[("from", &update.current), ("to", &update.latest)])} }
                if let Some(n) = &update.notes {
                    p { class: "tk-extupd__notes", "{n}" }
                }
            }
            if working {
                LoadingIndicator { small: true }
            } else {
                Button {
                    label: t("ext.updates.one"),
                    variant: ButtonVariant::Tonal,
                    size: ButtonSize::Sm,
                    disabled: busy || update.url.is_none(),
                    onclick: move |_| on_update.call(id.clone()),
                }
            }
        }
    }
}

/// One option row (label + hint + control), the layout of the Settings rows.
#[component]
fn OptRow(label: String, #[props(default)] hint: String, #[props(default)] stack: bool, children: Element) -> Element {
    rsx! {
        div { class: "tk-setting-row", "data-stack": "{stack}",
            div { class: "tk-setting-row__text",
                div { class: "tk-setting-row__label", "{label}" }
                if !hint.is_empty() {
                    div { class: "tk-setting-row__hint", "{hint}" }
                }
            }
            div { class: "tk-setting-row__ctl", {children} }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Extension servers
// ---------------------------------------------------------------------------------------------

#[component]
fn ServersPanel() -> Element {
    let services = use_context::<Services>();
    let ui = use_context::<Ui>();
    let snackbar = use_snackbar();
    let commit = use_commit_ext();
    let backend = services.backend.clone();
    let mut servers = use_resource({
        let backend = backend.clone();
        move || {
            let backend = backend.clone();
            async move { backend.ext_servers().await }
        }
    });
    let mut pairing = use_signal(|| false);
    let mut confirm_link = use_signal(|| None::<String>);
    let mut link_error = use_signal(|| None::<UiError>);
    let mut link_busy = use_signal(|| false);
    let mut pending_remove = use_signal(|| None::<ExtServerVm>);
    let mut action_error = use_signal(|| None::<UiError>);
    let prefs = ui.settings.read().ext.clone();

    // A link the OS handed over: ask before connecting.
    use_effect(move || {
        let pending = PENDING_LINK.read().clone();
        if let Some(link) = pending {
            *PENDING_LINK.write() = None;
            link_error.set(None);
            confirm_link.set(Some(link));
        }
    });

    let body = match &*servers.read() {
        None => rsx! { SkeletonLines { widths: vec![100, 100] } },
        Some(Err(e)) => rsx! { ErrorState { error: e.clone(), on_retry: move |_| servers.restart() } },
        Some(Ok(list)) if list.is_empty() => rsx! {
            EmptyState {
                icon: "desktop".to_string(),
                title: t("extserver.empty.title"),
                body: t("extserver.empty.body"),
                action_label: t("extserver.pair"),
                on_action: move |_| pairing.set(true),
            }
        },
        Some(Ok(list)) => rsx! {
            ul { class: "tk-extlist",
                for s in list.clone() {
                    ServerRow {
                        key: "{s.id}",
                        server: s.clone(),
                        on_changed: move |_| servers.restart(),
                        on_error: move |e: UiError| action_error.set(Some(e)),
                        on_remove: move |s: ExtServerVm| pending_remove.set(Some(s)),
                    }
                }
            }
        },
    };

    let confirm_server = confirm_link().as_deref().and_then(link_server).unwrap_or_default();
    rsx! {
        section { class: "tk-panel tk-extsrv", "data-ext-servers": "true",
            div { class: "tk-extsrv__head",
                div {
                    h2 { class: "tk-panel__title", {t("extserver.title")} }
                    p { class: "tk-muted", {t("extserver.intro")} }
                }
                Button { label: t("extserver.pair"), icon: "link".to_string(), onclick: move |_| pairing.set(true) }
            }
            OptRow { label: t("extserver.fallback"), hint: t("extserver.fallback_hint"),
                Switch {
                    checked: prefs.server_fallback_local,
                    label: t("extserver.fallback"),
                    on_toggle: {
                        let p = prefs.clone();
                        move |on: bool| commit.call(ExtPrefs { server_fallback_local: on, ..p.clone() })
                    },
                }
            }
        }
        if let Some(e) = action_error() {
            ErrorState { error: e.clone(), compact: true, cause: server_cause(&e), on_retry: move |_| {
                action_error.set(None);
                servers.restart();
            } }
        }
        {body}
        if pairing() {
            PairSheet {
                on_close: move |_| pairing.set(false),
                on_paired: move |s: ExtServerVm| {
                    pairing.set(false);
                    snackbar.show(tf("extserver.paired", &[("name", &s.name)]));
                    servers.restart();
                },
            }
        }
        if let Some(link) = confirm_link() {
            ConfirmDialog {
                title: t("extserver.deep_link.title"),
                body: tf("extserver.deep_link.body", &[("server", &confirm_server)]),
                confirm_label: t("extserver.deep_link.connect"),
                icon: "link".to_string(),
                danger: false,
                confirm_disabled: link_busy(),
                on_cancel: move |_| {
                    confirm_link.set(None);
                    link_error.set(None);
                },
                on_confirm: {
                    let backend = backend.clone();
                    move |_| {
                        let (backend, link) = (backend.clone(), link.clone());
                        link_busy.set(true);
                        link_error.set(None);
                        spawn(async move {
                            let req = ExtServerPairVm { url: link, ..ExtServerPairVm::default() };
                            match backend.ext_server_pair(&req).await {
                                Ok(s) => {
                                    confirm_link.set(None);
                                    snackbar.show(tf("extserver.paired", &[("name", &s.name)]));
                                    servers.restart();
                                }
                                Err(e) => link_error.set(Some(e)),
                            }
                            link_busy.set(false);
                        });
                    }
                },
                div { class: "tk-extsrv__confirm", "data-deep-link": "pending",
                    code { class: "tk-mono", "{confirm_server}" }
                    if link_busy() {
                        LoadingIndicator { small: true }
                    }
                    if let Some(e) = link_error() {
                        ErrorState { error: e.clone(), compact: true, retry: false, cause: server_cause(&e) }
                    }
                }
            }
        }
        if let Some(s) = pending_remove() {
            ConfirmDialog {
                title: t("extserver.remove.title"),
                body: tf("extserver.remove.body", &[("name", &s.name)]),
                confirm_label: t("common.remove"),
                icon: "trash".to_string(),
                on_cancel: move |_| pending_remove.set(None),
                on_confirm: {
                    let backend = backend.clone();
                    move |_| {
                        let (backend, id) = (backend.clone(), s.id.clone());
                        pending_remove.set(None);
                        spawn(async move {
                            if let Err(e) = backend.ext_server_remove(&id).await {
                                action_error.set(Some(e));
                            }
                            servers.restart();
                        });
                    }
                },
            }
        }
    }
}

#[component]
fn ServerRow(
    server: ExtServerVm,
    #[props(default)] on_changed: EventHandler<()>,
    #[props(default)] on_error: EventHandler<UiError>,
    #[props(default)] on_remove: EventHandler<ExtServerVm>,
) -> Element {
    let services = use_context::<Services>();
    let mut testing = use_signal(|| false);
    let mut show_sources = use_signal(|| false);

    let s = server.clone();
    let online = match s.online {
        Some(true) => "true",
        Some(false) => "false",
        None => "unknown",
    };
    let (status_text, status_tone) = match s.online {
        Some(true) => (t("extserver.online"), Tone::Ok),
        Some(false) => (t("extserver.offline"), Tone::Err),
        None => (t("extserver.untested"), Tone::Neutral),
    };
    let now = now_unix();
    let test = {
        let (backend, id) = (services.backend.clone(), s.id.clone());
        move |_: MouseEvent| {
            let (backend, id) = (backend.clone(), id.clone());
            testing.set(true);
            spawn(async move {
                // The list re-reads the server state the test recorded (online, sources, last error).
                if let Err(e) = backend.ext_server_test(&id).await {
                    on_error.call(e);
                }
                testing.set(false);
                on_changed.call(());
            });
        }
    };
    let toggle = {
        let (backend, id) = (services.backend.clone(), s.id.clone());
        move |on: bool| {
            let (backend, id) = (backend.clone(), id.clone());
            spawn(async move {
                if let Err(e) = backend.ext_server_set_enabled(&id, on).await {
                    on_error.call(e);
                }
                on_changed.call(());
            });
        }
    };
    let for_remove = s.clone();
    let initial = s.name.chars().next().map(|c| c.to_uppercase().to_string()).unwrap_or_default();
    let offline_text = match &s.last_error {
        Some(e) => tf("extserver.offline_banner_detail", &[("detail", e)]),
        None => t("extserver.offline_banner"),
    };
    let sources_label = if show_sources() { t("extserver.sources.hide") } else { tn("extserver.sources.show", s.sources as usize) };

    rsx! {
        li { class: "tk-ext", "data-server-id": "{s.id}", "data-server-online": "{online}", "data-enabled": "{s.enabled}",
            div { class: "tk-ext__head",
                div { class: "tk-ext__ico", "aria-hidden": "true", "{initial}" }
                div { class: "tk-ext__name",
                    b {
                        span { "{s.name}" }
                        Pill { text: status_text, tone: status_tone }
                        if !s.role.is_empty() {
                            Pill { text: s.role.clone(), plain: true }
                        }
                    }
                    span { class: "tk-ext__ver", "{s.url}" }
                }
                div { class: "tk-ext__ctl",
                    span { class: "tk-ext__ctllabel", {t("ext.enabled")} }
                    Switch { checked: s.enabled, label: t("ext.enabled"), on_toggle: toggle }
                    IconButton { icon: "trash".to_string(), label: t("extserver.remove"), onclick: move |_| on_remove.call(for_remove.clone()) }
                }
            }
            if s.online == Some(false) {
                Banner { text: offline_text, icon: "offline".to_string(), tone: Tone::Warn }
            }
            div { class: "tk-ext__foot",
                if let Some(ts) = s.last_ok_unix {
                    span { {tf("extserver.last_ok", &[("when", &rel_time(now, ts))])} }
                }
                if testing() {
                    LoadingIndicator { small: true }
                } else {
                    Button { label: t("extserver.test"), icon: "sync".to_string(), variant: ButtonVariant::Text, size: ButtonSize::Xs, onclick: test }
                }
                Button {
                    label: sources_label,
                    icon: "extensions".to_string(),
                    variant: ButtonVariant::Text,
                    size: ButtonSize::Xs,
                    class: "tk-ext__logbtn".to_string(),
                    onclick: move |_| show_sources.set(!show_sources()),
                }
            }
            if show_sources() {
                ServerSources { id: s.id.clone() }
            }
        }
    }
}

/// Sources a server offers (`GET /sources`), with their ecosystem (Aniyomi, CloudStream, Sora ...).
#[component]
fn ServerSources(id: String) -> Element {
    let services = use_context::<Services>();
    let mut sources = use_resource({
        let (backend, id) = (services.backend.clone(), id.clone());
        move || {
            let (backend, id) = (backend.clone(), id.clone());
            async move { backend.ext_server_sources(&id).await }
        }
    });
    match &*sources.read() {
        None => rsx! { SkeletonLines { widths: vec![80, 60] } },
        Some(Err(e)) => rsx! { ErrorState { error: e.clone(), compact: true, cause: server_cause(e), on_retry: move |_| sources.restart() } },
        Some(Ok(list)) if list.is_empty() => rsx! { p { class: "tk-muted tk-log__empty", {t("extserver.sources.empty")} } },
        Some(Ok(list)) => rsx! {
            ul { class: "tk-extsrc",
                for src in list.clone() {
                    li { key: "{src.id}", class: "tk-extsrc__row", "data-source-id": "{src.id}", "data-ecosystem": "{src.ecosystem}", "data-available": "{src.available}",
                        b { "{src.name}" }
                        Pill { text: src.ecosystem.clone(), plain: true }
                        InfoChip { text: t(&format!("extserver.kind.{}", kind_id(&src.kind))) }
                        if !src.languages.is_empty() {
                            span { class: "tk-mono tk-muted", {src.languages.join(", ")} }
                        }
                        if !src.enabled {
                            Pill { text: t("extserver.sources.disabled"), tone: Tone::Warn }
                        } else if !src.available {
                            Pill { text: src.reason.clone().unwrap_or_else(|| t("extserver.sources.unavailable")), tone: Tone::Err }
                        }
                    }
                }
            }
        },
    }
}

/// `watch` / `read` / `novel` (anything else counts as `watch`).
fn kind_id(kind: &str) -> &'static str {
    match kind {
        "read" => "read",
        "novel" => "novel",
        _ => "watch",
    }
}

/// Pair with a server: a pasted deep link, or the address with a code or the viewer password.
#[component]
fn PairSheet(#[props(default)] on_close: EventHandler<()>, #[props(default)] on_paired: EventHandler<ExtServerVm>) -> Element {
    let services = use_context::<Services>();
    let mut url = use_signal(String::new);
    let mut code = use_signal(String::new);
    let mut password = use_signal(String::new);
    let mut name = use_signal(String::new);
    let mut busy = use_signal(|| false);
    let mut error = use_signal(|| None::<UiError>);
    let is_link = url().trim().to_ascii_lowercase().starts_with("tatakai://");
    let pairing_attr = if busy() { "busy" } else { "idle" };

    let submit = use_callback({
        let backend = services.backend.clone();
        move |_: ()| {
            if url.peek().trim().is_empty() || *busy.peek() {
                return;
            }
            let opt = |s: String| Some(s.trim().to_owned()).filter(|s| !s.is_empty());
            let req = ExtServerPairVm {
                url: url.peek().trim().to_owned(),
                code: opt(code.peek().clone()),
                password: Some(password.peek().clone()).filter(|p| !p.is_empty()),
                name: opt(name.peek().clone()),
            };
            let backend = backend.clone();
            busy.set(true);
            error.set(None);
            spawn(async move {
                match backend.ext_server_pair(&req).await {
                    Ok(s) => on_paired.call(s),
                    Err(e) => error.set(Some(e)),
                }
                busy.set(false);
            });
        }
    });

    let footer = rsx! {
        Button { label: t("common.cancel"), variant: ButtonVariant::Text, onclick: move |_| on_close.call(()) }
        Button { label: t("extserver.pair.go"), icon: "link".to_string(), disabled: busy() || url().trim().is_empty(), onclick: move |_| submit.call(()) }
    };
    rsx! {
        Sheet { title: t("extserver.pair.title"), on_close: move |_| on_close.call(()), footer,
            div { class: "tk-extpair", "data-pairing": "{pairing_attr}",
                p { class: "tk-muted", {t("extserver.pair.intro")} }
                TextField {
                    label: t("extserver.pair.url"),
                    value: url(),
                    icon: "link".to_string(),
                    mono: true,
                    placeholder: "tatakai://extensionserver/connect?… · http://192.168.1.10:8099".to_string(),
                    hint: t("extserver.pair.url_hint"),
                    oninput: move |v: String| url.set(v),
                    onsubmit: move |_| submit.call(()),
                }
                if !is_link {
                    TextField {
                        label: t("extserver.pair.code"),
                        value: code(),
                        mono: true,
                        hint: t("extserver.pair.code_hint"),
                        oninput: move |v: String| code.set(v),
                        onsubmit: move |_| submit.call(()),
                    }
                    TextField {
                        label: t("extserver.pair.password"),
                        value: password(),
                        kind: "password".to_string(),
                        hint: t("extserver.pair.password_hint"),
                        oninput: move |v: String| password.set(v),
                        onsubmit: move |_| submit.call(()),
                    }
                }
                TextField {
                    label: t("extserver.pair.name"),
                    value: name(),
                    hint: t("common.optional"),
                    oninput: move |v: String| name.set(v),
                    onsubmit: move |_| submit.call(()),
                }
                if busy() {
                    LoadingIndicator { small: true }
                }
                if let Some(e) = error() {
                    ErrorState { error: e.clone(), compact: true, retry: false, cause: server_cause(&e) }
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Stremio
// ---------------------------------------------------------------------------------------------

#[component]
fn StremioPanel() -> Element {
    let services = use_context::<Services>();
    let ui = use_context::<Ui>();
    let snackbar = use_snackbar();
    let commit = use_commit_ext();
    let backend = services.backend.clone();
    let mut addons = use_resource({
        let backend = backend.clone();
        move || {
            let backend = backend.clone();
            async move { backend.stremio_addons().await }
        }
    });
    let mut url = use_signal(String::new);
    let mut busy = use_signal(|| false);
    let mut error = use_signal(|| None::<UiError>);
    let mut pending_remove = use_signal(|| None::<StremioAddonVm>);
    let prefs = ui.settings.read().ext.clone();

    let add = use_callback({
        let backend = backend.clone();
        move |_: ()| {
            let u = url.peek().trim().to_owned();
            if !looks_like_url(&u) {
                error.set(Some(UiError::new(ErrorKind::Other, t("ext.install_url.invalid"))));
                return;
            }
            let backend = backend.clone();
            busy.set(true);
            error.set(None);
            spawn(async move {
                match backend.stremio_add(&u).await {
                    Ok(a) => {
                        url.set(String::new());
                        snackbar.show(tf("stremio.added", &[("name", &a.name)]));
                        addons.restart();
                    }
                    Err(e) => error.set(Some(e)),
                }
                busy.set(false);
            });
        }
    });

    let body = match &*addons.read() {
        None => rsx! { SkeletonLines { widths: vec![100, 100] } },
        Some(Err(e)) => rsx! { ErrorState { error: e.clone(), on_retry: move |_| addons.restart() } },
        Some(Ok(list)) if list.is_empty() => rsx! {
            EmptyState { icon: "film".to_string(), title: t("stremio.empty.title"), body: t("stremio.empty.body") }
        },
        Some(Ok(list)) => rsx! {
            ul { class: "tk-extlist",
                for a in list.clone() {
                    StremioRow {
                        key: "{a.id}",
                        addon: a.clone(),
                        on_changed: move |_| addons.restart(),
                        on_error: move |e: UiError| error.set(Some(e)),
                        on_remove: move |a: StremioAddonVm| pending_remove.set(Some(a)),
                    }
                }
            }
        },
    };

    rsx! {
        section { class: "tk-panel tk-install", "data-stremio": "true",
            div {
                h2 { class: "tk-panel__title", {t("stremio.title")} }
                p { class: "tk-muted", {t("stremio.intro")} }
            }
            OptRow { label: t("stremio.enabled"), hint: t("stremio.enabled_hint"),
                Switch {
                    checked: prefs.stremio_enabled,
                    label: t("stremio.enabled"),
                    on_toggle: {
                        let p = prefs.clone();
                        move |on: bool| commit.call(ExtPrefs { stremio_enabled: on, ..p.clone() })
                    },
                }
            }
            div { class: "tk-install__path",
                TextField {
                    label: t("stremio.url"),
                    value: url(),
                    icon: "link".to_string(),
                    mono: true,
                    placeholder: "https://…/manifest.json".to_string(),
                    hint: t("stremio.url_hint"),
                    oninput: move |v: String| url.set(v),
                    onsubmit: move |_| add.call(()),
                }
                Button { label: t("stremio.add"), variant: ButtonVariant::Tonal, disabled: busy() || url().trim().is_empty(), onclick: move |_| add.call(()) }
            }
            if busy() {
                LoadingIndicator { small: true }
            }
            if let Some(e) = error() {
                ErrorState { error: e, compact: true, retry: false }
            }
        }
        {body}
        if let Some(a) = pending_remove() {
            ConfirmDialog {
                title: t("stremio.remove.title"),
                body: tf("stremio.remove.body", &[("name", &a.name)]),
                confirm_label: t("common.remove"),
                icon: "trash".to_string(),
                on_cancel: move |_| pending_remove.set(None),
                on_confirm: {
                    let backend = backend.clone();
                    move |_| {
                        let (backend, id) = (backend.clone(), a.id.clone());
                        pending_remove.set(None);
                        spawn(async move {
                            if let Err(e) = backend.stremio_remove(&id).await {
                                error.set(Some(e));
                            }
                            addons.restart();
                        });
                    }
                },
            }
        }
    }
}

#[component]
fn StremioRow(
    addon: StremioAddonVm,
    #[props(default)] on_changed: EventHandler<()>,
    #[props(default)] on_error: EventHandler<UiError>,
    #[props(default)] on_remove: EventHandler<StremioAddonVm>,
) -> Element {
    let services = use_context::<Services>();
    let snackbar = use_snackbar();
    let mut busy = use_signal(|| false);
    let initial = addon.name.chars().next().map(|c| c.to_uppercase().to_string()).unwrap_or_default();
    let refresh = {
        let (backend, url) = (services.backend.clone(), addon.manifest_url.clone());
        move |reset: bool| {
            let (backend, url) = (backend.clone(), url.clone());
            busy.set(true);
            spawn(async move {
                match backend.stremio_add(&url).await {
                    Ok(a) => {
                        let key = if reset { "stremio.matches_reset" } else { "stremio.updated" };
                        snackbar.show(tf(key, &[("name", &a.name), ("v", &a.version)]));
                    }
                    Err(e) => on_error.call(e),
                }
                busy.set(false);
                on_changed.call(());
            });
        }
    };
    let (mut update, mut reset) = (refresh.clone(), refresh);
    let toggle = {
        let (backend, id) = (services.backend.clone(), addon.id.clone());
        move |on: bool| {
            let (backend, id) = (backend.clone(), id.clone());
            spawn(async move {
                if let Err(e) = backend.stremio_set_enabled(&id, on).await {
                    on_error.call(e);
                }
                on_changed.call(());
            });
        }
    };
    let for_remove = addon.clone();
    rsx! {
        li { class: "tk-ext", "data-stremio-id": "{addon.id}", "data-enabled": "{addon.enabled}",
            div { class: "tk-ext__head",
                div { class: "tk-ext__ico", "aria-hidden": "true", "{initial}" }
                div { class: "tk-ext__name",
                    b {
                        span { "{addon.name}" }
                        if !addon.version.is_empty() {
                            Pill { text: tf("ext.version", &[("v", &addon.version)]), plain: true }
                        }
                    }
                    span { class: "tk-ext__ver", "{addon.manifest_url}" }
                }
                div { class: "tk-ext__ctl",
                    span { class: "tk-ext__ctllabel", {t("ext.enabled")} }
                    Switch { checked: addon.enabled, label: t("ext.enabled"), on_toggle: toggle }
                    IconButton { icon: "trash".to_string(), label: t("stremio.remove"), onclick: move |_| on_remove.call(for_remove.clone()) }
                }
            }
            if let Some(d) = &addon.description {
                p { class: "tk-muted tk-extsrv__desc", "{d}" }
            }
            div { class: "tk-ext__perms",
                for ty in addon.types.iter() {
                    InfoChip { key: "t-{ty}", text: ty.clone(), icon: "film".to_string() }
                }
                for r in addon.resources.iter() {
                    InfoChip { key: "r-{r}", text: r.clone() }
                }
            }
            div { class: "tk-ext__foot",
                if busy() {
                    LoadingIndicator { small: true }
                }
                Button { label: t("stremio.update"), icon: "update".to_string(), variant: ButtonVariant::Text, size: ButtonSize::Xs, disabled: busy(), onclick: move |_| update(false) }
                Button { label: t("stremio.reset_matches"), icon: "replay".to_string(), variant: ButtonVariant::Text, size: ButtonSize::Xs, disabled: busy(), onclick: move |_| reset(true) }
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Repositories
// ---------------------------------------------------------------------------------------------

#[component]
fn ReposPanel(#[props(default)] on_installed: EventHandler<()>) -> Element {
    let services = use_context::<Services>();
    let snackbar = use_snackbar();
    let backend = services.backend.clone();
    let mut repos = use_resource({
        let backend = backend.clone();
        move || {
            let backend = backend.clone();
            async move { backend.ext_repos().await }
        }
    });
    let mut url = use_signal(String::new);
    let mut busy = use_signal(|| false);
    let mut error = use_signal(|| None::<UiError>);
    let mut pending_remove = use_signal(|| None::<ExtRepoVm>);

    let add = use_callback({
        let backend = backend.clone();
        move |_: ()| {
            let u = url.peek().trim().to_owned();
            if !looks_like_url(&u) {
                error.set(Some(UiError::new(ErrorKind::Other, t("ext.install_url.invalid"))));
                return;
            }
            let backend = backend.clone();
            busy.set(true);
            error.set(None);
            spawn(async move {
                match backend.ext_repo_add(&u).await {
                    Ok(r) => {
                        url.set(String::new());
                        snackbar.show(tf("repo.added", &[("name", &r.name), ("n", &r.items.to_string())]));
                        repos.restart();
                    }
                    Err(e) => error.set(Some(e)),
                }
                busy.set(false);
            });
        }
    });

    let body = match &*repos.read() {
        None => rsx! { SkeletonLines { widths: vec![100, 100] } },
        Some(Err(e)) => rsx! { ErrorState { error: e.clone(), on_retry: move |_| repos.restart() } },
        Some(Ok(list)) if list.is_empty() => rsx! {
            EmptyState { icon: "storage".to_string(), title: t("repo.empty.title"), body: t("repo.empty.body") }
        },
        Some(Ok(list)) => rsx! {
            ul { class: "tk-extlist",
                for r in list.clone() {
                    RepoRow {
                        key: "{r.id}",
                        repo: r.clone(),
                        on_installed: move |_| on_installed.call(()),
                        on_remove: move |r: ExtRepoVm| pending_remove.set(Some(r)),
                    }
                }
            }
        },
    };

    rsx! {
        section { class: "tk-panel tk-install", "data-repos": "true",
            div {
                h2 { class: "tk-panel__title", {t("repo.title")} }
                p { class: "tk-muted", {t("repo.intro")} }
            }
            div { class: "tk-install__path",
                TextField {
                    label: t("repo.url"),
                    value: url(),
                    icon: "link".to_string(),
                    mono: true,
                    placeholder: "https://…/index.json".to_string(),
                    oninput: move |v: String| url.set(v),
                    onsubmit: move |_| add.call(()),
                }
                Button { label: t("repo.add"), variant: ButtonVariant::Tonal, disabled: busy() || url().trim().is_empty(), onclick: move |_| add.call(()) }
            }
            if busy() {
                LoadingIndicator { small: true }
            }
            if let Some(e) = error() {
                ErrorState { error: e, compact: true, retry: false }
            }
        }
        {body}
        if let Some(r) = pending_remove() {
            ConfirmDialog {
                title: t("repo.remove.title"),
                body: tf("repo.remove.body", &[("name", &r.name)]),
                confirm_label: t("common.remove"),
                icon: "trash".to_string(),
                on_cancel: move |_| pending_remove.set(None),
                on_confirm: {
                    let backend = backend.clone();
                    move |_| {
                        let (backend, id) = (backend.clone(), r.id.clone());
                        pending_remove.set(None);
                        spawn(async move {
                            if let Err(e) = backend.ext_repo_remove(&id).await {
                                error.set(Some(e));
                            }
                            repos.restart();
                        });
                    }
                },
            }
        }
    }
}

#[component]
fn RepoRow(repo: ExtRepoVm, #[props(default)] on_installed: EventHandler<()>, #[props(default)] on_remove: EventHandler<ExtRepoVm>) -> Element {
    let mut open = use_signal(|| false);
    let now = now_unix();
    let for_remove = repo.clone();
    let initial = repo.name.chars().next().map(|c| c.to_uppercase().to_string()).unwrap_or_default();
    let label = if open() { t("repo.catalog.hide") } else { tn("repo.catalog.show", repo.items as usize) };
    rsx! {
        li { class: "tk-ext", "data-repo-id": "{repo.id}", "data-repo-items": "{repo.items}",
            div { class: "tk-ext__head",
                div { class: "tk-ext__ico", "aria-hidden": "true", "{initial}" }
                div { class: "tk-ext__name",
                    b { span { "{repo.name}" } }
                    span { class: "tk-ext__ver", "{repo.url}" }
                }
                div { class: "tk-ext__ctl",
                    IconButton { icon: "trash".to_string(), label: t("repo.remove"), onclick: move |_| on_remove.call(for_remove.clone()) }
                }
            }
            if let Some(e) = &repo.error {
                Banner { text: e.clone(), icon: "warning".to_string(), tone: Tone::Warn }
            }
            div { class: "tk-ext__foot",
                if let Some(ts) = repo.fetched_unix {
                    span { {tf("repo.fetched", &[("when", &rel_time(now, ts))])} }
                }
                Button { label, icon: "grid".to_string(), variant: ButtonVariant::Text, size: ButtonSize::Xs, class: "tk-ext__logbtn".to_string(), onclick: move |_| open.set(!open()) }
            }
            if open() {
                RepoCatalog { id: repo.id.clone(), on_installed: move |_| on_installed.call(()) }
            }
        }
    }
}

#[component]
fn RepoCatalog(id: String, #[props(default)] on_installed: EventHandler<()>) -> Element {
    let services = use_context::<Services>();
    let snackbar = use_snackbar();
    let mut refresh = use_signal(|| false);
    let mut catalog = use_resource({
        let (backend, id) = (services.backend.clone(), id.clone());
        move || {
            let (backend, id) = (backend.clone(), id.clone());
            let fresh = refresh();
            async move { backend.ext_repo_catalog(&id, fresh).await }
        }
    });
    let mut installing = use_signal(|| None::<String>);
    let mut error = use_signal(|| None::<UiError>);

    let install = use_callback({
        let backend = services.backend.clone();
        move |(item_id, url): (String, String)| {
            let backend = backend.clone();
            installing.set(Some(item_id));
            error.set(None);
            spawn(async move {
                match backend.install_extension_url(&url).await {
                    Ok(ext) => {
                        snackbar.show(tf("ext.install.ok", &[("name", &ext.name)]));
                        on_installed.call(());
                    }
                    Err(e) => error.set(Some(e)),
                }
                installing.set(None);
                catalog.restart();
            });
        }
    });

    let body = match &*catalog.read() {
        None => rsx! { SkeletonLines { widths: vec![90, 70, 80] } },
        Some(Err(e)) => rsx! { ErrorState { error: e.clone(), compact: true, on_retry: move |_| catalog.restart() } },
        Some(Ok(items)) if items.is_empty() => rsx! { p { class: "tk-muted tk-log__empty", {t("repo.catalog.empty")} } },
        Some(Ok(items)) => rsx! {
            ul { class: "tk-extsrc",
                for i in items.clone() {
                    {
                        let action = if !i.installed {
                            Some(t("repo.catalog.install"))
                        } else if i.installed_version.as_deref() != Some(i.version.as_str()) && !i.version.is_empty() {
                            Some(t("repo.catalog.update"))
                        } else {
                            None
                        };
                        let (item_id, url) = (i.id.clone(), i.url.clone());
                        let working = installing().as_deref() == Some(i.id.as_str());
                        rsx! {
                            li { key: "{i.id}", class: "tk-extsrc__row", "data-catalog-id": "{i.id}", "data-installed": "{i.installed}",
                                div { class: "tk-extsrc__text",
                                    b { "{i.name}" }
                                    if !i.description.is_empty() {
                                        span { class: "tk-muted", "{i.description}" }
                                    }
                                }
                                InfoChip { text: t(&format!("repo.kind.{}", repo_kind(&i.kind))) }
                                if !i.version.is_empty() {
                                    span { class: "tk-mono tk-muted", {tf("ext.version", &[("v", &i.version)])} }
                                }
                                if working {
                                    LoadingIndicator { small: true }
                                } else if let Some(label) = action {
                                    Button {
                                        label,
                                        variant: ButtonVariant::Tonal,
                                        size: ButtonSize::Sm,
                                        disabled: installing().is_some(),
                                        onclick: move |_| install.call((item_id.clone(), url.clone())),
                                    }
                                } else {
                                    Pill { text: t("repo.catalog.installed"), tone: Tone::Ok }
                                }
                            }
                        }
                    }
                }
            }
        },
    };
    rsx! {
        div { class: "tk-repocat", "data-repo-catalog": "{id}",
            div { class: "tk-ext__foot",
                Button { label: t("repo.catalog.refresh"), icon: "refresh".to_string(), variant: ButtonVariant::Text, size: ButtonSize::Xs, onclick: move |_| {
                    if refresh() { catalog.restart() } else { refresh.set(true) }
                } }
            }
            {body}
            if let Some(e) = error() {
                ErrorState { error: e, compact: true, retry: false }
            }
        }
    }
}

/// `anime` / `manga` / `novel` / `torrent` (anything else counts as `anime`).
fn repo_kind(kind: &str) -> &'static str {
    match kind {
        "manga" => "manga",
        "novel" => "novel",
        "torrent" => "torrent",
        _ => "anime",
    }
}

// ---------------------------------------------------------------------------------------------
// Custom providers
// ---------------------------------------------------------------------------------------------

/// Providers grouped by extension, in the order the backend lists them.
fn group_providers(list: &[CustomProviderVm]) -> Vec<(String, String, Vec<CustomProviderVm>)> {
    let mut out: Vec<(String, String, Vec<CustomProviderVm>)> = Vec::new();
    for p in list {
        match out.iter_mut().find(|(id, _, _)| *id == p.ext_id) {
            Some((_, _, v)) => v.push(p.clone()),
            None => out.push((p.ext_id.clone(), p.ext_name.clone(), vec![p.clone()])),
        }
    }
    out
}

#[component]
fn ProvidersPanel() -> Element {
    let services = use_context::<Services>();
    let navigator = use_navigator();
    let backend = services.backend.clone();
    let mut providers = use_resource({
        let backend = backend.clone();
        move || {
            let backend = backend.clone();
            async move { backend.custom_providers().await }
        }
    });
    let mut exts = use_resource({
        let backend = backend.clone();
        move || {
            let backend = backend.clone();
            async move { backend.extensions().await }
        }
    });
    let mut bookmarks = use_resource({
        let backend = backend.clone();
        move || {
            let backend = backend.clone();
            async move { backend.provider_bookmarks().await }
        }
    });
    let mut error = use_signal(|| None::<UiError>);
    let enabled_of = |id: &str| match &*exts.read() {
        Some(Ok(list)) => list.iter().find(|e| e.id == id).map(|e| (e.enabled, e.compat.can_enable())),
        _ => None,
    };

    let body = match &*providers.read() {
        None => rsx! { SkeletonLines { widths: vec![100, 100] } },
        Some(Err(e)) => rsx! { ErrorState { error: e.clone(), on_retry: move |_| providers.restart() } },
        Some(Ok(list)) if list.is_empty() => rsx! {
            EmptyState { icon: "extensions".to_string(), title: t("provider_page.list.empty.title"), body: t("provider_page.list.empty.body") }
        },
        Some(Ok(list)) => rsx! {
            ul { class: "tk-extlist",
                for (ext_id , ext_name , sources) in group_providers(list) {
                    {
                        let (enabled, can) = enabled_of(&ext_id).unwrap_or((false, false));
                        let open_id = ext_id.clone();
                        let toggle = {
                            let (backend, id) = (backend.clone(), ext_id.clone());
                            move |on: bool| {
                                let (backend, id) = (backend.clone(), id.clone());
                                spawn(async move {
                                    if let Err(e) = backend.set_extension_enabled(&id, on).await {
                                        error.set(Some(e));
                                    }
                                    exts.restart();
                                });
                            }
                        };
                        rsx! {
                            li { key: "{ext_id}", class: "tk-ext", "data-provider-ext": "{ext_id}", "data-enabled": "{enabled}",
                                div { class: "tk-ext__head",
                                    div { class: "tk-ext__name",
                                        button {
                                            class: "tk-provider__open",
                                            r#type: "button",
                                            disabled: !enabled,
                                            onclick: move |_| {
                                                navigator.push(Route::ProviderPage { ext_id: open_id.clone() });
                                            },
                                            b { "{ext_name}" }
                                            Icon { name: "external".to_string(), size: 16 }
                                        }
                                        span { class: "tk-ext__ver", "{ext_id}" }
                                    }
                                    div { class: "tk-ext__ctl",
                                        span { class: "tk-ext__ctllabel", {t("ext.enabled")} }
                                        Switch { checked: enabled && can, label: t("ext.enabled"), disabled: !can, on_toggle: toggle }
                                    }
                                }
                                div { class: "tk-ext__perms",
                                    for s in sources {
                                        InfoChip { key: "{s.name}", text: s.name.clone(), icon: if s.kind == "anime" { "film".to_string() } else { "library".to_string() } }
                                    }
                                }
                                if !enabled {
                                    p { class: "tk-muted", {t("provider_page.list.disabled")} }
                                }
                            }
                        }
                    }
                }
            }
        },
    };

    let saved = match &*bookmarks.read() {
        None => rsx! { SkeletonLines { widths: vec![60] } },
        Some(Err(e)) => rsx! { ErrorState { error: e.clone(), compact: true, on_retry: move |_| bookmarks.restart() } },
        Some(Ok(list)) if list.is_empty() => rsx! { p { class: "tk-muted", {t("provider_page.saved.empty")} } },
        Some(Ok(list)) => rsx! {
            div { class: "tk-row",
                for m in list.clone() {
                    {
                        let id = m.id.clone();
                        rsx! {
                            MediaCard {
                                key: "{m.id}",
                                media: m.clone(),
                                onclick: move |_| {
                                    navigator.push(provider_route(&id, None));
                                },
                            }
                        }
                    }
                }
            }
        },
    };

    rsx! {
        section { class: "tk-panel", "data-providers": "true",
            h2 { class: "tk-panel__title", {t("provider_page.list.title")} }
            p { class: "tk-muted", {t("provider_page.list.intro")} }
        }
        if let Some(e) = error() {
            ErrorState { error: e, compact: true, retry: false }
        }
        {body}
        section { class: "tk-section", "data-provider-bookmarks": "true",
            div { class: "tk-section__head",
                h2 { class: "tk-section__title", {t("provider_page.saved")} }
            }
            {saved}
        }
    }
}

/// Where a provider card goes: the manga / novel details for `read` / `novel` providers, the normal Details page
/// (episodes + player) otherwise. `kind` is the provider's kind when known (`anime` / `manga` / `novel`).
pub fn provider_route(id: &str, kind: Option<&str>) -> Route {
    match kind {
        Some("manga") => Route::MangaDetailsPage { id: id.to_owned() },
        Some("novel") => Route::NovelDetailsPage { id: id.to_owned() },
        _ => Route::DetailsPage { id: id.to_owned() },
    }
}

/// `true` for the ids [`provider_route`] handles (`ext:<ext>:<source>:<item>`).
pub fn is_provider_card(id: &str) -> bool {
    provider_id_parts(id).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_time_buckets() {
        assert_eq!(rel_bucket(-5), ("time.now", 0));
        assert_eq!(rel_bucket(59), ("time.now", 0));
        assert_eq!(rel_bucket(125), ("time.min_ago", 2));
        assert_eq!(rel_bucket(7_200), ("time.h_ago", 2));
        assert_eq!(rel_bucket(90_000), ("time.yesterday", 1));
        assert_eq!(rel_bucket(3 * 86_400 + 5), ("time.d_ago", 3));
    }

    #[test]
    fn permission_icons_and_paths() {
        assert_eq!(perm_icon("network:domain:example.org"), "language");
        assert_eq!(perm_icon("storage:kv"), "storage");
        assert_eq!(perm_icon("torrent:session"), "warning");
        assert_eq!(clean_path("  \"C:\\x\\toko.kai\" "), "C:\\x\\toko.kai");
    }

    #[test]
    fn tabs_round_trip_and_urls() {
        for tab in ExtTab::ALL {
            assert_eq!(ExtTab::from_id(tab.id()), Some(tab));
        }
        assert_eq!(ExtTab::from_id("nope"), None);
        assert!(looks_like_url("https://example.org/x.kai"));
        assert!(looks_like_url("stremio://addon.example/manifest.json"));
        assert!(!looks_like_url("C:\\x.kai"));
        assert_eq!(kind_id("read"), "read");
        assert_eq!(kind_id("anime"), "watch");
        assert_eq!(repo_kind("x"), "anime");
    }

    #[test]
    fn providers_group_by_extension() {
        let p = |ext: &str, name: &str| CustomProviderVm {
            ext_id: ext.into(),
            ext_name: ext.to_uppercase(),
            name: name.into(),
            kind: "anime".into(),
            has_home: true,
            has_search: true,
        };
        let g = group_providers(&[p("a", "1"), p("b", "2"), p("a", "3")]);
        assert_eq!(g.len(), 2);
        assert_eq!((g[0].0.as_str(), g[0].2.len()), ("a", 2));
        assert!(is_provider_card("ext:a:s:1"));
        assert!(matches!(provider_route("ext:a:s:1", Some("manga")), Route::MangaDetailsPage { .. }));
        assert!(matches!(provider_route("ext:a:s:1", None), Route::DetailsPage { .. }));
    }
}
