//! Settings > System: start with the system (+ minimized), tray icon, close to tray, check updates on start (`SettingsAction::SetSystem`, `Backend::set_autostart`, `system_caps`).
//!
//! Split out of `screens/settings.rs` by the wave-A contract (docs/redesign/15-parity-wave-a-contract.md
//! section 2). Owner: worker **f**. The page shell (`mod.rs`: index, scroll spy, `SettingsPanel`, `SettingRow`) is frozen.

use dioxus::prelude::*;

use crate::i18n::t;

/// Contract placeholder: the owner replaces the body. Keys for this section go to the `account` namespace.
#[component]
pub fn SystemSection() -> Element {
    rsx! {
        p { class: "tk-panel__hint", {t("common.placeholder")} }
    }
}
