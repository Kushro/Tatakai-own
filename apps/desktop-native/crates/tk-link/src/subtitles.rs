//! 0.4.0: the source's own subtitles on the phone while the PC processes the video.
//!
//! A session's HLS carries video + one audio stream only, so a phone that switches to "PC" (Media3) or "Mixto" (libmpv)
//! lost the subtitles embedded in the source (a torrent MKV with several ASS tracks, typically). The PC now lists them in
//! `GET /v1/sessions/{id}` (`subtitles`, `subtitles_state`) and serves each text track as
//! `GET /v1/sessions/{id}/subtitles/{n}.ass` (ASS / SSA sources, styles kept, for libmpv) and `.../{n}.vtt` (any text
//! track, for Media3), with the same mTLS as the rest of the session. Image tracks (PGS, VobSub, DVB) are listed with
//! `text: false` and are not served.
//!
//! The approach follows Enigmata-Temple's `GET /v1/sessions/{id}/subtitles/{n}.vtt` (`crates/temple-core/src/library.rs`
//! `extract_vtt_from`, `media/probe.rs`; AGPL-3.0, same author): `ffprobe` lists the tracks, FFmpeg extracts them, the
//! language / title are cleaned up and capped before they reach the contract. Tatakai differs in two ways:
//!
//! * **One FFmpeg pass extracts every text track** (both formats) the first time the phone asks for any of them, and the
//!   result is kept in RAM for the source ([`SubtitleStore`]). A Matroska file interleaves its subtitles with the video,
//!   so extracting means reading the whole file; with a torrent the PC is still downloading, that is minutes, and
//!   reading it once per track (Temple) would multiply it. While the pass runs the files answer `503
//!   segment_wait_timeout` + `Retry-After`, and the phone adds the tracks once they are ready.
//! * The store belongs to the source, not to one session: a profile change / renewal (`source: session`) reuses the
//!   store of the session it replaces, so a profile change does not extract (or download) everything again.
//!
//! **Timing**: the extracted files keep the source's own timestamps (FFmpeg's default: relative to the source's start
//! time). The session's HLS uses the same timeline whatever its `start_secs` (segment `k` starts at `k * 2` s through
//! `-output_ts_offset`, see `media::args`), so the phone adds the files as they are, with no offset, on Media3 and on
//! libmpv alike.
//!
//! Limits: at most [`MAX_LISTED`] tracks listed and [`MAX_EXTRACTED`] extracted, [`MAX_FILE_BYTES`] per file (a bigger
//! one answers `503 source_unavailable`), [`MAX_STORE_BYTES`] per store, one pass per store at a time, [`PROBE_TIMEOUT`]
//! for the listing and [`EXTRACT_TIMEOUT`] for the pass. Nothing about a track is logged but its number.

use std::collections::HashMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use bytes::Bytes;
use serde::{Deserialize, Serialize};
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;

use crate::media::args::PROTOCOL_WHITELIST;

/// Tracks listed in the session status at most (the others exist but are not offered).
pub const MAX_LISTED: usize = 64;
/// Text tracks extracted at most (anime releases carry up to ~15).
pub const MAX_EXTRACTED: usize = 24;
/// Size limit of one extracted file (FFmpeg `-fs`); a track that reaches it is not served.
pub const MAX_FILE_BYTES: u64 = 8 * 1024 * 1024;
/// RAM kept for the files of one source; the tracks past it are not served.
pub const MAX_STORE_BYTES: usize = 48 * 1024 * 1024;
/// Time limit of the `ffprobe` listing (it reads the container header only).
pub const PROBE_TIMEOUT: Duration = Duration::from_secs(20);
/// Time limit of the extraction pass (a torrent MKV still downloading on the PC is read whole).
pub const EXTRACT_TIMEOUT: Duration = Duration::from_secs(15 * 60);
/// How long a file request waits for the pass before it answers `503` + `Retry-After` (below the phones' 20 s read
/// timeout, like the segment wait).
pub const REQUEST_WAIT: Duration = Duration::from_secs(8);
/// Characters of a track title reported at most.
pub const MAX_TITLE_CHARS: usize = 120;
/// Bytes of a track language reported at most.
pub const MAX_LANGUAGE_BYTES: usize = 16;

