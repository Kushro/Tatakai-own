//! Release-name parsing for torrent sources and batch files (docs/redesign/10 section 3).
//!
//! `[SubsPlease] Sousou no Frieren - 12 (1080p) [A1B2C3D4].mkv`, `[Judas] Kimetsu no Yaiba S03 (BD 1080p HEVC 10-bit
//! x265 Dual Audio) [Batch]`... -> [`ReleaseTags`], used for the source picker chips / filters and to pick the right
//! episode file inside a season pack ([`pick_episode_file`]). Anitomy-like and dependency-free.
//!
//! Signatures are the wave 3 contract; bodies by wave 3 E.
//!
//! How a name is read (every step is best effort, nothing fails):
//! 1. the directory part and a known extension are dropped; a leading `[Group]` is the group (unless it only holds
//!    tags such as `[1080p]`), else a trailing scene group (`...x264-GROUP`);
//! 2. `_` becomes a space, and a name without spaces but with dots (`Show.S01E05.1080p`) is split on the dots;
//! 3. the name is cut into alphanumeric words, each one knowing whether it sits inside brackets;
//! 4. season / episode, in order: `S01E05`, `S01` / `Season 2` / `2nd Season`, `Title - 05` (also `- 01-12`,
//!    `- 01 ~ 12`), a bare range `01-12`, `EP05` / `Episode 5`, and last the final bare number before the tags;
//! 5. the title is the text before the first episode / season marker, bracket or tag word;
//! 6. tags come from the words (`BDRip`, `WEB-DL`, `REMUX`, `x265`, `H.264`, `AV1`, `10bit`, `Hi10P`, `Dual Audio`,
//!    `Multi-Subs`, more than two subtitle language codes in brackets, `Batch`, `Complete`).

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

/// Groups that only publish streaming-service rips, so a name of theirs without a source tag is `WEB`.
const WEB_GROUPS: [&str; 8] = ["subsplease", "horriblesubs", "erai-raws", "tsundere-raws", "varyg", "toonshub", "asw", "yameii"];

/// ISO 639-2 subtitle codes release names list in brackets (`[ENG][POR-BR][SPA-LA]...`); more than two = multi-sub.
const SUB_LANGS: [&str; 38] = [
    "eng", "spa", "por", "ara", "fre", "fra", "ger", "deu", "ita", "rus", "chi", "zho", "chs", "cht", "kor", "pol", "tur", "ind",
    "vie", "tha", "msa", "may", "hin", "ukr", "cze", "ces", "hun", "rum", "ron", "dut", "nld", "swe", "fin", "nor", "dan", "heb",
    "gre", "ell",
];

