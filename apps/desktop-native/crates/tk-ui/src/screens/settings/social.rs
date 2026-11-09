//! Settings > Social (wave B, worker **n**; checklist `social.notifications.badge`, `details.reviews`): notifications
//! polling interval (0 = off) and badge switch, "show AniList activity / reviews on Details", plus a line explaining
//! that privacy and the profile are edited on anilist.co (`open_url`). `SettingsAction::SetSocial`. Without an AniList
//! account the rows are disabled with a link to Settings > Accounts. Contract placeholder.

use dioxus::prelude::*;

use super::SettingRow;
use crate::i18n::t;
use crate::model::SettingsAction;

#[component]
pub fn SocialSection(commit_saved: Callback<SettingsAction>) -> Element {
    let _ = commit_saved;
    rsx! {
        p { class: "tk-panel__hint", {t("settings.social.hint")} }
        SettingRow { label: t("settings.social.placeholder"), hint: t("common.placeholder") }
    }
}
