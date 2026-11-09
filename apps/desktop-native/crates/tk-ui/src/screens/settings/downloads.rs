//! Settings > Downloads (wave B, worker **h**; checklist `downloads.location`, `downloads.prefs`,
//! `downloads.storage-limit`, `downloads.auto.profiles`, `downloads.auto.schedule`; classic `DesktopSettings.tsx`,
//! `StorageSettingsPanel.tsx`, `AutomationProfiles.tsx`): folder (`data-download-dir`, "Change..." with a native folder
//! picker through `<input type="file" webkitdirectory>` or a typed path), concurrent / speed / storage limit, preferred
//! server + audio, auto-download toggle + profile + interval + "Check now" / "Simulate" (`auto_download_check_now`,
//! `data-auto-download`), library folders. `SettingsAction::SetDownloads`. Contract placeholder.

use dioxus::prelude::*;

use super::SettingRow;
use crate::i18n::t;
use crate::model::SettingsAction;

#[component]
pub fn DownloadsSection(commit_saved: Callback<SettingsAction>) -> Element {
    let _ = commit_saved;
    rsx! {
        p { class: "tk-panel__hint", {t("settings.downloads.hint")} }
        SettingRow { label: t("settings.downloads.placeholder"), hint: t("common.placeholder") }
    }
}
