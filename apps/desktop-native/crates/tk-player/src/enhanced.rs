//! libmpv options per engine mode (docs/redesign/09 section 3.3, owner B). `mpv_player::Player::new` calls, in order:
//!
//! 1. [`pre_init`] inside `Mpv::with_initializer` (before `mpv_initialize`): for `Enhanced`, `config-dir`, `config=yes`,
//!    `load-scripts`, `input-conf`, uosc on/off ... Options that only work before initialization go here.
//! 2. [`base_options`]: the property list applied right after creation. `Embedded` keeps today's list; `Enhanced`
//!    drops what would override the build's own `mpv.conf` (and keeps input bindings on when uosc draws its UI).
//! 3. [`post_init`] after the event context is set up (e.g. reading back profile names for diagnostics).
//!
//! The release is extracted by tk-app (`anime_build.rs`); `config_dir` is that directory (it holds `mpv.conf`, already
//! run through [`crate::enhanced_conf::sanitize_mpv_conf`]). The pure rules (script list, sanitizing) live in
//! `enhanced_conf.rs` so they are tested without libmpv.

use libmpv2::Mpv;
use libmpv2::MpvInitializer;

use crate::engine::EngineMode;
use crate::enhanced_conf::select_scripts;
use crate::preset::list_separator;
use crate::types::PlayerError;

/// Options to set before `mpv_initialize`: the config directory itself and which scripts load. Anything the build's
/// `mpv.conf` also sets would be overwritten while mpv reads it, so those go in [`base_options`] instead.
pub(crate) fn pre_init(init: &MpvInitializer, mode: &EngineMode) -> libmpv2::Result<()> {
    match mode {
        EngineMode::Embedded => Ok(()),
        EngineMode::Enhanced { config_dir, uosc } => {
            let dir = config_dir.to_string_lossy().into_owned();
            init.set_option("config-dir", dir.as_str())?;
            init.set_option("config", "yes")?;
            // `~~/scripts` minus what cannot work embedded (enhanced_conf.rs); uosc only when it draws the UI.
            init.set_option("load-scripts", "no")?;
            let scripts: Vec<String> =
                select_scripts(config_dir, *uosc).iter().map(|p| p.to_string_lossy().into_owned()).collect();
            if !scripts.is_empty() {
                let sep = list_separator().to_string();
                init.set_option("scripts", scripts.join(sep.as_str()).as_str())?;
            }
            // The built-in OSC stays off in both variants (uosc replaces it; with our controls there is none).
            init.set_option("osc", "no")?;
            Ok(())
        }
    }
}

/// `(property, value)` pairs set right after creation (best effort: a refused one is logged, not fatal).
pub(crate) fn base_options(mode: &EngineMode) -> Vec<(&'static str, &'static str)> {
    match mode {
        EngineMode::Embedded => vec![
            ("vo", "gpu-next,gpu"),
            ("hwdec", "auto-safe"),
            ("osd-level", "0"),
            ("osd-bar", "no"),
            ("ytdl", "no"),
            ("input-default-bindings", "no"),
            ("input-vo-keyboard", "no"),
            ("cache", "yes"),
            ("title", "Tatakai"),
            (
                "stream-lavf-o",
                "reconnect=1,reconnect_streamed=1,reconnect_on_network_error=1,reconnect_delay_max=2,reconnect_max_retries=6",
            ),
        ],
        // Set after mpv read the build's (sanitized) mpv.conf, so these win over its `[default]` section. Left to the
        // build: OSD look and level, subtitles, scaling, profiles, shaders, audio.
        EngineMode::Enhanced { uosc, .. } => {
            let input = if *uosc { "yes" } else { "no" };
            vec![
                ("vo", "gpu-next,gpu"),
                ("hwdec", "auto-safe"),
                // uosc draws its own seek feedback; with our controls the bar would duplicate them.
                ("osd-bar", "no"),
                ("ytdl", "no"),
                // uosc: the build's input.conf drives uosc (the page relays pointer / keys). Ours: our shortcuts.
                ("input-default-bindings", input),
                ("input-vo-keyboard", input),
                ("input-cursor", input),
                // The app records progress and passes `start`; mpv's own resume would fight it.
                ("resume-playback", "no"),
                ("save-position-on-quit", "no"),
                ("input-ipc-server", ""),
                ("fullscreen", "no"),
                ("keep-open", "no"),
                ("cache", "yes"),
                ("title", "Tatakai"),
                (
                    "stream-lavf-o",
                    "reconnect=1,reconnect_streamed=1,reconnect_on_network_error=1,reconnect_delay_max=2,reconnect_max_retries=6",
                ),
            ]
        }
    }
}

/// Anything to do once mpv runs: log what the build loaded (diagnostics for "the profiles do nothing" reports).
pub(crate) fn post_init(mpv: &Mpv, mode: &EngineMode) -> Result<(), PlayerError> {
    if let EngineMode::Enhanced { config_dir, uosc } = mode {
        let mpv_dir = mpv.get_property::<String>("config-dir").unwrap_or_default();
        let scripts = mpv.get_property::<String>("scripts").unwrap_or_default();
        tracing::info!(
            config_dir = %config_dir.display(),
            mpv_config_dir = %mpv_dir,
            uosc = *uosc,
            scripts = scripts.split(list_separator()).filter(|s| !s.is_empty()).count(),
            "mpv-anime-build engine"
        );
        if !config_dir.join("mpv.conf").is_file() {
            tracing::warn!(config_dir = %config_dir.display(), "mpv-anime-build: mpv.conf missing, mpv defaults only");
        }
    }
    Ok(())
}

