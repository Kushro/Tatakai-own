//! Source model and automatic ranking (docs/redesign/07 section 3, contract in docs/redesign/08).
//!
//! [`crate::model::StreamSource`] stays what it is: the raw, playable output of one extension
//! (`sources/stream`). A [`SourceCandidate`] is the ranked wrapper the selector works with: it adds the
//! server / release group, quality, languages, reliability and the web or torrent details. It is a new
//! type instead of more fields on `StreamSource` because torrent candidates have no URL to play yet
//! (M2) and `StreamSource` is compared 1:1 with the Kotlin contract fixtures.
//!
//! Wave 0 contract: the signatures here are final; bodies marked `todo!()` are implemented by wave 1 C.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::model::{StreamSource, SubtitleTrack};

/// Reliability of a `(ext_id, label)` pair never seen before.
pub const INITIAL_RELIABILITY: f32 = 0.8;
/// Weight of the newest result in the reliability moving average.
pub const RELIABILITY_ALPHA: f32 = 0.2;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SourceKind {
    Web,
    Torrent,
}

impl SourceKind {
    /// `web` / `torrent`: the middle part of [`source_key`] and the i18n suffix (`src.<id>`).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Web => "web",
            Self::Torrent => "torrent",
        }
    }
}

/// Vertical resolution class. `Unknown` is declared first so it sorts below every real quality.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Quality {
    #[default]
    #[serde(rename = "unknown")]
    Unknown,
    #[serde(rename = "480p")]
    P480,
    #[serde(rename = "720p")]
    P720,
    #[serde(rename = "1080p")]
    P1080,
    #[serde(rename = "1440p")]
    P1440,
    #[serde(rename = "2160p")]
    P2160,
}

impl Quality {
    /// `Q` of `sources.js`: Unknown 0, 480p 1, 720p 2, 1080p 3, 1440p 4, 2160p 5.
    pub fn rank(self) -> u8 {
        match self {
            Self::Unknown => 0,
            Self::P480 => 1,
            Self::P720 => 2,
            Self::P1080 => 3,
            Self::P1440 => 4,
            Self::P2160 => 5,
        }
    }

