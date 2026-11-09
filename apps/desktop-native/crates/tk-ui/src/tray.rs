//! Tray icon and close-to-tray (wave A, worker **f**; docs/redesign/15-parity-wave-a-contract.md section 4).
//! The tray lives in tk-ui because dioxus-desktop's tray support needs a component scope:
//! `dioxus::desktop::trayicon::{init_tray_icon, DioxusTrayMenu}` (re-export of `tray-icon` 0.21),
//! `dioxus::desktop::muda::{Menu, MenuItem, PredefinedMenuItem}` for the menu (Show / Hide, Quit),
//! `dioxus::desktop::use_tray_menu_event_handler` for clicks, `dioxus::desktop::icon_from_memory` for the PNG
//! (`assets/icon.png`, to be added by f) and `use_window().set_close_behavior(WindowCloseBehaviour::...)` for
//! `Settings::system.close_to_tray`. Watch `Ui::settings` so toggling the settings creates / drops the icon live.
//!
//! Contract stub: a hook that does nothing. Called once by `app::App`.

use dioxus::prelude::*;

/// Create the tray according to `Settings::system` and keep it in sync. Contract stub (worker f).
pub fn use_tray() {
    use_hook(|| ());
}
