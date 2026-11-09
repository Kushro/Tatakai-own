//! Settings > Manga reader (wave B, worker **l**; checklist `manga.settings.section`, `manga.reader.fit`,
//! `manga.reader.loading`, `manga.reader.behavior`, `manga.reader.keybinds`, `manga.title-language`; classic
//! `panels/ReaderPanel.tsx`): defaults of `MangaReaderPrefs` (mode, fit, direction, background, prefetch, tap zones, page
//! number, zoom), preferred chapter language / source, auto-best, reader key bindings. `data-reader-mode`, `data-reader-fit`.
//! `SettingsAction::SetManga`. Contract placeholder.

use dioxus::prelude::*;

use super::SettingRow;
use crate::i18n::t;
use crate::model::SettingsAction;

#[component]
pub fn ReaderSection(commit_saved: Callback<SettingsAction>) -> Element {
    let _ = commit_saved;
    rsx! {
        p { class: "tk-panel__hint", {t("settings.reader.hint")} }
        SettingRow { label: t("settings.reader.placeholder"), hint: t("common.placeholder") }
    }
}
