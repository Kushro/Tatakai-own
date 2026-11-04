//! Settings: sticky side index (scroll spy) + sections Appearance, Player (incl. source selection),
//! Metadata, Account, Link (port), Data & storage and About. Prototype: `screens.js::S.settings`.
//!
//! The page owns its scroll container (`.tk-settings-scroll`, absolutely filling `.tk-main`) so its
//! `onscroll` fires; the active index entry is the last section whose top passed 25% of the viewport.

use std::rc::Rc;

use dioxus::prelude::*;
use tk_player::ShaderPreset;

use crate::app::{Route, Services, Ui};
use crate::backend::{AccountVm, AnimeBuildVm, SourceKindVm};
use crate::components::title_bar::ShellStatus;
use crate::components::{
    Button, ButtonSize, ButtonVariant, ErrorState, IconButton, IconButtonSize, LoadingIndicator, Pill, Segmented,
    SkeletonLines, Switch, TextField, Tone, use_snackbar,
};
use crate::i18n::{LangChoice, t, tf};
use crate::icons::Icon;
use crate::model::{
    Contrast, DUB_LANGS, EnhancedUiVm, ErrorKind, PALETTES, PlatformVm, PlayerModeVm, QualityVm, SUB_LANGS,
    SettingsAction, SourceModeVm, ThemeMode, UiError, reduce_settings,
};

/// `(section id, icon)`: anchors `set-<id>`, titles `settings.<id>`.
const SECTIONS: [(&str, &str); 7] = [
    ("appearance", "palette"),
    ("player", "play"),
    ("metadata", "sync"),
    ("account", "user"),
    ("link", "link"),
    ("data", "storage"),
    ("about", "info"),
];

/// Lowest port the Link field accepts (no privileged ports).
const MIN_PORT: u16 = 1024;

fn seg<I: Copy>(items: &[I], id: impl Fn(I) -> &'static str, key: &str) -> Vec<(String, String)> {
    items.iter().map(|i| (id(*i).to_owned(), t(&format!("{key}.{}", id(*i))))).collect()
}

/// Language options shown as upper-case codes (`ES`, `JA`), like the prototype.
fn lang_seg(langs: &[&str]) -> Vec<(String, String)> {
    langs.iter().map(|l| ((*l).to_owned(), l.to_uppercase())).collect()
}

/// Two-letter monogram of a metadata provider (decorative).
fn provider_mono(p: &str) -> String {
    match p {
        "anilist" => "AL".to_owned(),
        "kitsu" => "Ki".to_owned(),
        "anizip" => "az".to_owned(),
        other => other.chars().take(2).collect(),
    }
}

/// Parse the Link port field: digits only, `MIN_PORT..=65535`.
pub fn parse_port(text: &str) -> Option<u16> {
    text.trim().parse::<u16>().ok().filter(|p| *p >= MIN_PORT)
}

/// `key_at` with the real data directory (`Backend::data_dir_display`), or the older `key` text when the
/// backend does not know it (docs/redesign/09 section 4.2).
fn with_data_dir(dir: &str, key_at: &str, key: &str) -> String {
    if dir.is_empty() { t(key) } else { tf(key_at, &[("dir", dir)]) }
}

/// Example mpv location for the external-mpv path field, in the OS's own form.
fn mpv_placeholder(os: PlatformVm) -> &'static str {
    match os {
        PlatformVm::Linux => "/usr/bin/mpv",
        PlatformVm::Windows => "C:\\Program Files\\mpv\\mpv.exe",
        PlatformVm::Mac => "/opt/homebrew/bin/mpv",
    }
}

