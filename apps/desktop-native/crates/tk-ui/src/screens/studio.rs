//! Titles of a studio / producer (wave A, worker **a**; classic `/search/producer/:name`): `Backend::search_by_studio(name, page)`. Route `/studio/:name`.
//!
//! Contract placeholder (docs/redesign/15-parity-wave-a-contract.md section 4): the owner replaces this file.
//! Every state needs loading (`LoadingIndicator` / skeleton), empty (`EmptyState`) and error (`ErrorState`).

use dioxus::prelude::*;

use crate::components::EmptyState;
use crate::i18n::t;

#[component]
pub fn StudioPage(name: String) -> Element {
    rsx! {
        div { class: "tk-page",
            div { class: "tk-page__head",
                h1 { class: "tk-page__title", {t("studio.title")} }
            }
            EmptyState { icon: "info".to_string(), title: t("studio.title"), body: t("common.placeholder") }
        }
    }
}
