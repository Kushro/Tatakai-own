//! tk-player: libmpv embedded in a child HWND under the transparent main window (review E5).
//!
//! * `ShaderPreset` -> `glsl-shaders` mapping lives in [`preset`] (pure, tested).
//! * `Player` is the libmpv implementation with the default `mpv` feature, a no-op `NullPlayer`
//!   shape otherwise, so `cargo test` needs no libmpv.
//! * Independent of tk-core: the app converts its stream type into [`MediaSource`].

pub mod headers;
pub mod host;
pub mod preset;
pub mod rect;
#[cfg(all(windows, feature = "mpv"))]
mod surface;
pub mod types;

#[cfg(feature = "mpv")]
mod mpv_player;
#[cfg(feature = "mpv")]
pub use mpv_player::Player;

#[cfg(not(feature = "mpv"))]
mod null_player;
#[cfg(not(feature = "mpv"))]
pub use null_player::Player;

pub use host::PlayerHost;
pub use preset::{ShaderPreset, glsl_shaders_value, list_separator, shader_paths};
pub use types::{MediaSource, PlayerError, PlayerEvent, Rect, SubtitleSource, TrackInfo, TrackKind};

/// True when this build can actually play video.
pub const HAS_MPV: bool = cfg!(feature = "mpv");
