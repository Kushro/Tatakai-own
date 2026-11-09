//! Player flow preferences (wave A, worker **e**; docs/redesign/15-parity-wave-a-contract.md section 3):
//! auto-next, auto-skip intro, stable volume, Discord Rich Presence and the rebindable shortcut map.
//! Embedded in `Settings::flow`. Worker e owns this file (fields, bounds, tests); `model/mod.rs` stays frozen.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Bounds of `PlayerFlowPrefs::auto_next_secs`.
pub const AUTO_NEXT_MIN_SECS: u8 = 3;
pub const AUTO_NEXT_MAX_SECS: u8 = 60;

/// User key bindings: `shortcut action id -> key spec`, e.g. `"toggle_pause" -> "Space"`,
/// `"prev_episode" -> "j"`, `"fullscreen" -> "f"`. Action ids are `crate::shortcuts::Shortcut::id()`; key specs
/// are the `KeyboardEvent.key` value, optionally prefixed with `Ctrl+` (what the bridge reports). An action
/// missing from the map keeps its default binding (`crate::shortcuts::default_binding`).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ShortcutMapVm(pub BTreeMap<String, String>);

impl ShortcutMapVm {
    /// Drop blank specs and blank ids (a blank spec means "use the default").
    pub fn sanitized(mut self) -> Self {
        self.0.retain(|id, spec| !id.trim().is_empty() && !spec.trim().is_empty());
        for spec in self.0.values_mut() {
            *spec = spec.trim().to_owned();
        }
        self
    }

    /// The user's key for `action_id`, if rebound.
    pub fn get(&self, action_id: &str) -> Option<&str> {
        self.0.get(action_id).map(String::as_str)
    }
}

/// `Settings::flow`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct PlayerFlowPrefs {
    /// Show the next-episode countdown at the end and play it when it reaches zero.
    pub auto_next: bool,
    /// Countdown length, `AUTO_NEXT_MIN_SECS..=AUTO_NEXT_MAX_SECS`.
    pub auto_next_secs: u8,
    /// Skip the intro automatically when `Backend::skip_marks` knows it.
    pub auto_skip_intro: bool,
    /// Loudness normalisation (mpv audio filter), the classic "stable volume".
    pub stable_volume: bool,
    /// Discord Rich Presence (browsing / viewing / watching / paused).
    pub discord_presence: bool,
    pub shortcuts: ShortcutMapVm,
}

impl Default for PlayerFlowPrefs {
    fn default() -> Self {
        Self {
            auto_next: true,
            auto_next_secs: 10,
            auto_skip_intro: false,
            stable_volume: false,
            discord_presence: false,
            shortcuts: ShortcutMapVm::default(),
        }
    }
}

impl PlayerFlowPrefs {
    pub fn sanitized(mut self) -> Self {
        self.auto_next_secs = self.auto_next_secs.clamp(AUTO_NEXT_MIN_SECS, AUTO_NEXT_MAX_SECS);
        self.shortcuts = self.shortcuts.sanitized();
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_survive_sanitize_and_countdown_is_clamped() {
        let d = PlayerFlowPrefs::default();
        assert_eq!(d.clone().sanitized(), d);
        assert_eq!(PlayerFlowPrefs { auto_next_secs: 0, ..d.clone() }.sanitized().auto_next_secs, AUTO_NEXT_MIN_SECS);
        assert_eq!(PlayerFlowPrefs { auto_next_secs: 200, ..d }.sanitized().auto_next_secs, AUTO_NEXT_MAX_SECS);
    }

    #[test]
    fn shortcut_map_drops_blank_entries_and_is_transparent_json() {
        let mut m = BTreeMap::new();
        m.insert("toggle_pause".to_owned(), " k ".to_owned());
        m.insert("fullscreen".to_owned(), "".to_owned());
        let s = ShortcutMapVm(m).sanitized();
        assert_eq!(s.get("toggle_pause"), Some("k"));
        assert_eq!(s.get("fullscreen"), None);
        assert_eq!(serde_json::to_string(&s).unwrap(), r#"{"toggle_pause":"k"}"#);
    }
}