/// Parse a torrent or file name. Never fails; unknown parts stay `None` / `false`.
pub fn parse_release(name: &str) -> ReleaseTags {
    let base = name.rsplit(['/', '\\']).next().unwrap_or(name);
    let stem = strip_ext(base).trim();
    let mut tags = ReleaseTags::default();

    // 1. leading [Group] (a bracket that only holds tags is moved to the end instead)
    let mut text = stem.to_owned();
    if let Some((inner, rest)) = leading_bracket(stem) {
        if looks_like_tags(inner) {
            text = format!("{} [{inner}]", rest.trim());
        } else {
            if !inner.is_empty() {
                tags.group = Some(inner.to_owned());
            }
            text = rest.trim().to_owned();
        }
    }

    // 2. separators
    text = text.replace('_', " ").replace(['\u{2013}', '\u{2014}'], "-");
    let scene = !text.trim().contains(' ') && text.matches('.').count() >= 2;
    if scene {
        text = text.replace('.', " ");
    }
    if tags.group.is_none()
        && let Some(i) = text.rfind('-')
        && i > 0
        && !text[..i].ends_with(' ')
    {
        let tail = text[i + 1..].trim();
        let lower = tail.to_ascii_lowercase();
        if !tail.is_empty()
            && tail.len() <= 24
            && tail.bytes().all(|b| b.is_ascii_alphanumeric())
            && !tail.bytes().all(|b| b.is_ascii_digit())
            && !is_tag_word(&lower)
            && !matches!(lower.as_str(), "dl" | "rip" | "raws" | "audio" | "sub" | "subs" | "bit" | "bits" | "ray" | "hen")
        {
            tags.group = Some(tail.to_owned());
            text.truncate(i);
        }
    }

    // 3. words
    let sp = spans(&text);
    let mut tag_zone = text.find(['[', '(', '{', '\u{3010}']).unwrap_or(text.len());
    if let Some(s) = sp.iter().skip(1).find(|s| !s.bracketed && is_tag_word(&s.word)) {
        tag_zone = tag_zone.min(s.start);
    }
    let mut title_end = tag_zone;

    // 4. season / episode
    let mut season: Option<u32> = None;
    let mut episodes: Option<(u32, u32)> = None;
    let mut multi_season = false;
    for (i, s) in sp.iter().enumerate() {
        if let Some((se, ep)) = season_episode(&s.word) {
            if season.is_none() {
                season = Some(se);
            }
            if episodes.is_none() {
                let mut last = ep;
                if let Some(n) = sp.get(i + 1)
                    && range_gap(&text[s.end..n.start])
                    && let Some(b) = ep_word(&n.word).or_else(|| ep_number(&n.word))
                    && b > ep
                {
                    last = b;
                }
                episodes = Some((ep, last));
            }
            title_end = title_end.min(s.start);
            continue;
        }
        if let Some(se) = season_word(&s.word) {
            if season.is_none() {
                season = Some(se);
            }
            if let Some(n) = sp.get(i + 1) {
                let gap = &text[s.end..n.start];
                if episodes.is_none()
                    && gap.trim().is_empty()
                    && let Some(ep) = ep_word(&n.word)
                {
                    episodes = Some((ep, ep));
                }
                if range_gap(gap) && season_word(&n.word).is_some() {
                    multi_season = true;
                }
            }
            if !s.bracketed {
                title_end = title_end.min(s.start);
            }
            continue;
        }
        if matches!(s.word.as_str(), "season" | "seasons" | "saison" | "temporada") {
            let mut start = s.start;
            if let Some(n) = sp.get(i + 1)
                && text[s.end..n.start].trim().is_empty()
                && let Some(se) = digits(&n.word)
            {
                if season.is_none() {
                    season = Some(se);
                }
                if let Some(m) = sp.get(i + 2)
                    && range_gap(&text[n.end..m.start])
                    && digits(&m.word).is_some()
                {
                    multi_season = true;
                }
            }
            if let Some(p) = i.checked_sub(1).and_then(|j| sp.get(j))
                && let Some(se) = ordinal(&p.word)
            {
                if season.is_none() {
                    season = Some(se);
                }
                start = p.start;
            }
            if !s.bracketed {
                title_end = title_end.min(start);
            }
        }
    }
    // `Title - 05`, `Title - 01-12`, `Title - 01 ~ 12`
    if episodes.is_none() {
        for (i, s) in sp.iter().enumerate().skip(1) {
            if s.bracketed {
                continue;
            }
            let Some(a) = ep_number(&s.word) else { continue };
            let gap = text[sp[i - 1].end..s.start].trim_end();
            if !(gap.len() >= 2 && gap.ends_with('-') && gap[..gap.len() - 1].ends_with([' ', ')', ']'])) {
                continue;
            }
            let mut b = a;
            if let Some(n) = sp.get(i + 1)
                && range_gap(&text[s.end..n.start])
                && let Some(x) = ep_number(&n.word)
                && x > a
            {
                b = x;
            }
            episodes = Some((a, b));
            title_end = title_end.min(sp[i - 1].end);
            break;
        }
    }
    // bare range anywhere: `01-12`, `(01~24)`
    if episodes.is_none() {
        for i in 0..sp.len().saturating_sub(1) {
            let (s, n) = (&sp[i], &sp[i + 1]);
            if i == 0 && !s.bracketed {
                continue;
            }
            let (Some(a), Some(b)) = (ep_number(&s.word), ep_number(&n.word)) else { continue };
            if !range_gap(&text[s.end..n.start]) || a >= b || (s.word.len() == 4 && n.word.len() == 4) {
                continue;
            }
            if i > 0 && matches!(sp[i - 1].word.as_str(), "season" | "seasons" | "saison" | "temporada" | "part" | "vol" | "volume") {
                continue;
            }
            episodes = Some((a, b));
            if !s.bracketed {
                title_end = title_end.min(s.start);
            }
            break;
        }
    }
    // `EP05`, `E05`, `Episode 5`
    if episodes.is_none() {
        for (i, s) in sp.iter().enumerate() {
            let found = if matches!(s.word.as_str(), "ep" | "eps" | "episode" | "episodes" | "episodio" | "capitulo") {
                sp.get(i + 1).and_then(|n| ep_number(&n.word)).map(|a| (a, i + 1))
            } else {
                ep_word(&s.word).map(|a| (a, i))
            };
            let Some((a, last)) = found else { continue };
            let mut b = a;
            if let Some(n) = sp.get(last + 1)
                && range_gap(&text[sp[last].end..n.start])
                && let Some(x) = ep_word(&n.word).or_else(|| ep_number(&n.word))
                && x > a
            {
                b = x;
            }
            episodes = Some((a, b));
            if !s.bracketed {
                title_end = title_end.min(s.start);
            }
            break;
        }
    }
    // last bare number before the tags: `Show 05 [1080p]`
    if episodes.is_none() {
        let found = sp
            .iter()
            .enumerate()
            .skip(1)
            .filter(|(_, s)| !s.bracketed && s.start < tag_zone)
            .filter_map(|(i, s)| ep_number(&s.word).map(|a| (i, s, a)))
            .filter(|(i, s, a)| {
                let year = s.word.len() == 4 && (1900..=2100).contains(a);
                let prev = sp[*i - 1].word.as_str();
                let next = sp.get(*i + 1).map_or("", |n| n.word.as_str());
                !year
                    && !matches!(prev, "no" | "season" | "seasons" | "saison" | "temporada" | "part" | "vol" | "volume" | "cour" | "movie")
                    && !matches!(next, "season" | "seasons")
            })
            .last();
        if let Some((_, s, a)) = found {
            episodes = Some((a, a));
            title_end = title_end.min(s.start);
        }
    }

    // 5. title
    let title = text[..title_end.min(text.len())]
        .trim()
        .trim_end_matches(|c: char| matches!(c, '-' | '~' | ':' | ',' | '.') || c.is_whitespace())
        .trim();
    tags.title = (!title.is_empty()).then(|| title.to_owned());
    tags.season = season;
    tags.episodes = episodes;

    // 6. tags
    tags.resolution = sp
        .iter()
        .map(|s| {
            let q = resolution_of(&s.word);
            if q == Quality::Unknown && s.bracketed && matches!(s.word.as_str(), "480" | "720" | "1080" | "1440" | "2160") {
                Quality::parse(&s.word)
            } else {
                q
            }
        })
        .find(|q| *q != Quality::Unknown)
        .unwrap_or_default();
    tags.codec = sp.iter().enumerate().find_map(|(i, s)| {
        codec_word(&s.word).or_else(|| match (s.word.as_str(), sp.get(i + 1).map(|n| n.word.as_str())) {
            ("h" | "x", Some("264")) => Some(VideoCodec::H264),
            ("h" | "x", Some("265")) => Some(VideoCodec::Hevc),
            _ => None,
        })
    });
    tags.bit_depth = sp.iter().enumerate().find_map(|(i, s)| {
        let next = sp.get(i + 1).map_or("", |n| n.word.as_str());
        match s.word.as_str() {
            "hi10p" | "hi10" | "10bit" | "10bits" | "10bpp" | "main10" | "yuv420p10" | "yuv420p10le" => Some(10),
            "8bit" | "8bits" => Some(8),
            "12bit" | "12bits" => Some(12),
            "10" | "8" | "12" if matches!(next, "bit" | "bits") => s.word.parse::<u8>().ok(),
            _ => None,
        }
    });
    tags.dual_audio = sp.iter().enumerate().any(|(i, s)| {
        let next = sp.get(i + 1).map_or("", |n| n.word.as_str());
        matches!(s.word.as_str(), "dualaudio" | "multiaudio") || (matches!(s.word.as_str(), "dual" | "multi") && matches!(next, "audio" | "audios"))
    });
    let mut langs: Vec<&str> = Vec::new();
    for s in sp.iter().filter(|s| s.bracketed) {
        if SUB_LANGS.contains(&s.word.as_str()) && !langs.contains(&s.word.as_str()) {
            langs.push(s.word.as_str());
        }
    }
    tags.multi_sub = langs.len() > 2
        || sp.iter().enumerate().any(|(i, s)| {
            let next = sp.get(i + 1).map_or("", |n| n.word.as_str());
            matches!(s.word.as_str(), "multisub" | "multisubs" | "multisubtitle" | "multisubtitles")
                || (matches!(s.word.as_str(), "multi" | "multiple") && matches!(next, "sub" | "subs" | "subtitle" | "subtitles" | "subbed"))
        });
    tags.source = source_of(&sp, scene);
    if tags.source.is_none()
        && let Some(g) = &tags.group
        && WEB_GROUPS.contains(&g.to_ascii_lowercase().as_str())
    {
        tags.source = Some(ReleaseSource::Web);
    }
    let batch_word = sp.iter().any(|s| matches!(s.word.as_str(), "batch" | "complete" | "bdbox" | "boxset"));
    tags.batch = batch_word || multi_season || episodes.is_some_and(|(a, b)| b > a) || (season.is_some() && episodes.is_none());
    tags
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
///
/// Matching, in order:
/// * a torrent with a single (non-extra) video file plays it, unless its name states another episode;
/// * files whose parsed episode (or range) holds `episode`: single episode 100, multi-episode file 50; +20 when the
///   season (from the name or a parent folder) equals the one the titles ask for (`Season 2`, `2nd Season`, `S2`,
///   `II`), rejected when it differs (a second pass ignores seasons), -10 for a season > 1 when the titles say none;
///   +10 when the file title matches one of `titles`; ties -> largest file;
/// * absolute numbering: a pack whose numbers start above 1 (`13..24`) maps episode 1 to its first number.
pub fn pick_episode_file(files: &[CandidateFile], episode: u32, titles: &[String]) -> Option<usize> {
    let title_norms: Vec<String> = titles.iter().map(|t| norm_title(t)).filter(|t| !t.is_empty()).collect();
    let wants_extras =
        titles.iter().any(|t| words(t).iter().any(|w| matches!(w.as_str(), "ova" | "oad" | "special" | "specials" | "sp" | "omake")));
    let all_videos: Vec<&CandidateFile> = files.iter().filter(|f| is_video_path(&f.path)).collect();
    let videos: Vec<(&CandidateFile, ReleaseTags)> = all_videos
        .iter()
        .filter(|f| !is_extra(&f.path, wants_extras, &title_norms))
        .map(|f| (*f, file_tags(&f.path)))
        .collect();
    if let [(only, tags)] = videos.as_slice()
        && tags.episodes.is_none_or(|(a, b)| a <= episode && episode <= b)
    {
        return Some(only.index);
    }
    if videos.is_empty()
        && let [only] = all_videos.as_slice()
    {
        return Some(only.index);
    }
    let season = titles.iter().find_map(|t| title_season(t));
    let best = |strict: bool| -> Option<usize> {
        videos
            .iter()
            .filter_map(|(f, t)| {
                let (a, b) = t.episodes?;
                if episode < a || episode > b {
                    return None;
                }
                let mut score: i32 = if a == b { 100 } else { 50 };
                match (season, t.season) {
                    (Some(want), Some(have)) if want == have => score += 20,
                    (Some(_), Some(_)) if strict => return None,
                    (None, Some(have)) if have > 1 => score -= 10,
                    _ => {}
                }
                if title_matches(t.title.as_deref(), &title_norms) {
                    score += 10;
                }
                Some((score, f.size_bytes, f.index))
            })
            .max_by(|x, y| x.0.cmp(&y.0).then(x.1.cmp(&y.1)).then(y.2.cmp(&x.2)))
            .map(|x| x.2)
    };
    if let Some(i) = best(true).or_else(|| best(false)) {
        return Some(i);
    }
    // absolute numbering inside a later-season pack: `Show - 13.mkv` .. `Show - 24.mkv` for episodes 1..12
    let singles: Vec<(&CandidateFile, u32)> = videos
        .iter()
        .filter(|(_, t)| season.is_none() || t.season.is_none() || t.season == season)
        .filter_map(|(f, t)| t.episodes.filter(|(a, b)| a == b).map(|(a, _)| (*f, a)))
        .collect();
    if singles.len() >= 2 {
        let first = singles.iter().map(|(_, a)| *a).min()?;
        if first > 1 {
            let target = episode.checked_add(first - 1)?;
            return singles.iter().filter(|(_, a)| *a == target).max_by_key(|(f, _)| f.size_bytes).map(|(f, _)| f.index);
        }
    }
    None
}

// ---------------------------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------------------------

/// One alphanumeric run of a (normalized) name: byte range, lowercase text, and whether it sits inside
/// `[...]` / `(...)` / `{...}` / `【...】`.
#[derive(Debug, Clone)]
struct Span {
    start: usize,
    end: usize,
    word: String,
    bracketed: bool,
}

fn spans(s: &str) -> Vec<Span> {
    let mut out = Vec::new();
    let mut depth: u32 = 0;
    let mut cur: Option<(usize, bool)> = None;
    for (i, c) in s.char_indices() {
        if c.is_alphanumeric() {
            if cur.is_none() {
                cur = Some((i, depth > 0));
            }
            continue;
        }
        if let Some((start, bracketed)) = cur.take() {
            out.push(Span { start, end: i, word: s[start..i].to_lowercase(), bracketed });
        }
        match c {
            '[' | '(' | '{' | '\u{3010}' => depth += 1,
            ']' | ')' | '}' | '\u{3011}' => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    if let Some((start, bracketed)) = cur {
        out.push(Span { start, end: s.len(), word: s[start..].to_lowercase(), bracketed });
    }
    out
}

/// Lowercase alphanumeric words.
fn words(s: &str) -> Vec<String> {
    spans(s).into_iter().map(|s| s.word).collect()
}

/// Lowercase words joined by one space (`"Re:Zero - 2nd Season"` -> `"re zero 2nd season"`).
fn norm_title(s: &str) -> String {
    words(s).join(" ")
}

fn title_matches(title: Option<&str>, title_norms: &[String]) -> bool {
    let Some(t) = title else { return false };
    let n = norm_title(t);
    !n.is_empty() && title_norms.iter().any(|x| x.contains(&n) || n.contains(x.as_str()))
}

/// The season a series title names (`Season 3`, `2nd Season`, `S2`, a trailing `II` / `III` / `IV`).
fn title_season(title: &str) -> Option<u32> {
    if let Some(s) = parse_release(title).season {
        return Some(s);
    }
    match words(title).last().map(String::as_str) {
        Some("ii") => Some(2),
        Some("iii") => Some(3),
        Some("iv") => Some(4),
        Some("vi") => Some(6),
        _ => None,
    }
}

/// Parsed tags of a torrent file; the season falls back to the nearest parent folder (`Season 2/`, `Show S02/`).
fn file_tags(path: &str) -> ReleaseTags {
    let mut segs: Vec<&str> = path.split(['/', '\\']).filter(|s| !s.is_empty()).collect();
    let file = segs.pop().unwrap_or(path);
    let mut tags = parse_release(file);
    if tags.season.is_none() {
        tags.season = segs.iter().rev().find_map(|d| parse_release(d).season);
    }
    tags
}

/// Sample / NCOP / NCED / PV / CM / menu / extras file or folder (OVA / SP / specials too unless asked for).
fn is_extra(path: &str, wants_extras: bool, title_norms: &[String]) -> bool {
    let segs: Vec<&str> = path.split(['/', '\\']).filter(|s| !s.is_empty()).collect();
    let Some((file, folders)) = segs.split_last() else { return false };
    let in_titles = |w: &str| title_norms.iter().any(|t| t.split(' ').any(|x| x == w));
    let hit = |w: &String| w.starts_with("sample") || (is_extra_word(w, wants_extras) && !in_titles(w.as_str()));
    if words(strip_ext(file)).iter().any(|w| hit(w)) {
        return true;
    }
    // long folder names are torrent names ("Show + Extras [BD]"), not extras folders
    folders.iter().any(|f| {
        let ws = words(f);
        ws.len() <= 3 && ws.iter().any(|w| hit(w))
    })
}

fn is_extra_word(w: &str, wants_extras: bool) -> bool {
    let stem = w.trim_end_matches(|c: char| c.is_ascii_digit());
    match stem {
        "sample" | "samples" | "ncop" | "nced" | "nc" | "creditless" | "pv" | "cm" | "cms" | "preview" | "previews" | "trailer"
        | "trailers" | "teaser" | "menu" | "menus" | "extra" | "extras" | "bonus" | "featurette" | "featurettes" | "interview" => true,
        "sp" | "sps" | "ova" | "ovas" | "oad" | "special" | "specials" | "omake" => !wants_extras,
        _ => false,
    }
}

/// Drops a known media / torrent extension (`.mkv`, `.torrent` ...); anything else stays.
fn strip_ext(name: &str) -> &str {
    if let Some((stem, ext)) = name.rsplit_once('.')
        && !stem.is_empty()
    {
        let ext = ext.to_ascii_lowercase();
        if is_video_path(name) || matches!(ext.as_str(), "torrent" | "mka" | "ass" | "srt" | "ssa" | "m4v") {
            return stem;
        }
    }
    name
}

/// `[inner]` (or `【inner】`, `(inner)`) at the start of `s` and the rest after it.
fn leading_bracket(s: &str) -> Option<(&str, &str)> {
    let t = s.trim_start();
    let open = t.chars().next()?;
    let close = match open {
        '[' => ']',
        '(' => ')',
        '\u{3010}' => '\u{3011}',
        _ => return None,
    };
    let inner_start = open.len_utf8();
    let end = inner_start + t[inner_start..].find(close)?;
    Some((t[inner_start..end].trim(), &t[end + close.len_utf8()..]))
}

/// `[1080p]`, `[BD 1080p HEVC]`: a bracket made only of tag words is not a group.
fn looks_like_tags(inner: &str) -> bool {
    let ws = words(inner);
    !ws.is_empty() && ws.iter().all(|w| is_tag_word(w))
}

/// A word that belongs to the tag part of a name (resolution, codec, source, audio, bit depth, batch).
fn is_tag_word(w: &str) -> bool {
    resolution_of(w) != Quality::Unknown
        || codec_word(w).is_some()
        || matches!(
            w,
            "remux" | "bdremux" | "bd" | "bdrip" | "bluray" | "bdmv" | "brrip" | "web" | "webrip" | "webdl" | "hdtv" | "tvrip" | "dvd"
                | "dvdrip" | "amzn"
        )
        || matches!(
            w,
            "hi10p" | "hi10" | "10bit" | "10bits" | "8bit" | "8bits" | "12bit" | "aac" | "flac" | "opus" | "ac3" | "eac3" | "ddp" | "dts"
                | "truehd" | "batch" | "multisub" | "multisubs" | "dualaudio"
        )
}

/// `1080p`, `720i`, `1920x1080`, `BD1080p`, `4K`, `UHD`, `FHD`.
fn resolution_of(w: &str) -> Quality {
    let w = w.strip_prefix("bd").filter(|r| r.ends_with('p')).unwrap_or(w);
    if let Some(d) = w.strip_suffix('p').or_else(|| w.strip_suffix('i'))
        && (3..=4).contains(&d.len())
        && d.bytes().all(|b| b.is_ascii_digit())
    {
        return match d {
            "576" | "540" | "360" => Quality::P480,
            _ => Quality::parse(d),
        };
    }
    if let Some((a, b)) = w.split_once('x')
        && (3..=4).contains(&a.len())
        && (3..=4).contains(&b.len())
        && a.bytes().chain(b.bytes()).all(|c| c.is_ascii_digit())
    {
        return Quality::parse(b);
    }
    match w {
        "4k" | "uhd" => Quality::P2160,
        "fhd" | "fullhd" => Quality::P1080,
        _ => Quality::Unknown,
    }
}

fn codec_word(w: &str) -> Option<VideoCodec> {
    match w {
        "av1" => Some(VideoCodec::Av1),
        "x265" | "h265" | "hevc" => Some(VideoCodec::Hevc),
        "vp9" => Some(VideoCodec::Vp9),
        "x264" | "h264" | "avc" => Some(VideoCodec::H264),
        _ => None,
    }
}

/// `REMUX` wins over any other source word; otherwise the first one. Streaming-service codes that are also
/// common words (`NF`, `CR`, `ADN` ...) only count inside brackets or in scene names.
fn source_of(sp: &[Span], scene: bool) -> Option<ReleaseSource> {
    let mut found: Option<ReleaseSource> = None;
    for (i, s) in sp.iter().enumerate() {
        let next = sp.get(i + 1).map_or("", |n| n.word.as_str());
        let tagged = s.bracketed || scene;
        let src = match s.word.as_str() {
            "remux" | "bdremux" => Some(ReleaseSource::Remux),
            "dvdremux" => Some(ReleaseSource::Dvd),
            "bd" | "bdrip" | "bluray" | "bdmv" | "brrip" | "bdr" => Some(ReleaseSource::Bd),
            "blu" if next == "ray" => Some(ReleaseSource::Bd),
            "web" | "webrip" | "webdl" | "amzn" | "crunchyroll" | "funimation" | "hidive" => Some(ReleaseSource::Web),
            "nf" | "cr" | "dsnp" | "adn" | "baha" | "bilibili" | "abema" if tagged => Some(ReleaseSource::Web),
            "hdtv" | "tvrip" | "hdtvrip" | "dtv" => Some(ReleaseSource::Tv),
            "dvd" | "dvdrip" | "dvd5" | "dvd9" | "r2dvd" | "r1dvd" | "dvdr" => Some(ReleaseSource::Dvd),
            w if w.starts_with("bd") && resolution_of(&w[2..]) != Quality::Unknown => Some(ReleaseSource::Bd),
            _ => None,
        };
        match src {
            Some(ReleaseSource::Remux) => return Some(ReleaseSource::Remux),
            Some(x) if found.is_none() => found = Some(x),
            _ => {}
        }
    }
    found
}

/// `-`, `~` (with any spaces) between the two ends of a range.
fn range_gap(gap: &str) -> bool {
    matches!(gap.trim(), "-" | "~" | "\u{301c}" | "\u{ff5e}")
}

/// 1 to 4 ASCII digits.
fn digits(w: &str) -> Option<u32> {
    if w.is_empty() || w.len() > 4 || !w.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    w.parse().ok()
}

/// An episode number, tolerating a version suffix (`05`, `05v2`).
fn ep_number(w: &str) -> Option<u32> {
    let base = match w.rfind('v') {
        Some(i) if i > 0 && i + 1 < w.len() && w[i + 1..].bytes().all(|b| b.is_ascii_digit()) => &w[..i],
        _ => w,
    };
    digits(base)
}

/// `ep05`, `e05`, `e5v2`.
fn ep_word(w: &str) -> Option<u32> {
    w.strip_prefix("ep").or_else(|| w.strip_prefix('e')).and_then(ep_number)
}

/// `s01e05` / `s1e5v2` -> `(season, episode)`.
fn season_episode(w: &str) -> Option<(u32, u32)> {
    let rest = w.strip_prefix('s')?;
    let e = rest.find('e')?;
    if e == 0 || e > 2 {
        return None;
    }
    let season = digits(&rest[..e])?;
    Some((season, ep_number(&rest[e + 1..])?))
}

/// `s01`, `s2`.
fn season_word(w: &str) -> Option<u32> {
    w.strip_prefix('s').filter(|d| d.len() <= 2).and_then(digits)
}

/// `2nd`, `3rd`, `4th`, `1st`.
fn ordinal(w: &str) -> Option<u32> {
    ["st", "nd", "rd", "th"].iter().find_map(|suf| w.strip_suffix(*suf)).and_then(digits)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(index: usize, path: &str, size_bytes: u64) -> CandidateFile {
        CandidateFile { index, path: path.to_owned(), size_bytes }
    }

    fn titles(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn single_video_file_is_picked() {
        let files = vec![
            CandidateFile { index: 0, path: "Show/info.nfo".to_owned(), size_bytes: 10 },
            CandidateFile { index: 1, path: "Show/[Group] Show - 01 (1080p).mkv".to_owned(), size_bytes: 1 << 30 },
        ];
        assert_eq!(pick_episode_file(&files, 1, &["Show".to_owned()]), Some(1));
        assert!(is_video_path("a/B.MKV") && !is_video_path("a/b.ass") && !is_video_path("noext"));
    }

    #[test]
    fn subsplease_single_episode() {
        let t = parse_release("[SubsPlease] Sousou no Frieren - 12 (1080p) [A1B2C3D4].mkv");
        assert_eq!(t.group.as_deref(), Some("SubsPlease"));
        assert_eq!(t.title.as_deref(), Some("Sousou no Frieren"));
        assert_eq!((t.season, t.episodes), (None, Some((12, 12))));
        assert_eq!((t.resolution, t.source, t.codec), (Quality::P1080, Some(ReleaseSource::Web), None));
        assert!(!t.batch && !t.dual_audio && !t.multi_sub && !t.seadex_best);
    }

    #[test]
    fn erai_raws_multi_subs_and_dashed_title() {
        let t = parse_release(
            "[Erai-raws] Kimetsu no Yaiba - Hashira Geiko-hen - 03 [1080p CR WEB-DL AVC AAC][MultiSub][ENG][POR-BR][SPA-LA][SPA][ARA][FRE][GER][ITA][RUS]",
        );
        assert_eq!(t.group.as_deref(), Some("Erai-raws"));
        assert_eq!(t.title.as_deref(), Some("Kimetsu no Yaiba - Hashira Geiko-hen"));
        assert_eq!(t.episodes, Some((3, 3)));
        assert_eq!((t.resolution, t.source, t.codec), (Quality::P1080, Some(ReleaseSource::Web), Some(VideoCodec::H264)));
        assert!(t.multi_sub && !t.batch);
        // language codes alone (no MultiSub word) are enough when there are more than two
        assert!(parse_release("[Erai-raws] Show - 01 [720p][ENG][SPA][POR-BR]").multi_sub);
        assert!(!parse_release("[Erai-raws] Show - 01 [720p][ENG][SPA]").multi_sub);
    }

    #[test]
    fn judas_season_batch() {
        let t = parse_release("[Judas] Kimetsu no Yaiba S03 (BD 1080p HEVC 10-bit x265 Dual Audio) [Batch]");
        assert_eq!(t.group.as_deref(), Some("Judas"));
        assert_eq!(t.title.as_deref(), Some("Kimetsu no Yaiba"));
        assert_eq!((t.season, t.episodes), (Some(3), None));
        assert_eq!((t.source, t.resolution, t.codec, t.bit_depth), (Some(ReleaseSource::Bd), Quality::P1080, Some(VideoCodec::Hevc), Some(10)));
        assert!(t.dual_audio && t.batch);
    }

    #[test]
    fn asw_hevc_10bit() {
        let t = parse_release("[ASW] Dandadan - 05 [1080p HEVC x265 10Bit][AAC]");
        assert_eq!((t.group.as_deref(), t.title.as_deref(), t.episodes), (Some("ASW"), Some("Dandadan"), Some((5, 5))));
        assert_eq!((t.codec, t.bit_depth, t.source), (Some(VideoCodec::Hevc), Some(10), Some(ReleaseSource::Web)));
        let t = parse_release("[Group] Show - 01 [Hi10P 720p]");
        assert_eq!((t.bit_depth, t.resolution), (Some(10), Quality::P720));
    }

    #[test]
    fn ember_bdrip_season_pack() {
        let t = parse_release("[EMBER] Frieren (2023) (Season 1) [BDRip] [1080p Dual Audio HEVC 10 bits DDP] (Sousou no Frieren)");
        assert_eq!((t.group.as_deref(), t.title.as_deref()), (Some("EMBER"), Some("Frieren")));
        assert_eq!((t.season, t.episodes), (Some(1), None));
        assert_eq!((t.source, t.bit_depth, t.codec), (Some(ReleaseSource::Bd), Some(10), Some(VideoCodec::Hevc)));
        assert!(t.dual_audio && t.batch);
    }

    #[test]
    fn commie_version_suffix() {
        let t = parse_release("[Commie] Show - 01v2 [ABCD1234].mkv");
        assert_eq!((t.group.as_deref(), t.title.as_deref(), t.episodes), (Some("Commie"), Some("Show"), Some((1, 1))));
        assert_eq!(t.source, None);
    }

    #[test]
    fn scene_and_web_dl_names() {
        let t = parse_release("Sousou no Frieren S01E05 1080p WEB-DL AAC2.0 H.264-VARYG");
        assert_eq!((t.group.as_deref(), t.title.as_deref()), (Some("VARYG"), Some("Sousou no Frieren")));
        assert_eq!((t.season, t.episodes), (Some(1), Some((5, 5))));
        assert_eq!((t.source, t.codec, t.resolution), (Some(ReleaseSource::Web), Some(VideoCodec::H264), Quality::P1080));
        assert!(!t.batch);
        let t = parse_release("Sousou.no.Frieren.S01E05.1080p.NF.WEB.H264-GRP.mkv");
        assert_eq!((t.group.as_deref(), t.title.as_deref()), (Some("GRP"), Some("Sousou no Frieren")));
        assert_eq!((t.season, t.episodes, t.source), (Some(1), Some((5, 5)), Some(ReleaseSource::Web)));
        let t = parse_release("Show.S02.1080p.BluRay.REMUX.AVC.FLAC.2.0-GRP");
        assert_eq!((t.season, t.episodes, t.source, t.batch), (Some(2), None, Some(ReleaseSource::Remux), true));
    }

    #[test]
    fn ranges_and_complete() {
        let t = parse_release("[Group] Show 01-12 [BD 1080p]");
        assert_eq!((t.title.as_deref(), t.episodes, t.batch), (Some("Show"), Some((1, 12)), true));
        let t = parse_release("[Group] Show - 01 ~ 12 (Complete) [720p]");
        assert_eq!((t.episodes, t.batch, t.resolution), (Some((1, 12)), true, Quality::P720));
        let t = parse_release("[Group] Show (01-24) [1080p x264]");
        assert_eq!((t.title.as_deref(), t.episodes, t.batch), (Some("Show"), Some((1, 24)), true));
        let t = parse_release("[Group] Show - 01~12 [Batch]");
        assert_eq!((t.episodes, t.batch), (Some((1, 12)), true));
        let t = parse_release("[Group] Show Complete Series [1080p]");
        assert!(t.batch && t.episodes.is_none());
    }

    #[test]
    fn remux_av1_and_misc() {
        assert_eq!(parse_release("[Group] Show [BD REMUX 1080p]").source, Some(ReleaseSource::Remux));
        let t = parse_release("[Group] Show - 03 [1080p AV1 OPUS][Multi-Subs]");
        assert_eq!((t.codec, t.multi_sub), (Some(VideoCodec::Av1), true));
        assert_eq!(parse_release("[Group] One Piece - 1071 [1080p]").episodes, Some((1071, 1071)));
        let t = parse_release("Kaiju No. 8 - 05 (1080p)");
        assert_eq!((t.group, t.title.as_deref(), t.episodes), (None, Some("Kaiju No. 8"), Some((5, 5))));
        let t = parse_release("[SubsPlease] Re Zero kara Hajimeru Isekai Seikatsu 3rd Season - 05 (720p)");
        assert_eq!((t.title.as_deref(), t.season, t.episodes), (Some("Re Zero kara Hajimeru Isekai Seikatsu"), Some(3), Some((5, 5))));
        let t = parse_release("[Group] 86 - Eighty Six - 03 [WEBRip 1080p]");
        assert_eq!((t.title.as_deref(), t.episodes, t.source), (Some("86 - Eighty Six"), Some((3, 3)), Some(ReleaseSource::Web)));
        let t = parse_release("[Group]_Show_-_07_[720p].mkv");
        assert_eq!((t.title.as_deref(), t.episodes), (Some("Show"), Some((7, 7))));
        let t = parse_release("[Group] Show EP07 [DVDRip]");
        assert_eq!((t.episodes, t.source), (Some((7, 7)), Some(ReleaseSource::Dvd)));
        let t = parse_release("[1080p] Show - 02");
        assert_eq!((t.group, t.title.as_deref(), t.resolution), (None, Some("Show"), Quality::P1080));
    }

    #[test]
    fn never_fails() {
        for name in ["", " ", "-", "[]", "[", "]", "()", "\u{3010}\u{3011}", "[Group]", "....", "s", "e", "- - -", "01-", "~12", "v2", "Ünïcödé 名前 - 05"] {
            let _ = parse_release(name);
        }
        assert_eq!(parse_release("Ünïcödé 名前 - 05").episodes, Some((5, 5)));
        assert_eq!(parse_release("").title, None);
    }

    #[test]
    fn season_pack_skips_extras_and_samples() {
        let files = vec![
            file(0, "[Judas] Show S03/[Judas] Show - S03E01.mkv", 900),
            file(1, "[Judas] Show S03/[Judas] Show - S03E02.mkv", 900),
            file(2, "[Judas] Show S03/Extras/[Judas] Show - NCOP 02.mkv", 50),
            file(3, "[Judas] Show S03/[Judas] Show - S03E02 sample.mkv", 5),
            file(4, "[Judas] Show S03/[Judas] Show - S03E02.ass", 1),
            file(5, "[Judas] Show S03/[Judas] Show - NCED2.mkv", 40),
        ];
        assert_eq!(pick_episode_file(&files, 2, &titles(&["Show Season 3"])), Some(1));
        assert_eq!(pick_episode_file(&files, 2, &titles(&["Show"])), Some(1));
        assert_eq!(pick_episode_file(&files, 1, &titles(&["Show"])), Some(0));
        assert_eq!(pick_episode_file(&files, 9, &titles(&["Show"])), None);
    }

    #[test]
    fn absolute_numbering_pack() {
        let files: Vec<CandidateFile> = (13..=24).map(|n: usize| file(n - 13, &format!("Show S2/[Group] Show - {n}.mkv"), 1000)).collect();
        assert_eq!(pick_episode_file(&files, 1, &titles(&["Show 2nd Season"])), Some(0));
        assert_eq!(pick_episode_file(&files, 12, &titles(&["Show 2nd Season"])), Some(11));
        // absolute number asked directly
        assert_eq!(pick_episode_file(&files, 14, &titles(&["Show"])), Some(1));
        assert_eq!(pick_episode_file(&files, 13 + 12, &titles(&["Show"])), None);
    }

    #[test]
    fn multi_season_pack_uses_the_titles_season() {
        let files = vec![
            file(0, "Show/Season 1/[Group] Show - 01 [1080p].mkv", 1000),
            file(1, "Show/Season 2/[Group] Show - 01 [1080p].mkv", 1000),
            file(2, "Show/Season 1/[Group] Show - 02 [1080p].mkv", 1000),
        ];
        assert_eq!(pick_episode_file(&files, 1, &titles(&["Show Season 2", "Show II"])), Some(1));
        assert_eq!(pick_episode_file(&files, 1, &titles(&["Show II"])), Some(1));
        assert_eq!(pick_episode_file(&files, 1, &titles(&["Show"])), Some(0));
        // season 3 is not in the pack: the second pass ignores seasons
        assert_eq!(pick_episode_file(&files, 2, &titles(&["Show Season 3"])), Some(2));
    }

    #[test]
    fn ties_prefer_the_largest_file_and_ranges_match() {
        let files = vec![
            file(7, "[A] Show - 03 [720p].mkv", 300),
            file(8, "[A] Show - 03 [1080p].mkv", 900),
            file(9, "[A] Show - 04-05 [1080p].mkv", 1800),
        ];
        assert_eq!(pick_episode_file(&files, 3, &titles(&["Show"])), Some(8));
        assert_eq!(pick_episode_file(&files, 5, &titles(&["Show"])), Some(9));
        assert_eq!(pick_episode_file(&[], 1, &titles(&["Show"])), None);
        // a single file whose name says another episode is not played blindly
        assert_eq!(pick_episode_file(&files[..1], 4, &titles(&["Show"])), None);
    }

    #[test]
    fn ova_files_are_kept_when_the_titles_ask_for_them() {
        let files = vec![file(0, "Show/[G] Show - OVA 01.mkv", 500), file(1, "Show/[G] Show - 01.mkv", 500), file(2, "Show/[G] Show - 02.mkv", 500)];
        assert_eq!(pick_episode_file(&files, 1, &titles(&["Show"])), Some(1));
        assert!(pick_episode_file(&files, 1, &titles(&["Show OVA"])).is_some());
    }
}