/// Text codecs FFmpeg decodes into ASS events, so both outputs can be made from them.
const TEXT_CODECS: [&str; 15] = [
    "ass",
    "ssa",
    "subrip",
    "srt",
    "webvtt",
    "mov_text",
    "text",
    "microdvd",
    "subviewer",
    "subviewer1",
    "sami",
    "realtext",
    "mpl2",
    "vplayer",
    "pjs",
];

/// Codecs whose styling survives in the `.ass` output.
const ASS_CODECS: [&str; 2] = ["ass", "ssa"];

/// The two files a text track is served as.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SubFormat {
    /// `.ass`: only for ASS / SSA sources (styles, positions and fonts kept; libmpv renders them with libass).
    Ass,
    /// `.vtt`: any text track (Media3 renders WebVTT; styles are the player's own).
    Vtt,
}

impl SubFormat {
    pub fn ext(self) -> &'static str {
        match self {
            Self::Ass => "ass",
            Self::Vtt => "vtt",
        }
    }

    pub fn content_type(self) -> &'static str {
        match self {
            Self::Ass => "text/x-ssa; charset=utf-8",
            Self::Vtt => "text/vtt; charset=utf-8",
        }
    }

    fn encoder(self) -> &'static str {
        match self {
            Self::Ass => "ass",
            Self::Vtt => "webvtt",
        }
    }
}

/// `"<n>.ass"` / `"<n>.vtt"` of `GET .../subtitles/{file}` -> `(n, format)`; anything else is `None` (404).
pub fn parse_file_name(file: &str) -> Option<(u32, SubFormat)> {
    let (n, ext) = file.split_once('.')?;
    if n.is_empty() || n.len() > 3 || !n.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let format = match ext {
        "ass" => SubFormat::Ass,
        "vtt" => SubFormat::Vtt,
        _ => return None,
    };
    Some((n.parse().ok()?, format))
}

/// One subtitle track of the source, as `GET /v1/sessions/{id}` lists it. `index` is the position among the source's
/// subtitle tracks (`-map 0:s:<index>`, the `n` of the file URLs).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct SubtitleTrack {
    pub index: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    pub codec: String,
    pub default: bool,
    pub forced: bool,
    /// `false` for image tracks (PGS, VobSub, DVB...), which are listed but not served.
    pub text: bool,
    /// The files served for it: `["ass","vtt"]` for ASS / SSA, `["vtt"]` for other text tracks, `[]` for image ones or
    /// past [`MAX_EXTRACTED`].
    pub formats: Vec<&'static str>,
}

impl SubtitleTrack {
    pub fn offers(&self, format: SubFormat) -> bool {
        self.formats.contains(&format.ext())
    }
}

/// `ffprobe` arguments (without the program) that list the subtitle streams of the loopback input as JSON.
pub fn ffprobe_args(input_url: &str) -> Vec<String> {
    [
        "-v",
        "error",
        "-protocol_whitelist",
        PROTOCOL_WHITELIST,
        "-select_streams",
        "s",
        "-show_entries",
        "stream=codec_name:stream_tags=language,title:stream_disposition=default,forced",
        "-print_format",
        "json",
        "-i",
        input_url,
    ]
    .iter()
    .map(|s| (*s).to_owned())
    .collect()
}

