//! `surface.rs` chips: Pill (status) and InfoChip (metadata).

use dioxus::prelude::*;

#[component]
pub fn Pill(text: String, #[props(default)] tone: String) -> Element {
    rsx! { span { class: "tk-pill tk-pill--{tone}", "{text}" } }
}

#[component]
pub fn InfoChip(text: String) -> Element {
    rsx! { span { class: "tk-chip", "{text}" } }
}
