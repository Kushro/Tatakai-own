//! Source model and automatic ranking (docs/redesign/07 section 3, contract in docs/redesign/08; release tags and
//! torrent playback: docs/redesign/10).
//!
//! [`crate::model::StreamSource`] stays what it is: the raw, playable output of one extension
//! (`sources/stream`). A [`SourceCandidate`] is the ranked wrapper the selector works with: it adds the
//! server / release group, quality, languages, reliability and the web or torrent details. It is a new
//! type instead of more fields on `StreamSource` because torrent candidates have no URL to play yet
//! (M2) and `StreamSource` is compared 1:1 with the Kotlin contract fixtures.
//!
//! Signatures are the wave 1 contract (docs/redesign/08 section 3); bodies by wave 1 C.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::model::{Container, StreamSource, SubtitleTrack};
use crate::release::ReleaseTags;

/// Reliability of a `(ext_id, label)` pair never seen before.
pub const INITIAL_RELIABILITY: f32 = 0.8;
/// Weight of the newest result in the reliability moving average.
pub const RELIABILITY_ALPHA: f32 = 0.2;
/// Score bonus of a SeaDex "best" torrent release ([`score`]).
pub const SEADEX_BEST_BONUS: f32 = 4.0;

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
        let s = s.to_ascii_lowercase();
        // explicit heights first ("1080p", "1920x1080", "720"), then the usual marketing names
        for run in s.split(|c: char| !c.is_ascii_digit()).filter(|r| !r.is_empty()) {
            match run {
                "2160" | "4320" => return Self::P2160,
                "1440" => return Self::P1440,
                "1080" => return Self::P1080,
                "720" => return Self::P720,
                "480" => return Self::P480,
                _ => {}
            }
        }
        if s.contains("full hd") {
            return Self::P1080;
        }
        for word in s.split(|c: char| !c.is_ascii_alphanumeric()).filter(|w| !w.is_empty()) {
            match word {
                "4k" | "uhd" | "8k" => return Self::P2160,
                "2k" | "qhd" => return Self::P1440,
                "fhd" | "fullhd" => return Self::P1080,
                "hd" => return Self::P720,
                "sd" => return Self::P480,
                _ => {}
            }
        }
        Self::Unknown
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
    /// Release tags (wave 3, docs/redesign/10 section 3): parsed from the torrent / release name by tk-ext
    /// (`release::parse_release`) plus `seadex_best` from the extension entry. Default (empty) for web sources.
    #[serde(default)]
    pub tags: ReleaseTags,
}

fn initial_reliability() -> f32 {
    INITIAL_RELIABILITY
}

impl SourceCandidate {
    /// Wrap one raw extension stream as a web candidate (label = `StreamSource::provider`, quality parsed,
    /// container from the enum, reliability = [`INITIAL_RELIABILITY`] until the store says otherwise).
    pub fn from_stream(s: &StreamSource, ext_name: &str) -> Self {
        Self {
            ext_id: s.extension_id.clone(),
            ext_name: ext_name.to_owned(),
            kind: SourceKind::Web,
            label: s.provider.clone(),
            quality: s.quality.as_deref().map_or(Quality::Unknown, Quality::parse),
            container: Some(container_label(s.container).to_owned()),
            // external subtitle tracks are the only language data a plain StreamSource carries
            subs: lang_codes(s.subtitles.iter().map(|t| t.lang.as_str())),
            dubs: Vec::new(),
            reliability: INITIAL_RELIABILITY,
            note: None,
            web: Some(WebInfo { url: s.url.clone(), headers: s.headers.clone(), latency_ms: None, subtitles: s.subtitles.clone() }),
            torrent: None,
            tags: ReleaseTags::default(),
        }
    }

