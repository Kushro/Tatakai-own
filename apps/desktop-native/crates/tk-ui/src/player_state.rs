//! Player flow state (wave 1 E, docs/redesign/08): Resolving / Buffering / Playing / Error / Manual, source
//! failover over the ranked `SourceVm` list and the `failed[]` record. Pure and unit-tested like `model`;
//! no Dioxus, no backend, no clock (callers pass `now`). Reserved here so E never edits `lib.rs`.
//!
//! The screen drives it like this: after every transition it asks [`FlowState::to_resolve`]; `Some(i)`
//! means "resolve `sources[i]` now" (`Backend::playback_source`), tagged with [`FlowState::attempt`] so a
//! late answer for an older attempt is ignored. Nothing else in here starts network work.

use crate::backend::{SourceKindVm, SourceVm};
use crate::model::{ErrorKind, SourceModeVm, UiError};

/// Legacy path (`Backend::sources` not wired): at most this many `PlaybackVm::streams` are tried.
pub const MAX_LEGACY_STREAMS: usize = 3;
/// Next-episode card countdown.
pub const NEXT_COUNTDOWN_SECS: u64 = 10;
/// Short confirmations over the video ("Off", "1.5×") disappear after this.
pub const FLASH_MS: u64 = 2600;
/// Speed menu (prototype `player.js`).
pub const SPEEDS: [f64; 4] = [1.0, 1.25, 1.5, 2.0];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Phase {
    /// Asking the extension for a playable URL of `sources[current]`.
    #[default]
    Resolving,
    /// The stream is loaded into mpv and has not produced a frame yet (or stalls while playing).
    Buffering,
    Playing,
    /// Nothing left to try (`error` says why, `failed` lists every attempt).
    Error,
    /// Manual mode without a pick: waiting for the user, nothing is resolved.
    Manual,
}

/// One entry of `failed[]`.
#[derive(Debug, Clone, PartialEq)]
pub struct FailedSource {
    pub id: String,
    /// `Ext · Server`.
    pub name: String,
    pub kind: ErrorKind,
    pub detail: String,
}

/// `Ext · Server` / `Ext · Group`, as in the picker rows.
pub fn source_name(s: &SourceVm) -> String {
    format!("{} · {}", s.ext_name, s.label)
}

