//! Torrent client shell: page head (title + the selected torrent with its state pill and a menu to switch it, on
//! the per-torrent sections) + sticky side index of `TorrentSection::ALL` (same look as Settings) + the section
//! page. Owner: wave 3 C (docs/redesign/10 section 5.4).

use dioxus::prelude::*;

use crate::app::{Route, Ui};
use crate::components::{Button, ButtonSize, ButtonVariant, Menu, MenuItem, MenuSeparator};
use crate::i18n::t;
use crate::icons::Icon;
use crate::model::TorrentSection;

// Private helpers of the C pages (formatters, poller, sortable headers, picker). Declared here so the
// contract's `screens/torrents/mod.rs` stays untouched; siblings use `super::layout::common`.
#[path = "common.rs"]
pub(super) mod common;

use common::{StatePill, format_percent, use_torrent_list};

#[component]
pub fn TorrentsLayout() -> Element {
    let route = use_route::<Route>();
    let navigator = use_navigator();
    let current = route.torrent_section();
    let per_torrent = current.is_some_and(|s| s.per_torrent());
    let title = t("torrent.title");
    rsx! {
        div { class: "tk-torrents-scroll",
            div { class: "tk-page tk-page--torrents",
                div { class: "tk-page__head",
                    div {
                        h1 { class: "tk-page__title", "{title}" }
                        p { class: "tk-page__intro", {t("torrent.shell.intro")} }
                    }
                    if per_torrent {
                        SelectedTorrent {}
                    }
                }
                div { class: "tk-torrents",
                    nav { class: "tk-torrents__nav", "aria-label": "{title}",
                        for section in TorrentSection::ALL {
                            {
                                let id = section.id();
                                let label = t(section.label_key());
                                let active = current == Some(section);
                                rsx! {
                                    button {
                                        key: "{id}",
                                        class: "tk-torrents__link",
                                        r#type: "button",
                                        "aria-current": if active { "page" } else { "false" },
                                        "data-per-torrent": "{section.per_torrent()}",
                                        onclick: move |_| {
                                            navigator.push(Route::for_torrent_section(section));
                                        },
                                        Icon { name: section.icon().to_string(), size: 20 }
                                        span { "{label}" }
                                    }
                                }
                            }
                        }
                    }
                    div { class: "tk-torrents__body", Outlet::<Route> {} }
                }
            }
        }
    }
}

/// The torrent the per-torrent pages show (`Ui::torrent`): name, state pill and a menu with every torrent of the
/// session (plus "Library" and "Clear selection"). Without a selection it is just the "Choose a torrent" menu.
#[component]
fn SelectedTorrent() -> Element {
    let ui = use_context::<Ui>();
    let navigator = use_navigator();
    let list = use_torrent_list();
    let mut open = use_signal(|| false);
    let mut selected_sig = ui.torrent;
    let selected = ui.torrent.read().clone();

    let (loading, items) = match &*list.read() {
        None => (true, None),
        Some(Ok(items)) => (false, Some(items.clone())),
        Some(Err(_)) => (false, None),
    };
    let current = selected.as_ref().and_then(|id| items.as_ref()?.iter().find(|i| &i.id == id).cloned());
    let name = match (&current, &selected) {
        (Some(c), _) => c.name.clone(),
        // Not in the list (yet): show the info-hash prefix.
        (None, Some(id)) => id.chars().take(12).collect(),
        (None, None) => String::new(),
    };
    let switch_label = if selected.is_some() { t("torrent.shell.switch") } else { t("torrent.shell.choose") };
    let has_selection = selected.is_some();

    rsx! {
        div { class: "tk-tshell",
            if has_selection {
                div { class: "tk-tshell__current",
                    span { class: "tk-tshell__label", {t("torrent.shell.selected")} }
                    span { class: "tk-tshell__name", title: "{name}", "{name}" }
                    if let Some(c) = &current {
                        StatePill { state: c.state }
                    }
                }
            }
            Menu {
                open: open(),
                on_close: move |_| open.set(false),
                label: switch_label.clone(),
                title: t("torrent.shell.menu_title"),
                trigger: rsx! {
                    Button {
                        label: switch_label.clone(),
                        variant: ButtonVariant::Tonal,
                        size: ButtonSize::Sm,
                        icon: "magnet".to_string(),
                        trailing_icon: "chevron-down".to_string(),
                        onclick: move |_| open.toggle(),
                    }
                },
                if loading {
                    MenuItem { label: t("common.loading"), disabled: true }
                } else if let Some(items) = items {
                    if items.is_empty() {
                        MenuItem { label: t("torrent.shell.empty_title"), disabled: true }
                    }
                    for item in items {
                        {
                            let id = item.id.clone();
                            let checked = selected.as_deref() == Some(item.id.as_str());
                            rsx! {
                                MenuItem {
                                    key: "{item.id}",
                                    label: item.name.clone(),
                                    hint: format_percent(item.progress),
                                    checked: Some(checked),
                                    on_select: move |_| {
                                        selected_sig.set(Some(id.clone()));
                                        open.set(false);
                                    },
                                }
                            }
                        }
                    }
                } else {
                    MenuItem { label: t("torrent.shell.list_error"), icon: "warning".to_string(), disabled: true }
                }
                MenuSeparator {}
                MenuItem {
                    label: t("torrent.shell.open_library"),
                    icon: "library".to_string(),
                    on_select: move |_| {
                        open.set(false);
                        navigator.push(Route::TorrentLibraryPage {});
                    },
                }
                if has_selection {
                    MenuItem {
                        label: t("torrent.shell.clear"),
                        icon: "close".to_string(),
                        on_select: move |_| {
                            open.set(false);
                            selected_sig.set(None);
                        },
                    }
                }
            }
        }
    }
}
