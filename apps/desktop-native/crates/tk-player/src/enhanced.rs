//! libmpv options per engine mode (docs/redesign/09 section 3.3, owner B). `mpv_player::Player::new` calls, in order:
//!
//! 1. [`pre_init`] inside `Mpv::with_initializer` (before `mpv_initialize`): for `Enhanced`, `config-dir`, `config=yes`,
//!    `load-scripts`, `input-conf`, uosc on/off ... Options that only work before initialization go here.
//! 2. [`base_options`]: the property list applied right after creation. `Embedded` keeps today's list; `Enhanced`
//!    drops what would override the build's own `mpv.conf` (and keeps input bindings on when uosc draws its UI).
//! 3. [`post_init`] after the event context is set up (e.g. reading back profile names for diagnostics).
//!
//! The release is extracted by tk-app (`anime_build.rs`); `config_dir` is that directory (it holds `mpv.conf`).

use std::path::Path;

use libmpv2::Mpv;
use libmpv2::MpvInitializer;

use crate::engine::EngineMode;
use crate::types::PlayerError;

/// Options to set before `mpv_initialize`.
pub(crate) fn pre_init(init: &MpvInitializer, mode: &EngineMode) -> libmpv2::Result<()> {
    match mode {
        EngineMode::Embedded => Ok(()),
        EngineMode::Enhanced { config_dir, uosc } => {
            // Contract stub: B sets config-dir / config / scripts here.
            let _: (&MpvInitializer, &Path, &bool) = (init, config_dir.as_path(), uosc);
            Ok(())
        }
    }
}

/// `(property, value)` pairs set right after creation (best effort: a refused one is logged, not fatal).
pub(crate) fn base_options(mode: &EngineMode) -> Vec<(&'static str, &'static str)> {
    let _ = mode;
    vec![
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
    ]
}

/// Anything to do once mpv runs.
pub(crate) fn post_init(mpv: &Mpv, mode: &EngineMode) -> Result<(), PlayerError> {
    let _ = (mpv, mode);
    Ok(())
}