/// The subtitle tracks in `ffprobe -select_streams s -print_format json` output, in order; `None` when it is not that.
pub fn parse_ffprobe(stdout: &str) -> Option<Vec<SubtitleTrack>> {
    #[derive(Deserialize, Default)]
    struct Disposition {
        #[serde(default)]
        default: u8,
        #[serde(default)]
        forced: u8,
    }
    #[derive(Deserialize, Default)]
    struct Tags {
        language: Option<String>,
        title: Option<String>,
    }
    #[derive(Deserialize)]
    struct Stream {
        codec_name: Option<String>,
        #[serde(default)]
        disposition: Disposition,
        #[serde(default)]
        tags: Tags,
    }
    #[derive(Deserialize)]
    struct Out {
        #[serde(default)]
        streams: Vec<Stream>,
    }
    let out: Out = serde_json::from_str(stdout).ok()?;
    let mut extracted = 0usize;
    Some(
        out.streams
            .into_iter()
            .take(MAX_LISTED)
            .zip(0u32..)
            .map(|(s, index)| {
                let codec: String = s
                    .codec_name
                    .unwrap_or_default()
                    .chars()
                    .filter(|c| c.is_ascii_alphanumeric() || *c == '_')
                    .take(32)
                    .collect();
                let text = TEXT_CODECS.contains(&codec.as_str());
                let mut formats = Vec::new();
                if text && extracted < MAX_EXTRACTED {
                    extracted += 1;
                    if ASS_CODECS.contains(&codec.as_str()) {
                        formats.push(SubFormat::Ass.ext());
                    }
                    formats.push(SubFormat::Vtt.ext());
                }
                SubtitleTrack {
                    index,
                    language: clean_language(s.tags.language.as_deref()),
                    title: clean_title(s.tags.title.as_deref()),
                    codec: if codec.is_empty() { "unknown".to_owned() } else { codec },
                    default: s.disposition.default != 0,
                    forced: s.disposition.forced != 0,
                    text,
                    formats,
                }
            })
            .collect(),
    )
}

/// The file's language tag in lowercase (`spa`, `en`, `pt-br`); `und` and anything that is not a short tag is dropped.
fn clean_language(raw: Option<&str>) -> Option<String> {
    let l = raw?.trim().to_ascii_lowercase();
    let ok = !l.is_empty()
        && l.len() <= MAX_LANGUAGE_BYTES
        && l.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_');
    (ok && l != "und").then_some(l)
}

/// The file's track title without control characters, trimmed, at most [`MAX_TITLE_CHARS`]; empty is dropped.
fn clean_title(raw: Option<&str>) -> Option<String> {
    let t: String =
        raw?.chars().filter(|c| !c.is_control()).collect::<String>().trim().chars().take(MAX_TITLE_CHARS).collect();
    (!t.is_empty()).then_some(t)
}

/// One file of the extraction pass.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SubtitleOutput {
    pub track: u32,
    pub format: SubFormat,
    pub path: PathBuf,
}

/// Every file to extract for `tracks` (the formats each one offers), under `dir`.
pub fn plan_outputs(tracks: &[SubtitleTrack], dir: &Path) -> Vec<SubtitleOutput> {
    tracks
        .iter()
        .flat_map(|t| {
            [SubFormat::Ass, SubFormat::Vtt].into_iter().filter(|f| t.offers(*f)).map(|format| SubtitleOutput {
                track: t.index,
                format,
                path: dir.join(format!("{}.{}", t.index, format.ext())),
            })
        })
        .collect()
}

/// FFmpeg arguments (without the program) of the single pass that writes every output of `outputs`: one input (the
/// loopback fetcher, same protocol whitelist as the video), and per output its own `-map 0:s:<n>`, encoder and `-fs`
/// limit. No `-ss`: the files cover the whole source with its own timestamps (see the module docs).
pub fn extract_args(input_url: &str, outputs: &[SubtitleOutput]) -> Vec<OsString> {
    let mut a: Vec<OsString> = [
        "-hide_banner",
        "-nostdin",
        "-loglevel",
        "error",
        "-y",
        "-protocol_whitelist",
        PROTOCOL_WHITELIST,
        "-i",
        input_url,
    ]
    .into_iter()
    .map(OsString::from)
    .collect();
    for o in outputs {
        for x in ["-map".to_owned(), format!("0:s:{}", o.track), "-c:s".to_owned(), o.format.encoder().to_owned()] {
            a.push(x.into());
        }
        for x in ["-fs".to_owned(), MAX_FILE_BYTES.to_string(), "-f".to_owned(), o.format.encoder().to_owned()] {
            a.push(x.into());
        }
        a.push(o.path.clone().into_os_string());
    }
    a
}

/// What a finished pass gives for one file.
#[derive(Clone, Debug)]
pub enum Extracted {
    File(Bytes),
    /// Reached [`MAX_FILE_BYTES`] or did not fit in [`MAX_STORE_BYTES`] (`503 source_unavailable`).
    TooLarge,
}

