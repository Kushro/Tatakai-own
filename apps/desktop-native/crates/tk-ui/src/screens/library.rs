//! Library: Continue watching (grid) + local history (docs/redesign/07 section 4).
//! Wave 0 only reserves the route and the rail destination; wave 1 (agent A) builds the screen from
//! `LocalStore::watch_history` through the backend.

use dioxus::prelude::*;

use crate::components::EmptyState;
use crate::i18n::t;

#[component]
pub fn LibraryPage() -> Element {
    rsx! {
        div { class: "tk-page tk-page--library",
            div { class: "tk-page__head",
                h1 { class: "tk-page__title", {t("lib.title")} }
            }
            EmptyState { icon: "library".to_string(), title: t("lib.empty.title"), body: t("lib.empty.body") }
        }
    }
}
