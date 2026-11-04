//! tk-player: libmpv embedded in the app window, plus an external-mpv mode (docs/redesign/09 section 3).
//!
//! * Windows: a child HWND under the transparent main window (review E5, `surface.rs`).
//! * Linux: libmpv's render API (OpenGL) into a `gtk::GLArea` under the WebKitGTK web view (`linux/`).
//! * [`engine`]: Embedded vs Embedded enhanced (mpv-anime-build config, `enhanced.rs`); [`embed`]: the opaque
//!   embed target tk-ui obtains from the window; [`external`]: the machine's own mpv over JSON IPC.
//! * `ShaderPreset` -> `glsl-shaders` mapping lives in [`preset`] (pure, tested).
//! * `Player` is the libmpv implementation with the default `mpv` feature, a no-op `NullPlayer`
//!   shape otherwise, so `cargo test` needs no libmpv.
//! * Independent of tk-core: the app converts its stream type into [`MediaSource`].

pub mod embed;
pub mod engine;
pub mod enhanced_conf;
pub mod external;
pub mod headers;
pub mod host;
pub mod preset;
pub mod rect;
#[cfg(all(windows, feature = "mpv"))]
mod surface;
pub mod types;

#[cfg(feature = "mpv")]
mod enhanced;
#[cfg(all(target_os = "linux", feature = "mpv"))]
mod linux;
#[cfg(feature = "mpv")]
mod mpv_player;
#[cfg(feature = "mpv")]
pub use mpv_player::Player;

#[cfg(not(feature = "mpv"))]
mod null_player;
#[cfg(not(feature = "mpv"))]
pub use null_player::Player;

pub use embed::EmbedTarget;
pub use engine::{EngineConfig, EngineMode};
pub use enhanced_conf::shader_presets_apply;
pub use external::{ExternalEvent, ExternalPlayer, find_mpv, mpv_version};
pub use host::PlayerHost;
pub use preset::{ShaderPreset, glsl_shaders_value, list_separator, shader_paths};
pub use types::{KeyAction, MediaSource, PlayerError, PlayerEvent, Rect, SubtitleSource, TrackInfo, TrackKind};

/// True when this build can actually play video.
pub const HAS_MPV: bool = cfg!(feature = "mpv");
