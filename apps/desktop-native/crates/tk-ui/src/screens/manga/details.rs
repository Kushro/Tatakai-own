//! Manga details `/manga/:id` (worker **l**; checklist `manga.details`, `manga.details.anime-link`,
//! `manga.chapters.providers`, `manga.chapters.auto-best`, `manga.chapters.tools`, `manga.readlist`,
//! `manga.download.chapters`; classic `MangaPage.tsx`, `MangaChapterProviderRows.tsx`): `Backend::manga_details(id)`,
//! chapters per provider (`manga_chapters`, `data-provider`, sort / filter / group by volume / refresh / clear cache),
//! read list (`read_list_entry` / `set_read_list_entry`, `data-read-status`), "Download chapters" sheet
//! (`manga_download_chapters`), link to the anime (`MangaDetailsVm::anime_id`). Contract placeholder.

use dioxus::prelude::*;

use crate::components::EmptyState;
use crate::i18n::t;

#[component]
pub fn MangaDetailsPage(id: String) -> Element {
    let _ = (id,);
    rsx! {
        div { class: "tk-page", "data-page": "manga.details.title",
            div { class: "tk-page__head",
                h1 { class: "tk-page__title", {t("manga.details.title")} }
            }
            EmptyState { icon: "grid".to_string(), title: t("manga.details.title"), body: t("common.placeholder") }
        }
    }
}
