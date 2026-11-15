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

/// One of mpv-anime-build's user-facing profiles (from mpv's `profile-list`), for the "Profiles" menu of our
/// controls in the enhanced engine. Applied with `apply-profile <name>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildProfile {
    /// mpv profile name (`HQ-SD-FSRCNNX`, `anime-shaders` ...).
    pub name: String,
    /// The build's own `profile-desc` ("Anime only (line art + clean gradients)").
    pub desc: String,
    /// An audio profile (spatial / standard / audio-only) rather than a picture one.
    pub audio: bool,
}

/// Name prefixes of profiles the build applies by itself (platform base, automatic downgrades, per-extension).
const INTERNAL_PREFIXES: &[&str] = &["Platform-", "SVP-", "Fallback-", "extension.", "protocol."];

/// The user-facing profiles in mpv's `profile-list` JSON: those with a `profile-desc` and no `profile-cond`
/// (conditional ones are applied automatically by mpv), minus the build's internal ones ([`INTERNAL_PREFIXES`],
/// `default`, `cache`) and Windows-only ones elsewhere. Picture profiles first, then audio, each in config order.
pub fn build_profiles_from_list(json: &str, windows: bool) -> Vec<BuildProfile> {
    let Ok(serde_json::Value::Array(list)) = serde_json::from_str::<serde_json::Value>(json) else {
        return Vec::new();
    };
    let mut seen = std::collections::HashSet::new();
    let mut out: Vec<BuildProfile> = list
        .iter()
        .filter_map(|p| {
            let name = p.get("name")?.as_str()?.trim();
            let desc = p.get("profile-desc").and_then(|d| d.as_str()).unwrap_or("").trim().trim_matches('"').trim();
            let cond = p.get("profile-cond").and_then(|c| c.as_str()).unwrap_or("").trim();
            if name.is_empty() || desc.is_empty() || !cond.is_empty() {
                return None;
            }
            if matches!(name, "default" | "cache") || INTERNAL_PREFIXES.iter().any(|pre| name.starts_with(pre)) {
                return None;
            }
            if !windows && desc.to_ascii_lowercase().contains("windows only") {
                return None;
            }
            // A profile defined twice (`[default]` appears twice in the build) is listed once.
            if !seen.insert(name.to_owned()) {
                return None;
            }
            let audio = name.contains("Audio") || name.starts_with("Cinema-");
            Some(BuildProfile { name: name.to_owned(), desc: desc.to_owned(), audio })
        })
        .collect();
    // Stable: config order inside each group.
    out.sort_by_key(|p| p.audio);
    out
}

/// mpv-anime-build's own profile choice in one `anime-state-broadcast` JSON: `is_anime_context` true means it runs
/// `anime-shaders`. `None` when the broadcast does not say.
pub fn broadcast_anime_context(json: &str) -> Option<bool> {
    serde_json::from_str::<serde_json::Value>(json).ok()?.get("is_anime_context")?.as_bool()
}

/// The profile the "Profiles" menu marks as active: the one picked there for the current file, else `anime-shaders`
/// when the build reports its anime context. Without either nothing is marked (the build chose a live-action tier
/// we cannot name).
pub fn active_build_profile(picked: Option<&str>, anime_context: Option<bool>) -> Option<String> {
    match (picked, anime_context) {
        (Some(p), _) if !p.is_empty() => Some(p.to_owned()),
        (_, Some(true)) => Some("anime-shaders".to_owned()),
        _ => None,
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

    #[test]
    fn build_profiles_keep_the_user_facing_ones() {
        let json = r#"[
            {"name":"default","options":[]},
            {"name":"Platform-Linux-Base","profile-desc":"Linux Core (Vulkan)","profile-cond":"platform ~= \"windows\"","options":[]},
            {"name":"cache","profile-cond":"demuxer_via_network","options":[]},
            {"name":"Cinema-Spatial-Pure","profile-desc":"\"Spatial (Native Surround Only)\"","options":[]},
            {"name":"Standard-Audio-PC","profile-desc":"Standard Audio","options":[]},
            {"name":"Bitstream-Audio","options":[]},
            {"name":"HQ-SD-FSRCNNX","profile-desc":"Non-Anime <576p (FSRCNNX Sharp)","options":[]},
            {"name":"HDR-High-Quality","profile-desc":"x","profile-cond":"p[\"video-params/sig-peak\"] > 1","options":[]},
            {"name":"anime-shaders","profile-desc":"Anime only (line art + clean gradients)","options":[]},
            {"name":"Nvidia-VSR","profile-desc":"Switches backend to D3D11 for VSR (Windows Only)","options":[]},
            {"name":"Fallback-SD-Tier2","profile-desc":"Auto-Downgrade to NNS128","options":[]},
            {"name":"Audio-Only","profile-desc":"Audio File Settings","options":[]},
            {"name":"extension.mp3","profile-desc":"x","options":[]}
        ]"#;
        let names = |w| build_profiles_from_list(json, w).into_iter().map(|p| p.name).collect::<Vec<_>>();
        assert_eq!(names(false), ["HQ-SD-FSRCNNX", "anime-shaders", "Cinema-Spatial-Pure", "Standard-Audio-PC", "Audio-Only"]);
        assert!(names(true).contains(&"Nvidia-VSR".to_owned()));
        let list = build_profiles_from_list(json, false);
        assert_eq!(list[2].desc, "Spatial (Native Surround Only)");
        assert!(list[2].audio && !list[0].audio);
        assert!(build_profiles_from_list("not json", false).is_empty());
        assert!(build_profiles_from_list("{}", false).is_empty());
    }

    #[test]
    fn active_profile_prefers_the_pick_then_the_build_context() {
        assert_eq!(broadcast_anime_context(r#"{"is_anime_context":true}"#), Some(true));
        assert_eq!(broadcast_anime_context(r#"{"anime_fidelity":true}"#), None);
        assert_eq!(active_build_profile(Some("Low-End"), Some(true)).as_deref(), Some("Low-End"));
        assert_eq!(active_build_profile(None, Some(true)).as_deref(), Some("anime-shaders"));
        assert_eq!(active_build_profile(Some(""), Some(false)), None);
        assert_eq!(active_build_profile(None, None), None);
    }
}
