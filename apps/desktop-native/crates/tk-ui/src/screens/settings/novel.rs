//! Settings > Novel reader (wave B, worker **m**; checklist `novel.tts.settings`, `novel.reader.typography`,
//! `novel.reader.custom-font`, `novel.tts.kitten`; classic `panels/NovelReaderPanel.tsx`, `TtsIntegrationSettings.tsx`):
//! typography (font incl. "custom" with a `.ttf` / `.otf` picker, size, line height, paragraph gap, width, theme,
//! justify), narration (provider `Segmented`, voice list from `tts_voices`, rate / pitch, endpoint / model, API key
//! stored with `set_tts_key` and shown only as "saved" / "remove" via `tts_key_present`, `data-tts-key="saved|none"`,
//! "Test voice"), and KittenTTS (`kitten_status` / `kitten_install` / `kitten_remove`, `data-kitten="missing|installing|ready"`).
//! `SettingsAction::SetNovel`. Contract placeholder.

use dioxus::prelude::*;

use super::SettingRow;
use crate::i18n::t;
use crate::model::SettingsAction;

#[component]
pub fn NovelSection(commit_saved: Callback<SettingsAction>) -> Element {
    let _ = commit_saved;
    rsx! {
        p { class: "tk-panel__hint", {t("settings.novel.hint")} }
        SettingRow { label: t("settings.novel.placeholder"), hint: t("common.placeholder") }
    }
}
