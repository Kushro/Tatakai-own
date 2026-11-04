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