    /// Display label (`1080p`); empty for `Unknown`.
    pub fn label(self) -> &'static str {
        match self {
            Self::Unknown => "",
            Self::P480 => "480p",
            Self::P720 => "720p",
            Self::P1080 => "1080p",
            Self::P1440 => "1440p",
            Self::P2160 => "2160p",
        }
    }

    /// Lenient parse of what extensions send (`"1080p"`, `"1080"`, `"FHD"`, `"4K"` ...); anything else is `Unknown`.
    pub fn parse(s: &str) -> Self {
        todo!("wave 1 C: parse {s:?}")
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WebInfo {
    pub url: String,
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
    /// Last measured time to first byte; `None` = not measured (no latency penalty).
    #[serde(default)]
    pub latency_ms: Option<u32>,
    #[serde(default)]
    pub subtitles: Vec<SubtitleTrack>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TorrentInfo {
    /// Hex v1 info-hash (40 chars).
    pub info_hash: String,
    #[serde(default)]
    pub size_bytes: Option<u64>,
    #[serde(default)]
    pub seeders: u32,
    #[serde(default)]
    pub leechers: u32,
    #[serde(default)]
    pub magnet: Option<String>,
}

/// One entry of the source selector. Exactly one of `web` / `torrent` is `Some`, matching `kind`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceCandidate {
    pub ext_id: String,
    pub ext_name: String,
    pub kind: SourceKind,
    /// Server ("Alpha", "Ok.ru") for web, release group ("SubsPlease") for torrent. Never contains `|`
    /// in practice, but [`parse_source_key`] tolerates it.
    pub label: String,
    #[serde(default)]
    pub quality: Quality,
    /// Display string: "HLS", "MP4", "HEVC 10-bit" ...
    #[serde(default)]
    pub container: Option<String>,
    /// ISO 639-1 lowercase codes (`es`, `en`, `ja`).
    #[serde(default)]
    pub subs: Vec<String>,
    #[serde(default)]
    pub dubs: Vec<String>,
    /// 0.0..=1.0 from [`crate::LocalStore::reliability`]; [`INITIAL_RELIABILITY`] when unknown.
    #[serde(default = "initial_reliability")]
    pub reliability: f32,
    /// "dub latino", "multi-sub", "mini".
    #[serde(default)]
    pub note: Option<String>,
    #[serde(default)]
    pub web: Option<WebInfo>,
    #[serde(default)]
    pub torrent: Option<TorrentInfo>,
}

fn initial_reliability() -> f32 {
    INITIAL_RELIABILITY
}

impl SourceCandidate {
    /// Wrap one raw extension stream as a web candidate (label = `StreamSource::provider`, quality parsed,
    /// container from the enum, reliability = [`INITIAL_RELIABILITY`] until the store says otherwise).
    pub fn from_stream(s: &StreamSource, ext_name: &str) -> Self {
        todo!("wave 1 C: wrap {} from {ext_name}", s.url)
    }

    /// The playable stream of a web candidate; `None` for torrents (M2).
    pub fn to_stream(&self) -> Option<StreamSource> {
        todo!("wave 1 C: {}", self.label)
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SourceMode {
    #[default]
    Auto,
    Manual,
}

/// Defaults: Auto / any kind / 720p / subs `es` / audio `ja`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SourcePrefs {
    pub mode: SourceMode,
    /// `None` = any kind.
    pub prefer: Option<SourceKind>,
    pub min_quality: Quality,
    pub sub_lang: String,
    pub dub_lang: String,
}

impl Default for SourcePrefs {
    fn default() -> Self {
        Self { mode: SourceMode::Auto, prefer: None, min_quality: Quality::P720, sub_lang: "es".to_owned(), dub_lang: "ja".to_owned() }
    }
}

/// `score` of `sources.js`:
/// `reliability*50 ± 20 (preferred kind) + 6*min(rank,3) − 25 if below min_quality + 10 (sub_lang in subs)
/// + 6 (dub_lang in dubs) + torrent: min(seeders,500)/50 − 20 if seeders < 20 | web: −min(latency,1000)/100
/// − 1000 when the extension is disabled`.
pub fn score(c: &SourceCandidate, prefs: &SourcePrefs, ext_enabled: bool) -> f32 {
    todo!("wave 1 C: score {} {prefs:?} {ext_enabled}", c.label)
}

/// Score every candidate and sort by score, highest first (stable: ties keep the input order).
/// `ext_enabled(ext_id)` says whether that extension is enabled and not `Unsupported`.
pub fn rank(candidates: Vec<SourceCandidate>, prefs: &SourcePrefs, ext_enabled: impl Fn(&str) -> bool) -> Vec<(SourceCandidate, f32)> {
    todo!("wave 1 C: rank {} {prefs:?} {}", candidates.len(), ext_enabled(""))
}

/// Exponential moving average: `prev*(1-α) + α*(ok ? 1 : 0)` with α = [`RELIABILITY_ALPHA`], clamped to 0..=1.
pub fn update_reliability(prev: f32, ok: bool) -> f32 {
    todo!("wave 1 C: {prev} {ok}")
}

/// Stable id `"<ext_id>|<kind>|<label>"` (`kind` = [`SourceKind::as_str`]). It is `SourceVm::id` in the UI.
pub fn source_key(c: &SourceCandidate) -> String {
    todo!("wave 1 C: key for {}", c.label)
}

/// Inverse of [`source_key`]: split on the first two `|` (the label may contain more). `None` if malformed.
pub fn parse_source_key(key: &str) -> Option<(String, SourceKind, String)> {
    todo!("wave 1 C: parse {key}")
}
