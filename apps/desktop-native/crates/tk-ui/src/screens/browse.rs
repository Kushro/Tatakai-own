//! "See all" of a home row (wave A, worker **a**): the feed's pages (`Backend::feed(feed, page)`) as an infinite grid. Route `/browse/:feed` with `feed` = suffix of `SectionVm::title_key`.
//!
//! Contract placeholder (docs/redesign/15-parity-wave-a-contract.md section 4): the owner replaces this file.
//! Every state needs loading (`LoadingIndicator` / skeleton), empty (`EmptyState`) and error (`ErrorState`).

use dioxus::prelude::*;

use crate::components::EmptyState;
use crate::i18n::t;

#[component]
pub fn BrowsePage(feed: String) -> Element {
    rsx! {
        div { class: "tk-page",
            div { class: "tk-page__head",
                h1 { class: "tk-page__title", {t("browse.title")} }
            }
            EmptyState { icon: "info".to_string(), title: t("browse.title"), body: t("common.placeholder") }
        }
    }
}
