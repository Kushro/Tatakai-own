//! `title_bar.rs`: frameless window title bar (drag region + window controls).
//! Adapted from Miko-Kagura `src/ui/title_bar.rs` (same author; see THIRD_PARTY_NOTICES.md).

use dioxus::desktop::use_window;
use dioxus::prelude::*;

use crate::i18n::t;
use crate::icons::Icon;

#[component]
pub fn TitleBar(#[props(default)] on_toggle_nav: EventHandler<()>) -> Element {
    let win = use_window();
    let mut maximized = use_signal(|| win.window.is_maximized());

    let (drag, dbl, min, max, close) = (win.clone(), win.clone(), win.clone(), win.clone(), win.clone());
    let max_label = if maximized() { t("window.restore") } else { t("window.maximize") };
    let max_icon = if maximized() { "restore" } else { "maximize" };
    let (min_label, close_label, nav_label) = (t("window.minimize"), t("window.close"), t("nav.expand"));

    rsx! {
        header { class: "tk-titlebar",
            button {
                class: "tk-titlebar__btn tk-titlebar__menu",
                r#type: "button",
                "aria-label": "{nav_label}",
                title: "{nav_label} (Ctrl+B)",
                onclick: move |_| on_toggle_nav.call(()),
                Icon { name: "menu".to_string(), size: 18 }
            }
            div {
                class: "tk-titlebar__drag",
                onmousedown: move |_| drag.drag(),
                ondoubleclick: move |_| {
                    dbl.toggle_maximized();
                    maximized.set(dbl.window.is_maximized());
                },
                span { class: "tk-titlebar__name", {t("app.name")} }
            }
            div { class: "tk-titlebar__controls",
                button {
                    class: "tk-titlebar__btn",
                    r#type: "button",
                    "aria-label": "{min_label}",
                    title: "{min_label}",
                    onclick: move |_| min.window.set_minimized(true),
                    Icon { name: "minimize".to_string(), size: 16 }
                }
                button {
                    class: "tk-titlebar__btn",
                    r#type: "button",
                    "aria-label": "{max_label}",
                    title: "{max_label}",
                    onclick: move |_| {
                        max.toggle_maximized();
                        maximized.set(max.window.is_maximized());
                    },
                    Icon { name: max_icon.to_string(), size: 16 }
                }
                button {
                    class: "tk-titlebar__btn tk-titlebar__btn--close",
                    r#type: "button",
                    "aria-label": "{close_label}",
                    title: "{close_label}",
                    onclick: move |_| close.close(),
                    Icon { name: "close".to_string(), size: 16 }
                }
            }
        }
    }
}
