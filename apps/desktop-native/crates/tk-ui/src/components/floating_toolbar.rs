//! `floating_toolbar.rs`: M3E horizontal floating toolbar, sticky at the bottom of a scrolling page
//! (Details: Play/Continue, add to list, share to device, source picker).

use dioxus::prelude::*;

#[component]
pub fn FloatingToolbar(label: String, children: Element) -> Element {
    rsx! {
        div { class: "tk-ftb", role: "toolbar", "aria-label": "{label}", {children} }
    }
}

#[component]
pub fn ToolbarSeparator() -> Element {
    rsx! { span { class: "tk-ftb__sep", role: "separator", "aria-orientation": "vertical" } }
}
