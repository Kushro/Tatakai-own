//! `loading.rs`: a shape that morphs between rounded forms (CSS `border-radius` + rotation keyframes;
//! docs/redesign/01 section 4 accepts CSS morphing instead of geometric Morph on desktop).

use dioxus::prelude::*;

use crate::i18n::t;

#[component]
pub fn LoadingIndicator(#[props(default)] contained: bool) -> Element {
    let label = t("common.loading");
    rsx! {
        div {
            class: "tk-loading",
            "data-contained": "{contained}",
            role: "progressbar",
            "aria-label": "{label}",
            div { class: "tk-loading__shape" }
        }
    }
}