/// What Auto mode may try on its own: enabled extension and a web stream (torrent playback is M2).
pub fn playable(s: &SourceVm) -> bool {
    s.ext_enabled && s.kind == SourceKindVm::Web
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct FlowState {
    pub phase: Phase,
    pub mode: SourceModeVm,
    /// Ranked best first (`Backend::sources`); `picked` marks the user's choice.
    pub sources: Vec<SourceVm>,
    /// Index into `sources` of the source being resolved / played.
    pub current: Option<usize>,
    pub failed: Vec<FailedSource>,
    pub error: Option<UiError>,
    /// Bumped every time a source starts resolving.
    pub attempt: u64,
    /// Built from `Backend::playback` streams: no reliability reports, no picker.
    pub legacy: bool,
    started: bool,
}

impl FlowState {
    pub fn new(sources: Vec<SourceVm>, mode: SourceModeVm) -> Self {
        let mut s = Self { sources, mode, ..Self::default() };
        s.begin();
        s
    }

    /// Sources synthesized from the old `PlaybackVm::streams` (always Auto, plain failover).
    pub fn legacy(sources: Vec<SourceVm>) -> Self {
        let mut s = Self { sources, mode: SourceModeVm::Auto, legacy: true, ..Self::default() };
        s.begin();
        s
    }

    fn begin(&mut self) {
        self.failed.clear();
        self.error = None;
        self.current = None;
        if self.sources.is_empty() {
            self.stop(UiError::new(ErrorKind::NoSource, ""));
            return;
        }
        if let Some(i) = self.sources.iter().position(|s| s.picked) {
            self.enter(i);
            return;
        }
        if self.mode == SourceModeVm::Manual && !self.legacy {
            self.phase = Phase::Manual;
            return;
        }
        match self.next_playable() {
            Some(i) => self.enter(i),
            None => {
                let e = self.no_playable_error();
                self.stop(e);
            }
        }
    }

    fn no_playable_error(&self) -> UiError {
        if self.sources.iter().any(|s| s.ext_enabled && s.kind == SourceKindVm::Torrent) {
            UiError::new(ErrorKind::TorrentM2, "")
        } else {
            UiError::new(ErrorKind::NoSource, "")
        }
    }

    fn enter(&mut self, i: usize) {
        self.current = Some(i);
        self.phase = Phase::Resolving;
        self.attempt += 1;
        self.started = false;
        self.error = None;
    }

    fn stop(&mut self, error: UiError) {
        self.phase = Phase::Error;
        self.error = Some(error);
    }

    /// First playable source in ranking order that has not failed yet.
    fn next_playable(&self) -> Option<usize> {
        (0..self.sources.len()).find(|&i| {
            let s = &self.sources[i];
            playable(s) && !self.failed.iter().any(|f| f.id == s.id)
        })
    }

    /// `Some(i)` while `sources[i]` must be resolved; `None` in every other phase.
    pub fn to_resolve(&self) -> Option<usize> {
        if self.phase == Phase::Resolving { self.current } else { None }
    }

    pub fn current_source(&self) -> Option<&SourceVm> {
        self.current.and_then(|i| self.sources.get(i))
    }

    /// `(n, total)` for "Source n of total".
    pub fn position(&self) -> Option<(usize, usize)> {
        self.current.map(|i| (i + 1, self.sources.len()))
    }

    /// The user picked `id` (picker, quality menu, "Use the best web source"). Marks it as the pick and
    /// starts resolving it, whatever the mode. Unknown ids change nothing.
    pub fn pick(&mut self, id: &str) -> bool {
        let Some(i) = self.sources.iter().position(|s| s.id == id) else {
            return false;
        };
        for s in &mut self.sources {
            s.picked = s.id == id;
        }
        self.failed.retain(|f| f.id != id);
        self.enter(i);
        true
    }

    /// The stream of `attempt` was handed to mpv.
    pub fn resolved(&mut self, attempt: u64) {
        if attempt == self.attempt && self.phase == Phase::Resolving {
            self.phase = Phase::Buffering;
        }
    }

    /// mpv shows frames. Returns the source id to report as working, once per attempt (never in legacy).
    pub fn started(&mut self) -> Option<String> {
        if !matches!(self.phase, Phase::Buffering | Phase::Playing) {
            return None;
        }
        self.phase = Phase::Playing;
        if self.started {
            return None;
        }
        self.started = true;
        if self.legacy { None } else { self.current_source().map(|s| s.id.clone()) }
    }

    /// Cache starvation while playing (`paused-for-cache`).
    pub fn stalled(&mut self, stalled: bool) {
        match (stalled, self.phase) {
            (true, Phase::Playing) => self.phase = Phase::Buffering,
            (false, Phase::Buffering) if self.started => self.phase = Phase::Playing,
            _ => {}
        }
    }

    /// The source of `attempt` failed (resolve, load or mpv error). Records it in `failed`; Auto (and
    /// legacy) moves on to the next playable source, Manual stops so the user picks again. Returns the
    /// failed source id to report (`None` for stale attempts and legacy).
    pub fn fail(&mut self, attempt: u64, error: UiError) -> Option<String> {
        if attempt != self.attempt || !matches!(self.phase, Phase::Resolving | Phase::Buffering | Phase::Playing) {
            return None;
        }
        let s = self.current_source()?.clone();
        self.failed.push(FailedSource { id: s.id.clone(), name: source_name(&s), kind: error.kind, detail: error.detail.clone() });
        let report = (!self.legacy).then(|| s.id.clone());
        if (self.mode == SourceModeVm::Auto || self.legacy)
            && let Some(n) = self.next_playable()
        {
            self.enter(n);
            return report;
        }
        self.stop(error);
        report
    }

    /// "Retry" on the final error: forget the failures and start over (the pick still wins).
    pub fn retry(&mut self) {
        self.begin();
    }

    /// Best web source to offer when a torrent fails with `TorrentM2` (not failed yet if possible).
    pub fn best_web(&self) -> Option<&SourceVm> {
        self.next_playable().map(|i| &self.sources[i]).or_else(|| self.sources.iter().find(|s| playable(s)))
    }

    /// Distinct qualities of the playable sources, highest first (quality menu).
    pub fn qualities(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for s in self.sources.iter().filter(|s| playable(s) && !s.quality.is_empty()) {
            if !out.contains(&s.quality) {
                out.push(s.quality.clone());
            }
        }
        out.sort_by_key(|q| std::cmp::Reverse(quality_rank(q)));
        out
    }

    /// Best-ranked playable source with that quality (not failed yet if possible).
    pub fn source_for_quality(&self, quality: &str) -> Option<&SourceVm> {
        let all: Vec<&SourceVm> = self.sources.iter().filter(|s| playable(s) && s.quality == quality).collect();
        all.iter().find(|s| !self.failed.iter().any(|f| f.id == s.id)).or(all.first()).copied()
    }
}

/// `"1080p"` -> 1080; unknown -> 0.
pub fn quality_rank(q: &str) -> u32 {
    q.trim_end_matches(['p', 'P']).parse().unwrap_or(0)
}

// ---------------------------------------------------------------------------------------------
// Intro / outro markers (ani.zip / AniSkip arrive in M1; the screen keeps the hook)
// ---------------------------------------------------------------------------------------------

/// A marked range in seconds.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Span {
    pub start: f64,
    pub end: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Markers {
    pub intro: Option<Span>,
    pub outro: Option<Span>,
}

impl Markers {
    /// Where "Skip intro" jumps to, while `pos` is inside the intro.
    pub fn skip_target(&self, pos: f64) -> Option<f64> {
        self.intro.filter(|s| pos >= s.start && pos < s.end).map(|s| s.end)
    }

    /// `(left %, width %)` of every marker on a seek bar of `duration` seconds.
    pub fn bar_spans(&self, duration: f64) -> Vec<(f64, f64)> {
        if !(duration.is_finite() && duration > 0.0) {
            return Vec::new();
        }
        [self.intro, self.outro]
            .into_iter()
            .flatten()
            .filter(|s| s.end > s.start)
            .map(|s| {
                let a = (s.start / duration).clamp(0.0, 1.0);
                let b = (s.end / duration).clamp(0.0, 1.0);
                (a * 100.0, (b - a) * 100.0)
            })
            .collect()
    }
}

// ---------------------------------------------------------------------------------------------
// Small helpers for the controls
// ---------------------------------------------------------------------------------------------

/// Pointer `x` over a seek bar `width` px wide -> 0.0..=1.0 (seek hover preview).
pub fn hover_fraction(x: f64, width: f64) -> f64 {
    if width > 0.0 && x.is_finite() { (x / width).clamp(0.0, 1.0) } else { 0.0 }
}

/// The controls (and the status they carry: "Looking for a source", the buffering spinner) stay up regardless of
/// pointer activity until the first frame: resolving, waiting for a manual pick, or loading before any picture.
pub fn keeps_controls_up(phase: Phase, video_ready: bool) -> bool {
    match phase {
        Phase::Resolving | Phase::Manual => true,
        Phase::Buffering => !video_ready,
        Phase::Playing | Phase::Error => false,
    }
}

/// Seconds left on the next-episode card that appeared at `since_ms`.
pub fn countdown_left(since_ms: u64, now_ms: u64) -> u64 {
    NEXT_COUNTDOWN_SECS.saturating_sub(now_ms.saturating_sub(since_ms) / 1000)
}

/// `1.25` -> `1.25×`, `2.0` -> `2×`.
pub fn format_speed(speed: f64) -> String {
    let s = format!("{speed:.2}");
    format!("{}×", s.trim_end_matches('0').trim_end_matches('.'))
}

/// Embedded enhanced with uosc (docs/redesign/09 section 4.4): the web `KeyboardEvent.key` -> mpv key name for
/// `Player::forward_key` (`SPACE`, `LEFT`, `Ctrl+s`, `Shift+RIGHT`, `F5`). Modifier-only and unknown keys give
/// `None`. Shift is implied by printable characters (`S`, `?`) and only prefixed on named keys, like mpv does.
pub fn mpv_key_name(key: &str, ctrl: bool, alt: bool, shift: bool, meta: bool) -> Option<String> {
    let named = match key {
        " " => Some("SPACE"),
        "ArrowLeft" => Some("LEFT"),
        "ArrowRight" => Some("RIGHT"),
        "ArrowUp" => Some("UP"),
        "ArrowDown" => Some("DOWN"),
        "Enter" => Some("ENTER"),
        "Escape" => Some("ESC"),
        "Backspace" => Some("BS"),
        "Tab" => Some("TAB"),
        "Delete" => Some("DEL"),
        "Insert" => Some("INS"),
        "Home" => Some("HOME"),
        "End" => Some("END"),
        "PageUp" => Some("PGUP"),
        "PageDown" => Some("PGDWN"),
        "#" => Some("SHARP"),
        _ => None,
    };
    let (base, printable) = match named {
        Some(n) => (n.to_owned(), key == "#"),
        None if key.chars().count() == 1 => (key.to_owned(), true),
        None if key.len() <= 3 && key.starts_with('F') && key[1..].parse::<u8>().is_ok_and(|n| (1..=24).contains(&n)) => {
            (key.to_owned(), false)
        }
        None => return None,
    };
    let mut out = String::new();
    if ctrl {
        out.push_str("Ctrl+");
    }
    if alt {
        out.push_str("Alt+");
    }
    if meta {
        out.push_str("Meta+");
    }
    if shift && !printable {
        out.push_str("Shift+");
    }
    out.push_str(&base);
    Some(out)
}

/// Mouse button index (`MouseEvent.button`: 0 primary, 1 middle, 2 secondary) -> mpv key name.
pub fn mpv_mouse_button(button: u8) -> Option<&'static str> {
    match button {
        0 => Some("MBTN_LEFT"),
        1 => Some("MBTN_MID"),
        2 => Some("MBTN_RIGHT"),
        3 => Some("MBTN_BACK"),
        4 => Some("MBTN_FORWARD"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn src(ext: &str, label: &str, kind: SourceKindVm, quality: &str) -> SourceVm {
        SourceVm {
            id: format!("{ext}|{}|{label}", kind.id()),
            ext_id: ext.to_owned(),
            ext_name: ext.to_owned(),
            kind,
            label: label.to_owned(),
            quality: quality.to_owned(),
            container: None,
            subs: vec!["es".into()],
            dubs: vec!["ja".into()],
            reliability: 0.8,
            note: None,
            latency_ms: Some(200),
            size_bytes: None,
            seeders: None,
            leechers: None,
            info_hash: None,
            ext_enabled: true,
            score: 0.0,
            recommended: false,
            picked: false,
            tags: Default::default(),
        }
    }

    /// Ranked: web 1080p, torrent, web 720p, disabled extension, web 480p.
    fn list() -> Vec<SourceVm> {
        let mut v = vec![
            src("anipro", "Alpha", SourceKindVm::Web, "1080p"),
            src("nyaa", "SubsPlease", SourceKindVm::Torrent, "1080p"),
            src("anipro", "Beta", SourceKindVm::Web, "720p"),
            src("off", "Gamma", SourceKindVm::Web, "1080p"),
            src("toko", "Ok.ru", SourceKindVm::Web, "480p"),
        ];
        v[0].recommended = true;
        v[3].ext_enabled = false;
        v
    }

    fn err() -> UiError {
        UiError::new(ErrorKind::Extension, "HTTP 403")
    }

    #[test]
    fn manual_mode_does_not_resolve_any_stream_until_a_source_is_picked() {
        let mut f = FlowState::new(list(), SourceModeVm::Manual);
        assert_eq!(f.phase, Phase::Manual);
        assert_eq!(f.to_resolve(), None);
        assert_eq!(f.current, None);
        assert_eq!(f.attempt, 0);
        // Player events before a pick change nothing and report nothing.
        f.resolved(0);
        assert_eq!(f.started(), None);
        f.stalled(false);
        assert_eq!(f.fail(0, err()), None);
        assert_eq!(f.phase, Phase::Manual);
        assert_eq!(f.to_resolve(), None);
        // Picking starts exactly one resolution.
        assert!(f.pick("anipro|web|Beta"));
        assert_eq!(f.to_resolve(), Some(2));
        assert_eq!(f.attempt, 1);
        assert!(f.sources[2].picked);
        assert!(!f.pick("nope|web|x"));
        assert_eq!(f.attempt, 1);
    }

    #[test]
    fn manual_mode_with_a_saved_pick_uses_it_directly() {
        let mut v = list();
        v[4].picked = true;
        let f = FlowState::new(v, SourceModeVm::Manual);
        assert_eq!(f.to_resolve(), Some(4));
    }

    #[test]
    fn auto_mode_prefers_the_pick_then_the_first_playable() {
        let f = FlowState::new(list(), SourceModeVm::Auto);
        assert_eq!(f.to_resolve(), Some(0));
        let mut v = list();
        v[2].picked = true;
        assert_eq!(FlowState::new(v, SourceModeVm::Auto).to_resolve(), Some(2));
        // A disabled extension or a torrent at the top is skipped: [Gamma(off), SubsPlease, Beta, Ok.ru].
        let mut v = list();
        v.remove(0);
        v.swap(0, 2);
        v.swap(1, 2);
        assert_eq!((v[0].ext_id.as_str(), v[1].kind), ("off", SourceKindVm::Torrent));
        assert_eq!(FlowState::new(v, SourceModeVm::Auto).to_resolve(), Some(2));
    }

    #[test]
    fn failover_follows_the_list_and_lists_every_failure() {
        let mut f = FlowState::new(list(), SourceModeVm::Auto);
        let a = f.attempt;
        assert_eq!(f.fail(a, err()).as_deref(), Some("anipro|web|Alpha"));
        // Skips the torrent (M2) and the disabled extension.
        assert_eq!(f.to_resolve(), Some(2));
        let a = f.attempt;
        f.resolved(a);
        assert_eq!(f.phase, Phase::Buffering);
        assert_eq!(f.fail(a, UiError::new(ErrorKind::Other, "mpv: 404")).as_deref(), Some("anipro|web|Beta"));
        assert_eq!(f.to_resolve(), Some(4));
        let a = f.attempt;
        assert_eq!(f.fail(a, UiError::new(ErrorKind::Offline, "")).as_deref(), Some("toko|web|Ok.ru"));
        assert_eq!(f.phase, Phase::Error);
        assert_eq!(f.to_resolve(), None);
        let names: Vec<_> = f.failed.iter().map(|x| x.name.as_str()).collect();
        assert_eq!(names, ["anipro · Alpha", "anipro · Beta", "toko · Ok.ru"]);
        assert_eq!(f.failed[0].detail, "HTTP 403");
        assert_eq!(f.error.as_ref().map(|e| e.kind), Some(ErrorKind::Offline));
        // Retry starts over from the top.
        f.retry();
        assert!(f.failed.is_empty());
        assert_eq!(f.to_resolve(), Some(0));
    }

    #[test]
    fn stale_attempts_are_ignored() {
        let mut f = FlowState::new(list(), SourceModeVm::Auto);
        let old = f.attempt;
        f.pick("anipro|web|Beta");
        assert_eq!(f.fail(old, err()), None);
        f.resolved(old);
        assert_eq!(f.phase, Phase::Resolving);
        assert!(f.failed.is_empty());
    }

    #[test]
    fn manual_mode_stops_on_failure_and_offers_the_best_web_source() {
        let mut f = FlowState::new(list(), SourceModeVm::Manual);
        f.pick("nyaa|torrent|SubsPlease");
        let a = f.attempt;
        assert!(f.fail(a, UiError::new(ErrorKind::TorrentM2, "")).is_some());
        assert_eq!(f.phase, Phase::Error);
        assert_eq!(f.error.as_ref().map(|e| e.kind), Some(ErrorKind::TorrentM2));
        assert_eq!(f.best_web().map(|s| s.label.as_str()), Some("Alpha"));
    }

    #[test]
    fn started_reports_once_and_stalls_toggle_buffering() {
        let mut f = FlowState::new(list(), SourceModeVm::Auto);
        assert_eq!(f.started(), None, "not loaded yet");
        let a = f.attempt;
        f.resolved(a);
        assert_eq!(f.started().as_deref(), Some("anipro|web|Alpha"));
        assert_eq!(f.phase, Phase::Playing);
        f.stalled(true);
        assert_eq!(f.phase, Phase::Buffering);
        assert_eq!(f.started(), None);
        f.stalled(false);
        assert_eq!(f.phase, Phase::Playing);
    }

    #[test]
    fn empty_or_unplayable_lists_end_in_an_error() {
        let f = FlowState::new(Vec::new(), SourceModeVm::Auto);
        assert_eq!((f.phase, f.error.map(|e| e.kind)), (Phase::Error, Some(ErrorKind::NoSource)));
        let f = FlowState::new(vec![src("nyaa", "Erai", SourceKindVm::Torrent, "1080p")], SourceModeVm::Auto);
        assert_eq!(f.error.map(|e| e.kind), Some(ErrorKind::TorrentM2));
        let mut off = src("x", "y", SourceKindVm::Web, "");
        off.ext_enabled = false;
        let f = FlowState::new(vec![off], SourceModeVm::Auto);
        assert_eq!(f.error.map(|e| e.kind), Some(ErrorKind::NoSource));
    }

    #[test]
    fn legacy_flow_never_reports_and_never_waits() {
        let v = vec![src("p", "1", SourceKindVm::Web, ""), src("p", "2", SourceKindVm::Web, "")];
        let mut f = FlowState::legacy(v);
        assert_eq!(f.to_resolve(), Some(0));
        let a = f.attempt;
        assert_eq!(f.fail(a, err()), None);
        assert_eq!(f.to_resolve(), Some(1));
        let a = f.attempt;
        f.resolved(a);
        assert_eq!(f.started(), None);
        assert_eq!(f.phase, Phase::Playing);
    }

    #[test]
    fn quality_menu_lists_playable_qualities_highest_first() {
        let f = FlowState::new(list(), SourceModeVm::Auto);
        assert_eq!(f.qualities(), ["1080p", "720p", "480p"]);
        assert_eq!(f.source_for_quality("720p").map(|s| s.label.as_str()), Some("Beta"));
        assert!(f.source_for_quality("2160p").is_none());
        assert_eq!(quality_rank("1080p"), 1080);
        assert_eq!(quality_rank(""), 0);
    }

    #[test]
    fn markers_skip_and_bar_spans() {
        let m = Markers { intro: Some(Span { start: 85.0, end: 175.0 }), outro: Some(Span { start: 1300.0, end: 1400.0 }) };
        assert_eq!(m.skip_target(90.0), Some(175.0));
        assert_eq!(m.skip_target(175.0), None);
        assert_eq!(m.skip_target(10.0), None);
        let spans = m.bar_spans(1400.0);
        assert_eq!(spans.len(), 2);
        assert!((spans[0].0 - 85.0 / 14.0).abs() < 1e-9);
        assert!(Markers::default().bar_spans(1400.0).is_empty());
        assert!(m.bar_spans(0.0).is_empty());
    }

    #[test]
    fn helpers() {
        assert_eq!(hover_fraction(50.0, 200.0), 0.25);
        assert_eq!(hover_fraction(-5.0, 200.0), 0.0);
        assert_eq!(hover_fraction(500.0, 200.0), 1.0);
        assert_eq!(hover_fraction(5.0, 0.0), 0.0);
        assert!(keeps_controls_up(Phase::Resolving, false));
        assert!(keeps_controls_up(Phase::Manual, false));
        assert!(keeps_controls_up(Phase::Buffering, false));
        assert!(!keeps_controls_up(Phase::Buffering, true), "a stall mid-episode does not pin the controls");
        assert!(!keeps_controls_up(Phase::Playing, true));
        assert!(!keeps_controls_up(Phase::Error, false));
        assert_eq!(countdown_left(1000, 1000), 10);
        assert_eq!(countdown_left(1000, 4500), 7);
        assert_eq!(countdown_left(1000, 60_000), 0);
        assert_eq!(format_speed(1.0), "1×");
        assert_eq!(format_speed(1.25), "1.25×");
        assert_eq!(format_speed(1.5), "1.5×");
        assert_eq!(format_speed(2.0), "2×");
    }

    #[test]
    fn web_keys_map_to_mpv_names() {
        assert_eq!(mpv_key_name(" ", false, false, false, false).as_deref(), Some("SPACE"));
        assert_eq!(mpv_key_name("ArrowLeft", false, false, true, false).as_deref(), Some("Shift+LEFT"));
        assert_eq!(mpv_key_name("S", false, false, true, false).as_deref(), Some("S"));
        assert_eq!(mpv_key_name("s", true, false, false, false).as_deref(), Some("Ctrl+s"));
        assert_eq!(mpv_key_name("#", false, false, true, false).as_deref(), Some("SHARP"));
        assert_eq!(mpv_key_name("F5", false, false, false, false).as_deref(), Some("F5"));
        assert_eq!(mpv_key_name("Shift", false, false, true, false), None);
        assert_eq!(mpv_key_name("Unidentified", false, false, false, false), None);
        assert_eq!(mpv_mouse_button(2), Some("MBTN_RIGHT"));
        assert_eq!(mpv_mouse_button(9), None);
    }
}
