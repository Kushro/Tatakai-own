//! First-run wizard (wave A, worker **f**; plan 14 section 0.1 #11, checklist `account.setup-wizard`; classic
//! `src/pages/auth/SetupPage.tsx`, trimmed to what Next has). Route `/setup`, drawn without the rail
//! (`Route::is_bare`). The shell redirects here while `Settings::system.setup_done` is false (`app.rs::Shell`, only
//! after `Ui::settings_loaded`); "Skip for now" and "Start watching" both set `setup_done = true` through
//! `SettingsAction::SetSystem` and go to `Route::HomePage`. Settings > System offers "Run the setup assistant again".
//!
//! Steps (each exposes `data-setup-step="<id>"` on the page):
//! 1. `appearance`: language, theme mode, palette, reduce motion (the Appearance `SettingsAction`s).
//! 2. `player`: player engine (`Settings::player_mode`; Embedded enhanced disabled while this installation lacks
//!    mpv-anime-build, `Backend::anime_build_status`) and source preferences: preferred kind, minimum quality,
//!    subtitle and audio language (`Settings::sources`, the same `SettingsAction`s as Settings > Player).
//! 3. `downloads`: torrent download folder and "after watching" (`Backend::torrent_settings` /
//!    `save_torrent_settings`; saved when leaving the step).
//! 4. `extensions`: what `.kai` extensions are, installed count, "Install from file" (`Backend::install_extension`).
//! 5. `accounts`: AniList / MAL connect buttons (`Backend::list_accounts` / `list_connect`). Optional.
//! 6. `system`: start with the system, tray icon (when `system_caps` offers them), Discord presence
//!    (`Settings::flow.discord_presence`), check for updates, open magnet links with Tatakai (when offered).
//! 7. `done`: summary + "Start watching".
//!
//! No step blocks: every control has a default, "Next" is always enabled, a failing backend call is shown inline
//! and the wizard goes on. No terms / privacy screens (nothing is collected: apps/README.md rule 3). Leaving the
//! wizard to another page is not offered (the shell would bring the user back while `setup_done` is false).

use dioxus::prelude::*;

use crate::app::{Route, Services, Ui};
use crate::backend::{AfterWatchVm, SourceKindVm, TorrentSettingsVm};
use crate::components::{
    Button, ButtonSize, ButtonVariant, ErrorState, LoadingIndicator, Pill, Segmented, SkeletonLines, Switch, TextField, Tone,
    use_snackbar,
};
use crate::i18n::{LangChoice, t, tf, tn};
use crate::icons::Icon;
use crate::model::{
    DUB_LANGS, ListServiceVm, PALETTES, PlayerModeVm, QualityVm, SUB_LANGS, SettingsAction, ThemeMode, UiError, is_kai_path,
    reduce_settings,
};

/// Step ids in order (`data-setup-step`, `setup.step.<id>` titles).
pub const STEPS: [&str; 7] = ["appearance", "player", "downloads", "extensions", "accounts", "system", "done"];
const FILE_INPUT_ID: &str = "tk-setup-kai";

/// Index of the step after `i` (stays on the last one).
pub fn next_step(i: usize) -> usize {
    (i + 1).min(STEPS.len() - 1)
}

/// Index of the step before `i` (stays on the first one).
pub fn prev_step(i: usize) -> usize {
    i.saturating_sub(1)
}

/// `data-state` of the progress dot `n` while step `current` shows.
pub fn dot_state(n: usize, current: usize) -> &'static str {
    match n.cmp(&current) {
        std::cmp::Ordering::Less => "done",
        std::cmp::Ordering::Equal => "current",
        std::cmp::Ordering::Greater => "todo",
    }
}

