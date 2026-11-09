//! Character page (wave A, worker **a**; classic `/character/:name`): portrait, names, bio facts (gender, age, birthday, blood type), description, appearances with voice actors (`Backend::character`). Route `/character/:id` (AniList character id).
//!
//! Contract placeholder (docs/redesign/15-parity-wave-a-contract.md section 4): the owner replaces this file.
//! Every state needs loading (`LoadingIndicator` / skeleton), empty (`EmptyState`) and error (`ErrorState`).

use dioxus::prelude::*;

use crate::components::EmptyState;
use crate::i18n::t;

#[component]
pub fn CharacterPage(id: String) -> Element {
    rsx! {
        div { class: "tk-page",
            div { class: "tk-page__head",
                h1 { class: "tk-page__title", {t("character.title")} }
            }
            EmptyState { icon: "info".to_string(), title: t("character.title"), body: t("common.placeholder") }
        }
    }
}