/// mpv command (name + args) that switches mpv-anime-build to the mode closest to `preset` (its input.conf
/// bindings); `None` when the build has no equivalent (FSR, NVScaler, CAS).
pub fn build_mode_command(preset: crate::ShaderPreset) -> Option<&'static [&'static str]> {
    use crate::ShaderPreset as P;
    Some(match preset {
        P::Off => &["change-list", "glsl-shaders", "clr", ""],
        P::Anime4kA => &["script-message", "anime4k-mode", "A"],
        P::Anime4kB => &["script-message", "anime4k-mode", "B"],
        P::Anime4kC => &["script-message", "anime4k-mode", "C"],
        P::Anime4kAA => &["script-message", "anime4k-mode", "AA"],
        P::Anime4kBB => &["script-message", "anime4k-mode", "BB"],
        P::Anime4kCA => &["script-message", "anime4k-mode", "CA"],
        P::ArtCnn => &["script-message", "anime4k-mode", "Ani4Kv2"],
        P::Fidelity => &["script-message", "toggle-anime-fidelity"],
        P::Fsr | P::NvScaler | P::Cas => return None,
    })
}

/// `toggle-anime-fidelity` of mpv-anime-build (a toggle, per resolution tier).
const TOGGLE_FIDELITY: &[&str] = &["script-message", "toggle-anime-fidelity"];

/// Commands that switch mpv-anime-build to `preset`, given the build's Fidelity state for the current resolution
/// (`fidelity`, from its `anime-state-broadcast`; `None` = not heard yet). The build refuses `anime4k-mode` while
/// Fidelity (FSRCNNX) is on ("Locked: Switch to Anime4K Mode to use.", BUG-032), so an Anime4K mode first leaves
/// Fidelity; and Fidelity is only toggled when it is off, so choosing it twice does not turn it off again.
pub fn build_mode_steps(preset: crate::ShaderPreset, fidelity: Option<bool>) -> Option<Vec<&'static [&'static str]>> {
    use crate::ShaderPreset as P;
    let cmd = build_mode_command(preset)?;
    Some(match preset {
        P::Fidelity if fidelity == Some(true) => Vec::new(),
        P::Fidelity | P::Off => vec![cmd],
        _ if fidelity == Some(true) => vec![TOGGLE_FIDELITY, cmd],
        _ => vec![cmd],
    })
}

/// The Fidelity state in one `anime-state-broadcast` JSON of mpv-anime-build (`anime_fidelity`); partial
/// broadcasts without it give `None`.
pub fn broadcast_fidelity(json: &str) -> Option<bool> {
    serde_json::from_str::<serde_json::Value>(json).ok()?.get("anime_fidelity")?.as_bool()
}

#[cfg(test)]
mod build_mode_tests {
    use super::build_mode_command;
    use crate::ShaderPreset;

    #[test]
    fn in_enhanced_build_matches_the_build_modes() {
        for p in ShaderPreset::ALL {
            assert_eq!(p.in_enhanced_build(), build_mode_command(p).is_some(), "{p:?}");
        }
    }

    #[test]
    fn anime4k_modes_map_to_the_build_bindings() {
        assert_eq!(build_mode_command(ShaderPreset::Anime4kCA), Some(&["script-message", "anime4k-mode", "CA"][..]));
        assert_eq!(build_mode_command(ShaderPreset::Off).map(|a| a[0]), Some("change-list"));
        assert_eq!(build_mode_command(ShaderPreset::Cas), None);
    }

    #[test]
    fn anime4k_leaves_fidelity_first_and_fidelity_is_not_toggled_off() {
        use super::{broadcast_fidelity, build_mode_steps};
        let a = build_mode_steps(ShaderPreset::Anime4kA, Some(true)).unwrap();
        assert_eq!(a, vec![&["script-message", "toggle-anime-fidelity"][..], &["script-message", "anime4k-mode", "A"][..]]);
        assert_eq!(build_mode_steps(ShaderPreset::Anime4kA, Some(false)).unwrap().len(), 1);
        assert_eq!(build_mode_steps(ShaderPreset::Anime4kA, None).unwrap().len(), 1);
        assert!(build_mode_steps(ShaderPreset::Fidelity, Some(true)).unwrap().is_empty());
        assert_eq!(build_mode_steps(ShaderPreset::Fidelity, Some(false)).unwrap().len(), 1);
        assert_eq!(build_mode_steps(ShaderPreset::Off, Some(true)).unwrap().len(), 1);
        assert_eq!(build_mode_steps(ShaderPreset::Cas, Some(true)), None);
        assert_eq!(broadcast_fidelity(r#"{"anime_fidelity":true,"anime4k_allowed":false}"#), Some(true));
        assert_eq!(broadcast_fidelity(r#"{"hdr_display_mode":"auto"}"#), None);
        assert_eq!(broadcast_fidelity("nope"), None);
    }
}