/// Reads what a pass wrote for `outputs` into RAM, within the limits. A file that is missing (no event at all for that
/// track) is served empty: the track exists and has nothing to show.
pub fn collect_outputs(outputs: &[SubtitleOutput]) -> HashMap<(u32, SubFormat), Extracted> {
    let mut total = 0usize;
    let mut files = HashMap::new();
    for o in outputs {
        let len = std::fs::metadata(&o.path).map(|m| m.len()).unwrap_or(0);
        let got = if len >= MAX_FILE_BYTES || total + len as usize > MAX_STORE_BYTES {
            tracing::info!(track = o.track, "subtitle track not served: larger than the limit");
            Extracted::TooLarge
        } else {
            let body = std::fs::read(&o.path).unwrap_or_default();
            total += body.len();
            Extracted::File(Bytes::from(body))
        };
        files.insert((o.track, o.format), got);
    }
    files
}

#[derive(Debug)]
enum Probe {
    Idle,
    Running {
        owner: String,
        cancel: CancellationToken,
    },
    Done(Vec<SubtitleTrack>),
    /// `ffprobe` missing, failed or timed out: the session has no list (old PCs did not have one either).
    Failed,
}

#[derive(Debug)]
enum Extraction {
    Idle,
    Running { owner: String, cancel: CancellationToken },
    Ready(HashMap<(u32, SubFormat), Extracted>),
    Failed,
}

/// What a request for one file finds now.
#[derive(Debug)]
pub enum Lookup {
    /// The listing has not finished: wait (or `409 session_not_ready`).
    Listing,
    /// No such text track / format (or no listing at all): `404`.
    NotFound,
    /// The pass is running (or must be started, `start = true`): wait (or `503` + `Retry-After`).
    Extracting {
        start: bool,
    },
    File(Bytes),
    /// Too large, or the pass failed: `503 source_unavailable`.
    Unavailable,
}

/// The subtitles of one source: its track list and, once the phone asked for a file, the extracted files. Shared by a
/// session and the sessions that renew it (profile changes). Jobs are owned by the session that started them: when it
/// closes ([`SubtitleStore::abandon`]) they stop and the next session starts them again with its own input.
#[derive(Debug)]
pub struct SubtitleStore {
    state: Mutex<(Probe, Extraction)>,
    pub(crate) notify: Notify,
}

impl Default for SubtitleStore {
    fn default() -> Self {
        Self { state: Mutex::new((Probe::Idle, Extraction::Idle)), notify: Notify::new() }
    }
}

impl SubtitleStore {
    fn lock(&self) -> MutexGuard<'_, (Probe, Extraction)> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The listing, once `ffprobe` gave it (`None` before, or when it could not).
    pub fn tracks(&self) -> Option<Vec<SubtitleTrack>> {
        match &self.lock().0 {
            Probe::Done(t) => Some(t.clone()),
            _ => None,
        }
    }

