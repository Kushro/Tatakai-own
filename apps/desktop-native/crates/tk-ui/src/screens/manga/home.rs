//! Manga home `/manga` (worker **l**; checklist `manga.route`, `manga.home`, `manga.home.followed-updates`,
//! `manga.adult-gate`; classic `MangaHomePage.tsx`): hero + rows from `Backend::manga_home()` (`SectionVm::title_key` =
//! `manga.feed.<id>`), "Continue reading" (`ContinueReadVm`), followed updates, genre chips -> `manga_search`, 18+ zone
//! behind the `Settings::home.mature_content` gate (`data-adult-gate`). Contract placeholder.

use dioxus::prelude::*;

use crate::components::EmptyState;
use crate::i18n::t;

#[component]
pub fn MangaHomePage() -> Element {
    rsx! {
        div { class: "tk-page", "data-page": "manga.title",
            div { class: "tk-page__head",
                h1 { class: "tk-page__title", {t("manga.title")} }
            }
            EmptyState { icon: "grid".to_string(), title: t("manga.title"), body: t("common.placeholder") }
        }
    }
}
