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
    /// The preset's shaders crash this renderer (BUG-033: ArtCNN aborts libmpv's `gpu` renderer in
    /// `copy_image`); it is refused before mpv ever sees it.
    #[error("the {0} preset is not supported by this video renderer")]
    PresetUnsupported(&'static str),
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

    /// A local HLS playlist (the filtered master of BUG-059, a path or `file://` URL ending in `.m3u8` / `.m3u`).
    /// FFmpeg gives a playlist opened through `file` the whitelist `file,crypto,data`, so its `http(s)` variants
    /// fail with "Protocol 'http' not on whitelist" (BUG-080) unless the player widens it
    /// ([`LOCAL_HLS_PROTOCOL_WHITELIST`]) for this file.
    pub fn is_local_hls(&self) -> bool {
        let url = self.url.trim();
        let local = url.starts_with("file://") || !url.contains("://");
        let path = url.split(['?', '#']).next().unwrap_or("").to_ascii_lowercase();
        local && (path.ends_with(".m3u8") || path.ends_with(".m3u"))
    }
}

/// FFmpeg `protocol_whitelist` for a local HLS master whose playlists and segments are remote (BUG-080): the
/// protocols an `http(s)` HLS stream needs (`crypto` for AES-128 segments, `httpproxy` for https through a proxy).
pub const LOCAL_HLS_PROTOCOL_WHITELIST: &str = "file,http,https,tcp,tls,crypto,data,httpproxy";

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

/// libmpv2 prints its errors as `Debug` (`Raw(-13)`, `Loadfile { error: Raw(-13) }`): each raw code becomes mpv's
/// own description, keeping the number (`loading failed: the stream could not be opened (mpv -13)`, BUG-103). Pure.
pub fn readable_mpv_error(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find("Raw(") {
        out.push_str(&rest[..at]);
        let tail = &rest[at + 4..];
        match tail.find(')').and_then(|end| tail[..end].trim().parse::<i32>().ok().map(|c| (end, c))) {
            Some((end, code)) => {
                match mpv_code_text(code) {
                    Some(t) => out.push_str(&format!("{t} (mpv {code})")),
                    None => out.push_str(&format!("mpv error {code}")),
                }
                rest = &tail[end + 1..];
            }
            None => {
                out.push_str("Raw(");
                rest = tail;
            }
        }
    }
    out.push_str(rest);
    out
}

/// `mpv_error_string` of libmpv's `client.h` codes.
fn mpv_code_text(code: i32) -> Option<&'static str> {
    Some(match code {
        -1 => "event queue full",
        -2 => "memory allocation failed",
        -3 => "core not initialized",
        -4 => "invalid parameter",
        -5 => "option not found",
        -6 => "unsupported format for accessing option",
        -7 => "error setting option",
        -8 => "property not found",
        -9 => "unsupported format for accessing property",
        -10 => "property unavailable",
        -11 => "error accessing property",
        -12 => "error running command",
        -13 => "loading failed: the stream could not be opened (network error or dead link)",
        -14 => "audio output initialization failed",
        -15 => "video output initialization failed",
        -16 => "no audio or video data played",
        -17 => "unrecognized file format",
        -18 => "not supported",
        -19 => "operation not implemented",
        -20 => "unspecified error",
        _ => return None,
    })
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

#[cfg(test)]
mod readable_mpv_error_tests {
    use super::readable_mpv_error;

    #[test]
    fn raw_codes_become_text() {
        assert_eq!(readable_mpv_error("Raw(-13)"), "loading failed: the stream could not be opened (network error or dead link) (mpv -13)");
        assert_eq!(readable_mpv_error("Loadfile { error: Raw(-17) }"), "Loadfile { error: unrecognized file format (mpv -17) }");
        assert_eq!(readable_mpv_error("Raw(-99)"), "mpv error -99");
        assert_eq!(readable_mpv_error("Raw(x) and InvalidUtf8"), "Raw(x) and InvalidUtf8");
    }
}
