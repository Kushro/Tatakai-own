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
    /// External mode: no mpv executable at the configured path nor on PATH.
    #[error("mpv was not found")]
    MpvNotFound,
    /// External mode: spawning mpv or talking to it over JSON IPC failed.
    #[error("external mpv: {0}")]
    External(String),
}

/// How a relayed key reaches mpv (`keydown` / `keyup` / `keypress`). Mouse buttons are keys too (`MBTN_LEFT`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyAction {
    Down,
    Up,
    Press,
}

/// What to play. The app converts a `tk_core::StreamSource` into this.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MediaSource {
    pub url: String,
    /// Request headers from the extension (`Referer` and `User-Agent` are routed to their own mpv options).
    pub headers: Vec<(String, String)>,
    pub subtitles: Vec<SubtitleSource>,
}

/// mpv `network-timeout` (seconds) for remote streams (embedded and enhanced engines).
pub const NETWORK_TIMEOUT_SECS: i64 = 20;
/// mpv `network-timeout` for streams served from this machine: the torrent engine's loopback HTTP server can stall
/// for a while when the pieces at the read position still have to arrive from peers.
pub const LOOPBACK_NETWORK_TIMEOUT_SECS: i64 = 120;

impl MediaSource {
    /// `http://127.0.0.1…`, `http://localhost…` or `http://[::1]…` (the torrent engine's stream server).
    pub fn is_loopback(&self) -> bool {
        let rest = self.url.strip_prefix("http://").unwrap_or("");
        ["127.0.0.1:", "127.0.0.1/", "localhost:", "localhost/", "[::1]:", "[::1]/"].iter().any(|p| rest.starts_with(p))
    }

    /// The `network-timeout` to use for this source.
    pub fn network_timeout_secs(&self) -> i64 {
        if self.is_loopback() { LOOPBACK_NETWORK_TIMEOUT_SECS } else { NETWORK_TIMEOUT_SECS }
    }
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
