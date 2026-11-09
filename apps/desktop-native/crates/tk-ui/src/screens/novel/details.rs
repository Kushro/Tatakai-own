//! Novel details `/novels/:id` (worker **m**; checklist `novel.details`, `novel.details.check-sources`,
//! `novel.chapters`, `novel.readlist`, `novel.download.chapters`; classic `NovelPage.tsx`,
//! `NovelChapterProviderRows.tsx`): `Backend::novel_details(id)`, chapters per provider with the translator filter
//! (`novel_chapters`, `data-provider`, `data-translator`), "Check all sources" (`novel_check_sources`,
//! `data-sources-checked`), read list (`read_list_entry` with `ReadKindVm::Novel`), "Download chapters" sheet
//! (`novel_download_chapters`). Contract placeholder.

use dioxus::prelude::*;

use crate::components::EmptyState;
use crate::i18n::t;

#[component]
pub fn NovelDetailsPage(id: String) -> Element {
    let _ = (id,);
    rsx! {
        div { class: "tk-page", "data-page": "novel.details.title",
            div { class: "tk-page__head",
                h1 { class: "tk-page__title", {t("novel.details.title")} }
            }
            EmptyState { icon: "bookmark".to_string(), title: t("novel.details.title"), body: t("common.placeholder") }
        }
    }
}
