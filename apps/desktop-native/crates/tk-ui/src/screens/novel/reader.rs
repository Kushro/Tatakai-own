//! Novel reader `/novels/:id/read/:ext_id/:chapter_id` (worker **m**; checklist `novel.reader.*`, `novel.tts.*`,
//! `novel.reader.offline`; classic `NovelReaderPage.tsx`, `NovelReaderPanel.tsx`): paragraphs from
//! `Backend::novel_content(&ChapterRefVm)` rendered with `Settings::novel` typography (`data-font-size`,
//! `data-theme`), settings sheet, progress by paragraph (`set_read_progress`, `data-paragraph`), continuous flow to
//! `NovelContentVm::next`, TTS bar (`tts_speak` / `tts_stop`, highlighted paragraph `data-tts-paragraph`,
//! `data-tts-state="idle|playing|paused"`). Bare route. Contract placeholder.

use dioxus::prelude::*;

use crate::components::EmptyState;
use crate::i18n::t;

#[component]
pub fn NovelReaderPage(id: String, ext_id: String, chapter_id: String) -> Element {
    let _ = (id, ext_id, chapter_id);
    rsx! {
        div { class: "tk-page", "data-page": "novel.reader.title",
            div { class: "tk-page__head",
                h1 { class: "tk-page__title", {t("novel.reader.title")} }
            }
            EmptyState { icon: "bookmark".to_string(), title: t("novel.reader.title"), body: t("common.placeholder") }
        }
    }
}