    /// The playable stream of a web candidate; `None` for torrents (M2).
    pub fn to_stream(&self) -> Option<StreamSource> {
        if self.kind != SourceKind::Web {
            return None;
        }
        let web = self.web.as_ref()?;
        let container = self.container.as_deref().and_then(parse_container).or_else(|| container_from_url(&web.url))?;
        Some(StreamSource {
            url: web.url.clone(),
            headers: web.headers.clone(),
            container,
            quality: Some(self.quality.label()).filter(|q| !q.is_empty()).map(str::to_owned),
            provider: self.label.clone(),
            extension_id: self.ext_id.clone(),
            subtitles: web.subtitles.clone(),
        })
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
///
/// Wave 3 (docs/redesign/10 section 3), torrent release tags: `+ SEADEX_BEST_BONUS` (4) when `tags.seadex_best`.
/// Small on purpose: it reorders otherwise close torrents but never beats a seeder or preference gap. (A batch bonus
/// would need the requested episode, which `score` does not get; batches are ranked like any other torrent.)
pub fn score(c: &SourceCandidate, prefs: &SourcePrefs, ext_enabled: bool) -> f32 {
    let mut v = c.reliability * 50.0;
    if let Some(kind) = prefs.prefer {
        v += if c.kind == kind { 20.0 } else { -20.0 };
    }
    let q = c.quality.rank();
    v += f32::from(q.min(3)) * 6.0;
    if q < prefs.min_quality.rank() {
        v -= 25.0;
    }
    if c.subs.iter().any(|l| *l == prefs.sub_lang) {
        v += 10.0;
    }
    if c.dubs.iter().any(|l| *l == prefs.dub_lang) {
        v += 6.0;
    }
    match c.kind {
        SourceKind::Torrent => {
            let seeders = c.torrent.as_ref().map_or(0, |t| t.seeders);
            v += seeders.min(500) as f32 / 50.0;
            if seeders < 20 {
                v -= 20.0;
            }
            if c.tags.seadex_best {
                v += SEADEX_BEST_BONUS;
            }
        }
        SourceKind::Web => {
            // not measured yet = no penalty
            if let Some(ms) = c.web.as_ref().and_then(|w| w.latency_ms) {
                v -= ms.min(1000) as f32 / 100.0;
            }
        }
    }
    if !ext_enabled {
        v -= 1000.0;
    }
    v
}

/// Score every candidate and sort by score, highest first (stable: ties keep the input order).
/// `ext_enabled(ext_id)` says whether that extension is enabled and not `Unsupported`.
pub fn rank(candidates: Vec<SourceCandidate>, prefs: &SourcePrefs, ext_enabled: impl Fn(&str) -> bool) -> Vec<(SourceCandidate, f32)> {
    let mut v: Vec<(SourceCandidate, f32)> = candidates
        .into_iter()
        .map(|c| {
            let s = score(&c, prefs, ext_enabled(&c.ext_id));
            (c, s)
        })
        .collect();
    // sort_by is stable: equal scores keep the input order
    v.sort_by(|a, b| b.1.total_cmp(&a.1));
    v
}

/// Exponential moving average: `prev*(1-α) + α*(ok ? 1 : 0)` with α = [`RELIABILITY_ALPHA`], clamped to 0..=1.
pub fn update_reliability(prev: f32, ok: bool) -> f32 {
    let prev = if prev.is_finite() { prev.clamp(0.0, 1.0) } else { INITIAL_RELIABILITY };
    let hit = if ok { 1.0 } else { 0.0 };
    (prev * (1.0 - RELIABILITY_ALPHA) + RELIABILITY_ALPHA * hit).clamp(0.0, 1.0)
}

/// Stable id `"<ext_id>|<kind>|<label>"` (`kind` = [`SourceKind::as_str`]). It is `SourceVm::id` in the UI.
pub fn source_key(c: &SourceCandidate) -> String {
    format!("{}|{}|{}", c.ext_id, c.kind.as_str(), c.label)
}

/// Inverse of [`source_key`]: split on the first two `|` (the label may contain more). `None` if malformed.
pub fn parse_source_key(key: &str) -> Option<(String, SourceKind, String)> {
    let mut parts = key.splitn(3, '|');
    let ext_id = parts.next().filter(|s| !s.is_empty())?;
    let kind = match parts.next()? {
        "web" => SourceKind::Web,
        "torrent" => SourceKind::Torrent,
        _ => return None,
    };
    let label = parts.next()?;
    Some((ext_id.to_owned(), kind, label.to_owned()))
}

/// `HLS` / `MP4` / `DASH`: the display string a web candidate carries in `container`.
pub fn container_label(c: Container) -> &'static str {
    match c {
        Container::Hls => "HLS",
        Container::Mp4 => "MP4",
        Container::Dash => "DASH",
    }
}

fn parse_container(s: &str) -> Option<Container> {
    match s.trim().to_ascii_lowercase().as_str() {
        "hls" | "m3u8" => Some(Container::Hls),
        "mp4" => Some(Container::Mp4),
        "dash" | "mpd" => Some(Container::Dash),
        _ => None,
    }
}

fn container_from_url(url: &str) -> Option<Container> {
    let path = url.split(['?', '#']).next().unwrap_or(url).to_ascii_lowercase();
    if path.ends_with(".m3u8") {
        Some(Container::Hls)
    } else if path.ends_with(".mpd") {
        Some(Container::Dash)
    } else if path.ends_with(".mp4") {
        Some(Container::Mp4)
    } else {
        None
    }
}

/// Normalize language tags to lowercase ISO 639-1 (`"es-419"` -> `es`, `"EN"` -> `en`), dropping `und` / empty
/// and duplicates while keeping the first-seen order.
pub fn lang_codes<'a>(tags: impl IntoIterator<Item = &'a str>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for t in tags {
        let code = t.trim().split(['-', '_']).next().unwrap_or("").to_ascii_lowercase();
        if code.is_empty() || code == "und" || out.contains(&code) {
            continue;
        }
        out.push(code);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stream(quality: Option<&str>) -> StreamSource {
        StreamSource {
            url: "https://cdn.example/ep.m3u8".to_owned(),
            headers: BTreeMap::from([("Referer".to_owned(), "https://site.example/".to_owned())]),
            container: Container::Hls,
            quality: quality.map(str::to_owned),
            provider: "Alpha".to_owned(),
            extension_id: "io.github.toko".to_owned(),
            subtitles: vec![
                SubtitleTrack { url: "https://cdn.example/es.vtt".to_owned(), lang: "es-419".to_owned(), label: "Español".to_owned(), format: "vtt".to_owned() },
                SubtitleTrack { url: "https://cdn.example/x.vtt".to_owned(), lang: "und".to_owned(), label: String::new(), format: "vtt".to_owned() },
                SubtitleTrack { url: "https://cdn.example/en.vtt".to_owned(), lang: "EN".to_owned(), label: "English".to_owned(), format: "vtt".to_owned() },
            ],
        }
    }

    #[test]
    fn quality_parse_is_lenient() {
        for (s, q) in [
            ("1080p", Quality::P1080),
            ("1080", Quality::P1080),
            ("1920x1080", Quality::P1080),
            ("FHD", Quality::P1080),
            ("Full HD", Quality::P1080),
            ("4K", Quality::P2160),
            ("2160p", Quality::P2160),
            ("UHD", Quality::P2160),
            ("1440p", Quality::P1440),
            ("720p", Quality::P720),
            ("HD", Quality::P720),
            ("480p", Quality::P480),
            ("SD", Quality::P480),
            ("", Quality::Unknown),
            ("auto", Quality::Unknown),
            ("default", Quality::Unknown),
        ] {
            assert_eq!(Quality::parse(s), q, "{s:?}");
        }
        assert!(Quality::Unknown < Quality::P480 && Quality::P1080 < Quality::P2160);
        assert_eq!(serde_json::to_string(&Quality::P720).unwrap(), "\"720p\"");
        assert_eq!(serde_json::from_str::<Quality>("\"unknown\"").unwrap(), Quality::Unknown);
    }

    #[test]
    fn from_stream_and_back() {
        let s = stream(Some("1080p"));
        let c = SourceCandidate::from_stream(&s, "Toko");
        assert_eq!(c.kind, SourceKind::Web);
        assert_eq!(c.label, "Alpha");
        assert_eq!(c.ext_name, "Toko");
        assert_eq!(c.quality, Quality::P1080);
        assert_eq!(c.container.as_deref(), Some("HLS"));
        assert_eq!(c.subs, ["es", "en"]);
        assert!(c.dubs.is_empty() && c.note.is_none() && c.torrent.is_none());
        assert_eq!(c.reliability, INITIAL_RELIABILITY);
        assert_eq!(c.web.as_ref().and_then(|w| w.latency_ms), None);
        assert_eq!(c.to_stream(), Some(s));
        // a missing quality stays missing
        let c = SourceCandidate::from_stream(&stream(None), "Toko");
        assert_eq!(c.quality, Quality::Unknown);
        assert_eq!(c.to_stream().unwrap().quality, None);
    }

    #[test]
    fn to_stream_falls_back_to_the_url_and_never_plays_torrents() {
        let mut c = SourceCandidate::from_stream(&stream(Some("720p")), "Toko");
        c.container = Some("HEVC".to_owned());
        assert_eq!(c.to_stream().unwrap().container, Container::Hls);
        c.web.as_mut().unwrap().url = "https://cdn.example/watch".to_owned();
        assert_eq!(c.to_stream(), None);
        let mut t = SourceCandidate::from_stream(&stream(Some("720p")), "Nyaa");
        t.kind = SourceKind::Torrent;
        t.torrent = Some(TorrentInfo { info_hash: "ab".repeat(20), size_bytes: None, seeders: 10, leechers: 1, magnet: None });
        assert_eq!(t.to_stream(), None);
    }

    #[test]
    fn source_key_round_trip() {
        let mut c = SourceCandidate::from_stream(&stream(None), "Toko");
        assert_eq!(source_key(&c), "io.github.toko|web|Alpha");
        assert_eq!(parse_source_key("io.github.toko|web|Alpha"), Some(("io.github.toko".to_owned(), SourceKind::Web, "Alpha".to_owned())));
        c.label = "odd|label".to_owned();
        c.kind = SourceKind::Torrent;
        let key = source_key(&c);
        assert_eq!(parse_source_key(&key), Some(("io.github.toko".to_owned(), SourceKind::Torrent, "odd|label".to_owned())));
        assert_eq!(parse_source_key("x|web|"), Some(("x".to_owned(), SourceKind::Web, String::new())));
        for bad in ["", "x", "x|web", "x|ftp|y", "|web|y"] {
            assert_eq!(parse_source_key(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn ema_and_clamp() {
        assert!((update_reliability(INITIAL_RELIABILITY, true) - 0.84).abs() < 1e-6);
        assert!((update_reliability(INITIAL_RELIABILITY, false) - 0.64).abs() < 1e-6);
        assert_eq!(update_reliability(1.0, true), 1.0);
        assert_eq!(update_reliability(0.0, false), 0.0);
        assert!((update_reliability(7.0, false) - 0.8).abs() < 1e-6); // clamped to 1 first
        assert!((update_reliability(f32::NAN, true) - 0.84).abs() < 1e-6);
    }

    #[test]
    fn seadex_best_torrents_get_a_small_bonus() {
        let mut t = SourceCandidate::from_stream(&stream(Some("1080p")), "Toko");
        t.kind = SourceKind::Torrent;
        t.web = None;
        t.torrent = Some(TorrentInfo { info_hash: "ab".repeat(20), size_bytes: None, seeders: 100, leechers: 1, magnet: None });
        let prefs = SourcePrefs::default();
        let plain = score(&t, &prefs, true);
        t.tags.seadex_best = true;
        assert!((score(&t, &prefs, true) - plain - SEADEX_BEST_BONUS).abs() < 1e-4);
        // web candidates never carry it, and the flag alone does nothing for them
        let mut w = SourceCandidate::from_stream(&stream(Some("1080p")), "Toko");
        let base = score(&w, &prefs, true);
        w.tags.seadex_best = true;
        assert!((score(&w, &prefs, true) - base).abs() < 1e-4);
    }

    #[test]
    fn unmeasured_latency_is_not_penalized_and_ties_are_stable() {
        let mut a = SourceCandidate::from_stream(&stream(Some("1080p")), "Toko");
        let prefs = SourcePrefs::default();
        let base = score(&a, &prefs, true);
        // 0.8*50 + 18 + 10 (es sub) = 68, no dub, no latency
        assert!((base - 68.0).abs() < 1e-4, "{base}");
        a.web.as_mut().unwrap().latency_ms = Some(5000);
        assert!((score(&a, &prefs, true) - 58.0).abs() < 1e-4); // capped at 1000 ms
        let mut b = a.clone();
        b.label = "Beta".to_owned();
        let mut c = a.clone();
        c.label = "Gamma".to_owned();
        let order: Vec<String> = rank(vec![a, b, c], &prefs, |_| true).into_iter().map(|(c, _)| c.label).collect();
        assert_eq!(order, ["Alpha", "Beta", "Gamma"]);
    }
}
