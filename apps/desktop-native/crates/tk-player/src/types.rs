//! Plain data types shared by the mpv and null players.

use std::fmt;

/// Failure modes of the player. Messages are English diagnostics; the UI maps them to i18n keys.
#[derive(Debug, thiserror::Error)]
pub enum PlayerError {
    #[error("the video surface could not be created: {0}")]
    Surface(String),
    #[error("mpv: {0}")]
    Mpv(String),
    #[error("shader files missing: {0}")]
    ShadersMissing(String),
    #[error("no shaders directory configured")]
    NoShadersDir,
    #[error("the player is unavailable in this build")]
    Unavailable,
}

/// What to play. The app converts a `tk_core::StreamSource` into this.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MediaSource {
    pub url: String,
    /// Request headers from the extension (`Referer` and `User-Agent` are routed to their own mpv options).
    pub headers: Vec<(String, String)>,
    pub subtitles: Vec<SubtitleSource>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubtitleSource {
    pub url: String,
    pub lang: String,
    pub label: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrackKind {
    Video,
    Audio,
    Sub,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrackInfo {
    pub id: i64,
    pub kind: TrackKind,
    pub lang: Option<String>,
    pub title: Option<String>,
    pub selected: bool,
}

/// Events emitted by the player (position in seconds).
#[derive(Debug, Clone, PartialEq)]
pub enum PlayerEvent {
    Position(f64),
    Duration(f64),
    Paused(bool),
    /// Demuxer cache is starving.
    Buffering(bool),
    /// mpv's video output is configured: only now may the UI open the hole.
    VideoReady(bool),
    Tracks(Vec<TrackInfo>),
    Eof,
    Error(String),
}

/// A rectangle in CSS pixels, relative to the web view viewport.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl fmt::Display for Rect {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}x{}+{}+{}", self.w, self.h, self.x, self.y)
    }
}
