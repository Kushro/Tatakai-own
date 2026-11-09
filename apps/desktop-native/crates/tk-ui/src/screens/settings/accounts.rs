//! Settings > Accounts: AniList and MyAnimeList connect / disconnect (`Backend::list_connect`), username + avatar, pending changes, last sync, import / export, auto progress and primary service (`SettingsAction::SetLists`).
//!
//! This is the ONLY account UI of the app (plan 14 section 0.1 #9: no own login; the former Supabase "Account"
//! section is gone). Without a configured client id (`TATAKAI_ANILIST_CLIENT_ID` / `TATAKAI_MAL_CLIENT_ID` at build
//! time) "Connect" shows a clear error (`ErrorKind::Auth`). Watch history is not an account feature: it stays local
//! and syncs with the phone over Tatakai Link (Settings > Link).
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
