//! Manga reader `/manga/:id/read/:ext_id/:chapter_id` (worker **l**; checklist `manga.reader.*`,
//! `manga.download.offline-read`; classic `MangaReaderPage.tsx`, `components/reader/*`): pages from
//! `Backend::manga_pages(&ChapterRefVm)`, modes paged / double / vertical / webtoon (`data-mode`), fit (`data-fit`),
//! direction, background, prefetch (`Settings::manga.reader`, per-title override in the settings sheet), tap zones /
//! keys / page rail / chapter navigator, provider switch on failure, progress saved on every page
//! (`set_read_progress`, `data-page`, `data-chapter`). Bare route (`Route::is_bare`): the reader draws its own chrome.
//! Contract placeholder.

use dioxus::prelude::*;

use crate::components::EmptyState;
use crate::i18n::t;

#[component]
pub fn MangaReaderPage(id: String, ext_id: String, chapter_id: String) -> Element {
    let _ = (id, ext_id, chapter_id);
    rsx! {
        div { class: "tk-page", "data-page": "manga.reader.title",
            div { class: "tk-page__head",
                h1 { class: "tk-page__title", {t("manga.reader.title")} }
            }
            EmptyState { icon: "grid".to_string(), title: t("manga.reader.title"), body: t("common.placeholder") }
        }
    }
}
