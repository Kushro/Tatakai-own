//! Torrent client shell: page head + sticky side index of `TorrentSection::ALL` (same look as Settings) + the
//! section page. Owner: wave 3 C (contract placeholder: index + outlet only).

use dioxus::prelude::*;

use crate::app::Route;
use crate::i18n::t;
use crate::icons::Icon;
use crate::model::TorrentSection;

#[component]
pub fn TorrentsLayout() -> Element {
    let route = use_route::<Route>();
    let navigator = use_navigator();
    let current = route.torrent_section();
    let title = t("torrent.title");
    rsx! {
        div { class: "tk-torrents-scroll",
            div { class: "tk-page tk-page--torrents",
                div { class: "tk-page__head",
                    h1 { class: "tk-page__title", "{title}" }
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
