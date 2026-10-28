//! `sheet.rs`: modal side sheet (bottom sheet when the window is narrower than 600px, in CSS).
//! Scrim click, the close button and Esc call `on_close`; focus moves into the sheet on open.
//! Render it conditionally (`if open() { Sheet { .. } }`).

use dioxus::prelude::*;

use crate::components::button::IconButton;
use crate::i18n::t;

#[component]
pub fn Sheet(
    title: String,
    /// 560px instead of 400px (source picker, filters with many options).
    #[props(default)] wide: bool,
    #[props(default)] on_close: EventHandler<()>,
    /// Sticky actions at the bottom ("Apply", "Reset").
    #[props(default)] footer: Option<Element>,
    children: Element,
) -> Element {
    rsx! {
        div { class: "tk-sheet", onclick: move |_| on_close.call(()),
            aside {
                class: "tk-sheet__box",
                role: "dialog",
                "aria-modal": "true",
                "aria-label": "{title}",
                tabindex: "-1",
                "data-wide": "{wide}",
                onmounted: move |e| async move {
                    let _ = e.set_focus(true).await;
                },
                onclick: move |e| e.stop_propagation(),
                onkeydown: move |e| {
                    if e.key() == Key::Escape {
                        e.stop_propagation();
                        on_close.call(());
                    }
                },
                header { class: "tk-sheet__head",
                    h2 { class: "tk-sheet__title", "{title}" }
                    IconButton { icon: "close".to_string(), label: t("common.close"), onclick: move |_| on_close.call(()) }
                }
                div { class: "tk-sheet__body", {children} }
                if let Some(footer) = footer {
                    footer { class: "tk-sheet__foot", {footer} }
                }
            }
        }
    }
}
