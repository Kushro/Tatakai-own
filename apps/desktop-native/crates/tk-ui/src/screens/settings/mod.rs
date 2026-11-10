//! Settings: side index (one section shown at a time, plus a row search) + one section file per owner (wave A contract,
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



use dioxus::prelude::*;

use crate::app::{Services, Ui};
use crate::i18n::{t, tf};
use crate::components::TextField;
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
    ("reader", "book_open"),
    ("novel", "book"),
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

    // UX review #3: one section at a time (the single 9000 px page was hard to scan) + a search over every row.
    let mut active = use_signal(|| 0_usize);
    let mut query = use_signal(String::new);
    use_context_provider(|| SettingsQuery(query));
    let searching = !query().trim().is_empty();


    rsx! {
        div { class: "tk-settings-scroll",
            div { class: "tk-page tk-page--settings",
                div { class: "tk-page__head tk-settings__head",
                    h1 { class: "tk-page__title", {t("settings.title")} }
                    div { class: "tk-settings__search", "data-settings-search": "{searching}",
                        TextField {
                            label: t("settings.search"),
                            value: query(),
                            icon: "search".to_string(),
                            placeholder: t("settings.search_hint"),
                            oninput: move |v: String| query.set(v),
                        }
                    }
                }
                div { class: "tk-settings",
                    nav { class: "tk-settings__nav", "aria-label": t("settings.title"),
                        for (i , (key , icon)) in SECTIONS.iter().enumerate() {
                            button {
                                key: "{key}",
                                class: "tk-settings__link",
                                r#type: "button",
                                "aria-current": if !searching && active() == i { "true" } else { "false" },
                                onclick: move |_| {
                                    active.set(i);
                                    query.set(String::new());
                                },
                                Icon { name: icon.to_string(), size: 20 }
                                span { {t(&format!("settings.{key}"))} }
                            }
                        }
                    }

                    div { class: "tk-settings__body", "data-searching": "{searching}",
                        SettingsPanel { index: 0, shown: searching || active() == 0, searching, AppearanceSection { commit, commit_saved } }
                        SettingsPanel { index: 1, shown: searching || active() == 1, searching, HomeSection { commit_saved } }
                        SettingsPanel { index: 2, shown: searching || active() == 2, searching, PlayerSection { commit_saved } }
                        SettingsPanel { index: 3, shown: searching || active() == 3, searching, SubtitlesSection {} }
                        SettingsPanel { index: 4, shown: searching || active() == 4, searching, ReaderSection { commit_saved } }
                        SettingsPanel { index: 5, shown: searching || active() == 5, searching, NovelSection { commit_saved } }
                        SettingsPanel { index: 6, shown: searching || active() == 6, searching, DownloadsSection { commit_saved } }
                        SettingsPanel { index: 7, shown: searching || active() == 7, searching, MetadataSection { commit_saved } }
                        SettingsPanel { index: 8, shown: searching || active() == 8, searching, AccountsSection {} }
                        SettingsPanel { index: 9, shown: searching || active() == 9, searching, SocialSection { commit_saved } }
                        SettingsPanel { index: 10, shown: searching || active() == 10, searching, LinkSection {} }
                        SettingsPanel { index: 11, shown: searching || active() == 11, searching, ServerSection { commit_saved } }
                        SettingsPanel { index: 12, shown: searching || active() == 12, searching, DataSection {} }
                        SettingsPanel { index: 13, shown: searching || active() == 13, searching, SystemSection {} }
                        SettingsPanel { index: 14, shown: searching || active() == 14, searching, AboutSection {} }
                    }
                }
            }
        }
    }
}

/// The Settings search text, read by every [`SettingRow`] (rows that do not match hide while searching).
#[derive(Clone, Copy)]
pub(super) struct SettingsQuery(Signal<String>);

/// Lower-case, accent-free form for the search ("Subtítulos" matches "subtitulos").
pub(super) fn fold(text: &str) -> String {
    text.to_lowercase()
        .chars()
        .map(|c| match c {
            'á' | 'à' | 'ä' | 'â' => 'a',
            'é' | 'è' | 'ë' | 'ê' => 'e',
            'í' | 'ì' | 'ï' | 'î' => 'i',
            'ó' | 'ò' | 'ö' | 'ô' => 'o',
            'ú' | 'ù' | 'ü' | 'û' => 'u',
            'ñ' => 'n',
            other => other,
        })
        .collect()
}

/// One settings section: anchor `set-<id>`, icon + title. Every section stays mounted (state and test hooks);
/// only the chosen one is shown, or all of them while searching (CSS hides those without a matching row).
#[component]
fn SettingsPanel(index: usize, shown: bool, searching: bool, children: Element) -> Element {
    let (sid, icon) = SECTIONS[index];
    rsx! {
        section {
            class: "tk-panel tk-settings__panel",
            id: "set-{sid}",
            "data-shown": "{shown}",
            "data-searching": "{searching}",
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
    let matches = match try_consume_context::<SettingsQuery>() {
        Some(SettingsQuery(q)) => {
            let q = fold(q().trim());
            q.is_empty() || fold(&label).contains(&q) || fold(&hint).contains(&q)
        }
        None => true,
    };
    rsx! {
        div { class: "tk-setting-row", "data-stack": "{stack}", "data-match": "{matches}",
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
    use super::{fold, parse_port};

    #[test]
    fn search_ignores_case_and_accents() {
        assert_eq!(fold("Subtítulos ÑANDÚ"), "subtitulos nandu");
        assert!(fold("Reproducción automática").contains(&fold("reproduccion")));
    }

    #[test]
    fn port_field_accepts_only_unprivileged_ports() {
        assert_eq!(parse_port(" 47331 "), Some(47331));
        assert_eq!(parse_port("80"), None);
        assert_eq!(parse_port("70000"), None);
        assert_eq!(parse_port("abc"), None);
        assert_eq!(parse_port(""), None);
    }
}
