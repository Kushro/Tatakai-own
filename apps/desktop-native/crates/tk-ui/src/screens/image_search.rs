//! Search by image (wave A, worker **a**; classic trace.moe page): drop / pick an image or paste a URL, confidence slider, matches with scene preview, episode and timestamp (`Backend::search_by_image`). Route `/search/image`.
//!
//! Contract placeholder (docs/redesign/15-parity-wave-a-contract.md section 4): the owner replaces this file.
//! Every state needs loading (`LoadingIndicator` / skeleton), empty (`EmptyState`) and error (`ErrorState`).

use dioxus::prelude::*;

use crate::components::EmptyState;
use crate::i18n::t;

#[component]
pub fn ImageSearchPage() -> Element {
    rsx! {
        div { class: "tk-page",
            div { class: "tk-page__head",
                h1 { class: "tk-page__title", {t("imgsearch.title")} }
            }
            EmptyState { icon: "info".to_string(), title: t("imgsearch.title"), body: t("common.placeholder") }
        }
    }
}
