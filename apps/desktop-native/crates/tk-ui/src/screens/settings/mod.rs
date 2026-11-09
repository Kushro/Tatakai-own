//! Settings: sticky side index (scroll spy) + one section file per owner (wave A contract,
//! docs/redesign/15-parity-wave-a-contract.md section 2): Appearance, Player (e), Subtitles (d), Metadata,
//! Accounts (g: AniList / MAL, the only account UI since plan 14 section 0.1 #9 removed the own login), Link,
//! Data & storage (f), System (f) and About (f). Prototype: `screens.js::S.settings`.
//!
//! This file (page shell, `SECTIONS`, `SettingsPanel`, `SettingRow`, helpers) is frozen after the contract: a
//! section's owner edits only its `<section>.rs`.
//!
//! The page owns its scroll container (`.tk-settings-scroll`, absolutely filling `.tk-main`) so its
//! `onscroll` fires; the active index entry is the last section whose top passed 25% of the viewport.

pub mod about;
pub mod accounts;
pub mod appearance;
pub mod data;
/// Wave B, worker h: Settings > Downloads.
pub mod downloads;
/// Wave B, worker b: Settings > Home & discover (blocks, calendar reminders, recommendations, mature content).
pub mod home;
pub mod link;
pub mod metadata;
/// Wave B, worker m: Settings > Novel reader (typography, flow, narration incl. KittenTTS).
pub mod novel;
pub mod player;
/// Wave B, worker l: Settings > Manga reader.
pub mod reader;
/// Wave B, worker k: Settings > Server & network (extension server, proxy, FlareSolverr, embed sniffer).
pub mod server;
/// Wave B, worker n: Settings > Social (AniList notifications polling, details extras).
pub mod social;
pub mod subtitles;
pub mod system;

use std::rc::Rc;

use dioxus::prelude::*;

use crate::app::{Services, Ui};
use crate::i18n::{t, tf};
use crate::icons::Icon;
use crate::model::{PlatformVm, SettingsAction, reduce_settings};

use self::about::AboutSection;
use self::accounts::AccountsSection;
use self::appearance::AppearanceSection;
use self::data::DataSection;
use self::downloads::DownloadsSection;
use self::home::HomeSection;
use self::link::LinkSection;
use self::metadata::MetadataSection;
use self::novel::NovelSection;
use self::player::PlayerSection;
use self::reader::ReaderSection;
use self::server::ServerSection;
use self::social::SocialSection;
use self::subtitles::SubtitlesSection;
use self::system::SystemSection;

/// `(section id, icon)`: anchors `set-<id>`, titles `settings.<id>`. Order = index order; `SettingsPanel { index }`
/// below must match. Wave B (docs/redesign/16 section 2) adds home (b), downloads (h), reader (l), novel (m),
/// server (k) and social (n); the title keys live in each owner's namespace.
const SECTIONS: [(&str, &str); 15] = [
    ("appearance", "palette"),
    ("home", "home"),
    ("player", "play"),
    ("subtitles", "cc"),
    ("reader", "grid"),
    ("novel", "bookmark"),
    ("downloads", "download"),
    ("metadata", "sync"),
    ("accounts", "user"),
    ("social", "heart"),
    ("link", "link"),
    ("server", "desktop"),
    ("data", "storage"),
    ("system", "tune"),
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
        "mal" => "MAL".to_owned(),
        "shikimori" => "Sh".to_owned(),
        "tenrai" => "Te".to_owned(),
        "mangadex" => "MD".to_owned(),
        "mangaupdates" => "MU".to_owned(),
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
                        SettingsPanel { index: 1, panels, HomeSection { commit_saved } }
                        SettingsPanel { index: 2, panels, PlayerSection { commit_saved } }
                        SettingsPanel { index: 3, panels, SubtitlesSection {} }
                        SettingsPanel { index: 4, panels, ReaderSection { commit_saved } }
                        SettingsPanel { index: 5, panels, NovelSection { commit_saved } }
                        SettingsPanel { index: 6, panels, DownloadsSection { commit_saved } }
                        SettingsPanel { index: 7, panels, MetadataSection { commit_saved } }
                        SettingsPanel { index: 8, panels, AccountsSection {} }
                        SettingsPanel { index: 9, panels, SocialSection { commit_saved } }
                        SettingsPanel { index: 10, panels, LinkSection {} }
                        SettingsPanel { index: 11, panels, ServerSection { commit_saved } }
                        SettingsPanel { index: 12, panels, DataSection {} }
                        SettingsPanel { index: 13, panels, SystemSection {} }
                        SettingsPanel { index: 14, panels, AboutSection {} }
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
