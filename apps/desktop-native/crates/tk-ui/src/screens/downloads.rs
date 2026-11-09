//! Downloads page `/downloads` (wave B, worker **h**; checklist `downloads.page`, `downloads.history`,
//! `downloads.queue`, `downloads.local-library.*`, `downloads.integrity-check`, `downloads.storage-limit`; classic
//! `pages/profile/OfflineLibraryPage.tsx`, `core/download/*`): tabs Active · History · Anime · Manga · Novels · Local
//! library (`role="tab"` + `data-tab`), rows from `Backend::downloads()` merged with the torrent client's background
//! downloads (`torrent_library`), per-row pause / resume / cancel / retry / open folder / remove (`data-download-id`,
//! `data-state`, `data-kind`), storage header (`download_storage`, `data-storage-used`), "Check now" of the
//! auto-downloader, local library import / scan / match / play offline (`data-local-id`, `data-matched`). Contract
//! placeholder: h replaces the body.

use dioxus::prelude::*;

use crate::components::EmptyState;
use crate::i18n::t;

#[component]
pub fn DownloadsPage() -> Element {
    rsx! {
        div { class: "tk-page", "data-page": "downloads.title",
            div { class: "tk-page__head",
                h1 { class: "tk-page__title", {t("downloads.title")} }
            }
            EmptyState { icon: "download".to_string(), title: t("downloads.title"), body: t("common.placeholder") }
        }
    }
}