#[component]
pub fn SettingsPage() -> Element {
    let services = use_context::<Services>();
    let ui = use_context::<Ui>();
    let navigator = use_navigator();
    let snack = use_snackbar();
    let backend = services.backend.clone();

    let commit = use_callback({
        let backend = backend.clone();
        move |action: SettingsAction| {
            let next = reduce_settings(&ui.settings.peek(), action);
            ui.commit_settings(&backend, next);
        }
    });
    // Changes whose effect is not visible at once also confirm with a snackbar (prototype `apply`).
    let commit_saved = use_callback(move |action: SettingsAction| {
        commit.call(action);
        snack.show(t("settings.saved"));
    });

    // --- side index + scroll spy ---
    let mut active = use_signal(|| 0_usize);
    let panels = use_signal(|| vec![None::<Rc<MountedData>>; SECTIONS.len()]);
    let mut scroller = use_signal(|| None::<Rc<MountedData>>);
    let mut spy_busy = use_signal(|| false);
    let mut spy_dirty = use_signal(|| false);

    // Measures the panels against the scroll container; coalesces scroll events while a pass runs.
    let spy = use_callback(move |at_bottom: bool| {
        if at_bottom {
            if *active.peek() != SECTIONS.len() - 1 {
                active.set(SECTIONS.len() - 1);
            }
            return;
        }
        if *spy_busy.peek() {
            spy_dirty.set(true);
            return;
        }
        spy_busy.set(true);
        spawn(async move {
            loop {
                spy_dirty.set(false);
                let container = scroller.peek().clone();
                let list = panels.peek().clone();
                if let Some(c) = container
                    && let Ok(cr) = c.get_client_rect().await
                {
                    let line = cr.origin.y + cr.size.height * 0.25;
                    let mut current = 0;
                    for (i, p) in list.iter().enumerate() {
                        if let Some(p) = p
                            && let Ok(r) = p.get_client_rect().await
                            && r.origin.y <= line
                        {
                            current = i;
                        }
                    }
                    if *active.peek() != current {
                        active.set(current);
                    }
                }
                if !*spy_dirty.peek() {
                    break;
                }
            }
            spy_busy.set(false);
        });
    });

    let s = (ui.settings)();
    let reduce = s.reduce_motion;
    let prefer_id = s.sources.prefer.map(|k| k.id()).unwrap_or("any").to_owned();
    let mut prefer_options = vec![("any".to_owned(), t("src.prefer.any"))];
    prefer_options.extend(SourceKindVm::ALL.iter().map(|k| (k.id().to_owned(), t(&format!("src.{}", k.id())))));
    let version = format!("{} · tk-app (Dioxus 0.7)", env!("CARGO_PKG_VERSION"));
    let data_hint = with_data_dir(&backend.data_dir_display(), "settings.data.hint_at", "settings.data.hint");

    rsx! {
        div {
            class: "tk-settings-scroll",
            onmounted: move |e| scroller.set(Some(e.data())),
            onscroll: move |e: Event<ScrollData>| {
                let bottom = e.scroll_top() + f64::from(e.client_height()) >= f64::from(e.scroll_height()) - 4.0;
                spy.call(bottom);
            },
            div { class: "tk-page tk-page--settings",
                div { class: "tk-page__head",
                    h1 { class: "tk-page__title", {t("settings.title")} }
                }
                div { class: "tk-settings",
                    nav { class: "tk-settings__nav", "aria-label": t("settings.title"),
                        for (i , (key , icon)) in SECTIONS.iter().enumerate() {
                            button {
                                key: "{key}",
                                class: "tk-settings__link",
                                r#type: "button",
                                "aria-current": if active() == i { "true" } else { "false" },
                                onclick: move |_| {
                                    active.set(i);
                                    let behavior = if reduce { ScrollBehavior::Instant } else { ScrollBehavior::Smooth };
                                    if let Some(p) = panels.peek()[i].clone() {
                                        spawn(async move {
                                            let _ = p.scroll_to(behavior).await;
                                        });
                                    }
                                },
                                Icon { name: icon.to_string(), size: 20 }
                                span { {t(&format!("settings.{key}"))} }
                            }
                        }
                    }

                    div { class: "tk-settings__body",
                        SettingsPanel { index: 0, panels,
                            SettingRow { label: t("settings.palette"), hint: t("settings.palette_hint"), stack: true,
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
                            SettingRow { label: t("settings.mode"),
                                Segmented {
                                    options: seg(&ThemeMode::ALL, ThemeMode::id, "mode"),
                                    selected: s.mode.id().to_owned(),
                                    label: t("settings.mode"),
                                    on_select: move |id: String| {
                                        if let Some(m) = ThemeMode::ALL.into_iter().find(|m| m.id() == id) {
                                            commit.call(SettingsAction::SetMode(m));
                                        }
                                    },
                                }
                            }
                            SettingRow { label: t("settings.contrast"),
                                Segmented {
                                    options: seg(&Contrast::ALL, Contrast::id, "contrast"),
                                    selected: s.contrast.id().to_owned(),
                                    label: t("settings.contrast"),
                                    on_select: move |id: String| {
                                        if let Some(c) = Contrast::ALL.into_iter().find(|c| c.id() == id) {
                                            commit.call(SettingsAction::SetContrast(c));
                                        }
                                    },
                                }
                            }
                            SettingRow { label: t("settings.language"),
                                Segmented {
                                    options: seg(&LangChoice::ALL, LangChoice::id, "lang"),
                                    selected: s.language.id().to_owned(),
                                    label: t("settings.language"),
                                    on_select: move |id: String| {
                                        if let Some(l) = LangChoice::ALL.into_iter().find(|l| l.id() == id) {
                                            commit.call(SettingsAction::SetLanguage(l));
                                        }
                                    },
                                }
                            }
                            SettingRow { label: t("settings.reduce_motion"), hint: t("settings.reduce_motion_hint"),
                                Switch {
                                    checked: s.reduce_motion,
                                    label: t("settings.reduce_motion"),
                                    on_toggle: move |v: bool| commit_saved.call(SettingsAction::SetReduceMotion(v)),
                                }
                            }
                        }

                        SettingsPanel { index: 1, panels,
                            PlayerEngineRows { on_action: commit_saved }
                            SettingRow { label: t("settings.player.preset"), hint: t("settings.player.preset_hint_embedded"), stack: true,
                                Segmented {
                                    options: ShaderPreset::ALL
                                        .iter()
                                        .map(|p| (p.id().to_owned(), t(&format!("preset.{}", p.id()))))
                                        .collect::<Vec<_>>(),
                                    selected: s.shader_preset.clone(),
                                    label: t("settings.player.preset"),
                                    small: true,
                                    on_select: move |id: String| {
                                        if let Some(p) = ShaderPreset::from_id(&id) {
                                            commit_saved.call(SettingsAction::SetPreset(p));
                                        }
                                    },
                                }
                            }
                            SettingRow { label: t("src.mode"), hint: t("src.mode.hint"),
                                Segmented {
                                    options: seg(&SourceModeVm::ALL, SourceModeVm::id, "src.mode"),
                                    selected: s.sources.mode.id().to_owned(),
                                    label: t("src.mode"),
                                    on_select: move |id: String| {
                                        if let Some(m) = SourceModeVm::ALL.into_iter().find(|m| m.id() == id) {
                                            commit_saved.call(SettingsAction::SetSourceMode(m));
                                        }
                                    },
                                }
                            }
                            SettingRow { label: t("src.prefer"), hint: t("src.prefer.hint"),
                                Segmented {
                                    options: prefer_options,
                                    selected: prefer_id,
                                    label: t("src.prefer"),
                                    small: true,
                                    on_select: move |id: String| {
                                        // "any" (or anything unknown) clears the preference.
                                        let kind = SourceKindVm::ALL.into_iter().find(|k| k.id() == id);
                                        commit_saved.call(SettingsAction::SetSourcePrefer(kind));
                                    },
                                }
                            }
                            SettingRow { label: t("src.quality"),
                                Segmented {
                                    options: QualityVm::OFFERED.iter().map(|q| (q.id().to_owned(), q.id().to_owned())).collect::<Vec<_>>(),
                                    selected: s.sources.min_quality.id().to_owned(),
                                    label: t("src.quality"),
                                    small: true,
                                    on_select: move |id: String| {
                                        if let Some(q) = QualityVm::ALL.into_iter().find(|q| q.id() == id) {
                                            commit_saved.call(SettingsAction::SetMinQuality(q));
                                        }
                                    },
                                }
                            }
                            SettingRow { label: t("src.sub_lang"),
                                Segmented {
                                    options: lang_seg(&SUB_LANGS),
                                    selected: s.sources.sub_lang.clone(),
                                    label: t("src.sub_lang"),
                                    small: true,
                                    on_select: move |id: String| commit_saved.call(SettingsAction::SetSubLang(id)),
                                }
                            }
                            SettingRow { label: t("src.dub_lang"),
                                Segmented {
                                    options: lang_seg(&DUB_LANGS),
                                    selected: s.sources.dub_lang.clone(),
                                    label: t("src.dub_lang"),
                                    small: true,
                                    on_select: move |id: String| commit_saved.call(SettingsAction::SetDubLang(id)),
                                }
                            }
                        }

                        SettingsPanel { index: 2, panels,
                            p { class: "tk-panel__hint", {t("settings.metadata.hint")} }
                            ol { class: "tk-priority",
                                for (i , provider) in s.provider_priority.iter().enumerate() {
                                    {
                                        let name = t(&format!("provider.{provider}"));
                                        let sub = t(&format!("provider.{provider}.sub"));
                                        let mono = provider_mono(provider);
                                        let last = i + 1 == s.provider_priority.len();
                                        rsx! {
                                            li { key: "{provider}", class: "tk-priority__row",
                                                span { class: "tk-priority__rank", "{i + 1}" }
                                                span { class: "tk-priority__ico", "aria-hidden": "true", "{mono}" }
                                                span { class: "tk-priority__name",
                                                    "{name}"
                                                    small { "{sub}" }
                                                }
                                                IconButton {
                                                    icon: "chevron-up".to_string(),
                                                    label: tf("settings.metadata.up", &[("name", &name)]),
                                                    size: IconButtonSize::Sm,
                                                    disabled: i == 0,
                                                    onclick: move |_| commit_saved.call(SettingsAction::MoveProvider { index: i, delta: -1 }),
                                                }
                                                IconButton {
                                                    icon: "chevron-down".to_string(),
                                                    label: tf("settings.metadata.down", &[("name", &name)]),
                                                    size: IconButtonSize::Sm,
                                                    disabled: last,
                                                    onclick: move |_| commit_saved.call(SettingsAction::MoveProvider { index: i, delta: 1 }),
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                            SettingRow { label: t("settings.metadata.cache"), hint: t("settings.metadata.cache_hint") }
                        }

                        SettingsPanel { index: 3, panels, AccountSection {} }

                        SettingsPanel { index: 4, panels,
                            p { class: "tk-panel__hint", {t("settings.link.hint")} }
                            LinkPortRow {}
                            div {
                                Button {
                                    label: t("settings.link.manage"),
                                    variant: ButtonVariant::Tonal,
                                    icon: "link".to_string(),
                                    onclick: move |_| {
                                        navigator.push(Route::LinkPage {});
                                    },
                                }
                            }
                        }

                        SettingsPanel { index: 5, panels,
                            p { class: "tk-panel__hint", "{data_hint}" }
                            SettingRow { label: t("settings.data.telemetry"), hint: t("settings.data.telemetry_hint"),
                                Pill { text: t("settings.data.telemetry_none"), tone: Tone::Ok }
                            }
                            // No backend calls exist for these yet (docs/redesign/08): shown disabled, not faked.
                            div { class: "tk-panel__row",
                                Button {
                                    label: t("settings.data.clear_cache"),
                                    icon: "trash".to_string(),
                                    variant: ButtonVariant::Outlined,
                                    size: ButtonSize::Sm,
                                    disabled: true,
                                }
                                Button {
                                    label: t("settings.data.open_folder"),
                                    icon: "folder".to_string(),
                                    variant: ButtonVariant::Text,
                                    size: ButtonSize::Sm,
                                    disabled: true,
                                }
                                Button {
                                    label: t("settings.data.export"),
                                    icon: "download".to_string(),
                                    variant: ButtonVariant::Text,
                                    size: ButtonSize::Sm,
                                    disabled: true,
                                }
                            }
                        }

                        SettingsPanel { index: 6, panels,
                            div { class: "tk-about",
                                p { {t("settings.about.body")} }
                                dl {
                                    dt { "Tatakai Next" }
                                    dd { "{version}" }
                                    dt { "Tatakai-Link" }
                                    dd { "v1 · openapi.v1.json" }
                                    dt { {t("settings.about.license")} }
                                    dd { "AGPL-3.0-or-later" }
                                }
                            }
                            div { class: "tk-panel__row",
                                Button {
                                    label: t("settings.about.check_updates"),
                                    icon: "update".to_string(),
                                    variant: ButtonVariant::Tonal,
                                    size: ButtonSize::Sm,
                                    disabled: true,
                                }
                                Button {
                                    label: t("settings.about.licenses"),
                                    icon: "external".to_string(),
                                    variant: ButtonVariant::Text,
                                    size: ButtonSize::Sm,
                                    onclick: move |_| snack.show("THIRD_PARTY_NOTICES.md"),
                                }
                                Button {
                                    label: t("settings.about.logs"),
                                    icon: "terminal".to_string(),
                                    variant: ButtonVariant::Text,
                                    size: ButtonSize::Sm,
                                    disabled: true,
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// One settings section: anchor `set-<id>`, icon + title, and registers its element for the index.
#[component]
fn SettingsPanel(index: usize, panels: Signal<Vec<Option<Rc<MountedData>>>>, children: Element) -> Element {
    let mut panels = panels;
    let (sid, icon) = SECTIONS[index];
    rsx! {
        section {
            class: "tk-panel tk-settings__panel",
            id: "set-{sid}",
            onmounted: move |e| {
                if let Some(slot) = panels.write().get_mut(index) {
                    *slot = Some(e.data());
                }
            },
            h2 { class: "tk-panel__title",
                Icon { name: icon.to_string(), size: 24 }
                {t(&format!("settings.{sid}"))}
            }
            {children}
        }
    }
}

/// Label + hint on the left, control on the right (`stack` puts the control below).
#[component]
fn SettingRow(label: String, #[props(default)] hint: String, #[props(default)] stack: bool, children: Element) -> Element {
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

/// Link server port: shows the current port; Enter / Save restarts the server there
/// (`Backend::link_restart`). A taken port is reported inline with the readable cause.
#[component]
fn LinkPortRow() -> Element {
    let services = use_context::<Services>();
    let backend = services.backend.clone();
    let shell = try_use_context::<ShellStatus>();
    let snack = use_snackbar();
    let mut status = use_resource({
        let backend = backend.clone();
        move || {
            let backend = backend.clone();
            async move { backend.link_status().await }
        }
    });
    let mut draft = use_signal(|| None::<String>);
    let mut busy = use_signal(|| false);
    let mut error = use_signal(|| None::<UiError>);

    let apply = use_callback({
        let backend = backend.clone();
        move |_: ()| {
            if busy() {
                return;
            }
            let Some(port) = draft().as_deref().and_then(parse_port) else { return };
            busy.set(true);
            error.set(None);
            let backend = backend.clone();
            spawn(async move {
                match backend.link_restart(port).await {
                    Ok(s) => {
                        if let Some(mut sh) = shell {
                            sh.link.set(Some(s.clone()));
                        }
                        if s.running {
                            snack.show(tf("link.status.running", &[("port", &s.port.to_string())]));
                            draft.set(None);
                        } else if let Some(e) = s.error {
                            error.set(Some(e));
                        }
                        status.restart();
                    }
                    Err(e) => error.set(Some(e)),
                }
                busy.set(false);
            });
        }
    });

    let current_port = status.read().as_ref().and_then(|r| r.as_ref().ok()).map(|s| s.port);
    let text = draft().unwrap_or_else(|| current_port.map(|p| p.to_string()).unwrap_or_default());
    let invalid = draft().is_some_and(|d| parse_port(&d).is_none());
    let err = error();
    let cause = match &err {
        Some(e) if e.kind == ErrorKind::AddrInUse => {
            format!("{} {}", tf("link.err.port", &[("port", &text)]), t("link.err.port_fix"))
        }
        _ => String::new(),
    };
    let load_error = status.read().as_ref().and_then(|r| r.as_ref().err()).cloned();

    if status.read().is_none() {
        return rsx! {
            SkeletonLines { widths: vec![30] }
        };
    }
    if let Some(e) = load_error {
        return rsx! {
            ErrorState { error: e, compact: true, on_retry: move |_| status.restart() }
        };
    }

    rsx! {
        div { class: "tk-settings__port",
            TextField {
                label: t("settings.link.port"),
                value: text,
                mono: true,
                error: invalid,
                hint: if invalid { t("settings.link.port_invalid") } else { t("settings.link.port_hint") },
                disabled: busy(),
                oninput: move |v: String| draft.set(Some(v)),
                onsubmit: move |_| apply.call(()),
            }
            Button {
                label: t("common.save"),
                variant: ButtonVariant::Tonal,
                size: ButtonSize::Sm,
                disabled: busy() || invalid || draft().is_none(),
                onclick: move |_| apply.call(()),
            }
        }
        if let Some(e) = err {
            ErrorState { error: e, compact: true, retry: false, cause }
        }
    }
}

/// Settings > Player > Engine (docs/redesign/09 section 4.3): Embedded / Embedded enhanced / External mpv with
/// a line per mode, then the enhanced rows (who draws the controls + mpv-anime-build status and Prepare) or
/// the external rows (mpv path + detection). Enhanced is disabled when this installation lacks the release.
#[component]
fn PlayerEngineRows(on_action: EventHandler<SettingsAction>) -> Element {
    let services = use_context::<Services>();
    let ui = use_context::<Ui>();
    let snack = use_snackbar();
    let backend = services.backend.clone();
    let mut build = use_resource({
        let backend = backend.clone();
        move || {
            let backend = backend.clone();
            async move { backend.anime_build_status().await }
        }
    });
    let mut preparing = use_signal(|| false);
    let mut prepare_error = use_signal(|| None::<UiError>);

    let prepare = move |_| {
        if preparing() {
            return;
        }
        preparing.set(true);
        prepare_error.set(None);
        let backend = backend.clone();
        spawn(async move {
            match backend.prepare_anime_build().await {
                Ok(_) => {
                    snack.show(t("settings.player.anime.prepared"));
                    build.restart();
                }
                Err(e) => prepare_error.set(Some(e)),
            }
            preparing.set(false);
        });
    };

    let (mode, enhanced_ui) = {
        let s = ui.settings.read();
        (s.player_mode, s.enhanced_ui)
    };
    let build_vm: Option<Result<AnimeBuildVm, UiError>> = build.read().clone();
    let unavailable = matches!(&build_vm, Some(Ok(b)) if !b.available);
    // A saved Enhanced choice stays selectable so the status row can explain why it cannot run.
    let disabled_keys = if unavailable && mode != PlayerModeVm::EmbeddedEnhanced {
        vec![PlayerModeVm::EmbeddedEnhanced.id().to_owned()]
    } else {
        Vec::new()
    };

    let build_block = match build_vm {
        None => rsx! {
            SkeletonLines { widths: vec![40] }
        },
        Some(Err(e)) => rsx! {
            ErrorState { error: e, compact: true, on_retry: move |_| build.restart() }
        },
        Some(Ok(b)) if !b.available => rsx! {
            div { class: "tk-engine-status",
                Pill { text: t("settings.player.anime.missing"), tone: Tone::Err }
                span { class: "tk-engine-status__note", {t("settings.player.anime.missing_hint")} }
            }
        },
        Some(Ok(b)) if b.extracted => {
            let label = tf("settings.player.anime.ready", &[("version", &b.version)]);
            let path = b.path.unwrap_or_default();
            rsx! {
                div { class: "tk-engine-status",
                    Pill { text: label, tone: Tone::Ok }
                    if !path.is_empty() {
                        code { class: "tk-engine-status__path", "{path}" }
                    }
                }
            }
        }
        Some(Ok(b)) => {
            let label = tf("settings.player.anime.not_prepared", &[("version", &b.version)]);
            rsx! {
                div { class: "tk-engine-status",
                    Pill { text: label, tone: Tone::Warn }
                    Button {
                        label: if preparing() { t("settings.player.anime.preparing") } else { t("settings.player.anime.prepare") },
                        icon: "download".to_string(),
                        variant: ButtonVariant::Tonal,
                        size: ButtonSize::Sm,
                        disabled: preparing(),
                        onclick: prepare,
                    }
                }
            }
        }
    };

    rsx! {
        SettingRow { label: t("settings.player.engine"), hint: t("settings.player.engine_hint"), stack: true,
            Segmented {
                options: seg(&PlayerModeVm::ALL, PlayerModeVm::id, "settings.player.mode"),
                selected: mode.id().to_owned(),
                label: t("settings.player.engine"),
                disabled_keys,
                on_select: move |id: String| {
                    if let Some(m) = PlayerModeVm::ALL.into_iter().find(|m| m.id() == id) {
                        on_action.call(SettingsAction::SetPlayerMode(m));
                    }
                },
            }
            ul { class: "tk-engine-modes",
                for m in PlayerModeVm::ALL {
                    {
                        let id = m.id();
                        let name = t(&format!("settings.player.mode.{id}"));
                        let hint = if m == PlayerModeVm::EmbeddedEnhanced && unavailable {
                            t("settings.player.anime.missing_hint")
                        } else {
                            t(&format!("settings.player.mode.{id}_hint"))
                        };
                        let selected = m == mode;
                        rsx! {
                            li { key: "{id}", "data-selected": "{selected}",
                                b { "{name}" }
                                span { "{hint}" }
                            }
                        }
                    }
                }
            }
        }
        if mode == PlayerModeVm::EmbeddedEnhanced {
            SettingRow {
                label: t("settings.player.enhanced_ui"),
                hint: t(&format!("settings.player.enhanced_ui.{}_hint", enhanced_ui.id())),
                Segmented {
                    options: seg(&EnhancedUiVm::ALL, EnhancedUiVm::id, "settings.player.enhanced_ui"),
                    selected: enhanced_ui.id().to_owned(),
                    label: t("settings.player.enhanced_ui"),
                    small: true,
                    on_select: move |id: String| {
                        if let Some(u) = EnhancedUiVm::ALL.into_iter().find(|u| u.id() == id) {
                            on_action.call(SettingsAction::SetEnhancedUi(u));
                        }
                    },
                }
            }
            SettingRow { label: t("settings.player.anime"), hint: t("settings.player.anime_hint"), stack: true,
                {build_block}
                if let Some(e) = prepare_error() {
                    ErrorState { error: e, compact: true, retry: false }
                }
            }
        }
        if mode == PlayerModeVm::External {
            ExternalMpvRows { on_action }
        }
    }
}

/// External mpv: path field (empty = autodetect) and the detection status (`tk_player::find_mpv` +
/// `mpv_version`, off the UI thread), re-run whenever the saved path changes.
#[component]
fn ExternalMpvRows(on_action: EventHandler<SettingsAction>) -> Element {
    let ui = use_context::<Ui>();
    let os = PlatformVm::current();
    let mut draft = use_signal(|| None::<String>);
    let mut detect = use_resource(move || {
        let custom = ui.settings.read().external_mpv_path.clone();
        async move {
            tokio::task::spawn_blocking(move || {
                let found = tk_player::find_mpv(custom.as_deref().map(std::path::Path::new))?;
                let version = tk_player::mpv_version(&found);
                Some((found.display().to_string(), version))
            })
            .await
            .ok()
            .flatten()
        }
    });

    let saved = ui.settings.read().external_mpv_path.clone();
    let save = use_callback(move |_: ()| {
        if let Some(d) = draft() {
            on_action.call(SettingsAction::SetExternalMpvPath(Some(d)));
            draft.set(None);
        }
    });
    let text = draft().unwrap_or_else(|| saved.clone().unwrap_or_default());
    let missing_hint = if saved.is_some() {
        t("settings.player.mpv.not_found_custom")
    } else {
        t(&format!("settings.player.mpv.install.{}", os.id()))
    };

    let detected: Option<Option<(String, Option<String>)>> = detect.read().clone();
    let status = match detected {
        None => rsx! {
            LoadingIndicator { small: true }
            span { class: "tk-engine-status__note", {t("settings.player.mpv.detecting")} }
        },
        Some(None) => rsx! {
            Pill { text: t("settings.player.mpv.not_found"), tone: Tone::Err }
            span { class: "tk-engine-status__note", "{missing_hint}" }
        },
        Some(Some((path, Some(v)))) => {
            // "mpv v0.40.0 Copyright © ..." -> "mpv v0.40.0".
            let short = v.split_whitespace().take(2).collect::<Vec<_>>().join(" ");
            rsx! {
                Pill { text: tf("settings.player.mpv.found", &[("version", &short)]), tone: Tone::Ok }
                code { class: "tk-engine-status__path", "{path}" }
            }
        }
        Some(Some((path, None))) => rsx! {
            Pill { text: t("settings.player.mpv.no_version"), tone: Tone::Warn }
            code { class: "tk-engine-status__path", "{path}" }
        },
    };

    rsx! {
        SettingRow { label: t("settings.player.mpv_path"), hint: t("settings.player.mpv_path_hint"), stack: true,
            div { class: "tk-settings__mpv",
                TextField {
                    label: t("settings.player.mpv_path"),
                    value: text,
                    mono: true,
                    placeholder: mpv_placeholder(os).to_string(),
                    hint: t("settings.player.mpv_path_auto"),
                    oninput: move |v: String| draft.set(Some(v)),
                    onsubmit: move |_| save.call(()),
                }
                Button {
                    label: t("common.save"),
                    variant: ButtonVariant::Tonal,
                    size: ButtonSize::Sm,
                    disabled: draft().is_none(),
                    onclick: move |_| save.call(()),
                }
            }
            div { class: "tk-engine-status", role: "status",
                {status}
                IconButton {
                    icon: "refresh".to_string(),
                    label: t("settings.player.mpv.redetect"),
                    size: IconButtonSize::Sm,
                    disabled: detect.read().is_none(),
                    onclick: move |_| detect.restart(),
                }
            }
        }
    }
}

#[component]
fn AccountSection() -> Element {
    let services = use_context::<Services>();
    let backend = services.backend.clone();
    let snack = use_snackbar();
    let mut account = use_resource({
        let backend = backend.clone();
        move || {
            let backend = backend.clone();
            async move { backend.account().await }
        }
    });
    let mut email = use_signal(String::new);
    let mut password = use_signal(String::new);
    let mut busy = use_signal(|| false);
    let mut error = use_signal(|| None::<UiError>);
    let mut syncing = use_signal(|| false);
    let mut sync_error = use_signal(|| None::<UiError>);

    let sign_in = use_callback({
        let backend = backend.clone();
        move |_: ()| {
            if busy() {
                return;
            }
            let (e, p) = (email.peek().trim().to_owned(), password.peek().clone());
            if e.is_empty() || p.is_empty() {
                return;
            }
            busy.set(true);
            error.set(None);
            let backend = backend.clone();
            spawn(async move {
                match backend.sign_in(&e, &p).await {
                    Ok(_) => {
                        password.set(String::new());
                        account.restart();
                    }
                    Err(err) => error.set(Some(err)),
                }
                busy.set(false);
            });
        }
    });
    let sign_out = {
        let backend = backend.clone();
        move |_| {
            let backend = backend.clone();
            spawn(async move {
                let _ = backend.sign_out().await;
                account.restart();
            });
        }
    };
    let sync_now = {
        let backend = backend.clone();
        move |_| {
            if syncing() {
                return;
            }
            syncing.set(true);
            sync_error.set(None);
            let backend = backend.clone();
            spawn(async move {
                match backend.sync_now().await {
                    Ok(()) => snack.show(tf("account.synced", &[("when", &t("time.now"))])),
                    Err(e) => sync_error.set(Some(e)),
                }
                syncing.set(false);
            });
        }
    };

    let loaded: Option<Result<AccountVm, UiError>> = account.read().clone();

    match loaded {
        None => rsx! {
            SkeletonLines { widths: vec![60, 40] }
        },
        Some(Err(e)) => rsx! {
            ErrorState { error: e, compact: true, on_retry: move |_| account.restart() }
        },
        Some(Ok(a)) if !a.configured => {
            let text = with_data_dir(&backend.data_dir_display(), "account.not_configured_at", "account.not_configured");
            rsx! {
                p { class: "tk-panel__hint", "{text}" }
            }
        }
        Some(Ok(AccountVm { email: Some(mail), .. })) => {
            let initial = mail.chars().next().map(|c| c.to_uppercase().to_string()).unwrap_or_default();
            rsx! {
                div { class: "tk-account",
                    div { class: "tk-account__avatar", "aria-hidden": "true", "{initial}" }
                    div { class: "tk-account__text",
                        div { class: "tk-title-md", {tf("account.signed_in_as", &[("email", &mail)])} }
                        div { class: "tk-body-sm tk-muted", {t("account.lists")} }
                    }
                    Button {
                        label: t("account.sync_now"),
                        icon: "sync".to_string(),
                        variant: ButtonVariant::Tonal,
                        size: ButtonSize::Sm,
                        disabled: syncing(),
                        onclick: sync_now,
                    }
                    Button {
                        label: t("account.sign_out"),
                        icon: "logout".to_string(),
                        variant: ButtonVariant::Outlined,
                        size: ButtonSize::Sm,
                        onclick: sign_out,
                    }
                }
                if let Some(e) = sync_error() {
                    ErrorState { error: e, compact: true, retry: false }
                }
            }
        }
        Some(Ok(_)) => rsx! {
            h3 { class: "tk-setting__label", {t("account.title")} }
            p { class: "tk-panel__hint", {t("account.body")} }
            div { class: "tk-form",
                TextField {
                    label: t("account.email"),
                    value: email(),
                    kind: "email".to_string(),
                    icon: "user".to_string(),
                    oninput: move |v: String| email.set(v),
                    onsubmit: move |_| sign_in.call(()),
                }
                TextField {
                    label: t("account.password"),
                    value: password(),
                    kind: "password".to_string(),
                    oninput: move |v: String| password.set(v),
                    onsubmit: move |_| sign_in.call(()),
                }
                if let Some(err) = error() {
                    p { class: "tk-form__error", role: "alert", {t(err.kind.key())} }
                }
                div {
                    Button {
                        label: if busy() { t("account.signing_in") } else { t("account.sign_in") },
                        disabled: busy(),
                        onclick: move |_| sign_in.call(()),
                    }
                }
            }
        },
    }
}

#[cfg(test)]
mod tests {
    use super::parse_port;

    #[test]
    fn port_field_accepts_only_unprivileged_ports() {
        assert_eq!(parse_port(" 47331 "), Some(47331));
        assert_eq!(parse_port("80"), None);
        assert_eq!(parse_port("70000"), None);
        assert_eq!(parse_port("abc"), None);
        assert_eq!(parse_port(""), None);
    }
}
