//! System integration preferences (wave A, worker **f**; docs/redesign/15-parity-wave-a-contract.md section 3):
//! start with the system, tray icon, close to tray, update check. Embedded in `Settings::system`. Worker f owns
//! this file (fields, bounds, tests); `model/mod.rs` stays frozen.

use serde::{Deserialize, Serialize};

/// `Settings::system`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SystemPrefs {
    /// Register the app with the OS autostart (`tk-app/src/autostart.rs`).
    pub start_with_system: bool,
    /// When started by the OS, stay in the tray without showing the window.
    pub start_minimized: bool,
    /// Show a tray icon while the app runs (`tk-app/src/tray.rs`).
    pub tray_icon: bool,
    /// The window close button hides to the tray instead of quitting (only meaningful with `tray_icon`).
    pub close_to_tray: bool,
    /// Ask GitHub releases for a newer version at start-up (`tk-app/src/update_check.rs`).
    pub check_updates: bool,
    /// A version the user chose to skip ("Don't remind me"), as the release tag (`v0.2.0`).
    pub skipped_version: Option<String>,
}

impl Default for SystemPrefs {
    fn default() -> Self {
        Self {
            start_with_system: false,
            start_minimized: false,
            tray_icon: true,
            close_to_tray: false,
            check_updates: true,
            skipped_version: None,
        }
    }
}

impl SystemPrefs {
    pub fn sanitized(mut self) -> Self {
        if !self.tray_icon {
            self.close_to_tray = false;
        }
        if !self.start_with_system {
            self.start_minimized = false;
        }
        self.skipped_version = self.skipped_version.map(|v| v.trim().to_owned()).filter(|v| !v.is_empty());
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dependent_flags_follow_their_parent() {
        let d = SystemPrefs::default();
        assert_eq!(d.clone().sanitized(), d);
        let s = SystemPrefs { tray_icon: false, close_to_tray: true, start_minimized: true, skipped_version: Some("  ".into()), ..d };
        let s = s.sanitized();
        assert!(!s.close_to_tray && !s.start_minimized && s.skipped_version.is_none());
    }
}
