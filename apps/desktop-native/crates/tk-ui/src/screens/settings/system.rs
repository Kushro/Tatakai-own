//! Settings > System: start with the system (+ minimized), tray icon, close to tray, check updates on start (`SettingsAction::SetSystem`, `Backend::set_autostart`, `system_caps`); "Open magnet links with Tatakai" (`Backend::magnet_handler_registered` / `set_magnet_handler`, row shown only when `system_caps().magnet_handler`; the backend side is worker i's, f just renders the switch); "Run the setup assistant again" (`Route::SetupPage`); danger zone "Reset app" with `ConfirmDialog` + "keep downloaded torrents" switch (`Backend::reset_app`), `data-reset-app`.
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
