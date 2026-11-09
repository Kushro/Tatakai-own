//! Settings > Accounts: AniList and MyAnimeList connect / disconnect (`Backend::list_connect`), username + avatar, pending changes, last sync, import / export, auto progress and primary service (`SettingsAction::SetLists`).
//!
//! Split out of `screens/settings.rs` by the wave-A contract (docs/redesign/15-parity-wave-a-contract.md
//! section 2). Owner: worker **g**. The page shell (`mod.rs`: index, scroll spy, `SettingsPanel`, `SettingRow`) is frozen.

use dioxus::prelude::*;

use crate::i18n::t;

/// Contract placeholder: the owner replaces the body. Keys for this section go to the `library` namespace.
#[component]
pub fn AccountsSection() -> Element {
    rsx! {
        p { class: "tk-panel__hint", {t("common.placeholder")} }
    }
}
