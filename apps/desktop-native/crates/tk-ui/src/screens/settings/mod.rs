//! Settings: sticky side index (scroll spy) + one section file per owner (wave A contract,
//! docs/redesign/15-parity-wave-a-contract.md section 2): Appearance, Player (e), Subtitles (d), Metadata,
//! Account (f), Accounts (g), Link, Data & storage (f), System (f) and About (f). Prototype: `screens.js::S.settings`.
//!
//! This file (page shell, `SECTIONS`, `SettingsPanel`, `SettingRow`, helpers) is frozen after the contract: a
//! section's owner edits only its `<section>.rs`.
//!
//! The page owns its scroll container (`.tk-settings-scroll`, absolutely filling `.tk-main`) so its
//! `onscroll` fires; the active index entry is the last section whose top passed 25% of the viewport.

pub mod about;
pub mod account;
pub mod accounts;
pub mod appearance;
pub mod data;
pub mod link;
pub mod metadata;
pub mod player;
pub mod subtitles;
pub mod system;

use std::rc::Rc;

use dioxus::prelude::*;

use crate::app::{Services, Ui};
use crate::i18n::{t, tf};
use crate::icons::Icon;
use crate::model::{PlatformVm, SettingsAction, reduce_settings};

use self::about::AboutSection;
use self::account::AccountSection;
use self::accounts::AccountsSection;
use self::appearance::AppearanceSection;
use self::data::DataSection;
use self::link::LinkSection;
use self::metadata::MetadataSection;
use self::player::PlayerSection;
use self::subtitles::SubtitlesSection;
use self::system::SystemSection;

/// `(section id, icon)`: anchors `set-<id>`, titles `settings.<id>`. Order = index order; `SettingsPanel { index }`
/// below must match.
const SECTIONS: [(&str, &str); 10] = [
    ("appearance", "palette"),
    ("player", "play"),
    ("subtitles", "cc"),
    ("metadata", "sync"),
    ("account", "user"),
    ("accounts", "bookmark"),
    ("link", "link"),
    ("data", "storage"),
    ("system", "desktop"),
    ("about", "info"),
];

/// Lowest port the Link field accepts (no privileged ports).
const MIN_PORT: u16 = 1024;

pub(super) fn seg<I: Copy>(items: &[I], id: impl Fn(I) -> &'static str, key: &str) -> Vec<(String, String)> {
    items.iter().map(|i| (id(*i).to_owned(), t(&format!("{key}.{}", id(*i))))).collect()
}

/// Language options shown as upper-case codes (`ES`, `JA`), like the prototype.
pub(super) fn lang_seg(langs: &[&str]) -> Vec<(String, String)> {
    langs.iter().map(|l| ((*l).to_owned(), l.to_uppercase())).collect()
}

/// Two-letter monogram of a metadata provider (decorative).
pub(super) fn provider_mono(p: &str) -> String {
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
pub(super) fn with_data_dir(dir: &str, key_at: &str, key: &str) -> String {
    if dir.is_empty() { t(key) } else { tf(key_at, &[("dir", dir)]) }
}

/// Example mpv location for the external-mpv path field, in the OS's own form.
pub(super) fn mpv_placeholder(os: PlatformVm) -> &'static str {
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
    let snack = crate::components::use_snackbar();

    let commit = use_callback({
        let backend = services.backend.clone();
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

    let reduce = ui.settings.read().reduce_motion;

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
                        SettingsPanel { index: 0, panels, AppearanceSection { commit, commit_saved } }
                        SettingsPanel { index: 1, panels, PlayerSection { commit_saved } }
                        SettingsPanel { index: 2, panels, SubtitlesSection {} }
                        SettingsPanel { index: 3, panels, MetadataSection { commit_saved } }
                        SettingsPanel { index: 4, panels, AccountSection {} }
                        SettingsPanel { index: 5, panels, AccountsSection {} }
                        SettingsPanel { index: 6, panels, LinkSection {} }
                        SettingsPanel { index: 7, panels, DataSection {} }
                        SettingsPanel { index: 8, panels, SystemSection {} }
                        SettingsPanel { index: 9, panels, AboutSection {} }
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
pub(super) fn SettingRow(label: String, #[props(default)] hint: String, #[props(default)] stack: bool, children: Element) -> Element {
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