/// Engine options to disable: Embedded enhanced when the installation is known to lack mpv-anime-build
/// (`available == Some(false)`), unless it is already the saved choice (it stays selectable so Settings > Player
/// can explain why it cannot run). Unknown availability (still loading, or the call failed) disables nothing.
pub fn engine_disabled_keys(available: Option<bool>, mode: PlayerModeVm) -> Vec<String> {
    if available == Some(false) && mode != PlayerModeVm::EmbeddedEnhanced {
        vec![PlayerModeVm::EmbeddedEnhanced.id().to_owned()]
    } else {
        Vec::new()
    }
}

/// `(id, label)` options of the preferred-source-kind Segmented: `any` first (no preference), then every kind.
pub fn prefer_options() -> Vec<(String, String)> {
    let mut out = vec![("any".to_owned(), t("src.prefer.any"))];
    out.extend(SourceKindVm::ALL.iter().map(|k| (k.id().to_owned(), t(&format!("src.{}", k.id())))));
    out
}

/// Language options shown as upper-case codes (`ES`, `JA`), like Settings > Player.
pub fn lang_options(langs: &[&str]) -> Vec<(String, String)> {
    langs.iter().map(|l| ((*l).to_owned(), l.to_uppercase())).collect()
}

/// `(id, label)` options of a Segmented from `ids` and an i18n prefix.
fn options(ids: &[&'static str], prefix: &str) -> Vec<(String, String)> {
    ids.iter().map(|id| ((*id).to_owned(), t(&format!("{prefix}.{id}")))).collect()
}

#[component]
pub fn SetupPage() -> Element {
    let services = use_context::<Services>();
    let ui = use_context::<Ui>();
    let navigator = use_navigator();
    let mut step = use_signal(|| 0_usize);
    // the downloads step's unsaved edit, saved when leaving the step
    let draft = use_signal(|| None::<TorrentSettingsVm>);
    let mut save_error = use_signal(|| None::<UiError>);

    let finish = use_callback({
        let backend = services.backend.clone();
        move |_: ()| {
            let mut system = ui.settings.peek().system.clone();
            system.setup_done = true;
            let next = reduce_settings(&ui.settings.peek(), SettingsAction::SetSystem(system));
            ui.commit_settings(&backend, next);
            navigator.replace(Route::HomePage {});
        }
    });
    let flush_downloads = use_callback({
        let backend = services.backend.clone();
        move |_: ()| {
            let Some(s) = draft.peek().clone() else { return };
            let backend = backend.clone();
            let mut draft = draft;
            spawn(async move {
                match backend.save_torrent_settings(&s).await {
                    Ok(()) => draft.set(None),
                    Err(e) => save_error.set(Some(e)),
                }
            });
        }
    });
    let mut go = move |to: usize| {
        if STEPS[*step.peek()] == "downloads" {
            flush_downloads.call(());
        }
        step.set(to);
    };

    let i = step();
    let id = STEPS[i];
    let last = i == STEPS.len() - 1;
    let progress = tf("setup.progress", &[("n", &(i + 1).to_string()), ("total", &STEPS.len().to_string())]);
    let body = match id {
        "appearance" => rsx! {
            AppearanceStep {}
        },
        "player" => rsx! {
            PlayerStep {}
        },
        "downloads" => rsx! {
            DownloadsStep { draft }
        },
        "extensions" => rsx! {
            ExtensionsStep {}
        },
        "accounts" => rsx! {
            AccountsStep {}
        },
        "system" => rsx! {
            SystemStep {}
        },
        _ => rsx! {
            DoneStep {}
        },
    };

    rsx! {
        div { class: "tk-page tk-page--setup", "data-setup-step": "{id}",
            div { class: "tk-setup",
                header { class: "tk-setup__head",
                    h1 { class: "tk-page__title", {t("setup.title")} }
                    p { class: "tk-page__intro", {t("setup.intro")} }
                    ol { class: "tk-setup__steps", "aria-label": "{progress}",
                        for (n , s) in STEPS.iter().enumerate() {
                            li {
                                key: "{s}",
                                class: "tk-setup__dot",
                                "data-state": dot_state(n, i),
                                "aria-current": if n == i { "step" } else { "false" },
                                span { class: "tk-setup__dot-n", {(n + 1).to_string()} }
                                span { class: "tk-setup__dot-label", {t(&format!("setup.step.{s}"))} }
                            }
                        }
                    }
                }
                section { class: "tk-setup__card", "aria-label": t(&format!("setup.step.{id}")),
                    h2 { class: "tk-setup__title", {t(&format!("setup.step.{id}"))} }
                    p { class: "tk-setup__progress", "{progress}" }
                    {body}
                    if let Some(e) = save_error() {
                        ErrorState { error: e, compact: true, retry: false }
                    }
                }
                footer { class: "tk-setup__foot",
                    if i > 0 {
                        Button {
                            label: t("setup.back"),
                            icon: "chevron-left".to_string(),
                            variant: ButtonVariant::Text,
                            onclick: move |_| go(prev_step(i)),
                        }
                    }
                    span { class: "tk-setup__spacer" }
                    if !last {
                        Button { label: t("setup.skip"), variant: ButtonVariant::Text, onclick: move |_| finish.call(()) }
                        Button {
                            label: t("setup.next"),
                            trailing_icon: "chevron-right".to_string(),
                            onclick: move |_| go(next_step(i)),
                        }
                    } else {
                        Button { label: t("setup.finish"), icon: "play".to_string(), onclick: move |_| finish.call(()) }
                    }
                }
            }
        }
    }
}

/// Step 1: language and look.
#[component]
fn AppearanceStep() -> Element {
    let services = use_context::<Services>();
    let ui = use_context::<Ui>();
    let commit = use_callback({
        let backend = services.backend.clone();
        move |action: SettingsAction| {
            let next = reduce_settings(&ui.settings.peek(), action);
            ui.commit_settings(&backend, next);
        }
    });
    let s = (ui.settings)();
    let langs: Vec<&'static str> = LangChoice::ALL.iter().map(|l| l.id()).collect();
    let modes: Vec<&'static str> = ThemeMode::ALL.iter().map(|m| m.id()).collect();
    rsx! {
        p { class: "tk-setup__hint", {t("setup.appearance.hint")} }
        div { class: "tk-setup__field",
            span { class: "tk-setup__label", {t("settings.language")} }
            Segmented {
                options: options(&langs, "lang"),
                selected: s.language.id().to_owned(),
                label: t("settings.language"),
                on_select: move |id: String| {
                    if let Some(l) = LangChoice::ALL.into_iter().find(|l| l.id() == id) {
                        commit.call(SettingsAction::SetLanguage(l));
                    }
                },
            }
        }
        div { class: "tk-setup__field",
            span { class: "tk-setup__label", {t("settings.mode")} }
            Segmented {
                options: options(&modes, "mode"),
                selected: s.mode.id().to_owned(),
                label: t("settings.mode"),
                on_select: move |id: String| {
                    if let Some(m) = ThemeMode::ALL.into_iter().find(|m| m.id() == id) {
                        commit.call(SettingsAction::SetMode(m));
                    }
                },
            }
        }
        div { class: "tk-setup__field",
            span { class: "tk-setup__label", {t("settings.palette")} }
            div { class: "tk-swatches", role: "radiogroup", "aria-label": t("settings.palette"),
                for p in PALETTES {
                    {
                        let label = t(&format!("palette.{}", p.id));
                        let selected = s.palette == p.id;
                        rsx! {
                            button {
                                key: "{p.id}",
                                class: "tk-swatch",
                                r#type: "button",
                                role: "radio",
                                "aria-checked": "{selected}",
                                "aria-label": "{label}",
                                title: "{label}",
                                "data-selected": "{selected}",
                                onclick: move |_| commit.call(SettingsAction::SetPalette(p.id.to_owned())),
                                span { class: "tk-swatch__dot", style: "background-color:{p.seed}" }
                                span { class: "tk-swatch__name", "{label}" }
                            }
                        }
                    }
                }
            }
        }
        div { class: "tk-setup__switch",
            span { class: "tk-setup__label", {t("settings.reduce_motion")} }
            Switch {
                checked: s.reduce_motion,
                label: t("settings.reduce_motion"),
                on_toggle: move |v: bool| commit.call(SettingsAction::SetReduceMotion(v)),
            }
        }
    }
}

/// Step 2: player engine and source preferences. Every change is saved at once (like the Appearance step).
#[component]
fn PlayerStep() -> Element {
    let services = use_context::<Services>();
    let ui = use_context::<Ui>();
    let backend = services.backend.clone();
    let commit = use_callback({
        let backend = backend.clone();
        move |action: SettingsAction| {
            let next = reduce_settings(&ui.settings.peek(), action);
            ui.commit_settings(&backend, next);
        }
    });
    let build = use_resource({
        let backend = backend.clone();
        move || {
            let backend = backend.clone();
            async move { backend.anime_build_status().await }
        }
    });

    let s = (ui.settings)();
    let mode = s.player_mode;
    let available = match &*build.read() {
        Some(Ok(b)) => Some(b.available),
        _ => None,
    };
    let disabled_keys = engine_disabled_keys(available, mode);
    let modes: Vec<&'static str> = PlayerModeVm::ALL.iter().map(|m| m.id()).collect();
    let mode_hint = if mode == PlayerModeVm::EmbeddedEnhanced && available == Some(false) {
        t("settings.player.anime.missing_hint")
    } else {
        t(&format!("settings.player.mode.{}_hint", mode.id()))
    };
    let prefer_id = s.sources.prefer.map(|k| k.id()).unwrap_or("any").to_owned();
    let qualities: Vec<(String, String)> = QualityVm::OFFERED.iter().map(|q| (q.id().to_owned(), q.id().to_owned())).collect();

    rsx! {
        p { class: "tk-setup__hint", {t("setup.player.hint")} }
        div { class: "tk-setup__field", "data-player-mode": "{mode.id()}",
            span { class: "tk-setup__label", {t("settings.player.engine")} }
            Segmented {
                options: options(&modes, "settings.player.mode"),
                selected: mode.id().to_owned(),
                label: t("settings.player.engine"),
                disabled_keys,
                on_select: move |id: String| {
                    if let Some(m) = PlayerModeVm::ALL.into_iter().find(|m| m.id() == id) {
                        commit.call(SettingsAction::SetPlayerMode(m));
                    }
                },
            }
            span { class: "tk-setup__hint", "{mode_hint}" }
        }
        div { class: "tk-setup__field", "data-source-prefer": "{prefer_id}",
            span { class: "tk-setup__label", {t("src.prefer")} }
            Segmented {
                options: prefer_options(),
                selected: prefer_id.clone(),
                label: t("src.prefer"),
                small: true,
                on_select: move |id: String| {
                    // "any" (or anything unknown) clears the preference.
                    let kind = SourceKindVm::ALL.into_iter().find(|k| k.id() == id);
                    commit.call(SettingsAction::SetSourcePrefer(kind));
                },
            }
            span { class: "tk-setup__hint", {t("src.prefer.hint")} }
        }
        div { class: "tk-setup__field", "data-min-quality": "{s.sources.min_quality.id()}",
            span { class: "tk-setup__label", {t("src.quality")} }
            Segmented {
                options: qualities,
                selected: s.sources.min_quality.id().to_owned(),
                label: t("src.quality"),
                small: true,
                on_select: move |id: String| {
                    if let Some(q) = QualityVm::ALL.into_iter().find(|q| q.id() == id) {
                        commit.call(SettingsAction::SetMinQuality(q));
                    }
                },
            }
        }
        div { class: "tk-setup__field", "data-sub-lang": "{s.sources.sub_lang}",
            span { class: "tk-setup__label", {t("src.sub_lang")} }
            Segmented {
                options: lang_options(&SUB_LANGS),
                selected: s.sources.sub_lang.clone(),
                label: t("src.sub_lang"),
                small: true,
                on_select: move |id: String| commit.call(SettingsAction::SetSubLang(id)),
            }
        }
        div { class: "tk-setup__field", "data-dub-lang": "{s.sources.dub_lang}",
            span { class: "tk-setup__label", {t("src.dub_lang")} }
            Segmented {
                options: lang_options(&DUB_LANGS),
                selected: s.sources.dub_lang.clone(),
                label: t("src.dub_lang"),
                small: true,
                on_select: move |id: String| commit.call(SettingsAction::SetDubLang(id)),
            }
        }
        p { class: "tk-setup__hint", {t("setup.player.later")} }
    }
}

/// Step 3: download folder and what happens after watching. Edits go to `draft` (saved when leaving the step).
#[component]
fn DownloadsStep(draft: Signal<Option<TorrentSettingsVm>>) -> Element {
    let services = use_context::<Services>();
    let backend = services.backend.clone();
    let mut draft = draft;
    let mut loaded = use_resource({
        let backend = backend.clone();
        move || {
            let backend = backend.clone();
            async move { backend.torrent_settings().await }
        }
    });

    let current = match (draft(), &*loaded.read()) {
        (Some(d), _) => Ok(Some(d)),
        (None, Some(Ok(s))) => Ok(Some(s.clone())),
        (None, Some(Err(e))) => Err(e.clone()),
        (None, None) => Ok(None),
    };
    let body = match current {
        Err(e) => rsx! {
            ErrorState { error: e, compact: true, on_retry: move |_| loaded.restart() }
            p { class: "tk-setup__hint", {t("setup.optional_later")} }
        },
        Ok(None) => rsx! {
            SkeletonLines { widths: vec![80, 40] }
        },
        Ok(Some(s)) => {
            let shown = if s.download_dir.trim().is_empty() { s.default_download_dir.clone() } else { s.download_dir.clone() };
            let after: Vec<&'static str> = AfterWatchVm::ALL.iter().map(|a| a.id()).collect();
            let base = s.clone();
            let base2 = s.clone();
            rsx! {
                TextField {
                    label: t("setup.downloads.folder"),
                    value: shown,
                    icon: "folder".to_string(),
                    mono: true,
                    placeholder: s.default_download_dir.clone(),
                    hint: t("setup.downloads.folder_hint"),
                    oninput: move |v: String| {
                        let mut next = draft.peek().clone().unwrap_or_else(|| base.clone());
                        next.download_dir = v.trim().to_owned();
                        draft.set(Some(next));
                    },
                }
                div { class: "tk-setup__field",
                    span { class: "tk-setup__label", {t("torrent.settings.after_watch")} }
                    Segmented {
                        options: options(&after, "torrent.settings.after_watch"),
                        selected: s.after_watch.id().to_owned(),
                        label: t("torrent.settings.after_watch"),
                        on_select: move |id: String| {
                            if let Some(a) = AfterWatchVm::ALL.into_iter().find(|a| a.id() == id) {
                                let mut next = draft.peek().clone().unwrap_or_else(|| base2.clone());
                                next.after_watch = a;
                                draft.set(Some(next));
                            }
                        },
                    }
                    span { class: "tk-setup__hint", {t("torrent.settings.after_watch_hint")} }
                }
            }
        }
    };
    rsx! {
        p { class: "tk-setup__hint", {t("setup.downloads.hint")} }
        {body}
    }
}

/// Step 4: extensions.
#[component]
fn ExtensionsStep() -> Element {
    let services = use_context::<Services>();
    let backend = services.backend.clone();
    let snack = use_snackbar();
    let mut list = use_resource({
        let backend = backend.clone();
        move || {
            let backend = backend.clone();
            async move { backend.extensions().await }
        }
    });
    let mut busy = use_signal(|| false);
    let mut invalid = use_signal(|| false);
    let mut error = use_signal(|| None::<UiError>);

    let install = use_callback({
        let backend = backend.clone();
        move |path: String| {
            let path = path.trim().trim_matches('"').to_owned();
            if !is_kai_path(&path) {
                invalid.set(true);
                return;
            }
            invalid.set(false);
            error.set(None);
            busy.set(true);
            let backend = backend.clone();
            spawn(async move {
                match backend.install_extension(&path).await {
                    Ok(ext) => {
                        snack.show(tf("ext.install.ok", &[("name", &ext.name)]));
                        list.restart();
                    }
                    Err(e) => error.set(Some(e)),
                }
                busy.set(false);
            });
        }
    });

    let count = match &*list.read() {
        None => rsx! {
            SkeletonLines { widths: vec![40] }
        },
        Some(Err(e)) => rsx! {
            ErrorState { error: e.clone(), compact: true, on_retry: move |_| list.restart() }
        },
        Some(Ok(items)) => {
            let n = tn("setup.extensions.installed", items.len());
            let names: Vec<String> = items.iter().map(|e| e.name.clone()).collect();
            rsx! {
                p { class: "tk-setup__count", "data-extensions": "{items.len()}", "{n}" }
                if !names.is_empty() {
                    ul { class: "tk-setup__list",
                        for name in names {
                            li { key: "{name}", "{name}" }
                        }
                    }
                }
            }
        }
    };

    rsx! {
        p { class: "tk-setup__hint", {t("setup.extensions.hint")} }
        {count}
        div { class: "tk-panel__row",
            label { class: "tk-btn tk-btn--tonal tk-btn--sm", r#for: "{FILE_INPUT_ID}", "aria-disabled": "{busy()}",
                if busy() {
                    LoadingIndicator { small: true }
                } else {
                    Icon { name: "download".to_string(), size: 18 }
                }
                span { {t("setup.extensions.install")} }
            }
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
        if invalid() {
            p { class: "tk-setup__error", {t("ext.install.invalid")} }
        }
        if let Some(e) = error() {
            ErrorState { error: e, compact: true, retry: false }
        }
        p { class: "tk-setup__hint", {t("setup.extensions.later")} }
    }
}

/// Step 5: AniList / MAL (optional).
#[component]
fn AccountsStep() -> Element {
    let services = use_context::<Services>();
    let backend = services.backend.clone();
    let mut accounts = use_resource({
        let backend = backend.clone();
        move || {
            let backend = backend.clone();
            async move { backend.list_accounts().await }
        }
    });
    let mut busy = use_signal(|| None::<ListServiceVm>);
    let mut error = use_signal(|| None::<UiError>);

    let connect = use_callback({
        let backend = backend.clone();
        move |service: ListServiceVm| {
            if busy.peek().is_some() {
                return;
            }
            busy.set(Some(service));
            error.set(None);
            let backend = backend.clone();
            spawn(async move {
                if let Err(e) = backend.list_connect(service).await {
                    error.set(Some(e));
                }
                accounts.restart();
                busy.set(None);
            });
        }
    });

    let rows = match &*accounts.read() {
        None => rsx! {
            SkeletonLines { widths: vec![60, 60] }
        },
        Some(Err(e)) => rsx! {
            ErrorState { error: e.clone(), compact: true, on_retry: move |_| accounts.restart() }
        },
        Some(Ok(list)) => {
            let items: Vec<(ListServiceVm, bool, Option<String>)> = ListServiceVm::ALL
                .iter()
                .map(|svc| {
                    let acc = list.iter().find(|a| a.service == *svc);
                    (*svc, acc.is_some_and(|a| a.connected), acc.and_then(|a| a.username.clone()))
                })
                .collect();
            rsx! {
                for (svc , connected , user) in items {
                    div {
                        key: "{svc.id()}",
                        class: "tk-setup__account",
                        "data-service": "{svc.id()}",
                        "data-connected": "{connected}",
                        b { {svc.label()} }
                        if connected {
                            Pill { text: user.clone().map(|u| tf("setup.accounts.as", &[("user", &u)])).unwrap_or_else(|| t("setup.accounts.connected")), tone: Tone::Ok }
                        } else {
                            Button {
                                label: t("setup.accounts.connect"),
                                icon: "link".to_string(),
                                variant: ButtonVariant::Tonal,
                                size: ButtonSize::Sm,
                                disabled: busy().is_some(),
                                onclick: move |_| connect.call(svc),
                            }
                        }
                    }
                }
            }
        }
    };

    rsx! {
        p { class: "tk-setup__hint", {t("setup.accounts.hint")} }
        {rows}
        if let Some(e) = error() {
            ErrorState { error: e, compact: true, retry: false }
        }
    }
}

/// Step 6: OS integration.
#[component]
fn SystemStep() -> Element {
    let services = use_context::<Services>();
    let backend = services.backend.clone();
    let ui = use_context::<Ui>();
    let caps = backend.system_caps();
    let s = (ui.settings)();
    let mut error = use_signal(|| None::<UiError>);
    let mut magnet = use_signal(|| None::<bool>);
    let mut autostart = use_signal(|| s.system.start_with_system);

    let commit = use_callback({
        let backend = backend.clone();
        move |action: SettingsAction| {
            let next = reduce_settings(&ui.settings.peek(), action);
            ui.commit_settings(&backend, next);
        }
    });
    let set_autostart = {
        let backend = backend.clone();
        move |on: bool| {
            error.set(None);
            let backend = backend.clone();
            spawn(async move {
                match backend.set_autostart(on).await {
                    Ok(()) => {
                        autostart.set(on);
                        let mut system = ui.settings.peek().system.clone();
                        system.start_with_system = on;
                        commit.call(SettingsAction::SetSystem(system));
                    }
                    Err(e) => error.set(Some(e)),
                }
            });
        }
    };
    let set_magnet = {
        let backend = backend.clone();
        move |on: bool| {
            error.set(None);
            let backend = backend.clone();
            spawn(async move {
                match backend.set_magnet_handler(on).await {
                    Ok(()) => magnet.set(Some(on)),
                    Err(e) => error.set(Some(e)),
                }
            });
        }
    };
    // real OS state of the magnet handler, once
    use_future({
        let backend = backend.clone();
        move || {
            let backend = backend.clone();
            async move {
                if backend.system_caps().magnet_handler
                    && let Ok(on) = backend.magnet_handler_registered().await
                {
                    magnet.set(Some(on));
                }
            }
        }
    });

    let sys = s.system.clone();
    let sys_tray = sys.clone();
    let sys_upd = sys.clone();
    let flow = s.flow.clone();
    let (tray_on, updates_on, discord_on) = (sys.tray_icon, sys.check_updates, flow.discord_presence);
    rsx! {
        p { class: "tk-setup__hint", {t("setup.system.hint")} }
        if caps.autostart {
            div { class: "tk-setup__switch", "data-autostart": "{autostart()}",
                span { class: "tk-setup__label", {t("settings.system.autostart")} }
                Switch { checked: autostart(), label: t("settings.system.autostart"), on_toggle: set_autostart }
            }
        }
        if caps.tray {
            div { class: "tk-setup__switch",
                span { class: "tk-setup__label", {t("settings.system.tray")} }
                Switch {
                    checked: tray_on,
                    label: t("settings.system.tray"),
                    on_toggle: move |v: bool| {
                        let mut system = sys_tray.clone();
                        system.tray_icon = v;
                        commit.call(SettingsAction::SetSystem(system));
                    },
                }
            }
        }
        div { class: "tk-setup__switch",
            span { class: "tk-setup__label", {t("setup.system.discord")} }
            Switch {
                checked: discord_on,
                label: t("setup.system.discord"),
                on_toggle: move |v: bool| {
                    let mut f = flow.clone();
                    f.discord_presence = v;
                    commit.call(SettingsAction::SetFlow(f));
                },
            }
        }
        div { class: "tk-setup__switch",
            span { class: "tk-setup__label", {t("settings.system.check_updates")} }
            Switch {
                checked: updates_on,
                label: t("settings.system.check_updates"),
                on_toggle: move |v: bool| {
                    let mut system = sys_upd.clone();
                    system.check_updates = v;
                    commit.call(SettingsAction::SetSystem(system));
                },
            }
        }
        if caps.magnet_handler {
            div { class: "tk-setup__switch", "data-magnet-handler": "{magnet().unwrap_or(false)}",
                span { class: "tk-setup__label", {t("settings.system.magnet_handler")} }
                Switch {
                    checked: magnet().unwrap_or(false),
                    label: t("settings.system.magnet_handler"),
                    disabled: magnet().is_none(),
                    on_toggle: set_magnet,
                }
            }
        }
        if let Some(e) = error() {
            ErrorState { error: e, compact: true, retry: false }
        }
    }
}

/// Step 7: ready.
#[component]
fn DoneStep() -> Element {
    rsx! {
        div { class: "tk-setup__done",
            Icon { name: "check".to_string(), size: 40 }
            p { {t("setup.done.body")} }
            p { class: "tk-setup__hint", {t("setup.done.later")} }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steps_follow_the_acceptance_order_and_stay_in_bounds() {
        assert_eq!(STEPS, ["appearance", "player", "downloads", "extensions", "accounts", "system", "done"]);
        let mut i = 0;
        for expected in &STEPS[1..] {
            i = next_step(i);
            assert_eq!(STEPS[i], *expected);
        }
        assert_eq!(next_step(i), i, "Next on the last step stays there");
        assert_eq!(prev_step(0), 0);
        assert_eq!(prev_step(3), 2);
        assert_eq!([dot_state(0, 1), dot_state(1, 1), dot_state(2, 1)], ["done", "current", "todo"]);
    }

    #[test]
    fn player_step_comes_right_after_appearance() {
        // checklist account.setup-wizard: language, then engine and source preferences, then accounts / system.
        let at = |id: &str| STEPS.iter().position(|s| *s == id).unwrap();
        assert_eq!(at("player"), at("appearance") + 1);
        assert!(at("player") < at("accounts") && at("accounts") < at("system"));
    }

    #[test]
    fn enhanced_engine_is_disabled_only_when_known_missing_and_not_chosen() {
        let enhanced = vec![PlayerModeVm::EmbeddedEnhanced.id().to_owned()];
        assert_eq!(engine_disabled_keys(Some(false), PlayerModeVm::Embedded), enhanced);
        assert_eq!(engine_disabled_keys(Some(false), PlayerModeVm::External), enhanced);
        assert!(engine_disabled_keys(Some(false), PlayerModeVm::EmbeddedEnhanced).is_empty());
        assert!(engine_disabled_keys(Some(true), PlayerModeVm::Embedded).is_empty());
        assert!(engine_disabled_keys(None, PlayerModeVm::Embedded).is_empty(), "loading or failed: nothing disabled");
    }

    #[test]
    fn source_option_lists_cover_every_choice() {
        let prefer: Vec<String> = prefer_options().into_iter().map(|(id, _)| id).collect();
        assert_eq!(prefer[0], "any");
        assert_eq!(prefer.len(), SourceKindVm::ALL.len() + 1);
        assert!(SourceKindVm::ALL.iter().all(|k| prefer.iter().any(|p| p == k.id())));
        assert_eq!(lang_options(&DUB_LANGS)[0], ("ja".to_owned(), "JA".to_owned()));
        assert_eq!(lang_options(&SUB_LANGS).len(), SUB_LANGS.len());
    }
}