    /// `subtitles_state` of the session status: `listing` | `unavailable` (no listing) | `idle` (listed, nothing
    /// extracted yet: the first file request starts it) | `extracting` | `ready` | `failed`.
    pub fn state_str(&self) -> &'static str {
        let st = self.lock();
        match (&st.0, &st.1) {
            (Probe::Idle | Probe::Running { .. }, _) => "listing",
            (Probe::Failed, _) => "unavailable",
            (Probe::Done(_), Extraction::Idle) => "idle",
            (Probe::Done(_), Extraction::Running { .. }) => "extracting",
            (Probe::Done(_), Extraction::Ready(_)) => "ready",
            (Probe::Done(_), Extraction::Failed) => "failed",
        }
    }

    /// Claims the listing for session `owner` when nobody ran it yet; the caller runs it and calls [`Self::finish_probe`].
    pub fn begin_probe(&self, owner: &str) -> Option<CancellationToken> {
        let mut st = self.lock();
        if !matches!(st.0, Probe::Idle) {
            return None;
        }
        let cancel = CancellationToken::new();
        st.0 = Probe::Running { owner: owner.to_owned(), cancel: cancel.clone() };
        Some(cancel)
    }

    pub fn finish_probe(&self, owner: &str, tracks: Option<Vec<SubtitleTrack>>) {
        {
            let mut st = self.lock();
            if !matches!(&st.0, Probe::Running { owner: o, .. } if o == owner) {
                return;
            }
            st.0 = match tracks {
                Some(t) => Probe::Done(t),
                None => Probe::Failed,
            };
        }
        self.notify.notify_waiters();
    }

    /// The text tracks to extract, claimed for session `owner`, when the pass has not run yet.
    pub fn begin_extraction(&self, owner: &str) -> Option<(Vec<SubtitleTrack>, CancellationToken)> {
        let mut st = self.lock();
        let (Probe::Done(tracks), Extraction::Idle) = (&st.0, &st.1) else { return None };
        let tracks = tracks.clone();
        let cancel = CancellationToken::new();
        st.1 = Extraction::Running { owner: owner.to_owned(), cancel: cancel.clone() };
        Some((tracks, cancel))
    }

    pub fn finish_extraction(&self, owner: &str, files: Option<HashMap<(u32, SubFormat), Extracted>>) {
        {
            let mut st = self.lock();
            if !matches!(&st.1, Extraction::Running { owner: o, .. } if o == owner) {
                return;
            }
            st.1 = match files {
                Some(f) => Extraction::Ready(f),
                None => Extraction::Failed,
            };
        }
        self.notify.notify_waiters();
    }

    /// Session `owner` closed: the jobs it runs stop (its loopback input is gone) and go back to idle, so the session
    /// that renews it starts them again with its own input.
    pub fn abandon(&self, owner: &str) {
        {
            let mut st = self.lock();
            if let Probe::Running { owner: o, cancel } = &st.0
                && o == owner
            {
                cancel.cancel();
                st.0 = Probe::Idle;
            }
            if let Extraction::Running { owner: o, cancel } = &st.1
                && o == owner
            {
                cancel.cancel();
                st.1 = Extraction::Idle;
            }
        }
        self.notify.notify_waiters();
    }

    /// File `n` in `format`, as far as it is known now.
    pub fn lookup(&self, n: u32, format: SubFormat) -> Lookup {
        let st = self.lock();
        let tracks = match &st.0 {
            Probe::Idle | Probe::Running { .. } => return Lookup::Listing,
            Probe::Failed => return Lookup::NotFound,
            Probe::Done(t) => t,
        };
        if !tracks.iter().any(|t| t.index == n && t.text && t.offers(format)) {
            return Lookup::NotFound;
        }
        match &st.1 {
            Extraction::Idle => Lookup::Extracting { start: true },
            Extraction::Running { .. } => Lookup::Extracting { start: false },
            Extraction::Failed => Lookup::Unavailable,
            Extraction::Ready(files) => match files.get(&(n, format)) {
                Some(Extracted::File(b)) => Lookup::File(b.clone()),
                Some(Extracted::TooLarge) | None => Lookup::Unavailable,
            },
        }
    }
}

impl Drop for SubtitleStore {
    fn drop(&mut self) {
        let st = self.state.get_mut().unwrap_or_else(PoisonError::into_inner);
        if let Probe::Running { cancel, .. } = &st.0 {
            cancel.cancel();
        }
        if let Extraction::Running { cancel, .. } = &st.1 {
            cancel.cancel();
        }
    }
}

/// Removes the scratch folder of a pass when the pass ends, however it ends (done, failed, cancelled).
pub struct ScratchDir(pub PathBuf);

