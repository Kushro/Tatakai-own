//! System integration preferences (wave A, worker **f**; docs/redesign/15-parity-wave-a-contract.md section 3):
//! start with the system, tray icon, close to tray, update check, first-run wizard done. Embedded in
//! `Settings::system`. Worker f owns this file (fields, bounds, tests); `model/mod.rs` stays frozen.
//!
//! Wave A addition (worker f, private to its area, serde default so older files load): `metadata_cache_ttl_hours`,
//! the TTL of the metadata cache (Settings > Metadata, `system.metadata-cache-control`; same 1..=168 h range and
//! 24 h default as Android's `AppSettings.metadataCacheTtlHours`).

use serde::{Deserialize, Serialize};

/// Bounds and default of [`SystemPrefs::metadata_cache_ttl_hours`] (= Android `SettingsRepositoryImpl`).
pub const METADATA_TTL_MIN_HOURS: u16 = 1;
pub const METADATA_TTL_MAX_HOURS: u16 = 168;
pub const METADATA_TTL_DEFAULT_HOURS: u16 = 24;
/// Choices offered by the TTL selector.
pub const METADATA_TTL_CHOICES: [u16; 5] = [1, 6, 24, 72, 168];

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
    /// The first-run wizard (`screens/setup.rs`, route `/setup`) was completed or skipped. False = the shell
    /// redirects to the wizard once settings are loaded; "Reset app" clears it. Older `ui-settings.json` files load
    /// as `false`, so existing installs see the wizard once (it is skippable in one click).
    pub setup_done: bool,
    /// How long metadata (details, characters, watch order) stays cached before AniList is asked again, in hours
    /// (`METADATA_TTL_MIN_HOURS..=METADATA_TTL_MAX_HOURS`).
    pub metadata_cache_ttl_hours: u16,
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
            setup_done: false,
            metadata_cache_ttl_hours: METADATA_TTL_DEFAULT_HOURS,
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
        self.metadata_cache_ttl_hours = self.metadata_cache_ttl_hours.clamp(METADATA_TTL_MIN_HOURS, METADATA_TTL_MAX_HOURS);
        self
    }

    /// The new-version notice applies to `tag` (`v0.2.0`): checking is on and the user did not skip that version.
    pub fn should_notify(&self, tag: &str) -> bool {
        let norm = |s: &str| s.trim().trim_start_matches(['v', 'V']).to_owned();
        self.check_updates && self.skipped_version.as_deref().is_none_or(|skipped| norm(skipped) != norm(tag))
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

    #[test]
    fn metadata_ttl_is_clamped_and_older_files_get_the_default() {
        let s = SystemPrefs { metadata_cache_ttl_hours: 0, ..SystemPrefs::default() }.sanitized();
        assert_eq!(s.metadata_cache_ttl_hours, METADATA_TTL_MIN_HOURS);
        let s = SystemPrefs { metadata_cache_ttl_hours: 10_000, ..SystemPrefs::default() }.sanitized();
        assert_eq!(s.metadata_cache_ttl_hours, METADATA_TTL_MAX_HOURS);
        let old: SystemPrefs = serde_json::from_str(r#"{"tray_icon":false,"setup_done":true}"#).unwrap();
        assert_eq!(old.metadata_cache_ttl_hours, METADATA_TTL_DEFAULT_HOURS);
        assert!(old.setup_done && !old.tray_icon && old.check_updates);
        assert!(METADATA_TTL_CHOICES.iter().all(|h| (METADATA_TTL_MIN_HOURS..=METADATA_TTL_MAX_HOURS).contains(h)));
    }

    #[test]
    fn the_update_notice_respects_the_switch_and_the_skipped_version() {
        let mut s = SystemPrefs::default();
        assert!(s.should_notify("v0.2.0"));
        s.skipped_version = Some("v0.2.0".into());
        assert!(!s.should_notify("v0.2.0"));
        assert!(!s.should_notify("0.2.0"), "the v prefix does not matter");
        assert!(s.should_notify("v0.3.0"));
        s.check_updates = false;
        assert!(!s.should_notify("v0.3.0"));
    }
}
