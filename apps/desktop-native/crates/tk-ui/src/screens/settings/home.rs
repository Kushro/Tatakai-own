//! Settings > Home & discover (wave B, worker **b**; checklist `home.*` toggles, `home.calendar.reminders`,
//! `settings.privacy.mature-content`, `settings.appearance.more-palettes` lives in `appearance.rs` which b also owns in
//! this wave): one `Switch` per `HomePrefs` block (`data-home-block="<field>"`), reminders on / off + minutes, the
//! recommendations toggle, and the mature-content `Segmented` (`MatureContentVm::ALL`, `data-mature`). Dispatches
//! `SettingsAction::SetHome(prefs)` through `commit_saved`. Contract placeholder: b replaces the body.

use dioxus::prelude::*;

use super::SettingRow;
use crate::i18n::t;
use crate::model::SettingsAction;

#[component]
pub fn HomeSection(commit_saved: Callback<SettingsAction>) -> Element {
    let _ = commit_saved;
    rsx! {
        p { class: "tk-panel__hint", {t("settings.home.hint")} }
        SettingRow { label: t("settings.home.placeholder"), hint: t("common.placeholder") }
    }
}
