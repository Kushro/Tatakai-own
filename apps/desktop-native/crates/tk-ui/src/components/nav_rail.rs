//! `nav_rail.rs`: navigation rail that expands to a drawer (Ctrl+B).

use dioxus::prelude::*;

use crate::i18n::t;
use crate::icons::Icon;
use crate::model::NavDest;

fn icon_for(dest: NavDest) -> &'static str {
    match dest {
        NavDest::Home => "home",
        NavDest::Search => "search",
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
) -> Element {
    let toggle_label = if expanded { t("nav.collapse") } else { t("nav.expand") };
    let nav_label = t("nav.label");
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
            for dest in NavDest::ALL {
                {
                    let label = t(dest.label_key());
                    let active = current == Some(dest);
                    rsx! {
                        button {
                            key: "{label}",
                            class: "tk-rail__item",
                            r#type: "button",
                            "data-active": "{active}",
                            "aria-current": if active { "page" } else { "false" },
                            "aria-label": "{label}",
                            title: "{label}",
                            onclick: move |_| on_navigate.call(dest),
                            span { class: "tk-rail__indicator",
                                Icon { name: icon_for(dest).to_string(), size: 24 }
                            }
                            span { class: "tk-rail__label", "{label}" }
                        }
                    }
                }
            }
        }
    }
}