impl Drop for ScratchDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PROBE: &str = r#"{"programs":[],"streams":[
        {"codec_name":"ass","disposition":{"default":1,"forced":0},"tags":{"language":"eng","title":"English"}},
        {"codec_name":"ass","disposition":{"default":0,"forced":0},"tags":{"language":"spa","title":"Spanish(Latin America)"}},
        {"codec_name":"subrip","disposition":{"default":0,"forced":1},"tags":{"language":"und","title":"  Signs\u0007 "}},
        {"codec_name":"hdmv_pgs_subtitle","tags":{"language":"POR"}},
        {"codec_name":"webvtt"}
    ]}"#;

    #[test]
    fn the_listing_keeps_order_language_title_and_flags_and_marks_image_tracks() {
        let t = parse_ffprobe(PROBE).unwrap();
        assert_eq!(t.len(), 5);
        assert_eq!(
            t[0],
            SubtitleTrack {
                index: 0,
                language: Some("eng".into()),
                title: Some("English".into()),
                codec: "ass".into(),
                default: true,
                forced: false,
                text: true,
                formats: vec!["ass", "vtt"],
            }
        );
        assert_eq!((t[1].index, t[1].title.as_deref()), (1, Some("Spanish(Latin America)")));
        // `und` is no language; control characters and blanks are dropped from titles
        assert_eq!((t[2].language.as_deref(), t[2].title.as_deref(), t[2].forced), (None, Some("Signs"), true));
        assert_eq!(t[2].formats, vec!["vtt"], "SRT has no styles to keep: WebVTT only");
        assert_eq!((t[3].text, t[3].formats.len(), t[3].language.as_deref()), (false, 0, Some("por")));
        assert_eq!((t[4].language.as_deref(), t[4].title.as_deref(), t[4].formats.clone()), (None, None, vec!["vtt"]));
        assert_eq!(parse_ffprobe(r#"{"streams":[]}"#), Some(vec![]));
        assert_eq!(parse_ffprobe("not json"), None);
        let json = serde_json::to_value(&t[3]).unwrap();
        assert!(json.get("title").is_none(), "empty fields do not travel");
    }

    #[test]
    fn long_or_odd_metadata_is_capped() {
        let long = "x".repeat(500);
        let j =
            format!(r#"{{"streams":[{{"codec_name":"ass","tags":{{"language":"es; rm -rf","title":"{long}"}}}}]}}"#);
        let t = parse_ffprobe(&j).unwrap();
        assert_eq!(t[0].language, None);
        assert_eq!(t[0].title.as_ref().unwrap().chars().count(), MAX_TITLE_CHARS);
        let many = format!(r#"{{"streams":[{}]}}"#, vec![r#"{"codec_name":"subrip"}"#; 100].join(","));
        let t = parse_ffprobe(&many).unwrap();
        assert_eq!(t.len(), MAX_LISTED);
        assert_eq!(t.iter().filter(|x| !x.formats.is_empty()).count(), MAX_EXTRACTED);
    }

    #[test]
    fn file_names_and_the_single_pass_arguments() {
        assert_eq!(parse_file_name("0.ass"), Some((0, SubFormat::Ass)));
        assert_eq!(parse_file_name("12.vtt"), Some((12, SubFormat::Vtt)));
        for bad in ["a.vtt", "1.srt", ".vtt", "1", "1.vtt.x", "-1.vtt", "1234.vtt", "01x.ass"] {
            assert_eq!(parse_file_name(bad), None, "{bad}");
        }
        let tracks = parse_ffprobe(PROBE).unwrap();
        let outs = plan_outputs(&tracks, Path::new("/w"));
        let names: Vec<String> =
            outs.iter().map(|o| o.path.file_name().unwrap().to_string_lossy().into_owned()).collect();
        assert_eq!(names, ["0.ass", "0.vtt", "1.ass", "1.vtt", "2.vtt", "4.vtt"]);
        let a: Vec<String> =
            extract_args("http://127.0.0.1:9/u/t", &outs).iter().map(|x| x.to_string_lossy().into_owned()).collect();
        assert_eq!(
            &a[..9],
            [
                "-hide_banner",
                "-nostdin",
                "-loglevel",
                "error",
                "-y",
                "-protocol_whitelist",
                "http,tcp,crypto",
                "-i",
                "http://127.0.0.1:9/u/t"
            ]
        );
        assert!(!a.contains(&"-ss".to_owned()), "the files keep the source's own timeline");
        assert_eq!(
            &a[9..18],
            ["-map", "0:s:0", "-c:s", "ass", "-fs", &MAX_FILE_BYTES.to_string(), "-f", "ass", "/w/0.ass"]
        );
        assert_eq!(a.iter().filter(|x| *x == "-map").count(), 6);
        assert!(a.ends_with(&[
            "-map".into(),
            "0:s:4".into(),
            "-c:s".into(),
            "webvtt".into(),
            "-fs".into(),
            MAX_FILE_BYTES.to_string(),
            "-f".into(),
            "webvtt".into(),
            "/w/4.vtt".into()
        ]));
    }

    #[test]
    fn the_store_lists_extracts_once_and_restarts_what_a_closed_session_left() {
        let s = SubtitleStore::default();
        assert!(matches!(s.lookup(0, SubFormat::Vtt), Lookup::Listing));
        assert_eq!(s.state_str(), "listing");
        let probe = s.begin_probe("a").unwrap();
        assert!(s.begin_probe("b").is_none(), "one listing per source");
        // the session that ran it closed: the renewing one lists again
        s.abandon("a");
        assert!(probe.is_cancelled());
        assert!(s.begin_probe("b").is_some());
        s.finish_probe("a", Some(vec![])); // stale owner: ignored
        assert_eq!(s.state_str(), "listing");
        s.finish_probe("b", parse_ffprobe(PROBE));
        assert_eq!(s.state_str(), "idle");
        assert!(matches!(s.lookup(3, SubFormat::Vtt), Lookup::NotFound), "image track");
        assert!(matches!(s.lookup(2, SubFormat::Ass), Lookup::NotFound), "SRT has no .ass");
        assert!(matches!(s.lookup(9, SubFormat::Vtt), Lookup::NotFound));
        assert!(matches!(s.lookup(1, SubFormat::Ass), Lookup::Extracting { start: true }));
        let (tracks, cancel) = s.begin_extraction("b").unwrap();
        assert_eq!(tracks.len(), 5);
        assert!(s.begin_extraction("c").is_none(), "one pass per source");
        assert!(matches!(s.lookup(1, SubFormat::Ass), Lookup::Extracting { start: false }));
        assert_eq!(s.state_str(), "extracting");
        s.abandon("c"); // not the owner: nothing stops
        assert!(!cancel.is_cancelled());
        let mut files = HashMap::new();
        files.insert((1, SubFormat::Ass), Extracted::File(Bytes::from_static(b"[Script Info]")));
        files.insert((0, SubFormat::Vtt), Extracted::TooLarge);
        s.finish_extraction("b", Some(files));
        assert_eq!(s.state_str(), "ready");
        assert!(matches!(s.lookup(1, SubFormat::Ass), Lookup::File(b) if &b[..] == b"[Script Info]"));
        assert!(matches!(s.lookup(0, SubFormat::Vtt), Lookup::Unavailable));
        // a ready store survives its owner closing (a profile change renews the session)
        s.abandon("b");
        assert!(matches!(s.lookup(1, SubFormat::Ass), Lookup::File(_)));
    }

    #[test]
    fn a_failed_listing_or_pass_is_final() {
        let s = SubtitleStore::default();
        s.begin_probe("a").unwrap();
        s.finish_probe("a", None);
        assert_eq!(s.state_str(), "unavailable");
        assert!(s.tracks().is_none());
        assert!(matches!(s.lookup(0, SubFormat::Vtt), Lookup::NotFound));
        let s = SubtitleStore::default();
        s.begin_probe("a").unwrap();
        s.finish_probe("a", parse_ffprobe(PROBE));
        s.begin_extraction("a").unwrap();
        s.finish_extraction("a", None);
        assert_eq!(s.state_str(), "failed");
        assert!(matches!(s.lookup(0, SubFormat::Vtt), Lookup::Unavailable));
    }

    #[test]
    fn outputs_over_the_limit_are_not_served_and_missing_ones_are_empty() {
        let dir = tempfile::tempdir().unwrap();
        let outs = vec![
            SubtitleOutput { track: 0, format: SubFormat::Vtt, path: dir.path().join("0.vtt") },
            SubtitleOutput { track: 1, format: SubFormat::Vtt, path: dir.path().join("1.vtt") },
            SubtitleOutput { track: 2, format: SubFormat::Vtt, path: dir.path().join("2.vtt") },
        ];
        std::fs::write(&outs[0].path, "WEBVTT\n").unwrap();
        std::fs::write(&outs[1].path, vec![b'x'; MAX_FILE_BYTES as usize]).unwrap();
        let got = collect_outputs(&outs);
        assert!(matches!(&got[&(0, SubFormat::Vtt)], Extracted::File(b) if &b[..] == b"WEBVTT\n"));
        assert!(matches!(&got[&(1, SubFormat::Vtt)], Extracted::TooLarge));
        assert!(matches!(&got[&(2, SubFormat::Vtt)], Extracted::File(b) if b.is_empty()));
        let scratch = dir.path().join("subs-x");
        std::fs::create_dir_all(&scratch).unwrap();
        drop(ScratchDir(scratch.clone()));
        assert!(!scratch.exists());
    }
}
