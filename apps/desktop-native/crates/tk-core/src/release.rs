//! Release-name parsing for torrent sources and batch files (docs/redesign/10 section 3).
//!
//! `[SubsPlease] Sousou no Frieren - 12 (1080p) [A1B2C3D4].mkv`, `[Judas] Kimetsu no Yaiba S03 (BD 1080p HEVC 10-bit
//! x265 Dual Audio) [Batch]`... -> [`ReleaseTags`], used for the source picker chips / filters and to pick the right
//! episode file inside a season pack ([`pick_episode_file`]). Anitomy-like and dependency-free.
//!
//! Signatures are the wave 3 contract; bodies by wave 3 E (stubs below).

use serde::{Deserialize, Serialize};

use crate::sources::Quality;

/// Where the video comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ReleaseSource {
    /// Blu-ray encode (`BD`, `BDRip`, `Blu-ray`).
    Bd,
    /// Untouched Blu-ray remux (`REMUX`, `BDMV` remux).
    Remux,
    /// Streaming service (`WEB`, `WEB-DL`, `WEBRip`, `CR`, `AMZN`, `NF` ...).
    Web,
    /// TV capture (`HDTV`, `TV`).
    Tv,
    Dvd,
}

impl ReleaseSource {
    /// Short display code (`BD`, `REMUX`, `WEB`, `TV`, `DVD`); not translated.
    pub fn label(self) -> &'static str {
        match self {
            Self::Bd => "BD",
            Self::Remux => "REMUX",
            Self::Web => "WEB",
            Self::Tv => "TV",
            Self::Dvd => "DVD",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum VideoCodec {
    /// `H.264`, `x264`, `AVC`.
    H264,
    /// `H.265`, `x265`, `HEVC`.
    Hevc,
    Av1,
    Vp9,
}

impl VideoCodec {
    /// `H.264`, `HEVC`, `AV1`, `VP9`; not translated.
    pub fn label(self) -> &'static str {
        match self {
            Self::H264 => "H.264",
            Self::Hevc => "HEVC",
            Self::Av1 => "AV1",
            Self::Vp9 => "VP9",
        }
    }
}

/// What a release name says. Every field is optional / `false` when the name does not say it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ReleaseTags {
    /// Release group (`SubsPlease`, `Judas`, `Erai-raws`).
    pub group: Option<String>,
    /// Anime title as written in the name, without group / episode / tags (used to match batch files).
    pub title: Option<String>,
    pub season: Option<u32>,
    /// Episode or episode range (`- 12` -> `(12, 12)`, `01-12` / `01~12` -> `(1, 12)`).
    pub episodes: Option<(u32, u32)>,
    pub source: Option<ReleaseSource>,
    pub resolution: Quality,
    pub codec: Option<VideoCodec>,
    /// `10` for `10-bit` / `Hi10P`, `8` when stated.
    pub bit_depth: Option<u8>,
    pub dual_audio: bool,
    /// `Multi-Sub`, `Multiple Subtitle`, or more than two subtitle languages listed.
    pub multi_sub: bool,
    /// Season pack / complete batch (`Batch`, `Complete`, an episode range, `S01` without an episode).
    pub batch: bool,
    /// SeaDex "best" release. Never set from the name: tk-ext copies it from the extension entry
    /// (`seadexBest` / `isBest`, Toko's SeaDex provider).
    pub seadex_best: bool,
}

/// Parse a torrent or file name. Never fails; unknown parts stay `None` / `false`.
pub fn parse_release(name: &str) -> ReleaseTags {
    // wave 3 E: real parser + unit tests with real nyaa / SubsPlease / Judas / Erai-raws / ASW names.
    ReleaseTags { resolution: Quality::parse(name), ..ReleaseTags::default() }
}

/// One file of a torrent, as [`pick_episode_file`] sees it (tk-app maps `tk_torrent::TorrentFile`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CandidateFile {
    /// The torrent's file index (returned as is).
    pub index: usize,
    /// Path inside the torrent, `/`-separated.
    pub path: String,
    pub size_bytes: u64,
}

/// Video extensions a batch file can have (`mkv`, `mp4`, `avi`, `webm`, `m2ts`, `ts`, `mov`, `wmv`, `flv`).
pub fn is_video_path(path: &str) -> bool {
    let ext = path.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default();
    matches!(ext.as_str(), "mkv" | "mp4" | "avi" | "webm" | "m2ts" | "ts" | "mov" | "wmv" | "flv")
}

/// The file of `files` that holds `episode` (1-based, as the UI counts it) of the series known by `titles` (main
/// title first, then synonyms). Skips non-video files, samples, NCOP / NCED / PV / extras folders; understands
/// absolute numbering vs `S02E03` when `titles` says which season; prefers the largest file on ties. `None` when no
/// file matches (the caller then lets the user pick in the Files table). Returns `CandidateFile::index`.
pub fn pick_episode_file(files: &[CandidateFile], episode: u32, titles: &[String]) -> Option<usize> {
    // wave 3 E: real matching. Contract stub: a torrent with a single video file plays that file.
    let _ = (episode, titles);
    let mut videos = files.iter().filter(|f| is_video_path(&f.path));
    match (videos.next(), videos.next()) {
        (Some(only), None) => Some(only.index),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_video_file_is_picked() {
        let files = vec![
            CandidateFile { index: 0, path: "Show/info.nfo".to_owned(), size_bytes: 10 },
            CandidateFile { index: 1, path: "Show/[Group] Show - 01 (1080p).mkv".to_owned(), size_bytes: 1 << 30 },
        ];
        assert_eq!(pick_episode_file(&files, 1, &["Show".to_owned()]), Some(1));
        assert!(is_video_path("a/B.MKV") && !is_video_path("a/b.ass") && !is_video_path("noext"));
    }
}
