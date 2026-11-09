//! Novels home `/novels` (worker **m**; checklist `novel.route`, `novel.home`; classic `NovelHomePage.tsx`): hero + rows
//! from `Backend::novel_home()` (`SectionVm::title_key` = `novel.feed.<id>`), "Continue reading", followed updates,
//! genre chips -> `novel_search`. Contract placeholder.

use dioxus::prelude::*;

use crate::components::EmptyState;
use crate::i18n::t;

#[component]
pub fn NovelHomePage() -> Element {
    rsx! {
        div { class: "tk-page", "data-page": "novel.title",
            div { class: "tk-page__head",
                h1 { class: "tk-page__title", {t("novel.title")} }
            }
            EmptyState { icon: "bookmark".to_string(), title: t("novel.title"), body: t("common.placeholder") }
        }
    }
}
