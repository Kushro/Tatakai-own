//! Engine selection for the embedded player (docs/redesign/09 section 3). Pure data, no libmpv.
//!
//! Settings > Player picks one of three modes; the two embedded ones map to [`EngineMode`] and are passed to
//! [`crate::PlayerHost::set_engine`] before `get_or_init`. The third ("External mpv") never creates a [`crate::Player`]:
//! the UI uses [`crate::external::ExternalPlayer`] instead.

use std::path::PathBuf;

/// How the embedded libmpv is configured.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum EngineMode {
    /// Our own options only (`config=no`), our controls, our Anime4K / FSR presets. The default.
    #[default]
    Embedded,
    /// mpv-anime-build: libmpv loads `config_dir` (the extracted release: `mpv.conf`, `input.conf`, `scripts/`,
    /// `script-opts/`, `shaders/`, `fonts/`) as its config directory. `uosc: false` keeps the build's profiles,
    /// shaders and scripts but disables the uosc UI (our controls stay); `uosc: true` lets uosc draw its own UI
    /// and the page relays pointer / key input to mpv (`Player::forward_*`), hiding our controls.
    Enhanced { config_dir: PathBuf, uosc: bool },
}

impl EngineMode {
    pub fn is_enhanced(&self) -> bool {
        matches!(self, Self::Enhanced { .. })
    }

    /// True when mpv draws its own UI (uosc) and our player controls must hide.
    pub fn mpv_draws_ui(&self) -> bool {
        matches!(self, Self::Enhanced { uosc: true, .. })
    }
}

/// Everything `Player::new` needs besides the embed target. `PlayerHost` recreates the player when it changes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EngineConfig {
    pub mode: EngineMode,
    /// Pinned Anime4K / FSR shader files of our presets (resolved by the app). Unused by uosc's own menus.
    pub shaders_dir: PathBuf,
}

impl EngineConfig {
    pub fn embedded(shaders_dir: PathBuf) -> Self {
        Self { mode: EngineMode::Embedded, shaders_dir }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_enhanced_with_uosc_draws_its_own_ui() {
        assert!(!EngineMode::Embedded.mpv_draws_ui());
        assert!(!EngineMode::Enhanced { config_dir: PathBuf::from("x"), uosc: false }.mpv_draws_ui());
        assert!(EngineMode::Enhanced { config_dir: PathBuf::from("x"), uosc: true }.mpv_draws_ui());
        assert!(EngineMode::Enhanced { config_dir: PathBuf::from("x"), uosc: false }.is_enhanced());
    }
}
