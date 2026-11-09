//! `nav_rail.rs`: navigation rail that expands to a drawer (Ctrl+B), with the search FAB on top,
//! optional badges per destination and a footer slot (debug: component gallery).

use dioxus::prelude::*;

use crate::i18n::t;
use crate::icons::Icon;
use crate::model::NavDest;

pub fn icon_for(dest: NavDest) -> &'static str {
    match dest {
        NavDest::Home => "home",
        NavDest::Search => "search",
        NavDest::Library => "library",
        // Wave B (docs/redesign/16 section 2): Downloads (h), Manga (l), Novels (m).
        NavDest::Downloads => "download",
        NavDest::Manga => "grid",
        NavDest::Novels => "bookmark",
        NavDest::Torrents => "magnet",
        NavDest::Extensions => "extensions",
        NavDest::Link => "link",
        NavDest::Settings => "settings",
    }
}

#[component]
pub fn NavRail(
    current: Option<NavDest>,
    expanded: bool,
    #[props(default)] on_navigate: EventHandler<NavDest>,
    #[props(default)] on_toggle: EventHandler<()>,
    /// Search FAB (Ctrl+K).
    #[props(default)] on_search: EventHandler<()>,
    /// `(destination, count)`; 0 hides the badge (e.g. extension updates).
    #[props(default)] badges: Vec<(NavDest, u32)>,
    /// Items at the bottom of the rail (render `RailFootItem`s).
    children: Element,
) -> Element {
    let toggle_label = if expanded { t("nav.collapse") } else { t("nav.expand") };
    let nav_label = t("nav.label");
    let search_label = t("nav.search_fab");
    rsx! {
        nav { class: "tk-rail", "data-expanded": "{expanded}", "aria-label": "{nav_label}",
            button {
                class: "tk-rail__toggle",
                r#type: "button",
                "aria-label": "{toggle_label}",
                "aria-expanded": "{expanded}",
                title: "{toggle_label} (Ctrl+B)",
                onclick: move |_| on_toggle.call(()),
                Icon { name: "menu".to_string(), size: 24 }
            }
            button {
                class: "tk-rail__fab",
                r#type: "button",
                "aria-label": "{search_label}",
                title: "{search_label} (Ctrl+K)",
                onclick: move |_| on_search.call(()),
                Icon { name: "search".to_string(), size: 24 }
                span { class: "tk-rail__label", "{search_label}" }
            }
            for (i , dest) in NavDest::ALL.into_iter().enumerate() {
                {
                    let label = t(dest.label_key());
                    let active = current == Some(dest);
                    let badge = badges.iter().find(|(d, _)| *d == dest).map(|(_, n)| *n).unwrap_or(0);
                    let n = i + 1;
                    rsx! {
                        button {
                            key: "{label}",
                            class: "tk-rail__item",
                            r#type: "button",
                            "data-active": "{active}",
                            "aria-current": if active { "page" } else { "false" },
                            "aria-label": "{label}",
                            title: "{label} ({n})",
                            onclick: move |_| on_navigate.call(dest),
                            span { class: "tk-rail__indicator",
                                Icon { name: icon_for(dest).to_string(), size: 24 }
                                if badge > 0 {
                                    span { class: "tk-rail__badge", "aria-hidden": "true", "{badge}" }
                                }
                            }
                            span { class: "tk-rail__label", "{label}" }
                        }
                    }
                }
            }
            div { class: "tk-rail__foot", {children} }
        }
    }
}

/// Small secondary item for the rail footer.
#[component]
pub fn RailFootItem(icon: String, label: String, #[props(default)] onclick: EventHandler<MouseEvent>) -> Element {
    rsx! {
        button {
            class: "tk-rail__item",
            r#type: "button",
            "aria-label": "{label}",
            title: "{label}",
            onclick: move |e| onclick.call(e),
            span { class: "tk-rail__indicator", Icon { name: icon.clone(), size: 20 } }
            span { class: "tk-rail__label", "{label}" }
        }
    }
}
